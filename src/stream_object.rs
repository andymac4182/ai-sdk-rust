use std::fmt;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::rc::Rc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::VERSION;
use crate::generate_object::{
    GenerateObjectEndEvent, GenerateObjectOnFinish, GenerateObjectOnStart,
    GenerateObjectOnStepFinish, GenerateObjectOnStepStart, GenerateObjectOutputKind,
    GenerateObjectRepairText, GenerateObjectRepairTextOptions, GenerateObjectRequest,
    GenerateObjectResponse, GenerateObjectStartEvent, GenerateObjectStepEndEvent,
    GenerateObjectStepStartEvent, generate_object_call_id, generate_object_output_kind,
    generate_object_response_format, parse_generated_object,
};
use crate::headers::Headers;
use crate::json::{JsonSchema, JsonValue};
use crate::language_model::{
    FinishReason, LanguageModel, LanguageModelAbortController, LanguageModelAbortSignal,
    LanguageModelCallOptions, LanguageModelPrompt, LanguageModelRequest,
    LanguageModelResponseFormat, LanguageModelStreamPart, LanguageModelStreamResult,
    LanguageModelUsage,
};
use crate::prompt::{Prompt, prompt_has_url_files, standardize_prompt};
use crate::provider::{ApiCallError, InvalidPromptError, ProviderMetadata, ProviderOptions};
use crate::provider_utils::{
    FlexibleSchema, ParseJsonResult, ValidateTypesResult, safe_validate_types,
    with_user_agent_suffix,
};
use crate::retry::DEFAULT_MAX_RETRIES;
use crate::stream_text::StreamTextResponseMetadata;
use crate::telemetry::{TelemetryOptions, create_telemetry_dispatcher};
use crate::text_stream_response::{
    TextStreamResponse, TextStreamResponseInit, TextStreamResponseOptions,
    TextStreamResponseWriter, create_text_stream_response, pipe_text_stream_to_response,
};
use crate::util::{ParsePartialJsonState, is_deep_equal_data, parse_partial_json};
use crate::warning::Warning;

/// Response metadata returned by high-level object streaming.
pub type StreamObjectResponseMetadata = StreamTextResponseMetadata;

/// Stream event emitted by [`stream_object`].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ObjectStreamPart {
    /// Parsed partial object update.
    Object {
        /// Partial object parsed from the accumulated JSON text.
        object: JsonValue,
    },

    /// JSON text delta that contributed to the streamed object.
    TextDelta {
        /// Text delta emitted after a new partial object is available.
        text_delta: String,
    },

    /// Provider stream error.
    Error {
        /// Provider error represented as JSON.
        error: JsonValue,
    },

    /// Final metadata for the object stream.
    Finish(Box<ObjectStreamFinishPart>),
}

/// Event passed to a stream-object error callback.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamObjectOnErrorEvent {
    /// Provider error represented as JSON.
    pub error: JsonValue,
}

/// Future returned by a stream-object error callback.
pub type StreamObjectOnErrorFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked when a provider error stream part is received.
pub type StreamObjectOnErrorFunction<'a> =
    dyn Fn(StreamObjectOnErrorEvent) -> StreamObjectOnErrorFuture<'a> + 'a;

/// Upstream callback alias for stream-object `onError`.
pub type StreamObjectOnErrorCallback<'a> = StreamObjectOnErrorFunction<'a>;

/// Callback wrapper for upstream stream-object `onError`.
pub struct StreamObjectOnError<'a> {
    on_error: Rc<StreamObjectOnErrorFunction<'a>>,
}

impl<'a> StreamObjectOnError<'a> {
    /// Creates a stream-object error callback.
    pub fn new<F, Fut>(on_error: F) -> Self
    where
        F: Fn(StreamObjectOnErrorEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_error: Rc::new(move |event| Box::pin(on_error(event))),
        }
    }

    /// Runs the stream-object error callback.
    pub fn error(&self, event: StreamObjectOnErrorEvent) -> StreamObjectOnErrorFuture<'a> {
        match catch_unwind(AssertUnwindSafe(|| (self.on_error)(event))) {
            Ok(callback) => callback,
            Err(_) => Box::pin(async {}),
        }
    }
}

impl fmt::Debug for StreamObjectOnError<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamObjectOnError")
            .finish_non_exhaustive()
    }
}

/// Caller-controlled abort signal for Rust `stream_object` calls.
pub type StreamObjectAbortSignal = LanguageModelAbortSignal;

/// Controller used to trigger a [`StreamObjectAbortSignal`].
pub type StreamObjectAbortController = LanguageModelAbortController;

/// Final metadata emitted by an object stream.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectStreamFinishPart {
    /// Unified finish reason.
    pub finish_reason: FinishReason,

    /// Token usage reported by the provider.
    pub usage: LanguageModelUsage,

    /// Response metadata for the stream.
    pub response: StreamObjectResponseMetadata,

    /// Provider-specific metadata from the finish part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ObjectStreamFinishPart {
    /// Creates an object-stream finish part.
    pub fn new(
        finish_reason: FinishReason,
        usage: LanguageModelUsage,
        response: StreamObjectResponseMetadata,
        provider_metadata: Option<ProviderMetadata>,
    ) -> Self {
        Self {
            finish_reason,
            usage,
            response,
            provider_metadata,
        }
    }
}

/// Request options for a high-level object streaming call.
pub struct StreamObjectOptions<'a, M: LanguageModel + ?Sized> {
    /// Language model used for the streaming call.
    pub model: &'a M,

    /// Provider-level call options sent to the model.
    pub call_options: LanguageModelCallOptions,

    /// Optional schema used to guide and validate the generated object.
    pub schema: Option<FlexibleSchema>,

    /// Optional schema name sent in the JSON response format.
    pub schema_name: Option<String>,

    /// Optional schema description sent in the JSON response format.
    pub schema_description: Option<String>,

    /// Optional enum values for upstream enum output mode.
    pub enum_values: Option<Vec<String>>,

    /// Whether schema validation should use upstream array output mode.
    pub array_output: bool,

    /// Optional callback that can repair invalid final object text.
    pub repair_text: Option<GenerateObjectRepairText<'a>>,

    /// Optional callback invoked before stream-object model work begins.
    pub on_start: Option<GenerateObjectOnStart<'a>>,

    /// Optional callback invoked before the single streamed model step starts.
    pub on_step_start: Option<GenerateObjectOnStepStart<'a>>,

    /// Optional callback invoked after the streamed model step completes.
    pub on_step_finish: Option<GenerateObjectOnStepFinish<'a>>,

    /// Optional callback invoked after final object parsing and validation.
    pub on_finish: Option<GenerateObjectOnFinish<'a>>,

    /// Optional callback invoked for provider stream errors.
    pub on_error: Option<StreamObjectOnError<'a>>,

    /// Optional abort signal checked before and during streamed collection.
    pub abort_signal: Option<StreamObjectAbortSignal>,

    /// Optional telemetry dispatcher settings.
    pub telemetry: Option<TelemetryOptions>,

    /// Maximum number of retries for failed provider stream requests.
    pub max_retries: usize,
}

impl<'a, M: LanguageModel + ?Sized> StreamObjectOptions<'a, M> {
    /// Creates stream-object options for a model and standardized prompt.
    pub fn new(model: &'a M, prompt: LanguageModelPrompt) -> Self {
        Self::from_call_options(model, LanguageModelCallOptions::new(prompt))
    }

    /// Creates stream-object options from the high-level upstream prompt shape.
    pub fn from_prompt(model: &'a M, prompt: Prompt) -> Result<Self, InvalidPromptError> {
        let prompt = standardize_prompt(prompt)?.into_language_model_prompt();
        Ok(Self::new(model, prompt))
    }

    /// Creates stream-object options from already prepared provider call options.
    pub fn from_call_options(model: &'a M, mut call_options: LanguageModelCallOptions) -> Self {
        let abort_signal = call_options.abort_signal.clone();
        call_options.response_format =
            Some(crate::language_model::LanguageModelResponseFormat::json());
        Self {
            model,
            call_options,
            schema: None,
            schema_name: None,
            schema_description: None,
            enum_values: None,
            array_output: false,
            repair_text: None,
            on_start: None,
            on_step_start: None,
            on_step_finish: None,
            on_finish: None,
            on_error: None,
            abort_signal,
            telemetry: None,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }

    /// Sets the maximum number of output tokens.
    pub fn with_max_output_tokens(mut self, max_output_tokens: u64) -> Self {
        self.call_options.max_output_tokens = Some(max_output_tokens);
        self
    }

    /// Sets the sampling temperature.
    pub fn with_temperature(mut self, temperature: f64) -> Self {
        self.call_options.temperature = Some(temperature);
        self
    }

    /// Sets the deterministic sampling seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.call_options.seed = Some(seed);
        self
    }

    /// Adds an HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.call_options
            .headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.call_options.provider_options = Some(provider_options);
        self
    }

    /// Sets the schema used to validate the generated object and guide the provider.
    pub fn with_schema(mut self, schema: impl Into<FlexibleSchema>) -> Self {
        self.schema = Some(schema.into());
        self.enum_values = None;
        self.array_output = false;
        self
    }

    /// Uses upstream array output mode with a schema for each element.
    pub fn with_array_schema(mut self, schema: impl Into<FlexibleSchema>) -> Self {
        self.schema = Some(schema.into());
        self.enum_values = None;
        self.array_output = true;
        self
    }

    /// Uses upstream enum output mode.
    pub fn with_enum_values(
        mut self,
        enum_values: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.enum_values = Some(enum_values.into_iter().map(Into::into).collect());
        self.schema = None;
        self.array_output = false;
        self
    }

    /// Sets the schema name sent to providers that support named JSON schemas.
    pub fn with_schema_name(mut self, schema_name: impl Into<String>) -> Self {
        self.schema_name = Some(schema_name.into());
        self
    }

    /// Sets the schema description sent to providers that support schema descriptions.
    pub fn with_schema_description(mut self, schema_description: impl Into<String>) -> Self {
        self.schema_description = Some(schema_description.into());
        self
    }

    /// Sets a callback that can repair streamed text after parse or validation failure.
    pub fn with_repair_text<F, Fut>(mut self, repair_text: F) -> Self
    where
        F: Fn(GenerateObjectRepairTextOptions) -> Fut + 'a,
        Fut: Future<Output = Option<String>> + 'a,
    {
        self.repair_text = Some(GenerateObjectRepairText::new(repair_text));
        self
    }

    /// Sets the upstream experimental repair-text callback alias.
    pub fn with_experimental_repair_text<F, Fut>(self, repair_text: F) -> Self
    where
        F: Fn(GenerateObjectRepairTextOptions) -> Fut + 'a,
        Fut: Future<Output = Option<String>> + 'a,
    {
        self.with_repair_text(repair_text)
    }

    /// Sets a callback that is invoked when stream-object work starts before model streaming.
    pub fn with_on_start<F, Fut>(mut self, on_start: F) -> Self
    where
        F: Fn(GenerateObjectStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_start = Some(GenerateObjectOnStart::new(on_start));
        self
    }

    /// Upstream experimental alias for [`StreamObjectOptions::with_on_start`].
    pub fn with_experimental_on_start<F, Fut>(self, on_start: F) -> Self
    where
        F: Fn(GenerateObjectStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.with_on_start(on_start)
    }

    /// Sets a callback that is invoked before the single streamed model step starts.
    pub fn with_on_step_start<F, Fut>(mut self, on_step_start: F) -> Self
    where
        F: Fn(GenerateObjectStepStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_step_start = Some(GenerateObjectOnStepStart::new(on_step_start));
        self
    }

    /// Upstream experimental alias for [`StreamObjectOptions::with_on_step_start`].
    pub fn with_experimental_on_step_start<F, Fut>(self, on_step_start: F) -> Self
    where
        F: Fn(GenerateObjectStepStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.with_on_step_start(on_step_start)
    }

    /// Sets a callback that is invoked after the streamed model step returns raw text.
    pub fn with_on_step_finish<F, Fut>(mut self, on_step_finish: F) -> Self
    where
        F: Fn(GenerateObjectStepEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_step_finish = Some(GenerateObjectOnStepFinish::new(on_step_finish));
        self
    }

    /// Sets a callback that is invoked after final object parsing and validation.
    pub fn with_on_finish<F, Fut>(mut self, on_finish: F) -> Self
    where
        F: Fn(GenerateObjectEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_finish = Some(GenerateObjectOnFinish::new(on_finish));
        self
    }

    /// Sets a callback that is invoked for provider stream errors.
    pub fn with_on_error<F, Fut>(mut self, on_error: F) -> Self
    where
        F: Fn(StreamObjectOnErrorEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_error = Some(StreamObjectOnError::new(on_error));
        self
    }

    /// Sets a caller-controlled abort signal for this stream.
    pub fn with_abort_signal(mut self, abort_signal: StreamObjectAbortSignal) -> Self {
        self.call_options.abort_signal = Some(abort_signal.clone());
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Sets telemetry options for this object streaming operation.
    pub fn with_telemetry(mut self, telemetry: TelemetryOptions) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    /// Deprecated upstream alias for [`StreamObjectOptions::with_telemetry`].
    pub fn with_experimental_telemetry(self, telemetry: TelemetryOptions) -> Self {
        self.with_telemetry(telemetry)
    }

    /// Sets the maximum number of retries for failed provider stream requests.
    pub fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
        self
    }
}

