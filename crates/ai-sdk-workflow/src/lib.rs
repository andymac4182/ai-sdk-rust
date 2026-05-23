//! Portable workflow helpers for the Rust port of upstream `@ai-sdk/workflow`.

#![forbid(unsafe_code)]

pub mod chat_transport;
pub mod stream_text_iterator;
pub mod workflow_agent;

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

pub use chat_transport::{
    DEFAULT_WORKFLOW_CHAT_API, ReconnectToStreamOptions, SendMessagesOptions, WorkflowChatEnd,
    WorkflowChatRequest, WorkflowChatRequestMethod, WorkflowChatResponse, WorkflowChatTransport,
    WorkflowChatTransportClient, WorkflowChatTransportError, WorkflowChatTransportOptions,
    WorkflowChatTransportResult, WorkflowChatTrigger,
};
pub use stream_text_iterator::{
    DoStreamStepOptions, DoStreamStepOutput, ParsedToolCall, ProviderExecutedToolResult,
    ScriptedStreamTextStepCall, ScriptedStreamTextStepExecutor, StreamFinish, StreamTextIterator,
    StreamTextIteratorYieldValue, WorkflowGenerationSettings, WorkflowModelInfo,
    WorkflowPrepareStepCallback, WorkflowPrepareStepInfo, WorkflowPrepareStepResult,
    WorkflowPrompt, WorkflowRuntimeContext, WorkflowStreamStep, WorkflowStreamStepContent,
    WorkflowStreamTextError, WorkflowStreamTextStepExecutor, WorkflowToolsContext,
    do_stream_step_from_parts, sanitize_provider_metadata_for_tool_call,
};
pub use workflow_agent::{
    WorkflowAgent, WorkflowAgentError, WorkflowAgentFinishInfo, WorkflowAgentOnFinishCallback,
    WorkflowAgentOnStartCallback, WorkflowAgentOnStepFinishCallback,
    WorkflowAgentOnStepStartCallback, WorkflowAgentOnToolExecutionEndCallback,
    WorkflowAgentOnToolExecutionStartCallback, WorkflowAgentOptions, WorkflowAgentStartInfo,
    WorkflowAgentStepStartInfo, WorkflowAgentStreamOptions, WorkflowAgentStreamResult,
    WorkflowAgentToolExecutionEndInfo, WorkflowAgentToolExecutionStartInfo,
};

use ai_sdk_provider::json::{JsonObject, JsonSchema, JsonValue};
use ai_sdk_provider::{
    FileDataContent, LanguageModelFileData, LanguageModelSource, LanguageModelStreamPart,
};
use ai_sdk_provider_utils::{Tool, convert_to_base64};
use ai_sdk_rust::UiMessageChunk;
use serde::{Deserialize, Serialize};

/// The workflow crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Plain-object tool definitions that can cross workflow step boundaries.
pub type SerializableToolSet = BTreeMap<String, SerializableToolDef>;

/// Serializable tool definition.
///
/// This mirrors the portable fields from upstream `SerializableToolDef`.
/// Runtime-only callbacks and executors are intentionally stripped.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializableToolDef {
    /// Function tool description, when one was configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// JSON Schema 7 object describing the tool input.
    pub input_schema: JsonSchema,

    /// Provider tool discriminator.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub tool_type: Option<SerializableToolType>,

    /// Whether a provider tool is executed by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_provider_executed: Option<bool>,

    /// Provider tool identifier, for example `anthropic.web_search_20250305`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Provider tool configuration arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<JsonObject>,
}

impl SerializableToolDef {
    /// Creates a serializable function-tool definition.
    pub fn function(input_schema: JsonSchema) -> Self {
        Self {
            description: None,
            input_schema,
            tool_type: None,
            is_provider_executed: None,
            id: None,
            args: None,
        }
    }

