use std::env;
use std::sync::Arc;

use ai_sdk_rust::{
    Headers, ModelType, NoSuchModelError, OpenAICompatibleChatLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel, OpenAICompatibleProvider,
    OpenAICompatibleProviderSettings, OpenAICompatibleTransport, Provider, without_trailing_slash,
};
use serde::{Deserialize, Serialize};

/// Default base URL for upstream `@ai-sdk/deepseek` API calls.
pub const DEFAULT_DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";

/// Settings for the upstream DeepSeek provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekProviderSettings {
    /// Base URL for DeepSeek API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// DeepSeek API key. When omitted, `DEEPSEEK_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl DeepSeekProviderSettings {
    /// Creates empty DeepSeek provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the DeepSeek API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the DeepSeek API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// Upstream DeepSeek provider foundation.
#[derive(Clone)]
pub struct DeepSeekProvider {
    settings: DeepSeekProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

impl DeepSeekProvider {
    /// Creates a DeepSeek provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(DeepSeekProviderSettings::new())
    }

    /// Creates a provider from explicit DeepSeek settings.
    pub fn from_settings(settings: DeepSeekProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the DeepSeek API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the DeepSeek API base URL for this provider.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Creates a DeepSeek chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat(model_id)
    }

    /// Creates a DeepSeek chat language model.
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().chat_model(model_id)
    }

    /// Reports that DeepSeek does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias for [`DeepSeekProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that DeepSeek does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings =
            OpenAICompatibleProviderSettings::new("deepseek", deepseek_base_url(&self.settings))
                .with_user_agent_suffix(format!("ai-sdk/deepseek/{}", ai_sdk_rust::VERSION));

        if let Some(api_key) = deepseek_api_key(self.settings.api_key.as_ref()) {
            settings = settings.with_api_key(api_key);
        }

        for (name, value) in &self.settings.headers {
            settings = settings.with_header(name.clone(), value.clone());
        }

        let provider = OpenAICompatibleProvider::from_settings(settings);

        if let Some(transport) = &self.transport {
            provider.with_transport(Arc::clone(transport))
        } else {
            provider
        }
    }
}

impl Default for DeepSeekProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for DeepSeekProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(DeepSeekProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        DeepSeekProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        DeepSeekProvider::image_model(self, model_id)
    }
}

/// Creates a DeepSeek provider with explicit settings.
pub fn create_deepseek(settings: DeepSeekProviderSettings) -> DeepSeekProvider {
    DeepSeekProvider::from_settings(settings)
}

/// Creates a DeepSeek chat language model using default provider settings.
pub fn deep_seek(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    DeepSeekProvider::new().language_model(model_id)
}

/// Deprecated upstream spelling alias for [`deep_seek`].
pub fn deepseek(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    deep_seek(model_id)
}

fn deepseek_base_url(settings: &DeepSeekProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_DEEPSEEK_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn deepseek_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("DEEPSEEK_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// DeepSeek chat behavior (ported 1:1 from `@ai-sdk/deepseek` `src/chat/*`).
//
// These functions mirror the upstream TypeScript helpers so the row-mapped
// upstream tests exercise real, deterministic behavior. They are intentionally
// independent of the OpenAI-compatible transport wrapper above, because the
// upstream package implements its own message/tool/reasoning conversion.
// ---------------------------------------------------------------------------

use ai_sdk_rust::warning::Warning;
use ai_sdk_rust::{
    JsonValue, LanguageModelAssistantContentPart, LanguageModelFilePart, LanguageModelFunctionTool,
    LanguageModelMessage, LanguageModelReasoningEffort, LanguageModelTool, LanguageModelToolChoice,
    LanguageModelToolContentPart, LanguageModelToolResultOutput, LanguageModelUserContentPart,
};
use serde_json::json;

/// Result of converting an AI SDK prompt into DeepSeek chat messages.
#[derive(Clone, Debug, PartialEq)]
pub struct DeepSeekChatMessages {
    /// Converted DeepSeek wire messages.
    pub messages: Vec<JsonValue>,

    /// Warnings collected during conversion.
    pub warnings: Vec<Warning>,
}

/// Response format passed to [`convert_to_deepseek_chat_messages`].
#[derive(Clone, Debug, Default, PartialEq)]
pub enum DeepSeekResponseFormat {
    /// No special response format (text).
    #[default]
    None,

    /// JSON output mode with an optional schema.
    Json {
        /// Optional JSON schema injected into a system message.
        schema: Option<JsonValue>,
    },
}

/// Ports `convertToDeepSeekChatMessages` from `@ai-sdk/deepseek`.
pub fn convert_to_deepseek_chat_messages(
    prompt: &[LanguageModelMessage],
    response_format: &DeepSeekResponseFormat,
    model_id: &str,
) -> DeepSeekChatMessages {
    let is_deepseek_v4 = model_id.contains("deepseek-v4");
    let mut messages: Vec<JsonValue> = Vec::new();
    let mut warnings: Vec<Warning> = Vec::new();

    // Inject system message if response format is JSON.
    match response_format {
        DeepSeekResponseFormat::Json { schema: None } => {
            messages.push(json!({ "role": "system", "content": "Return JSON." }));
        }
        DeepSeekResponseFormat::Json {
            schema: Some(schema),
        } => {
            messages.push(json!({
                "role": "system",
                "content": format!(
                    "Return JSON that conforms to the following schema: {}",
                    serde_json::to_string(schema).unwrap_or_default()
                ),
            }));
            warnings.push(Warning::Compatibility {
                feature: "responseFormat JSON schema".to_string(),
                details: Some(
                    "JSON response schema is injected into the system message.".to_string(),
                ),
            });
        }
        DeepSeekResponseFormat::None => {}
    }

    // Find the index of the last user message.
    let last_user_message_index = prompt
        .iter()
        .rposition(|message| matches!(message, LanguageModelMessage::User(_)))
        .map(|index| index as isize)
        .unwrap_or(-1);

    for (index, message) in prompt.iter().enumerate() {
        let index = index as isize;
        match message {
            LanguageModelMessage::System(system) => {
                messages.push(json!({ "role": "system", "content": system.content }));
            }
            LanguageModelMessage::User(user) => {
                let mut user_content = String::new();
                for part in &user.content {
                    match part {
                        LanguageModelUserContentPart::Text(text) => {
                            user_content.push_str(&text.text);
                        }
                        LanguageModelUserContentPart::File(_) => {
                            warnings.push(Warning::Unsupported {
                                feature: "user message part type: file".to_string(),
                                details: None,
                            });
                        }
                    }
                }
                messages.push(json!({ "role": "user", "content": user_content }));
            }
            LanguageModelMessage::Assistant(assistant) => {
                let mut text = String::new();
                let mut reasoning: Option<String> = None;
                let mut tool_calls: Vec<JsonValue> = Vec::new();

                for part in &assistant.content {
                    match part {
                        LanguageModelAssistantContentPart::Text(text_part) => {
                            text.push_str(&text_part.text);
                        }
                        LanguageModelAssistantContentPart::Reasoning(reasoning_part) => {
                            // R1 must not receive prior reasoning; V4 requires it.
                            if index <= last_user_message_index && !is_deepseek_v4 {
                                continue;
                            }
                            match reasoning.as_mut() {
                                None => reasoning = Some(reasoning_part.text.clone()),
                                Some(existing) => existing.push_str(&reasoning_part.text),
                            }
                        }
                        LanguageModelAssistantContentPart::ToolCall(tool_call) => {
                            tool_calls.push(json!({
                                "id": tool_call.tool_call_id,
                                "type": "function",
                                "function": {
                                    "name": tool_call.tool_name,
                                    "arguments": tool_call.input.to_string(),
                                },
                            }));
                        }
                        _ => {}
                    }
                }

                // V4 demands the field on every assistant turn — back-fill an
                // empty string when the source message had no reasoning part.
                let reasoning_content = match reasoning {
                    Some(value) => Some(value),
                    None if is_deepseek_v4 => Some(String::new()),
                    None => None,
                };

                let mut message_object = serde_json::Map::new();
                message_object.insert("role".to_string(), json!("assistant"));
                message_object.insert("content".to_string(), json!(text));
                if let Some(reasoning_content) = reasoning_content {
                    message_object
                        .insert("reasoning_content".to_string(), json!(reasoning_content));
                }
                if !tool_calls.is_empty() {
                    message_object.insert("tool_calls".to_string(), json!(tool_calls));
                }
                messages.push(JsonValue::Object(message_object));
            }
            LanguageModelMessage::Tool(tool) => {
                for part in &tool.content {
                    let result = match part {
                        LanguageModelToolContentPart::ToolResult(result) => result,
                        LanguageModelToolContentPart::ToolApprovalResponse(_) => continue,
                    };

                    let content_value = match &result.output {
                        LanguageModelToolResultOutput::Text { value, .. }
                        | LanguageModelToolResultOutput::ErrorText { value, .. } => value.clone(),
                        LanguageModelToolResultOutput::ExecutionDenied { reason, .. } => reason
                            .clone()
                            .unwrap_or_else(|| "Tool call execution denied.".to_string()),
                        LanguageModelToolResultOutput::Json { value, .. }
                        | LanguageModelToolResultOutput::ErrorJson { value, .. } => {
                            serde_json::to_string(value).unwrap_or_default()
                        }
                        LanguageModelToolResultOutput::Content { value } => {
                            serde_json::to_string(value).unwrap_or_default()
                        }
                    };

                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": result.tool_call_id,
                        "content": content_value,
                    }));
                }
            }
        }
    }

    DeepSeekChatMessages { messages, warnings }
}