/// Collected result of a high-level object streaming call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamObjectResult {
    /// All object stream events emitted by the collector.
    pub parts: Vec<ObjectStreamPart>,

    /// Parsed partial object updates emitted during streaming.
    pub partial_object_stream: Vec<JsonValue>,

    /// Complete array elements emitted during array output mode.
    pub element_stream: Vec<JsonValue>,

    /// JSON text deltas emitted by the object stream.
    pub text_stream: Vec<String>,

    /// Full JSON text accumulated from text deltas.
    pub text: String,

    /// Final generated object, array, enum, or no-schema JSON value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<JsonValue>,

    /// Final parse or provider error, when one occurred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonValue>,

    /// Unified finish reason.
    pub finish_reason: FinishReason,

    /// Token usage reported by the provider.
    pub usage: LanguageModelUsage,

    /// Warnings reported by the provider.
    pub warnings: Vec<Warning>,

    /// Optional provider request metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<LanguageModelRequest>,

    /// Provider response metadata.
    pub response: StreamObjectResponseMetadata,

    /// Provider-specific metadata returned with the finish part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl StreamObjectResult {
    /// Deserializes the final object into a caller-provided Rust type.
    pub fn object_as<T>(&self) -> Result<Option<T>, serde_json::Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.object.clone().map(serde_json::from_value).transpose()
    }

    /// Deserializes partial object stream entries into a caller-provided Rust type.
    pub fn partial_objects_as<T>(&self) -> Result<Vec<T>, serde_json::Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.partial_object_stream
            .iter()
            .cloned()
            .map(serde_json::from_value)
            .collect()
    }

    /// Deserializes array output elements into a caller-provided Rust type.
    pub fn elements_as<T>(&self) -> Result<Vec<T>, serde_json::Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.element_stream
            .iter()
            .cloned()
            .map(serde_json::from_value)
            .collect()
    }

    /// Creates a text-stream response from the collected object text stream.
    pub fn to_text_stream_response(&self, init: TextStreamResponseInit) -> TextStreamResponse {
        create_text_stream_response(TextStreamResponseOptions::from_init(
            self.text_stream.clone(),
            init,
        ))
    }

    /// Pipes the collected object text stream to a response writer.
    pub fn pipe_text_stream_to_response<W>(
        &self,
        response: &mut W,
        init: TextStreamResponseInit,
    ) -> Result<(), W::Error>
    where
        W: TextStreamResponseWriter,
    {
        pipe_text_stream_to_response(
            response,
            TextStreamResponseOptions::from_init(self.text_stream.clone(), init),
        )
    }
}

