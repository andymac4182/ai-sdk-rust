use std::fmt;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::VERSION;
use crate::headers::Headers;
use crate::json::{JsonObject, JsonSchema, JsonValue};
use crate::language_model::{
    FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
    LanguageModelGenerateResult, LanguageModelPrompt, LanguageModelReasoning, LanguageModelRequest,
    LanguageModelResponse, LanguageModelResponseFormat, LanguageModelText, LanguageModelUsage,
};
use crate::logger::{LogWarningsOptions, log_warnings};
use crate::prompt::{
    Prompt, prompt_has_url_files, standardize_and_convert_to_language_model_prompt,
};
use crate::provider::{
    ApiCallError, InvalidPromptError, ProviderMetadata, ProviderOptions, TypeValidationError,
};
use crate::provider_utils::{
    FlexibleSchema, IdGeneratorOptions, ParseJsonError, ParseJsonResult, ValidateTypesResult,
    create_id_generator, generate_id, safe_parse_json, safe_parse_json_with_schema,
    safe_validate_types, with_user_agent_suffix,
};
use crate::retry::DEFAULT_MAX_RETRIES;
use crate::telemetry::{TelemetryOptions, create_telemetry_dispatcher};
use crate::warning::Warning;

pub use crate::generate_text::NoObjectGeneratedError;

const fn default_max_retries() -> usize {
    DEFAULT_MAX_RETRIES
}

/// Request metadata returned by high-level object generation.
///
/// Upstream `GenerateObjectResult.request` omits prompt messages and retains
/// only lower-level request details such as the provider request body.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateObjectRequest {
    /// Request HTTP body that was sent to the provider API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,
}

impl GenerateObjectRequest {
    /// Creates empty generate-object request metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the raw provider request body.
    pub fn with_body(mut self, body: JsonValue) -> Self {
        self.body = Some(body);
        self
    }
}

/// Response metadata returned by high-level object generation.
///
/// Upstream `GenerateObjectResult.response` omits response messages and keeps
/// provider response id, timestamp, model id, headers, and raw body metadata.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateObjectResponse {
    /// Provider response identifier, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Start timestamp for the generated response, when one is available.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    pub timestamp: Option<OffsetDateTime>,

    /// Provider model identifier used for the response, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,

    /// Response headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Provider response body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,
}

impl GenerateObjectResponse {
    /// Creates empty generate-object response metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the provider response identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the response start timestamp.
    pub fn with_timestamp(mut self, timestamp: OffsetDateTime) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Sets the provider model identifier used for the response.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = Some(model_id.into());
        self
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets the raw provider response body.
    pub fn with_body(mut self, body: JsonValue) -> Self {
        self.body = Some(body);
        self
    }
}

/// Result of a high-level `generate_object` call.
///
/// This ports the upstream `GenerateObjectResult` data boundary. The
/// JavaScript-only `toJsonResponse` convenience method is intentionally omitted
/// from this Rust contract until a concrete HTTP response type is introduced.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateObjectResult<T = JsonValue> {
    /// Generated object, typed according to the caller's schema.
    pub object: T,

    /// Reasoning text concatenated from all reasoning parts, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,

    /// Unified reason why generation finished.
    pub finish_reason: FinishReason,

    /// Token usage of the generated response.
    pub usage: LanguageModelUsage,

    /// Warnings from the model provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<Warning>>,

    /// Additional request information.
    pub request: GenerateObjectRequest,

    /// Additional response information.
    pub response: GenerateObjectResponse,

    /// Additional provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl<T> GenerateObjectResult<T> {
    /// Creates a generate-object result with required upstream fields.
    pub fn new(
        object: T,
        finish_reason: FinishReason,
        usage: LanguageModelUsage,
        request: GenerateObjectRequest,
        response: GenerateObjectResponse,
    ) -> Self {
        Self {
            object,
            reasoning: None,
            finish_reason,
            usage,
            warnings: None,
            request,
            response,
            provider_metadata: None,
        }
    }

    /// Sets reasoning text for the generated object.
    pub fn with_reasoning(mut self, reasoning: impl Into<String>) -> Self {
        self.reasoning = Some(reasoning.into());
        self
    }

    /// Adds one model-provider warning.
    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.get_or_insert_with(Vec::new).push(warning);
        self
    }

    /// Sets all model-provider warnings.
    pub fn with_warnings(mut self, warnings: Vec<Warning>) -> Self {
        self.warnings = Some(warnings);
        self
    }

    /// Sets provider-specific result metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

/// Output strategy selected for a high-level `generate_object` call.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GenerateObjectOutputKind {
    /// Object output with optional schema validation.
    Object,

    /// Array output wrapped in upstream's `{ elements: [...] }` provider response shape.
    Array,

    /// Enum output wrapped in upstream's `{ result: <enum> }` provider response shape.
    Enum,

    /// JSON output without a schema.
    NoSchema,
}

/// Event passed to the start callback for `generate_object`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateObjectStartEvent {
    /// Unique identifier for this object generation call.
    pub call_id: String,

    /// Upstream operation identifier.
    pub operation_id: String,

    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model identifier.
    pub model_id: String,

    /// Prompt messages being sent to the language model.
    pub messages: LanguageModelPrompt,

    /// Maximum number of tokens configured for generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,

    /// Sampling temperature configured for generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Nucleus sampling value configured for generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    /// Top-k sampling value configured for generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u64>,

    /// Presence penalty configured for generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,

    /// Frequency penalty configured for generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,

    /// Deterministic sampling seed configured for generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,

    /// Maximum number of retries configured for failed requests.
    #[serde(default = "default_max_retries")]
    pub max_retries: usize,

    /// Additional HTTP headers sent to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Additional provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,

    /// Output strategy used for the call.
    pub output: GenerateObjectOutputKind,

    /// JSON schema sent to the model provider, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<JsonSchema>,

    /// Optional provider-facing schema name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_name: Option<String>,

    /// Optional provider-facing schema description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_description: Option<String>,
}

/// Event passed to the step-start callback for `generate_object`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateObjectStepStartEvent {
    /// Unique identifier for this object generation call.
    pub call_id: String,

    /// Zero-based step index. Non-streaming object generation always has one step.
    pub step_number: usize,

    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model identifier.
    pub model_id: String,

    /// Additional provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,

    /// Additional HTTP headers sent to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Prompt messages sent to the provider.
    pub prompt_messages: LanguageModelPrompt,
}

/// Event passed to the step-finish callback for `generate_object`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateObjectStepEndEvent {
    /// Unique identifier for this object generation call.
    pub call_id: String,

    /// Zero-based step index. Non-streaming object generation always has one step.
    pub step_number: usize,

    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model identifier.
    pub model_id: String,

    /// Unified reason why generation finished.
    pub finish_reason: FinishReason,

    /// Token usage reported by the model.
    pub usage: LanguageModelUsage,

    /// Raw object text before JSON parsing and validation.
    pub object_text: String,

    /// Reasoning text concatenated from reasoning parts, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,

    /// Warnings returned by the model provider.
    pub warnings: Vec<Warning>,

    /// Additional request information.
    pub request: GenerateObjectRequest,

    /// Additional response information.
    pub response: GenerateObjectResponse,

    /// Additional provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Milliseconds to the first stream chunk. Always absent for non-streaming generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms_to_first_chunk: Option<u64>,
}

/// Event passed to the finish callback for `generate_object`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateObjectEndEvent<T = JsonValue> {
    /// Unique identifier for this object generation call.
    pub call_id: String,

    /// Parsed and validated object. Always present for non-streaming `generate_object`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<T>,

    /// Parse or validation error. Always absent for successful non-streaming `generate_object`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Reasoning text concatenated from reasoning parts, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,

    /// Unified reason why generation finished.
    pub finish_reason: FinishReason,

    /// Token usage reported by the model.
    pub usage: LanguageModelUsage,

    /// Warnings returned by the model provider.
    pub warnings: Vec<Warning>,

    /// Additional request information.
    pub request: GenerateObjectRequest,

    /// Additional response information.
    pub response: GenerateObjectResponse,

    /// Additional provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Options passed to a generate-object repair-text callback.
#[derive(Clone, Debug, PartialEq)]
pub struct GenerateObjectRepairTextOptions {
    /// Raw model text that failed parsing or schema validation.
    pub text: String,

    /// Parse or validation error that triggered repair.
    pub error: ParseJsonError,
}

impl GenerateObjectRepairTextOptions {
    /// Creates repair-text callback options.
    pub fn new(text: impl Into<String>, error: ParseJsonError) -> Self {
        Self {
            text: text.into(),
            error,
        }
    }

    /// Returns the raw model text that failed parsing or validation.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the parse or validation error that triggered repair.
    pub fn error(&self) -> &ParseJsonError {
        &self.error
    }

    /// Converts these options into their raw parts.
    pub fn into_parts(self) -> (String, ParseJsonError) {
        (self.text, self.error)
    }
}

/// Future returned by a generate-object repair-text callback.
pub type GenerateObjectRepairTextFuture<'a> = Pin<Box<dyn Future<Output = Option<String>> + 'a>>;

/// Callback that can repair raw model text after parse or validation failure.
pub type GenerateObjectRepairTextFunction<'a> =
    dyn Fn(GenerateObjectRepairTextOptions) -> GenerateObjectRepairTextFuture<'a> + 'a;

/// Upstream callback alias for `experimental_repairText`.
pub type RepairTextFunction<'a> = GenerateObjectRepairTextFunction<'a>;

