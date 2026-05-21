use std::fmt;

use crate::agent::{ToolLoopAgent, ToolLoopAgentCallOptions, ToolLoopAgentModelSettings};
use crate::file_data::{FileData, ProviderReference};
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    LanguageModel, LanguageModelAssistantContentPart, LanguageModelAssistantMessage,
    LanguageModelCustomPart, LanguageModelFileData, LanguageModelFilePart, LanguageModelMessage,
    LanguageModelPrompt, LanguageModelReasoningFilePart, LanguageModelReasoningPart,
    LanguageModelStreamPart, LanguageModelSystemMessage, LanguageModelTextPart,
    LanguageModelToolApprovalRequestPart, LanguageModelToolApprovalResponsePart,
    LanguageModelToolCallPart, LanguageModelToolContentPart, LanguageModelToolMessage,
    LanguageModelToolResultOutput, LanguageModelToolResultPart, LanguageModelUserContentPart,
    LanguageModelUserMessage,
};
use crate::prompt::Prompt;
use crate::provider::ProviderOptions;
use crate::provider_utils::{ParseJsonResult, normalize_headers, parse_json_event_stream};
use crate::stream_text::StreamTextUiMessageStreamOptions;
use crate::ui_message_stream::{
    UiMessage, UiMessageChunk, UiMessageRole, transform_text_to_ui_message_stream,
};

/// Credentials mode used by upstream browser fetch transports.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequestCredentials {
    Omit,
    SameOrigin,
    Include,
}

/// Chat transport trigger used by upstream `ChatTransport.sendMessages`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChatTransportTrigger {
    SubmitMessage,
    RegenerateMessage,
}

/// Extra request options shared by chat transport sends and reconnects.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequestOptions {
    /// Additional HTTP headers passed to the API endpoint.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,

    /// Additional JSON body properties sent to the API endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonObject>,

    /// Transport-specific request metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
}

impl ChatRequestOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    pub fn with_body(mut self, body: JsonObject) -> Self {
        self.body = Some(body);
        self
    }

    pub fn with_body_property(
        mut self,
        name: impl Into<String>,
        value: impl Into<JsonValue>,
    ) -> Self {
        self.body
            .get_or_insert_with(JsonObject::new)
            .insert(name.into(), value.into());
        self
    }

    pub fn with_metadata(mut self, metadata: impl Into<JsonValue>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }
}

/// Options passed to [`ChatTransport::send_messages`].
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTransportSendOptions {
    pub trigger: ChatTransportTrigger,
    pub chat_id: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,

    #[serde(default)]
    pub messages: Vec<UiMessage>,

    #[serde(default)]
    pub request: ChatRequestOptions,
}

impl ChatTransportSendOptions {
    pub fn new(trigger: ChatTransportTrigger, chat_id: impl Into<String>) -> Self {
        Self {
            trigger,
            chat_id: chat_id.into(),
            message_id: None,
            messages: Vec::new(),
            request: ChatRequestOptions::default(),
        }
    }

    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    pub fn with_messages(mut self, messages: impl IntoIterator<Item = UiMessage>) -> Self {
        self.messages = messages.into_iter().collect();
        self
    }

    pub fn with_request(mut self, request: ChatRequestOptions) -> Self {
        self.request = request;
        self
    }
}

/// Options passed to [`ChatTransport::reconnect_to_stream`].
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTransportReconnectOptions {
    pub chat_id: String,

    #[serde(default)]
    pub request: ChatRequestOptions,
}

impl ChatTransportReconnectOptions {
    pub fn new(chat_id: impl Into<String>) -> Self {
        Self {
            chat_id: chat_id.into(),
            request: ChatRequestOptions::default(),
        }
    }

    pub fn with_request(mut self, request: ChatRequestOptions) -> Self {
        self.request = request;
        self
    }
}

/// Error returned by Rust chat transport implementations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChatTransportError {
    Fetch(String),
    ResponseStatus { status: u16, body: String },
    EmptyBody,
    InvalidMessage(String),
    Agent(String),
}

