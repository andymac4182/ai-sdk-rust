use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::VERSION;
use crate::headers::Headers;
use crate::json::JsonValue;
use crate::language_model::{
    FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
    LanguageModelPrompt, LanguageModelReasoning, LanguageModelRequest, LanguageModelResponse,
    LanguageModelResponseFormat, LanguageModelText, LanguageModelUsage,
};
use crate::provider::ProviderMetadata;
use crate::provider_utils::{
    ParseJsonResult, generate_id, safe_parse_json, with_user_agent_suffix,
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

/// Options for a high-level non-streaming object generation call.
#[derive(Debug)]
pub struct GenerateObjectOptions<'a, M: LanguageModel + ?Sized> {
    /// Language model used for object generation.
    pub model: &'a M,

    /// Provider-level call options sent to the model.
    pub call_options: LanguageModelCallOptions,
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
/// This is the first Rust-native runtime slice for upstream `generateObject`.
/// It uses the no-schema output strategy: the model is called with a JSON
/// response format, generated text is parsed as JSON, and schema-specific
/// validation/repair remain future extensions.
pub async fn generate_object<M>(
    options: GenerateObjectOptions<'_, M>,
) -> Result<GenerateObjectResult, NoObjectGeneratedError>
where
    M: LanguageModel + ?Sized,
{
    let GenerateObjectOptions {
        model,
        mut call_options,
    } = options;

    call_options.response_format = Some(LanguageModelResponseFormat::json());
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

    let object = match safe_parse_json(&text) {
        ParseJsonResult::Success { value, .. } => value,
        ParseJsonResult::Failure { error, .. } => {
            return Err(NoObjectGeneratedError::with_message(
                "No object generated: could not parse the response.",
                response,
                usage,
                finish_reason,
            )
            .with_text(text)
            .with_cause(error));
        }
    };

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

fn append_generate_object_user_agent(call_options: &mut LanguageModelCallOptions) {
    let headers = call_options.headers.take().map(|headers| {
        headers
            .into_iter()
            .map(|(name, value)| (name, Some(value)))
            .collect::<Vec<_>>()
    });

    call_options.headers = Some(with_user_agent_suffix(headers, [format!("ai/{VERSION}")]));
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
        generate_object,
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
