use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::headers::Headers;
use crate::json::JsonValue;
use crate::language_model::{FinishReason, LanguageModelUsage};
use crate::provider::ProviderMetadata;
use crate::warning::Warning;

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

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use time::OffsetDateTime;

    use super::{GenerateObjectRequest, GenerateObjectResponse, GenerateObjectResult};
    use crate::language_model::{
        FinishReason, InputTokenUsage, LanguageModelUsage, OutputTokenUsage,
    };
    use crate::provider::ProviderMetadata;
    use crate::warning::Warning;

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
}