impl fmt::Display for ChatTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch(message) => formatter.write_str(message),
            Self::ResponseStatus { status, body } => {
                write!(formatter, "chat transport returned status {status}: {body}")
            }
            Self::EmptyBody => formatter.write_str("The response body is empty."),
            Self::InvalidMessage(message) | Self::Agent(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ChatTransportError {}

/// Portable Rust equivalent of upstream `ChatTransport`.
pub trait ChatTransport {
    fn send_messages(
        &self,
        options: ChatTransportSendOptions,
    ) -> Result<Vec<UiMessageChunk>, ChatTransportError>;

    fn reconnect_to_stream(
        &self,
        options: ChatTransportReconnectOptions,
    ) -> Result<Option<Vec<UiMessageChunk>>, ChatTransportError>;
}

/// Constructor options for the Rust equivalent of upstream `DirectChatTransport`.
pub struct DirectChatTransportOptions<'transport, 'agent, M: LanguageModel + ?Sized> {
    pub agent: &'transport ToolLoopAgent<'agent, M>,
    pub model_settings: ToolLoopAgentModelSettings,
    pub ui_message_stream_options: StreamTextUiMessageStreamOptions,
}

impl<'transport, 'agent, M: LanguageModel + ?Sized>
    DirectChatTransportOptions<'transport, 'agent, M>
{
    pub fn new(agent: &'transport ToolLoopAgent<'agent, M>) -> Self {
        Self {
            agent,
            model_settings: ToolLoopAgentModelSettings::default(),
            ui_message_stream_options: StreamTextUiMessageStreamOptions::default(),
        }
    }

    pub fn with_model_settings(mut self, model_settings: ToolLoopAgentModelSettings) -> Self {
        self.model_settings = model_settings;
        self
    }

    pub fn with_ui_message_stream_options(
        mut self,
        ui_message_stream_options: StreamTextUiMessageStreamOptions,
    ) -> Self {
        self.ui_message_stream_options = ui_message_stream_options;
        self
    }
}

/// In-process chat transport that streams directly from a [`ToolLoopAgent`].
///
/// This mirrors upstream `DirectChatTransport` for Rust-native applications:
/// UI messages are validated and converted into model messages, the configured
/// agent is streamed in-process, and the result is converted to UI-message
/// chunks. Browser-only `AbortSignal`, Web `ReadableStream`, and backpressure
/// semantics are intentionally outside this Rust surface.
pub struct DirectChatTransport<'transport, 'agent, M: LanguageModel + ?Sized> {
    agent: &'transport ToolLoopAgent<'agent, M>,
    model_settings: ToolLoopAgentModelSettings,
    ui_message_stream_options: StreamTextUiMessageStreamOptions,
}

impl<'transport, 'agent, M: LanguageModel + ?Sized> DirectChatTransport<'transport, 'agent, M> {
    pub fn new(agent: &'transport ToolLoopAgent<'agent, M>) -> Self {
        Self::with_options(DirectChatTransportOptions::new(agent))
    }

    pub fn with_options(options: DirectChatTransportOptions<'transport, 'agent, M>) -> Self {
        Self {
            agent: options.agent,
            model_settings: options.model_settings,
            ui_message_stream_options: options.ui_message_stream_options,
        }
    }

    pub fn with_model_settings(mut self, model_settings: ToolLoopAgentModelSettings) -> Self {
        self.model_settings = model_settings;
        self
    }

    pub fn with_ui_message_stream_options(
        mut self,
        ui_message_stream_options: StreamTextUiMessageStreamOptions,
    ) -> Self {
        self.ui_message_stream_options = ui_message_stream_options;
        self
    }

    pub async fn send_messages(
        &self,
        options: ChatTransportSendOptions,
    ) -> Result<Vec<UiMessageChunk>, ChatTransportError>
    where
        M::Stream: IntoIterator<Item = LanguageModelStreamPart>,
    {
        let model_messages = convert_ui_messages_to_model_messages(&options.messages)?;
        let prompt = Prompt::from_messages(model_messages).with_allow_system_in_messages(true);
        let call_options =
            ToolLoopAgentCallOptions::new(prompt).with_model_settings(self.model_settings.clone());
        let result = self
            .agent
            .stream(call_options)
            .await
            .map_err(|error| ChatTransportError::Agent(error.to_string()))?;

        Ok(result.to_ui_message_stream_with_options(self.ui_message_stream_options.clone()))
    }

    pub async fn reconnect_to_stream(
        &self,
        _options: ChatTransportReconnectOptions,
    ) -> Result<Option<Vec<UiMessageChunk>>, ChatTransportError> {
        Ok(None)
    }
}

/// Converts portable UI messages into model messages for in-process transports.
pub fn convert_ui_messages_to_model_messages(
    messages: &[UiMessage],
) -> Result<LanguageModelPrompt, ChatTransportError> {
    let mut model_messages = Vec::new();

    for message in messages {
        model_messages.extend(convert_ui_message_to_model_messages(message)?);
    }

    Ok(model_messages)
}

fn convert_ui_message_to_model_messages(
    message: &UiMessage,
) -> Result<Vec<LanguageModelMessage>, ChatTransportError> {
    if message.id.is_empty() {
        return Err(ChatTransportError::InvalidMessage(
            "UI message id must not be empty.".to_string(),
        ));
    }

    match message.role {
        UiMessageRole::System => convert_system_ui_message(message).map(|message| vec![message]),
        UiMessageRole::User => convert_user_ui_message(message).map(|message| vec![message]),
        UiMessageRole::Assistant => convert_assistant_ui_message(message),
    }
}

fn convert_system_ui_message(
    message: &UiMessage,
) -> Result<LanguageModelMessage, ChatTransportError> {
    let mut content = String::new();
    let mut provider_options = ProviderOptions::new();

    for part in &message.parts {
        let kind = ui_message_part_type(part)?;
        match kind {
            "text" => {
                content.push_str(ui_message_text(part)?);
                if let Some(options) = ui_message_provider_options(part)? {
                    provider_options.extend(options);
                }
            }
            kind if ui_message_part_is_data(kind) => {}
            _ => return Err(unsupported_part_error(message, kind)),
        }
    }

    let mut system_message = LanguageModelSystemMessage::new(content);
    if !provider_options.is_empty() {
        system_message = system_message.with_provider_options(provider_options);
    }
    Ok(LanguageModelMessage::System(system_message))
}

fn convert_user_ui_message(
    message: &UiMessage,
) -> Result<LanguageModelMessage, ChatTransportError> {
    let mut content = Vec::new();

    for part in &message.parts {
        let kind = ui_message_part_type(part)?;
        match kind {
            "text" => {
                let mut text_part = LanguageModelTextPart::new(ui_message_text(part)?);
                if let Some(provider_options) = ui_message_provider_options(part)? {
                    text_part = text_part.with_provider_options(provider_options);
                }
                content.push(LanguageModelUserContentPart::Text(text_part));
            }
            "file" => {
                content.push(LanguageModelUserContentPart::File(ui_message_file_part(
                    part,
                )?));
            }
            kind if ui_message_part_is_data(kind) => {}
            _ => return Err(unsupported_part_error(message, kind)),
        }
    }

    Ok(LanguageModelMessage::User(LanguageModelUserMessage::new(
        content,
    )))
}

fn convert_assistant_ui_message(
    message: &UiMessage,
) -> Result<Vec<LanguageModelMessage>, ChatTransportError> {
    let mut model_messages = Vec::new();
    let mut block = Vec::new();

    for part in &message.parts {
        let kind = ui_message_part_type(part)?;
        if kind == "step-start" {
            flush_assistant_ui_message_block(&mut model_messages, &mut block)?;
        } else {
            block.push(part);
        }
    }
    flush_assistant_ui_message_block(&mut model_messages, &mut block)?;

    Ok(model_messages)
}

fn flush_assistant_ui_message_block(
    model_messages: &mut Vec<LanguageModelMessage>,
    block: &mut Vec<&JsonValue>,
) -> Result<(), ChatTransportError> {
    if block.is_empty() {
        return Ok(());
    }

    let mut content = Vec::new();
    let mut tool_content = Vec::new();

    for part in block.iter().copied() {
        let kind = ui_message_part_type(part)?;
        match kind {
            "text" => {
                let mut text_part = LanguageModelTextPart::new(ui_message_text(part)?);
                if let Some(provider_options) = ui_message_provider_options(part)? {
                    text_part = text_part.with_provider_options(provider_options);
                }
                content.push(LanguageModelAssistantContentPart::Text(text_part));
            }
            "reasoning" => {
                let mut reasoning_part = LanguageModelReasoningPart::new(ui_message_text(part)?);
                if let Some(provider_options) = ui_message_provider_options(part)? {
                    reasoning_part = reasoning_part.with_provider_options(provider_options);
                }
                content.push(LanguageModelAssistantContentPart::Reasoning(reasoning_part));
            }
            "reasoning-file" => {
                let mut reasoning_file = LanguageModelReasoningFilePart::new(
                    LanguageModelFileData::Url {
                        url: ui_message_url(part)?,
                    },
                    ui_message_media_type(part)?,
                );
                if let Some(provider_options) = ui_message_provider_options(part)? {
                    reasoning_file = reasoning_file.with_provider_options(provider_options);
                }
                content.push(LanguageModelAssistantContentPart::ReasoningFile(
                    reasoning_file,
                ));
            }
            "custom" => {
                let mut custom_part = LanguageModelCustomPart::new(ui_message_custom_kind(part)?);
                if let Some(provider_options) = ui_message_provider_options(part)? {
                    custom_part = custom_part.with_provider_options(provider_options);
                }
                content.push(LanguageModelAssistantContentPart::Custom(custom_part));
            }
            "file" => {
                content.push(LanguageModelAssistantContentPart::File(
                    ui_message_file_part(part)?,
                ));
            }
            kind if kind == "dynamic-tool" || kind.starts_with("tool-") => {
                convert_assistant_tool_ui_part(part, kind, &mut content, &mut tool_content)?;
            }
            kind if ui_message_part_is_data(kind) => {}
            _ => {
                return Err(ChatTransportError::InvalidMessage(format!(
                    "Unsupported UI message part type `{kind}` for assistant message."
                )));
            }
        }
    }

    model_messages.push(LanguageModelMessage::Assistant(
        LanguageModelAssistantMessage::new(content),
    ));

    if !tool_content.is_empty() {
        model_messages.push(LanguageModelMessage::Tool(LanguageModelToolMessage::new(
            tool_content,
        )));
    }

    block.clear();
    Ok(())
}

fn ui_message_file_part(part: &JsonValue) -> Result<LanguageModelFilePart, ChatTransportError> {
    let mut file_part =
        LanguageModelFilePart::new(ui_message_file_data(part)?, ui_message_media_type(part)?);
    if let Some(filename) = ui_message_optional_string(part, "filename")? {
        file_part = file_part.with_filename(filename);
    }
    if let Some(provider_options) = ui_message_provider_options(part)? {
        file_part = file_part.with_provider_options(provider_options);
    }
    Ok(file_part)
}

fn ui_message_file_data(part: &JsonValue) -> Result<FileData, ChatTransportError> {
    if let Some(provider_reference) = ui_message_provider_reference(part)? {
        return Ok(FileData::Reference {
            reference: provider_reference,
        });
    }

    Ok(FileData::Url {
        url: ui_message_url(part)?,
    })
}

fn ui_message_provider_reference(
    part: &JsonValue,
) -> Result<Option<ProviderReference>, ChatTransportError> {
    let Some(provider_reference) = ui_message_field(part, "providerReference") else {
        return Ok(None);
    };

    serde_json::from_value(provider_reference.clone())
        .map(Some)
        .map_err(|error| {
            ChatTransportError::InvalidMessage(format!(
                "UI file part providerReference must match provider reference shape: {error}"
            ))
        })
}

fn ui_message_url(part: &JsonValue) -> Result<url::Url, ChatTransportError> {
    let url = ui_message_string_field(part, "url")?;
    url::Url::parse(url).map_err(|error| {
        ChatTransportError::InvalidMessage(format!("UI file part url must be a valid URL: {error}"))
    })
}

fn ui_message_media_type(part: &JsonValue) -> Result<&str, ChatTransportError> {
    ui_message_string_field(part, "mediaType")
}

fn ui_message_custom_kind(part: &JsonValue) -> Result<&str, ChatTransportError> {
    ui_message_string_field(part, "kind")
}

fn convert_assistant_tool_ui_part(
    part: &JsonValue,
    kind: &str,
    assistant_content: &mut Vec<LanguageModelAssistantContentPart>,
    tool_content: &mut Vec<LanguageModelToolContentPart>,
) -> Result<(), ChatTransportError> {
    let state = ui_message_string_field(part, "state")?;
    if state == "input-streaming" {
        return Ok(());
    }

    let tool_name = ui_message_tool_name(part, kind)?;
    let tool_call_id = ui_message_string_field(part, "toolCallId")?;
    let provider_executed = ui_message_optional_bool(part, "providerExecuted")?.unwrap_or(false);
    let input = tool_part_input(part, state);
    let call_provider_options =
        ui_message_provider_options_from_field(part, "callProviderMetadata")?;
    let approval = ui_message_tool_approval(part)?;

    let mut tool_call = LanguageModelToolCallPart::new(
        tool_call_id.to_string(),
        tool_name.to_string(),
        input.clone(),
    );
    if provider_executed {
        tool_call = tool_call.with_provider_executed(true);
    }
    if let Some(provider_options) = call_provider_options.clone() {
        tool_call = tool_call.with_provider_options(provider_options);
    }
    assistant_content.push(LanguageModelAssistantContentPart::ToolCall(tool_call));

    if let Some(approval) = &approval {
        let mut approval_request =
            LanguageModelToolApprovalRequestPart::new(&approval.id, tool_call_id);
        if let Some(is_automatic) = approval.is_automatic {
            approval_request = approval_request.with_automatic(is_automatic);
        }
        assistant_content.push(LanguageModelAssistantContentPart::ToolApprovalRequest(
            approval_request,
        ));

        if let Some(approved) = approval.approved {
            let mut approval_response =
                LanguageModelToolApprovalResponsePart::new(&approval.id, approved);
            if provider_executed {
                approval_response = approval_response.with_provider_executed(true);
            }
            if let Some(reason) = &approval.reason {
                approval_response = approval_response.with_reason(reason.clone());
            }
            tool_content.push(LanguageModelToolContentPart::ToolApprovalResponse(
                approval_response,
            ));

            if state == "approval-responded" && !approved {
                let mut output = LanguageModelToolResultOutput::execution_denied();
                if let Some(reason) = &approval.reason {
                    output = output.with_reason(reason.clone());
                }
                let mut result_part = LanguageModelToolResultPart::new(
                    tool_call_id.to_string(),
                    tool_name.to_string(),
                    output,
                );
                if let Some(provider_options) = call_provider_options.clone() {
                    result_part = result_part.with_provider_options(provider_options);
                }
                tool_content.push(LanguageModelToolContentPart::ToolResult(result_part));
            }
        }
    }

    match state {
        "output-available" | "output-error" => {
            let result_provider_options =
                ui_message_provider_options_from_field(part, "resultProviderMetadata")?
                    .or(call_provider_options);
            let output = tool_result_output_from_ui_part(part, state, provider_executed)?;
            let mut result_part = LanguageModelToolResultPart::new(
                tool_call_id.to_string(),
                tool_name.to_string(),
                output,
            );
            if let Some(provider_options) = result_provider_options {
                result_part = result_part.with_provider_options(provider_options);
            }

            if provider_executed {
                assistant_content.push(LanguageModelAssistantContentPart::ToolResult(result_part));
            } else {
                tool_content.push(LanguageModelToolContentPart::ToolResult(result_part));
            }
        }
        "output-denied" => {
            let reason = approval
                .as_ref()
                .and_then(|approval| approval.reason.clone())
                .unwrap_or_else(|| "Tool call execution denied.".to_string());
            let mut result_part = LanguageModelToolResultPart::new(
                tool_call_id.to_string(),
                tool_name.to_string(),
                LanguageModelToolResultOutput::error_text(reason),
            );
            if let Some(provider_options) = call_provider_options {
                result_part = result_part.with_provider_options(provider_options);
            }
            tool_content.push(LanguageModelToolContentPart::ToolResult(result_part));
        }
        "input-available" | "approval-requested" | "approval-responded" => {}
        other => {
            return Err(ChatTransportError::InvalidMessage(format!(
                "Unsupported UI tool part state `{other}`."
            )));
        }
    }

    Ok(())
}

fn ui_message_tool_name<'a>(
    part: &'a JsonValue,
    kind: &'a str,
) -> Result<&'a str, ChatTransportError> {
    if kind == "dynamic-tool" {
        return ui_message_string_field(part, "toolName");
    }

    kind.strip_prefix("tool-")
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            ChatTransportError::InvalidMessage(
                "UI tool part type must include a tool name.".to_string(),
            )
        })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UiToolApproval {
    id: String,
    approved: Option<bool>,
    reason: Option<String>,
    is_automatic: Option<bool>,
}

