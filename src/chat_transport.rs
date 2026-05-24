use std::fmt;
use std::sync::Arc;

use crate::agent::{ToolLoopAgent, ToolLoopAgentCallOptions, ToolLoopAgentModelSettings};
use crate::file_data::{FileData, ProviderReference};
use crate::generate_text::{GenerateTextTool, ToolModelOutputErrorMode, create_tool_model_output};
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    FinishReason, LanguageModel, LanguageModelAbortSignal, LanguageModelAssistantContentPart,
    LanguageModelAssistantMessage, LanguageModelCustomPart, LanguageModelFileData,
    LanguageModelFilePart, LanguageModelMessage, LanguageModelPrompt,
    LanguageModelReasoningFilePart, LanguageModelReasoningPart, LanguageModelStreamPart,
    LanguageModelSystemMessage, LanguageModelTextPart, LanguageModelToolApprovalRequestPart,
    LanguageModelToolApprovalResponsePart, LanguageModelToolCallPart, LanguageModelToolContentPart,
    LanguageModelToolMessage, LanguageModelToolResultOutput, LanguageModelToolResultPart,
    LanguageModelUserContentPart, LanguageModelUserMessage,
};
use crate::prompt::Prompt;
use crate::provider::ProviderOptions;
use crate::provider_utils::{ParseJsonResult, Tool, normalize_headers, parse_json_event_stream};
use crate::stream_text::StreamTextUiMessageStreamOptions;
use crate::ui_message_stream::{
    StreamingUiMessageState, UiMessage, UiMessageChunk, UiMessageRole, process_ui_message_stream,
    transform_text_to_ui_message_stream,
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
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
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

    #[serde(skip)]
    pub abort_signal: Option<LanguageModelAbortSignal>,
}

impl ChatTransportSendOptions {
    pub fn new(trigger: ChatTransportTrigger, chat_id: impl Into<String>) -> Self {
        Self {
            trigger,
            chat_id: chat_id.into(),
            message_id: None,
            messages: Vec::new(),
            request: ChatRequestOptions::default(),
            abort_signal: None,
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

    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }
}

impl PartialEq for ChatTransportSendOptions {
    fn eq(&self, other: &Self) -> bool {
        self.trigger == other.trigger
            && self.chat_id == other.chat_id
            && self.message_id == other.message_id
            && self.messages == other.messages
            && self.request == other.request
            && match (&self.abort_signal, &other.abort_signal) {
                (None, None) => true,
                (Some(left), Some(right)) => left.is_same_signal(right),
                _ => false,
            }
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
#[derive(Clone, Debug, PartialEq)]
pub enum ChatTransportError {
    Fetch(String),
    StreamDisconnect {
        message: String,
        chunks: Vec<UiMessageChunk>,
    },
    ResponseStatus {
        status: u16,
        body: String,
    },
    EmptyBody,
    InvalidMessage(String),
    Agent(String),
}

impl fmt::Display for ChatTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch(message) => formatter.write_str(message),
            Self::StreamDisconnect { message, .. } => formatter.write_str(message),
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

/// Status of a portable Rust [`Chat`] session.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChatStatus {
    Ready,
    Submitted,
    Streaming,
    Error,
}

/// Error returned by the portable Rust [`Chat`] state manager.
#[derive(Clone, Debug, PartialEq)]
pub enum ChatError {
    Transport(ChatTransportError),
    Stream(String),
}

impl fmt::Display for ChatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(formatter, "{error}"),
            Self::Stream(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ChatError {}

impl From<ChatTransportError> for ChatError {
    fn from(error: ChatTransportError) -> Self {
        Self::Transport(error)
    }
}

/// Text-message input accepted by [`Chat::send_message`].
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,

    pub text: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,

    #[serde(default)]
    pub request: ChatRequestOptions,
}

impl ChatMessageInput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            message_id: None,
            text: text.into(),
            metadata: None,
            request: ChatRequestOptions::default(),
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.message_id = Some(id.into());
        self
    }

    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    pub fn with_metadata(mut self, metadata: impl Into<JsonValue>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    pub fn with_request(mut self, request: ChatRequestOptions) -> Self {
        self.request = request;
        self
    }
}

/// Finish payload recorded by the Rust equivalent of upstream `Chat.onFinish`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatFinishEvent {
    pub finish_reason: Option<FinishReason>,
    pub is_abort: bool,
    pub is_disconnect: bool,
    pub is_error: bool,
    pub message: Option<UiMessage>,
    pub messages: Vec<UiMessage>,
}

struct FoldedChatResponse {
    states: Vec<UiMessage>,
    assistant_message: Option<UiMessage>,
    finish_reason: Option<FinishReason>,
}

/// Portable Rust state manager for upstream `Chat` submit-message flows.
pub struct Chat<T: ChatTransport> {
    id: String,
    transport: T,
    messages: Vec<UiMessage>,
    status: ChatStatus,
    error: Option<String>,
    is_aborted: bool,
    abort_reason: Option<JsonValue>,
    last_finish_event: Option<ChatFinishEvent>,
    next_message_index: usize,
}

impl<T: ChatTransport> Chat<T> {
    pub fn new(id: impl Into<String>, transport: T) -> Self {
        Self {
            id: id.into(),
            transport,
            messages: Vec::new(),
            status: ChatStatus::Ready,
            error: None,
            is_aborted: false,
            abort_reason: None,
            last_finish_event: None,
            next_message_index: 0,
        }
    }

    pub fn with_messages(mut self, messages: impl IntoIterator<Item = UiMessage>) -> Self {
        self.messages = messages.into_iter().collect();
        self.next_message_index = self.messages.len();
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn status(&self) -> ChatStatus {
        self.status
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn is_aborted(&self) -> bool {
        self.is_aborted
    }

    pub fn abort_reason(&self) -> Option<&JsonValue> {
        self.abort_reason.as_ref()
    }

    pub fn last_finish_event(&self) -> Option<&ChatFinishEvent> {
        self.last_finish_event.as_ref()
    }

    pub fn messages(&self) -> &[UiMessage] {
        &self.messages
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn clear_error(&mut self) {
        self.error = None;
        if self.status == ChatStatus::Error {
            self.status = ChatStatus::Ready;
        }
    }

    pub fn send_message(&mut self, input: ChatMessageInput) -> Result<Vec<UiMessage>, ChatError> {
        let message_id = input
            .message_id
            .unwrap_or_else(|| self.generate_message_id());
        let mut user_message = UiMessage::new(message_id.clone(), UiMessageRole::User)
            .with_part(json_text_part(input.text));
        if let Some(metadata) = input.metadata {
            user_message = user_message.with_metadata(metadata);
        }

        if let Some(index) = self
            .messages
            .iter()
            .position(|message| message.role == UiMessageRole::User && message.id == message_id)
        {
            self.messages.truncate(index);
        }
        self.messages.push(user_message);
        self.status = ChatStatus::Submitted;
        self.error = None;
        self.is_aborted = false;
        self.abort_reason = None;
        self.last_finish_event = None;

        let send_options =
            ChatTransportSendOptions::new(ChatTransportTrigger::SubmitMessage, self.id.clone())
                .with_message_id(message_id)
                .with_messages(self.messages.clone())
                .with_request(input.request);

        self.status = ChatStatus::Streaming;
        match self.transport.send_messages(send_options) {
            Ok(chunks) => self.apply_response_chunks(chunks),
            Err(ChatTransportError::StreamDisconnect { message, chunks }) => {
                self.apply_disconnected_response_chunks(chunks, message)
            }
            Err(error) => {
                self.status = ChatStatus::Error;
                let message = error.to_string();
                self.error = Some(message);
                self.last_finish_event = None;
                Err(error.into())
            }
        }
    }

    pub fn add_tool_output(
        &mut self,
        tool_call_id: impl Into<String>,
        output: impl Into<JsonValue>,
    ) -> Result<(), ChatError> {
        self.update_last_assistant_with_chunks([UiMessageChunk::tool_output_available(
            tool_call_id,
            output,
        )])
    }

    pub fn add_dynamic_tool_output(
        &mut self,
        tool_call_id: impl Into<String>,
        output: impl Into<JsonValue>,
    ) -> Result<(), ChatError> {
        self.update_last_assistant_with_chunks([UiMessageChunk::ToolOutputAvailable {
            tool_call_id: tool_call_id.into(),
            output: output.into(),
            provider_executed: None,
            provider_metadata: None,
            tool_metadata: None,
            preliminary: None,
            dynamic: Some(true),
        }])
    }

    pub fn add_tool_error(
        &mut self,
        tool_call_id: impl Into<String>,
        error_text: impl Into<String>,
    ) -> Result<(), ChatError> {
        self.update_last_assistant_with_chunks([UiMessageChunk::tool_output_error(
            tool_call_id,
            error_text,
        )])
    }

    fn generate_message_id(&mut self) -> String {
        self.next_message_index += 1;
        format!("msg-{}", self.next_message_index)
    }

    fn update_last_assistant_with_chunks(
        &mut self,
        chunks: impl IntoIterator<Item = UiMessageChunk>,
    ) -> Result<(), ChatError> {
        let assistant_index = self
            .messages
            .iter()
            .rposition(|message| message.role == UiMessageRole::Assistant)
            .ok_or_else(|| ChatError::Stream("No assistant message exists.".to_string()))?;

        let last_message = self.messages[assistant_index].clone();
        let mut state = StreamingUiMessageState::new(last_message.id.clone(), Some(last_message));
        process_ui_message_stream(&mut state, chunks, false)
            .map_err(|error| ChatError::Stream(error.to_string()))?;
        self.messages[assistant_index] = state.message;
        Ok(())
    }

    fn apply_response_chunks(
        &mut self,
        chunks: Vec<UiMessageChunk>,
    ) -> Result<Vec<UiMessage>, ChatError> {
        let has_error_chunk = chunks.iter().find_map(|chunk| match chunk {
            UiMessageChunk::Error { error_text } => Some(error_text.clone()),
            _ => None,
        });
        if let Some(error_text) = has_error_chunk {
            self.status = ChatStatus::Error;
            self.error = Some(error_text.clone());
            self.is_aborted = false;
            self.abort_reason = None;
            self.last_finish_event = None;
            return Err(ChatError::Stream(error_text));
        }

        let folded = self.fold_response_chunks(chunks, false).map_err(|error| {
            self.last_finish_event = None;
            ChatError::Stream(error.to_string())
        })?;
        self.status = ChatStatus::Ready;
        self.last_finish_event = Some(ChatFinishEvent {
            finish_reason: folded.finish_reason,
            is_abort: self.is_aborted,
            is_disconnect: false,
            is_error: false,
            message: folded.assistant_message,
            messages: self.messages.clone(),
        });
        Ok(folded.states)
    }

    fn apply_disconnected_response_chunks(
        &mut self,
        chunks: Vec<UiMessageChunk>,
        message: String,
    ) -> Result<Vec<UiMessage>, ChatError> {
        let original_chunks = chunks.clone();
        let folded = self.fold_response_chunks(chunks, true).map_err(|error| {
            self.last_finish_event = None;
            ChatError::Stream(error.to_string())
        })?;
        self.status = ChatStatus::Error;
        self.error = Some(message.clone());
        self.is_aborted = false;
        self.abort_reason = None;
        self.last_finish_event = Some(ChatFinishEvent {
            finish_reason: folded.finish_reason,
            is_abort: false,
            is_disconnect: true,
            is_error: true,
            message: folded.assistant_message,
            messages: self.messages.clone(),
        });
        Err(ChatError::Transport(ChatTransportError::StreamDisconnect {
            message,
            chunks: original_chunks,
        }))
    }

    fn fold_response_chunks(
        &mut self,
        chunks: Vec<UiMessageChunk>,
        keep_active_parts_streaming: bool,
    ) -> Result<FoldedChatResponse, ChatTransportError> {
        let mut state = StreamingUiMessageState::new("", None);
        let states = process_ui_message_stream(&mut state, chunks, keep_active_parts_streaming)
            .map_err(|error| ChatTransportError::InvalidMessage(error.to_string()))?;

        let has_assistant_message = !state.message.id.is_empty()
            || !state.message.parts.is_empty()
            || state.message.metadata.is_some();
        let assistant_message = if has_assistant_message {
            Some(state.message.clone())
        } else {
            None
        };
        if let Some(message) = &assistant_message {
            self.messages.push(message.clone());
        }
        self.is_aborted = state.aborted;
        self.abort_reason = state.abort_reason;
        Ok(FoldedChatResponse {
            states,
            assistant_message,
            finish_reason: state.finish_reason,
        })
    }
}

fn json_text_part(text: impl Into<String>) -> JsonValue {
    let mut part = JsonObject::new();
    part.insert("type".to_string(), JsonValue::String("text".to_string()));
    part.insert("text".to_string(), JsonValue::String(text.into()));
    JsonValue::Object(part)
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
        let mut call_options =
            ToolLoopAgentCallOptions::new(prompt).with_model_settings(self.model_settings.clone());
        if let Some(abort_signal) = options.abort_signal {
            call_options = call_options.with_abort_signal(abort_signal);
        }
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

/// Options for converting portable UI messages into model messages.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertUiMessagesToModelMessagesOptions {
    /// Ignore incomplete tool calls before converting the remaining parts.
    #[serde(default)]
    pub ignore_incomplete_tool_calls: bool,
}

impl ConvertUiMessagesToModelMessagesOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_ignore_incomplete_tool_calls(mut self, ignore: bool) -> Self {
        self.ignore_incomplete_tool_calls = ignore;
        self
    }
}

/// A converted UI data part that can be inserted into user or assistant model content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConvertedUiMessageDataPart {
    Text(LanguageModelTextPart),
    File(LanguageModelFilePart),
}

impl ConvertedUiMessageDataPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(LanguageModelTextPart::new(text))
    }

    pub fn file(file: LanguageModelFilePart) -> Self {
        Self::File(file)
    }
}

