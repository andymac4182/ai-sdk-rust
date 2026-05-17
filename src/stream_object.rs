use serde::{Deserialize, Serialize};

use crate::VERSION;
use crate::generate_object::{
    GenerateObjectOutputKind, generate_object_output_kind, generate_object_response_format,
    parse_generated_object,
};
use crate::headers::Headers;
use crate::json::JsonValue;
use crate::language_model::{
    FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelPrompt,
    LanguageModelRequest, LanguageModelStreamPart, LanguageModelUsage,
};
use crate::prompt::{Prompt, standardize_prompt};
use crate::provider::{InvalidPromptError, ProviderMetadata, ProviderOptions};
use crate::provider_utils::{FlexibleSchema, ParseJsonResult, with_user_agent_suffix};
use crate::stream_text::StreamTextResponseMetadata;
use crate::util::{is_deep_equal_data, parse_partial_json};
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
    } = options;

    let output_kind = generate_object_output_kind(&schema, enum_values.as_deref(), array_output);
    call_options.response_format = Some(generate_object_response_format(
        &schema,
        &schema_name,
        &schema_description,
        enum_values.as_deref(),
        array_output,
    ));
    call_options.include_raw_chunks = Some(false);
    append_stream_object_user_agent(&mut call_options);

    let stream_result = model.do_stream(call_options).await;
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
    let mut warnings = Vec::new();
    let mut usage = LanguageModelUsage::default();
    let mut finish_reason = FinishReason::Other;
    let mut provider_metadata = None;
    let mut error = None;

    for part in stream_result.stream {
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

                if let Some(partial) =
                    next_partial_object(&text, output_kind, latest_partial.as_ref())
                {
                    latest_partial = Some(partial.clone());
                    partial_object_stream.push(partial.clone());
                    parts.push(ObjectStreamPart::Object { object: partial });
                    flush_pending_text_delta(&mut pending_text_delta, &mut text_stream, &mut parts);
                }
            }
            LanguageModelStreamPart::Finish(part) => {
                usage = part.usage;
                finish_reason = part.finish_reason.unified;
                provider_metadata = part.provider_metadata;
            }
            LanguageModelStreamPart::Error(part) => {
                finish_reason = FinishReason::Error;
                error = Some(part.error.clone());
                parts.push(ObjectStreamPart::Error { error: part.error });
            }
            _ => {}
        }
    }

    flush_pending_text_delta(&mut pending_text_delta, &mut text_stream, &mut parts);

    let object =
        match parse_generated_object(&text, schema.clone(), enum_values.as_deref(), array_output) {
            ParseJsonResult::Success { value, .. } => {
                if output_kind == GenerateObjectOutputKind::Array {
                    element_stream = value.as_array().cloned().unwrap_or_default();
                }
                Some(value)
            }
            ParseJsonResult::Failure { error: cause, .. } => {
                if error.is_none() {
                    error = Some(JsonValue::String(cause.to_string()));
                }
                None
            }
        };

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

fn next_partial_object(
    text: &str,
    output_kind: GenerateObjectOutputKind,
    latest_partial: Option<&JsonValue>,
) -> Option<JsonValue> {
    let (value, _) = parse_partial_json(Some(text)).into_parts();
    let value = value?;
    let partial = partial_value_for_output(value, output_kind)?;

    if latest_partial.is_some_and(|latest| is_deep_equal_data(latest, &partial)) {
        None
    } else {
        Some(partial)
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
    text_stream.push(text_delta.clone());
    parts.push(ObjectStreamPart::TextDelta { text_delta });
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

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    use serde_json::json;

    use super::*;
    use crate::json::JsonSchema;
    use crate::language_model::{
        InputTokenUsage, LanguageModelErrorStreamPart, LanguageModelFinishReason,
        LanguageModelMessage, LanguageModelResponseFormat, LanguageModelStreamFinish,
        LanguageModelStreamResponseMetadata, LanguageModelStreamResult, LanguageModelTextDelta,
        LanguageModelTextPart, LanguageModelUserContentPart, LanguageModelUserMessage,
        OutputTokenUsage,
    };
    use crate::mock_models::MockLanguageModel;
    use crate::provider_utils::{Schema, json_schema};

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
}