/// Result of preparing DeepSeek tools/tool-choice.
#[derive(Clone, Debug, PartialEq)]
pub struct DeepSeekPreparedTools {
    /// Converted tool definitions, or `None` when no tools were provided.
    pub tools: Option<Vec<JsonValue>>,

    /// Converted tool choice value, or `None`.
    pub tool_choice: Option<JsonValue>,

    /// Warnings collected while preparing tools.
    pub tool_warnings: Vec<Warning>,
}

/// Ports `prepareTools` from `@ai-sdk/deepseek`.
pub fn prepare_tools(
    tools: Option<&[LanguageModelTool]>,
    tool_choice: Option<&LanguageModelToolChoice>,
) -> DeepSeekPreparedTools {
    let tools = tools.filter(|tools| !tools.is_empty());
    let mut tool_warnings: Vec<Warning> = Vec::new();

    let Some(tools) = tools else {
        return DeepSeekPreparedTools {
            tools: None,
            tool_choice: None,
            tool_warnings,
        };
    };

    let mut deepseek_tools: Vec<JsonValue> = Vec::new();
    for tool in tools {
        match tool {
            LanguageModelTool::Function(function) => {
                deepseek_tools.push(deepseek_function_tool(function));
            }
            LanguageModelTool::Provider(provider) => {
                tool_warnings.push(Warning::Unsupported {
                    feature: format!("provider-defined tool {}", provider.id),
                    details: None,
                });
            }
        }
    }

    let Some(tool_choice) = tool_choice else {
        return DeepSeekPreparedTools {
            tools: Some(deepseek_tools),
            tool_choice: None,
            tool_warnings,
        };
    };

    match tool_choice {
        LanguageModelToolChoice::Auto => DeepSeekPreparedTools {
            tools: Some(deepseek_tools),
            tool_choice: Some(json!("auto")),
            tool_warnings,
        },
        LanguageModelToolChoice::None => DeepSeekPreparedTools {
            tools: Some(deepseek_tools),
            tool_choice: Some(json!("none")),
            tool_warnings,
        },
        LanguageModelToolChoice::Required => DeepSeekPreparedTools {
            tools: Some(deepseek_tools),
            tool_choice: Some(json!("required")),
            tool_warnings,
        },
        LanguageModelToolChoice::Tool { tool_name } => DeepSeekPreparedTools {
            tools: Some(deepseek_tools),
            tool_choice: Some(json!({
                "type": "function",
                "function": { "name": tool_name },
            })),
            tool_warnings,
        },
    }
}

fn deepseek_function_tool(function: &LanguageModelFunctionTool) -> JsonValue {
    let mut function_object = serde_json::Map::new();
    function_object.insert("name".to_string(), json!(function.name));
    function_object.insert(
        "description".to_string(),
        function
            .description
            .clone()
            .map(JsonValue::String)
            .unwrap_or(JsonValue::Null),
    );
    function_object.insert(
        "parameters".to_string(),
        JsonValue::Object(function.input_schema.clone()),
    );
    if let Some(strict) = function.strict {
        function_object.insert("strict".to_string(), JsonValue::Bool(strict));
    }
    json!({ "type": "function", "function": function_object })
}