fn ui_message_tool_approval(
    part: &JsonValue,
) -> Result<Option<UiToolApproval>, ChatTransportError> {
    let Some(approval) = ui_message_field(part, "approval") else {
        return Ok(None);
    };
    let object = approval.as_object().ok_or_else(|| {
        ChatTransportError::InvalidMessage("UI tool approval must be an object.".to_string())
    })?;

    let id = object
        .get("id")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            ChatTransportError::InvalidMessage("UI tool approval id must be a string.".to_string())
        })?
        .to_string();
    let approved = match object.get("approved") {
        Some(value) => Some(value.as_bool().ok_or_else(|| {
            ChatTransportError::InvalidMessage(
                "UI tool approval approved field must be a boolean.".to_string(),
            )
        })?),
        None => None,
    };
    let reason = match object.get("reason") {
        Some(value) if !value.is_null() => Some(
            value
                .as_str()
                .ok_or_else(|| {
                    ChatTransportError::InvalidMessage(
                        "UI tool approval reason must be a string.".to_string(),
                    )
                })?
                .to_string(),
        ),
        _ => None,
    };
    let is_automatic = match object.get("isAutomatic") {
        Some(value) => Some(value.as_bool().ok_or_else(|| {
            ChatTransportError::InvalidMessage(
                "UI tool approval isAutomatic field must be a boolean.".to_string(),
            )
        })?),
        None => None,
    };

    Ok(Some(UiToolApproval {
        id,
        approved,
        reason,
        is_automatic,
    }))
}

fn tool_part_input(part: &JsonValue, state: &str) -> JsonValue {
    if state == "output-error" {
        if let Some(input) = ui_message_field(part, "input").filter(|value| !value.is_null()) {
            return input.clone();
        }
        if let Some(raw_input) = ui_message_field(part, "rawInput") {
            return raw_input.clone();
        }
    }

    ui_message_field(part, "input")
        .cloned()
        .unwrap_or(JsonValue::Null)
}

fn tool_result_output_from_ui_part(
    part: &JsonValue,
    state: &str,
    provider_executed: bool,
) -> Result<LanguageModelToolResultOutput, ChatTransportError> {
    if state == "output-error" {
        let error_text = ui_message_string_field(part, "errorText")?;
        return Ok(if provider_executed {
            LanguageModelToolResultOutput::error_json(JsonValue::String(error_text.to_string()))
        } else {
            LanguageModelToolResultOutput::error_text(error_text)
        });
    }

    let output = ui_message_field(part, "output")
        .cloned()
        .unwrap_or(JsonValue::Null);
    Ok(match output {
        JsonValue::String(value) => LanguageModelToolResultOutput::text(value),
        value => LanguageModelToolResultOutput::json(value),
    })
}

fn ui_message_part_type(part: &JsonValue) -> Result<&str, ChatTransportError> {
    part.as_object()
        .and_then(|object| object.get("type"))
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            ChatTransportError::InvalidMessage(
                "UI message part must be an object with a string type.".to_string(),
            )
        })
}

fn ui_message_part_is_data(kind: &str) -> bool {
    kind.starts_with("data-")
}

fn ui_message_text(part: &JsonValue) -> Result<&str, ChatTransportError> {
    ui_message_string_field(part, "text").map_err(|_| {
        ChatTransportError::InvalidMessage(
            "UI text part must include a string text field.".to_string(),
        )
    })
}

fn ui_message_provider_options(
    part: &JsonValue,
) -> Result<Option<ProviderOptions>, ChatTransportError> {
    ui_message_provider_options_from_field(part, "providerMetadata")
}

fn ui_message_provider_options_from_field(
    part: &JsonValue,
    field: &str,
) -> Result<Option<ProviderOptions>, ChatTransportError> {
    let Some(provider_metadata) = ui_message_field(part, field) else {
        return Ok(None);
    };

    serde_json::from_value(provider_metadata.clone())
        .map(Some)
        .map_err(|error| {
            ChatTransportError::InvalidMessage(format!(
                "UI message providerMetadata must match provider options: {error}"
            ))
        })
}

fn ui_message_field<'a>(part: &'a JsonValue, field: &str) -> Option<&'a JsonValue> {
    part.as_object().and_then(|object| object.get(field))
}

fn ui_message_string_field<'a>(
    part: &'a JsonValue,
    field: &str,
) -> Result<&'a str, ChatTransportError> {
    ui_message_field(part, field)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            ChatTransportError::InvalidMessage(format!(
                "UI message part field `{field}` must be a string."
            ))
        })
}

fn ui_message_optional_string(
    part: &JsonValue,
    field: &str,
) -> Result<Option<String>, ChatTransportError> {
    match ui_message_field(part, field) {
        Some(value) => value
            .as_str()
            .map(|value| Some(value.to_string()))
            .ok_or_else(|| {
                ChatTransportError::InvalidMessage(format!(
                    "UI message part field `{field}` must be a string."
                ))
            }),
        None => Ok(None),
    }
}

fn ui_message_optional_bool(
    part: &JsonValue,
    field: &str,
) -> Result<Option<bool>, ChatTransportError> {
    match ui_message_field(part, field) {
        Some(value) => value.as_bool().map(Some).ok_or_else(|| {
            ChatTransportError::InvalidMessage(format!(
                "UI message part field `{field}` must be a boolean."
            ))
        }),
        None => Ok(None),
    }
}

fn unsupported_part_error(message: &UiMessage, kind: &str) -> ChatTransportError {
    ChatTransportError::InvalidMessage(format!(
        "Unsupported UI message part type `{kind}` for {:?} message.",
        message.role
    ))
}

/// HTTP method used by deterministic chat transport request builders.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpChatTransportMethod {
    Get,
    Post,
}

/// Deterministic HTTP request produced by [`HttpChatTransport`].
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpChatTransportRequest {
    pub method: HttpChatTransportMethod,
    pub api: String,

    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<RequestCredentials>,
}

/// Constructor options for the Rust equivalent of upstream `HttpChatTransport`.
#[derive(Clone, Debug, PartialEq)]
pub struct HttpChatTransportOptions {
    pub api: String,
    pub credentials: Option<RequestCredentials>,
    pub headers: Headers,
    pub body: Option<JsonObject>,
}

impl Default for HttpChatTransportOptions {
    fn default() -> Self {
        Self {
            api: "/api/chat".to_string(),
            credentials: None,
            headers: Headers::new(),
            body: None,
        }
    }
}

impl HttpChatTransportOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_api(mut self, api: impl Into<String>) -> Self {
        self.api = api.into();
        self
    }

    pub fn with_credentials(mut self, credentials: RequestCredentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    pub fn with_body(mut self, body: JsonObject) -> Self {
        self.body = Some(body);
        self
    }

    pub fn with_body_property(
        mut self,
        name: impl Into<String>,
        value: impl Into<JsonValue>,
    ) -> Self {
        self.body
            .get_or_insert_with(JsonObject::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Request object passed to an upstream-style `prepareSendMessagesRequest`.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareSendMessagesRequestOptions {
    pub api: String,
    pub id: String,
    pub messages: Vec<UiMessage>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_metadata: Option<JsonValue>,

    pub body: JsonObject,
    pub headers: Headers,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<RequestCredentials>,

    pub trigger: ChatTransportTrigger,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

/// Request object passed to an upstream-style `prepareReconnectToStreamRequest`.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareReconnectToStreamRequestOptions {
    pub api: String,
    pub id: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_metadata: Option<JsonValue>,

    pub body: JsonObject,
    pub headers: Headers,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<RequestCredentials>,
}

/// Return value from an upstream-style `prepareSendMessagesRequest`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreparedSendMessagesRequest {
    pub api: Option<String>,
    pub headers: Option<Headers>,
    pub credentials: Option<RequestCredentials>,
    pub body: Option<JsonObject>,
}

impl PreparedSendMessagesRequest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_api(mut self, api: impl Into<String>) -> Self {
        self.api = Some(api.into());
        self
    }

    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }

    pub fn with_credentials(mut self, credentials: RequestCredentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    pub fn with_body(mut self, body: JsonObject) -> Self {
        self.body = Some(body);
        self
    }
}

/// Return value from an upstream-style `prepareReconnectToStreamRequest`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreparedReconnectToStreamRequest {
    pub api: Option<String>,
    pub headers: Option<Headers>,
    pub credentials: Option<RequestCredentials>,
}

impl PreparedReconnectToStreamRequest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_api(mut self, api: impl Into<String>) -> Self {
        self.api = Some(api.into());
        self
    }

    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }

    pub fn with_credentials(mut self, credentials: RequestCredentials) -> Self {
        self.credentials = Some(credentials);
        self
    }
}

/// Deterministic Rust equivalent of upstream `HttpChatTransport`.
#[derive(Clone, Debug, PartialEq)]
pub struct HttpChatTransport {
    options: HttpChatTransportOptions,
}

impl HttpChatTransport {
    pub fn new() -> Self {
        Self::with_options(HttpChatTransportOptions::default())
    }