/// Callback wrapper for upstream generate-object `experimental_repairText`.
pub struct GenerateObjectRepairText<'a> {
    repair_text: Rc<GenerateObjectRepairTextFunction<'a>>,
}

impl<'a> GenerateObjectRepairText<'a> {
    /// Creates a repair-text callback.
    pub fn new<F, Fut>(repair_text: F) -> Self
    where
        F: Fn(GenerateObjectRepairTextOptions) -> Fut + 'a,
        Fut: Future<Output = Option<String>> + 'a,
    {
        Self {
            repair_text: Rc::new(move |options| Box::pin(repair_text(options))),
        }
    }

    /// Runs the repair-text callback.
    pub fn repair(
        &self,
        options: GenerateObjectRepairTextOptions,
    ) -> GenerateObjectRepairTextFuture<'a> {
        (self.repair_text)(options)
    }
}

impl fmt::Debug for GenerateObjectRepairText<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateObjectRepairText")
            .finish_non_exhaustive()
    }
}

/// Future returned by a high-level generate-object start callback.
pub type GenerateObjectOnStartFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked before a high-level generate-object operation calls the model.
pub type GenerateObjectOnStartFunction<'a> =
    dyn Fn(GenerateObjectStartEvent) -> GenerateObjectOnStartFuture<'a> + 'a;

/// Upstream callback alias for [`GenerateObjectOnStartFunction`].
pub type GenerateObjectOnStartCallback<'a> = GenerateObjectOnStartFunction<'a>;

/// Callback wrapper for upstream generate-object `experimental_onStart`.
pub struct GenerateObjectOnStart<'a> {
    on_start: Rc<GenerateObjectOnStartFunction<'a>>,
}

impl<'a> GenerateObjectOnStart<'a> {
    /// Creates a generate-object start callback.
    pub fn new<F, Fut>(on_start: F) -> Self
    where
        F: Fn(GenerateObjectStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_start: Rc::new(move |event| Box::pin(on_start(event))),
        }
    }

    /// Runs the generate-object start callback.
    pub fn start(&self, event: GenerateObjectStartEvent) -> GenerateObjectOnStartFuture<'a> {
        match catch_unwind(AssertUnwindSafe(|| (self.on_start)(event))) {
            Ok(callback) => callback,
            Err(_) => Box::pin(async {}),
        }
    }
}

impl fmt::Debug for GenerateObjectOnStart<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateObjectOnStart")
            .finish_non_exhaustive()
    }
}

/// Future returned by a high-level generate-object step-start callback.
pub type GenerateObjectOnStepStartFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked before the single generate-object model step.
pub type GenerateObjectOnStepStartFunction<'a> =
    dyn Fn(GenerateObjectStepStartEvent) -> GenerateObjectOnStepStartFuture<'a> + 'a;

/// Upstream callback alias for [`GenerateObjectOnStepStartFunction`].
pub type GenerateObjectOnStepStartCallback<'a> = GenerateObjectOnStepStartFunction<'a>;

/// Callback wrapper for upstream generate-object `experimental_onStepStart`.
pub struct GenerateObjectOnStepStart<'a> {
    on_step_start: Rc<GenerateObjectOnStepStartFunction<'a>>,
}

impl<'a> GenerateObjectOnStepStart<'a> {
    /// Creates a generate-object step-start callback.
    pub fn new<F, Fut>(on_step_start: F) -> Self
    where
        F: Fn(GenerateObjectStepStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_step_start: Rc::new(move |event| Box::pin(on_step_start(event))),
        }
    }

    /// Runs the generate-object step-start callback.
    pub fn start(
        &self,
        event: GenerateObjectStepStartEvent,
    ) -> GenerateObjectOnStepStartFuture<'a> {
        match catch_unwind(AssertUnwindSafe(|| (self.on_step_start)(event))) {
            Ok(callback) => callback,
            Err(_) => Box::pin(async {}),
        }
    }
}

impl fmt::Debug for GenerateObjectOnStepStart<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateObjectOnStepStart")
            .finish_non_exhaustive()
    }
}

/// Future returned by a high-level generate-object step-finish callback.
pub type GenerateObjectOnStepFinishFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked after the generate-object model step returns raw text.
pub type GenerateObjectOnStepFinishFunction<'a> =
    dyn Fn(GenerateObjectStepEndEvent) -> GenerateObjectOnStepFinishFuture<'a> + 'a;

/// Upstream callback alias for [`GenerateObjectOnStepFinishFunction`].
pub type GenerateObjectOnStepFinishCallback<'a> = GenerateObjectOnStepFinishFunction<'a>;

/// Callback wrapper for upstream generate-object `onStepFinish`.
pub struct GenerateObjectOnStepFinish<'a> {
    on_step_finish: Rc<GenerateObjectOnStepFinishFunction<'a>>,
}

impl<'a> GenerateObjectOnStepFinish<'a> {
    /// Creates a generate-object step-finish callback.
    pub fn new<F, Fut>(on_step_finish: F) -> Self
    where
        F: Fn(GenerateObjectStepEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_step_finish: Rc::new(move |event| Box::pin(on_step_finish(event))),
        }
    }

    /// Runs the generate-object step-finish callback.
    pub fn finish(
        &self,
        event: GenerateObjectStepEndEvent,
    ) -> GenerateObjectOnStepFinishFuture<'a> {
        match catch_unwind(AssertUnwindSafe(|| (self.on_step_finish)(event))) {
            Ok(callback) => callback,
            Err(_) => Box::pin(async {}),
        }
    }
}

impl fmt::Debug for GenerateObjectOnStepFinish<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateObjectOnStepFinish")
            .finish_non_exhaustive()
    }
}

/// Future returned by a high-level generate-object finish callback.
pub type GenerateObjectOnFinishFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked after `generate_object` parses and validates the final object.
pub type GenerateObjectOnFinishFunction<'a> =
    dyn Fn(GenerateObjectEndEvent) -> GenerateObjectOnFinishFuture<'a> + 'a;

/// Upstream callback alias for [`GenerateObjectOnFinishFunction`].
pub type GenerateObjectOnFinishCallback<'a> = GenerateObjectOnFinishFunction<'a>;

/// Callback wrapper for upstream generate-object `onFinish`.
pub struct GenerateObjectOnFinish<'a> {
    on_finish: Rc<GenerateObjectOnFinishFunction<'a>>,
}

impl<'a> GenerateObjectOnFinish<'a> {
    /// Creates a generate-object finish callback.
    pub fn new<F, Fut>(on_finish: F) -> Self
    where
        F: Fn(GenerateObjectEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_finish: Rc::new(move |event| Box::pin(on_finish(event))),
        }
    }

    /// Runs the generate-object finish callback.
    pub fn finish(&self, event: GenerateObjectEndEvent) -> GenerateObjectOnFinishFuture<'a> {
        match catch_unwind(AssertUnwindSafe(|| (self.on_finish)(event))) {
            Ok(callback) => callback,
            Err(_) => Box::pin(async {}),
        }
    }
}

impl fmt::Debug for GenerateObjectOnFinish<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateObjectOnFinish")
            .finish_non_exhaustive()
    }
}

/// Options for a high-level non-streaming object generation call.
#[derive(Debug)]
pub struct GenerateObjectOptions<'a, M: LanguageModel + ?Sized> {
    /// Language model used for object generation.
    pub model: &'a M,

    /// Provider-level call options sent to the model.
    pub call_options: LanguageModelCallOptions,

    /// Optional schema used for object output validation and provider guidance.
    pub schema: Option<FlexibleSchema>,

    /// Optional schema name sent to providers that support named JSON outputs.
    pub schema_name: Option<String>,

    /// Optional schema description sent to providers that support JSON output descriptions.
    pub schema_description: Option<String>,

    /// Optional callback that can repair invalid model text before the final error is returned.
    pub repair_text: Option<GenerateObjectRepairText<'a>>,

    /// Optional enum values for upstream enum output mode.
    pub enum_values: Option<Vec<String>>,

    /// Whether schema validation should use upstream array output mode.
    pub array_output: bool,

    /// Optional callback invoked before generation starts.
    pub on_start: Option<GenerateObjectOnStart<'a>>,

    /// Optional callback invoked before the single model step starts.
    pub on_step_start: Option<GenerateObjectOnStepStart<'a>>,

    /// Optional callback invoked after the model step returns raw text.
    pub on_step_finish: Option<GenerateObjectOnStepFinish<'a>>,

    /// Optional callback invoked after the final object is parsed and validated.
    pub on_finish: Option<GenerateObjectOnFinish<'a>>,

    /// Optional telemetry dispatcher settings.
    pub telemetry: Option<TelemetryOptions>,

    /// Maximum number of retries for failed provider requests.
    pub max_retries: usize,
}

impl<'a, M: LanguageModel + ?Sized> GenerateObjectOptions<'a, M> {
    /// Creates object generation options for a model and standardized prompt.
    pub fn new(model: &'a M, prompt: LanguageModelPrompt) -> Self {
        Self::from_call_options(model, LanguageModelCallOptions::new(prompt))
    }

    /// Creates object generation options from the high-level upstream prompt shape.
    ///
    /// This standardizes text prompts and instructions before delegating to
    /// the provider-v4 language model prompt boundary.
    pub fn from_prompt(model: &'a M, prompt: Prompt) -> Result<Self, InvalidPromptError> {
        let prompt = standardize_and_convert_to_language_model_prompt(prompt)?;
        Ok(Self::new(model, prompt))
    }

