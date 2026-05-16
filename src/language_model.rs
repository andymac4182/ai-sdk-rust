use serde::{Deserialize, Serialize};

use crate::json::JsonObject;

/// Unified reason why a language model finished generating a response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FinishReason {
    /// The model generated a stop sequence or otherwise finished normally.
    Stop,
    /// The model reached its maximum output length.
    Length,
    /// A content filter stopped generation.
    ContentFilter,
    /// The model emitted one or more tool calls.
    ToolCalls,
    /// The model stopped because of an error.
    Error,
    /// The provider reported another finish reason.
    Other,
}

/// Finish reason reported for a language model response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LanguageModelFinishReason {
    /// Provider-independent finish reason.
    pub unified: FinishReason,

    /// Provider-specific raw finish reason, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

/// Usage information for input tokens in a language model call.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputTokenUsage {
    /// Total input tokens used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,

    /// Non-cached input tokens used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_cache: Option<u64>,

    /// Cached input tokens read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u64>,

    /// Cached input tokens written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<u64>,
}

/// Usage information for output tokens in a language model call.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTokenUsage {
    /// Total output tokens used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,

    /// Text output tokens used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<u64>,

    /// Reasoning output tokens used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<u64>,
}

/// Usage information for a language model call.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelUsage {
    /// Information about input tokens.
    pub input_tokens: InputTokenUsage,

    /// Information about output tokens.
    pub output_tokens: OutputTokenUsage,

    /// Raw provider usage information.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<JsonObject>,
}

#[cfg(test)]
mod tests {
    use super::{
        FinishReason, InputTokenUsage, LanguageModelFinishReason, LanguageModelUsage,
        OutputTokenUsage,
    };
    use serde_json::json;

    #[test]
    fn finish_reason_uses_upstream_kebab_case_names() {
        let reason = LanguageModelFinishReason {
            unified: FinishReason::ToolCalls,
            raw: Some("tool_calls".to_string()),
        };

        assert_eq!(
            serde_json::to_value(reason).expect("finish reason serializes"),
            json!({
                "unified": "tool-calls",
                "raw": "tool_calls"
            })
        );
    }

    #[test]
    fn usage_uses_upstream_camel_case_token_fields() {
        let usage = LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(120),
                cache_read: Some(40),
                ..InputTokenUsage::default()
            },
            output_tokens: OutputTokenUsage {
                total: Some(32),
                reasoning: Some(8),
                ..OutputTokenUsage::default()
            },
            raw: Some(
                serde_json::from_value(json!({
                    "providerTotal": 152
                }))
                .expect("raw usage is a JSON object"),
            ),
        };

        assert_eq!(
            serde_json::to_value(usage).expect("usage serializes"),
            json!({
                "inputTokens": {
                    "total": 120,
                    "cacheRead": 40
                },
                "outputTokens": {
                    "total": 32,
                    "reasoning": 8
                },
                "raw": {
                    "providerTotal": 152
                }
            })
        );
    }

    #[test]
    fn usage_deserializes_when_optional_counts_are_missing() {
        let usage: LanguageModelUsage = serde_json::from_value(json!({
            "inputTokens": {},
            "outputTokens": {
                "text": 10
            }
        }))
        .expect("usage deserializes");

        assert_eq!(
            usage,
            LanguageModelUsage {
                input_tokens: InputTokenUsage::default(),
                output_tokens: OutputTokenUsage {
                    text: Some(10),
                    ..OutputTokenUsage::default()
                },
                raw: None,
            }
        );
    }
}