    pub fn with_options(options: HttpChatTransportOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &HttpChatTransportOptions {
        &self.options
    }

    pub fn prepare_send_messages_request_options(
        &self,
        options: &ChatTransportSendOptions,
    ) -> PrepareSendMessagesRequestOptions {
        let body = merged_body(self.options.body.as_ref(), options.request.body.as_ref());

        PrepareSendMessagesRequestOptions {
            api: self.options.api.clone(),
            id: options.chat_id.clone(),
            messages: options.messages.clone(),
            request_metadata: options.request.metadata.clone(),
            body,
            headers: merged_headers(&self.options.headers, &options.request.headers),
            credentials: self.options.credentials,
            trigger: options.trigger,
            message_id: options.message_id.clone(),
        }
    }

    pub fn build_send_messages_request(
        &self,
        options: &ChatTransportSendOptions,
        prepared: Option<PreparedSendMessagesRequest>,
    ) -> HttpChatTransportRequest {
        let prepare_options = self.prepare_send_messages_request_options(options);
        let prepared = prepared.unwrap_or_default();
        let api = prepared.api.unwrap_or_else(|| self.options.api.clone());
        let mut headers = prepared
            .headers
            .map(|headers| normalize_header_map(&headers))
            .unwrap_or_else(|| prepare_options.headers.clone());
        headers
            .entry("content-type".to_string())
            .or_insert_with(|| "application/json".to_string());
        let credentials = prepared.credentials.or(prepare_options.credentials);
        let body = prepared.body.unwrap_or_else(|| {
            default_send_messages_body(
                &prepare_options.body,
                &options.chat_id,
                &options.messages,
                options.trigger,
                options.message_id.as_deref(),
            )
        });

        HttpChatTransportRequest {
            method: HttpChatTransportMethod::Post,
            api,
            headers,
            body: Some(JsonValue::Object(body)),
            credentials,
        }
    }

    pub fn prepare_reconnect_to_stream_request_options(
        &self,
        options: &ChatTransportReconnectOptions,
    ) -> PrepareReconnectToStreamRequestOptions {
        PrepareReconnectToStreamRequestOptions {
            api: self.options.api.clone(),
            id: options.chat_id.clone(),
            request_metadata: options.request.metadata.clone(),
            body: merged_body(self.options.body.as_ref(), options.request.body.as_ref()),
            headers: merged_headers(&self.options.headers, &options.request.headers),
            credentials: self.options.credentials,
        }
    }

    pub fn build_reconnect_to_stream_request(
        &self,
        options: &ChatTransportReconnectOptions,
        prepared: Option<PreparedReconnectToStreamRequest>,
    ) -> HttpChatTransportRequest {
        let prepare_options = self.prepare_reconnect_to_stream_request_options(options);
        let prepared = prepared.unwrap_or_default();
        let api = prepared
            .api
            .unwrap_or_else(|| format!("{}/{}/stream", self.options.api, options.chat_id));
        let headers = prepared
            .headers
            .map(|headers| normalize_header_map(&headers))
            .unwrap_or_else(|| prepare_options.headers.clone());
        let credentials = prepared.credentials.or(prepare_options.credentials);

        HttpChatTransportRequest {
            method: HttpChatTransportMethod::Get,
            api,
            headers,
            body: None,
            credentials,
        }
    }
}

impl Default for HttpChatTransport {
    fn default() -> Self {
        Self::new()
    }
}

/// Rust equivalent of upstream `DefaultChatTransport`.
///
/// Networking is still caller-owned in this crate. This wrapper preserves the
/// deterministic request-building behavior from [`HttpChatTransport`] and adds
/// upstream UI-message JSON event stream parsing.
#[derive(Clone, Debug, PartialEq)]
pub struct DefaultChatTransport {
    transport: HttpChatTransport,
}

impl DefaultChatTransport {
    pub fn new() -> Self {
        Self::with_options(HttpChatTransportOptions::default())
    }

    pub fn with_options(options: HttpChatTransportOptions) -> Self {
        Self {
            transport: HttpChatTransport::with_options(options),
        }
    }

    pub fn options(&self) -> &HttpChatTransportOptions {
        self.transport.options()
    }

    pub fn prepare_send_messages_request_options(
        &self,
        options: &ChatTransportSendOptions,
    ) -> PrepareSendMessagesRequestOptions {
        self.transport
            .prepare_send_messages_request_options(options)
    }

    pub fn build_send_messages_request(
        &self,
        options: &ChatTransportSendOptions,
        prepared: Option<PreparedSendMessagesRequest>,
    ) -> HttpChatTransportRequest {
        self.transport
            .build_send_messages_request(options, prepared)
    }

    pub fn prepare_reconnect_to_stream_request_options(
        &self,
        options: &ChatTransportReconnectOptions,
    ) -> PrepareReconnectToStreamRequestOptions {
        self.transport
            .prepare_reconnect_to_stream_request_options(options)
    }

    pub fn build_reconnect_to_stream_request(
        &self,
        options: &ChatTransportReconnectOptions,
        prepared: Option<PreparedReconnectToStreamRequest>,
    ) -> HttpChatTransportRequest {
        self.transport
            .build_reconnect_to_stream_request(options, prepared)
    }

    pub fn process_response_event_stream<B>(
        &self,
        chunks: impl IntoIterator<Item = B>,
    ) -> Result<Vec<UiMessageChunk>, ChatTransportError>
    where
        B: AsRef<[u8]>,
    {
        parse_ui_message_event_stream(chunks)
    }
}

impl Default for DefaultChatTransport {
    fn default() -> Self {
        Self::new()
    }
}

/// Rust equivalent of upstream `TextStreamChatTransport`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStreamChatTransport {
    transport: HttpChatTransport,
}

impl TextStreamChatTransport {
    pub fn new() -> Self {
        Self::with_options(HttpChatTransportOptions::default())
    }

    pub fn with_options(options: HttpChatTransportOptions) -> Self {
        Self {
            transport: HttpChatTransport::with_options(options),
        }
    }

    pub fn options(&self) -> &HttpChatTransportOptions {
        self.transport.options()
    }

    pub fn prepare_send_messages_request_options(
        &self,
        options: &ChatTransportSendOptions,
    ) -> PrepareSendMessagesRequestOptions {
        self.transport
            .prepare_send_messages_request_options(options)
    }

    pub fn build_send_messages_request(
        &self,
        options: &ChatTransportSendOptions,
        prepared: Option<PreparedSendMessagesRequest>,
    ) -> HttpChatTransportRequest {
        self.transport
            .build_send_messages_request(options, prepared)
    }

    pub fn prepare_reconnect_to_stream_request_options(
        &self,
        options: &ChatTransportReconnectOptions,
    ) -> PrepareReconnectToStreamRequestOptions {
        self.transport
            .prepare_reconnect_to_stream_request_options(options)
    }

    pub fn build_reconnect_to_stream_request(
        &self,
        options: &ChatTransportReconnectOptions,
        prepared: Option<PreparedReconnectToStreamRequest>,
    ) -> HttpChatTransportRequest {
        self.transport
            .build_reconnect_to_stream_request(options, prepared)
    }

    pub fn process_text_response_stream<S>(
        &self,
        chunks: impl IntoIterator<Item = S>,
    ) -> Vec<UiMessageChunk>
    where
        S: Into<String>,
    {
        transform_text_to_ui_message_stream(chunks)
    }
}

impl Default for TextStreamChatTransport {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_ui_message_event_stream<B>(
    chunks: impl IntoIterator<Item = B>,
) -> Result<Vec<UiMessageChunk>, ChatTransportError>
where
    B: AsRef<[u8]>,
{
    let parsed_chunks = parse_json_event_stream(chunks, |value| {
        serde_json::from_value::<UiMessageChunk>(value.clone())
    });
    let mut chunks = Vec::new();

    for parsed_chunk in parsed_chunks {
        match parsed_chunk {
            ParseJsonResult::Success { value, .. } => chunks.push(value),
            ParseJsonResult::Failure { error, .. } => {
                return Err(ChatTransportError::InvalidMessage(format!(
                    "UI message stream chunk parse failed: {error}"
                )));
            }
        }
    }

    Ok(chunks)
}

fn merged_body(base: Option<&JsonObject>, overrides: Option<&JsonObject>) -> JsonObject {
    let mut body = base.cloned().unwrap_or_default();
    if let Some(overrides) = overrides {
        body.extend(overrides.clone());
    }
    body
}

fn merged_headers(base: &Headers, overrides: &Headers) -> Headers {
    let mut headers = normalize_header_map(base);
    headers.extend(normalize_header_map(overrides));
    headers
}

fn normalize_header_map(headers: &Headers) -> Headers {
    normalize_headers(Some(
        headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    ))
}

fn default_send_messages_body(
    base_body: &JsonObject,
    chat_id: &str,
    messages: &[UiMessage],
    trigger: ChatTransportTrigger,
    message_id: Option<&str>,
) -> JsonObject {
    let mut body = base_body.clone();
    body.insert("id".to_string(), JsonValue::String(chat_id.to_string()));
    body.insert(
        "messages".to_string(),
        serde_json::to_value(messages).expect("UI messages serialize"),
    );
    body.insert(
        "trigger".to_string(),
        serde_json::to_value(trigger).expect("chat transport trigger serializes"),
    );
    if let Some(message_id) = message_id {
        body.insert(
            "messageId".to_string(),
            JsonValue::String(message_id.to_string()),
        );
    }
    body
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    use super::*;
    use crate::agent::{ToolLoopAgent, ToolLoopAgentSettings};
    use crate::language_model::{
        FinishReason, InputTokenUsage, LanguageModelFinishReason, LanguageModelReasoningDelta,
        LanguageModelReasoningEnd, LanguageModelReasoningStart, LanguageModelStreamFinish,
        LanguageModelStreamResult, LanguageModelStreamStart, LanguageModelTextDelta,
        LanguageModelTextEnd, LanguageModelTextStart, LanguageModelUsage, OutputTokenUsage,
    };
    use crate::mock_models::MockLanguageModel;
    use serde_json::json;

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Future::poll(Pin::as_mut(&mut future), &mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("future unexpectedly pending"),
        }
    }