    /// Sets the function tool description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Creates a serializable provider-tool definition.
    pub fn provider(
        id: impl Into<String>,
        args: JsonObject,
        input_schema: JsonSchema,
        is_provider_executed: bool,
    ) -> Self {
        Self {
            description: None,
            input_schema,
            tool_type: Some(SerializableToolType::Provider),
            is_provider_executed: Some(is_provider_executed),
            id: Some(id.into()),
            args: Some(args),
        }
    }
}

/// Serializable tool discriminator.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SerializableToolType {
    /// Provider-defined tool.
    Provider,
}

/// Error returned when a serialized tool cannot be reconstructed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializableToolError {
    /// A provider tool definition omitted its required provider id.
    MissingProviderToolId { tool_name: String },
}

impl fmt::Display for SerializableToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProviderToolId { tool_name } => {
                write!(formatter, "provider tool '{tool_name}' is missing an id")
            }
        }
    }
}

impl Error for SerializableToolError {}

/// Converts runtime tools into plain serializable tool definitions.
///
/// This mirrors upstream `serializeToolSet`: descriptions, input schemas, and
/// provider tool identity survive serialization; runtime callbacks do not.
pub fn serialize_tool_set(tools: impl IntoIterator<Item = Tool>) -> SerializableToolSet {
    tools
        .into_iter()
        .map(|tool| {
            let name = tool.name.clone();
            let mut serializable_tool = SerializableToolDef::function(tool.input_schema.clone());
            serializable_tool.description = tool.description.clone();

            if let Some(provider_tool_id) = tool.provider_tool_id() {
                serializable_tool.tool_type = Some(SerializableToolType::Provider);
                serializable_tool.is_provider_executed = Some(tool.is_provider_executed());
                serializable_tool.id = Some(provider_tool_id.to_string());
                serializable_tool.args = tool.provider_tool_args().cloned();
            }

            (name, serializable_tool)
        })
        .collect()
}

/// Reconstructs workflow tools from serializable definitions.
pub fn resolve_serializable_tools(
    tools: &SerializableToolSet,
) -> Result<BTreeMap<String, Tool>, SerializableToolError> {
    tools
        .iter()
        .map(|(name, tool)| {
            let resolved_tool = match tool.tool_type {
                Some(SerializableToolType::Provider) => {
                    let id = tool.id.clone().ok_or_else(|| {
                        SerializableToolError::MissingProviderToolId {
                            tool_name: name.clone(),
                        }
                    })?;
                    Tool::provider_tool(
                        name.clone(),
                        id,
                        tool.args.clone().unwrap_or_default(),
                        tool.input_schema.clone(),
                        tool.is_provider_executed.unwrap_or(false),
                    )
                }
                None => {
                    let mut function_tool = Tool::new(name.clone(), tool.input_schema.clone());
                    if let Some(description) = &tool.description {
                        function_tool = function_tool.with_description(description.clone());
                    }
                    function_tool
                }
            };

            Ok((name.clone(), resolved_tool))
        })
        .collect()
}

