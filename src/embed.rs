use serde::{Deserialize, Serialize};

use crate::embedding_model::{
    EmbeddingModelEmbedding, EmbeddingModelResponse, EmbeddingModelUsage,
};
use crate::provider::ProviderMetadata;
use crate::warning::Warning;

/// Embedding vector returned by high-level embed operations.
pub type Embedding = EmbeddingModelEmbedding;

/// Result of a high-level `embed` call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedResult {
    /// The value that was embedded.
    pub value: String,

    /// The embedding of the value.
    pub embedding: Embedding,

    /// Token usage for the embedding operation.
    pub usage: EmbeddingModelUsage,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,

    /// Optional provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Optional provider response data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<EmbeddingModelResponse>,
}

impl EmbedResult {
    /// Creates an embed result with no warnings.
    pub fn new(value: impl Into<String>, embedding: Embedding, usage: EmbeddingModelUsage) -> Self {
        Self {
            value: value.into(),
            embedding,
            usage,
            warnings: Vec::new(),
            provider_metadata: None,
            response: None,
        }
    }

    /// Adds a warning returned by the provider.
    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.push(warning);
        self
    }

    /// Adds provider-specific metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }

    /// Sets optional provider response data.
    pub fn with_response(mut self, response: EmbeddingModelResponse) -> Self {
        self.response = Some(response);
        self
    }
}

/// Result of a high-level `embedMany` call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedManyResult {
    /// The values that were embedded.
    pub values: Vec<String>,

    /// Embeddings in the same order as the values.
    pub embeddings: Vec<Embedding>,

    /// Token usage for the embedding operation.
    pub usage: EmbeddingModelUsage,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,

    /// Optional provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Optional raw response data for each provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responses: Option<Vec<Option<EmbeddingModelResponse>>>,
}

impl EmbedManyResult {
    /// Creates an embed-many result with no warnings.
    pub fn new(
        values: Vec<String>,
        embeddings: Vec<Embedding>,
        usage: EmbeddingModelUsage,
    ) -> Self {
        Self {
            values,
            embeddings,
            usage,
            warnings: Vec::new(),
            provider_metadata: None,
            responses: None,
        }
    }

    /// Adds a warning returned by the provider.
    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.push(warning);
        self
    }

    /// Adds provider-specific metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }

    /// Sets optional raw response data for each provider call.
    pub fn with_responses(mut self, responses: Vec<Option<EmbeddingModelResponse>>) -> Self {
        self.responses = Some(responses);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{EmbedManyResult, EmbedResult, Embedding};
    use crate::embedding_model::{EmbeddingModelResponse, EmbeddingModelUsage};
    use crate::provider::ProviderMetadata;
    use crate::warning::Warning;
    use serde_json::json;

    #[test]
    fn embed_result_serializes_upstream_shape_with_metadata_and_response() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "embeddingModel": "text-embedding-3-small"
            }
        }))
        .expect("provider metadata deserializes");

        let result = EmbedResult::new("sunrise", vec![0.1, 0.2, 0.3], EmbeddingModelUsage::new(7))
            .with_warning(Warning::Unsupported {
                feature: "truncate".to_string(),
                details: Some("The selected model ignores truncate.".to_string()),
            })
            .with_provider_metadata(provider_metadata)
            .with_response(
                EmbeddingModelResponse::new()
                    .with_header("x-request-id", "req_123")
                    .with_body(json!({ "id": "emb_123" })),
            );

        assert_eq!(
            serde_json::to_value(result).expect("embed result serializes"),
            json!({
                "value": "sunrise",
                "embedding": [0.1, 0.2, 0.3],
                "usage": {
                    "tokens": 7
                },
                "warnings": [
                    {
                        "type": "unsupported",
                        "feature": "truncate",
                        "details": "The selected model ignores truncate."
                    }
                ],
                "providerMetadata": {
                    "openai": {
                        "embeddingModel": "text-embedding-3-small"
                    }
                },
                "response": {
                    "headers": {
                        "x-request-id": "req_123"
                    },
                    "body": {
                        "id": "emb_123"
                    }
                }
            })
        );
    }

    #[test]
    fn embed_result_deserializes_minimal_upstream_shape_and_omits_options() {
        let result: EmbedResult = serde_json::from_value(json!({
            "value": "sunrise",
            "embedding": [0.1, 0.2, 0.3],
            "usage": {
                "tokens": 7
            },
            "warnings": []
        }))
        .expect("embed result deserializes");

        assert_eq!(
            result,
            EmbedResult::new("sunrise", vec![0.1, 0.2, 0.3], EmbeddingModelUsage::new(7))
        );
        assert_eq!(
            serde_json::to_value(result).expect("embed result serializes"),
            json!({
                "value": "sunrise",
                "embedding": [0.1, 0.2, 0.3],
                "usage": {
                    "tokens": 7
                },
                "warnings": []
            })
        );
    }

    #[test]
    fn embed_many_result_serializes_upstream_shape_with_optional_responses() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "dimensions": 3
            }
        }))
        .expect("provider metadata deserializes");
        let embeddings: Vec<Embedding> = vec![vec![0.1, 0.2, 0.3], vec![0.4, 0.5, 0.6]];

        let result = EmbedManyResult::new(
            vec!["sunrise".to_string(), "sunset".to_string()],
            embeddings,
            EmbeddingModelUsage::new(12),
        )
        .with_warning(Warning::Other {
            message: "Provider chunked the request.".to_string(),
        })
        .with_provider_metadata(provider_metadata)
        .with_responses(vec![
            Some(EmbeddingModelResponse::new().with_header("x-request-id", "req_123")),
            None,
        ]);

        assert_eq!(
            serde_json::to_value(result).expect("embed many result serializes"),
            json!({
                "values": ["sunrise", "sunset"],
                "embeddings": [
                    [0.1, 0.2, 0.3],
                    [0.4, 0.5, 0.6]
                ],
                "usage": {
                    "tokens": 12
                },
                "warnings": [
                    {
                        "type": "other",
                        "message": "Provider chunked the request."
                    }
                ],
                "providerMetadata": {
                    "openai": {
                        "dimensions": 3
                    }
                },
                "responses": [
                    {
                        "headers": {
                            "x-request-id": "req_123"
                        }
                    },
                    null
                ]
            })
        );
    }

    #[test]
    fn embed_many_result_deserializes_minimal_upstream_shape_and_omits_options() {
        let result: EmbedManyResult = serde_json::from_value(json!({
            "values": ["sunrise", "sunset"],
            "embeddings": [
                [0.1, 0.2, 0.3],
                [0.4, 0.5, 0.6]
            ],
            "usage": {
                "tokens": 12
            },
            "warnings": []
        }))
        .expect("embed many result deserializes");

        assert_eq!(
            result,
            EmbedManyResult::new(
                vec!["sunrise".to_string(), "sunset".to_string()],
                vec![vec![0.1, 0.2, 0.3], vec![0.4, 0.5, 0.6]],
                EmbeddingModelUsage::new(12)
            )
        );
        assert_eq!(
            serde_json::to_value(result).expect("embed many result serializes"),
            json!({
                "values": ["sunrise", "sunset"],
                "embeddings": [
                    [0.1, 0.2, 0.3],
                    [0.4, 0.5, 0.6]
                ],
                "usage": {
                    "tokens": 12
                },
                "warnings": []
            })
        );
    }
}
