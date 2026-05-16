use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use url::Url;

use crate::file_data::FileDataContent;
use crate::json::JsonObject;
use crate::provider::ProviderMetadata;

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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelTextKind {
    #[serde(rename = "text")]
    Text,
}

/// Text that the model has generated.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelText {
    #[serde(rename = "type")]
    kind: LanguageModelTextKind,

    /// The text content.
    pub text: String,

    /// Optional provider-specific metadata for the text part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelText {
    /// Creates a generated text part.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelTextKind::Text,
            text: text.into(),
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this generated text part.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelReasoningKind {
    #[serde(rename = "reasoning")]
    Reasoning,
}

/// Reasoning that the model has generated.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelReasoning {
    #[serde(rename = "type")]
    kind: LanguageModelReasoningKind,

    /// The reasoning text content.
    pub text: String,

    /// Optional provider-specific metadata for the reasoning part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelReasoning {
    /// Creates a generated reasoning part.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelReasoningKind::Reasoning,
            text: text.into(),
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this generated reasoning part.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelCustomContentType {
    #[serde(rename = "custom")]
    Custom,
}

/// Provider-specific generated content that does not map to a standardized part.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelCustomContent {
    #[serde(rename = "type")]
    content_type: LanguageModelCustomContentType,

    /// Provider-specific kind in the `{provider}.{provider-type}` format.
    pub kind: String,

    /// Optional provider-specific metadata for the custom content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelCustomContent {
    /// Creates a provider-specific generated content part.
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            content_type: LanguageModelCustomContentType::Custom,
            kind: kind.into(),
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this custom content part.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

/// Generated file data returned by a language model.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LanguageModelFileData {
    /// Raw bytes or base64-encoded generated file content.
    Data { data: FileDataContent },

    /// A URL pointing to the generated file.
    Url { url: Url },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelFileKind {
    #[serde(rename = "file")]
    File,
}