/// Converts one language-model stream part into a UI-message chunk.
///
/// This mirrors upstream workflow `toUIMessageChunk`: stream lifecycle and raw
/// provider chunks do not emit UI chunks, while generated text, reasoning,
/// files, sources, tool input/output, approval requests, and errors do.
pub fn to_ui_message_chunk(part: &LanguageModelStreamPart) -> Option<UiMessageChunk> {
    match part {
        LanguageModelStreamPart::TextStart(part) => Some(UiMessageChunk::TextStart {
            id: part.id.clone(),
            provider_metadata: part.provider_metadata.clone(),
        }),
        LanguageModelStreamPart::TextDelta(part) => Some(UiMessageChunk::TextDelta {
            id: part.id.clone(),
            delta: part.delta.clone(),
            provider_metadata: part.provider_metadata.clone(),
        }),
        LanguageModelStreamPart::TextEnd(part) => Some(UiMessageChunk::TextEnd {
            id: part.id.clone(),
            provider_metadata: part.provider_metadata.clone(),
        }),
        LanguageModelStreamPart::ReasoningStart(part) => Some(UiMessageChunk::ReasoningStart {
            id: part.id.clone(),
            provider_metadata: part.provider_metadata.clone(),
        }),
        LanguageModelStreamPart::ReasoningDelta(part) => Some(UiMessageChunk::ReasoningDelta {
            id: part.id.clone(),
            delta: part.delta.clone(),
            provider_metadata: part.provider_metadata.clone(),
        }),
        LanguageModelStreamPart::ReasoningEnd(part) => Some(UiMessageChunk::ReasoningEnd {
            id: part.id.clone(),
            provider_metadata: part.provider_metadata.clone(),
        }),
        LanguageModelStreamPart::File(part) => Some(UiMessageChunk::File {
            media_type: part.media_type.clone(),
            url: language_model_file_url(&part.media_type, &part.data),
            provider_metadata: part.provider_metadata.clone(),
        }),
        LanguageModelStreamPart::Source(source) => match source {
            LanguageModelSource::Url(source) => Some(UiMessageChunk::SourceUrl {
                source_id: source.id.clone(),
                url: source.url.clone(),
                title: source.title.clone(),
                provider_metadata: source.provider_metadata.clone(),
            }),
            LanguageModelSource::Document(source) => Some(UiMessageChunk::SourceDocument {
                source_id: source.id.clone(),
                media_type: source.media_type.clone(),
                title: source.title.clone(),
                filename: source.filename.clone(),
                provider_metadata: source.provider_metadata.clone(),
            }),
        },
        LanguageModelStreamPart::ToolInputStart(part) => Some(UiMessageChunk::ToolInputStart {
            tool_call_id: part.id.clone(),
            tool_name: part.tool_name.clone(),
            provider_executed: part.provider_executed,
            provider_metadata: part.provider_metadata.clone(),
            dynamic: part.dynamic,
            title: part.title.clone(),
        }),
        LanguageModelStreamPart::ToolInputDelta(part) => Some(UiMessageChunk::ToolInputDelta {
            tool_call_id: part.id.clone(),
            input_text_delta: part.delta.clone(),
        }),
        LanguageModelStreamPart::ToolCall(part) => Some(UiMessageChunk::ToolInputAvailable {
            tool_call_id: part.tool_call_id.clone(),
            tool_name: part.tool_name.clone(),
            input: parse_tool_input(&part.input),
            provider_executed: part.provider_executed,
            provider_metadata: part.provider_metadata.clone(),
            tool_metadata: None,
            dynamic: part.dynamic,
            title: None,
        }),
        LanguageModelStreamPart::ToolResult(part) => Some(UiMessageChunk::ToolOutputAvailable {
            tool_call_id: part.tool_call_id.clone(),
            output: part.result.as_value().clone(),
            provider_executed: None,
            provider_metadata: part.provider_metadata.clone(),
            tool_metadata: None,
            preliminary: part.preliminary,
            dynamic: part.dynamic,
        }),
        LanguageModelStreamPart::ToolApprovalRequest(part) => {
            Some(UiMessageChunk::ToolApprovalRequest {
                approval_id: part.approval_id.clone(),
                tool_call_id: part.tool_call_id.clone(),
                is_automatic: None,
                provider_metadata: part.provider_metadata.clone(),
            })
        }
        LanguageModelStreamPart::Error(part) => Some(UiMessageChunk::Error {
            error_text: json_error_text(&part.error),
        }),
        LanguageModelStreamPart::ToolInputEnd(_)
        | LanguageModelStreamPart::Custom(_)
        | LanguageModelStreamPart::ReasoningFile(_)
        | LanguageModelStreamPart::StreamStart(_)
        | LanguageModelStreamPart::ResponseMetadata(_)
        | LanguageModelStreamPart::Finish(_)
        | LanguageModelStreamPart::Raw(_) => None,
    }
}