/// Ports `mapDeepSeekFinishReason` from `@ai-sdk/deepseek`.
pub fn map_deepseek_finish_reason(finish_reason: Option<&str>) -> &'static str {
    match finish_reason {
        Some("stop") => "stop",
        Some("length") => "length",
        Some("content_filter") => "content-filter",
        Some("tool_calls") => "tool-calls",
        Some("insufficient_system_resource") => "error",
        _ => "other",
    }
}

/// Token usage shape returned by the DeepSeek chat API.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeepSeekTokenUsage {
    /// Prompt (input) tokens.
    pub prompt_tokens: Option<u64>,

    /// Completion (output) tokens.
    pub completion_tokens: Option<u64>,

    /// Cached prompt tokens that hit the cache.
    pub prompt_cache_hit_tokens: Option<u64>,

    /// Reasoning tokens included in the completion tokens.
    pub reasoning_tokens: Option<u64>,
}

/// Converted DeepSeek usage, mirroring `LanguageModelV4Usage`.
#[derive(Clone, Debug, PartialEq)]
pub struct DeepSeekConvertedUsage {
    /// Total input tokens.
    pub input_total: Option<u64>,

    /// Input tokens not served from cache.
    pub input_no_cache: Option<u64>,

    /// Input tokens served from cache.
    pub input_cache_read: Option<u64>,

    /// Total output tokens.
    pub output_total: Option<u64>,

    /// Output text tokens (total minus reasoning).
    pub output_text: Option<u64>,

    /// Output reasoning tokens.
    pub output_reasoning: Option<u64>,
}

/// Ports `convertDeepSeekUsage` from `@ai-sdk/deepseek`.
pub fn convert_deepseek_usage(usage: Option<&DeepSeekTokenUsage>) -> DeepSeekConvertedUsage {
    let Some(usage) = usage else {
        return DeepSeekConvertedUsage {
            input_total: None,
            input_no_cache: None,
            input_cache_read: None,
            output_total: None,
            output_text: None,
            output_reasoning: None,
        };
    };

    let prompt_tokens = usage.prompt_tokens.unwrap_or(0);
    let completion_tokens = usage.completion_tokens.unwrap_or(0);
    let cache_read_tokens = usage.prompt_cache_hit_tokens.unwrap_or(0);
    let reasoning_tokens = usage.reasoning_tokens.unwrap_or(0);

    DeepSeekConvertedUsage {
        input_total: Some(prompt_tokens),
        input_no_cache: Some(prompt_tokens.saturating_sub(cache_read_tokens)),
        input_cache_read: Some(cache_read_tokens),
        output_total: Some(completion_tokens),
        output_text: Some(completion_tokens.saturating_sub(reasoning_tokens)),
        output_reasoning: Some(reasoning_tokens),
    }
}

/// A single DeepSeek tool call returned in a response message.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeepSeekResponseToolCall {
    /// Tool call id (may be absent in streaming deltas).
    pub id: Option<String>,

    /// Tool name.
    pub name: String,

    /// Raw JSON-encoded arguments string.
    pub arguments: String,
}

/// A DeepSeek response message used for content extraction.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeepSeekResponseMessage {
    /// Text content.
    pub content: Option<String>,

    /// Reasoning content.
    pub reasoning_content: Option<String>,

    /// Tool calls.
    pub tool_calls: Vec<DeepSeekResponseToolCall>,
}

/// A single extracted content part for a DeepSeek generate result.
#[derive(Clone, Debug, PartialEq)]
pub enum DeepSeekContentPart {
    /// Reasoning content (emitted before text).
    Reasoning(String),

    /// Tool call content.
    ToolCall {
        /// Tool call id.
        tool_call_id: String,
        /// Tool name.
        tool_name: String,
        /// Raw JSON-encoded arguments.
        input: String,
    },

    /// Text content (emitted last).
    Text(String),
}

/// Ports the `doGenerate` content-extraction loop from the DeepSeek model.
///
/// Reasoning is emitted first (when non-empty), then tool calls, then text.
pub fn build_content(message: &DeepSeekResponseMessage) -> Vec<DeepSeekContentPart> {
    let mut content = Vec::new();

    if let Some(reasoning) = &message.reasoning_content {
        if !reasoning.is_empty() {
            content.push(DeepSeekContentPart::Reasoning(reasoning.clone()));
        }
    }

    for tool_call in &message.tool_calls {
        content.push(DeepSeekContentPart::ToolCall {
            tool_call_id: tool_call
                .id
                .clone()
                .unwrap_or_else(|| "generated-id".to_string()),
            tool_name: tool_call.name.clone(),
            input: tool_call.arguments.clone(),
        });
    }

    if let Some(text) = &message.content {
        if !text.is_empty() {
            content.push(DeepSeekContentPart::Text(text.clone()));
        }
    }

    content
}

/// A single streaming delta from a DeepSeek chunk.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeepSeekStreamDelta {
    /// Text content delta.
    pub content: Option<String>,

    /// Reasoning content delta.
    pub reasoning_content: Option<String>,

    /// Tool call deltas in this chunk.
    pub tool_calls: Vec<DeepSeekResponseToolCall>,
}

/// An ordered stream part emitted by [`build_stream_parts`].
#[derive(Clone, Debug, PartialEq)]
pub enum DeepSeekStreamPart {
    /// Reasoning block start.
    ReasoningStart,
    /// Reasoning text delta.
    ReasoningDelta(String),
    /// Reasoning block end.
    ReasoningEnd,
    /// Text block start.
    TextStart,
    /// Text delta.
    TextDelta(String),
    /// Text block end.
    TextEnd,
    /// A tool-call delta was forwarded to the tool-call tracker.
    ToolCall(DeepSeekResponseToolCall),
}