/// Runs an object streaming call against a language model and collects the stream.
pub async fn stream_object<M>(options: StreamObjectOptions<'_, M>) -> StreamObjectResult
where
    M: LanguageModel + ?Sized,
    M::Stream: IntoIterator<Item = LanguageModelStreamPart>,
{
    let StreamObjectOptions {
        model,
        mut call_options,
        schema,
        schema_name,
        schema_description,
        enum_values,
        array_output,
        repair_text,
        on_start,
        on_step_start,
        on_step_finish,
        on_finish,
        on_error,
        abort_signal,
        telemetry,
        max_retries,
    } = options;

    let output_kind = generate_object_output_kind(&schema, enum_values.as_deref(), array_output);
    let response_format = generate_object_response_format(
        &schema,
        &schema_name,
        &schema_description,
        enum_values.as_deref(),
        array_output,
    );
    let event_schema = stream_object_response_format_schema(&response_format);
    call_options.response_format = Some(response_format);
    call_options.include_raw_chunks = Some(false);
    append_stream_object_user_agent(&mut call_options);
    if prompt_has_url_files(&call_options.prompt) {
        let _ = model.supported_urls().await;
    }
    let call_id = generate_object_call_id();
    let telemetry_dispatcher = create_telemetry_dispatcher(telemetry);

    if on_start.is_some() || telemetry_dispatcher.is_enabled() {
        let start_event = GenerateObjectStartEvent {
            call_id: call_id.clone(),
            operation_id: "ai.streamObject".to_string(),
            provider: model.provider().to_string(),
            model_id: model.model_id().to_string(),
            messages: call_options.prompt.clone(),
            max_output_tokens: call_options.max_output_tokens,
            temperature: call_options.temperature,
            top_p: call_options.top_p,
            top_k: call_options.top_k,
            presence_penalty: call_options.presence_penalty,
            frequency_penalty: call_options.frequency_penalty,
            seed: call_options.seed,
            max_retries,
            headers: call_options.headers.clone(),
            provider_options: call_options.provider_options.clone(),
            output: output_kind,
            schema: event_schema.clone(),
            schema_name: schema_name.clone(),
            schema_description: schema_description.clone(),
        };
        if let Some(on_start) = &on_start {
            on_start.start(start_event.clone()).await;
        }
        telemetry_dispatcher.on_start(&start_event);
    }

    if on_step_start.is_some() || telemetry_dispatcher.is_enabled() {
        let step_start_event = GenerateObjectStepStartEvent {
            call_id: call_id.clone(),
            step_number: 0,
            provider: model.provider().to_string(),
            model_id: model.model_id().to_string(),
            provider_options: call_options.provider_options.clone(),
            headers: call_options.headers.clone(),
            prompt_messages: call_options.prompt.clone(),
        };
        if let Some(on_step_start) = &on_step_start {
            on_step_start.start(step_start_event.clone()).await;
        }
        telemetry_dispatcher.on_object_step_start(&step_start_event);
    }

    if let Some(abort_error) = stream_object_abort_error_from_signal(abort_signal.as_ref()) {
        if let Some(on_error) = &on_error {
            on_error
                .error(StreamObjectOnErrorEvent {
                    error: abort_error.clone(),
                })
                .await;
        }

        return StreamObjectResult {
            parts: vec![ObjectStreamPart::Error {
                error: abort_error.clone(),
            }],
            partial_object_stream: Vec::new(),
            element_stream: Vec::new(),
            text_stream: Vec::new(),
            text: String::new(),
            object: None,
            error: Some(abort_error),
            finish_reason: FinishReason::Error,
            usage: LanguageModelUsage::default(),
            warnings: Vec::new(),
            request: None,
            response: StreamObjectResponseMetadata::new(),
            provider_metadata: None,
        };
    }

    let stream_started_at = Instant::now();
    let stream_result = do_stream_object_with_retries(model, call_options, max_retries).await;
    let request = stream_result.request;
    let envelope_response = stream_result.response;
    let mut response = StreamObjectResponseMetadata::new();
    if let Some(envelope_response) = envelope_response.clone() {
        response = response.with_stream_response(envelope_response);
    }

    let mut parts = Vec::new();
    let mut partial_object_stream = Vec::new();
    let mut element_stream = Vec::new();
    let mut text_stream = Vec::new();
    let mut text = String::new();
    let mut pending_text_delta = String::new();
    let mut latest_partial: Option<JsonValue> = None;
    let mut is_first_text_delta = true;
    let mut warnings = Vec::new();
    let mut usage = LanguageModelUsage::default();
    let mut finish_reason = FinishReason::Other;
    let mut provider_metadata = None;
    let mut error = None;
    let mut ms_to_first_chunk = None;
    let mut aborted = false;

    for part in stream_result.stream {
        if let Some(abort_error) = stream_object_abort_error_from_signal(abort_signal.as_ref()) {
            if let Some(on_error) = &on_error {
                on_error
                    .error(StreamObjectOnErrorEvent {
                        error: abort_error.clone(),
                    })
                    .await;
            }
            finish_reason = FinishReason::Error;
            error = Some(abort_error.clone());
            parts.push(ObjectStreamPart::Error { error: abort_error });
            aborted = true;
            break;
        }

        if ms_to_first_chunk.is_none() && !matches!(&part, LanguageModelStreamPart::StreamStart(_))
        {
            ms_to_first_chunk =
                Some(u64::try_from(stream_started_at.elapsed().as_millis()).unwrap_or(u64::MAX));
        }

        match part {
            LanguageModelStreamPart::StreamStart(part) => {
                warnings = part.warnings;
            }
            LanguageModelStreamPart::ResponseMetadata(part) => {
                response = response.with_response_metadata(part);
                if let Some(envelope_response) = envelope_response.clone() {
                    response = response.with_stream_response(envelope_response);
                }
            }
            LanguageModelStreamPart::TextDelta(part) => {
                if part.delta.is_empty() {
                    continue;
                }

                text.push_str(&part.delta);
                pending_text_delta.push_str(&part.delta);

                if let Some(partial_delta) = next_partial_object_delta(
                    &text,
                    &pending_text_delta,
                    output_kind,
                    latest_partial.as_ref(),
                    schema.as_ref(),
                    enum_values.as_deref(),
                    is_first_text_delta,
                ) {
                    append_new_array_elements(
                        &mut element_stream,
                        latest_partial.as_ref(),
                        &partial_delta.partial,
                        output_kind,
                    );
                    latest_partial = Some(partial_delta.partial.clone());
                    partial_object_stream.push(partial_delta.partial.clone());
                    parts.push(ObjectStreamPart::Object {
                        object: partial_delta.partial,
                    });
                    push_text_delta(partial_delta.text_delta, &mut text_stream, &mut parts);
                    pending_text_delta.clear();
                    is_first_text_delta = false;
                }
            }
            LanguageModelStreamPart::Finish(part) => {
                usage = part.usage;
                finish_reason = part.finish_reason.unified;
                provider_metadata = part.provider_metadata;
            }
            LanguageModelStreamPart::Error(part) => {
                if let Some(on_error) = &on_error {
                    on_error
                        .error(StreamObjectOnErrorEvent {
                            error: part.error.clone(),
                        })
                        .await;
                }
                finish_reason = FinishReason::Error;
                error = Some(part.error.clone());
                parts.push(ObjectStreamPart::Error { error: part.error });
            }
            _ => {}
        }
    }

    if aborted {
        return StreamObjectResult {
            parts,
            partial_object_stream,
            element_stream,
            text_stream,
            text,
            object: None,
            error,
            finish_reason,
            usage,
            warnings,
            request,
            response,
            provider_metadata,
        };
    }

    if output_kind != GenerateObjectOutputKind::Array {
        flush_pending_text_delta(&mut pending_text_delta, &mut text_stream, &mut parts);
    }

    let object =
        match parse_generated_object(&text, schema.clone(), enum_values.as_deref(), array_output) {
            ParseJsonResult::Success { value, .. } => Some(value),
            ParseJsonResult::Failure { error: cause, .. } => {
                let original_error = cause.clone();
                match repair_text.as_ref() {
                    Some(repair_text) => {
                        let repaired_text = repair_text
                            .repair(GenerateObjectRepairTextOptions::new(text.clone(), cause))
                            .await;

                        match repaired_text {
                            Some(repaired_text) => match parse_generated_object(
                                &repaired_text,
                                schema,
                                enum_values.as_deref(),
                                array_output,
                            ) {
                                ParseJsonResult::Success { value, .. } => Some(value),
                                ParseJsonResult::Failure {
                                    error: repair_error,
                                    ..
                                } => {
                                    if error.is_none() {
                                        error = Some(JsonValue::String(repair_error.to_string()));
                                    }
                                    None
                                }
                            },
                            None => {
                                if error.is_none() {
                                    error = Some(JsonValue::String(original_error.to_string()));
                                }
                                None
                            }
                        }
                    }
                    None => {
                        if error.is_none() {
                            error = Some(JsonValue::String(original_error.to_string()));
                        }
                        None
                    }
                }
            }
        };

    let callback_request = stream_object_callback_request(request.clone());
    let callback_response = stream_object_callback_response(&response);
    if on_step_finish.is_some() || telemetry_dispatcher.is_enabled() {
        let step_finish_event = GenerateObjectStepEndEvent {
            call_id: call_id.clone(),
            step_number: 0,
            provider: model.provider().to_string(),
            model_id: model.model_id().to_string(),
            finish_reason: finish_reason.clone(),
            usage: usage.clone(),
            object_text: text.clone(),
            reasoning: None,
            warnings: warnings.clone(),
            request: callback_request.clone(),
            response: callback_response.clone(),
            provider_metadata: provider_metadata.clone(),
            ms_to_first_chunk,
        };
        if let Some(on_step_finish) = &on_step_finish {
            on_step_finish.finish(step_finish_event.clone()).await;
        }
        telemetry_dispatcher.on_object_step_finish(&step_finish_event);
    }

    if on_finish.is_some() || telemetry_dispatcher.is_enabled() {
        let finish_event = GenerateObjectEndEvent {
            call_id,
            object: object.clone(),
            error: error.as_ref().map(stream_object_error_message),
            reasoning: None,
            finish_reason: finish_reason.clone(),
            usage: usage.clone(),
            warnings: warnings.clone(),
            request: callback_request,
            response: callback_response,
            provider_metadata: provider_metadata.clone(),
        };
        if let Some(on_finish) = &on_finish {
            on_finish.finish(finish_event.clone()).await;
        }
        telemetry_dispatcher.on_end(&finish_event);
    }

    parts.push(ObjectStreamPart::Finish(Box::new(
        ObjectStreamFinishPart::new(
            finish_reason.clone(),
            usage.clone(),
            response.clone(),
            provider_metadata.clone(),
        ),
    )));

    StreamObjectResult {
        parts,
        partial_object_stream,
        element_stream,
        text_stream,
        text,
        object,
        error,
        finish_reason,
        usage,
        warnings,
        request,
        response,
        provider_metadata,
    }
}

async fn do_stream_object_with_retries<M>(
    model: &M,
    call_options: LanguageModelCallOptions,
    max_retries: usize,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>>
where
    M: LanguageModel + ?Sized,
    M::Stream: IntoIterator<Item = LanguageModelStreamPart>,
{
    let mut retries = 0;

    loop {
        let stream_result = model.do_stream(call_options.clone()).await;
        let stream_result = LanguageModelStreamResult {
            stream: stream_result.stream.into_iter().collect::<Vec<_>>(),
            request: stream_result.request,
            response: stream_result.response,
        };

        if retries < max_retries
            && stream_object_result_is_retryable_pre_stream_failure(&stream_result)
        {
            retries += 1;
            continue;
        }

        return stream_result;
    }
}

fn stream_object_result_is_retryable_pre_stream_failure(
    stream_result: &LanguageModelStreamResult<Vec<LanguageModelStreamPart>>,
) -> bool {
    let mut errors = stream_result.stream.iter().filter_map(|part| match part {
        LanguageModelStreamPart::Error(part) => Some(&part.error),
        _ => None,
    });
    let Some(error) = errors.next() else {
        return false;
    };

    errors.next().is_none()
        && stream_result.stream.iter().all(|part| {
            matches!(
                part,
                LanguageModelStreamPart::StreamStart(_) | LanguageModelStreamPart::Error(_)
            )
        })
        && stream_object_error_is_retryable(error)
}

fn stream_object_error_is_retryable(error: &JsonValue) -> bool {
    error
        .get("isRetryable")
        .or_else(|| error.get("is_retryable"))
        .and_then(JsonValue::as_bool)
        .unwrap_or_else(|| {
            error
                .get("statusCode")
                .or_else(|| error.get("status_code"))
                .and_then(JsonValue::as_u64)
                .and_then(|status_code| u16::try_from(status_code).ok())
                .is_some_and(ApiCallError::is_retryable_status_code)
        })
}

#[derive(Clone, Debug, PartialEq)]
struct PartialObjectDelta {
    partial: JsonValue,
    text_delta: String,
}

fn next_partial_object_delta(
    text: &str,
    text_delta: &str,
    output_kind: GenerateObjectOutputKind,
    latest_partial: Option<&JsonValue>,
    schema: Option<&FlexibleSchema>,
    enum_values: Option<&[String]>,
    is_first_delta: bool,
) -> Option<PartialObjectDelta> {
    let (value, parse_state) = parse_partial_json(Some(text)).into_parts();
    let value = value?;

    if output_kind == GenerateObjectOutputKind::Array {
        return array_partial_object_delta(
            value,
            latest_partial,
            schema,
            is_first_delta,
            parse_state == ParsePartialJsonState::SuccessfulParse,
        );
    }

    if output_kind == GenerateObjectOutputKind::Enum {
        return enum_partial_object_delta(value, latest_partial, enum_values, text_delta);
    }

    let partial = partial_value_for_output(value, output_kind)?;

    if latest_partial.is_some_and(|latest| is_deep_equal_data(latest, &partial)) {
        None
    } else {
        Some(PartialObjectDelta {
            partial,
            text_delta: text_delta.to_string(),
        })
    }
}

fn array_partial_object_delta(
    value: JsonValue,
    latest_partial: Option<&JsonValue>,
    schema: Option<&FlexibleSchema>,
    is_first_delta: bool,
    is_final_delta: bool,
) -> Option<PartialObjectDelta> {
    let elements = value
        .as_object()
        .and_then(|object| object.get("elements"))
        .and_then(JsonValue::as_array)?;

    let mut result_array = Vec::with_capacity(elements.len());

    for (index, element) in elements.iter().enumerate() {
        if index == elements.len().saturating_sub(1) && !is_final_delta {
            continue;
        }

        let element = match schema {
            Some(schema) => match safe_validate_types(element.clone(), schema.clone(), None) {
                ValidateTypesResult::Success { value, .. } => value,
                ValidateTypesResult::Failure { .. } => return None,
            },
            None => element.clone(),
        };

        result_array.push(element);
    }

    let partial = JsonValue::Array(result_array.clone());

    if latest_partial.is_some_and(|latest| is_deep_equal_data(latest, &partial)) {
        return None;
    }

    let published_element_count = latest_partial
        .and_then(JsonValue::as_array)
        .map_or(0, Vec::len);
    let mut text_delta = String::new();

    if is_first_delta {
        text_delta.push('[');
    }

    if published_element_count > 0 {
        text_delta.push(',');
    }

    let new_element_start = published_element_count.min(result_array.len());
    text_delta.push_str(
        &result_array[new_element_start..]
            .iter()
            .map(|element| serde_json::to_string(element).expect("JSON value serializes"))
            .collect::<Vec<_>>()
            .join(","),
    );

    if is_final_delta {
        text_delta.push(']');
    }

    Some(PartialObjectDelta {
        partial,
        text_delta,
    })
}

fn enum_partial_object_delta(
    value: JsonValue,
    latest_partial: Option<&JsonValue>,
    enum_values: Option<&[String]>,
    text_delta: &str,
) -> Option<PartialObjectDelta> {
    let result = value
        .as_object()
        .and_then(|object| object.get("result"))
        .and_then(JsonValue::as_str)?;

    if result.is_empty() {
        return None;
    }

    let possible_enum_values = enum_values?
        .iter()
        .filter(|enum_value| enum_value.starts_with(result))
        .collect::<Vec<_>>();

    let partial = match possible_enum_values.as_slice() {
        [] => return None,
        [enum_value] => JsonValue::String((*enum_value).clone()),
        _ => JsonValue::String(result.to_string()),
    };

    if latest_partial.is_some_and(|latest| is_deep_equal_data(latest, &partial)) {
        None
    } else {
        Some(PartialObjectDelta {
            partial,
            text_delta: text_delta.to_string(),
        })
    }
}

fn partial_value_for_output(
    value: JsonValue,
    output_kind: GenerateObjectOutputKind,
) -> Option<JsonValue> {
    match output_kind {
        GenerateObjectOutputKind::Object | GenerateObjectOutputKind::NoSchema => Some(value),
        GenerateObjectOutputKind::Array => value
            .as_object()
            .and_then(|object| object.get("elements"))
            .cloned(),
        GenerateObjectOutputKind::Enum => value
            .as_object()
            .and_then(|object| object.get("result"))
            .cloned(),
    }
}

fn flush_pending_text_delta(
    pending_text_delta: &mut String,
    text_stream: &mut Vec<String>,
    parts: &mut Vec<ObjectStreamPart>,
) {
    if pending_text_delta.is_empty() {
        return;
    }

    let text_delta = std::mem::take(pending_text_delta);
    push_text_delta(text_delta, text_stream, parts);
}

fn push_text_delta(
    text_delta: String,
    text_stream: &mut Vec<String>,
    parts: &mut Vec<ObjectStreamPart>,
) {
    if text_delta.is_empty() {
        return;
    }

    text_stream.push(text_delta.clone());
    parts.push(ObjectStreamPart::TextDelta { text_delta });
}

fn append_new_array_elements(
    element_stream: &mut Vec<JsonValue>,
    latest_partial: Option<&JsonValue>,
    partial: &JsonValue,
    output_kind: GenerateObjectOutputKind,
) {
    if output_kind != GenerateObjectOutputKind::Array {
        return;
    }

    let Some(partial_array) = partial.as_array() else {
        return;
    };

    let published_element_count = latest_partial
        .and_then(JsonValue::as_array)
        .map_or(0, Vec::len);

    let new_element_start = published_element_count.min(partial_array.len());
    element_stream.extend(partial_array[new_element_start..].iter().cloned());
}

fn append_stream_object_user_agent(call_options: &mut LanguageModelCallOptions) {
    let headers = call_options.headers.take().map(|headers| {
        headers
            .into_iter()
            .map(|(name, value)| (name, Some(value)))
            .collect::<Vec<_>>()
    });

    call_options.headers = Some(with_user_agent_suffix(headers, [format!("ai/{VERSION}")]));
}

fn stream_object_response_format_schema(
    response_format: &LanguageModelResponseFormat,
) -> Option<JsonSchema> {
    match response_format {
        LanguageModelResponseFormat::Json { schema, .. } => schema.clone(),
        LanguageModelResponseFormat::Text => None,
    }
}

fn stream_object_callback_request(request: Option<LanguageModelRequest>) -> GenerateObjectRequest {
    GenerateObjectRequest {
        body: request.and_then(|request| request.body),
    }
}

fn stream_object_callback_response(
    response: &StreamObjectResponseMetadata,
) -> GenerateObjectResponse {
    GenerateObjectResponse {
        id: response.id.clone(),
        timestamp: response.timestamp,
        model_id: response.model_id.clone(),
        headers: response.headers.clone(),
        body: None,
    }
}

fn stream_object_error_message(error: &JsonValue) -> String {
    error
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| error.to_string())
}

fn stream_object_abort_error_from_signal(
    abort_signal: Option<&StreamObjectAbortSignal>,
) -> Option<JsonValue> {
    let abort_signal = abort_signal?;
    if !abort_signal.is_aborted() {
        return None;
    }

    let mut error = serde_json::Map::new();
    error.insert(
        "name".to_string(),
        JsonValue::String("AbortError".to_string()),
    );
    error.insert(
        "message".to_string(),
        JsonValue::String("The streamObject request was aborted.".to_string()),
    );

    if let Some(reason) = abort_signal.reason() {
        error.insert("reason".to_string(), reason);
    }

    Some(JsonValue::Object(error))
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    use serde_json::json;

    use super::*;
    use crate::file_data::FileData;
    use crate::json::JsonSchema;
    use crate::language_model::{
        InputTokenUsage, LanguageModelContent, LanguageModelErrorStreamPart, LanguageModelFilePart,
        LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelMessage,
        LanguageModelResponseFormat, LanguageModelStreamFinish,
        LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
        LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
        LanguageModelSystemMessage, LanguageModelTextDelta, LanguageModelTextPart,
        LanguageModelUserContentPart, LanguageModelUserMessage, OutputTokenUsage,
    };
    use crate::mock_models::MockLanguageModel;
    use crate::provider_utils::{Schema, ValidationResult, json_schema};
    use crate::telemetry::{
        TelemetryEvent, TelemetryEventKind, TelemetryIntegration, TelemetryOptions,
    };
    use url::Url;

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("mock futures should be ready"),
        }
    }

    fn user_message(text: &str) -> LanguageModelMessage {
        LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new(text)),
        ]))
    }

    fn prompt() -> LanguageModelPrompt {
        vec![user_message("prompt")]
    }

    fn usage() -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(4),
                no_cache: Some(4),
                cache_read: Some(0),
                cache_write: Some(0),
            },
            output_tokens: OutputTokenUsage {
                total: Some(8),
                text: Some(8),
                reasoning: Some(0),
            },
            raw: None,
        }
    }

    fn finish_reason() -> LanguageModelFinishReason {
        LanguageModelFinishReason {
            unified: FinishReason::Stop,
            raw: Some("stop".to_string()),
        }
    }

    fn answer_schema() -> Schema {
        json_schema(
            serde_json::from_value::<JsonSchema>(json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "type": "object",
                "properties": {
                    "content": { "type": "string" }
                },
                "required": ["content"],
                "additionalProperties": false
            }))
            .expect("schema should be an object"),
        )
        .with_validator(|value| {
            if value.get("content").is_some_and(JsonValue::is_string) {
                ValidationResult::success(value.clone())
            } else {
                ValidationResult::failure("content must be a string")
            }
        })
    }

    fn number_schema() -> Schema {
        json_schema(
            serde_json::from_value::<JsonSchema>(json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "type": "object",
                "properties": {
                    "number": { "type": "number" }
                },
                "required": ["number"],
                "additionalProperties": false
            }))
            .expect("schema should be an object"),
        )
        .with_validator(|value| {
            if value.get("number").is_some_and(JsonValue::is_number) {
                ValidationResult::success(value.clone())
            } else {
                ValidationResult::failure("number must be numeric")
            }
        })
    }

    fn enum_response_schema(enum_values: &[&str]) -> JsonSchema {
        serde_json::from_value(json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "result": {
                    "type": "string",
                    "enum": enum_values,
                }
            },
            "required": ["result"],
            "additionalProperties": false,
        }))
        .expect("enum response schema is a JSON object")
    }

    #[derive(Debug, serde::Deserialize, PartialEq)]
    struct NumberObject {
        number: u64,
    }

    #[derive(Debug, serde::Deserialize, PartialEq)]
    struct ContentObject {
        content: String,
    }

    fn object_stream() -> Vec<LanguageModelStreamPart> {
        vec![
            LanguageModelStreamPart::ResponseMetadata(
                LanguageModelStreamResponseMetadata::new()
                    .with_id("id-0")
                    .with_model_id("mock-model-id"),
            ),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                "1",
                "\"content\": \"Hello, ",
            )),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "!\"")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", " }")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]
    }

    fn object_stream_with_warnings(warnings: Vec<Warning>) -> Vec<LanguageModelStreamPart> {
        let mut stream = vec![LanguageModelStreamPart::StreamStart(
            LanguageModelStreamStart::new(warnings),
        )];
        stream.extend(object_stream());
        stream
    }

    fn array_three_element_stream() -> Vec<LanguageModelStreamPart> {
        vec![
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                "1",
                r#"{"elements":["#,
            )),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#""content":"#)),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#""element 1""#)),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "},")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ ")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#""content": "#)),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#""element 2""#)),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "},")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#""content":"#)),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#""element 3""#)),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "}")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "]")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "}")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]
    }

    fn array_two_element_single_chunk_stream() -> Vec<LanguageModelStreamPart> {
        vec![
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                "1",
                r#"{"elements":[{"content":"element 1"},{"content":"element 2"}]}"#,
            )),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]
    }

    fn expected_three_element_array() -> JsonValue {
        json!([
            {"content": "element 1"},
            {"content": "element 2"},
            {"content": "element 3"}
        ])
    }

    fn expected_two_element_array() -> JsonValue {
        json!([
            {"content": "element 1"},
            {"content": "element 2"}
        ])
    }

    fn collect_array_stream_result(
        stream: Vec<LanguageModelStreamPart>,
    ) -> (StreamObjectResult, Vec<GenerateObjectEndEvent>) {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(stream));
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateObjectEndEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_array_schema(answer_schema())
                .with_on_finish(move |event| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events
                            .lock()
                            .expect("finish events lock")
                            .push(event);
                    }
                }),
        ));
        let events = finish_events.lock().expect("finish events lock").clone();

        (result, events)
    }

    #[derive(Default)]
    struct ObjectTextStreamResponseSink {
        status: Option<u16>,
        status_text: Option<String>,
        headers: Headers,
        chunks: Vec<Vec<u8>>,
        ended: bool,
    }

    impl ObjectTextStreamResponseSink {
        fn decoded_chunks(&self) -> Vec<String> {
            self.chunks
                .iter()
                .map(|chunk| String::from_utf8(chunk.clone()).expect("chunk decodes"))
                .collect()
        }
    }

    impl TextStreamResponseWriter for ObjectTextStreamResponseSink {
        type Error = Infallible;

        fn write_head(
            &mut self,
            status: u16,
            status_text: Option<&str>,
            headers: &Headers,
        ) -> Result<(), Self::Error> {
            self.status = Some(status);
            self.status_text = status_text.map(ToString::to_string);
            self.headers = headers.clone();
            Ok(())
        }

        fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), Self::Error> {
            self.chunks.push(chunk.to_vec());
            Ok(())
        }

        fn end(&mut self) -> Result<(), Self::Error> {
            self.ended = true;
            Ok(())
        }
    }

    #[derive(Clone, Debug)]
    struct AbortingStreamModel {
        abort_controller: StreamObjectAbortController,
        stream_calls: Arc<Mutex<Vec<LanguageModelCallOptions>>>,
    }

    impl AbortingStreamModel {
        fn new(abort_controller: StreamObjectAbortController) -> Self {
            Self {
                abort_controller,
                stream_calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn stream_calls(&self) -> Vec<LanguageModelCallOptions> {
            self.stream_calls.lock().expect("stream calls lock").clone()
        }
    }

    impl LanguageModel for AbortingStreamModel {
        type SupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a;

        type Stream = Vec<LanguageModelStreamPart>;

        type StreamFuture<'a>
            = Ready<LanguageModelStreamResult<Self::Stream>>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "abort-provider"
        }

        fn model_id(&self) -> &str {
            "abort-model"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(LanguageModelSupportedUrls::new())
        }

        fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(LanguageModelGenerateResult::new(
                Vec::<LanguageModelContent>::new(),
                LanguageModelFinishReason {
                    unified: FinishReason::Other,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ))
        }

        fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            self.stream_calls
                .lock()
                .expect("stream calls lock")
                .push(options);
            self.abort_controller
                .abort_with_reason("client-disconnected");
            ready(LanguageModelStreamResult::new(object_stream()))
        }
    }

    #[derive(Clone, Debug)]
    struct RecordingStreamModel {
        events: Arc<Mutex<Vec<String>>>,
        stream: Vec<LanguageModelStreamPart>,
    }

    impl RecordingStreamModel {
        fn new(events: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                events,
                stream: object_stream(),
            }
        }
    }

    impl LanguageModel for RecordingStreamModel {
        type SupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a;

        type Stream = Vec<LanguageModelStreamPart>;

        type StreamFuture<'a>
            = Ready<LanguageModelStreamResult<Self::Stream>>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "test-model"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(LanguageModelSupportedUrls::new())
        }

        fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(LanguageModelGenerateResult::new(
                Vec::<LanguageModelContent>::new(),
                LanguageModelFinishReason {
                    unified: FinishReason::Other,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ))
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            self.events
                .lock()
                .expect("recording events lock")
                .push("doStream".to_string());
            ready(LanguageModelStreamResult::new(self.stream.clone()))
        }
    }

    #[derive(Clone, Debug)]
    struct SupportedUrlsStreamModel {
        supported_urls_called: Arc<Mutex<bool>>,
    }

    impl SupportedUrlsStreamModel {
        fn new(supported_urls_called: Arc<Mutex<bool>>) -> Self {
            Self {
                supported_urls_called,
            }
        }
    }

    impl LanguageModel for SupportedUrlsStreamModel {
        type SupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a;

        type Stream = Vec<LanguageModelStreamPart>;

        type StreamFuture<'a>
            = Ready<LanguageModelStreamResult<Self::Stream>>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "mock-model-id"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            *self
                .supported_urls_called
                .lock()
                .expect("supported urls called lock") = self.model_id() == "mock-model-id";

            ready(LanguageModelSupportedUrls::from([(
                "image/*".to_string(),
                vec![r"^https://.*$".to_string()],
            )]))
        }

        fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(LanguageModelGenerateResult::new(
                Vec::<LanguageModelContent>::new(),
                LanguageModelFinishReason {
                    unified: FinishReason::Other,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ))
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(object_stream()))
        }
    }

    #[test]
    fn stream_object_calls_model_with_json_response_format_and_standardized_prompt() {
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));

        let result = poll_ready(stream_object(
            StreamObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt("prompt").with_instructions("Return JSON"),
            )
            .expect("prompt should standardize")
            .with_schema(answer_schema())
            .with_schema_name("answer")
            .with_schema_description("Answer object")
            .with_header("x-trace", "trace_123"),
        ));

        assert_eq!(result.object, Some(json!({"content": "Hello, world!"})));

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        let Some(LanguageModelResponseFormat::Json {
            schema,
            name,
            description,
        }) = &calls[0].response_format
        else {
            panic!("expected JSON response format");
        };
        assert!(schema.is_some());
        assert_eq!(name.as_deref(), Some("answer"));
        assert_eq!(description.as_deref(), Some("Answer object"));
        assert_eq!(calls[0].include_raw_chunks, Some(false));
        assert_eq!(
            calls[0]
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-trace")),
            Some(&"trace_123".to_string())
        );
        assert!(
            calls[0]
                .headers
                .as_ref()
                .and_then(|headers| headers.get("user-agent"))
                .is_some_and(|user_agent| user_agent.contains("ai/"))
        );
        assert_eq!(calls[0].prompt.len(), 2);
    }

    #[test]
    fn stream_object_object_stream_sends_object_deltas() {
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(
            result.partial_object_stream,
            vec![
                json!({}),
                json!({"content": "Hello, "}),
                json!({"content": "Hello, world"}),
                json!({"content": "Hello, world!"})
            ]
        );

        let calls = model.stream_calls();
        let Some(LanguageModelResponseFormat::Json {
            schema,
            name,
            description,
        }) = &calls[0].response_format
        else {
            panic!("expected JSON response format");
        };
        assert!(schema.is_some());
        assert_eq!(name, &None);
        assert_eq!(description, &None);
    }

    #[test]
    fn stream_object_object_stream_uses_schema_name_and_description() {
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_schema_name("test-name")
                .with_schema_description("test description"),
        ));

        assert_eq!(
            result.partial_object_stream,
            vec![
                json!({}),
                json!({"content": "Hello, "}),
                json!({"content": "Hello, world"}),
                json!({"content": "Hello, world!"})
            ]
        );

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].prompt, prompt());
        let Some(LanguageModelResponseFormat::Json {
            name, description, ..
        }) = &calls[0].response_format
        else {
            panic!("expected JSON response format");
        };
        assert_eq!(name.as_deref(), Some("test-name"));
        assert_eq!(description.as_deref(), Some("test description"));
    }

    #[test]
    fn stream_object_collects_partial_objects_text_and_finish_metadata() {
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(result.text, "{ \"content\": \"Hello, world!\" }");
        assert_eq!(result.object, Some(json!({"content": "Hello, world!"})));
        assert_eq!(
            result.partial_object_stream,
            vec![
                json!({}),
                json!({"content": "Hello, "}),
                json!({"content": "Hello, world"}),
                json!({"content": "Hello, world!"})
            ]
        );
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage, usage());
        assert_eq!(result.response.id, Some("id-0".to_string()));
        assert!(matches!(
            result.parts.last(),
            Some(ObjectStreamPart::Finish(_))
        ));
    }

    #[test]
    fn stream_object_result_full_stream_matches_upstream_object_chunks() {
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(
            &result.parts[..9],
            &[
                ObjectStreamPart::Object { object: json!({}) },
                ObjectStreamPart::TextDelta {
                    text_delta: "{ ".to_string(),
                },
                ObjectStreamPart::Object {
                    object: json!({ "content": "Hello, " }),
                },
                ObjectStreamPart::TextDelta {
                    text_delta: "\"content\": \"Hello, ".to_string(),
                },
                ObjectStreamPart::Object {
                    object: json!({ "content": "Hello, world" }),
                },
                ObjectStreamPart::TextDelta {
                    text_delta: "world".to_string(),
                },
                ObjectStreamPart::Object {
                    object: json!({ "content": "Hello, world!" }),
                },
                ObjectStreamPart::TextDelta {
                    text_delta: "!\"".to_string(),
                },
                ObjectStreamPart::TextDelta {
                    text_delta: " }".to_string(),
                },
            ]
        );

        let Some(ObjectStreamPart::Finish(finish)) = result.parts.last() else {
            panic!("full stream ends with finish part");
        };
        assert_eq!(finish.finish_reason, FinishReason::Stop);
        assert_eq!(finish.usage, usage());
        assert_eq!(finish.response.id.as_deref(), Some("id-0"));
        assert_eq!(finish.response.model_id.as_deref(), Some("mock-model-id"));
        assert_eq!(finish.provider_metadata, None);
    }

    #[test]
    fn stream_object_result_full_stream_sends_finish_provider_metadata_and_timestamp() {
        let mut provider_metadata = ProviderMetadata::new();
        let mut test_provider_metadata = serde_json::Map::new();
        test_provider_metadata.insert("testKey".to_string(), json!("testValue"));
        provider_metadata.insert("testProvider".to_string(), test_provider_metadata);
        let timestamp = time::OffsetDateTime::UNIX_EPOCH;
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-0")
                        .with_model_id("mock-model-id")
                        .with_timestamp(timestamp),
                ),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#"{ "content": "Hello, world!" }"#,
                )),
                LanguageModelStreamPart::Finish(
                    LanguageModelStreamFinish::new(usage(), finish_reason())
                        .with_provider_metadata(provider_metadata.clone()),
                ),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        let Some(ObjectStreamPart::Finish(finish)) = result.parts.last() else {
            panic!("full stream ends with finish part");
        };
        assert_eq!(finish.finish_reason, FinishReason::Stop);
        assert_eq!(finish.usage, usage());
        assert_eq!(finish.response.id.as_deref(), Some("id-0"));
        assert_eq!(finish.response.model_id.as_deref(), Some("mock-model-id"));
        assert_eq!(finish.response.timestamp, Some(timestamp));
        assert_eq!(finish.provider_metadata, Some(provider_metadata));
    }

    #[test]
    fn stream_object_result_text_stream_and_response_match_upstream_object_chunks() {
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        let expected_chunks = vec![
            "{ ".to_string(),
            "\"content\": \"Hello, ".to_string(),
            "world".to_string(),
            "!\"".to_string(),
            " }".to_string(),
        ];
        assert_eq!(result.text_stream, expected_chunks);

        let response = result.to_text_stream_response(TextStreamResponseInit::new());
        assert_eq!(response.status, 200);
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some(crate::text_stream_response::TEXT_STREAM_CONTENT_TYPE)
        );
        assert_eq!(
            response.decoded_body().expect("response body decodes"),
            expected_chunks
        );
    }

    #[test]
    fn stream_object_result_pipe_text_stream_to_response_writes_default_headers_chunks_and_end() {
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));
        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));
        let mut response = ObjectTextStreamResponseSink::default();

        result
            .pipe_text_stream_to_response(&mut response, TextStreamResponseInit::new())
            .expect("response writer succeeds");

        assert_eq!(response.status, Some(200));
        assert_eq!(response.status_text, None);
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some(crate::text_stream_response::TEXT_STREAM_CONTENT_TYPE)
        );
        assert_eq!(response.decoded_chunks(), result.text_stream);
        assert!(response.ended);
    }

    #[test]
    fn stream_object_passes_headers_to_model() {
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_header("custom-request-header", "request-header-value"),
        ));

        assert_eq!(
            result.partial_object_stream.last(),
            Some(&json!({"content": "Hello, world!"}))
        );
        let calls = model.stream_calls();
        let headers = calls[0].headers.as_ref().expect("headers are forwarded");
        assert_eq!(
            headers.get("custom-request-header").map(String::as_str),
            Some("request-header-value")
        );
        assert!(
            headers
                .get("user-agent")
                .is_some_and(|user_agent| user_agent.contains("ai/"))
        );
    }

    #[test]
    fn stream_object_passes_provider_options_to_model() {
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));
        let mut provider_options = ProviderOptions::new();
        let mut provider = serde_json::Map::new();
        provider.insert("someKey".to_string(), json!("someValue"));
        provider_options.insert("aProvider".to_string(), provider);

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_provider_options(provider_options.clone()),
        ));

        assert_eq!(
            result.partial_object_stream.last(),
            Some(&json!({"content": "Hello, world!"}))
        );
        let calls = model.stream_calls();
        assert_eq!(calls[0].provider_options, Some(provider_options));
    }

    #[test]
    fn stream_object_result_usage_resolves_with_token_usage() {
        let expected_usage = LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(3),
                no_cache: Some(3),
                cache_read: None,
                cache_write: None,
            },
            output_tokens: OutputTokenUsage {
                total: Some(10),
                text: Some(10),
                reasoning: None,
            },
            raw: None,
        };
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{ \"content\": \"Hello, world!\" }",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    expected_usage.clone(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(result.object, Some(json!({"content": "Hello, world!"})));
        assert_eq!(result.usage, expected_usage);
    }

    #[test]
    fn stream_object_result_provider_metadata_resolves_with_provider_metadata() {
        let mut provider_metadata = ProviderMetadata::new();
        let mut test_provider_metadata = serde_json::Map::new();
        test_provider_metadata.insert("testKey".to_string(), json!("testValue"));
        provider_metadata.insert("testProvider".to_string(), test_provider_metadata);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{ \"content\": \"Hello, world!\" }",
                )),
                LanguageModelStreamPart::Finish(
                    LanguageModelStreamFinish::new(usage(), finish_reason())
                        .with_provider_metadata(provider_metadata.clone()),
                ),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(result.object, Some(json!({"content": "Hello, world!"})));
        assert_eq!(result.provider_metadata, Some(provider_metadata));
    }

    #[test]
    fn stream_object_result_response_resolves_with_response_information() {
        let mut response_headers = Headers::new();
        response_headers.insert("call".to_string(), "2".to_string());
        let model = MockLanguageModel::new().with_stream_result(
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-0")
                        .with_model_id("mock-model-id")
                        .with_timestamp(time::OffsetDateTime::UNIX_EPOCH),
                ),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"content\": \"Hello, world!\"}",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ])
            .with_response(LanguageModelStreamResultResponse {
                headers: Some(response_headers.clone()),
            }),
        );

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(result.object, Some(json!({"content": "Hello, world!"})));
        assert_eq!(result.response.id.as_deref(), Some("id-0"));
        assert_eq!(result.response.model_id.as_deref(), Some("mock-model-id"));
        assert_eq!(
            result.response.timestamp,
            Some(time::OffsetDateTime::UNIX_EPOCH)
        );
        assert_eq!(result.response.headers, Some(response_headers));
    }

    #[test]
    fn stream_object_result_request_contains_request_information() {
        let request = LanguageModelRequest::new().with_body(json!("test body"));
        let model = MockLanguageModel::new().with_stream_result(
            LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-0")
                        .with_model_id("mock-model-id")
                        .with_timestamp(time::OffsetDateTime::UNIX_EPOCH),
                ),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"content\": \"Hello, world!\"}",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ])
            .with_request(request.clone()),
        );

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(result.object, Some(json!({"content": "Hello, world!"})));
        assert_eq!(result.request, Some(request));
    }

    #[test]
    fn stream_object_result_object_resolves_with_typed_object() {
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(result.object, Some(json!({"content": "Hello, world!"})));
        assert_eq!(result.error, None);
    }

    #[test]
    fn stream_object_result_object_errors_when_streamed_object_does_not_match_schema() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#""invalid": "#,
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#""Hello, "#)),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#"!""#)),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", " }")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(result.object, None);
        assert!(result.error.is_some());
    }

    #[test]
    fn stream_object_result_object_schema_error_is_observable_without_unhandled_rejection() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#""invalid": "#,
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#""Hello, "#)),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#"!""#)),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", " }")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert!(result.error.is_some());
        assert_eq!(
            result.partial_object_stream.last(),
            Some(&json!({"invalid": "Hello, world!"}))
        );
        assert!(matches!(
            result.parts.last(),
            Some(ObjectStreamPart::Finish(_))
        ));
    }

    #[test]
    fn stream_object_result_finish_reason_resolves_with_finish_reason() {
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(result.object, Some(json!({"content": "Hello, world!"})));
        assert_eq!(result.finish_reason, FinishReason::Stop);
    }

    #[test]
    fn stream_object_type_counterpart_finish_reason_property_has_finish_reason_type() {
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));
        let finish_reason: FinishReason = result.finish_reason.clone();

        assert_eq!(finish_reason, FinishReason::Stop);
    }

    #[test]
    fn stream_object_type_counterpart_supports_schema_types() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"number\":42}",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(number_schema()),
        ));
        let object: Option<NumberObject> = result.object_as().expect("object is typed");

        assert_eq!(object, Some(NumberObject { number: 42 }));
    }

    #[test]
    fn stream_object_type_counterpart_supports_no_schema_output_mode() {
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));

        let result = poll_ready(stream_object(StreamObjectOptions::new(&model, prompt())));
        let object: Option<JsonValue> = result.object_as().expect("object is JSON");

        assert_eq!(object, Some(json!({"content": "Hello, world!"})));
    }

    #[test]
    fn stream_object_type_counterpart_supports_enum_types() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"result\":\"green\"}",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_enum_values(["red", "green"]),
        ));
        let object: Option<String> = result.object_as().expect("enum object is typed");
        let partials: Vec<String> = result
            .partial_objects_as()
            .expect("enum partials are typed");

        assert_eq!(object, Some("green".to_string()));
        assert_eq!(partials, vec!["green".to_string()]);
    }

    #[test]
    fn stream_object_type_counterpart_supports_array_output_mode() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"elements\":[",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"content\":\"one\"},",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"content\":\"two\"}",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "]}")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_array_schema(answer_schema()),
        ));
        let object: Option<Vec<ContentObject>> = result.object_as().expect("array is typed");
        let partials: Vec<Vec<ContentObject>> = result
            .partial_objects_as()
            .expect("array partials are typed");
        let elements: Vec<ContentObject> = result.elements_as().expect("elements are typed");

        assert_eq!(
            object,
            Some(vec![
                ContentObject {
                    content: "one".to_string()
                },
                ContentObject {
                    content: "two".to_string()
                }
            ])
        );
        assert_eq!(
            partials,
            vec![
                vec![],
                vec![ContentObject {
                    content: "one".to_string()
                }],
                vec![
                    ContentObject {
                        content: "one".to_string()
                    },
                    ContentObject {
                        content: "two".to_string()
                    }
                ]
            ]
        );
        assert_eq!(
            elements,
            vec![
                ContentObject {
                    content: "one".to_string()
                },
                ContentObject {
                    content: "two".to_string()
                }
            ]
        );
    }

    #[test]
    fn stream_object_array_output_unwraps_elements() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"elements\":[",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"content\":\"one\"},",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"content\":\"two\"}",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "]}")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_array_schema(answer_schema()),
        ));

        assert_eq!(
            result.object,
            Some(json!([
                {"content": "one"},
                {"content": "two"}
            ]))
        );
        assert_eq!(
            result.element_stream,
            vec![json!({"content": "one"}), json!({"content": "two"})]
        );
        assert_eq!(
            result.partial_object_stream,
            vec![
                json!([]),
                json!([{"content": "one"}]),
                json!([{"content": "one"}, {"content": "two"}])
            ]
        );
        assert_eq!(
            result.text_stream,
            vec![
                "[".to_string(),
                r#"{"content":"one"}"#.to_string(),
                r#",{"content":"two"}]"#.to_string()
            ]
        );

        let text_response = result.to_text_stream_response(
            TextStreamResponseInit::new()
                .with_status(206)
                .with_header("x-stream", "object"),
        );

        assert_eq!(text_response.status, 206);
        assert_eq!(
            text_response
                .headers
                .get("content-type")
                .map(String::as_str),
            Some(crate::text_stream_response::TEXT_STREAM_CONTENT_TYPE)
        );
        assert_eq!(
            text_response.headers.get("x-stream").map(String::as_str),
            Some("object")
        );
        assert_eq!(
            text_response.decoded_body().expect("response body decodes"),
            result.text_stream
        );
    }

    #[test]
    fn stream_object_array_output_formats_single_chunk_text_delta() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#"{"elements":[{"content":"one"},{"content":"two"}]}"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_array_schema(answer_schema()),
        ));

        assert_eq!(
            result.partial_object_stream,
            vec![json!([{"content": "one"}, {"content": "two"}])]
        );
        assert_eq!(
            result.text_stream,
            vec![r#"[{"content":"one"},{"content":"two"}]"#.to_string()]
        );
        assert_eq!(
            result.element_stream,
            vec![json!({"content": "one"}), json!({"content": "two"})]
        );
    }

    #[test]
    fn stream_object_array_three_elements_streams_complete_objects_in_partial_object_stream() {
        let (result, _) = collect_array_stream_result(array_three_element_stream());

        assert_eq!(
            result.partial_object_stream,
            vec![
                json!([]),
                json!([{"content": "element 1"}]),
                json!([{"content": "element 1"}, {"content": "element 2"}]),
                expected_three_element_array()
            ]
        );
    }

    #[test]
    fn stream_object_array_three_elements_streams_complete_objects_in_text_stream() {
        let (result, _) = collect_array_stream_result(array_three_element_stream());

        assert_eq!(
            result.text_stream,
            vec![
                "[".to_string(),
                r#"{"content":"element 1"}"#.to_string(),
                r#",{"content":"element 2"}"#.to_string(),
                r#",{"content":"element 3"}]"#.to_string()
            ]
        );
    }

    #[test]
    fn stream_object_array_three_elements_has_correct_object_result() {
        let (result, _) = collect_array_stream_result(array_three_element_stream());

        assert_eq!(result.object, Some(expected_three_element_array()));
    }

    #[test]
    fn stream_object_array_three_elements_calls_on_finish_with_full_array() {
        let (_, finish_events) = collect_array_stream_result(array_three_element_stream());

        assert_eq!(finish_events.len(), 1);
        assert_eq!(
            finish_events[0].object,
            Some(expected_three_element_array())
        );
    }

    #[test]
    fn stream_object_array_three_elements_streams_elements_individually() {
        let (result, _) = collect_array_stream_result(array_three_element_stream());

        assert_eq!(
            result.element_stream,
            vec![
                json!({"content": "element 1"}),
                json!({"content": "element 2"}),
                json!({"content": "element 3"})
            ]
        );
    }

    #[test]
    fn stream_object_array_single_chunk_streams_complete_objects_in_partial_object_stream() {
        let (result, _) = collect_array_stream_result(array_two_element_single_chunk_stream());

        assert_eq!(
            result.partial_object_stream,
            vec![expected_two_element_array()]
        );
    }

    #[test]
    fn stream_object_array_single_chunk_streams_complete_objects_in_text_stream() {
        let (result, _) = collect_array_stream_result(array_two_element_single_chunk_stream());

        assert_eq!(
            result.text_stream,
            vec![r#"[{"content":"element 1"},{"content":"element 2"}]"#.to_string()]
        );
    }

    #[test]
    fn stream_object_array_single_chunk_has_correct_object_result() {
        let (result, _) = collect_array_stream_result(array_two_element_single_chunk_stream());

        assert_eq!(result.object, Some(expected_two_element_array()));
    }

    #[test]
    fn stream_object_array_single_chunk_calls_on_finish_with_full_array() {
        let (_, finish_events) =
            collect_array_stream_result(array_two_element_single_chunk_stream());

        assert_eq!(finish_events.len(), 1);
        assert_eq!(finish_events[0].object, Some(expected_two_element_array()));
    }

    #[test]
    fn stream_object_array_single_chunk_streams_elements_individually() {
        let (result, _) = collect_array_stream_result(array_two_element_single_chunk_stream());

        assert_eq!(
            result.element_stream,
            vec![
                json!({"content": "element 1"}),
                json!({"content": "element 2"})
            ]
        );
    }

    #[test]
    fn stream_object_warnings_resolve_empty_when_no_warnings_are_present() {
        let model = MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(
            object_stream_with_warnings(Vec::new()),
        ));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(result.warnings, Vec::new());
    }

    #[test]
    fn stream_object_warnings_resolve_model_warnings() {
        let expected_warnings = vec![
            Warning::Unsupported {
                feature: "frequency_penalty".to_string(),
                details: Some("This model does not support the frequency_penalty setting.".into()),
            },
            Warning::Other {
                message: "Test warning message".to_string(),
            },
        ];
        let model = MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(
            object_stream_with_warnings(expected_warnings.clone()),
        ));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(result.warnings, expected_warnings);
    }

    #[test]
    fn stream_object_warnings_are_available_to_step_finish_and_finish_callbacks() {
        let expected_warnings = vec![
            Warning::Other {
                message: "Setting is not supported".to_string(),
            },
            Warning::Unsupported {
                feature: "temperature".to_string(),
                details: Some("Temperature parameter not supported".to_string()),
            },
        ];
        let model = MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(
            object_stream_with_warnings(expected_warnings.clone()),
        ));
        let step_warnings = Arc::new(Mutex::new(Vec::<Warning>::new()));
        let finish_warnings = Arc::new(Mutex::new(Vec::<Warning>::new()));
        let step_warnings_for_callback = Arc::clone(&step_warnings);
        let finish_warnings_for_callback = Arc::clone(&finish_warnings);

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_on_step_finish(move |event| {
                    let step_warnings = Arc::clone(&step_warnings_for_callback);
                    async move {
                        *step_warnings.lock().expect("step warnings lock") = event.warnings;
                    }
                })
                .with_on_finish(move |event| {
                    let finish_warnings = Arc::clone(&finish_warnings_for_callback);
                    async move {
                        *finish_warnings.lock().expect("finish warnings lock") = event.warnings;
                    }
                }),
        ));

        assert_eq!(result.warnings, expected_warnings);
        assert_eq!(
            *step_warnings.lock().expect("step warnings lock"),
            expected_warnings
        );
        assert_eq!(
            *finish_warnings.lock().expect("finish warnings lock"),
            expected_warnings
        );
    }

    #[test]
    fn stream_object_enum_output_unwraps_result() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"result\":\"green\"}",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_enum_values(["red", "green"]),
        ));

        assert_eq!(result.object, Some(json!("green")));
        assert_eq!(result.partial_object_stream, vec![json!("green")]);
    }

    #[test]
    fn stream_object_enum_output_streams_value_and_sends_response_format() {
        let enum_values = ["sunny", "rainy", "snowy"];
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#""result": "#,
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#""su"#)),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "nny")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#"""#)),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", " }")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_enum_values(enum_values),
        ));

        assert_eq!(result.object, Some(json!("sunny")));
        assert_eq!(result.partial_object_stream, vec![json!("sunny")]);
        assert_eq!(
            model.stream_calls()[0].response_format,
            Some(
                LanguageModelResponseFormat::json().with_schema(enum_response_schema(&enum_values))
            )
        );
    }

    #[test]
    fn stream_object_enum_output_completes_unambiguous_prefixes() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"result\":\"su",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "nny\"}")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_enum_values(["sunny", "rainy"]),
        ));

        assert_eq!(result.object, Some(json!("sunny")));
        assert_eq!(result.partial_object_stream, vec![json!("sunny")]);
    }

    #[test]
    fn stream_object_enum_output_keeps_ambiguous_prefixes_until_resolved() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"result\":\"foo",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "bar\"}")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_enum_values(["foobar", "foobar2"]),
        ));

        assert_eq!(result.object, Some(json!("foobar")));
        assert_eq!(
            result.partial_object_stream,
            vec![json!("foo"), json!("foobar")]
        );
    }

    #[test]
    fn stream_object_enum_output_suppresses_impossible_prefixes() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{\"result\":\"foo",
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "bar\"}")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_enum_values(["sunny", "rainy"]),
        ));

        assert_eq!(result.object, None);
        assert_eq!(result.partial_object_stream, Vec::<JsonValue>::new());
    }

    #[test]
    fn stream_object_enum_output_handles_non_ambiguous_values() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#""result": "#,
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#""foo"#)),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "bar")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#"""#)),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", " }")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_enum_values(["foobar", "barfoo"]),
        ));

        assert_eq!(result.object, Some(json!("foobar")));
        assert_eq!(result.partial_object_stream, vec![json!("foobar")]);
    }

    #[test]
    fn stream_object_no_schema_output_streams_partial_objects_without_response_schema() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#""content": "#,
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#""Hello, "#)),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#"!""#)),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", " }")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(StreamObjectOptions::new(&model, prompt())));

        assert_eq!(result.object, Some(json!({"content": "Hello, world!"})));
        assert_eq!(
            result.partial_object_stream,
            vec![
                json!({}),
                json!({"content": "Hello, "}),
                json!({"content": "Hello, world"}),
                json!({"content": "Hello, world!"})
            ]
        );

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].response_format,
            Some(LanguageModelResponseFormat::json())
        );
    }

    #[test]
    fn stream_object_messages_with_url_file_calls_model_supported_urls() {
        let supported_urls_called = Arc::new(Mutex::new(false));
        let model = SupportedUrlsStreamModel::new(Arc::clone(&supported_urls_called));
        let prompt = vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::File(
                LanguageModelFilePart::new(
                    FileData::Url {
                        url: Url::parse("https://example.com/test.jpg").expect("url parses"),
                    },
                    "image/jpeg",
                ),
            )],
        ))];

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt).with_schema(answer_schema()),
        ));

        assert_eq!(result.text, r#"{ "content": "Hello, world!" }"#);
        assert_eq!(result.object, Some(json!({ "content": "Hello, world!" })));
        assert!(
            *supported_urls_called
                .lock()
                .expect("supported urls called lock")
        );
    }

    #[test]
    fn stream_object_custom_schema_sends_object_deltas() {
        let schema = answer_schema();
        let expected_schema = schema.json_schema().clone();
        let model = MockLanguageModel::new()
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(schema),
        ));

        assert_eq!(
            result.partial_object_stream,
            vec![
                json!({}),
                json!({"content": "Hello, "}),
                json!({"content": "Hello, world"}),
                json!({"content": "Hello, world!"})
            ]
        );
        let calls = model.stream_calls();
        let Some(LanguageModelResponseFormat::Json {
            schema,
            name,
            description,
        }) = &calls[0].response_format
        else {
            panic!("expected JSON response format");
        };
        assert_eq!(schema.as_ref(), Some(&expected_schema));
        assert_eq!(name, &None);
        assert_eq!(description, &None);
    }

    #[test]
    fn stream_object_error_handling_reports_no_object_when_schema_validation_fails() {
        let timestamp = time::OffsetDateTime::UNIX_EPOCH + time::Duration::milliseconds(123);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#"{ "content": 123 }"#,
                )),
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-1")
                        .with_timestamp(timestamp)
                        .with_model_id("model-1"),
                ),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(result.object, None);
        assert!(result.error.is_some());
        assert_eq!(result.response.id.as_deref(), Some("id-1"));
        assert_eq!(result.response.timestamp, Some(timestamp));
        assert_eq!(result.response.model_id.as_deref(), Some("model-1"));
        assert_eq!(result.usage, usage());
        assert_eq!(result.finish_reason, FinishReason::Stop);
    }

    #[test]
    fn stream_object_error_handling_reports_no_object_when_parsing_fails() {
        let timestamp = time::OffsetDateTime::UNIX_EPOCH + time::Duration::milliseconds(123);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "{ broken json",
                )),
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-1")
                        .with_timestamp(timestamp)
                        .with_model_id("model-1"),
                ),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(result.object, None);
        assert!(result.error.is_some());
        assert_eq!(result.response.id.as_deref(), Some("id-1"));
        assert_eq!(result.response.timestamp, Some(timestamp));
        assert_eq!(result.response.model_id.as_deref(), Some("model-1"));
        assert_eq!(result.usage, usage());
        assert_eq!(result.finish_reason, FinishReason::Stop);
    }

    #[test]
    fn stream_object_error_handling_reports_no_object_when_no_text_is_generated() {
        let timestamp = time::OffsetDateTime::UNIX_EPOCH + time::Duration::milliseconds(123);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-1")
                        .with_timestamp(timestamp)
                        .with_model_id("model-1"),
                ),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ));

        assert_eq!(result.text, "");
        assert_eq!(result.object, None);
        assert!(result.error.is_some());
        assert_eq!(result.response.id.as_deref(), Some("id-1"));
        assert_eq!(result.response.timestamp, Some(timestamp));
        assert_eq!(result.response.model_id.as_deref(), Some("model-1"));
        assert_eq!(result.usage, usage());
        assert_eq!(result.finish_reason, FinishReason::Stop);
    }

    #[test]
    fn stream_object_partial_object_stream_suppresses_provider_errors() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(
                    json!({"message": "test error"}),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_on_error(|_| ready(())),
        ));

        assert!(result.partial_object_stream.is_empty());
        assert_eq!(result.finish_reason, FinishReason::Error);
        assert_eq!(result.error, Some(json!({"message": "test error"})));
    }

    #[test]
    fn stream_object_retains_error_parts() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ broken")),
                LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(
                    json!({"message": "chunk failed"}),
                )),
            ]));

        let result = poll_ready(stream_object(StreamObjectOptions::new(&model, prompt())));

        assert_eq!(result.finish_reason, FinishReason::Error);
        assert_eq!(result.error, Some(json!({"message": "chunk failed"})));
        assert!(matches!(
            result.parts.last(),
            Some(ObjectStreamPart::Finish(_))
        ));
        assert!(
            result
                .parts
                .iter()
                .any(|part| matches!(part, ObjectStreamPart::Error { .. }))
        );
    }

    #[test]
    fn stream_object_invokes_error_callback_for_error_parts() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(
                    json!({"message": "chunk failed"}),
                )),
            ]));
        let callback_errors = Arc::new(Mutex::new(Vec::new()));
        let errors_for_callback = Arc::clone(&callback_errors);

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt()).with_on_error(move |event| {
                let errors = Arc::clone(&errors_for_callback);
                async move {
                    errors
                        .lock()
                        .expect("callback errors lock")
                        .push(event.error);
                }
            }),
        ));

        assert_eq!(result.finish_reason, FinishReason::Error);
        assert_eq!(result.error, Some(json!({"message": "chunk failed"})));
        assert_eq!(
            callback_errors
                .lock()
                .expect("callback errors lock")
                .as_slice(),
            [json!({"message": "chunk failed"})]
        );
    }

    #[test]
    fn stream_object_retries_retryable_pre_stream_errors() {
        let retryable_error = LanguageModelStreamResult::new(vec![LanguageModelStreamPart::Error(
            LanguageModelErrorStreamPart::new(json!({
                "message": "rate limited",
                "statusCode": 429,
                "isRetryable": true
            })),
        )]);
        let successful_stream = LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                "json",
                "{\"answer\":42}",
            )),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);
        let model =
            MockLanguageModel::new().with_stream_results([retryable_error, successful_stream]);
        let callback_errors = Arc::new(Mutex::new(Vec::<String>::new()));
        let errors = Arc::clone(&callback_errors);

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_max_retries(1)
                .with_on_error(move |event| {
                    let errors = Arc::clone(&errors);
                    async move {
                        errors
                            .lock()
                            .expect("errors lock")
                            .push(event.error["message"].as_str().unwrap_or("").to_string());
                    }
                }),
        ));

        assert_eq!(model.stream_calls().len(), 2);
        assert_eq!(result.object, Some(json!({ "answer": 42 })));
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.error, None);
        assert!(
            !result
                .parts
                .iter()
                .any(|part| matches!(part, ObjectStreamPart::Error { .. }))
        );
        assert!(callback_errors.lock().expect("errors lock").is_empty());
    }

    #[test]
    fn stream_object_repair_text_repairs_json_parse_error() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#"{ "content": "provider metadata test" "#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_experimental_repair_text(|options| async move {
                    assert_eq!(options.text(), r#"{ "content": "provider metadata test" "#);
                    assert!(options.error().as_json_parse_error().is_some());

                    Some(format!("{}}}", options.text()))
                }),
        ));

        assert_eq!(
            result.object,
            Some(json!({ "content": "provider metadata test" }))
        );
        assert_eq!(result.error, None);
    }

    #[test]
    fn stream_object_repair_text_repairs_type_validation_error() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#"{ "content-a": "provider metadata test" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_repair_text(|options| async move {
                    assert_eq!(
                        options.text(),
                        r#"{ "content-a": "provider metadata test" }"#
                    );
                    assert!(options.error().as_type_validation_error().is_some());

                    Some(r#"{ "content": "provider metadata test" }"#.to_string())
                }),
        ));

        assert_eq!(
            result.object,
            Some(json!({ "content": "provider metadata test" }))
        );
        assert_eq!(result.error, None);
    }

    #[test]
    fn stream_object_repair_text_handles_repair_returning_none() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#"{ "content-a": "provider metadata test" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_repair_text(|options| async move {
                    assert_eq!(
                        options.text(),
                        r#"{ "content-a": "provider metadata test" }"#
                    );
                    assert!(options.error().as_type_validation_error().is_some());

                    None
                }),
        ));

        assert_eq!(result.object, None);
        assert!(result.error.is_some());
    }

    #[test]
    fn stream_object_repair_text_repairs_json_wrapped_with_markdown_code_blocks() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    "```json\n{ \"content\": \"test message\" }\n```",
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_experimental_repair_text(|options| async move {
                    assert_eq!(
                        options.text(),
                        "```json\n{ \"content\": \"test message\" }\n```"
                    );
                    assert!(options.error().as_json_parse_error().is_some());

                    Some(
                        options
                            .text()
                            .trim_start_matches("```json")
                            .trim_start()
                            .trim_end_matches("```")
                            .trim_end()
                            .to_string(),
                    )
                }),
        ));

        assert_eq!(result.object, Some(json!({ "content": "test message" })));
        assert_eq!(result.error, None);
    }

    #[test]
    fn stream_object_repair_text_reports_no_object_when_parsing_still_fails() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ bad")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_repair_text(|options| async move {
                    assert_eq!(options.text(), "{ bad");
                    assert!(options.error().as_json_parse_error().is_some());

                    Some(format!("{}{{", options.text()))
                }),
        ));

        assert_eq!(result.object, None);
        assert!(result.error.is_some());
    }

    #[test]
    fn stream_object_on_start_runs_before_model_call() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let model = RecordingStreamModel::new(Arc::clone(&events));
        let start_events = Arc::clone(&events);

        poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_experimental_on_start(move |_| {
                    let start_events = Arc::clone(&start_events);
                    async move {
                        start_events
                            .lock()
                            .expect("recording events lock")
                            .push("onStart".to_string());
                    }
                }),
        ));

        assert_eq!(
            *events.lock().expect("recording events lock"),
            vec!["onStart".to_string(), "doStream".to_string()]
        );
    }

    #[test]
    fn stream_object_on_start_sends_text_prompt_information() {
        let model = MockLanguageModel::new()
            .with_provider("test-provider")
            .with_model_id("test-model")
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));
        let start_event = Arc::new(Mutex::new(None::<GenerateObjectStartEvent>));
        let start_event_for_callback = Arc::clone(&start_event);

        poll_ready(stream_object(
            StreamObjectOptions::from_prompt(
                &model,
                Prompt::from_prompt("test-prompt").with_instructions("Return an answer"),
            )
            .expect("prompt standardizes")
            .with_schema(answer_schema())
            .with_schema_name("test-schema")
            .with_schema_description("A test schema")
            .with_temperature(0.5)
            .with_max_output_tokens(100)
            .with_experimental_on_start(move |event| {
                let start_event = Arc::clone(&start_event_for_callback);
                async move {
                    *start_event.lock().expect("start event lock") = Some(event);
                }
            }),
        ));

        let event = start_event
            .lock()
            .expect("start event lock")
            .clone()
            .expect("start event captured");
        assert!(event.call_id.starts_with("aiobj-"));
        assert_eq!(event.operation_id, "ai.streamObject");
        assert_eq!(event.provider, "test-provider");
        assert_eq!(event.model_id, "test-model");
        assert_eq!(event.temperature, Some(0.5));
        assert_eq!(event.max_output_tokens, Some(100));
        assert_eq!(event.output, GenerateObjectOutputKind::Object);
        assert_eq!(event.schema_name.as_deref(), Some("test-schema"));
        assert_eq!(event.schema_description.as_deref(), Some("A test schema"));
        assert_eq!(
            event.messages,
            vec![
                LanguageModelMessage::System(LanguageModelSystemMessage::new("Return an answer")),
                user_message("test-prompt")
            ]
        );
    }

    #[test]
    fn stream_object_on_step_start_runs_before_model_call() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let model = RecordingStreamModel::new(Arc::clone(&events));
        let step_start_events = Arc::clone(&events);

        poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_experimental_on_step_start(move |_| {
                    let step_start_events = Arc::clone(&step_start_events);
                    async move {
                        step_start_events
                            .lock()
                            .expect("recording events lock")
                            .push("onStepStart".to_string());
                    }
                }),
        ));

        assert_eq!(
            *events.lock().expect("recording events lock"),
            vec!["onStepStart".to_string(), "doStream".to_string()]
        );
    }

    #[test]
    fn stream_object_on_step_start_provides_step_number_and_model_info() {
        let model = MockLanguageModel::new()
            .with_provider("test-provider")
            .with_model_id("test-model")
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));
        let step_start_event = Arc::new(Mutex::new(None::<GenerateObjectStepStartEvent>));
        let step_start_event_for_callback = Arc::clone(&step_start_event);

        poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_experimental_on_step_start(move |event| {
                    let step_start_event = Arc::clone(&step_start_event_for_callback);
                    async move {
                        *step_start_event.lock().expect("step start event lock") = Some(event);
                    }
                }),
        ));

        let event = step_start_event
            .lock()
            .expect("step start event lock")
            .clone()
            .expect("step start event captured");
        assert_eq!(event.step_number, 0);
        assert_eq!(event.provider, "test-provider");
        assert_eq!(event.model_id, "test-model");
        assert!(event.call_id.starts_with("aiobj-"));
        assert_eq!(event.prompt_messages, prompt());
    }

    #[test]
    fn stream_object_on_step_finish_runs_after_model_call() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let model = RecordingStreamModel::new(Arc::clone(&events));
        let step_finish_events = Arc::clone(&events);

        poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_on_step_finish(move |_| {
                    let step_finish_events = Arc::clone(&step_finish_events);
                    async move {
                        step_finish_events
                            .lock()
                            .expect("recording events lock")
                            .push("onStepFinish".to_string());
                    }
                }),
        ));

        assert_eq!(
            *events.lock().expect("recording events lock"),
            vec!["doStream".to_string(), "onStepFinish".to_string()]
        );
    }

    #[test]
    fn stream_object_on_step_finish_provides_raw_object_text_and_usage() {
        let model = MockLanguageModel::new()
            .with_provider("test-provider")
            .with_model_id("test-model")
            .with_stream_result(LanguageModelStreamResult::new(object_stream()));
        let step_finish_event = Arc::new(Mutex::new(None::<GenerateObjectStepEndEvent>));
        let step_finish_event_for_callback = Arc::clone(&step_finish_event);

        poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_on_step_finish(move |event| {
                    let step_finish_event = Arc::clone(&step_finish_event_for_callback);
                    async move {
                        *step_finish_event.lock().expect("step finish event lock") = Some(event);
                    }
                }),
        ));

        let event = step_finish_event
            .lock()
            .expect("step finish event lock")
            .clone()
            .expect("step finish event captured");
        assert_eq!(event.step_number, 0);
        assert_eq!(event.provider, "test-provider");
        assert_eq!(event.model_id, "test-model");
        assert_eq!(event.object_text, r#"{ "content": "Hello, world!" }"#);
        assert_eq!(event.finish_reason, FinishReason::Stop);
        assert_eq!(event.usage, usage());
        assert!(event.call_id.starts_with("aiobj-"));
    }

    #[test]
    fn stream_object_callbacks_fire_in_upstream_order_with_model_call() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let model = RecordingStreamModel::new(Arc::clone(&events));

        let on_start_events = Arc::clone(&events);
        let on_step_start_events = Arc::clone(&events);
        let on_step_finish_events = Arc::clone(&events);
        let on_finish_events = Arc::clone(&events);

        poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_experimental_on_start(move |_| {
                    let events = Arc::clone(&on_start_events);
                    async move {
                        events
                            .lock()
                            .expect("recording events lock")
                            .push("onStart".to_string());
                    }
                })
                .with_experimental_on_step_start(move |_| {
                    let events = Arc::clone(&on_step_start_events);
                    async move {
                        events
                            .lock()
                            .expect("recording events lock")
                            .push("onStepStart".to_string());
                    }
                })
                .with_on_step_finish(move |_| {
                    let events = Arc::clone(&on_step_finish_events);
                    async move {
                        events
                            .lock()
                            .expect("recording events lock")
                            .push("onStepFinish".to_string());
                    }
                })
                .with_on_finish(move |_| {
                    let events = Arc::clone(&on_finish_events);
                    async move {
                        events
                            .lock()
                            .expect("recording events lock")
                            .push("onFinish".to_string());
                    }
                }),
        ));

        assert_eq!(
            *events.lock().expect("recording events lock"),
            vec![
                "onStart".to_string(),
                "onStepStart".to_string(),
                "doStream".to_string(),
                "onStepFinish".to_string(),
                "onFinish".to_string(),
            ]
        );
    }

    #[test]
    fn stream_object_callbacks_correlate_all_events_with_same_call_id() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#"{ "content": "Hello, world!" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let call_ids = Arc::new(Mutex::new(Vec::<String>::new()));
        let start_call_ids = Arc::clone(&call_ids);
        let step_start_call_ids = Arc::clone(&call_ids);
        let step_finish_call_ids = Arc::clone(&call_ids);
        let finish_call_ids = Arc::clone(&call_ids);

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_experimental_on_start(move |event| {
                    let call_ids = Arc::clone(&start_call_ids);
                    async move {
                        call_ids
                            .lock()
                            .expect("callback call ids lock")
                            .push(event.call_id);
                    }
                })
                .with_experimental_on_step_start(move |event| {
                    let call_ids = Arc::clone(&step_start_call_ids);
                    async move {
                        call_ids
                            .lock()
                            .expect("callback call ids lock")
                            .push(event.call_id);
                    }
                })
                .with_on_step_finish(move |event| {
                    let call_ids = Arc::clone(&step_finish_call_ids);
                    async move {
                        call_ids
                            .lock()
                            .expect("callback call ids lock")
                            .push(event.call_id);
                    }
                })
                .with_on_finish(move |event| {
                    let call_ids = Arc::clone(&finish_call_ids);
                    async move {
                        call_ids
                            .lock()
                            .expect("callback call ids lock")
                            .push(event.call_id);
                    }
                }),
        ));

        assert_eq!(result.object, Some(json!({ "content": "Hello, world!" })));
        let call_ids = call_ids.lock().expect("callback call ids lock");
        assert_eq!(call_ids.len(), 4);
        assert!(call_ids[0].starts_with("aiobj-"));
        assert!(call_ids.iter().all(|call_id| call_id == &call_ids[0]));
    }

    fn panicking_stream_object_callback<T>(_event: T) -> Ready<()> {
        panic!("callback error")
    }

    #[test]
    fn stream_object_callback_panics_do_not_break_stream() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#"{ "content": "Hello, world!" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_experimental_on_start(panicking_stream_object_callback)
                .with_experimental_on_step_start(panicking_stream_object_callback)
                .with_on_step_finish(panicking_stream_object_callback)
                .with_on_finish(panicking_stream_object_callback),
        ));

        assert_eq!(result.object, Some(json!({ "content": "Hello, world!" })));
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.error, None);
        assert!(!result.partial_object_stream.is_empty());
    }

    #[test]
    fn stream_object_invokes_lifecycle_callbacks_with_streamed_step() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-callback")
                        .with_model_id("mock-model-id"),
                ),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#"{ "content": "Hello, world!" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));

        let callback_events = Arc::new(Mutex::new(Vec::new()));
        let callback_call_ids = Arc::new(Mutex::new(Vec::new()));
        let step_texts = Arc::new(Mutex::new(Vec::new()));
        let finish_objects = Arc::new(Mutex::new(Vec::new()));

        let start_events = Arc::clone(&callback_events);
        let step_start_events = Arc::clone(&callback_events);
        let step_finish_events = Arc::clone(&callback_events);
        let finish_events = Arc::clone(&callback_events);

        let start_call_ids = Arc::clone(&callback_call_ids);
        let step_start_call_ids = Arc::clone(&callback_call_ids);
        let step_finish_call_ids = Arc::clone(&callback_call_ids);
        let finish_call_ids = Arc::clone(&callback_call_ids);

        let step_texts_for_callback = Arc::clone(&step_texts);
        let finish_objects_for_callback = Arc::clone(&finish_objects);

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_schema_name("answer")
                .with_experimental_on_start(move |event| {
                    let start_events = Arc::clone(&start_events);
                    let start_call_ids = Arc::clone(&start_call_ids);
                    async move {
                        assert_eq!(event.operation_id, "ai.streamObject");
                        assert_eq!(event.output, GenerateObjectOutputKind::Object);
                        assert_eq!(event.schema_name.as_deref(), Some("answer"));
                        start_events
                            .lock()
                            .expect("callback events lock")
                            .push("start".to_string());
                        start_call_ids
                            .lock()
                            .expect("callback call ids lock")
                            .push(event.call_id);
                    }
                })
                .with_experimental_on_step_start(move |event| {
                    let step_start_events = Arc::clone(&step_start_events);
                    let step_start_call_ids = Arc::clone(&step_start_call_ids);
                    async move {
                        assert_eq!(event.step_number, 0);
                        assert_eq!(event.prompt_messages, prompt());
                        step_start_events
                            .lock()
                            .expect("callback events lock")
                            .push("step-start".to_string());
                        step_start_call_ids
                            .lock()
                            .expect("callback call ids lock")
                            .push(event.call_id);
                    }
                })
                .with_on_step_finish(move |event| {
                    let step_finish_events = Arc::clone(&step_finish_events);
                    let step_finish_call_ids = Arc::clone(&step_finish_call_ids);
                    let step_texts = Arc::clone(&step_texts_for_callback);
                    async move {
                        assert_eq!(event.step_number, 0);
                        assert_eq!(event.finish_reason, FinishReason::Stop);
                        assert_eq!(event.usage, usage());
                        assert_eq!(event.response.id.as_deref(), Some("id-callback"));
                        step_texts
                            .lock()
                            .expect("step texts lock")
                            .push(event.object_text);
                        step_finish_events
                            .lock()
                            .expect("callback events lock")
                            .push("step-finish".to_string());
                        step_finish_call_ids
                            .lock()
                            .expect("callback call ids lock")
                            .push(event.call_id);
                    }
                })
                .with_on_finish(move |event| {
                    let finish_events = Arc::clone(&finish_events);
                    let finish_call_ids = Arc::clone(&finish_call_ids);
                    let finish_objects = Arc::clone(&finish_objects_for_callback);
                    async move {
                        assert_eq!(event.finish_reason, FinishReason::Stop);
                        assert_eq!(event.error, None);
                        finish_objects
                            .lock()
                            .expect("finish objects lock")
                            .push(event.object);
                        finish_events
                            .lock()
                            .expect("callback events lock")
                            .push("finish".to_string());
                        finish_call_ids
                            .lock()
                            .expect("callback call ids lock")
                            .push(event.call_id);
                    }
                }),
        ));

        assert_eq!(result.object, Some(json!({ "content": "Hello, world!" })));
        let callback_events = callback_events
            .lock()
            .expect("callback events lock")
            .clone();
        assert_eq!(
            callback_events,
            vec!["start", "step-start", "step-finish", "finish"]
        );
        let step_texts = step_texts.lock().expect("step texts lock").clone();
        assert_eq!(
            step_texts,
            vec![r#"{ "content": "Hello, world!" }"#.to_string()]
        );
        let finish_objects = finish_objects.lock().expect("finish objects lock").clone();
        assert_eq!(
            finish_objects,
            vec![Some(json!({ "content": "Hello, world!" }))]
        );

        let call_ids = callback_call_ids.lock().expect("callback call ids lock");
        assert_eq!(call_ids.len(), 4);
        assert!(call_ids[0].starts_with("aiobj-"));
        assert!(call_ids.iter().all(|call_id| call_id == &call_ids[0]));
    }

    #[test]
    fn stream_object_on_finish_is_called_when_valid_object_is_generated() {
        let mut provider_metadata = ProviderMetadata::new();
        let mut test_provider_metadata = serde_json::Map::new();
        test_provider_metadata.insert("testKey".to_string(), json!("testValue"));
        provider_metadata.insert("testProvider".to_string(), test_provider_metadata);
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-0")
                        .with_model_id("mock-model-id")
                        .with_timestamp(time::OffsetDateTime::UNIX_EPOCH),
                ),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#"{ "content": "Hello, world!" }"#,
                )),
                LanguageModelStreamPart::Finish(
                    LanguageModelStreamFinish::new(usage(), finish_reason())
                        .with_provider_metadata(provider_metadata.clone()),
                ),
            ]));
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateObjectEndEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_on_finish(move |event| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events
                            .lock()
                            .expect("finish events lock")
                            .push(event);
                    }
                }),
        ));

        assert_eq!(result.object, Some(json!({ "content": "Hello, world!" })));
        let events = finish_events.lock().expect("finish events lock");
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.call_id.starts_with("aiobj-"));
        assert_eq!(event.object, Some(json!({ "content": "Hello, world!" })));
        assert_eq!(event.error, None);
        assert_eq!(event.finish_reason, FinishReason::Stop);
        assert_eq!(event.usage, usage());
        assert_eq!(event.response.id.as_deref(), Some("id-0"));
        assert_eq!(event.response.model_id.as_deref(), Some("mock-model-id"));
        assert_eq!(
            event.response.timestamp,
            Some(time::OffsetDateTime::UNIX_EPOCH)
        );
        assert_eq!(event.provider_metadata, Some(provider_metadata));
    }

    #[test]
    fn stream_object_on_finish_is_called_when_object_does_not_match_schema() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-0")
                        .with_model_id("mock-model-id")
                        .with_timestamp(time::OffsetDateTime::UNIX_EPOCH),
                ),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "{ ")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#""invalid": "#,
                )),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#""Hello, "#)),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "world")),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", r#"!""#)),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", " }")),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let finish_events = Arc::new(Mutex::new(Vec::<GenerateObjectEndEvent>::new()));
        let finish_events_for_callback = Arc::clone(&finish_events);

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_on_finish(move |event| {
                    let finish_events = Arc::clone(&finish_events_for_callback);
                    async move {
                        finish_events
                            .lock()
                            .expect("finish events lock")
                            .push(event);
                    }
                }),
        ));

        assert_eq!(result.object, None);
        assert!(result.error.is_some());
        let events = finish_events.lock().expect("finish events lock");
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.call_id.starts_with("aiobj-"));
        assert_eq!(event.object, None);
        assert!(event.error.is_some());
        assert_eq!(event.finish_reason, FinishReason::Stop);
        assert_eq!(event.usage, usage());
        assert_eq!(event.response.id.as_deref(), Some("id-0"));
        assert_eq!(event.response.model_id.as_deref(), Some("mock-model-id"));
        assert_eq!(
            event.response.timestamp,
            Some(time::OffsetDateTime::UNIX_EPOCH)
        );
        assert_eq!(event.provider_metadata, None);
    }

    #[test]
    fn stream_object_dispatches_telemetry_lifecycle_events() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::ResponseMetadata(
                    LanguageModelStreamResponseMetadata::new()
                        .with_id("id-telemetry")
                        .with_model_id("mock-model-id"),
                ),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#"{ "content": "Hello, world!" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let events = Arc::new(Mutex::new(Vec::<TelemetryEvent>::new()));
        let mut integration = TelemetryIntegration::new();
        for kind in [
            TelemetryEventKind::OnStart,
            TelemetryEventKind::OnObjectStepStart,
            TelemetryEventKind::OnObjectStepFinish,
            TelemetryEventKind::OnEnd,
        ] {
            let captured = Arc::clone(&events);
            integration = integration.with_callback(kind, move |event| {
                captured.lock().expect("telemetry event lock").push(event);
            });
        }

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_telemetry(
                    TelemetryOptions::new()
                        .with_function_id("stream-object-test")
                        .with_record_inputs(false)
                        .with_record_outputs(true)
                        .with_integration(integration),
                ),
        ));

        assert_eq!(result.object, Some(json!({ "content": "Hello, world!" })));
        let events = events.lock().expect("telemetry event lock");
        assert_eq!(
            events.iter().map(|event| event.kind).collect::<Vec<_>>(),
            vec![
                TelemetryEventKind::OnStart,
                TelemetryEventKind::OnObjectStepStart,
                TelemetryEventKind::OnObjectStepFinish,
                TelemetryEventKind::OnEnd,
            ]
        );
        assert!(
            events
                .iter()
                .all(|event| event.function_id.as_deref() == Some("stream-object-test"))
        );
        assert!(
            events
                .iter()
                .all(|event| event.record_inputs == Some(false))
        );
        assert!(
            events
                .iter()
                .all(|event| event.record_outputs == Some(true))
        );
        assert_eq!(events[0].event["operationId"], json!("ai.streamObject"));
        assert_eq!(events[0].event["provider"], json!("mock-provider"));
        assert_eq!(
            events[2].event["objectText"],
            json!(r#"{ "content": "Hello, world!" }"#)
        );
        assert_eq!(
            events[3].event["object"],
            json!({ "content": "Hello, world!" })
        );
    }

    #[test]
    fn stream_object_accepts_experimental_telemetry_alias() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#"{ "content": "Hello, world!" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let start_events = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let telemetry_events = Arc::new(Mutex::new(Vec::<TelemetryEvent>::new()));
        let start_events_for_callback = Arc::clone(&start_events);
        let telemetry_events_for_callback = Arc::clone(&telemetry_events);
        let integration =
            TelemetryIntegration::new().with_callback(TelemetryEventKind::OnStart, move |event| {
                telemetry_events_for_callback
                    .lock()
                    .expect("telemetry event lock")
                    .push(event);
            });

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_experimental_telemetry(
                    TelemetryOptions::new()
                        .with_enabled(true)
                        .with_function_id("deprecated-fn")
                        .with_integration(integration),
                )
                .with_experimental_on_start(move |event| {
                    start_events_for_callback
                        .lock()
                        .expect("start event lock")
                        .push(serde_json::to_value(event).expect("event serializes"));
                    ready(())
                }),
        ));

        assert_eq!(result.object, Some(json!({ "content": "Hello, world!" })));
        let start_events = start_events.lock().expect("start event lock");
        assert_eq!(start_events.len(), 1);
        assert!(start_events[0].get("isEnabled").is_none());
        assert!(start_events[0].get("functionId").is_none());
        let telemetry_events = telemetry_events.lock().expect("telemetry event lock");
        assert_eq!(telemetry_events.len(), 1);
        assert_eq!(
            telemetry_events[0].function_id.as_deref(),
            Some("deprecated-fn")
        );
    }

    #[test]
    fn stream_object_step_finish_reports_ms_to_first_chunk() {
        let model =
            MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(
                    "1",
                    r#"{ "content": "Hello, world!" }"#,
                )),
                LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                    usage(),
                    finish_reason(),
                )),
            ]));
        let chunk_timings = Arc::new(Mutex::new(Vec::new()));
        let chunk_timings_for_callback = Arc::clone(&chunk_timings);

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_on_step_finish(move |event| {
                    let chunk_timings = Arc::clone(&chunk_timings_for_callback);
                    async move {
                        chunk_timings
                            .lock()
                            .expect("chunk timings lock")
                            .push(event.ms_to_first_chunk);
                    }
                }),
        ));

        assert_eq!(result.object, Some(json!({ "content": "Hello, world!" })));

        let chunk_timings = chunk_timings.lock().expect("chunk timings lock");
        assert_eq!(chunk_timings.len(), 1);
        assert!(chunk_timings[0].is_some());
    }

    #[test]
    fn stream_object_aborts_before_model_call_and_suppresses_finish() {
        let model = MockLanguageModel::new();
        let abort_controller = StreamObjectAbortController::new();
        abort_controller.abort_with_reason("manual abort");

        let callback_events = Arc::new(Mutex::new(Vec::new()));
        let error_events = Arc::new(Mutex::new(Vec::new()));

        let on_error_events = Arc::clone(&callback_events);
        let on_error_payloads = Arc::clone(&error_events);
        let step_finish_events = Arc::clone(&callback_events);
        let finish_events = Arc::clone(&callback_events);

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_abort_signal(abort_controller.signal())
                .with_on_error(move |event| {
                    let on_error_events = Arc::clone(&on_error_events);
                    let on_error_payloads = Arc::clone(&on_error_payloads);
                    async move {
                        on_error_events
                            .lock()
                            .expect("callback events lock")
                            .push("error".to_string());
                        on_error_payloads
                            .lock()
                            .expect("error events lock")
                            .push(event.error);
                    }
                })
                .with_on_step_finish(move |_| {
                    let step_finish_events = Arc::clone(&step_finish_events);
                    async move {
                        step_finish_events
                            .lock()
                            .expect("callback events lock")
                            .push("step-finish".to_string());
                    }
                })
                .with_on_finish(move |_| {
                    let finish_events = Arc::clone(&finish_events);
                    async move {
                        finish_events
                            .lock()
                            .expect("callback events lock")
                            .push("finish".to_string());
                    }
                }),
        ));

        let abort_error = json!({
            "name": "AbortError",
            "message": "The streamObject request was aborted.",
            "reason": "manual abort"
        });

        assert!(model.stream_calls().is_empty());
        assert_eq!(result.finish_reason, FinishReason::Error);
        assert_eq!(result.object, None);
        assert_eq!(result.text, "");
        assert_eq!(result.error, Some(abort_error.clone()));
        assert_eq!(
            result.parts,
            vec![ObjectStreamPart::Error {
                error: abort_error.clone()
            }]
        );
        assert_eq!(
            *callback_events.lock().expect("callback events lock"),
            vec!["error".to_string()]
        );
        assert_eq!(
            *error_events.lock().expect("error events lock"),
            vec![abort_error]
        );
    }

    #[test]
    fn stream_object_aborts_after_model_call_and_suppresses_finish() {
        let abort_controller = StreamObjectAbortController::new();
        let model = AbortingStreamModel::new(abort_controller.clone());

        let callback_events = Arc::new(Mutex::new(Vec::new()));
        let on_error_events = Arc::clone(&callback_events);
        let step_finish_events = Arc::clone(&callback_events);
        let finish_events = Arc::clone(&callback_events);

        let result = poll_ready(stream_object(
            StreamObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_abort_signal(abort_controller.signal())
                .with_on_error(move |_| {
                    let on_error_events = Arc::clone(&on_error_events);
                    async move {
                        on_error_events
                            .lock()
                            .expect("callback events lock")
                            .push("error".to_string());
                    }
                })
                .with_on_step_finish(move |_| {
                    let step_finish_events = Arc::clone(&step_finish_events);
                    async move {
                        step_finish_events
                            .lock()
                            .expect("callback events lock")
                            .push("step-finish".to_string());
                    }
                })
                .with_on_finish(move |_| {
                    let finish_events = Arc::clone(&finish_events);
                    async move {
                        finish_events
                            .lock()
                            .expect("callback events lock")
                            .push("finish".to_string());
                    }
                }),
        ));

        let abort_error = json!({
            "name": "AbortError",
            "message": "The streamObject request was aborted.",
            "reason": "client-disconnected"
        });

        let stream_calls = model.stream_calls();
        assert_eq!(stream_calls.len(), 1);
        let provider_abort_signal = stream_calls[0]
            .abort_signal
            .as_ref()
            .expect("abort signal should propagate to provider call options");
        assert!(provider_abort_signal.is_aborted());
        assert_eq!(
            provider_abort_signal.reason(),
            Some(json!("client-disconnected"))
        );
        assert_eq!(result.finish_reason, FinishReason::Error);
        assert_eq!(result.object, None);
        assert_eq!(result.partial_object_stream, Vec::<JsonValue>::new());
        assert_eq!(result.text_stream, Vec::<String>::new());
        assert_eq!(result.error, Some(abort_error.clone()));
        assert_eq!(
            result.parts,
            vec![ObjectStreamPart::Error { error: abort_error }]
        );
        assert_eq!(
            *callback_events.lock().expect("callback events lock"),
            vec!["error".to_string()]
        );
    }
}