    /// Creates object generation options from already prepared provider call options.
    pub fn from_call_options(model: &'a M, mut call_options: LanguageModelCallOptions) -> Self {
        call_options.response_format = Some(LanguageModelResponseFormat::json());
        Self {
            model,
            call_options,
            schema: None,
            schema_name: None,
            schema_description: None,
            repair_text: None,
            enum_values: None,
            array_output: false,
            on_start: None,
            on_step_start: None,
            on_step_finish: None,
            on_finish: None,
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

    /// Adds provider-specific options.
    pub fn with_provider_options(
        mut self,
        provider_options: crate::provider::ProviderOptions,
    ) -> Self {
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
    ///
    /// Upstream asks the provider for an object shaped as
    /// `{ "elements": [<item>] }` and returns only the generated array to
    /// callers.
    pub fn with_array_schema(mut self, schema: impl Into<FlexibleSchema>) -> Self {
        self.schema = Some(schema.into());
        self.enum_values = None;
        self.array_output = true;
        self
    }

    /// Sets the provider-facing schema name.
    pub fn with_schema_name(mut self, schema_name: impl Into<String>) -> Self {
        self.schema_name = Some(schema_name.into());
        self
    }

    /// Sets the provider-facing schema description.
    pub fn with_schema_description(mut self, schema_description: impl Into<String>) -> Self {
        self.schema_description = Some(schema_description.into());
        self
    }

    /// Uses upstream enum output mode with the allowed string values.
    ///
    /// Upstream asks the provider for an object shaped as `{ "result": <enum> }`
    /// and returns the selected enum string as the final object value. Setting
    /// enum values clears any schema metadata previously configured.
    pub fn with_enum_values<T, I>(mut self, enum_values: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        self.schema = None;
        self.schema_name = None;
        self.schema_description = None;
        self.enum_values = Some(enum_values.into_iter().map(Into::into).collect());
        self.array_output = false;
        self
    }

    /// Sets a callback that can repair generated text after parse or validation failure.
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

    /// Sets a callback that is invoked before object generation starts.
    pub fn with_on_start<F, Fut>(mut self, on_start: F) -> Self
    where
        F: Fn(GenerateObjectStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_start = Some(GenerateObjectOnStart::new(on_start));
        self
    }

    /// Upstream experimental alias for [`GenerateObjectOptions::with_on_start`].
    pub fn with_experimental_on_start<F, Fut>(self, on_start: F) -> Self
    where
        F: Fn(GenerateObjectStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.with_on_start(on_start)
    }

    /// Sets a callback that is invoked before the single model step starts.
    pub fn with_on_step_start<F, Fut>(mut self, on_step_start: F) -> Self
    where
        F: Fn(GenerateObjectStepStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_step_start = Some(GenerateObjectOnStepStart::new(on_step_start));
        self
    }

    /// Upstream experimental alias for [`GenerateObjectOptions::with_on_step_start`].
    pub fn with_experimental_on_step_start<F, Fut>(self, on_step_start: F) -> Self
    where
        F: Fn(GenerateObjectStepStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.with_on_step_start(on_step_start)
    }

    /// Sets a callback that is invoked after the model step returns raw text.
    pub fn with_on_step_finish<F, Fut>(mut self, on_step_finish: F) -> Self
    where
        F: Fn(GenerateObjectStepEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_step_finish = Some(GenerateObjectOnStepFinish::new(on_step_finish));
        self
    }

    /// Sets a callback that is invoked after the final object is parsed and validated.
    pub fn with_on_finish<F, Fut>(mut self, on_finish: F) -> Self
    where
        F: Fn(GenerateObjectEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_finish = Some(GenerateObjectOnFinish::new(on_finish));
        self
    }

    /// Sets telemetry options for this object generation operation.
    pub fn with_telemetry(mut self, telemetry: TelemetryOptions) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    /// Deprecated upstream alias for [`GenerateObjectOptions::with_telemetry`].
    pub fn with_experimental_telemetry(self, telemetry: TelemetryOptions) -> Self {
        self.with_telemetry(telemetry)
    }

    /// Sets the maximum number of retries for failed provider requests.
    pub fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
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
}

/// Generates a JSON value with a language model and parses the text response.
///
/// This Rust-native runtime slice for upstream `generateObject` calls the
/// model with a JSON response format, parses generated text as JSON, and
/// applies optional schema, enum-output, and repair-text handling.
pub async fn generate_object<M>(
    options: GenerateObjectOptions<'_, M>,
) -> Result<GenerateObjectResult, NoObjectGeneratedError>
where
    M: LanguageModel + ?Sized,
{
    let GenerateObjectOptions {
        model,
        mut call_options,
        schema,
        schema_name,
        schema_description,
        repair_text,
        enum_values,
        array_output,
        on_start,
        on_step_start,
        on_step_finish,
        on_finish,
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
    let event_schema = response_format_schema(&response_format);
    call_options.response_format = Some(response_format);
    append_generate_object_user_agent(&mut call_options);
    if prompt_has_url_files(&call_options.prompt) {
        let _ = model.supported_urls().await;
    }
    let call_id = generate_object_call_id();
    let telemetry_dispatcher = create_telemetry_dispatcher(telemetry);

    if on_start.is_some() || telemetry_dispatcher.is_enabled() {
        let start_event = GenerateObjectStartEvent {
            call_id: call_id.clone(),
            operation_id: "ai.generateObject".to_string(),
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

    let generate_result = do_generate_object_with_retries(model, call_options, max_retries).await;
    log_warnings(
        &LogWarningsOptions::new(generate_result.warnings.clone())
            .with_scope(model.provider(), model.model_id()),
    );
    let finish_reason = generate_result.finish_reason.unified;
    let usage = generate_result.usage;
    let request = generate_object_request(generate_result.request);
    let response = generate_object_language_response(generate_result.response, model.model_id());
    let result_response = generate_object_response(&response);

    let Some(text) = extract_object_text(&generate_result.content) else {
        return Err(NoObjectGeneratedError::with_message(
            "No object generated: the model did not return a response.",
            response,
            usage,
            finish_reason,
        ));
    };
    let reasoning = extract_object_reasoning(&generate_result.content);

    if on_step_finish.is_some() || telemetry_dispatcher.is_enabled() {
        let step_finish_event = GenerateObjectStepEndEvent {
            call_id: call_id.clone(),
            step_number: 0,
            provider: model.provider().to_string(),
            model_id: model.model_id().to_string(),
            finish_reason: finish_reason.clone(),
            usage: usage.clone(),
            object_text: text.clone(),
            reasoning: reasoning.clone(),
            warnings: generate_result.warnings.clone(),
            request: request.clone(),
            response: result_response.clone(),
            provider_metadata: generate_result.provider_metadata.clone(),
            ms_to_first_chunk: None,
        };
        if let Some(on_step_finish) = &on_step_finish {
            on_step_finish.finish(step_finish_event.clone()).await;
        }
        telemetry_dispatcher.on_object_step_finish(&step_finish_event);
    }

    let object = parse_generated_object_with_repair(
        text,
        schema,
        enum_values.as_deref(),
        array_output,
        repair_text.as_ref(),
        GenerateObjectParseContext {
            response: &response,
            usage: &usage,
            finish_reason: &finish_reason,
        },
    )
    .await?;

    let mut result =
        GenerateObjectResult::new(object, finish_reason, usage, request, result_response)
            .with_warnings(generate_result.warnings);

    if let Some(reasoning) = reasoning {
        result = result.with_reasoning(reasoning);
    }

    if let Some(provider_metadata) = generate_result.provider_metadata {
        result = result.with_provider_metadata(provider_metadata);
    }

    if on_finish.is_some() || telemetry_dispatcher.is_enabled() {
        let finish_event = GenerateObjectEndEvent {
            call_id,
            object: Some(result.object.clone()),
            error: None,
            reasoning: result.reasoning.clone(),
            finish_reason: result.finish_reason.clone(),
            usage: result.usage.clone(),
            warnings: result.warnings.clone().unwrap_or_default(),
            request: result.request.clone(),
            response: result.response.clone(),
            provider_metadata: result.provider_metadata.clone(),
        };
        if let Some(on_finish) = &on_finish {
            on_finish.finish(finish_event.clone()).await;
        }
        telemetry_dispatcher.on_end(&finish_event);
    }

    Ok(result)
}

async fn do_generate_object_with_retries<M>(
    model: &M,
    call_options: LanguageModelCallOptions,
    max_retries: usize,
) -> LanguageModelGenerateResult
where
    M: LanguageModel + ?Sized,
{
    let mut retries = 0;

    loop {
        let result = model.do_generate(call_options.clone()).await;

        if retries < max_retries && generate_object_result_is_retryable_pre_content_failure(&result)
        {
            retries += 1;
            continue;
        }

        return result;
    }
}

fn generate_object_result_is_retryable_pre_content_failure(
    result: &LanguageModelGenerateResult,
) -> bool {
    result.finish_reason.unified == FinishReason::Error
        && result.content.is_empty()
        && result
            .provider_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.values().any(provider_metadata_is_retryable))
}

fn provider_metadata_is_retryable(metadata: &JsonObject) -> bool {
    metadata
        .get("isRetryable")
        .or_else(|| metadata.get("is_retryable"))
        .and_then(JsonValue::as_bool)
        .unwrap_or_else(|| {
            metadata
                .get("statusCode")
                .or_else(|| metadata.get("status_code"))
                .and_then(JsonValue::as_u64)
                .and_then(|status_code| u16::try_from(status_code).ok())
                .is_some_and(ApiCallError::is_retryable_status_code)
        })
}

pub(crate) fn generate_object_output_kind(
    schema: &Option<FlexibleSchema>,
    enum_values: Option<&[String]>,
    array_output: bool,
) -> GenerateObjectOutputKind {
    if enum_values.is_some() {
        GenerateObjectOutputKind::Enum
    } else if array_output {
        GenerateObjectOutputKind::Array
    } else if schema.is_some() {
        GenerateObjectOutputKind::Object
    } else {
        GenerateObjectOutputKind::NoSchema
    }
}

fn response_format_schema(response_format: &LanguageModelResponseFormat) -> Option<JsonSchema> {
    match response_format {
        LanguageModelResponseFormat::Json { schema, .. } => schema.clone(),
        LanguageModelResponseFormat::Text => None,
    }
}

pub(crate) fn generate_object_response_format(
    schema: &Option<FlexibleSchema>,
    schema_name: &Option<String>,
    schema_description: &Option<String>,
    enum_values: Option<&[String]>,
    array_output: bool,
) -> LanguageModelResponseFormat {
    let mut response_format = LanguageModelResponseFormat::json();

    if let Some(enum_values) = enum_values {
        response_format = response_format.with_schema(enum_json_schema(enum_values));
    } else if let Some(schema) = schema {
        let json_schema = if array_output {
            array_json_schema(schema)
        } else {
            schema.as_schema().json_schema().clone()
        };

        response_format = response_format.with_schema(json_schema);

        if let Some(schema_name) = schema_name {
            response_format = response_format.with_name(schema_name.clone());
        }

        if let Some(schema_description) = schema_description {
            response_format = response_format.with_description(schema_description.clone());
        }
    }

    response_format
}

fn array_json_schema(schema: &FlexibleSchema) -> JsonSchema {
    let mut item_schema = schema.as_schema().json_schema().clone();
    item_schema.remove("$schema");

    serde_json::from_value(serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "elements": {
                "type": "array",
                "items": item_schema
            }
        },
        "required": ["elements"],
        "additionalProperties": false
    }))
    .expect("array output schema is a JSON object")
}

fn enum_json_schema(enum_values: &[String]) -> JsonSchema {
    serde_json::from_value(serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "result": {
                "type": "string",
                "enum": enum_values
            }
        },
        "required": ["result"],
        "additionalProperties": false
    }))
    .expect("enum output schema is a JSON object")
}