type UiMessageDataPartConverter<'a> =
    dyn Fn(&JsonValue) -> Result<Option<ConvertedUiMessageDataPart>, ChatTransportError> + 'a;

/// Converts portable UI messages into model messages for in-process transports.
pub fn convert_ui_messages_to_model_messages(
    messages: &[UiMessage],
) -> Result<LanguageModelPrompt, ChatTransportError> {
    convert_ui_messages_to_model_messages_with_options(
        messages,
        ConvertUiMessagesToModelMessagesOptions::default(),
    )
}

/// Converts portable UI messages into model messages with conversion options.
pub fn convert_ui_messages_to_model_messages_with_options(
    messages: &[UiMessage],
    options: ConvertUiMessagesToModelMessagesOptions,
) -> Result<LanguageModelPrompt, ChatTransportError> {
    convert_ui_messages_to_model_messages_internal(messages, options, None)
}

/// Converts portable UI messages into model messages with a UI data-part converter.
pub fn convert_ui_messages_to_model_messages_with_data_part_converter<F>(
    messages: &[UiMessage],
    options: ConvertUiMessagesToModelMessagesOptions,
    convert_data_part: F,
) -> Result<LanguageModelPrompt, ChatTransportError>
where
    F: Fn(&JsonValue) -> Result<Option<ConvertedUiMessageDataPart>, ChatTransportError>,
{
    let convert_data_part: &UiMessageDataPartConverter<'_> = &convert_data_part;
    convert_ui_messages_to_model_messages_internal(messages, options, Some(convert_data_part))
}

/// Converts portable UI messages into model messages and resolves local tool
/// outputs through upstream `toModelOutput` callbacks when a matching tool is
/// provided.
pub async fn convert_ui_messages_to_model_messages_with_tools(
    messages: &[UiMessage],
    options: ConvertUiMessagesToModelMessagesOptions,
    tools: &[GenerateTextTool],
) -> Result<LanguageModelPrompt, ChatTransportError> {
    let mut model_messages = Vec::new();

    for message in messages {
        model_messages.extend(
            convert_ui_message_to_model_messages_with_tools(message, options, None, tools).await?,
        );
    }

    Ok(model_messages)
}

fn convert_ui_messages_to_model_messages_internal(
    messages: &[UiMessage],
    options: ConvertUiMessagesToModelMessagesOptions,
    convert_data_part: Option<&UiMessageDataPartConverter<'_>>,
) -> Result<LanguageModelPrompt, ChatTransportError> {
    let mut model_messages = Vec::new();

    for message in messages {
        model_messages.extend(convert_ui_message_to_model_messages(
            message,
            options,
            convert_data_part,
        )?);
    }

    Ok(model_messages)
}

async fn convert_ui_message_to_model_messages_with_tools(
    message: &UiMessage,
    options: ConvertUiMessagesToModelMessagesOptions,
    convert_data_part: Option<&UiMessageDataPartConverter<'_>>,
    tools: &[GenerateTextTool],
) -> Result<Vec<LanguageModelMessage>, ChatTransportError> {
    if message.id.is_empty() {
        return Err(ChatTransportError::InvalidMessage(
            "UI message id must not be empty.".to_string(),
        ));
    }

    match message.role {
        UiMessageRole::System => convert_system_ui_message(message).map(|message| vec![message]),
        UiMessageRole::User => {
            convert_user_ui_message(message, convert_data_part).map(|message| vec![message])
        }
        UiMessageRole::Assistant => {
            convert_assistant_ui_message_with_tools(message, options, convert_data_part, tools)
                .await
        }
    }
}

fn convert_ui_message_to_model_messages(
    message: &UiMessage,
    options: ConvertUiMessagesToModelMessagesOptions,
    convert_data_part: Option<&UiMessageDataPartConverter<'_>>,
) -> Result<Vec<LanguageModelMessage>, ChatTransportError> {
    if message.id.is_empty() {
        return Err(ChatTransportError::InvalidMessage(
            "UI message id must not be empty.".to_string(),
        ));
    }

    match message.role {
        UiMessageRole::System => convert_system_ui_message(message).map(|message| vec![message]),
        UiMessageRole::User => {
            convert_user_ui_message(message, convert_data_part).map(|message| vec![message])
        }
        UiMessageRole::Assistant => {
            convert_assistant_ui_message(message, options, convert_data_part)
        }
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
    convert_data_part: Option<&UiMessageDataPartConverter<'_>>,
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
            kind if ui_message_part_is_data(kind) => {
                if let Some(converted) = convert_ui_message_data_part(part, convert_data_part)? {
                    push_converted_user_data_part(&mut content, converted);
                }
            }
            _ => return Err(unsupported_part_error(message, kind)),
        }
    }

    Ok(LanguageModelMessage::User(LanguageModelUserMessage::new(
        content,
    )))
}

fn convert_assistant_ui_message(
    message: &UiMessage,
    options: ConvertUiMessagesToModelMessagesOptions,
    convert_data_part: Option<&UiMessageDataPartConverter<'_>>,
) -> Result<Vec<LanguageModelMessage>, ChatTransportError> {
    let mut model_messages = Vec::new();
    let mut block = Vec::new();

    for part in &message.parts {
        let kind = ui_message_part_type(part)?;
        if kind == "step-start" {
            flush_assistant_ui_message_block(&mut model_messages, &mut block, convert_data_part)?;
        } else if should_ignore_incomplete_tool_part(part, kind, options)? {
            continue;
        } else {
            block.push(part);
        }
    }
    flush_assistant_ui_message_block(&mut model_messages, &mut block, convert_data_part)?;

    Ok(model_messages)
}

async fn convert_assistant_ui_message_with_tools(
    message: &UiMessage,
    options: ConvertUiMessagesToModelMessagesOptions,
    convert_data_part: Option<&UiMessageDataPartConverter<'_>>,
    tools: &[GenerateTextTool],
) -> Result<Vec<LanguageModelMessage>, ChatTransportError> {
    let mut model_messages = Vec::new();
    let mut block = Vec::new();

    for part in &message.parts {
        let kind = ui_message_part_type(part)?;
        if kind == "step-start" {
            flush_assistant_ui_message_block_with_tools(
                &mut model_messages,
                &mut block,
                convert_data_part,
                tools,
            )
            .await?;
        } else if should_ignore_incomplete_tool_part(part, kind, options)? {
            continue;
        } else {
            block.push(part);
        }
    }
    flush_assistant_ui_message_block_with_tools(
        &mut model_messages,
        &mut block,
        convert_data_part,
        tools,
    )
    .await?;

    Ok(model_messages)
}

