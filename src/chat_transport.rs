use std::fmt;

use crate::agent::{ToolLoopAgent, ToolLoopAgentCallOptions, ToolLoopAgentModelSettings};
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    LanguageModel, LanguageModelAssistantContentPart, LanguageModelAssistantMessage,
    LanguageModelMessage, LanguageModelPrompt, LanguageModelStreamPart, LanguageModelSystemMessage,
    LanguageModelTextPart, LanguageModelUserContentPart, LanguageModelUserMessage,
};
use crate::prompt::Prompt;
use crate::provider::ProviderOptions;
use crate::provider_utils::normalize_headers;
use crate::stream_text::StreamTextUiMessageStreamOptions;
use crate::ui_message_stream::{UiMessage, UiMessageChunk, UiMessageRole};

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
    messages
        .iter()
        .map(convert_ui_message_to_model_message)
        .collect()
}

fn convert_ui_message_to_model_message(
    message: &UiMessage,
) -> Result<LanguageModelMessage, ChatTransportError> {
    if message.id.is_empty() {
        return Err(ChatTransportError::InvalidMessage(
            "UI message id must not be empty.".to_string(),
        ));
    }

    match message.role {
        UiMessageRole::System => convert_system_ui_message(message),
        UiMessageRole::User => convert_user_ui_message(message),
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
        if kind != "text" {
            return Err(unsupported_part_error(message, kind));
        }
        content.push_str(ui_message_text(part)?);
        if let Some(options) = ui_message_provider_options(part)? {
            provider_options.extend(options);
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
        if kind != "text" {
            return Err(unsupported_part_error(message, kind));
        }
        let mut text_part = LanguageModelTextPart::new(ui_message_text(part)?);
        if let Some(provider_options) = ui_message_provider_options(part)? {
            text_part = text_part.with_provider_options(provider_options);
        }
        content.push(LanguageModelUserContentPart::Text(text_part));
    }

    Ok(LanguageModelMessage::User(LanguageModelUserMessage::new(
        content,
    )))
}

fn convert_assistant_ui_message(
    message: &UiMessage,
) -> Result<LanguageModelMessage, ChatTransportError> {
    let mut content = Vec::new();

    for part in &message.parts {
        let kind = ui_message_part_type(part)?;
        if kind == "step-start" {
            continue;
        }
        if kind != "text" {
            return Err(unsupported_part_error(message, kind));
        }
        let mut text_part = LanguageModelTextPart::new(ui_message_text(part)?);
        if let Some(provider_options) = ui_message_provider_options(part)? {
            text_part = text_part.with_provider_options(provider_options);
        }
        content.push(LanguageModelAssistantContentPart::Text(text_part));
    }

    Ok(LanguageModelMessage::Assistant(
        LanguageModelAssistantMessage::new(content),
    ))
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

fn ui_message_text(part: &JsonValue) -> Result<&str, ChatTransportError> {
    part.as_object()
        .and_then(|object| object.get("text"))
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            ChatTransportError::InvalidMessage(
                "UI text part must include a string text field.".to_string(),
            )
        })
}

fn ui_message_provider_options(
    part: &JsonValue,
) -> Result<Option<ProviderOptions>, ChatTransportError> {
    let Some(provider_metadata) = part
        .as_object()
        .and_then(|object| object.get("providerMetadata"))
    else {
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