fn append_generate_object_user_agent(call_options: &mut LanguageModelCallOptions) {
    let headers = call_options.headers.take().map(|headers| {
        headers
            .into_iter()
            .map(|(name, value)| (name, Some(value)))
            .collect::<Vec<_>>()
    });

    call_options.headers = Some(with_user_agent_suffix(headers, [format!("ai/{VERSION}")]));
}

pub(crate) fn parse_generated_object(
    text: &str,
    schema: Option<FlexibleSchema>,
    enum_values: Option<&[String]>,
    array_output: bool,
) -> ParseJsonResult<JsonValue> {
    let parse_result = match schema {
        Some(schema) if array_output => match safe_parse_json(text) {
            ParseJsonResult::Success {
                value, raw_value, ..
            } => validate_array_generated_object(value, raw_value, schema),
            ParseJsonResult::Failure { error, raw_value } => {
                ParseJsonResult::Failure { error, raw_value }
            }
        },
        Some(schema) => safe_parse_json_with_schema(text, schema),
        None => safe_parse_json(text),
    };

    match (parse_result, enum_values) {
        (
            ParseJsonResult::Success {
                value, raw_value, ..
            },
            Some(enum_values),
        ) => validate_enum_generated_object(value, raw_value, enum_values),
        (parse_result, _) => parse_result,
    }
}

fn validate_array_generated_object(
    value: JsonValue,
    raw_value: JsonValue,
    schema: FlexibleSchema,
) -> ParseJsonResult<JsonValue> {
    let elements = value
        .as_object()
        .and_then(|object| object.get("elements"))
        .and_then(JsonValue::as_array);

    let Some(elements) = elements else {
        return array_validation_failure(
            value,
            raw_value,
            "value must be an object that contains an array of elements",
        );
    };

    let mut validated_elements = Vec::with_capacity(elements.len());

    for element in elements {
        match safe_validate_types(element.clone(), schema.clone(), None) {
            ValidateTypesResult::Success { value, .. } => validated_elements.push(value),
            ValidateTypesResult::Failure { error, .. } => {
                return ParseJsonResult::failure(error, Some(raw_value));
            }
        }
    }

    ParseJsonResult::success(JsonValue::Array(validated_elements), raw_value)
}

fn array_validation_failure(
    value: JsonValue,
    raw_value: JsonValue,
    cause_message: &'static str,
) -> ParseJsonResult<JsonValue> {
    ParseJsonResult::failure(
        TypeValidationError::with_cause_message(value, cause_message, None),
        Some(raw_value),
    )
}

fn validate_enum_generated_object(
    value: JsonValue,
    raw_value: JsonValue,
    enum_values: &[String],
) -> ParseJsonResult<JsonValue> {
    let result = value
        .as_object()
        .and_then(|object| object.get("result"))
        .and_then(JsonValue::as_str)
        .map(ToOwned::to_owned);

    let Some(result) = result else {
        return enum_validation_failure(
            value,
            raw_value,
            "value must be an object that contains a string in the \"result\" property.",
        );
    };

    if enum_values.iter().any(|value| value == &result) {
        ParseJsonResult::success(JsonValue::String(result), raw_value)
    } else {
        enum_validation_failure(value, raw_value, "value must be a string in the enum")
    }
}

fn enum_validation_failure(
    value: JsonValue,
    raw_value: JsonValue,
    cause_message: &'static str,
) -> ParseJsonResult<JsonValue> {
    ParseJsonResult::failure(
        TypeValidationError::with_cause_message(value, cause_message, None),
        Some(raw_value),
    )
}

async fn parse_generated_object_with_repair(
    text: String,
    schema: Option<FlexibleSchema>,
    enum_values: Option<&[String]>,
    array_output: bool,
    repair_text: Option<&GenerateObjectRepairText<'_>>,
    context: GenerateObjectParseContext<'_>,
) -> Result<JsonValue, NoObjectGeneratedError> {
    match parse_generated_object(&text, schema.clone(), enum_values, array_output) {
        ParseJsonResult::Success { value, .. } => Ok(value),
        ParseJsonResult::Failure { error, .. } => {
            let Some(repair_text) = repair_text else {
                return Err(parse_failure_error(
                    text,
                    error,
                    context.response,
                    context.usage,
                    context.finish_reason,
                ));
            };

            let original_error = error.clone();
            let Some(repaired_text) = repair_text
                .repair(GenerateObjectRepairTextOptions::new(text.clone(), error))
                .await
            else {
                return Err(parse_failure_error(
                    text,
                    original_error,
                    context.response,
                    context.usage,
                    context.finish_reason,
                ));
            };

            match parse_generated_object(&repaired_text, schema, enum_values, array_output) {
                ParseJsonResult::Success { value, .. } => Ok(value),
                ParseJsonResult::Failure { error, .. } => Err(parse_failure_error(
                    repaired_text,
                    error,
                    context.response,
                    context.usage,
                    context.finish_reason,
                )),
            }
        }
    }
}

struct GenerateObjectParseContext<'a> {
    response: &'a LanguageModelResponse,
    usage: &'a LanguageModelUsage,
    finish_reason: &'a FinishReason,
}

fn parse_failure_error(
    text: String,
    error: ParseJsonError,
    response: &LanguageModelResponse,
    usage: &LanguageModelUsage,
    finish_reason: &FinishReason,
) -> NoObjectGeneratedError {
    let message = if error.as_type_validation_error().is_some() {
        "No object generated: response did not match schema."
    } else {
        "No object generated: could not parse the response."
    };

    NoObjectGeneratedError::with_message(
        message,
        response.clone(),
        usage.clone(),
        finish_reason.clone(),
    )
    .with_text(text)
    .with_cause(error)
}

fn generate_object_request(request: Option<LanguageModelRequest>) -> GenerateObjectRequest {
    GenerateObjectRequest {
        body: request.and_then(|request| request.body),
    }
}

fn generate_object_language_response(
    response: Option<LanguageModelResponse>,
    model_id: &str,
) -> LanguageModelResponse {
    let mut response = response.unwrap_or_default();

    if response.id.is_none() {
        response.id = Some(generate_id());
    }

    if response.timestamp.is_none() {
        response.timestamp = Some(OffsetDateTime::now_utc());
    }

    if response.model_id.is_none() {
        response.model_id = Some(model_id.to_string());
    }

    response
}

fn generate_object_response(response: &LanguageModelResponse) -> GenerateObjectResponse {
    GenerateObjectResponse {
        id: response.id.clone(),
        timestamp: response.timestamp,
        model_id: response.model_id.clone(),
        headers: response.headers.clone(),
        body: response.body.clone(),
    }
}

pub(crate) fn generate_object_call_id() -> String {
    let generate_call_id =
        create_id_generator(IdGeneratorOptions::new().with_prefix("aiobj").with_size(24))
            .expect("default generate_object call id configuration is valid");

    generate_call_id()
}

fn extract_object_text(content: &[LanguageModelContent]) -> Option<String> {
    let mut text = String::new();
    let mut has_text = false;

    for part in content {
        if let LanguageModelContent::Text(LanguageModelText { text: part, .. }) = part {
            has_text = true;
            text.push_str(part);
        }
    }

    has_text.then_some(text)
}