fn should_ignore_incomplete_tool_part(
    part: &JsonValue,
    kind: &str,
    options: ConvertUiMessagesToModelMessagesOptions,
) -> Result<bool, ChatTransportError> {
    if !options.ignore_incomplete_tool_calls
        || !(kind == "dynamic-tool" || kind.starts_with("tool-"))
    {
        return Ok(false);
    }

    let state = ui_message_string_field(part, "state")?;
    Ok(state == "input-streaming" || state == "input-available")
}

fn flush_assistant_ui_message_block(
    model_messages: &mut Vec<LanguageModelMessage>,
    block: &mut Vec<&JsonValue>,
    convert_data_part: Option<&UiMessageDataPartConverter<'_>>,
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
            kind if ui_message_part_is_data(kind) => {
                if let Some(converted) = convert_ui_message_data_part(part, convert_data_part)? {
                    push_converted_assistant_data_part(&mut content, converted);
                }
            }
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

async fn flush_assistant_ui_message_block_with_tools(
    model_messages: &mut Vec<LanguageModelMessage>,
    block: &mut Vec<&JsonValue>,
    convert_data_part: Option<&UiMessageDataPartConverter<'_>>,
    tools: &[GenerateTextTool],
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
                convert_assistant_tool_ui_part_with_tools(
                    part,
                    kind,
                    &mut content,
                    &mut tool_content,
                    tools,
                )
                .await?;
            }
            kind if ui_message_part_is_data(kind) => {
                if let Some(converted) = convert_ui_message_data_part(part, convert_data_part)? {
                    push_converted_assistant_data_part(&mut content, converted);
                }
            }
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

fn convert_ui_message_data_part(
    part: &JsonValue,
    convert_data_part: Option<&UiMessageDataPartConverter<'_>>,
) -> Result<Option<ConvertedUiMessageDataPart>, ChatTransportError> {
    match convert_data_part {
        Some(convert_data_part) => convert_data_part(part),
        None => Ok(None),
    }
}

fn push_converted_user_data_part(
    content: &mut Vec<LanguageModelUserContentPart>,
    converted: ConvertedUiMessageDataPart,
) {
    match converted {
        ConvertedUiMessageDataPart::Text(text) => {
            content.push(LanguageModelUserContentPart::Text(text));
        }
        ConvertedUiMessageDataPart::File(file) => {
            content.push(LanguageModelUserContentPart::File(file));
        }
    }
}

fn push_converted_assistant_data_part(
    content: &mut Vec<LanguageModelAssistantContentPart>,
    converted: ConvertedUiMessageDataPart,
) {
    match converted {
        ConvertedUiMessageDataPart::Text(text) => {
            content.push(LanguageModelAssistantContentPart::Text(text));
        }
        ConvertedUiMessageDataPart::File(file) => {
            content.push(LanguageModelAssistantContentPart::File(file));
        }
    }
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

async fn convert_assistant_tool_ui_part_with_tools(
    part: &JsonValue,
    kind: &str,
    assistant_content: &mut Vec<LanguageModelAssistantContentPart>,
    tool_content: &mut Vec<LanguageModelToolContentPart>,
    tools: &[GenerateTextTool],
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
            let output = tool_result_output_from_ui_part_with_tools(
                part,
                state,
                provider_executed,
                tool_call_id,
                tool_name,
                tools,
            )
            .await?;
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

async fn tool_result_output_from_ui_part_with_tools(
    part: &JsonValue,
    state: &str,
    provider_executed: bool,
    tool_call_id: &str,
    tool_name: &str,
    tools: &[GenerateTextTool],
) -> Result<LanguageModelToolResultOutput, ChatTransportError> {
    if state == "output-error" || provider_executed {
        return tool_result_output_from_ui_part(part, state, provider_executed);
    }

    let input = tool_part_input(part, state);
    let output = ui_message_field(part, "output").cloned();
    Ok(create_tool_model_output(
        tool_call_id,
        &input,
        output.as_ref(),
        rust_tool_by_name(tools, tool_name),
        ToolModelOutputErrorMode::None,
    )
    .await)
}

fn rust_tool_by_name<'a>(tools: &'a [GenerateTextTool], name: &str) -> Option<&'a Tool> {
    tools.iter().find_map(|tool| match tool {
        GenerateTextTool::Rust(tool) if tool.name == name => Some(tool.as_ref()),
        _ => None,
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
#[derive(Clone)]
pub struct HttpChatTransportOptions {
    pub api: String,
    pub credentials: Option<RequestCredentials>,
    pub headers: Headers,
    pub header_callback: Option<Arc<dyn Fn() -> Headers + Send + Sync>>,
    pub body: Option<JsonObject>,
    pub body_callback: Option<Arc<dyn Fn() -> JsonObject + Send + Sync>>,
}

impl Default for HttpChatTransportOptions {
    fn default() -> Self {
        Self {
            api: "/api/chat".to_string(),
            credentials: None,
            headers: Headers::new(),
            header_callback: None,
            body: None,
            body_callback: None,
        }
    }
}

impl fmt::Debug for HttpChatTransportOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpChatTransportOptions")
            .field("api", &self.api)
            .field("credentials", &self.credentials)
            .field("headers", &self.headers)
            .field("has_header_callback", &self.header_callback.is_some())
            .field("body", &self.body)
            .field("has_body_callback", &self.body_callback.is_some())
            .finish()
    }
}

impl PartialEq for HttpChatTransportOptions {
    fn eq(&self, other: &Self) -> bool {
        self.api == other.api
            && self.credentials == other.credentials
            && self.headers == other.headers
            && self.header_callback.is_some() == other.header_callback.is_some()
            && self.body == other.body
            && self.body_callback.is_some() == other.body_callback.is_some()
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

    pub fn with_header_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn() -> Headers + Send + Sync + 'static,
    {
        self.headers = Headers::new();
        self.header_callback = Some(Arc::new(callback));
        self
    }

    pub fn with_body(mut self, body: JsonObject) -> Self {
        self.body = Some(body);
        self.body_callback = None;
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
        self.body_callback = None;
        self
    }

    pub fn with_body_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn() -> JsonObject + Send + Sync + 'static,
    {
        self.body = None;
        self.body_callback = Some(Arc::new(callback));
        self
    }

    fn resolved_headers(&self) -> Headers {
        if let Some(callback) = &self.header_callback {
            callback()
        } else {
            self.headers.clone()
        }
    }

    fn resolved_body(&self) -> Option<JsonObject> {
        if let Some(callback) = &self.body_callback {
            Some(callback())
        } else {
            self.body.clone()
        }
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
        let transport_body = self.options.resolved_body();
        let transport_headers = self.options.resolved_headers();
        let body = merged_body(transport_body.as_ref(), options.request.body.as_ref());

        PrepareSendMessagesRequestOptions {
            api: self.options.api.clone(),
            id: options.chat_id.clone(),
            messages: options.messages.clone(),
            request_metadata: options.request.metadata.clone(),
            body,
            headers: merged_headers(&transport_headers, &options.request.headers),
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
        let transport_body = self.options.resolved_body();
        let transport_headers = self.options.resolved_headers();

        PrepareReconnectToStreamRequestOptions {
            api: self.options.api.clone(),
            id: options.chat_id.clone(),
            request_metadata: options.request.metadata.clone(),
            body: merged_body(transport_body.as_ref(), options.request.body.as_ref()),
            headers: merged_headers(&transport_headers, &options.request.headers),
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
    use std::sync::Mutex;
    use std::task::{Context, Poll, Waker};

    use super::*;
    use crate::agent::{ToolLoopAgent, ToolLoopAgentSettings};
    use crate::language_model::{
        FinishReason, InputTokenUsage, LanguageModelAbortController, LanguageModelFinishReason,
        LanguageModelReasoningDelta, LanguageModelReasoningEnd, LanguageModelReasoningStart,
        LanguageModelStreamFinish, LanguageModelStreamResult, LanguageModelStreamStart,
        LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextStart, LanguageModelUsage,
        OutputTokenUsage,
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

    #[derive(Debug)]
    struct RecordingChatTransport {
        response: Vec<UiMessageChunk>,
        error: Option<ChatTransportError>,
        captured_send: Mutex<Option<ChatTransportSendOptions>>,
    }

    impl RecordingChatTransport {
        fn new(response: impl IntoIterator<Item = UiMessageChunk>) -> Self {
            Self {
                response: response.into_iter().collect(),
                error: None,
                captured_send: Mutex::new(None),
            }
        }

        fn with_error(error: ChatTransportError) -> Self {
            Self {
                response: Vec::new(),
                error: Some(error),
                captured_send: Mutex::new(None),
            }
        }

        fn captured_send(&self) -> ChatTransportSendOptions {
            self.captured_send
                .lock()
                .expect("captured send mutex is not poisoned")
                .clone()
                .expect("send was captured")
        }
    }

    impl ChatTransport for RecordingChatTransport {
        fn send_messages(
            &self,
            options: ChatTransportSendOptions,
        ) -> Result<Vec<UiMessageChunk>, ChatTransportError> {
            *self
                .captured_send
                .lock()
                .expect("captured send mutex is not poisoned") = Some(options);
            if let Some(error) = &self.error {
                return Err(error.clone());
            }
            Ok(self.response.clone())
        }

        fn reconnect_to_stream(
            &self,
            _options: ChatTransportReconnectOptions,
        ) -> Result<Option<Vec<UiMessageChunk>>, ChatTransportError> {
            Ok(None)
        }
    }

    #[test]
    fn chat_should_include_the_metadata_of_text_message() {
        let transport = RecordingChatTransport::new([
            UiMessageChunk::start_with_message_id("assistant-1"),
            UiMessageChunk::start_step(),
            UiMessageChunk::text_start("text-1"),
            UiMessageChunk::text_delta("text-1", "Hello."),
            UiMessageChunk::text_end("text-1"),
            UiMessageChunk::finish_step(),
            UiMessageChunk::finish(),
        ]);
        let mut chat = Chat::new("chat-1", transport);

        let states = chat
            .send_message(
                ChatMessageInput::text("Hello")
                    .with_id("user-1")
                    .with_metadata(json!({ "test": "metadata" }))
                    .with_request(
                        ChatRequestOptions::new().with_body_property("session", json!("s1")),
                    ),
            )
            .expect("message sends");

        let captured = chat.transport().captured_send();
        assert_eq!(captured.trigger, ChatTransportTrigger::SubmitMessage);
        assert_eq!(captured.chat_id, "chat-1");
        assert_eq!(captured.message_id, Some("user-1".to_string()));
        assert_eq!(
            captured.request.body,
            Some(JsonObject::from_iter([(
                "session".to_string(),
                json!("s1")
            )]))
        );
        assert_eq!(
            serde_json::to_value(&captured.messages).expect("messages serialize"),
            json!([
                {
                    "id": "user-1",
                    "role": "user",
                    "metadata": { "test": "metadata" },
                    "parts": [{ "type": "text", "text": "Hello" }]
                }
            ])
        );
        assert_eq!(chat.status(), ChatStatus::Ready);
        assert_eq!(
            serde_json::to_value(chat.messages()).expect("history serializes"),
            json!([
                {
                    "id": "user-1",
                    "role": "user",
                    "metadata": { "test": "metadata" },
                    "parts": [{ "type": "text", "text": "Hello" }]
                },
                {
                    "id": "assistant-1",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        { "type": "text", "text": "Hello.", "state": "done" }
                    ]
                }
            ])
        );
        assert_eq!(
            states.last().and_then(|message| message.parts.last()),
            Some(&json!({ "type": "text", "text": "Hello.", "state": "done" }))
        );
    }

    #[test]
    fn chat_should_call_on_finish_with_message_and_messages() {
        let transport = RecordingChatTransport::new([
            UiMessageChunk::start_with_message_id("assistant-1"),
            UiMessageChunk::start_step(),
            UiMessageChunk::text_start("text-1"),
            UiMessageChunk::text_delta("text-1", "Hello"),
            UiMessageChunk::text_delta("text-1", ","),
            UiMessageChunk::text_delta("text-1", " world"),
            UiMessageChunk::text_delta("text-1", "."),
            UiMessageChunk::text_end("text-1"),
            UiMessageChunk::finish_step(),
            UiMessageChunk::finish_with_reason(FinishReason::Stop),
        ]);
        let mut chat = Chat::new("chat-1", transport);

        chat.send_message(ChatMessageInput::text("Hello, world!").with_id("user-1"))
            .expect("message sends");

        let event = chat.last_finish_event().expect("finish event is recorded");
        assert_eq!(event.finish_reason, Some(FinishReason::Stop));
        assert!(!event.is_abort);
        assert!(!event.is_disconnect);
        assert!(!event.is_error);
        assert_eq!(
            serde_json::to_value(event.message.as_ref().expect("assistant message exists"))
                .expect("message serializes"),
            json!({
                "id": "assistant-1",
                "role": "assistant",
                "parts": [
                    { "type": "step-start" },
                    { "type": "text", "text": "Hello, world.", "state": "done" }
                ]
            })
        );
        assert_eq!(
            serde_json::to_value(&event.messages).expect("messages serialize"),
            json!([
                {
                    "id": "user-1",
                    "role": "user",
                    "parts": [{ "type": "text", "text": "Hello, world!" }]
                },
                {
                    "id": "assistant-1",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        { "type": "text", "text": "Hello, world.", "state": "done" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn chat_should_handle_error_parts() {
        let transport = RecordingChatTransport::new([
            UiMessageChunk::start(),
            UiMessageChunk::error("test-error"),
        ]);
        let mut chat = Chat::new("chat-1", transport);

        let error = chat
            .send_message(ChatMessageInput::text("Hello, world!").with_id("user-1"))
            .expect_err("error chunk fails chat send");

        assert_eq!(error, ChatError::Stream("test-error".to_string()));
        assert_eq!(chat.status(), ChatStatus::Error);
        assert_eq!(chat.error(), Some("test-error"));
        assert_eq!(
            serde_json::to_value(chat.messages()).expect("history serializes"),
            json!([
                {
                    "id": "user-1",
                    "role": "user",
                    "parts": [{ "type": "text", "text": "Hello, world!" }]
                }
            ])
        );
    }

    #[test]
    fn chat_should_clear_the_error_and_set_the_status_to_ready() {
        let transport = RecordingChatTransport::new([
            UiMessageChunk::start(),
            UiMessageChunk::error("test-error"),
        ]);
        let mut chat = Chat::new("chat-1", transport);

        chat.send_message(ChatMessageInput::text("Hello, world!").with_id("user-1"))
            .expect_err("error chunk fails chat send");

        assert_eq!(chat.error(), Some("test-error"));
        assert_eq!(chat.status(), ChatStatus::Error);

        chat.clear_error();

        assert_eq!(chat.error(), None);
        assert_eq!(chat.status(), ChatStatus::Ready);
    }

    #[test]
    fn chat_should_set_error_status_when_transport_send_fails() {
        let transport = RecordingChatTransport::with_error(ChatTransportError::ResponseStatus {
            status: 500,
            body: "Internal Server Error".to_string(),
        });
        let mut chat = Chat::new("chat-1", transport);

        let error = chat
            .send_message(ChatMessageInput::text("Hello, world!").with_id("user-1"))
            .expect_err("transport failure is surfaced");

        assert_eq!(
            error.to_string(),
            "chat transport returned status 500: Internal Server Error"
        );
        assert_eq!(chat.status(), ChatStatus::Error);
        assert_eq!(
            chat.error(),
            Some("chat transport returned status 500: Internal Server Error")
        );
        assert_eq!(
            serde_json::to_value(chat.messages()).expect("history serializes"),
            json!([
                {
                    "id": "user-1",
                    "role": "user",
                    "parts": [{ "type": "text", "text": "Hello, world!" }]
                }
            ])
        );
    }

    #[test]
    fn chat_should_handle_a_disconnected_response_stream() {
        let partial_chunks = vec![
            UiMessageChunk::start_with_message_id("assistant-1"),
            UiMessageChunk::start_step(),
            UiMessageChunk::text_start("text-1"),
            UiMessageChunk::text_delta("text-1", "Hello"),
        ];
        let transport = RecordingChatTransport::with_error(ChatTransportError::StreamDisconnect {
            message: "fetch failed".to_string(),
            chunks: partial_chunks.clone(),
        });
        let mut chat = Chat::new("chat-1", transport);

        let error = chat
            .send_message(ChatMessageInput::text("Hello, world!").with_id("user-1"))
            .expect_err("disconnect is surfaced");

        assert_eq!(
            error,
            ChatError::Transport(ChatTransportError::StreamDisconnect {
                message: "fetch failed".to_string(),
                chunks: partial_chunks
            })
        );
        assert_eq!(chat.status(), ChatStatus::Error);
        assert_eq!(chat.error(), Some("fetch failed"));
        assert!(!chat.is_aborted());
        assert_eq!(chat.abort_reason(), None);
        let event = chat.last_finish_event().expect("finish event is recorded");
        assert_eq!(event.finish_reason, None);
        assert!(!event.is_abort);
        assert!(event.is_disconnect);
        assert!(event.is_error);
        assert_eq!(
            serde_json::to_value(event.message.as_ref().expect("assistant message exists"))
                .expect("message serializes"),
            json!({
                "id": "assistant-1",
                "role": "assistant",
                "parts": [
                    { "type": "step-start" },
                    { "type": "text", "text": "Hello", "state": "streaming" }
                ]
            })
        );
        assert_eq!(
            serde_json::to_value(chat.messages()).expect("history serializes"),
            json!([
                {
                    "id": "user-1",
                    "role": "user",
                    "parts": [{ "type": "text", "text": "Hello, world!" }]
                },
                {
                    "id": "assistant-1",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        { "type": "text", "text": "Hello", "state": "streaming" }
                    ]
                }
            ])
        );
        assert_eq!(
            serde_json::to_value(&event.messages).expect("messages serialize"),
            serde_json::to_value(chat.messages()).expect("history serializes")
        );
    }

    #[test]
    fn chat_should_handle_a_stop_and_an_aborted_response_stream() {
        let transport = RecordingChatTransport::new([
            UiMessageChunk::start_with_message_id("assistant-1"),
            UiMessageChunk::start_step(),
            UiMessageChunk::text_start("text-1"),
            UiMessageChunk::text_delta("text-1", "Hello"),
            UiMessageChunk::abort_with_reason(json!({ "source": "client" })),
        ]);
        let mut chat = Chat::new("chat-1", transport);

        let states = chat
            .send_message(ChatMessageInput::text("Hello, world!").with_id("user-1"))
            .expect("abort chunk does not fail chat send");

        assert_eq!(chat.status(), ChatStatus::Ready);
        assert_eq!(chat.error(), None);
        assert!(chat.is_aborted());
        assert_eq!(chat.abort_reason(), Some(&json!({ "source": "client" })));
        let event = chat.last_finish_event().expect("finish event is recorded");
        assert_eq!(event.finish_reason, None);
        assert!(event.is_abort);
        assert!(!event.is_disconnect);
        assert!(!event.is_error);
        assert_eq!(
            serde_json::to_value(event.message.as_ref().expect("assistant message exists"))
                .expect("message serializes"),
            json!({
                "id": "assistant-1",
                "role": "assistant",
                "parts": [
                    { "type": "step-start" },
                    { "type": "text", "text": "Hello", "state": "done" }
                ]
            })
        );
        assert_eq!(
            states.last().and_then(|message| message.parts.last()),
            Some(&json!({ "type": "text", "text": "Hello", "state": "done" }))
        );
        assert_eq!(
            serde_json::to_value(chat.messages()).expect("history serializes"),
            json!([
                {
                    "id": "user-1",
                    "role": "user",
                    "parts": [{ "type": "text", "text": "Hello, world!" }]
                },
                {
                    "id": "assistant-1",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        { "type": "text", "text": "Hello", "state": "done" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn chat_should_add_tool_output_to_the_latest_assistant_message() {
        let transport = RecordingChatTransport::new([
            UiMessageChunk::start_with_message_id("assistant-1"),
            UiMessageChunk::start_step(),
            UiMessageChunk::tool_input_available(
                "tool-call-0",
                "test-tool",
                json!({ "testArg": "test-value" }),
            ),
            UiMessageChunk::finish_step(),
            UiMessageChunk::finish(),
        ]);
        let mut chat = Chat::new("chat-1", transport);

        chat.send_message(ChatMessageInput::text("Hello, world!").with_id("user-1"))
            .expect("message sends");

        chat.add_tool_output("tool-call-0", json!("test-output"))
            .expect("tool output is added");

        assert_eq!(
            serde_json::to_value(chat.messages()).expect("history serializes"),
            json!([
                {
                    "id": "user-1",
                    "role": "user",
                    "parts": [{ "type": "text", "text": "Hello, world!" }]
                },
                {
                    "id": "assistant-1",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        {
                            "type": "tool-test-tool",
                            "toolCallId": "tool-call-0",
                            "state": "output-available",
                            "input": { "testArg": "test-value" },
                            "output": "test-output"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn chat_should_add_tool_error_to_the_latest_assistant_message() {
        let transport = RecordingChatTransport::new([
            UiMessageChunk::start_with_message_id("assistant-1"),
            UiMessageChunk::start_step(),
            UiMessageChunk::tool_input_available(
                "tool-call-0",
                "test-tool",
                json!({ "testArg": "test-value" }),
            ),
            UiMessageChunk::finish_step(),
            UiMessageChunk::finish(),
        ]);
        let mut chat = Chat::new("chat-1", transport);

        chat.send_message(ChatMessageInput::text("Hello, world!").with_id("user-1"))
            .expect("message sends");

        chat.add_tool_error("tool-call-0", "test-error")
            .expect("tool error is added");

        assert_eq!(
            serde_json::to_value(chat.messages()).expect("history serializes"),
            json!([
                {
                    "id": "user-1",
                    "role": "user",
                    "parts": [{ "type": "text", "text": "Hello, world!" }]
                },
                {
                    "id": "assistant-1",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        {
                            "type": "tool-test-tool",
                            "toolCallId": "tool-call-0",
                            "state": "output-error",
                            "input": { "testArg": "test-value" },
                            "errorText": "test-error"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn chat_should_add_dynamic_tool_output_to_the_latest_assistant_message() {
        let transport = RecordingChatTransport::new([
            UiMessageChunk::start_with_message_id("assistant-1"),
            UiMessageChunk::start_step(),
            UiMessageChunk::ToolInputAvailable {
                tool_call_id: "tool-call-0".to_string(),
                tool_name: "test-tool".to_string(),
                input: json!({ "testArg": "test-value" }),
                provider_executed: None,
                provider_metadata: None,
                tool_metadata: None,
                dynamic: Some(true),
                title: None,
            },
            UiMessageChunk::finish_step(),
            UiMessageChunk::finish(),
        ]);
        let mut chat = Chat::new("chat-1", transport);

        chat.send_message(ChatMessageInput::text("Hello, world!").with_id("user-1"))
            .expect("message sends");

        chat.add_dynamic_tool_output("tool-call-0", json!("test-output"))
            .expect("dynamic tool output is added");

        assert_eq!(
            serde_json::to_value(chat.messages()).expect("history serializes"),
            json!([
                {
                    "id": "user-1",
                    "role": "user",
                    "parts": [{ "type": "text", "text": "Hello, world!" }]
                },
                {
                    "id": "assistant-1",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        {
                            "type": "dynamic-tool",
                            "toolName": "test-tool",
                            "toolCallId": "tool-call-0",
                            "state": "output-available",
                            "input": { "testArg": "test-value" },
                            "output": "test-output"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn chat_should_replace_an_existing_user_message() {
        let transport = RecordingChatTransport::new([
            UiMessageChunk::start_with_message_id("assistant-new"),
            UiMessageChunk::start_step(),
            UiMessageChunk::text_start("text-1"),
            UiMessageChunk::text_delta("text-1", "Hello"),
            UiMessageChunk::text_delta("text-1", ","),
            UiMessageChunk::text_delta("text-1", " world"),
            UiMessageChunk::text_delta("text-1", "."),
            UiMessageChunk::text_end("text-1"),
            UiMessageChunk::finish_step(),
            UiMessageChunk::finish(),
        ]);
        let mut chat = Chat::new("chat-1", transport).with_messages([
            UiMessage::new("user-1", UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "Hi!" })),
            UiMessage::new("assistant-old", UiMessageRole::Assistant).with_part(
                json!({ "type": "text", "text": "How can I help you?", "state": "done" }),
            ),
        ]);

        chat.send_message(ChatMessageInput::text("Hello, world!").with_message_id("user-1"))
            .expect("message sends");

        let captured = chat.transport().captured_send();
        assert_eq!(captured.message_id, Some("user-1".to_string()));
        assert_eq!(
            serde_json::to_value(&captured.messages).expect("messages serialize"),
            json!([
                {
                    "id": "user-1",
                    "role": "user",
                    "parts": [{ "type": "text", "text": "Hello, world!" }]
                }
            ])
        );
        assert_eq!(
            serde_json::to_value(chat.messages()).expect("history serializes"),
            json!([
                {
                    "id": "user-1",
                    "role": "user",
                    "parts": [{ "type": "text", "text": "Hello, world!" }]
                },
                {
                    "id": "assistant-new",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        { "type": "text", "text": "Hello, world.", "state": "done" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn chat_should_update_the_messages_during_streaming() {
        let transport = RecordingChatTransport::new([
            UiMessageChunk::start_with_message_id("assistant-1"),
            UiMessageChunk::start_step(),
            UiMessageChunk::text_start("text-1"),
            UiMessageChunk::text_delta("text-1", "Hello"),
            UiMessageChunk::text_delta("text-1", ","),
            UiMessageChunk::text_delta("text-1", " world"),
            UiMessageChunk::text_delta("text-1", "."),
            UiMessageChunk::text_end("text-1"),
            UiMessageChunk::finish_step(),
            UiMessageChunk::finish(),
        ]);
        let mut chat = Chat::new("chat-1", transport);

        let states = chat
            .send_message(ChatMessageInput::text("Hello, world!").with_id("user-1"))
            .expect("message sends");

        assert_eq!(
            serde_json::to_value(&states[..3]).expect("states serialize"),
            json!([
                {
                    "id": "assistant-1",
                    "role": "assistant",
                    "parts": []
                },
                {
                    "id": "assistant-1",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        { "type": "text", "text": "", "state": "streaming" }
                    ]
                },
                {
                    "id": "assistant-1",
                    "role": "assistant",
                    "parts": [
                        { "type": "step-start" },
                        { "type": "text", "text": "Hello", "state": "streaming" }
                    ]
                }
            ])
        );
        assert_eq!(
            states.last().and_then(|message| message.parts.last()),
            Some(&json!({ "type": "text", "text": "Hello, world.", "state": "done" }))
        );
        assert_eq!(chat.status(), ChatStatus::Ready);
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
    fn http_chat_transport_includes_body_in_request_when_function_is_provided() {
        let transport = HttpChatTransport::with_options(
            HttpChatTransportOptions::new()
                .with_api("https://example.test/api/chat")
                .with_body_callback(|| {
                    JsonObject::from_iter([("someData".to_string(), json!(true))])
                }),
        );
        let send = ChatTransportSendOptions::new(ChatTransportTrigger::SubmitMessage, "c123")
            .with_message_id("m123")
            .with_messages([UiMessage::new("m123", crate::UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "Hello, world!" }))]);

        let request = transport.build_send_messages_request(&send, None);

        assert_eq!(
            request.body,
            Some(json!({
                "id": "c123",
                "messageId": "m123",
                "messages": [
                    {
                        "id": "m123",
                        "role": "user",
                        "parts": [{ "type": "text", "text": "Hello, world!" }]
                    }
                ],
                "someData": true,
                "trigger": "submit-message"
            }))
        );
    }

    #[test]
    fn http_chat_transport_includes_headers_in_request_when_function_is_provided() {
        let transport = HttpChatTransport::with_options(
            HttpChatTransportOptions::new()
                .with_api("https://example.test/api/chat")
                .with_header_callback(|| {
                    Headers::from([("X-Test-Header".to_string(), "test-value-fn".to_string())])
                }),
        );
        let send = ChatTransportSendOptions::new(ChatTransportTrigger::SubmitMessage, "c123")
            .with_message_id("m123")
            .with_messages([UiMessage::new("m123", crate::UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "Hello, world!" }))]);

        let request = transport.build_send_messages_request(&send, None);

        assert_eq!(
            request.headers.get("x-test-header").map(String::as_str),
            Some("test-value-fn")
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
    fn direct_chat_transport_passes_abort_signal_to_agent() {
        let model = MockLanguageModel::new().with_stream_result(text_stream_result(["test"]));
        let agent = ToolLoopAgent::for_model(&model);
        let transport = DirectChatTransport::new(&agent);
        let abort_controller = LanguageModelAbortController::new();
        let abort_signal = abort_controller.signal();

        poll_ready(
            transport.send_messages(
                ChatTransportSendOptions::new(ChatTransportTrigger::SubmitMessage, "chat-1")
                    .with_messages([user_text_message("msg-1", "Hello!")])
                    .with_abort_signal(abort_signal.clone()),
            ),
        )
        .expect("direct transport streams");

        let calls = model.stream_calls();
        assert_eq!(calls.len(), 1);
        let captured_signal = calls[0]
            .abort_signal
            .as_ref()
            .expect("abort signal is forwarded");
        assert!(captured_signal.is_same_signal(&abort_signal));
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
    fn convert_ui_messages_maps_simple_system_message() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::System,
        )
        .with_part(json!({
            "type": "text",
            "text": "System message"
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "system",
                    "content": "System message"
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_simple_user_message() {
        let messages =
            convert_ui_messages_to_model_messages(&[UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({
                    "type": "text",
                    "text": "Hello, AI!"
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
                            "text": "Hello, AI!"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_custom_assistant_part() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({
            "type": "custom",
            "kind": "test-provider.compaction",
            "providerMetadata": {
                "openai": {
                    "itemId": "cmp_123"
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
                            "type": "custom",
                            "kind": "test-provider.compaction",
                            "providerOptions": {
                                "openai": {
                                    "itemId": "cmp_123"
                                }
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_simple_assistant_text_message() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({
            "type": "text",
            "text": "Hello, human!",
            "state": "done"
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
                            "text": "Hello, human!"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_assistant_reasoning_parts() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({
            "type": "reasoning",
            "text": "Thinking...",
            "providerMetadata": {
                "testProvider": {
                    "signature": "1234567890"
                }
            },
            "state": "done"
        }))
        .with_part(json!({
            "type": "reasoning",
            "text": "redacted-data",
            "providerMetadata": {
                "testProvider": {
                    "isRedacted": true
                }
            },
            "state": "done"
        }))
        .with_part(json!({
            "type": "text",
            "text": "Hello, human!",
            "state": "done"
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "reasoning",
                            "text": "Thinking...",
                            "providerOptions": {
                                "testProvider": { "signature": "1234567890" }
                            }
                        },
                        {
                            "type": "reasoning",
                            "text": "redacted-data",
                            "providerOptions": {
                                "testProvider": { "isRedacted": true }
                            }
                        },
                        {
                            "type": "text",
                            "text": "Hello, human!"
                        }
                    ]
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
    fn convert_ui_messages_maps_user_file_url_part() {
        let messages =
            convert_ui_messages_to_model_messages(&[UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({
                    "type": "file",
                    "mediaType": "image/jpeg",
                    "url": "https://example.com/image.jpg"
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
    fn convert_ui_messages_includes_user_file_filename() {
        let messages =
            convert_ui_messages_to_model_messages(&[UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({
                    "type": "file",
                    "mediaType": "image/jpeg",
                    "url": "https://example.com/image.jpg",
                    "filename": "image.jpg"
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
                            "filename": "image.jpg",
                            "data": {
                                "type": "url",
                                "url": "https://example.com/image.jpg"
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_user_file_provider_reference() {
        let messages =
            convert_ui_messages_to_model_messages(&[UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({
                    "type": "file",
                    "mediaType": "application/pdf",
                    "filename": "doc.pdf",
                    "url": "data:application/pdf;base64,abc",
                    "providerReference": {
                        "openai": "file-abc123"
                    }
                }))
                .with_part(json!({
                    "type": "text",
                    "text": "Summarize this"
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
                            "mediaType": "application/pdf",
                            "filename": "doc.pdf",
                            "data": {
                                "type": "reference",
                                "reference": {
                                    "openai": "file-abc123"
                                }
                            }
                        },
                        {
                            "type": "text",
                            "text": "Summarize this"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_omits_user_file_filename_when_absent() {
        let messages =
            convert_ui_messages_to_model_messages(&[UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({
                    "type": "file",
                    "mediaType": "image/jpeg",
                    "url": "https://example.com/image.jpg"
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
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_assistant_file_url_part() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({
            "type": "file",
            "mediaType": "image/png",
            "url": "data:image/png;base64,dGVzdA=="
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
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_includes_assistant_file_filename() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({
            "type": "file",
            "mediaType": "image/png",
            "url": "data:image/png;base64,dGVzdA==",
            "filename": "test.png"
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
                            "filename": "test.png",
                            "data": {
                                "type": "url",
                                "url": "data:image/png;base64,dGVzdA=="
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_assistant_file_provider_reference() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({
            "type": "file",
            "mediaType": "application/pdf",
            "filename": "doc.pdf",
            "url": "data:application/pdf;base64,xyz",
            "providerReference": {
                "anthropic": "file-xyz789"
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
                            "mediaType": "application/pdf",
                            "filename": "doc.pdf",
                            "data": {
                                "type": "reference",
                                "reference": {
                                    "anthropic": "file-xyz789"
                                }
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
    fn convert_ui_messages_maps_tool_output_available_with_provider_metadata() {
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
            "callProviderMetadata": {
                "testProvider": {
                    "signature": "1234567890"
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
                            "type": "text",
                            "text": "Let me calculate that for you."
                        },
                        {
                            "type": "tool-call",
                            "toolCallId": "call1",
                            "toolName": "calculator",
                            "input": { "operation": "add", "numbers": [1, 2] },
                            "providerOptions": {
                                "testProvider": {
                                    "signature": "1234567890"
                                }
                            }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-result",
                            "toolCallId": "call1",
                            "toolName": "calculator",
                            "output": { "type": "text", "value": "3" },
                            "providerOptions": {
                                "testProvider": {
                                    "signature": "1234567890"
                                }
                            }
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
    fn convert_ui_messages_maps_tool_output_error_input_to_error_text() {
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
                            "type": "text",
                            "text": "Let me calculate that for you."
                        },
                        {
                            "type": "tool-call",
                            "toolCallId": "call1",
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
                            "toolCallId": "call1",
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
    fn convert_ui_messages_maps_tool_invocation_multi_part_response() {
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
            "type": "tool-screenshot",
            "state": "output-available",
            "toolCallId": "call1",
            "input": {},
            "output": "imgbase64"
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
                            "toolName": "screenshot",
                            "input": {}
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-result",
                            "toolCallId": "call1",
                            "toolName": "screenshot",
                            "output": {
                                "type": "text",
                                "value": "imgbase64"
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_empty_tool_invocation_conversation() {
        let messages = convert_ui_messages_to_model_messages(&[
            UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "text1" })),
            UiMessage::new("msg-2", UiMessageRole::Assistant)
                .with_part(json!({ "type": "text", "text": "text2", "state": "done" })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [{ "type": "text", "text": "text1" }]
                },
                {
                    "role": "assistant",
                    "content": [{ "type": "text", "text": "text2" }]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_multiple_messages_conversation() {
        let messages = convert_ui_messages_to_model_messages(&[
            UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "What's the weather like?" })),
            UiMessage::new("msg-2", UiMessageRole::Assistant).with_part(
                json!({ "type": "text", "text": "I'll check that for you.", "state": "done" }),
            ),
            UiMessage::new("msg-3", UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "Thanks!" })),
        ])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "What's the weather like?"
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "I'll check that for you."
                        }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "Thanks!"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_multiple_tool_invocations_with_steps() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({ "type": "text", "text": "response", "state": "done" }))
        .with_part(json!({
            "type": "tool-screenshot",
            "state": "output-available",
            "toolCallId": "call-1",
            "input": { "value": "value-1" },
            "output": "result-1"
        }))
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "tool-screenshot",
            "state": "output-available",
            "toolCallId": "call-2",
            "input": { "value": "value-2" },
            "output": "result-2"
        }))
        .with_part(json!({
            "type": "tool-screenshot",
            "state": "output-available",
            "toolCallId": "call-3",
            "input": { "value": "value-3" },
            "output": "result-3"
        }))
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "tool-screenshot",
            "state": "output-available",
            "toolCallId": "call-4",
            "input": { "value": "value-4" },
            "output": "result-4"
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "response" },
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "screenshot",
                            "input": { "value": "value-1" }
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
                            "output": { "type": "text", "value": "result-1" }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-2",
                            "toolName": "screenshot",
                            "input": { "value": "value-2" }
                        },
                        {
                            "type": "tool-call",
                            "toolCallId": "call-3",
                            "toolName": "screenshot",
                            "input": { "value": "value-3" }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-result",
                            "toolCallId": "call-2",
                            "toolName": "screenshot",
                            "output": { "type": "text", "value": "result-2" }
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-3",
                            "toolName": "screenshot",
                            "output": { "type": "text", "value": "result-3" }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-4",
                            "toolName": "screenshot",
                            "input": { "value": "value-4" }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-result",
                            "toolCallId": "call-4",
                            "toolName": "screenshot",
                            "output": { "type": "text", "value": "result-4" }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_tool_invocations_mixed_with_text() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "text",
            "text": "i am gonna use tool1",
            "state": "done"
        }))
        .with_part(json!({
            "type": "tool-screenshot",
            "state": "output-available",
            "toolCallId": "call-1",
            "input": { "value": "value-1" },
            "output": "result-1"
        }))
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "text",
            "text": "i am gonna use tool2 and tool3",
            "state": "done"
        }))
        .with_part(json!({
            "type": "tool-screenshot",
            "state": "output-available",
            "toolCallId": "call-2",
            "input": { "value": "value-2" },
            "output": "result-2"
        }))
        .with_part(json!({
            "type": "tool-screenshot",
            "state": "output-available",
            "toolCallId": "call-3",
            "input": { "value": "value-3" },
            "output": "result-3"
        }))
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "tool-screenshot",
            "state": "output-available",
            "toolCallId": "call-4",
            "input": { "value": "value-4" },
            "output": "result-4"
        }))
        .with_part(json!({ "type": "step-start" }))
        .with_part(json!({
            "type": "text",
            "text": "final response",
            "state": "done"
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
                            "text": "i am gonna use tool1"
                        },
                        {
                            "type": "tool-call",
                            "toolCallId": "call-1",
                            "toolName": "screenshot",
                            "input": { "value": "value-1" }
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
                            "output": { "type": "text", "value": "result-1" }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "i am gonna use tool2 and tool3"
                        },
                        {
                            "type": "tool-call",
                            "toolCallId": "call-2",
                            "toolName": "screenshot",
                            "input": { "value": "value-2" }
                        },
                        {
                            "type": "tool-call",
                            "toolCallId": "call-3",
                            "toolName": "screenshot",
                            "input": { "value": "value-3" }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-result",
                            "toolCallId": "call-2",
                            "toolName": "screenshot",
                            "output": { "type": "text", "value": "result-2" }
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-3",
                            "toolName": "screenshot",
                            "output": { "type": "text", "value": "result-3" }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-4",
                            "toolName": "screenshot",
                            "input": { "value": "value-4" }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-result",
                            "toolCallId": "call-4",
                            "toolName": "screenshot",
                            "output": { "type": "text", "value": "result-4" }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "final response"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_maps_multiple_tool_invocations_with_trailing_user_message() {
        let messages = convert_ui_messages_to_model_messages(&[
            UiMessage::new("msg-1", UiMessageRole::Assistant)
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "tool-screenshot",
                    "state": "output-available",
                    "toolCallId": "call-1",
                    "input": { "value": "value-1" },
                    "output": "result-1"
                }))
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "tool-screenshot",
                    "state": "output-available",
                    "toolCallId": "call-2",
                    "input": { "value": "value-2" },
                    "output": "result-2"
                }))
                .with_part(json!({
                    "type": "tool-screenshot",
                    "state": "output-available",
                    "toolCallId": "call-3",
                    "input": { "value": "value-3" },
                    "output": "result-3"
                }))
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({
                    "type": "tool-screenshot",
                    "state": "output-available",
                    "toolCallId": "call-4",
                    "input": { "value": "value-4" },
                    "output": "result-4"
                }))
                .with_part(json!({ "type": "step-start" }))
                .with_part(json!({ "type": "text", "text": "response", "state": "done" })),
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
                            "input": { "value": "value-1" }
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
                            "output": { "type": "text", "value": "result-1" }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-2",
                            "toolName": "screenshot",
                            "input": { "value": "value-2" }
                        },
                        {
                            "type": "tool-call",
                            "toolCallId": "call-3",
                            "toolName": "screenshot",
                            "input": { "value": "value-3" }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-result",
                            "toolCallId": "call-2",
                            "toolName": "screenshot",
                            "output": { "type": "text", "value": "result-2" }
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-3",
                            "toolName": "screenshot",
                            "output": { "type": "text", "value": "result-3" }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool-call",
                            "toolCallId": "call-4",
                            "toolName": "screenshot",
                            "input": { "value": "value-4" }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": [
                        {
                            "type": "tool-result",
                            "toolCallId": "call-4",
                            "toolName": "screenshot",
                            "output": { "type": "text", "value": "result-4" }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "response" }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "Thanks!" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_can_ignore_incomplete_tool_calls() {
        let messages = convert_ui_messages_to_model_messages_with_options(
            &[
                UiMessage::new("msg-1", UiMessageRole::Assistant)
                    .with_part(json!({ "type": "step-start" }))
                    .with_part(json!({
                        "type": "tool-screenshot",
                        "state": "output-available",
                        "toolCallId": "call-1",
                        "input": { "value": "value-1" },
                        "output": "result-1"
                    }))
                    .with_part(json!({ "type": "step-start" }))
                    .with_part(json!({
                        "type": "tool-screenshot",
                        "state": "input-streaming",
                        "toolCallId": "call-2",
                        "input": { "value": "value-2" }
                    }))
                    .with_part(json!({
                        "type": "tool-screenshot",
                        "state": "input-available",
                        "toolCallId": "call-3",
                        "input": { "value": "value-3" }
                    }))
                    .with_part(json!({
                        "type": "dynamic-tool",
                        "toolName": "tool-screenshot2",
                        "state": "input-available",
                        "toolCallId": "call-3",
                        "input": { "value": "value-3" }
                    }))
                    .with_part(json!({ "type": "text", "text": "response", "state": "done" })),
                UiMessage::new("msg-2", UiMessageRole::User)
                    .with_part(json!({ "type": "text", "text": "Thanks!" })),
            ],
            ConvertUiMessagesToModelMessagesOptions::new().with_ignore_incomplete_tool_calls(true),
        )
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
                            "input": { "value": "value-1" }
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
                            "output": { "type": "text", "value": "result-1" }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "response" }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "Thanks!" }
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
    fn convert_ui_messages_maps_dynamic_tool_with_trailing_user_message() {
        let messages = convert_ui_messages_to_model_messages_with_options(
            &[
                UiMessage::new("msg-1", UiMessageRole::Assistant)
                    .with_part(json!({ "type": "step-start" }))
                    .with_part(json!({
                        "type": "dynamic-tool",
                        "toolName": "screenshot",
                        "state": "output-available",
                        "toolCallId": "call-1",
                        "input": { "value": "value-1" },
                        "output": "result-1"
                    })),
                UiMessage::new("msg-2", UiMessageRole::User)
                    .with_part(json!({ "type": "text", "text": "Thanks!" })),
            ],
            ConvertUiMessagesToModelMessagesOptions::new().with_ignore_incomplete_tool_calls(true),
        )
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
                            "input": { "value": "value-1" }
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
                            "output": { "type": "text", "value": "result-1" }
                        }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "Thanks!" }
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
    fn convert_ui_messages_maps_provider_executed_dynamic_tool_with_trailing_user_message() {
        let messages = convert_ui_messages_to_model_messages_with_options(
            &[
                UiMessage::new("msg-1", UiMessageRole::Assistant)
                    .with_part(json!({ "type": "step-start" }))
                    .with_part(json!({
                        "type": "dynamic-tool",
                        "toolName": "screenshot",
                        "state": "output-available",
                        "toolCallId": "call-1",
                        "input": { "value": "value-1" },
                        "output": "result-1",
                        "providerExecuted": true,
                        "callProviderMetadata": {
                            "test-provider": {
                                "key-a": "test-value-1",
                                "key-b": "test-value-2"
                            }
                        }
                    })),
                UiMessage::new("msg-2", UiMessageRole::User)
                    .with_part(json!({ "type": "text", "text": "Thanks!" })),
            ],
            ConvertUiMessagesToModelMessagesOptions::new().with_ignore_incomplete_tool_calls(true),
        )
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
                                "test-provider": {
                                    "key-a": "test-value-1",
                                    "key-b": "test-value-2"
                                }
                            }
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call-1",
                            "toolName": "screenshot",
                            "output": { "type": "text", "value": "result-1" },
                            "providerOptions": {
                                "test-provider": {
                                    "key-a": "test-value-1",
                                    "key-b": "test-value-2"
                                }
                            }
                        }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "Thanks!" }
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
        let messages = convert_ui_messages_to_model_messages_with_options(
            &[
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
            ],
            ConvertUiMessagesToModelMessagesOptions::new().with_ignore_incomplete_tool_calls(true),
        )
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
    fn convert_ui_messages_converts_user_data_url_to_text_with_converter() {
        let messages = convert_ui_messages_to_model_messages_with_data_part_converter(
            &[
                UiMessage::new("msg-1", UiMessageRole::User).with_part(json!({
                    "type": "data-url",
                    "data": {
                        "url": "https://example.com",
                        "content": "Article text"
                    }
                })),
            ],
            ConvertUiMessagesToModelMessagesOptions::default(),
            |part| {
                let data = &part["data"];
                let url = data["url"].as_str().expect("url is a string");
                let content = data["content"].as_str().expect("content is a string");
                Ok(Some(ConvertedUiMessageDataPart::text(format!(
                    "\n\n[{url}]\n{content}"
                ))))
            },
        )
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "\n\n[https://example.com]\nArticle text"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_skips_user_data_parts_when_no_converter_provided() {
        let messages =
            convert_ui_messages_to_model_messages(&[UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "Hello" }))
                .with_part(json!({
                    "type": "data-url",
                    "data": { "url": "https://example.com" }
                }))])
            .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "Hello" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_selectively_converts_user_data_parts() {
        let messages = convert_ui_messages_to_model_messages_with_data_part_converter(
            &[UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({
                    "type": "data-url",
                    "data": { "url": "https://example.com" }
                }))
                .with_part(json!({
                    "type": "data-ui-state",
                    "data": { "enabled": true }
                }))],
            ConvertUiMessagesToModelMessagesOptions::default(),
            |part| {
                if ui_message_part_type(part)? == "data-url" {
                    let url = part["data"]["url"].as_str().expect("url is a string");
                    Ok(Some(ConvertedUiMessageDataPart::text(url.to_string())))
                } else {
                    Ok(None)
                }
            },
        )
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "https://example.com" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_preserves_user_data_part_order_with_converter() {
        let messages = convert_ui_messages_to_model_messages_with_data_part_converter(
            &[UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "First" }))
                .with_part(json!({ "type": "data-tag", "data": { "value": "tag1" } }))
                .with_part(json!({ "type": "text", "text": "Second" }))
                .with_part(json!({ "type": "data-tag", "data": { "value": "tag2" } }))
                .with_part(json!({ "type": "text", "text": "Third" }))],
            ConvertUiMessagesToModelMessagesOptions::default(),
            |part| {
                if ui_message_part_type(part)? == "data-tag" {
                    let value = part["data"]["value"]
                        .as_str()
                        .expect("data tag value is a string");
                    Ok(Some(ConvertedUiMessageDataPart::text(format!("[{value}]"))))
                } else {
                    Ok(None)
                }
            },
        )
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "First" },
                        { "type": "text", "text": "[tag1]" },
                        { "type": "text", "text": "Second" },
                        { "type": "text", "text": "[tag2]" },
                        { "type": "text", "text": "Third" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_converts_multiple_user_data_part_types() {
        let messages = convert_ui_messages_to_model_messages_with_data_part_converter(
            &[UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "Review these:" }))
                .with_part(json!({
                    "type": "data-url",
                    "data": {
                        "url": "https://example.com",
                        "title": "Example"
                    }
                }))
                .with_part(json!({
                    "type": "data-code",
                    "data": {
                        "code": "console.log(\"test\")",
                        "language": "javascript"
                    }
                }))
                .with_part(json!({
                    "type": "data-note",
                    "data": { "text": "Internal note" }
                }))],
            ConvertUiMessagesToModelMessagesOptions::default(),
            |part| match ui_message_part_type(part)? {
                "data-url" => {
                    let data = &part["data"];
                    let title = data["title"].as_str().expect("title is a string");
                    let url = data["url"].as_str().expect("url is a string");
                    Ok(Some(ConvertedUiMessageDataPart::text(format!(
                        "[{title}]({url})"
                    ))))
                }
                "data-code" => {
                    let data = &part["data"];
                    let language = data["language"].as_str().expect("language is a string");
                    let code = data["code"].as_str().expect("code is a string");
                    Ok(Some(ConvertedUiMessageDataPart::text(format!(
                        "```{language}\n{code}\n```"
                    ))))
                }
                _ => Ok(None),
            },
        )
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "Review these:" },
                        { "type": "text", "text": "[Example](https://example.com)" },
                        {
                            "type": "text",
                            "text": "```javascript\nconsole.log(\"test\")\n```"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_handles_user_message_without_data_parts_with_converter() {
        let messages = convert_ui_messages_to_model_messages_with_data_part_converter(
            &[UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "Hello" }))
                .with_part(json!({
                    "type": "file",
                    "mediaType": "image/png",
                    "url": "https://example.com/image.png"
                }))],
            ConvertUiMessagesToModelMessagesOptions::default(),
            |_| -> Result<Option<ConvertedUiMessageDataPart>, ChatTransportError> {
                panic!("converter should not run for non-data parts")
            },
        )
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "Hello" },
                        {
                            "type": "file",
                            "data": {
                                "type": "url",
                                "url": "https://example.com/image.png"
                            },
                            "mediaType": "image/png"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_converts_user_data_parts_to_file_with_converter() {
        let messages = convert_ui_messages_to_model_messages_with_data_part_converter(
            &[UiMessage::new("msg-1", UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "Check this file" }))
                .with_part(json!({
                    "type": "data-attachment",
                    "data": {
                        "mediaType": "application/pdf",
                        "filename": "document.pdf",
                        "data": "base64data"
                    }
                }))],
            ConvertUiMessagesToModelMessagesOptions::default(),
            |part| {
                if ui_message_part_type(part)? == "data-attachment" {
                    let data = &part["data"];
                    let file = LanguageModelFilePart::new(
                        FileData::Data {
                            data: crate::file_data::FileDataContent::Base64(
                                data["data"]
                                    .as_str()
                                    .expect("attachment data is a string")
                                    .to_string(),
                            ),
                        },
                        data["mediaType"]
                            .as_str()
                            .expect("attachment media type is a string"),
                    )
                    .with_filename(
                        data["filename"]
                            .as_str()
                            .expect("attachment filename is a string"),
                    );
                    Ok(Some(ConvertedUiMessageDataPart::file(file)))
                } else {
                    Ok(None)
                }
            },
        )
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "Check this file" },
                        {
                            "type": "file",
                            "filename": "document.pdf",
                            "data": {
                                "type": "data",
                                "data": "base64data"
                            },
                            "mediaType": "application/pdf"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_converts_assistant_data_url_to_text_with_converter() {
        let messages = convert_ui_messages_to_model_messages_with_data_part_converter(
            &[
                UiMessage::new("msg-1", UiMessageRole::Assistant).with_part(json!({
                    "type": "data-url",
                    "data": {
                        "url": "https://example.com",
                        "content": "Article text"
                    }
                })),
            ],
            ConvertUiMessagesToModelMessagesOptions::default(),
            |part| {
                let data = &part["data"];
                let url = data["url"].as_str().expect("url is a string");
                let content = data["content"].as_str().expect("content is a string");
                Ok(Some(ConvertedUiMessageDataPart::text(format!(
                    "\n\n[{url}]\n{content}"
                ))))
            },
        )
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "\n\n[https://example.com]\nArticle text"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_skips_assistant_data_parts_when_no_converter_provided() {
        let messages = convert_ui_messages_to_model_messages(&[UiMessage::new(
            "msg-1",
            UiMessageRole::Assistant,
        )
        .with_part(json!({ "type": "text", "text": "Hello", "state": "done" }))
        .with_part(json!({
            "type": "data-url",
            "data": { "url": "https://example.com" }
        }))])
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "Hello" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_selectively_converts_assistant_data_parts() {
        let messages = convert_ui_messages_to_model_messages_with_data_part_converter(
            &[UiMessage::new("msg-1", UiMessageRole::Assistant)
                .with_part(json!({
                    "type": "data-url",
                    "data": { "url": "https://example.com" }
                }))
                .with_part(json!({
                    "type": "data-ui-state",
                    "data": { "enabled": true }
                }))],
            ConvertUiMessagesToModelMessagesOptions::default(),
            |part| {
                if ui_message_part_type(part)? == "data-url" {
                    let url = part["data"]["url"].as_str().expect("url is a string");
                    Ok(Some(ConvertedUiMessageDataPart::text(url.to_string())))
                } else {
                    Ok(None)
                }
            },
        )
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "https://example.com" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_preserves_assistant_data_part_order_with_converter() {
        let messages = convert_ui_messages_to_model_messages_with_data_part_converter(
            &[UiMessage::new("msg-1", UiMessageRole::Assistant)
                .with_part(json!({ "type": "text", "text": "First", "state": "done" }))
                .with_part(json!({ "type": "data-tag", "data": { "value": "tag1" } }))
                .with_part(json!({ "type": "text", "text": "Second", "state": "done" }))
                .with_part(json!({ "type": "data-tag", "data": { "value": "tag2" } }))
                .with_part(json!({ "type": "text", "text": "Third", "state": "done" }))],
            ConvertUiMessagesToModelMessagesOptions::default(),
            |part| {
                if ui_message_part_type(part)? == "data-tag" {
                    let value = part["data"]["value"]
                        .as_str()
                        .expect("data tag value is a string");
                    Ok(Some(ConvertedUiMessageDataPart::text(format!("[{value}]"))))
                } else {
                    Ok(None)
                }
            },
        )
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "First" },
                        { "type": "text", "text": "[tag1]" },
                        { "type": "text", "text": "Second" },
                        { "type": "text", "text": "[tag2]" },
                        { "type": "text", "text": "Third" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_converts_multiple_assistant_data_part_types() {
        let messages = convert_ui_messages_to_model_messages_with_data_part_converter(
            &[UiMessage::new("msg-1", UiMessageRole::Assistant)
                .with_part(json!({ "type": "text", "text": "Review these:", "state": "done" }))
                .with_part(json!({
                    "type": "data-url",
                    "data": {
                        "url": "https://example.com",
                        "title": "Example"
                    }
                }))
                .with_part(json!({
                    "type": "data-code",
                    "data": {
                        "code": "console.log(\"test\")",
                        "language": "javascript"
                    }
                }))
                .with_part(json!({
                    "type": "data-note",
                    "data": { "text": "Internal note" }
                }))],
            ConvertUiMessagesToModelMessagesOptions::default(),
            |part| match ui_message_part_type(part)? {
                "data-url" => {
                    let data = &part["data"];
                    let title = data["title"].as_str().expect("title is a string");
                    let url = data["url"].as_str().expect("url is a string");
                    Ok(Some(ConvertedUiMessageDataPart::text(format!(
                        "[{title}]({url})"
                    ))))
                }
                "data-code" => {
                    let data = &part["data"];
                    let language = data["language"].as_str().expect("language is a string");
                    let code = data["code"].as_str().expect("code is a string");
                    Ok(Some(ConvertedUiMessageDataPart::text(format!(
                        "```{language}\n{code}\n```"
                    ))))
                }
                _ => Ok(None),
            },
        )
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "Review these:" },
                        { "type": "text", "text": "[Example](https://example.com)" },
                        {
                            "type": "text",
                            "text": "```javascript\nconsole.log(\"test\")\n```"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_handles_assistant_message_without_data_parts_with_converter() {
        let messages = convert_ui_messages_to_model_messages_with_data_part_converter(
            &[UiMessage::new("msg-1", UiMessageRole::Assistant)
                .with_part(json!({ "type": "text", "text": "Hello", "state": "done" }))
                .with_part(json!({
                    "type": "file",
                    "mediaType": "image/png",
                    "url": "https://example.com/image.png"
                }))],
            ConvertUiMessagesToModelMessagesOptions::default(),
            |_| -> Result<Option<ConvertedUiMessageDataPart>, ChatTransportError> {
                panic!("converter should not run for non-data parts")
            },
        )
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "Hello" },
                        {
                            "type": "file",
                            "data": {
                                "type": "url",
                                "url": "https://example.com/image.png"
                            },
                            "mediaType": "image/png"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn convert_ui_messages_converts_assistant_data_parts_to_file_with_converter() {
        let messages = convert_ui_messages_to_model_messages_with_data_part_converter(
            &[UiMessage::new("msg-1", UiMessageRole::Assistant)
                .with_part(json!({ "type": "text", "text": "Check this file", "state": "done" }))
                .with_part(json!({
                    "type": "data-attachment",
                    "data": {
                        "mediaType": "application/pdf",
                        "filename": "document.pdf",
                        "data": "base64data"
                    }
                }))],
            ConvertUiMessagesToModelMessagesOptions::default(),
            |part| {
                if ui_message_part_type(part)? == "data-attachment" {
                    let data = &part["data"];
                    let file = LanguageModelFilePart::new(
                        FileData::Data {
                            data: crate::file_data::FileDataContent::Base64(
                                data["data"]
                                    .as_str()
                                    .expect("attachment data is a string")
                                    .to_string(),
                            ),
                        },
                        data["mediaType"]
                            .as_str()
                            .expect("attachment media type is a string"),
                    )
                    .with_filename(
                        data["filename"]
                            .as_str()
                            .expect("attachment filename is a string"),
                    );
                    Ok(Some(ConvertedUiMessageDataPart::file(file)))
                } else {
                    Ok(None)
                }
            },
        )
        .expect("messages convert");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([
                {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "Check this file" },
                        {
                            "type": "file",
                            "filename": "document.pdf",
                            "data": {
                                "type": "data",
                                "data": "base64data"
                            },
                            "mediaType": "application/pdf"
                        }
                    ]
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
    fn convert_ui_messages_rejects_unknown_roles_at_json_boundary() {
        let error = serde_json::from_value::<UiMessage>(json!({
            "id": "msg-1",
            "role": "unknown",
            "parts": [
                { "type": "text", "text": "unknown role message" }
            ]
        }))
        .expect_err("unknown UI message roles are rejected before conversion");

        assert!(
            error.to_string().contains("unknown variant `unknown`"),
            "{error}"
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