/// A file that the model has generated.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelFile {
    #[serde(rename = "type")]
    kind: LanguageModelFileKind,

    /// The IANA media type of the generated file.
    pub media_type: String,

    /// Generated file data.
    pub data: LanguageModelFileData,

    /// Optional provider-specific metadata for the file part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelFile {
    /// Creates a generated file part.
    pub fn new(media_type: impl Into<String>, data: LanguageModelFileData) -> Self {
        Self {
            kind: LanguageModelFileKind::File,
            media_type: media_type.into(),
            data,
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this generated file part.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelReasoningFileKind {
    #[serde(rename = "reasoning-file")]
    ReasoningFile,
}

/// A file that the model has generated as part of reasoning.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelReasoningFile {
    #[serde(rename = "type")]
    kind: LanguageModelReasoningFileKind,

    /// The IANA media type of the generated reasoning file.
    pub media_type: String,

    /// Generated file data.
    pub data: LanguageModelFileData,

    /// Optional provider-specific metadata for the reasoning file part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelReasoningFile {
    /// Creates a generated reasoning file part.
    pub fn new(media_type: impl Into<String>, data: LanguageModelFileData) -> Self {
        Self {
            kind: LanguageModelReasoningFileKind::ReasoningFile,
            media_type: media_type.into(),
            data,
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this reasoning file part.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum LanguageModelToolApprovalRequestKind {
    #[serde(rename = "tool-approval-request")]
    ToolApprovalRequest,
}

/// Tool approval request emitted for a provider-executed tool call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelToolApprovalRequest {
    #[serde(rename = "type")]
    kind: LanguageModelToolApprovalRequestKind,

    /// Identifier for the approval request.
    pub approval_id: String,

    /// Identifier of the tool call that requires approval.
    pub tool_call_id: String,

    /// Optional provider-specific metadata for the approval request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl LanguageModelToolApprovalRequest {
    /// Creates a provider-executed tool approval request.
    pub fn new(approval_id: impl Into<String>, tool_call_id: impl Into<String>) -> Self {
        Self {
            kind: LanguageModelToolApprovalRequestKind::ToolApprovalRequest,
            approval_id: approval_id.into(),
            tool_call_id: tool_call_id.into(),
            provider_metadata: None,
        }
    }

    /// Adds provider-specific metadata to this tool approval request.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
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
        FinishReason, InputTokenUsage, LanguageModelCustomContent, LanguageModelFile,
        LanguageModelFileData, LanguageModelFinishReason, LanguageModelReasoning,
        LanguageModelReasoningFile, LanguageModelResponseMetadata, LanguageModelText,
        LanguageModelToolApprovalRequest, LanguageModelToolChoice, LanguageModelUsage,
        OutputTokenUsage,
    };
    use crate::file_data::FileDataContent;
    use serde_json::json;
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};
    use url::Url;

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
    fn text_part_serializes_upstream_shape_with_provider_metadata() {
        let text = LanguageModelText::new("Hello").with_provider_metadata(
            serde_json::from_value(json!({
                "openai": {
                    "logprobs": true
                }
            }))
            .expect("provider metadata deserializes"),
        );

        assert_eq!(
            serde_json::to_value(text).expect("text part serializes"),
            json!({
                "type": "text",
                "text": "Hello",
                "providerMetadata": {
                    "openai": {
                        "logprobs": true
                    }
                }
            })
        );
    }

    #[test]
    fn text_part_deserializes_and_omits_missing_provider_metadata() {
        let text: LanguageModelText = serde_json::from_value(json!({
            "type": "text",
            "text": "Hello"
        }))
        .expect("text part deserializes");

        assert_eq!(text, LanguageModelText::new("Hello"));
        assert_eq!(
            serde_json::to_value(text).expect("text part serializes"),
            json!({
                "type": "text",
                "text": "Hello"
            })
        );
    }

    #[test]
    fn reasoning_part_serializes_upstream_shape_with_provider_metadata() {
        let reasoning = LanguageModelReasoning::new("I should check the source.")
            .with_provider_metadata(
                serde_json::from_value(json!({
                    "anthropic": {
                        "signature": "sig_123"
                    }
                }))
                .expect("provider metadata deserializes"),
            );

        assert_eq!(
            serde_json::to_value(reasoning).expect("reasoning part serializes"),
            json!({
                "type": "reasoning",
                "text": "I should check the source.",
                "providerMetadata": {
                    "anthropic": {
                        "signature": "sig_123"
                    }
                }
            })
        );
    }

    #[test]
    fn reasoning_part_rejects_other_content_types() {
        let error = serde_json::from_value::<LanguageModelReasoning>(json!({
            "type": "text",
            "text": "Not reasoning"
        }))
        .expect_err("wrong discriminator is rejected");

        assert!(error.to_string().contains("unknown variant `text`"));
    }

    #[test]
    fn custom_content_serializes_upstream_shape_with_provider_metadata() {
        let custom = LanguageModelCustomContent::new("openai.audio").with_provider_metadata(
            serde_json::from_value(json!({
                "openai": {
                    "format": "wav"
                }
            }))
            .expect("provider metadata deserializes"),
        );

        assert_eq!(
            serde_json::to_value(custom).expect("custom content serializes"),
            json!({
                "type": "custom",
                "kind": "openai.audio",
                "providerMetadata": {
                    "openai": {
                        "format": "wav"
                    }
                }
            })
        );
    }

    #[test]
    fn custom_content_deserializes_and_omits_missing_provider_metadata() {
        let custom: LanguageModelCustomContent = serde_json::from_value(json!({
            "type": "custom",
            "kind": "provider.block"
        }))
        .expect("custom content deserializes");

        assert_eq!(custom, LanguageModelCustomContent::new("provider.block"));
        assert_eq!(
            serde_json::to_value(custom).expect("custom content serializes"),
            json!({
                "type": "custom",
                "kind": "provider.block"
            })
        );
    }

    #[test]
    fn file_part_serializes_upstream_data_shape_with_provider_metadata() {
        let file = LanguageModelFile::new(
            "image/png",
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("iVBORw0KGgo=".to_string()),
            },
        )
        .with_provider_metadata(
            serde_json::from_value(json!({
                "openai": {
                    "fileId": "file_123"
                }
            }))
            .expect("provider metadata deserializes"),
        );

        assert_eq!(
            serde_json::to_value(file).expect("file part serializes"),
            json!({
                "type": "file",
                "mediaType": "image/png",
                "data": {
                    "type": "data",
                    "data": "iVBORw0KGgo="
                },
                "providerMetadata": {
                    "openai": {
                        "fileId": "file_123"
                    }
                }
            })
        );
    }

    #[test]
    fn reasoning_file_part_deserializes_url_data_and_omits_missing_provider_metadata() {
        let reasoning_file: LanguageModelReasoningFile = serde_json::from_value(json!({
            "type": "reasoning-file",
            "mediaType": "application/pdf",
            "data": {
                "type": "url",
                "url": "https://example.com/reasoning.pdf"
            }
        }))
        .expect("reasoning file part deserializes");

        assert_eq!(
            reasoning_file,
            LanguageModelReasoningFile::new(
                "application/pdf",
                LanguageModelFileData::Url {
                    url: Url::parse("https://example.com/reasoning.pdf").expect("valid URL"),
                },
            )
        );
        assert_eq!(
            serde_json::to_value(reasoning_file).expect("reasoning file part serializes"),
            json!({
                "type": "reasoning-file",
                "mediaType": "application/pdf",
                "data": {
                    "type": "url",
                    "url": "https://example.com/reasoning.pdf"
                }
            })
        );
    }

    #[test]
    fn language_model_file_data_rejects_prompt_only_file_variants() {
        let error = serde_json::from_value::<LanguageModelFileData>(json!({
            "type": "reference",
            "reference": {
                "openai": "file_123"
            }
        }))
        .expect_err("reference data is rejected for generated file data");

        assert!(error.to_string().contains("unknown variant `reference`"));
    }

    #[test]
    fn tool_approval_request_serializes_upstream_shape_with_provider_metadata() {
        let request = LanguageModelToolApprovalRequest::new("approval_123", "tool_call_456")
            .with_provider_metadata(
                serde_json::from_value(json!({
                    "openai": {
                        "serverLabel": "mcp-server"
                    }
                }))
                .expect("provider metadata deserializes"),
            );

        assert_eq!(
            serde_json::to_value(request).expect("tool approval request serializes"),
            json!({
                "type": "tool-approval-request",
                "approvalId": "approval_123",
                "toolCallId": "tool_call_456",
                "providerMetadata": {
                    "openai": {
                        "serverLabel": "mcp-server"
                    }
                }
            })
        );
    }

    #[test]
    fn tool_approval_request_deserializes_and_omits_missing_provider_metadata() {
        let request: LanguageModelToolApprovalRequest = serde_json::from_value(json!({
            "type": "tool-approval-request",
            "approvalId": "approval_123",
            "toolCallId": "tool_call_456"
        }))
        .expect("tool approval request deserializes");

        assert_eq!(
            request,
            LanguageModelToolApprovalRequest::new("approval_123", "tool_call_456")
        );
        assert_eq!(
            serde_json::to_value(request).expect("tool approval request serializes"),
            json!({
                "type": "tool-approval-request",
                "approvalId": "approval_123",
                "toolCallId": "tool_call_456"
            })
        );
    }

    #[test]
    fn tool_approval_request_rejects_other_content_types() {
        let error = serde_json::from_value::<LanguageModelToolApprovalRequest>(json!({
            "type": "tool-call",
            "approvalId": "approval_123",
            "toolCallId": "tool_call_456"
        }))
        .expect_err("wrong discriminator is rejected");

        assert!(error.to_string().contains("unknown variant `tool-call`"));
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