    fn usage() -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: Some(3),
                no_cache: Some(3),
                cache_read: None,
                cache_write: None,
            },
            output_tokens: OutputTokenUsage {
                total: Some(10),
                text: Some(10),
                reasoning: None,
            },
            raw: None,
        }
    }

    fn finish_reason() -> LanguageModelFinishReason {
        LanguageModelFinishReason {
            unified: FinishReason::Stop,
            raw: Some("stop".to_string()),
        }
    }

    fn text_stream_result(
        deltas: impl IntoIterator<Item = &'static str>,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let mut parts = vec![
            LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
        ];
        parts.extend(deltas.into_iter().map(|delta| {
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", delta))
        }));
        parts.extend([
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ]);

        LanguageModelStreamResult::new(parts)
    }

    fn reasoning_stream_result() -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
            LanguageModelStreamPart::ReasoningStart(LanguageModelReasoningStart::new("r1")),
            LanguageModelStreamPart::ReasoningDelta(LanguageModelReasoningDelta::new(
                "r1",
                "thinking...",
            )),
            LanguageModelStreamPart::ReasoningEnd(LanguageModelReasoningEnd::new("r1")),
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("1")),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("1", "result")),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("1")),
            LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
                usage(),
                finish_reason(),
            )),
        ])
    }

    fn user_text_message(id: &str, text: &str) -> UiMessage {
        UiMessage::new(id, UiMessageRole::User).with_part(json!({ "type": "text", "text": text }))
    }

    #[test]
    fn chat_request_options_serialize_upstream_shape() {
        let options = ChatRequestOptions::new()
            .with_header("X-Test", "yes")
            .with_body_property("sessionId", json!("session-1"))
            .with_metadata(json!({ "source": "unit-test" }));

        assert_eq!(
            serde_json::to_value(options).expect("options serialize"),
            json!({
                "headers": { "X-Test": "yes" },
                "body": { "sessionId": "session-1" },
                "metadata": { "source": "unit-test" }
            })
        );
    }

    #[test]
    fn http_chat_transport_builds_default_send_messages_request() {
        let transport = HttpChatTransport::with_options(
            HttpChatTransportOptions::new()
                .with_api("https://example.test/api/chat")
                .with_credentials(RequestCredentials::Include)
                .with_header("X-Transport", "base")
                .with_header("X-Override", "base")
                .with_body_property("someData", json!(true))
                .with_body_property("overlap", json!("base")),
        );
        let send = ChatTransportSendOptions::new(ChatTransportTrigger::SubmitMessage, "chat-123")
            .with_message_id("message-123")
            .with_messages([UiMessage::new("message-123", crate::UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "Hello, world!" }))])
            .with_request(
                ChatRequestOptions::new()
                    .with_header("X-Override", "request")
                    .with_body_property("overlap", json!("request")),
            );

        let request = transport.build_send_messages_request(&send, None);

        assert_eq!(request.method, HttpChatTransportMethod::Post);
        assert_eq!(request.api, "https://example.test/api/chat");
        assert_eq!(request.credentials, Some(RequestCredentials::Include));
        assert_eq!(
            request.headers,
            Headers::from([
                ("content-type".to_string(), "application/json".to_string()),
                ("x-override".to_string(), "request".to_string()),
                ("x-transport".to_string(), "base".to_string()),
            ])
        );
        assert_eq!(
            request.body,
            Some(json!({
                "id": "chat-123",
                "messageId": "message-123",
                "messages": [
                    {
                        "id": "message-123",
                        "role": "user",
                        "parts": [{ "type": "text", "text": "Hello, world!" }]
                    }
                ],
                "overlap": "request",
                "someData": true,
                "trigger": "submit-message"
            }))
        );
    }

    #[test]
    fn http_chat_transport_prepare_send_options_match_upstream_callback_input() {
        let transport = HttpChatTransport::with_options(
            HttpChatTransportOptions::new()
                .with_api("/custom/chat")
                .with_credentials(RequestCredentials::SameOrigin)
                .with_header("X-Base", "base")
                .with_body_property("base", json!(true)),
        );
        let send =
            ChatTransportSendOptions::new(ChatTransportTrigger::RegenerateMessage, "chat-123")
                .with_request(
                    ChatRequestOptions::new()
                        .with_header("X-Request", "request")
                        .with_body_property("request", json!(true))
                        .with_metadata(json!({ "trace": "trace-1" })),
                );

        let options = transport.prepare_send_messages_request_options(&send);

        assert_eq!(options.api, "/custom/chat");
        assert_eq!(options.id, "chat-123");
        assert_eq!(options.trigger, ChatTransportTrigger::RegenerateMessage);
        assert_eq!(options.credentials, Some(RequestCredentials::SameOrigin));
        assert_eq!(
            options.headers,
            Headers::from([
                ("x-base".to_string(), "base".to_string()),
                ("x-request".to_string(), "request".to_string()),
            ])
        );
        assert_eq!(
            options.body,
            JsonObject::from_iter([
                ("base".to_string(), json!(true)),
                ("request".to_string(), json!(true)),
            ])
        );
        assert_eq!(
            options.request_metadata,
            Some(json!({ "trace": "trace-1" }))
        );
    }

    #[test]
    fn http_chat_transport_prepared_send_request_overrides_defaults() {
        let transport = HttpChatTransport::with_options(
            HttpChatTransportOptions::new()
                .with_api("/api/chat")
                .with_credentials(RequestCredentials::SameOrigin)
                .with_header("X-Base", "base")
                .with_body_property("base", json!(true)),
        );
        let send = ChatTransportSendOptions::new(ChatTransportTrigger::SubmitMessage, "chat-123");
        let request = transport.build_send_messages_request(
            &send,
            Some(
                PreparedSendMessagesRequest::new()
                    .with_api("/prepared/chat")
                    .with_headers(Headers::from([(
                        "X-Prepared".to_string(),
                        "prepared".to_string(),
                    )]))
                    .with_credentials(RequestCredentials::Omit)
                    .with_body(JsonObject::from_iter([(
                        "prepared".to_string(),
                        json!(true),
                    )])),
            ),
        );

        assert_eq!(request.api, "/prepared/chat");
        assert_eq!(request.credentials, Some(RequestCredentials::Omit));
        assert_eq!(
            request.headers,
            Headers::from([
                ("content-type".to_string(), "application/json".to_string()),
                ("x-prepared".to_string(), "prepared".to_string()),
            ])
        );
        assert_eq!(request.body, Some(json!({ "prepared": true })));
    }

    #[test]
    fn http_chat_transport_builds_default_reconnect_request() {
        let transport = HttpChatTransport::with_options(
            HttpChatTransportOptions::new()
                .with_api("/api/chat")
                .with_credentials(RequestCredentials::Include)
                .with_header("X-Transport", "base"),
        );
        let reconnect = ChatTransportReconnectOptions::new("chat-123").with_request(
            ChatRequestOptions::new()
                .with_header("X-Request", "request")
                .with_body_property("ignoredByFetch", json!(true))
                .with_metadata(json!({ "trace": "trace-1" })),
        );

        let prepare_options = transport.prepare_reconnect_to_stream_request_options(&reconnect);
        let request = transport.build_reconnect_to_stream_request(&reconnect, None);

        assert_eq!(prepare_options.id, "chat-123");
        assert_eq!(prepare_options.body["ignoredByFetch"], json!(true));
        assert_eq!(
            prepare_options.request_metadata,
            Some(json!({ "trace": "trace-1" }))
        );
        assert_eq!(request.method, HttpChatTransportMethod::Get);
        assert_eq!(request.api, "/api/chat/chat-123/stream");
        assert_eq!(request.credentials, Some(RequestCredentials::Include));
        assert_eq!(
            request.headers,
            Headers::from([
                ("x-request".to_string(), "request".to_string()),
                ("x-transport".to_string(), "base".to_string()),
            ])
        );
        assert_eq!(request.body, None);
    }

    #[test]
    fn http_chat_transport_prepared_reconnect_request_overrides_defaults() {
        let transport = HttpChatTransport::with_options(
            HttpChatTransportOptions::new()
                .with_api("/api/chat")
                .with_credentials(RequestCredentials::SameOrigin)
                .with_header("X-Base", "base"),
        );
        let reconnect = ChatTransportReconnectOptions::new("chat-123");

        let request = transport.build_reconnect_to_stream_request(
            &reconnect,
            Some(
                PreparedReconnectToStreamRequest::new()
                    .with_api("/prepared/stream")
                    .with_headers(Headers::from([(
                        "X-Prepared".to_string(),
                        "prepared".to_string(),
                    )]))
                    .with_credentials(RequestCredentials::Omit),
            ),
        );

        assert_eq!(request.method, HttpChatTransportMethod::Get);
        assert_eq!(request.api, "/prepared/stream");
        assert_eq!(request.credentials, Some(RequestCredentials::Omit));
        assert_eq!(
            request.headers,
            Headers::from([("x-prepared".to_string(), "prepared".to_string())])
        );
    }

    #[test]
    fn default_chat_transport_parses_ui_message_event_stream() {
        let transport = DefaultChatTransport::with_options(
            HttpChatTransportOptions::new().with_api("/custom/chat"),
        );
        let send = ChatTransportSendOptions::new(ChatTransportTrigger::SubmitMessage, "chat-123");
        assert_eq!(
            transport.build_send_messages_request(&send, None).api,
            "/custom/chat"
        );

        let chunks = transport
            .process_response_event_stream(vec![
                br#"data: {"type":"start","messageId":"msg-1"}"#.as_slice(),
                b"\n\n".as_slice(),
                br#"data: {"type":"text-start","id":"text-1"}"#.as_slice(),
                b"\n\n".as_slice(),
                br#"data: {"type":"text-delta","id":"text-1","delta":"Hello"}"#.as_slice(),
                b"\n\n".as_slice(),
                br#"data: {"type":"text-end","id":"text-1"}"#.as_slice(),
                b"\n\n".as_slice(),
                b"data: [DONE]\n\n".as_slice(),
            ])
            .expect("UI-message stream parses");

        assert_eq!(
            chunks,
            vec![
                UiMessageChunk::start_with_message_id("msg-1"),
                UiMessageChunk::text_start("text-1"),
                UiMessageChunk::text_delta("text-1", "Hello"),
                UiMessageChunk::text_end("text-1"),
            ]
        );
    }

    #[test]
    fn default_chat_transport_reports_invalid_ui_message_event() {
        let transport = DefaultChatTransport::new();
        let error = transport
            .process_response_event_stream([br#"data: {"type":"missing-chunk"}"#.as_slice()])
            .expect_err("invalid UI-message event fails");

        assert!(
            error
                .to_string()
                .contains("UI message stream chunk parse failed")
        );
    }

    #[test]
    fn text_stream_chat_transport_maps_text_to_ui_message_stream() {
        let transport = TextStreamChatTransport::new();
        let chunks = transport.process_text_response_stream(["Hello", ", ", "world!"]);

        assert_eq!(
            chunks,
            vec![
                UiMessageChunk::start(),
                UiMessageChunk::start_step(),
                UiMessageChunk::text_start("text-1"),
                UiMessageChunk::text_delta("text-1", "Hello"),
                UiMessageChunk::text_delta("text-1", ", "),
                UiMessageChunk::text_delta("text-1", "world!"),
                UiMessageChunk::text_end("text-1"),
                UiMessageChunk::finish_step(),
                UiMessageChunk::finish(),
            ]
        );
    }

    #[test]
    fn direct_chat_transport_streams_text_response_from_agent() {
        let model = MockLanguageModel::new()
            .with_stream_result(text_stream_result(["Hello", ", ", "world!"]));
        let agent = ToolLoopAgent::for_model(&model);
        let transport = DirectChatTransport::new(&agent);

        let chunks = poll_ready(
            transport.send_messages(
                ChatTransportSendOptions::new(ChatTransportTrigger::SubmitMessage, "chat-1")
                    .with_messages([user_text_message("msg-1", "Hello!")]),
            ),
        )
        .expect("direct transport streams");

        let text_deltas = chunks
            .iter()
            .filter_map(|chunk| match chunk {
                UiMessageChunk::TextDelta { delta, .. } => Some(delta.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(text_deltas, vec!["Hello", ", ", "world!"]);
    }

    #[test]
    fn direct_chat_transport_passes_prepared_agent_options() {
        let model = MockLanguageModel::new().with_stream_result(text_stream_result(["test"]));
        let agent = ToolLoopAgent::new(ToolLoopAgentSettings::new(&model));
        let provider_options = ProviderOptions::from_iter([(
            "custom".to_string(),
            JsonObject::from_iter([("value".to_string(), json!("test-value"))]),
        )]);
        let transport = DirectChatTransport::new(&agent).with_model_settings(
            ToolLoopAgentModelSettings::new().with_provider_options(provider_options.clone()),
        );

        poll_ready(
            transport.send_messages(
                ChatTransportSendOptions::new(ChatTransportTrigger::SubmitMessage, "chat-1")
                    .with_messages([user_text_message("msg-1", "Hello!")]),
            ),
        )
        .expect("direct transport streams");

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].provider_options, Some(provider_options));
    }

    #[test]
    fn direct_chat_transport_applies_ui_message_stream_options() {
        let model = MockLanguageModel::new().with_stream_result(reasoning_stream_result());
        let agent = ToolLoopAgent::for_model(&model);
        let transport = DirectChatTransport::with_options(
            DirectChatTransportOptions::new(&agent).with_ui_message_stream_options(
                StreamTextUiMessageStreamOptions::new()
                    .with_send_reasoning(false)
                    .with_send_finish(false),
            ),
        );

        let chunks = poll_ready(
            transport.send_messages(
                ChatTransportSendOptions::new(ChatTransportTrigger::SubmitMessage, "chat-1")
                    .with_messages([user_text_message("msg-1", "Hello!")]),
            ),
        )
        .expect("direct transport streams");

        assert!(!chunks.iter().any(|chunk| matches!(
            chunk,
            UiMessageChunk::ReasoningStart { .. }
                | UiMessageChunk::ReasoningDelta { .. }
                | UiMessageChunk::ReasoningEnd { .. }
        )));
        assert!(
            !chunks
                .iter()
                .any(|chunk| matches!(chunk, UiMessageChunk::Finish { .. }))
        );
        assert!(chunks.iter().any(
            |chunk| matches!(chunk, UiMessageChunk::TextDelta { delta, .. } if delta == "result")
        ));
    }

    #[test]
    fn direct_chat_transport_converts_ui_messages_to_model_messages_in_order() {
        let model = MockLanguageModel::new().with_stream_result(text_stream_result(["response"]));
        let agent = ToolLoopAgent::for_model(&model);
        let transport = DirectChatTransport::new(&agent);

        poll_ready(
            transport.send_messages(
                ChatTransportSendOptions::new(ChatTransportTrigger::SubmitMessage, "chat-1")
                    .with_messages([
                        user_text_message("msg-1", "First message"),
                        UiMessage::new("msg-2", UiMessageRole::Assistant)
                            .with_part(json!({ "type": "text", "text": "Assistant reply" })),
                        user_text_message("msg-3", "Second message"),
                    ]),
            ),
        )
        .expect("direct transport streams");

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            serde_json::to_value(&calls[0].prompt).expect("prompt serializes"),
            json!([
                {
                    "role": "user",
                    "content": [{ "type": "text", "text": "First message" }]
                },
                {
                    "role": "assistant",
                    "content": [{ "type": "text", "text": "Assistant reply" }]
                },
                {
                    "role": "user",
                    "content": [{ "type": "text", "text": "Second message" }]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_system_provider_metadata() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::System,
        )
        .with_part(json!({
            "type": "text",
            "text": "System message with metadata",
            "providerMetadata": {
                "testProvider": { "systemSignature": "abc123" }
            }
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "system",
                    "content": "System message with metadata",
                    "providerOptions": {
                        "testProvider": { "systemSignature": "abc123" }
                    }
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_merges_system_provider_metadata_from_text_parts() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::System,
        )
        .with_part(json!({
            "type": "text",
            "text": "Part 1",
            "providerMetadata": {
                "provider1": { "key1": "value1" }
            }
        }))
        .with_part(json!({
            "type": "text",
            "text": " Part 2",
            "providerMetadata": {
                "provider2": { "key2": "value2" }
            }
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "system",
                    "content": "Part 1 Part 2",
                    "providerOptions": {
                        "provider1": { "key1": "value1" },
                        "provider2": { "key2": "value2" }
                    }
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_system_anthropic_cache_control_metadata() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "system",
            UiMessageRole::System,
        )
        .with_part(json!({
            "type": "text",
            "text": "You are a helpful assistant.",
            "providerMetadata": {
                "anthropic": {
                    "cacheControl": { "type": "ephemeral" }
                }
            }
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "system",
                    "content": "You are a helpful assistant.",
                    "providerOptions": {
                        "anthropic": {
                            "cacheControl": { "type": "ephemeral" }
                        }
                    }
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_user_text_provider_metadata() {
        let messages =
            convert_ui_messages_to_model_messages(&[UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({
                    "type": "text",
                    "text": "Hello, AI!",
                    "providerMetadata": {
                        "testProvider": { "signature": "1234567890" }
                    }
                }))])
            .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "Hello, AI!",
                            "providerOptions": {
                                "testProvider": { "signature": "1234567890" }
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_user_file_provider_metadata() {
        let messages =
            convert_ui_messages_to_model_messages(&[UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({
                    "type": "file",
                    "mediaType": "image/jpeg",
                    "url": "https://example.com/image.jpg",
                    "providerMetadata": {
                        "testProvider": { "signature": "1234567890" }
                    }
                }))
                .with_part(json!({
                    "type": "text",
                    "text": "Check this image"
                }))])
            .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "file",
                            "mediaType": "image/jpeg",
                            "data": {
                                "type": "url",
                                "url": "https://example.com/image.jpg"
                            },
                            "providerOptions": {
                                "testProvider": { "signature": "1234567890" }
                            }
                        },
                        {
                            "type": "text",
                            "text": "Check this image"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_assistant_text_provider_metadata() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({
            "type": "text",
            "text": "Hello, human!",
            "state": "done",
            "providerMetadata": {
                "testProvider": { "signature": "1234567890" }
            }
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "Hello, human!",
                            "providerOptions": {
                                "testProvider": { "signature": "1234567890" }
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_assistant_file_provider_metadata() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({
            "type": "file",
            "mediaType": "image/png",
            "url": "data:image/png;base64,dGVzdA==",
            "providerMetadata": {
                "testProvider": { "signature": "test-signature" }
            }
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "file",
                            "mediaType": "image/png",
                            "data": {
                                "type": "url",
                                "url": "data:image/png;base64,dGVzdA=="
                            },
                            "providerOptions": {
                                "testProvider": { "signature": "test-signature" }
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_static_tool_output_available_to_assistant_and_tool_messages() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "text",
            "text": "Let me calculate that.",
            "state": "done"
        }))
        .with_part(json!({
            "type": "tool-calculator",
            "state": "output-available",
            "toolCallId": "call-1",
            "input": { "operation": "add", "numbers": [1, 2] },
            "output": "3",
            "callProviderMetadata": { "testProvider": { "signature": "sig-1" } }
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "Let me calculate that." },
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "calculator",
                            "input": { "operation": "add", "numbers": [1, 2] },
                            "providerOptions": { "testProvider": { "signature": "sig-1" } }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "calculator",
                            "output": { "type": "text", "value": "3" },
                            "providerOptions": { "testProvider": { "signature": "sig-1" } }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_tool_output_error_raw_input_to_error_text() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "tool-calculator",
            "state": "output-error",
            "toolCallId": "call-1",
            "input": null,
            "rawInput": { "operation": "add", "numbers": [1, 2] },
            "errorText": "Error: Invalid input"
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "calculator",
                            "input": { "operation": "add", "numbers": [1, 2] }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "calculator",
                            "output": {
                                "type": "error-text",
                                "value": "Error: Invalid input"
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_dynamic_tool_output_available_tool_name() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "dynamic-tool",
            "toolName": "screenshot",
            "state": "output-available",
            "toolCallId": "call-1",
            "input": { "selector": "#hero" },
            "output": { "ok": true }
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "screenshot",
                            "input": { "selector": "#hero" }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "screenshot",
                            "output": { "type": "json", "value": { "ok": true } }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_preserves_step_start_blocks_as_assistant_tool_pairs() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "tool-first",
            "state": "output-available",
            "toolCallId": "call-1",
            "input": { "value": 1 },
            "output": "one"
        }))
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({ "type": "text", "text": "Next step.", "state": "done" }))
        .with_part(json!({
            "type": "tool-second",
            "state": "output-available",
            "toolCallId": "call-2",
            "input": { "value": 2 },
            "output": "two"
        }))])
        .expect("messages convert");

        let roles = messages
            .iter()
            .map(|message| match message {
                LanguageModelMessage::Assistant(_) => "assistant",
                LanguageModelMessage::Tool(_) => "tool",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(roles, vec!["assistant", "tool", "assistant", "tool"]);
    }

    #[test]
    fn convert_ui_messages_places_provider_executed_tool_result_in_assistant() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "tool-calculator",
            "state": "output-available",
            "toolCallId": "call-1",
            "input": { "operation": "add", "numbers": [1, 2] },
            "output": "3",
            "providerExecuted": true
        }))])
        .expect("messages convert");

        assert_eq!(messages.len(), 1);
        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "calculator",
                            "input": { "operation": "add", "numbers": [1, 2] },
                            "providerExecuted": true
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "calculator",
                            "output": { "type": "text", "value": "3" }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_provider_executed_tool_output_available() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "text",
            "text": "Let me calculate that for you.",
            "state": "done"
        }))
        .with_part(json!({
            "type": "tool-calculator",
            "state": "output-available",
            "toolCallId": "call1",
            "input": { "operation": "add", "numbers": [1, 2] },
            "output": "3",
            "providerExecuted": true
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "Let me calculate that for you."
                        },
                        {
                            "type": "tool-call",
                            "toolCallId": "call1",
                            "toolName": "calculator",
                            "input": { "operation": "add", "numbers": [1, 2] },
                            "providerExecuted": true
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call1",
                            "toolName": "calculator",
                            "output": { "type": "text", "value": "3" }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_provider_executed_tool_output_error() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "text",
            "text": "Let me calculate that for you.",
            "state": "done"
        }))
        .with_part(json!({
            "type": "tool-calculator",
            "state": "output-error",
            "toolCallId": "call1",
            "input": { "operation": "add", "numbers": [1, 2] },
            "errorText": "Error: Invalid input",
            "providerExecuted": true
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "Let me calculate that for you."
                        },
                        {
                            "type": "tool-call",
                            "toolCallId": "call1",
                            "toolName": "calculator",
                            "input": { "operation": "add", "numbers": [1, 2] },
                            "providerExecuted": true
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call1",
                            "toolName": "calculator",
                            "output": {
                                "type": "error-json",
                                "value": "Error: Invalid input"
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_propagates_provider_metadata_to_provider_executed_tool_result() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "tool-calculator",
            "state": "output-available",
            "toolCallId": "call1",
            "input": { "operation": "multiply", "numbers": [3, 4] },
            "output": "12",
            "providerExecuted": true,
            "callProviderMetadata": {
                "testProvider": {
                    "executionTime": 75
                }
            }
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call1",
                            "toolName": "calculator",
                            "input": {
                                "operation": "multiply",
                                "numbers": [3, 4]
                            },
                            "providerExecuted": true,
                            "providerOptions": {
                                "testProvider": {
                                    "executionTime": 75
                                }
                            }
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call1",
                            "toolName": "calculator",
                            "output": {
                                "type": "text",
                                "value": "12"
                            },
                            "providerOptions": {
                                "testProvider": {
                                    "executionTime": 75
                                }
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_prefers_result_provider_metadata_for_provider_executed_tool_result() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "tool-calculator",
            "state": "output-available",
            "toolCallId": "call1",
            "input": { "operation": "multiply", "numbers": [3, 4] },
            "output": "12",
            "providerExecuted": true,
            "callProviderMetadata": {
                "testProvider": {
                    "itemId": "call-item"
                }
            },
            "resultProviderMetadata": {
                "testProvider": {
                    "itemId": "result-item"
                }
            }
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call1",
                            "toolName": "calculator",
                            "input": {
                                "operation": "multiply",
                                "numbers": [3, 4]
                            },
                            "providerExecuted": true,
                            "providerOptions": {
                                "testProvider": {
                                    "itemId": "call-item"
                                }
                            }
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call1",
                            "toolName": "calculator",
                            "output": {
                                "type": "text",
                                "value": "12"
                            },
                            "providerOptions": {
                                "testProvider": {
                                    "itemId": "result-item"
                                }
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_denied_approval_response_to_execution_denied_result() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "tool-weather",
            "state": "approval-responded",
            "toolCallId": "call-1",
            "input": { "city": "Tokyo" },
            "callProviderMetadata": { "testProvider": { "signature": "sig-1" } },
            "approval": {
                "id": "approval-1",
                "approved": false,
                "reason": "User denied the request"
            }
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "input": { "city": "Tokyo" },
                            "providerOptions": { "testProvider": { "signature": "sig-1" } }
                        },
                        {
                            "type": "tool-approval-request",
                            "approvalId": "approval-1",
                            "toolCallId": "call-1"
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-approval-response",
                            "approvalId": "approval-1",
                            "approved": false,
                            "reason": "User denied the request"
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "output": {
                                "type": "execution-denied",
                                "reason": "User denied the request"
                            },
                            "providerOptions": {
                                "testProvider": { "signature": "sig-1" }
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_approved_static_tool_approval_response() {
        let messages = convert_ui_messages_to_model_messages(&[
            UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "What is the weather in Tokyo?" })),
            UiMessage::new("msg-2", UiMessageRole::Assistant)
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "tool-weather",
                    "state": "approval-responded",
                    "toolCallId": "call-1",
                    "input": { "city": "Tokyo" },
                    "approval": {
                        "id": "approval-1",
                        "approved": true
                    }
                })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "What is the weather in Tokyo?" }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "input": { "city": "Tokyo" }
                        },
                        {
                            "type": "tool-approval-request",
                            "approvalId": "approval-1",
                            "toolCallId": "call-1"
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-approval-response",
                            "approvalId": "approval-1",
                            "approved": true
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_approved_dynamic_tool_approval_response() {
        let messages = convert_ui_messages_to_model_messages(&[
            UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "What is the weather in Tokyo?" })),
            UiMessage::new("msg-2", UiMessageRole::Assistant)
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "dynamic-tool",
                    "toolName": "weather",
                    "state": "approval-responded",
                    "toolCallId": "call-1",
                    "input": { "city": "Tokyo" },
                    "approval": {
                        "id": "approval-1",
                        "approved": true
                    }
                })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "What is the weather in Tokyo?" }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "input": { "city": "Tokyo" }
                        },
                        {
                            "type": "tool-approval-request",
                            "approvalId": "approval-1",
                            "toolCallId": "call-1"
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-approval-response",
                            "approvalId": "approval-1",
                            "approved": true
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_preserves_automatic_approval_metadata_for_tool_result() {
        let messages = convert_ui_messages_to_model_messages(&[
            UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "What is the weather in Tokyo?" })),
            UiMessage::new("msg-2", UiMessageRole::Assistant)
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "tool-weather",
                    "state": "output-available",
                    "toolCallId": "call-1",
                    "input": { "city": "Tokyo" },
                    "output": { "weather": "Sunny" },
                    "approval": {
                        "id": "approval-1",
                        "approved": true,
                        "isAutomatic": true,
                        "reason": "trusted internal tool"
                    }
                })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "What is the weather in Tokyo?" }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "input": { "city": "Tokyo" }
                        },
                        {
                            "type": "tool-approval-request",
                            "approvalId": "approval-1",
                            "toolCallId": "call-1",
                            "isAutomatic": true
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-approval-response",
                            "approvalId": "approval-1",
                            "approved": true,
                            "reason": "trusted internal tool"
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "output": {
                                "type": "json",
                                "value": { "weather": "Sunny" }
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_marks_provider_executed_denied_approval_response() {
        let messages = convert_ui_messages_to_model_messages(&[
            UiMessage::new("msg-1", UiMessageRole::Assistant)
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "dynamic-tool",
                    "toolName": "screenshot",
                    "state": "approval-responded",
                    "toolCallId": "call-1",
                    "input": { "value": "value-1" },
                    "providerExecuted": true,
                    "callProviderMetadata": {
                        "test-provider": { "key-a": "test-value-1" }
                    },
                    "approval": {
                        "id": "approval-1",
                        "approved": false,
                        "reason": "User denied the request"
                    }
                })),
            UiMessage::new("msg-2", UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "Thanks!" })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "screenshot",
                            "input": { "value": "value-1" },
                            "providerExecuted": true,
                            "providerOptions": {
                                "test-provider": { "key-a": "test-value-1" }
                            }
                        },
                        {
                            "type": "tool-approval-request",
                            "approvalId": "approval-1",
                            "toolCallId": "call-1"
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-approval-response",
                            "approvalId": "approval-1",
                            "approved": false,
                            "reason": "User denied the request",
                            "providerExecuted": true
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "screenshot",
                            "output": {
                                "type": "execution-denied",
                                "reason": "User denied the request"
                            },
                            "providerOptions": {
                                "test-provider": { "key-a": "test-value-1" }
                            }
                        }
                    ]
                },
                {
                    "role": "user",
                    "content": [{ "type": "text", "text": "Thanks!" }]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_denied_static_tool_approval_with_follow_up_text() {
        let messages = convert_ui_messages_to_model_messages(&[
            user_text_message("msg-1", "What is the weather in Tokyo?"),
            UiMessage::new("msg-2", UiMessageRole::Assistant)
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "tool-weather",
                    "state": "approval-responded",
                    "toolCallId": "call-1",
                    "input": { "city": "Tokyo" },
                    "approval": {
                        "id": "approval-1",
                        "approved": false,
                        "reason": "I don't want to approve this"
                    }
                }))
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "text",
                    "text": "I was not able to retrieve the weather.",
                    "state": "done"
                })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "What is the weather in Tokyo?" }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "input": { "city": "Tokyo" }
                        },
                        {
                            "type": "tool-approval-request",
                            "approvalId": "approval-1",
                            "toolCallId": "call-1"
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-approval-response",
                            "approvalId": "approval-1",
                            "approved": false,
                            "reason": "I don't want to approve this"
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "output": {
                                "type": "execution-denied",
                                "reason": "I don't want to approve this"
                            }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "I was not able to retrieve the weather."
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_denied_dynamic_tool_approval_with_follow_up_text() {
        let messages = convert_ui_messages_to_model_messages(&[
            user_text_message("msg-1", "What is the weather in Tokyo?"),
            UiMessage::new("msg-2", UiMessageRole::Assistant)
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "dynamic-tool",
                    "toolName": "weather",
                    "state": "approval-responded",
                    "toolCallId": "call-1",
                    "input": { "city": "Tokyo" },
                    "approval": {
                        "id": "approval-1",
                        "approved": false,
                        "reason": "I don't want to approve this"
                    }
                }))
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "text",
                    "text": "I was not able to retrieve the weather.",
                    "state": "done"
                })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "What is the weather in Tokyo?" }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "input": { "city": "Tokyo" }
                        },
                        {
                            "type": "tool-approval-request",
                            "approvalId": "approval-1",
                            "toolCallId": "call-1"
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-approval-response",
                            "approvalId": "approval-1",
                            "approved": false,
                            "reason": "I don't want to approve this"
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "output": {
                                "type": "execution-denied",
                                "reason": "I don't want to approve this"
                            }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "I was not able to retrieve the weather."
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_static_tool_output_denied() {
        let messages = convert_ui_messages_to_model_messages(&[
            user_text_message("msg-1", "What is the weather in Tokyo?"),
            UiMessage::new("msg-2", UiMessageRole::Assistant)
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "tool-weather",
                    "state": "output-denied",
                    "toolCallId": "call-1",
                    "input": { "city": "Tokyo" },
                    "approval": {
                        "id": "approval-1",
                        "approved": false,
                        "reason": "I don't want to approve this"
                    }
                })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "What is the weather in Tokyo?" }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "input": { "city": "Tokyo" }
                        },
                        {
                            "type": "tool-approval-request",
                            "approvalId": "approval-1",
                            "toolCallId": "call-1"
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-approval-response",
                            "approvalId": "approval-1",
                            "approved": false,
                            "reason": "I don't want to approve this"
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "output": {
                                "type": "error-text",
                                "value": "I don't want to approve this"
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_dynamic_tool_output_denied() {
        let messages = convert_ui_messages_to_model_messages(&[
            user_text_message("msg-1", "What is the weather in Tokyo?"),
            UiMessage::new("msg-2", UiMessageRole::Assistant)
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "dynamic-tool",
                    "toolName": "weather",
                    "state": "output-denied",
                    "toolCallId": "call-1",
                    "input": { "city": "Tokyo" },
                    "approval": {
                        "id": "approval-1",
                        "approved": false,
                        "reason": "I don't want to approve this"
                    }
                })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "What is the weather in Tokyo?" }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "input": { "city": "Tokyo" }
                        },
                        {
                            "type": "tool-approval-request",
                            "approvalId": "approval-1",
                            "toolCallId": "call-1"
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-approval-response",
                            "approvalId": "approval-1",
                            "approved": false,
                            "reason": "I don't want to approve this"
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "output": {
                                "type": "error-text",
                                "value": "I don't want to approve this"
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_approved_tool_result_with_follow_up_text() {
        let messages = convert_ui_messages_to_model_messages(&[
            user_text_message("msg-1", "What is the weather in Tokyo?"),
            UiMessage::new("msg-2", UiMessageRole::Assistant)
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "tool-weather",
                    "state": "output-available",
                    "toolCallId": "call-1",
                    "input": { "city": "Tokyo" },
                    "output": {
                        "weather": "Sunny",
                        "temperature": "20\u{00b0}C"
                    },
                    "approval": {
                        "id": "approval-1",
                        "approved": true
                    }
                }))
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "text",
                    "text": "The weather in Tokyo is sunny.",
                    "state": "done"
                })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "What is the weather in Tokyo?" }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "input": { "city": "Tokyo" }
                        },
                        {
                            "type": "tool-approval-request",
                            "approvalId": "approval-1",
                            "toolCallId": "call-1"
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-approval-response",
                            "approvalId": "approval-1",
                            "approved": true
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "output": {
                                "type": "json",
                                "value": {
                                    "weather": "Sunny",
                                    "temperature": "20\u{00b0}C"
                                }
                            }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "The weather in Tokyo is sunny."
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_approved_tool_error_with_follow_up_text() {
        let messages = convert_ui_messages_to_model_messages(&[
            user_text_message("msg-1", "What is the weather in Tokyo?"),
            UiMessage::new("msg-2", UiMessageRole::Assistant)
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "tool-weather",
                    "state": "output-error",
                    "toolCallId": "call-1",
                    "input": { "city": "Tokyo" },
                    "errorText": "Error: Fetching weather data failed",
                    "approval": {
                        "id": "approval-1",
                        "approved": true
                    }
                }))
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "text",
                    "text": "The weather in Tokyo is sunny.",
                    "state": "done"
                })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "What is the weather in Tokyo?" }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "input": { "city": "Tokyo" }
                        },
                        {
                            "type": "tool-approval-request",
                            "approvalId": "approval-1",
                            "toolCallId": "call-1"
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-approval-response",
                            "approvalId": "approval-1",
                            "approved": true
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "weather",
                            "output": {
                                "type": "error-text",
                                "value": "Error: Fetching weather data failed"
                            }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "The weather in Tokyo is sunny."
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_skips_unconverted_data_parts() {
        let messages = convert_ui_messages_to_model_messages(&[
            UiMessage::new("msg-1", UiMessageRole::System)
                .with_part(json!({ "type": "data-context", "data": { "ignored": true } }))
                .with_part(json!({ "type": "text", "text": "Use concise answers." }))
                .with_part(json!({ "type": "data-tail", "data": "ignored" })),
            UiMessage::new("msg-2", UiMessageRole::User)
                .with_part(json!({ "type": "data-input", "data": { "ignored": true } }))
                .with_part(json!({ "type": "text", "text": "Hello" })),
            UiMessage::new("msg-3", UiMessageRole::Assistant)
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({ "type": "data-status", "data": "ignored" }))
                .with_part(json!({ "type": "text", "text": "Hi there" }))
                .with_part(json!({ "type": "data-result", "data": { "ignored": true } })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "system",
                    "content": "Use concise answers."
                },
                {
                    "role": "user",
                    "content": [{ "type": "text", "text": "Hello" }]
                },
                {
                    "role": "assistant",
                    "content": [{ "type": "text", "text": "Hi there" }]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_file_provider_reference_and_metadata_parts() {
        let messages = convert_ui_messages_to_model_messages(&[
            UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({
                    "type": "file",
                    "mediaType": "image/png",
                    "filename": "input.png",
                    "url": "https://example.com/input.png",
                    "providerMetadata": {
                        "testProvider": { "purpose": "vision" }
                    }
                }))
                .with_part(json!({
                    "type": "file",
                    "mediaType": "application/pdf",
                    "providerReference": {
                        "openai": "file-abc123"
                    }
                })),
            UiMessage::new("msg-2", UiMessageRole::Assistant)
                .with_part(json!({
                    "type": "reasoning",
                    "text": "I should include the image.",
                    "providerMetadata": {
                        "testProvider": { "signature": "reasoning-sig" }
                    }
                }))
                .with_part(json!({
                    "type": "reasoning-file",
                    "mediaType": "application/json",
                    "url": "https://example.com/reasoning.json",
                    "providerMetadata": {
                        "testProvider": { "signature": "reasoning-file-sig" }
                    }
                }))
                .with_part(json!({
                    "type": "custom",
                    "kind": "openai.audio",
                    "providerMetadata": {
                        "testProvider": { "signature": "custom-sig" }
                    }
                }))
                .with_part(json!({
                    "type": "file",
                    "mediaType": "image/jpeg",
                    "providerReference": {
                        "gateway": "file-gw123"
                    }
                })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "file",
                            "filename": "input.png",
                            "data": {
                                "type": "url",
                                "url": "https://example.com/input.png"
                            },
                            "mediaType": "image/png",
                            "providerOptions": {
                                "testProvider": { "purpose": "vision" }
                            }
                        },
                        {
                            "type": "file",
                            "data": {
                                "type": "reference",
                                "reference": {
                                    "openai": "file-abc123"
                                }
                            },
                            "mediaType": "application/pdf"
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "reasoning",
                            "text": "I should include the image.",
                            "providerOptions": {
                                "testProvider": { "signature": "reasoning-sig" }
                            }
                        },
                        {
                            "type": "reasoning-file",
                            "data": {
                                "type": "url",
                                "url": "https://example.com/reasoning.json"
                            },
                            "mediaType": "application/json",
                            "providerOptions": {
                                "testProvider": { "signature": "reasoning-file-sig" }
                            }
                        },
                        {
                            "type": "custom",
                            "kind": "openai.audio",
                            "providerOptions": {
                                "testProvider": { "signature": "custom-sig" }
                            }
                        },
                        {
                            "type": "file",
                            "data": {
                                "type": "reference",
                                "reference": {
                                    "gateway": "file-gw123"
                                }
                            },
                            "mediaType": "image/jpeg"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn direct_chat_transport_rejects_invalid_ui_message_part_shape() {
        let error =
            convert_ui_messages_to_model_messages(&[
                UiMessage::new("msg-1", UiMessageRole::User).with_part(json!({ "type": "text" }))
            ])
            .expect_err("missing text is invalid");

        assert_eq!(
            error,
            ChatTransportError::InvalidMessage(
                "UI text part must include a string text field.".to_string()
            )
        );
    }

    #[test]
    fn direct_chat_transport_reconnect_returns_none() {
        let model = MockLanguageModel::new();
        let agent = ToolLoopAgent::for_model(&model);
        let transport = DirectChatTransport::new(&agent);

        let result =
            poll_ready(transport.reconnect_to_stream(ChatTransportReconnectOptions::new("chat-1")))
                .expect("reconnect succeeds");

        assert_eq!(result, None);
    }
}