/// Converts a model-call stream into UI-message chunks with workflow lifecycle
/// markers around the converted model chunks.
pub fn model_call_stream_to_ui_chunks(
    parts: impl IntoIterator<Item = LanguageModelStreamPart>,
) -> Vec<UiMessageChunk> {
    let mut chunks = vec![UiMessageChunk::start(), UiMessageChunk::start_step()];

    chunks.extend(
        parts
            .into_iter()
            .filter_map(|part| to_ui_message_chunk(&part)),
    );

    chunks.push(UiMessageChunk::finish_step());
    chunks.push(UiMessageChunk::finish());
    chunks
}

fn language_model_file_url(media_type: &str, data: &LanguageModelFileData) -> String {
    match data {
        LanguageModelFileData::Data { data } => {
            format!(
                "data:{media_type};base64,{}",
                file_data_content_base64(data)
            )
        }
        LanguageModelFileData::Url { url } => url.as_str().to_string(),
    }
}

fn file_data_content_base64(data: &FileDataContent) -> String {
    convert_to_base64(data)
}

fn parse_tool_input(input: &str) -> JsonValue {
    serde_json::from_str(input).unwrap_or_else(|_| JsonValue::String(input.to_string()))
}

fn json_error_text(error: &JsonValue) -> String {
    error
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_sdk_provider::json::NonNullJsonValue;
    use ai_sdk_provider::{
        LanguageModelErrorStreamPart, LanguageModelFile, LanguageModelReasoningDelta,
        LanguageModelStreamStart, LanguageModelTextDelta, LanguageModelTextStart,
        LanguageModelToolApprovalRequest, LanguageModelToolCall, LanguageModelToolInputDelta,
        LanguageModelToolInputStart, LanguageModelToolResult, LanguageModelUrlSource,
    };
    use serde_json::json;

    fn weather_schema() -> JsonSchema {
        serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string"
                }
            },
            "required": ["city"]
        }))
        .expect("schema is an object")
    }

    #[test]
    fn serialize_tool_set_serializes_function_tools_with_description_and_input_schema() {
        let tools = vec![
            Tool::new("getWeather", weather_schema()).with_description("Get weather for a city"),
        ];

        assert_eq!(
            serialize_tool_set(tools),
            BTreeMap::from([(
                "getWeather".to_string(),
                SerializableToolDef::function(weather_schema())
                    .with_description("Get weather for a city")
            )])
        );
    }

    #[test]
    fn serialize_tool_set_preserves_provider_tool_identity_and_args() {
        let tools = vec![Tool::provider_tool(
            "webSearch",
            "anthropic.web_search_20250305",
            serde_json::from_value(json!({
                "maxUses": 5,
                "allowedDomains": ["vercel.com", "nextjs.org"]
            }))
            .expect("args are an object"),
            weather_schema(),
            true,
        )];

        let serialized = serialize_tool_set(tools);

        assert_eq!(
            serialized.get("webSearch"),
            Some(&SerializableToolDef::provider(
                "anthropic.web_search_20250305",
                serde_json::from_value(json!({
                    "maxUses": 5,
                    "allowedDomains": ["vercel.com", "nextjs.org"]
                }))
                .expect("args are an object"),
                weather_schema(),
                true,
            ))
        );
    }

    #[test]
    fn resolve_serializable_tools_reconstructs_function_tools() {
        let tools = BTreeMap::from([(
            "getWeather".to_string(),
            SerializableToolDef::function(weather_schema())
                .with_description("Get weather for a city"),
        )]);

        let resolved = resolve_serializable_tools(&tools).expect("tools resolve");
        let tool = resolved.get("getWeather").expect("tool exists");

        assert_eq!(tool.name, "getWeather");
        assert_eq!(tool.description.as_deref(), Some("Get weather for a city"));
        assert_eq!(tool.input_schema, weather_schema());
        assert!(!tool.is_provider_tool());
    }

    #[test]
    fn resolve_serializable_tools_reconstructs_provider_tools() {
        let args: JsonObject = serde_json::from_value(json!({
            "maxUses": 5,
            "allowedDomains": ["vercel.com"]
        }))
        .expect("args are an object");
        let tools = BTreeMap::from([(
            "webSearch".to_string(),
            SerializableToolDef::provider(
                "anthropic.web_search_20250305",
                args.clone(),
                weather_schema(),
                true,
            ),
        )]);

        let resolved = resolve_serializable_tools(&tools).expect("tools resolve");
        let tool = resolved.get("webSearch").expect("tool exists");

        assert!(tool.is_provider_tool());
        assert!(tool.is_provider_executed());
        assert_eq!(
            tool.provider_tool_id(),
            Some("anthropic.web_search_20250305")
        );
        assert_eq!(tool.provider_tool_args(), Some(&args));
    }

    #[test]
    fn resolve_serializable_tools_reports_missing_provider_tool_id() {
        let tools = BTreeMap::from([(
            "webSearch".to_string(),
            SerializableToolDef {
                description: None,
                input_schema: weather_schema(),
                tool_type: Some(SerializableToolType::Provider),
                is_provider_executed: Some(true),
                id: None,
                args: None,
            },
        )]);

        let error = resolve_serializable_tools(&tools).expect_err("provider id is required");
        assert_eq!(
            error,
            SerializableToolError::MissingProviderToolId {
                tool_name: "webSearch".to_string()
            }
        );
    }

    #[test]
    fn to_ui_message_chunk_maps_text_reasoning_and_tool_call_parts() {
        let provider_metadata = BTreeMap::from([(
            "google".to_string(),
            serde_json::from_value(json!({
                "thoughtSignature": "sig_abc123"
            }))
            .expect("provider metadata is an object"),
        )]);

        assert_eq!(
            to_ui_message_chunk(&LanguageModelStreamPart::TextStart(
                LanguageModelTextStart::new("text-1")
                    .with_provider_metadata(provider_metadata.clone())
            )),
            Some(UiMessageChunk::TextStart {
                id: "text-1".to_string(),
                provider_metadata: Some(provider_metadata.clone()),
            })
        );

        assert_eq!(
            to_ui_message_chunk(&LanguageModelStreamPart::ReasoningDelta(
                LanguageModelReasoningDelta::new("reasoning-1", "checking")
                    .with_provider_metadata(provider_metadata.clone())
            )),
            Some(UiMessageChunk::ReasoningDelta {
                id: "reasoning-1".to_string(),
                delta: "checking".to_string(),
                provider_metadata: Some(provider_metadata.clone()),
            })
        );

        assert_eq!(
            to_ui_message_chunk(&LanguageModelStreamPart::ToolInputStart(
                LanguageModelToolInputStart::new("call-1", "getWeather")
                    .with_provider_executed(true)
            )),
            Some(UiMessageChunk::ToolInputStart {
                tool_call_id: "call-1".to_string(),
                tool_name: "getWeather".to_string(),
                provider_executed: Some(true),
                provider_metadata: None,
                dynamic: None,
                title: None,
            })
        );

        assert_eq!(
            to_ui_message_chunk(&LanguageModelStreamPart::ToolInputDelta(
                LanguageModelToolInputDelta::new("call-1", r#"{"city""#)
            )),
            Some(UiMessageChunk::ToolInputDelta {
                tool_call_id: "call-1".to_string(),
                input_text_delta: r#"{"city""#.to_string(),
            })
        );

        assert_eq!(
            to_ui_message_chunk(&LanguageModelStreamPart::ToolCall(
                LanguageModelToolCall::new("call-1", "getWeather", r#"{"city":"Brisbane"}"#)
                    .with_provider_executed(true)
                    .with_provider_metadata(provider_metadata.clone())
            )),
            Some(UiMessageChunk::ToolInputAvailable {
                tool_call_id: "call-1".to_string(),
                tool_name: "getWeather".to_string(),
                input: json!({ "city": "Brisbane" }),
                provider_executed: Some(true),
                provider_metadata: Some(provider_metadata),
                tool_metadata: None,
                dynamic: None,
                title: None,
            })
        );
    }

    #[test]
    fn to_ui_message_chunk_maps_files_sources_results_approval_and_errors() {
        let source_metadata = BTreeMap::from([(
            "gateway".to_string(),
            serde_json::from_value(json!({ "source": "search" }))
                .expect("provider metadata is an object"),
        )]);

        assert_eq!(
            to_ui_message_chunk(&LanguageModelStreamPart::File(LanguageModelFile::new(
                "text/plain",
                LanguageModelFileData::Data {
                    data: FileDataContent::Bytes(b"hi".to_vec()),
                },
            ))),
            Some(UiMessageChunk::File {
                media_type: "text/plain".to_string(),
                url: "data:text/plain;base64,aGk=".to_string(),
                provider_metadata: None,
            })
        );

        assert_eq!(
            to_ui_message_chunk(&LanguageModelStreamPart::Source(LanguageModelSource::Url(
                LanguageModelUrlSource::new("source-1", "https://example.com")
                    .with_title("Example")
                    .with_provider_metadata(source_metadata.clone())
            ))),
            Some(UiMessageChunk::SourceUrl {
                source_id: "source-1".to_string(),
                url: "https://example.com".to_string(),
                title: Some("Example".to_string()),
                provider_metadata: Some(source_metadata),
            })
        );

        assert_eq!(
            to_ui_message_chunk(&LanguageModelStreamPart::Source(
                LanguageModelSource::document("doc-1", "application/pdf", "Spec")
            )),
            Some(UiMessageChunk::SourceDocument {
                source_id: "doc-1".to_string(),
                media_type: "application/pdf".to_string(),
                title: "Spec".to_string(),
                filename: None,
                provider_metadata: None,
            })
        );

        assert_eq!(
            to_ui_message_chunk(&LanguageModelStreamPart::ToolResult(
                LanguageModelToolResult::new(
                    "call-1",
                    "getWeather",
                    NonNullJsonValue::new(json!({ "temperature": 22 }))
                        .expect("tool result is non-null"),
                )
            )),
            Some(UiMessageChunk::ToolOutputAvailable {
                tool_call_id: "call-1".to_string(),
                output: json!({ "temperature": 22 }),
                provider_executed: None,
                provider_metadata: None,
                tool_metadata: None,
                preliminary: None,
                dynamic: None,
            })
        );

        assert_eq!(
            to_ui_message_chunk(&LanguageModelStreamPart::ToolApprovalRequest(
                LanguageModelToolApprovalRequest::new("approval-1", "call-1")
            )),
            Some(UiMessageChunk::ToolApprovalRequest {
                approval_id: "approval-1".to_string(),
                tool_call_id: "call-1".to_string(),
                is_automatic: None,
                provider_metadata: None,
            })
        );

        assert_eq!(
            to_ui_message_chunk(&LanguageModelStreamPart::Error(
                LanguageModelErrorStreamPart::new(json!("model failed"))
            )),
            Some(UiMessageChunk::Error {
                error_text: "model failed".to_string(),
            })
        );
    }

    #[test]
    fn model_call_stream_to_ui_chunks_adds_lifecycle_chunks_and_drops_internal_parts() {
        assert_eq!(
            model_call_stream_to_ui_chunks([
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "hello")),
            ]),
            vec![
                UiMessageChunk::start(),
                UiMessageChunk::start_step(),
                UiMessageChunk::TextDelta {
                    id: "text-1".to_string(),
                    delta: "hello".to_string(),
                    provider_metadata: None,
                },
                UiMessageChunk::finish_step(),
                UiMessageChunk::finish(),
            ]
        );
    }
}