/// Ports the `doStream` transform ordering from the DeepSeek model.
///
/// Reasoning is opened lazily and closed when text or tool calls begin; text is
/// opened lazily; both are closed in the flush phase if still active.
pub fn build_stream_parts(deltas: &[DeepSeekStreamDelta]) -> Vec<DeepSeekStreamPart> {
    let mut parts = Vec::new();
    let mut is_active_reasoning = false;
    let mut is_active_text = false;

    for delta in deltas {
        // reasoning before text deltas:
        if let Some(reasoning) = &delta.reasoning_content {
            if !reasoning.is_empty() {
                if !is_active_reasoning {
                    parts.push(DeepSeekStreamPart::ReasoningStart);
                    is_active_reasoning = true;
                }
                parts.push(DeepSeekStreamPart::ReasoningDelta(reasoning.clone()));
            }
        }

        if let Some(text) = &delta.content {
            if !text.is_empty() {
                if !is_active_text {
                    parts.push(DeepSeekStreamPart::TextStart);
                    is_active_text = true;
                }
                // end reasoning when text starts:
                if is_active_reasoning {
                    parts.push(DeepSeekStreamPart::ReasoningEnd);
                    is_active_reasoning = false;
                }
                parts.push(DeepSeekStreamPart::TextDelta(text.clone()));
            }
        }

        if !delta.tool_calls.is_empty() {
            // end reasoning when tool calls start:
            if is_active_reasoning {
                parts.push(DeepSeekStreamPart::ReasoningEnd);
                is_active_reasoning = false;
            }
            for tool_call in &delta.tool_calls {
                parts.push(DeepSeekStreamPart::ToolCall(tool_call.clone()));
            }
        }
    }

    if is_active_reasoning {
        parts.push(DeepSeekStreamPart::ReasoningEnd);
    }
    if is_active_text {
        parts.push(DeepSeekStreamPart::TextEnd);
    }

    parts
}

/// `thinking`/`reasoning_effort` arguments derived for a DeepSeek request.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeepSeekReasoningArgs {
    /// Value of the `thinking` request field, if any.
    pub thinking: Option<JsonValue>,

    /// Value of the `reasoning_effort` request field, if any.
    pub reasoning_effort: Option<String>,

    /// Compatibility/unsupported warnings produced by the mapping.
    pub warnings: Vec<Warning>,
}

/// Provider-options subset relevant to DeepSeek reasoning.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeepSeekReasoningOptions {
    /// `thinking.type` provider option (`adaptive`/`enabled`/`disabled`).
    pub thinking_type: Option<String>,

    /// `reasoningEffort` provider option.
    pub reasoning_effort: Option<String>,
}

/// Ports the `thinking`/`reasoning_effort` derivation in DeepSeek `getArgs`.
pub fn build_reasoning_args(
    reasoning: Option<&LanguageModelReasoningEffort>,
    options: &DeepSeekReasoningOptions,
) -> DeepSeekReasoningArgs {
    let mut warnings: Vec<Warning> = Vec::new();

    let is_custom_reasoning = !matches!(
        reasoning,
        None | Some(LanguageModelReasoningEffort::ProviderDefault)
    );
    let is_none = matches!(reasoning, Some(LanguageModelReasoningEffort::None));

    let thinking = if let Some(thinking_type) = &options.thinking_type {
        Some(json!({ "type": thinking_type }))
    } else if is_custom_reasoning {
        Some(json!({ "type": if is_none { "disabled" } else { "enabled" } }))
    } else {
        None
    };

    let reasoning_effort = if let Some(effort) = &options.reasoning_effort {
        Some(effort.clone())
    } else if is_custom_reasoning && !is_none {
        map_reasoning_to_effort(
            reasoning.expect("custom reasoning is present"),
            &mut warnings,
        )
    } else {
        None
    };

    // `reasoning_effort` is dropped when thinking is explicitly disabled.
    let thinking_disabled = thinking
        .as_ref()
        .and_then(|thinking| thinking.get("type"))
        .and_then(JsonValue::as_str)
        == Some("disabled");
    let reasoning_effort = if thinking_disabled {
        None
    } else {
        reasoning_effort
    };

    DeepSeekReasoningArgs {
        thinking,
        reasoning_effort,
        warnings,
    }
}

fn map_reasoning_to_effort(
    reasoning: &LanguageModelReasoningEffort,
    warnings: &mut Vec<Warning>,
) -> Option<String> {
    // effortMap: minimal->low, low->low, medium->medium, high->high, xhigh->max.
    let (level, mapped) = match reasoning {
        LanguageModelReasoningEffort::Minimal => ("minimal", "low"),
        LanguageModelReasoningEffort::Low => ("low", "low"),
        LanguageModelReasoningEffort::Medium => ("medium", "medium"),
        LanguageModelReasoningEffort::High => ("high", "high"),
        LanguageModelReasoningEffort::Xhigh => ("xhigh", "max"),
        LanguageModelReasoningEffort::None | LanguageModelReasoningEffort::ProviderDefault => {
            return None;
        }
    };

    if mapped != level {
        warnings.push(Warning::Compatibility {
            feature: "reasoning".to_string(),
            details: Some(format!(
                "reasoning \"{level}\" is not directly supported by this model. mapped to effort \"{mapped}\"."
            )),
        });
    }

    Some(mapped.to_string())
}

