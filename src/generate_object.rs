use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::VERSION;
use crate::headers::Headers;
use crate::json::{JsonSchema, JsonValue};
use crate::language_model::{
    FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
    LanguageModelPrompt, LanguageModelReasoning, LanguageModelRequest, LanguageModelResponse,
    LanguageModelResponseFormat, LanguageModelText, LanguageModelUsage,
};
use crate::provider::{ProviderMetadata, TypeValidationError};
use crate::provider_utils::{
    FlexibleSchema, ParseJsonError, ParseJsonResult, generate_id, safe_parse_json,
    safe_parse_json_with_schema, with_user_agent_suffix,
};
use crate::warning::Warning;

pub use crate::generate_text::NoObjectGeneratedError;

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
}

impl<'a, M: LanguageModel + ?Sized> GenerateObjectOptions<'a, M> {
    /// Creates object generation options for a model and standardized prompt.
    pub fn new(model: &'a M, prompt: LanguageModelPrompt) -> Self {
        Self::from_call_options(model, LanguageModelCallOptions::new(prompt))
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
    } = options;

    call_options.response_format = Some(generate_object_response_format(
        &schema,
        &schema_name,
        &schema_description,
        enum_values.as_deref(),
    ));
    append_generate_object_user_agent(&mut call_options);

    let generate_result = model.do_generate(call_options).await;
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

    let object = parse_generated_object_with_repair(
        text,
        schema,
        enum_values.as_deref(),
        repair_text.as_ref(),
        &response,
        &usage,
        &finish_reason,
    )
    .await?;

    let mut result =
        GenerateObjectResult::new(object, finish_reason, usage, request, result_response)
            .with_warnings(generate_result.warnings);

    if let Some(reasoning) = extract_object_reasoning(&generate_result.content) {
        result = result.with_reasoning(reasoning);
    }

    if let Some(provider_metadata) = generate_result.provider_metadata {
        result = result.with_provider_metadata(provider_metadata);
    }

    Ok(result)
}

fn generate_object_response_format(
    schema: &Option<FlexibleSchema>,
    schema_name: &Option<String>,
    schema_description: &Option<String>,
    enum_values: Option<&[String]>,
) -> LanguageModelResponseFormat {
    let mut response_format = LanguageModelResponseFormat::json();

    if let Some(enum_values) = enum_values {
        response_format = response_format.with_schema(enum_json_schema(enum_values));
    } else if let Some(schema) = schema {
        response_format = response_format.with_schema(schema.as_schema().json_schema().clone());

        if let Some(schema_name) = schema_name {
            response_format = response_format.with_name(schema_name.clone());
        }

        if let Some(schema_description) = schema_description {
            response_format = response_format.with_description(schema_description.clone());
        }
    }

    response_format
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

fn parse_generated_object(
    text: &str,
    schema: Option<FlexibleSchema>,
    enum_values: Option<&[String]>,
) -> ParseJsonResult<JsonValue> {
    let parse_result = schema.map_or_else(
        || safe_parse_json(text),
        |schema| safe_parse_json_with_schema(text, schema),
    );

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
    repair_text: Option<&GenerateObjectRepairText<'_>>,
    response: &LanguageModelResponse,
    usage: &LanguageModelUsage,
    finish_reason: &FinishReason,
) -> Result<JsonValue, NoObjectGeneratedError> {
    match parse_generated_object(&text, schema.clone(), enum_values) {
        ParseJsonResult::Success { value, .. } => Ok(value),
        ParseJsonResult::Failure { error, .. } => {
            let Some(repair_text) = repair_text else {
                return Err(parse_failure_error(
                    text,
                    error,
                    response,
                    usage,
                    finish_reason,
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
                    response,
                    usage,
                    finish_reason,
                ));
            };

            match parse_generated_object(&repaired_text, schema, enum_values) {
                ParseJsonResult::Success { value, .. } => Ok(value),
                ParseJsonResult::Failure { error, .. } => Err(parse_failure_error(
                    repaired_text,
                    error,
                    response,
                    usage,
                    finish_reason,
                )),
            }
        }
    }
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
    use std::sync::Mutex;
    use std::task::{Context, Poll, Waker};

    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use time::OffsetDateTime;

    use super::{
        GenerateObjectOptions, GenerateObjectRequest, GenerateObjectResponse, GenerateObjectResult,
        enum_json_schema, generate_object,
    };
    use crate::VERSION;
    use crate::language_model::{
        FinishReason, InputTokenUsage, LanguageModel, LanguageModelCallOptions,
        LanguageModelContent, LanguageModelFinishReason, LanguageModelGenerateResult,
        LanguageModelMessage, LanguageModelPrompt, LanguageModelReasoning, LanguageModelResponse,
        LanguageModelResponseFormat, LanguageModelStreamResult, LanguageModelSupportedUrls,
        LanguageModelSystemMessage, LanguageModelText, LanguageModelUsage, OutputTokenUsage,
    };
    use crate::provider::ProviderMetadata;
    use crate::provider_utils::{Schema, ValidationResult, json_schema};
    use crate::warning::Warning;

    #[derive(Debug)]
    struct StaticObjectModel {
        result: LanguageModelGenerateResult,
        seen_options: Mutex<Vec<LanguageModelCallOptions>>,
    }

    impl StaticObjectModel {
        fn new(result: LanguageModelGenerateResult) -> Self {
            Self {
                result,
                seen_options: Mutex::new(Vec::new()),
            }
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
            "object-test"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(LanguageModelSupportedUrls::new())
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
