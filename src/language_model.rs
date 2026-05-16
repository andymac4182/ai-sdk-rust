use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

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

/// Provider response metadata for a language model call.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelResponseMetadata {
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
}

/// Strategy for selecting a tool during a language model call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LanguageModelToolChoice {
    /// The model may choose whether to call a tool.
    Auto,

    /// The model must not call a tool.
    None,

    /// The model must call one of the available tools.
    Required,

    /// The model must call a specific tool.
    Tool {
        /// Name of the tool that must be selected.
        #[serde(rename = "toolName")]
        tool_name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{
        FinishReason, InputTokenUsage, LanguageModelFinishReason, LanguageModelResponseMetadata,
        LanguageModelToolChoice, LanguageModelUsage, OutputTokenUsage,
    };
    use serde_json::json;
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

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

    #[test]
    fn response_metadata_uses_upstream_camel_case_and_rfc3339_timestamp() {
        let metadata = LanguageModelResponseMetadata {
            id: Some("resp_123".to_string()),
            timestamp: Some(
                OffsetDateTime::parse("2026-05-16T09:30:00Z", &Rfc3339).expect("timestamp parses"),
            ),
            model_id: Some("openai/gpt-5".to_string()),
        };

        assert_eq!(
            serde_json::to_value(metadata).expect("response metadata serializes"),
            json!({
                "id": "resp_123",
                "timestamp": "2026-05-16T09:30:00Z",
                "modelId": "openai/gpt-5"
            })
        );
    }

    #[test]
    fn response_metadata_deserializes_when_optional_fields_are_missing() {
        let metadata: LanguageModelResponseMetadata = serde_json::from_value(json!({
            "modelId": "provider/model"
        }))
        .expect("response metadata deserializes");

        assert_eq!(
            metadata,
            LanguageModelResponseMetadata {
                model_id: Some("provider/model".to_string()),
                ..LanguageModelResponseMetadata::default()
            }
        );
    }

    #[test]
    fn tool_choice_serializes_upstream_tagged_shapes() {
        assert_eq!(
            serde_json::to_value(LanguageModelToolChoice::Auto).expect("tool choice serializes"),
            json!({ "type": "auto" })
        );
        assert_eq!(
            serde_json::to_value(LanguageModelToolChoice::None).expect("tool choice serializes"),
            json!({ "type": "none" })
        );
        assert_eq!(
            serde_json::to_value(LanguageModelToolChoice::Required)
                .expect("tool choice serializes"),
            json!({ "type": "required" })
        );
        assert_eq!(
            serde_json::to_value(LanguageModelToolChoice::Tool {
                tool_name: "search".to_string(),
            })
            .expect("tool choice serializes"),
            json!({
                "type": "tool",
                "toolName": "search"
            })
        );
    }

    #[test]
    fn tool_choice_deserializes_specific_tool_selection() {
        let tool_choice: LanguageModelToolChoice = serde_json::from_value(json!({
            "type": "tool",
            "toolName": "weather"
        }))
        .expect("tool choice deserializes");

        assert_eq!(
            tool_choice,
            LanguageModelToolChoice::Tool {
                tool_name: "weather".to_string()
            }
        );
    }
}