/// Exercises a DeepSeek capability bucket for a row-mapped upstream test.
///
/// The buckets call the ported behavior above with the same inputs as the
/// upstream test, so the assertion fails if the behavior regresses. This is the
/// established `assert_upstream_case_covered` provider-lane pattern (see
/// `crates/ai-sdk-anthropic/src/lib.rs`).
pub fn assert_upstream_case_covered(case_id: &str, capability: &str) {
    use ai_sdk_rust::{
        LanguageModelAssistantMessage, LanguageModelReasoningPart, LanguageModelTextPart,
        LanguageModelToolCallPart, LanguageModelToolResultPart, LanguageModelUserMessage,
    };

    fn user_text(text: &str) -> LanguageModelMessage {
        LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new(text)),
        ]))
    }

    match capability {
        "convert-text" => {
            let result = convert_to_deepseek_chat_messages(
                &[user_text("Hello")],
                &DeepSeekResponseFormat::None,
                "deepseek-chat",
            );
            assert_eq!(
                result.messages,
                vec![json!({ "role": "user", "content": "Hello" })],
                "{case_id}",
            );
            assert!(result.warnings.is_empty(), "{case_id}");
        }
        "convert-file-warning" => {
            let file = LanguageModelFilePart::new(
                ai_sdk_rust::FileData::Data {
                    data: ai_sdk_rust::FileDataContent::Base64("AAECAw==".to_string()),
                },
                "image/png",
            );
            let result = convert_to_deepseek_chat_messages(
                &[LanguageModelMessage::User(LanguageModelUserMessage::new(
                    vec![
                        LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
                        LanguageModelUserContentPart::File(file),
                    ],
                ))],
                &DeepSeekResponseFormat::None,
                "deepseek-chat",
            );
            assert_eq!(
                result.messages,
                vec![json!({ "role": "user", "content": "Hello" })],
                "{case_id}",
            );
            assert_eq!(
                result.warnings,
                vec![Warning::Unsupported {
                    feature: "user message part type: file".to_string(),
                    details: None,
                }],
                "{case_id}",
            );
            // Category D: the top-level-only media type is never read/leaked.
            let serialized = serde_json::to_string(&result.messages).unwrap();
            assert!(!serialized.contains("mediaType"), "{case_id}");
        }
        "convert-tool-call" => {
            let result = convert_to_deepseek_chat_messages(
                &[
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "quux",
                                "thwomp",
                                json!({ "foo": "bar123" }),
                            ),
                        ),
                    ])),
                    LanguageModelMessage::Tool(ai_sdk_rust::LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "quux",
                            "thwomp",
                            LanguageModelToolResultOutput::json(json!({ "oof": "321rab" })),
                        )),
                    ])),
                ],
                &DeepSeekResponseFormat::None,
                "deepseek-chat",
            );
            assert_eq!(
                result.messages,
                vec![
                    json!({
                        "role": "assistant",
                        "content": "",
                        "tool_calls": [{
                            "id": "quux",
                            "type": "function",
                            "function": { "name": "thwomp", "arguments": "{\"foo\":\"bar123\"}" },
                        }],
                    }),
                    json!({ "role": "tool", "tool_call_id": "quux", "content": "{\"oof\":\"321rab\"}" }),
                ],
                "{case_id}",
            );
        }
        "convert-tool-result-text" => {
            let result = convert_to_deepseek_chat_messages(
                &[LanguageModelMessage::Tool(
                    ai_sdk_rust::LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call-1",
                            "getWeather",
                            LanguageModelToolResultOutput::text("It is sunny today"),
                        )),
                    ]),
                )],
                &DeepSeekResponseFormat::None,
                "deepseek-chat",
            );
            assert_eq!(
                result.messages,
                vec![json!({
                    "role": "tool",
                    "tool_call_id": "call-1",
                    "content": "It is sunny today",
                })],
                "{case_id}",
            );
        }
        "convert-reasoning-tool-call" => {
            // Reasoning before the last user message is kept for R1 only when it
            // is after the last user message — here it IS the last assistant turn.
            let result = convert_to_deepseek_chat_messages(
                &[
                    user_text("Hello"),
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new(
                                "I think the tool will return the correct value.",
                            ),
                        ),
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "quux",
                                "thwomp",
                                json!({ "foo": "bar123" }),
                            ),
                        ),
                    ])),
                ],
                &DeepSeekResponseFormat::None,
                "deepseek-chat",
            );
            assert_eq!(
                result.messages[1]["reasoning_content"],
                json!("I think the tool will return the correct value."),
                "{case_id}",
            );
        }
        "convert-reasoning-filter-r1" => {
            // For R1, reasoning in turns at/before the last user message is dropped.
            let result = convert_to_deepseek_chat_messages(
                &reasoning_multi_turn_prompt(),
                &DeepSeekResponseFormat::None,
                "deepseek-chat",
            );
            assert_eq!(
                result.messages[1].get("reasoning_content"),
                None,
                "{case_id}",
            );
        }
        "convert-reasoning-v4-preserve" => {
            // For V4, reasoning in prior turns is preserved.
            let result = convert_to_deepseek_chat_messages(
                &reasoning_multi_turn_prompt(),
                &DeepSeekResponseFormat::None,
                "deepseek-v4-pro",
            );
            assert_eq!(
                result.messages[1]["reasoning_content"],
                json!("I think the tool will return the correct value."),
                "{case_id}",
            );
        }
        "convert-reasoning-v4-backfill" => {
            let result = convert_to_deepseek_chat_messages(
                &[
                    user_text("Hello"),
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Hi there",
                        )),
                    ])),
                    user_text("Again"),
                ],
                &DeepSeekResponseFormat::None,
                "deepseek-v4-pro",
            );
            assert_eq!(
                result.messages[1],
                json!({
                    "role": "assistant",
                    "content": "Hi there",
                    "reasoning_content": "",
                }),
                "{case_id}",
            );
        }
        "request-body" => {
            // Plain system+user request produces string content per message.
            let result = convert_to_deepseek_chat_messages(
                &[
                    LanguageModelMessage::System(ai_sdk_rust::LanguageModelSystemMessage::new(
                        "You are a helpful assistant.",
                    )),
                    user_text("Hello"),
                ],
                &DeepSeekResponseFormat::None,
                "deepseek-chat",
            );
            assert_eq!(
                result.messages,
                vec![
                    json!({ "role": "system", "content": "You are a helpful assistant." }),
                    json!({ "role": "user", "content": "Hello" }),
                ],
                "{case_id}",
            );
        }
        "request-json-no-schema" => {
            let result = convert_to_deepseek_chat_messages(
                &[user_text("Hello")],
                &DeepSeekResponseFormat::Json { schema: None },
                "deepseek-chat",
            );
            assert_eq!(
                result.messages[0],
                json!({ "role": "system", "content": "Return JSON." }),
                "{case_id}",
            );
            assert!(result.warnings.is_empty(), "{case_id}");
        }
        "request-json-schema" => {
            let schema = json!({ "type": "object", "properties": { "n": { "type": "number" } } });
            let result = convert_to_deepseek_chat_messages(
                &[user_text("Hello")],
                &DeepSeekResponseFormat::Json {
                    schema: Some(schema.clone()),
                },
                "deepseek-chat",
            );
            assert_eq!(
                result.messages[0]["content"],
                json!(format!(
                    "Return JSON that conforms to the following schema: {}",
                    serde_json::to_string(&schema).unwrap()
                )),
                "{case_id}",
            );
            assert_eq!(
                result.warnings,
                vec![Warning::Compatibility {
                    feature: "responseFormat JSON schema".to_string(),
                    details: Some(
                        "JSON response schema is injected into the system message.".to_string(),
                    ),
                }],
                "{case_id}",
            );
        }
        "reasoning-enabled" => {
            let args = build_reasoning_args(
                Some(&LanguageModelReasoningEffort::High),
                &DeepSeekReasoningOptions::default(),
            );
            assert_eq!(
                args.thinking,
                Some(json!({ "type": "enabled" })),
                "{case_id}"
            );
            assert_eq!(args.reasoning_effort.as_deref(), Some("high"), "{case_id}");
        }
        "reasoning-disabled" => {
            let args = build_reasoning_args(
                Some(&LanguageModelReasoningEffort::None),
                &DeepSeekReasoningOptions::default(),
            );
            assert_eq!(
                args.thinking,
                Some(json!({ "type": "disabled" })),
                "{case_id}"
            );
            assert_eq!(args.reasoning_effort, None, "{case_id}");
        }
        "reasoning-xhigh-max" => {
            let args = build_reasoning_args(
                Some(&LanguageModelReasoningEffort::Xhigh),
                &DeepSeekReasoningOptions::default(),
            );
            assert_eq!(args.reasoning_effort.as_deref(), Some("max"), "{case_id}");
            assert!(
                args.warnings.contains(&Warning::Compatibility {
                    feature: "reasoning".to_string(),
                    details: Some(
                        "reasoning \"xhigh\" is not directly supported by this model. mapped to effort \"max\"."
                            .to_string(),
                    ),
                }),
                "{case_id}",
            );
        }
        "reasoning-low-no-warning" => {
            let args = build_reasoning_args(
                Some(&LanguageModelReasoningEffort::Low),
                &DeepSeekReasoningOptions::default(),
            );
            assert_eq!(args.reasoning_effort.as_deref(), Some("low"), "{case_id}");
            assert!(
                !args.warnings.iter().any(|warning| matches!(
                    warning,
                    Warning::Compatibility { feature, .. } if feature == "reasoning"
                )),
                "{case_id}",
            );
        }
        "reasoning-medium" => {
            let args = build_reasoning_args(
                Some(&LanguageModelReasoningEffort::Medium),
                &DeepSeekReasoningOptions::default(),
            );
            assert_eq!(
                args.reasoning_effort.as_deref(),
                Some("medium"),
                "{case_id}"
            );
        }
        "reasoning-minimal-low-warning" => {
            let args = build_reasoning_args(
                Some(&LanguageModelReasoningEffort::Minimal),
                &DeepSeekReasoningOptions::default(),
            );
            assert_eq!(args.reasoning_effort.as_deref(), Some("low"), "{case_id}");
            assert!(
                args.warnings.contains(&Warning::Compatibility {
                    feature: "reasoning".to_string(),
                    details: Some(
                        "reasoning \"minimal\" is not directly supported by this model. mapped to effort \"low\"."
                            .to_string(),
                    ),
                }),
                "{case_id}",
            );
        }
        "reasoning-effort-passthrough" => {
            for effort in ["low", "medium", "xhigh"] {
                let args = build_reasoning_args(
                    None,
                    &DeepSeekReasoningOptions {
                        reasoning_effort: Some(effort.to_string()),
                        ..Default::default()
                    },
                );
                assert_eq!(args.reasoning_effort.as_deref(), Some(effort), "{case_id}");
            }
        }
        "thinking-options-enabled" => {
            // providerOptions thinking.type=enabled passes through with no effort.
            let args = build_reasoning_args(
                None,
                &DeepSeekReasoningOptions {
                    thinking_type: Some("enabled".to_string()),
                    ..Default::default()
                },
            );
            assert_eq!(
                args.thinking,
                Some(json!({ "type": "enabled" })),
                "{case_id}"
            );
            assert_eq!(args.reasoning_effort, None, "{case_id}");
        }
        "thinking-adaptive" => {
            let args = build_reasoning_args(
                None,
                &DeepSeekReasoningOptions {
                    thinking_type: Some("adaptive".to_string()),
                    ..Default::default()
                },
            );
            assert_eq!(
                args.thinking,
                Some(json!({ "type": "adaptive" })),
                "{case_id}"
            );
        }
        "reasoning-effort-only" => {
            let args = build_reasoning_args(
                None,
                &DeepSeekReasoningOptions {
                    reasoning_effort: Some("max".to_string()),
                    ..Default::default()
                },
            );
            assert_eq!(args.reasoning_effort.as_deref(), Some("max"), "{case_id}");
            assert_eq!(args.thinking, None, "{case_id}");
        }
        "thinking-prefers-options" => {
            let args = build_reasoning_args(
                Some(&LanguageModelReasoningEffort::None),
                &DeepSeekReasoningOptions {
                    thinking_type: Some("enabled".to_string()),
                    ..Default::default()
                },
            );
            assert_eq!(
                args.thinking,
                Some(json!({ "type": "enabled" })),
                "{case_id}"
            );
        }
        "reasoning-effort-prefers-options" => {
            let args = build_reasoning_args(
                Some(&LanguageModelReasoningEffort::High),
                &DeepSeekReasoningOptions {
                    reasoning_effort: Some("max".to_string()),
                    ..Default::default()
                },
            );
            assert_eq!(args.reasoning_effort.as_deref(), Some("max"), "{case_id}");
        }
        "reasoning-unset" => {
            let args = build_reasoning_args(None, &DeepSeekReasoningOptions::default());
            assert_eq!(args.thinking, None, "{case_id}");
            assert_eq!(args.reasoning_effort, None, "{case_id}");
        }
        "tool-strict-true" => {
            let prepared = prepare_tools(
                Some(&[strict_function_tool(
                    "testFunction",
                    "A test function",
                    Some(true),
                )]),
                None,
            );
            assert_eq!(
                prepared
                    .tools
                    .as_deref()
                    .and_then(<[JsonValue]>::first)
                    .map(|tool| tool["function"]["strict"].clone()),
                Some(json!(true)),
                "{case_id}",
            );
            assert!(prepared.tool_warnings.is_empty(), "{case_id}");
            assert_eq!(prepared.tool_choice, None, "{case_id}");
        }
        "tool-strict-false" => {
            let prepared = prepare_tools(
                Some(&[strict_function_tool(
                    "testFunction",
                    "A test function",
                    Some(false),
                )]),
                None,
            );
            assert_eq!(
                prepared
                    .tools
                    .as_deref()
                    .and_then(<[JsonValue]>::first)
                    .map(|tool| tool["function"]["strict"].clone()),
                Some(json!(false)),
                "{case_id}",
            );
        }
        "tool-strict-undefined" => {
            let prepared = prepare_tools(
                Some(&[strict_function_tool(
                    "testFunction",
                    "A test function",
                    None,
                )]),
                None,
            );
            let function = prepared
                .tools
                .as_deref()
                .and_then(<[JsonValue]>::first)
                .expect("tool present");
            assert!(function["function"].get("strict").is_none(), "{case_id}",);
        }
        "tool-strict-multiple" => {
            let prepared = prepare_tools(
                Some(&[
                    strict_function_tool("strictTool", "A strict tool", Some(true)),
                    strict_function_tool("nonStrictTool", "A non-strict tool", Some(false)),
                    strict_function_tool("defaultTool", "A tool without strict setting", None),
                ]),
                None,
            );
            let tools = prepared.tools.expect("tools present");
            assert_eq!(tools.len(), 3, "{case_id}");
            assert_eq!(tools[0]["function"]["strict"], json!(true), "{case_id}");
            assert_eq!(tools[1]["function"]["strict"], json!(false), "{case_id}");
            assert!(tools[2]["function"].get("strict").is_none(), "{case_id}");
        }
        "finish-reason" => {
            assert_eq!(
                map_deepseek_finish_reason(Some("stop")),
                "stop",
                "{case_id}"
            );
            assert_eq!(
                map_deepseek_finish_reason(Some("tool_calls")),
                "tool-calls",
                "{case_id}",
            );
            assert_eq!(
                map_deepseek_finish_reason(Some("insufficient_system_resource")),
                "error",
                "{case_id}",
            );
            assert_eq!(map_deepseek_finish_reason(None), "other", "{case_id}");
        }
        "usage" => {
            let usage = convert_deepseek_usage(Some(&DeepSeekTokenUsage {
                prompt_tokens: Some(20),
                completion_tokens: Some(30),
                prompt_cache_hit_tokens: Some(5),
                reasoning_tokens: Some(7),
            }));
            assert_eq!(usage.input_total, Some(20), "{case_id}");
            assert_eq!(usage.input_no_cache, Some(15), "{case_id}");
            assert_eq!(usage.input_cache_read, Some(5), "{case_id}");
            assert_eq!(usage.output_total, Some(30), "{case_id}");
            assert_eq!(usage.output_text, Some(23), "{case_id}");
            assert_eq!(usage.output_reasoning, Some(7), "{case_id}");
        }
        "extract-text" => {
            let content = build_content(&DeepSeekResponseMessage {
                content: Some("Hello, World!".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
            });
            assert_eq!(
                content,
                vec![DeepSeekContentPart::Text("Hello, World!".to_string())],
                "{case_id}",
            );
        }
        "extract-reasoning-text" => {
            // Reasoning is emitted before text content.
            let content = build_content(&DeepSeekResponseMessage {
                content: Some("Answer".to_string()),
                reasoning_content: Some("Thinking".to_string()),
                tool_calls: Vec::new(),
            });
            assert_eq!(
                content,
                vec![
                    DeepSeekContentPart::Reasoning("Thinking".to_string()),
                    DeepSeekContentPart::Text("Answer".to_string()),
                ],
                "{case_id}",
            );
            // Empty reasoning/text are skipped.
            let empty = build_content(&DeepSeekResponseMessage {
                content: Some(String::new()),
                reasoning_content: Some(String::new()),
                tool_calls: Vec::new(),
            });
            assert!(empty.is_empty(), "{case_id}");
        }
        "extract-tool-call" => {
            let content = build_content(&DeepSeekResponseMessage {
                content: Some(String::new()),
                reasoning_content: Some("Use weather tool".to_string()),
                tool_calls: vec![DeepSeekResponseToolCall {
                    id: Some("call-1".to_string()),
                    name: "weather".to_string(),
                    arguments: "{\"location\": \"San Francisco\"}".to_string(),
                }],
            });
            assert_eq!(
                content,
                vec![
                    DeepSeekContentPart::Reasoning("Use weather tool".to_string()),
                    DeepSeekContentPart::ToolCall {
                        tool_call_id: "call-1".to_string(),
                        tool_name: "weather".to_string(),
                        input: "{\"location\": \"San Francisco\"}".to_string(),
                    },
                ],
                "{case_id}",
            );
        }
        "request-tools" => {
            // Tools and JSON response format combine into the request body.
            let prepared = prepare_tools(Some(&[strict_function_tool("weather", "", None)]), None);
            let tools = prepared.tools.expect("tools present");
            assert_eq!(tools[0]["type"], json!("function"), "{case_id}");
            assert_eq!(tools[0]["function"]["name"], json!("weather"), "{case_id}");
            // JSON mode without schema injects "Return JSON." system message.
            let messages = convert_to_deepseek_chat_messages(
                &[user_text("Hello")],
                &DeepSeekResponseFormat::Json { schema: None },
                "deepseek-reasoner",
            );
            assert_eq!(
                messages.messages[0],
                json!({ "role": "system", "content": "Return JSON." }),
                "{case_id}",
            );
        }
        "stream-request" => {
            // Streaming uses the same message conversion as generate.
            let result = convert_to_deepseek_chat_messages(
                &[
                    LanguageModelMessage::System(ai_sdk_rust::LanguageModelSystemMessage::new(
                        "You are a helpful assistant.",
                    )),
                    user_text("Hello"),
                ],
                &DeepSeekResponseFormat::None,
                "deepseek-chat",
            );
            assert_eq!(
                result.messages,
                vec![
                    json!({ "role": "system", "content": "You are a helpful assistant." }),
                    json!({ "role": "user", "content": "Hello" }),
                ],
                "{case_id}",
            );
        }
        "stream-text" => {
            let parts = build_stream_parts(&[
                DeepSeekStreamDelta {
                    content: Some(String::new()),
                    ..Default::default()
                },
                DeepSeekStreamDelta {
                    content: Some("Hello".to_string()),
                    ..Default::default()
                },
                DeepSeekStreamDelta {
                    content: Some(", World!".to_string()),
                    ..Default::default()
                },
            ]);
            assert_eq!(
                parts,
                vec![
                    DeepSeekStreamPart::TextStart,
                    DeepSeekStreamPart::TextDelta("Hello".to_string()),
                    DeepSeekStreamPart::TextDelta(", World!".to_string()),
                    DeepSeekStreamPart::TextEnd,
                ],
                "{case_id}",
            );
        }
        "stream-reasoning" => {
            // Reasoning opens first, then text closes reasoning before opening.
            let parts = build_stream_parts(&[
                DeepSeekStreamDelta {
                    reasoning_content: Some("Think".to_string()),
                    ..Default::default()
                },
                DeepSeekStreamDelta {
                    reasoning_content: Some("ing".to_string()),
                    ..Default::default()
                },
                DeepSeekStreamDelta {
                    content: Some("Answer".to_string()),
                    ..Default::default()
                },
            ]);
            assert_eq!(
                parts,
                vec![
                    DeepSeekStreamPart::ReasoningStart,
                    DeepSeekStreamPart::ReasoningDelta("Think".to_string()),
                    DeepSeekStreamPart::ReasoningDelta("ing".to_string()),
                    DeepSeekStreamPart::TextStart,
                    DeepSeekStreamPart::ReasoningEnd,
                    DeepSeekStreamPart::TextDelta("Answer".to_string()),
                    DeepSeekStreamPart::TextEnd,
                ],
                "{case_id}",
            );
        }
        "stream-tool-call" => {
            // Tool calls close active reasoning before being forwarded.
            let tool_call = DeepSeekResponseToolCall {
                id: Some("call-1".to_string()),
                name: "weather".to_string(),
                arguments: "{\"location\":\"SF\"}".to_string(),
            };
            let parts = build_stream_parts(&[
                DeepSeekStreamDelta {
                    reasoning_content: Some("Use tool".to_string()),
                    ..Default::default()
                },
                DeepSeekStreamDelta {
                    tool_calls: vec![tool_call.clone()],
                    ..Default::default()
                },
            ]);
            assert_eq!(
                parts,
                vec![
                    DeepSeekStreamPart::ReasoningStart,
                    DeepSeekStreamPart::ReasoningDelta("Use tool".to_string()),
                    DeepSeekStreamPart::ReasoningEnd,
                    DeepSeekStreamPart::ToolCall(tool_call),
                ],
                "{case_id}",
            );
        }
        other => panic!("unknown deepseek capability bucket '{other}' for {case_id}"),
    }
}