fn extract_object_reasoning(content: &[LanguageModelContent]) -> Option<String> {
    let parts = content
        .iter()
        .filter_map(|part| match part {
            LanguageModelContent::Reasoning(LanguageModelReasoning { text, .. }) => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use time::OffsetDateTime;

    use super::{
        GenerateObjectEndEvent, GenerateObjectOptions, GenerateObjectOutputKind,
        GenerateObjectRequest, GenerateObjectResponse, GenerateObjectResult,
        GenerateObjectStartEvent, GenerateObjectStepEndEvent, GenerateObjectStepStartEvent,
        array_json_schema, enum_json_schema, generate_object,
    };
    use crate::VERSION;
    use crate::file_data::FileData;
    use crate::headers::Headers;
    use crate::language_model::{
        FinishReason, InputTokenUsage, LanguageModel, LanguageModelCallOptions,
        LanguageModelContent, LanguageModelFilePart, LanguageModelFinishReason,
        LanguageModelGenerateResult, LanguageModelMessage, LanguageModelPrompt,
        LanguageModelReasoning, LanguageModelResponse, LanguageModelResponseFormat,
        LanguageModelStreamResult, LanguageModelSupportedUrls, LanguageModelSystemMessage,
        LanguageModelText, LanguageModelTextPart, LanguageModelUsage, LanguageModelUserContentPart,
        LanguageModelUserMessage, OutputTokenUsage,
    };
    use crate::logger::{LogWarningsOptions, take_log_warning_calls_for_tests};
    use crate::mock_models::MockLanguageModel;
    use crate::prompt::Prompt;
    use crate::provider::{ProviderMetadata, ProviderOptions};
    use crate::provider_utils::{Schema, ValidationResult, json_schema};
    use crate::retry::DEFAULT_MAX_RETRIES;
    use crate::telemetry::{
        TelemetryEvent, TelemetryEventKind, TelemetryIntegration, TelemetryOptions,
    };
    use crate::warning::Warning;
    use url::Url;

    #[derive(Debug)]
    struct StaticObjectModel {
        model_id: String,
        result: LanguageModelGenerateResult,
        seen_options: Mutex<Vec<LanguageModelCallOptions>>,
        supported_urls_called: Option<Arc<Mutex<bool>>>,
    }

    impl StaticObjectModel {
        fn new(result: LanguageModelGenerateResult) -> Self {
            Self {
                model_id: "object-test".to_string(),
                result,
                seen_options: Mutex::new(Vec::new()),
                supported_urls_called: None,
            }
        }

        fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
            self.model_id = model_id.into();
            self
        }

        fn with_supported_urls_called(mut self, supported_urls_called: Arc<Mutex<bool>>) -> Self {
            self.supported_urls_called = Some(supported_urls_called);
            self
        }

        fn seen_options(&self) -> Vec<LanguageModelCallOptions> {
            self.seen_options
                .lock()
                .expect("seen options lock is not poisoned")
                .clone()
        }
    }

    impl LanguageModel for StaticObjectModel {
        type SupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a;

        type Stream = ();

        type StreamFuture<'a>
            = Ready<LanguageModelStreamResult<Self::Stream>>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            if let Some(supported_urls_called) = &self.supported_urls_called {
                *supported_urls_called
                    .lock()
                    .expect("supported urls called lock") = self.model_id() == "mock-model-id";
            }

            ready(LanguageModelSupportedUrls::from([(
                "image/*".to_string(),
                vec![r"^https://.*$".to_string()],
            )]))
        }

        fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            self.seen_options
                .lock()
                .expect("seen options lock is not poisoned")
                .push(options);

            ready(self.result.clone())
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(()))
        }
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures should resolve without an async runtime"),
        }
    }

    fn prompt() -> LanguageModelPrompt {
        vec![LanguageModelMessage::System(
            LanguageModelSystemMessage::new("Return JSON."),
        )]
    }

    fn object_usage() -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(10),
                ..InputTokenUsage::default()
            },
            output_tokens: OutputTokenUsage {
                total: Some(4),
                text: Some(4),
                ..OutputTokenUsage::default()
            },
            raw: None,
        }
    }

    fn answer_json_schema() -> crate::json::JsonSchema {
        serde_json::from_value(json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "answer": {
                    "type": "number"
                }
            },
            "required": ["answer"],
            "additionalProperties": false
        }))
        .expect("answer schema is a JSON object")
    }

    fn answer_schema() -> Schema {
        json_schema(answer_json_schema()).with_validator(|value| {
            if value
                .get("answer")
                .is_some_and(serde_json::Value::is_number)
            {
                ValidationResult::success(value.clone())
            } else {
                ValidationResult::failure("answer must be a number")
            }
        })
    }

    #[test]
    fn generate_object_result_serializes_full_upstream_shape() {
        let usage = LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(12),
                cache_read: Some(3),
                ..InputTokenUsage::default()
            },
            output_tokens: OutputTokenUsage {
                total: Some(4),
                text: Some(4),
                ..OutputTokenUsage::default()
            },
            raw: Some(
                serde_json::from_value(json!({
                    "providerTokens": 16
                }))
                .expect("raw usage is an object"),
            ),
        };
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "test": {
                "traceId": "trace_123"
            }
        }))
        .expect("provider metadata deserializes");
        let timestamp = OffsetDateTime::from_unix_timestamp(0).expect("timestamp is valid");

        let result = GenerateObjectResult::new(
            json!({
                "answer": 42
            }),
            FinishReason::Stop,
            usage,
            GenerateObjectRequest::new().with_body(json!({
                "prompt": "Return JSON"
            })),
            GenerateObjectResponse::new()
                .with_id("resp_123")
                .with_timestamp(timestamp)
                .with_model_id("test-model")
                .with_header("x-request-id", "req_123")
                .with_body(json!({
                    "raw": true
                })),
        )
        .with_reasoning("The schema asks for an answer.")
        .with_warning(Warning::Other {
            message: "provider warning".to_string(),
        })
        .with_provider_metadata(provider_metadata);

        assert_eq!(
            serde_json::to_value(result).expect("generate object result serializes"),
            json!({
                "object": {
                    "answer": 42
                },
                "reasoning": "The schema asks for an answer.",
                "finishReason": "stop",
                "usage": {
                    "inputTokens": {
                        "total": 12,
                        "cacheRead": 3
                    },
                    "outputTokens": {
                        "total": 4,
                        "text": 4
                    },
                    "raw": {
                        "providerTokens": 16
                    }
                },
                "warnings": [
                    {
                        "type": "other",
                        "message": "provider warning"
                    }
                ],
                "request": {
                    "body": {
                        "prompt": "Return JSON"
                    }
                },
                "response": {
                    "id": "resp_123",
                    "timestamp": "1970-01-01T00:00:00Z",
                    "modelId": "test-model",
                    "headers": {
                        "x-request-id": "req_123"
                    },
                    "body": {
                        "raw": true
                    }
                },
                "providerMetadata": {
                    "test": {
                        "traceId": "trace_123"
                    }
                }
            })
        );
    }

    #[test]
    fn generate_object_result_deserializes_minimal_upstream_shape() {
        let result: GenerateObjectResult = serde_json::from_value(json!({
            "object": {
                "ok": true
            },
            "finishReason": "stop",
            "usage": {
                "inputTokens": {},
                "outputTokens": {}
            },
            "request": {},
            "response": {}
        }))
        .expect("minimal generate object result deserializes");

        assert_eq!(result.object, json!({ "ok": true }));
        assert_eq!(result.reasoning, None);
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage, LanguageModelUsage::default());
        assert_eq!(result.warnings, None);
        assert_eq!(result.request, GenerateObjectRequest::new());
        assert_eq!(result.response, GenerateObjectResponse::new());
        assert_eq!(result.provider_metadata, None);
    }

    #[test]
    fn generate_object_result_supports_typed_objects() {
        #[derive(Debug, Deserialize, PartialEq, Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Answer {
            final_answer: String,
        }

        let result = GenerateObjectResult::new(
            Answer {
                final_answer: "yes".to_string(),
            },
            FinishReason::Stop,
            LanguageModelUsage::default(),
            GenerateObjectRequest::new(),
            GenerateObjectResponse::new(),
        );

        assert_eq!(
            serde_json::to_value(result).expect("typed generate object result serializes"),
            json!({
                "object": {
                    "finalAnswer": "yes"
                },
                "finishReason": "stop",
                "usage": {
                    "inputTokens": {},
                    "outputTokens": {}
                },
                "request": {},
                "response": {}
            })
        );
    }

    #[test]
    fn generate_object_calls_model_with_json_response_format_and_parses_text() {
        let response_timestamp =
            OffsetDateTime::from_unix_timestamp(1).expect("timestamp is valid");
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "test-provider": {
                "traceId": "trace_123"
            }
        }))
        .expect("provider metadata deserializes");

        let result = LanguageModelGenerateResult::new(
            vec![
                LanguageModelContent::Reasoning(LanguageModelReasoning::new("first")),
                LanguageModelContent::Reasoning(LanguageModelReasoning::new("second")),
                LanguageModelContent::Text(LanguageModelText::new("{\"answer\":42}")),
            ],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        )
        .with_request(
            crate::language_model::LanguageModelRequest::new().with_body(json!({
                "prompt": "Return JSON."
            })),
        )
        .with_response(
            LanguageModelResponse::new()
                .with_id("resp_123")
                .with_timestamp(response_timestamp)
                .with_model_id("object-test")
                .with_header("x-request-id", "req_123")
                .with_body(json!({
                    "raw": true
                })),
        )
        .with_warning(Warning::Other {
            message: "provider warning".to_string(),
        })
        .with_provider_metadata(provider_metadata.clone());
        let model = StaticObjectModel::new(result);

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt()).with_header("X-Custom", "yes"),
        ))
        .expect("object is generated");

        assert_eq!(output.object, json!({ "answer": 42 }));
        assert_eq!(output.reasoning.as_deref(), Some("first\nsecond"));
        assert_eq!(output.finish_reason, FinishReason::Stop);
        assert_eq!(output.usage, object_usage());
        assert_eq!(
            output.warnings,
            Some(vec![Warning::Other {
                message: "provider warning".to_string()
            }])
        );
        assert_eq!(
            output.request,
            GenerateObjectRequest::new().with_body(json!({
                "prompt": "Return JSON."
            }))
        );
        assert_eq!(
            output.response,
            GenerateObjectResponse::new()
                .with_id("resp_123")
                .with_timestamp(response_timestamp)
                .with_model_id("object-test")
                .with_header("x-request-id", "req_123")
                .with_body(json!({
                    "raw": true
                }))
        );
        assert_eq!(output.provider_metadata, Some(provider_metadata));

        let seen_options = model.seen_options();
        assert_eq!(seen_options.len(), 1);
        assert_eq!(
            seen_options[0].response_format,
            Some(LanguageModelResponseFormat::json())
        );
        let headers = seen_options[0]
            .headers
            .as_ref()
            .expect("headers include user agent");
        assert_eq!(headers.get("x-custom").map(String::as_str), Some("yes"));
        assert_eq!(
            headers.get("user-agent").map(String::as_str),
            Some(format!("ai/{VERSION}").as_str())
        );
    }

    #[test]
    fn generate_object_calls_log_warnings_with_the_correct_warnings() {
        let expected_warnings = vec![
            Warning::Other {
                message: "Setting is not supported".to_string(),
            },
            Warning::Unsupported {
                feature: "temperature".to_string(),
                details: Some("Temperature parameter not supported".to_string()),
            },
        ];
        let mut generate_result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":42}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        for warning in expected_warnings.clone() {
            generate_result = generate_result.with_warning(warning);
        }
        let model = MockLanguageModel::new().with_generate_result(generate_result);
        take_log_warning_calls_for_tests();

        let _output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ))
        .expect("object is generated");

        assert_eq!(
            take_log_warning_calls_for_tests(),
            vec![
                LogWarningsOptions::new(expected_warnings)
                    .with_scope("mock-provider", "mock-model-id")
            ]
        );
    }

    #[test]
    fn generate_object_calls_log_warnings_with_empty_array_when_no_warnings_are_present() {
        let model =
            MockLanguageModel::new().with_generate_result(LanguageModelGenerateResult::new(
                vec![LanguageModelContent::Text(LanguageModelText::new(
                    "{\"answer\":42}",
                ))],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: Some("stop".to_string()),
                },
                object_usage(),
            ));
        take_log_warning_calls_for_tests();

        let _output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ))
        .expect("object is generated");

        assert_eq!(
            take_log_warning_calls_for_tests(),
            vec![LogWarningsOptions::new(Vec::new()).with_scope("mock-provider", "mock-model-id")]
        );
    }

    #[test]
    fn generate_object_result_contains_request_information() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":42}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        )
        .with_request(
            crate::language_model::LanguageModelRequest::new().with_body(json!("test body")),
        );
        let model = StaticObjectModel::new(result);

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ))
        .expect("object is generated");

        assert_eq!(
            output.request,
            GenerateObjectRequest::new().with_body(json!("test body"))
        );
    }

    #[test]
    fn generate_object_result_contains_response_information() {
        let response_timestamp =
            OffsetDateTime::from_unix_timestamp(10).expect("timestamp is valid");
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":42}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        )
        .with_response(
            LanguageModelResponse::new()
                .with_id("test-id-from-model")
                .with_timestamp(response_timestamp)
                .with_model_id("test-response-model-id")
                .with_header("custom-response-header", "response-header-value")
                .with_header("user-agent", format!("ai/{VERSION}"))
                .with_body(json!("test body")),
        );
        let model = StaticObjectModel::new(result);

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ))
        .expect("object is generated");

        assert_eq!(
            output.response,
            GenerateObjectResponse::new()
                .with_id("test-id-from-model")
                .with_timestamp(response_timestamp)
                .with_model_id("test-response-model-id")
                .with_header("custom-response-header", "response-header-value")
                .with_header("user-agent", format!("ai/{VERSION}"))
                .with_body(json!("test body"))
        );
    }

    #[test]
    fn generate_object_result_contains_provider_metadata() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "exampleProvider": {
                "a": 10,
                "b": 20
            }
        }))
        .expect("provider metadata deserializes");
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":42}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        )
        .with_provider_metadata(provider_metadata.clone());
        let model = StaticObjectModel::new(result);

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ))
        .expect("object is generated");

        assert_eq!(output.provider_metadata, Some(provider_metadata));
    }

    #[test]
    fn generate_object_passes_headers_to_model() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":42}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_header("custom-request-header", "request-header-value"),
        ))
        .expect("object is generated");

        assert_eq!(output.object, json!({ "answer": 42 }));
        let calls = model.seen_options();
        let headers = calls[0].headers.as_ref().expect("headers are captured");
        assert_eq!(
            headers.get("custom-request-header").map(String::as_str),
            Some("request-header-value")
        );
        assert_eq!(
            headers.get("user-agent").map(String::as_str),
            Some(format!("ai/{VERSION}").as_str())
        );
    }

    #[test]
    fn generate_object_passes_provider_options_to_model() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "aProvider": {
                "someKey": "someValue"
            }
        }))
        .expect("provider options deserialize");
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":42}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_provider_options(provider_options.clone()),
        ))
        .expect("object is generated");

        assert_eq!(output.object, json!({ "answer": 42 }));
        assert_eq!(
            model.seen_options()[0].provider_options,
            Some(provider_options)
        );
    }

    #[test]
    fn generate_object_messages_with_url_file_calls_model_supported_urls() {
        let supported_urls_called = Arc::new(Mutex::new(false));
        let schema = json_schema(
            serde_json::from_value(json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "type": "object",
                "properties": {
                    "content": { "type": "string" }
                },
                "required": ["content"],
                "additionalProperties": false
            }))
            .expect("content schema is valid"),
        )
        .with_validator(|value| {
            if value
                .get("content")
                .is_some_and(serde_json::Value::is_string)
            {
                ValidationResult::success(value.clone())
            } else {
                ValidationResult::failure("content must be a string")
            }
        });
        let model = StaticObjectModel::new(LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                r#"{ "content": "Hello, world!" }"#,
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        ))
        .with_model_id("mock-model-id")
        .with_supported_urls_called(Arc::clone(&supported_urls_called));
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

        let result = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt).with_schema(schema),
        ))
        .expect("object is generated");

        assert_eq!(result.object, json!({ "content": "Hello, world!" }));
        assert!(
            *supported_urls_called
                .lock()
                .expect("supported urls called lock")
        );
    }

    #[test]
    fn generate_object_retries_retryable_pre_content_errors() {
        let retry_metadata: ProviderMetadata = serde_json::from_value(json!({
            "mock": {
                "errorMessage": "rate limited",
                "statusCode": 429,
                "isRetryable": true
            }
        }))
        .expect("provider metadata deserializes");
        let retryable_error = LanguageModelGenerateResult::new(
            Vec::new(),
            LanguageModelFinishReason {
                unified: FinishReason::Error,
                raw: Some("api-error".to_string()),
            },
            LanguageModelUsage::default(),
        )
        .with_provider_metadata(retry_metadata);
        let successful_result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":42}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            LanguageModelUsage::default(),
        );
        let model =
            MockLanguageModel::new().with_generate_results([retryable_error, successful_result]);

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt()).with_max_retries(1),
        ))
        .expect("object is generated");

        assert_eq!(model.generate_calls().len(), 2);
        assert_eq!(output.object, json!({ "answer": 42 }));
        assert_eq!(output.finish_reason, FinishReason::Stop);
    }

    #[test]
    fn generate_object_options_from_prompt_standardizes_high_level_prompt() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":42}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let options = GenerateObjectOptions::from_prompt(
            &model,
            Prompt::from_prompt("Return the answer.").with_instructions("Use JSON."),
        )
        .expect("high-level prompt standardizes");
        let output = poll_ready(generate_object(options)).expect("object is generated");

        assert_eq!(output.object, json!({ "answer": 42 }));

        let seen_options = model.seen_options();
        assert_eq!(
            seen_options[0].prompt,
            vec![
                LanguageModelMessage::System(LanguageModelSystemMessage::new("Use JSON.")),
                LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                        "Return the answer."
                    ))
                ])),
            ]
        );
    }

    #[test]
    fn generate_object_options_from_prompt_rejects_invalid_high_level_prompt() {
        let model = StaticObjectModel::new(LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new("{}"))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        ));

        let error = GenerateObjectOptions::from_prompt(
            &model,
            Prompt::from_messages(vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new("Use JSON."),
            )]),
        )
        .expect_err("system messages are rejected by high-level prompt standardization");

        assert_eq!(
            error.message(),
            "Invalid prompt: System messages are not allowed in the prompt or messages fields. Use the instructions option instead."
        );
        assert!(model.seen_options().is_empty());
    }

    #[test]
    fn generate_object_forwards_schema_metadata_and_validates_output() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":42}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_schema_name("answer")
                .with_schema_description("A numeric answer object."),
        ))
        .expect("schema-valid object is generated");

        assert_eq!(output.object, json!({ "answer": 42 }));

        let seen_options = model.seen_options();
        assert_eq!(seen_options.len(), 1);
        assert_eq!(
            seen_options[0].response_format,
            Some(
                LanguageModelResponseFormat::json()
                    .with_schema(answer_json_schema())
                    .with_name("answer")
                    .with_description("A numeric answer object.")
            )
        );
    }

    #[test]
    fn generate_object_lifecycle_events_use_upstream_json_shapes() {
        let start = GenerateObjectStartEvent {
            call_id: "aiobj_call".to_string(),
            operation_id: "ai.generateObject".to_string(),
            provider: "test-provider".to_string(),
            model_id: "object-test".to_string(),
            messages: prompt(),
            max_output_tokens: Some(128),
            temperature: Some(0.2),
            top_p: Some(0.9),
            top_k: Some(40),
            presence_penalty: Some(0.1),
            frequency_penalty: Some(0.3),
            seed: Some(7),
            max_retries: DEFAULT_MAX_RETRIES,
            headers: Some(Headers::from([(
                "user-agent".to_string(),
                "ai/test".to_string(),
            )])),
            provider_options: None,
            output: GenerateObjectOutputKind::Object,
            schema: Some(answer_json_schema()),
            schema_name: Some("answer".to_string()),
            schema_description: Some("A numeric answer object.".to_string()),
        };

        let serialized_start =
            serde_json::to_value(&start).expect("start event serializes to JSON");
        assert_eq!(serialized_start["callId"], "aiobj_call");
        assert_eq!(serialized_start["operationId"], "ai.generateObject");
        assert_eq!(serialized_start["maxOutputTokens"], 128);
        assert_eq!(serialized_start["maxRetries"], DEFAULT_MAX_RETRIES);
        assert_eq!(serialized_start["output"], "object");
        assert_eq!(serialized_start["schemaName"], "answer");
        assert_eq!(
            serde_json::from_value::<GenerateObjectStartEvent>(serialized_start)
                .expect("start event deserializes"),
            start
        );

        let step_start = GenerateObjectStepStartEvent {
            call_id: "aiobj_call".to_string(),
            step_number: 0,
            provider: "test-provider".to_string(),
            model_id: "object-test".to_string(),
            provider_options: None,
            headers: Some(Headers::from([(
                "user-agent".to_string(),
                "ai/test".to_string(),
            )])),
            prompt_messages: prompt(),
        };

        let serialized_step_start =
            serde_json::to_value(&step_start).expect("step-start event serializes");
        assert_eq!(serialized_step_start["stepNumber"], 0);
        assert!(serialized_step_start.get("promptMessages").is_some());
        assert_eq!(
            serde_json::from_value::<GenerateObjectStepStartEvent>(serialized_step_start)
                .expect("step-start event deserializes"),
            step_start
        );

        let response_timestamp =
            OffsetDateTime::from_unix_timestamp(1).expect("timestamp is valid");
        let step_end = GenerateObjectStepEndEvent {
            call_id: "aiobj_call".to_string(),
            step_number: 0,
            provider: "test-provider".to_string(),
            model_id: "object-test".to_string(),
            finish_reason: FinishReason::Stop,
            usage: object_usage(),
            object_text: "{\"answer\":42}".to_string(),
            reasoning: Some("because".to_string()),
            warnings: vec![Warning::Other {
                message: "provider warning".to_string(),
            }],
            request: GenerateObjectRequest::new().with_body(json!({ "prompt": "Return JSON." })),
            response: GenerateObjectResponse::new()
                .with_id("resp_123")
                .with_timestamp(response_timestamp)
                .with_model_id("object-test"),
            provider_metadata: None,
            ms_to_first_chunk: None,
        };

        let serialized_step_end =
            serde_json::to_value(&step_end).expect("step-end event serializes");
        assert_eq!(serialized_step_end["objectText"], "{\"answer\":42}");
        assert_eq!(serialized_step_end["reasoning"], "because");
        assert_eq!(
            serde_json::from_value::<GenerateObjectStepEndEvent>(serialized_step_end)
                .expect("step-end event deserializes"),
            step_end
        );

        let end = GenerateObjectEndEvent {
            call_id: "aiobj_call".to_string(),
            object: Some(json!({ "answer": 42 })),
            error: None,
            reasoning: Some("because".to_string()),
            finish_reason: FinishReason::Stop,
            usage: object_usage(),
            warnings: Vec::new(),
            request: GenerateObjectRequest::new(),
            response: GenerateObjectResponse::new(),
            provider_metadata: None,
        };

        let serialized_end = serde_json::to_value(&end).expect("end event serializes");
        assert_eq!(serialized_end["object"], json!({ "answer": 42 }));
        assert!(serialized_end.get("error").is_none());
        assert_eq!(
            serde_json::from_value::<GenerateObjectEndEvent>(serialized_end)
                .expect("end event deserializes"),
            end
        );
    }

    #[test]
    fn generate_object_invokes_lifecycle_callbacks_with_single_step_events() {
        let response_timestamp =
            OffsetDateTime::from_unix_timestamp(2).expect("timestamp is valid");
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "test-provider": {
                "traceId": "trace_callback"
            }
        }))
        .expect("provider metadata deserializes");
        let result = LanguageModelGenerateResult::new(
            vec![
                LanguageModelContent::Reasoning(LanguageModelReasoning::new("because")),
                LanguageModelContent::Text(LanguageModelText::new("{\"answer\":42}")),
            ],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        )
        .with_request(
            crate::language_model::LanguageModelRequest::new().with_body(json!({
                "prompt": "Return JSON."
            })),
        )
        .with_response(
            LanguageModelResponse::new()
                .with_id("resp_callback")
                .with_timestamp(response_timestamp)
                .with_model_id("object-test")
                .with_header("x-request-id", "req_callback"),
        )
        .with_warning(Warning::Other {
            message: "provider warning".to_string(),
        })
        .with_provider_metadata(provider_metadata.clone());
        let model = StaticObjectModel::new(result);

        let start_events = Arc::new(Mutex::new(Vec::<GenerateObjectStartEvent>::new()));
        let step_start_events = Arc::new(Mutex::new(Vec::<GenerateObjectStepStartEvent>::new()));
        let step_end_events = Arc::new(Mutex::new(Vec::<GenerateObjectStepEndEvent>::new()));
        let end_events = Arc::new(Mutex::new(Vec::<GenerateObjectEndEvent>::new()));

        let start_events_for_callback = Arc::clone(&start_events);
        let step_start_events_for_callback = Arc::clone(&step_start_events);
        let step_end_events_for_callback = Arc::clone(&step_end_events);
        let end_events_for_callback = Arc::clone(&end_events);

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_schema_name("answer")
                .with_header("X-Custom", "yes")
                .with_experimental_on_start(move |event| {
                    let start_events = Arc::clone(&start_events_for_callback);
                    async move {
                        start_events
                            .lock()
                            .expect("start events lock is not poisoned")
                            .push(event);
                    }
                })
                .with_experimental_on_step_start(move |event| {
                    let step_start_events = Arc::clone(&step_start_events_for_callback);
                    async move {
                        step_start_events
                            .lock()
                            .expect("step-start events lock is not poisoned")
                            .push(event);
                    }
                })
                .with_on_step_finish(move |event| {
                    let step_end_events = Arc::clone(&step_end_events_for_callback);
                    async move {
                        step_end_events
                            .lock()
                            .expect("step-end events lock is not poisoned")
                            .push(event);
                    }
                })
                .with_on_finish(move |event| {
                    let end_events = Arc::clone(&end_events_for_callback);
                    async move {
                        end_events
                            .lock()
                            .expect("end events lock is not poisoned")
                            .push(event);
                    }
                }),
        ))
        .expect("object is generated");

        assert_eq!(output.object, json!({ "answer": 42 }));

        let start_events = start_events
            .lock()
            .expect("start events lock is not poisoned");
        let step_start_events = step_start_events
            .lock()
            .expect("step-start events lock is not poisoned");
        let step_end_events = step_end_events
            .lock()
            .expect("step-end events lock is not poisoned");
        let end_events = end_events.lock().expect("end events lock is not poisoned");

        assert_eq!(start_events.len(), 1);
        assert_eq!(step_start_events.len(), 1);
        assert_eq!(step_end_events.len(), 1);
        assert_eq!(end_events.len(), 1);

        let call_id = &start_events[0].call_id;
        assert!(call_id.starts_with("aiobj-"));
        assert_eq!(step_start_events[0].call_id, *call_id);
        assert_eq!(step_end_events[0].call_id, *call_id);
        assert_eq!(end_events[0].call_id, *call_id);

        assert_eq!(start_events[0].operation_id, "ai.generateObject");
        assert_eq!(start_events[0].provider, "test-provider");
        assert_eq!(start_events[0].model_id, "object-test");
        assert_eq!(start_events[0].output, GenerateObjectOutputKind::Object);
        assert_eq!(start_events[0].schema, Some(answer_json_schema()));
        assert_eq!(start_events[0].schema_name.as_deref(), Some("answer"));
        assert_eq!(
            start_events[0]
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-custom"))
                .map(String::as_str),
            Some("yes")
        );
        assert_eq!(
            start_events[0]
                .headers
                .as_ref()
                .and_then(|headers| headers.get("user-agent"))
                .map(String::as_str),
            Some(format!("ai/{VERSION}").as_str())
        );

        assert_eq!(step_start_events[0].step_number, 0);
        assert_eq!(step_start_events[0].prompt_messages, prompt());
        assert_eq!(step_end_events[0].object_text, "{\"answer\":42}");
        assert_eq!(step_end_events[0].reasoning.as_deref(), Some("because"));
        assert_eq!(step_end_events[0].warnings.len(), 1);
        assert_eq!(
            step_end_events[0].request.body,
            Some(json!({ "prompt": "Return JSON." }))
        );
        assert_eq!(
            step_end_events[0].response,
            GenerateObjectResponse::new()
                .with_id("resp_callback")
                .with_timestamp(response_timestamp)
                .with_model_id("object-test")
                .with_header("x-request-id", "req_callback")
        );
        assert_eq!(
            step_end_events[0].provider_metadata,
            Some(provider_metadata.clone())
        );

        assert_eq!(end_events[0].object, Some(json!({ "answer": 42 })));
        assert_eq!(end_events[0].error, None);
        assert_eq!(end_events[0].reasoning.as_deref(), Some("because"));
        assert_eq!(end_events[0].warnings.len(), 1);
        assert_eq!(end_events[0].provider_metadata, Some(provider_metadata));
    }

    fn panicking_generate_object_callback<T>(_event: T) -> Ready<()> {
        panic!("callback error")
    }

    #[test]
    fn generate_object_callback_panics_do_not_break_generation() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":42}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_experimental_on_start(panicking_generate_object_callback)
                .with_experimental_on_step_start(panicking_generate_object_callback)
                .with_on_step_finish(panicking_generate_object_callback)
                .with_on_finish(panicking_generate_object_callback),
        ))
        .expect("object is generated despite callback panics");

        assert_eq!(output.object, json!({ "answer": 42 }));
    }

    #[test]
    fn generate_object_dispatches_telemetry_lifecycle_events() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":42}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);
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

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_telemetry(
                    TelemetryOptions::new()
                        .with_function_id("generate-object-test")
                        .with_record_inputs(false)
                        .with_record_outputs(true)
                        .with_integration(integration),
                ),
        ))
        .expect("object is generated");

        assert_eq!(output.object, json!({ "answer": 42 }));
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
                .all(|event| event.function_id.as_deref() == Some("generate-object-test"))
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
        assert_eq!(events[0].event["operationId"], json!("ai.generateObject"));
        assert_eq!(events[0].event["provider"], json!("test-provider"));
        assert_eq!(events[2].event["objectText"], json!("{\"answer\":42}"));
        assert_eq!(events[3].event["object"], json!({ "answer": 42 }));
    }

    #[test]
    fn generate_object_accepts_experimental_telemetry_alias() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":42}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);
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

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt())
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
        ))
        .expect("object is generated");

        assert_eq!(output.object, json!({ "answer": 42 }));
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
    fn generate_object_forwards_array_schema_and_returns_elements() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"elements\":[{\"answer\":1},{\"answer\":2}]}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);
        let schema = answer_schema();

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt())
                .with_array_schema(schema.clone())
                .with_schema_name("answers")
                .with_schema_description("A list of numeric answer objects."),
        ))
        .expect("array output is generated");

        assert_eq!(output.object, json!([{ "answer": 1 }, { "answer": 2 }]));

        let seen_options = model.seen_options();
        assert_eq!(seen_options.len(), 1);
        assert_eq!(
            seen_options[0].response_format,
            Some(
                LanguageModelResponseFormat::json()
                    .with_schema(array_json_schema(&schema.into()))
                    .with_name("answers")
                    .with_description("A list of numeric answer objects.")
            )
        );
    }

    #[test]
    fn generate_object_reports_array_shape_failures_as_no_object_errors() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"items\":[{\"answer\":1}]}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let error = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt()).with_array_schema(answer_schema()),
        ))
        .expect_err("array output without elements should fail");

        assert_eq!(
            error.message(),
            "No object generated: response did not match schema."
        );
        assert_eq!(error.text(), Some("{\"items\":[{\"answer\":1}]}"));
        assert!(error.cause_message().is_some_and(|cause| {
            cause.contains("value must be an object that contains an array of elements")
        }));
    }

    #[test]
    fn generate_object_reports_array_element_validation_failures() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"elements\":[{\"answer\":1},{\"answer\":\"wrong\"}]}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let error = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt()).with_array_schema(answer_schema()),
        ))
        .expect_err("schema-invalid array element should fail");

        assert_eq!(
            error.message(),
            "No object generated: response did not match schema."
        );
        assert_eq!(
            error.text(),
            Some("{\"elements\":[{\"answer\":1},{\"answer\":\"wrong\"}]}")
        );
        assert!(
            error
                .cause_message()
                .is_some_and(|cause| cause.contains("answer must be a number"))
        );
    }

    #[test]
    fn generate_object_forwards_enum_schema_and_returns_selected_value() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"result\":\"green\"}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let enum_values = vec!["red".to_string(), "green".to_string()];
        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt()).with_enum_values(enum_values.clone()),
        ))
        .expect("enum value is generated");

        assert_eq!(output.object, json!("green"));

        let seen_options = model.seen_options();
        assert_eq!(seen_options.len(), 1);
        assert_eq!(
            seen_options[0].response_format,
            Some(LanguageModelResponseFormat::json().with_schema(enum_json_schema(&enum_values)))
        );
    }

    #[test]
    fn generate_object_reports_enum_validation_failures_as_no_object_errors() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"result\":\"blue\"}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let error = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt()).with_enum_values(["red", "green"]),
        ))
        .expect_err("enum value outside the allowed set should fail");

        assert_eq!(
            error.message(),
            "No object generated: response did not match schema."
        );
        assert_eq!(error.text(), Some("{\"result\":\"blue\"}"));
        assert!(
            error
                .cause_message()
                .is_some_and(|cause| cause.contains("value must be a string in the enum"))
        );
    }

    #[test]
    fn generate_object_repairs_enum_validation_failures() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"result\":\"blue\"}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt())
                .with_enum_values(["red", "green"])
                .with_repair_text(|options| async move {
                    assert_eq!(options.text(), "{\"result\":\"blue\"}");
                    assert!(options.error().as_type_validation_error().is_some());

                    Some("{\"result\":\"green\"}".to_string())
                }),
        ))
        .expect("enum output is repaired");

        assert_eq!(output.object, json!("green"));
    }

    #[test]
    fn generate_object_reports_schema_validation_failures_as_no_object_errors() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":\"wrong\"}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let error = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt()).with_schema(answer_schema()),
        ))
        .expect_err("schema-invalid JSON should fail");

        assert_eq!(
            error.message(),
            "No object generated: response did not match schema."
        );
        assert_eq!(error.text(), Some("{\"answer\":\"wrong\"}"));
        assert!(
            error
                .cause_message()
                .is_some_and(|cause| cause.contains("answer must be a number"))
        );
        assert_eq!(error.usage(), &object_usage());
        assert_eq!(error.finish_reason(), &FinishReason::Stop);
        assert_eq!(error.response().model_id.as_deref(), Some("object-test"));
    }

    #[test]
    fn generate_object_repairs_parse_failures() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":42",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_experimental_repair_text(|options| async move {
                    assert_eq!(options.text(), "{\"answer\":42");
                    assert!(options.error().as_json_parse_error().is_some());

                    Some(format!("{}}}", options.text()))
                }),
        ))
        .expect("repaired object is generated");

        assert_eq!(output.object, json!({ "answer": 42 }));
    }

    #[test]
    fn generate_object_repairs_schema_validation_failures() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":\"wrong\"}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let output = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_repair_text(|options| async move {
                    let (text, error) = options.into_parts();
                    assert_eq!(text, "{\"answer\":\"wrong\"}");
                    assert!(error.as_type_validation_error().is_some());

                    Some("{\"answer\":42}".to_string())
                }),
        ))
        .expect("schema-invalid object is repaired");

        assert_eq!(output.object, json!({ "answer": 42 }));
    }

    #[test]
    fn generate_object_keeps_original_error_when_repair_returns_none() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(
                "{\"answer\":\"wrong\"}",
            ))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let error = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_repair_text(|options| async move {
                    assert_eq!(options.text(), "{\"answer\":\"wrong\"}");
                    assert!(options.error().as_type_validation_error().is_some());

                    None
                }),
        ))
        .expect_err("unrepaired validation error should fail");

        assert_eq!(
            error.message(),
            "No object generated: response did not match schema."
        );
        assert_eq!(error.text(), Some("{\"answer\":\"wrong\"}"));
        assert!(
            error
                .cause_message()
                .is_some_and(|cause| cause.contains("answer must be a number"))
        );
    }

    #[test]
    fn generate_object_reports_repaired_text_error_when_repair_fails() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new("{ bad"))],
            LanguageModelFinishReason {
                unified: FinishReason::Other,
                raw: None,
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let error = poll_ready(generate_object(
            GenerateObjectOptions::new(&model, prompt())
                .with_schema(answer_schema())
                .with_repair_text(|options| async move {
                    assert_eq!(options.text(), "{ bad");
                    assert!(options.error().as_json_parse_error().is_some());

                    Some(format!("{}{{", options.text()))
                }),
        ))
        .expect_err("invalid repair should fail");

        assert_eq!(
            error.message(),
            "No object generated: could not parse the response."
        );
        assert_eq!(error.text(), Some("{ bad{"));
        assert!(error.cause_message().is_some());
    }

    #[test]
    fn generate_object_reports_parse_failures_as_no_object_errors() {
        let result = LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new("{ bad"))],
            LanguageModelFinishReason {
                unified: FinishReason::Other,
                raw: None,
            },
            object_usage(),
        );
        let model = StaticObjectModel::new(result);

        let error = poll_ready(generate_object(GenerateObjectOptions::new(
            &model,
            prompt(),
        )))
        .expect_err("invalid JSON should fail");

        assert_eq!(
            error.message(),
            "No object generated: could not parse the response."
        );
        assert_eq!(error.text(), Some("{ bad"));
        assert!(error.cause_message().is_some());
        assert_eq!(error.usage(), &object_usage());
        assert_eq!(error.finish_reason(), &FinishReason::Other);
        assert_eq!(error.response().model_id.as_deref(), Some("object-test"));
    }
}