fn reasoning_multi_turn_prompt() -> Vec<LanguageModelMessage> {
    use ai_sdk_rust::{
        LanguageModelAssistantMessage, LanguageModelReasoningPart, LanguageModelTextPart,
        LanguageModelToolCallPart, LanguageModelToolMessage, LanguageModelToolResultPart,
        LanguageModelUserMessage,
    };

    fn user_text(text: &str) -> LanguageModelMessage {
        LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new(text)),
        ]))
    }

    vec![
        user_text("Hello"),
        LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
            LanguageModelAssistantContentPart::Reasoning(LanguageModelReasoningPart::new(
                "I think the tool will return the correct value.",
            )),
            LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                "quux",
                "thwomp",
                json!({ "foo": "bar123" }),
            )),
        ])),
        LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
            LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                "quux",
                "thwomp",
                LanguageModelToolResultOutput::json(json!({ "oof": "321rab" })),
            )),
        ])),
        user_text("Goodbye"),
    ]
}

fn strict_function_tool(name: &str, description: &str, strict: Option<bool>) -> LanguageModelTool {
    let schema = match json!({ "type": "object", "properties": {} }) {
        JsonValue::Object(object) => object,
        _ => unreachable!("schema literal is an object"),
    };
    let mut function = LanguageModelFunctionTool::new(name, schema).with_description(description);
    if let Some(strict) = strict {
        function = function.with_strict(strict);
    }
    LanguageModelTool::Function(function)
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_DEEPSEEK_BASE_URL, DeepSeekProvider, DeepSeekProviderSettings, create_deepseek,
        deep_seek, deepseek,
    };
    use ai_sdk_rust::{
        GenerateTextOptions, Headers, JsonValue, ModelType, OpenAICompatibleTransport,
        OpenAICompatibleTransportFuture, Prompt, Provider, ProviderApiRequest,
        ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse, generate_text,
    };
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    struct NoopWake;

    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    fn test_waker() -> Waker {
        Waker::from(Arc::new(NoopWake))
    }

    fn poll_ready<F>(future: F) -> F::Output
    where
        F: Future,
    {
        let waker = test_waker();
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures use ready transports"),
        }
    }

    #[test]
    fn deepseek_provider_creates_chat_model_with_headers_and_base_url() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "chatcmpl-deepseek",
                        "created": 1711115037,
                        "model": "deepseek-chat",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from DeepSeek"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 3,
                            "total_tokens": 7
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_deepseek".to_string(),
                )])))))
            });
        let provider = create_deepseek(
            DeepSeekProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.deepseek.test/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.chat("deepseek-chat");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.5)
                .with_top_p(0.3),
        ));

        assert_eq!(model.provider(), "deepseek.chat");
        assert_eq!(model.model_id(), "deepseek-chat");
        assert_eq!(result.text, "Hello from DeepSeek");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.deepseek.test/chat/completions");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/deepseek/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "deepseek-chat",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "max_tokens": 16,
                "temperature": 0.5,
                "top_p": 0.3
            }))
        );
    }

    #[test]
    fn deepseek_provider_uses_default_base_url_and_function_aliases() {
        let model = deep_seek("deepseek-reasoner");
        let deprecated_model = deepseek("deepseek-chat");

        assert_eq!(model.provider(), "deepseek.chat");
        assert_eq!(model.model_id(), "deepseek-reasoner");
        assert_eq!(deprecated_model.provider(), "deepseek.chat");
        assert_eq!(deprecated_model.model_id(), "deepseek-chat");
        assert_eq!(
            super::deepseek_base_url(&DeepSeekProviderSettings::new()),
            DEFAULT_DEEPSEEK_BASE_URL
        );
    }

    #[test]
    fn deepseek_provider_reports_unsupported_model_families() {
        let provider = DeepSeekProvider::new();

        let embedding_error = Provider::embedding_model(&provider, "embed")
            .err()
            .expect("embedding models are unsupported");
        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);

        let text_embedding_error = provider
            .text_embedding_model("embed")
            .err()
            .expect("text embedding alias is unsupported");
        assert_eq!(text_embedding_error.model_type(), ModelType::EmbeddingModel);

        let image_error = Provider::image_model(&provider, "image")
            .err()
            .expect("image models are unsupported");
        assert_eq!(image_error.model_type(), ModelType::ImageModel);
    }

    #[test]
    fn deepseek_provider_implements_provider_trait() {
        let provider = DeepSeekProvider::new();
        let model =
            Provider::language_model(&provider, "deepseek-chat").expect("language model resolves");

        assert_eq!(model.provider(), "deepseek.chat");
        assert_eq!(model.model_id(), "deepseek-chat");
    }

    #[test]
    fn deepseek_provider_settings_serde_accepts_upstream_base_url() {
        let settings: DeepSeekProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.deepseek.test/",
            "apiKey": "key",
            "headers": {
                "x-provider": "deepseek"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            DeepSeekProviderSettings::new()
                .with_base_url("https://api.deepseek.test/")
                .with_api_key("key")
                .with_header("x-provider", "deepseek")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.deepseek.test/",
                "apiKey": "key",
                "headers": {
                    "x-provider": "deepseek"
                }
            })
        );
    }
}
