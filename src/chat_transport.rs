use std::fmt;

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::provider_utils::normalize_headers;
use crate::ui_message_stream::{UiMessage, UiMessageChunk};

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
}

impl fmt::Display for ChatTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch(message) => formatter.write_str(message),
            Self::ResponseStatus { status, body } => {
                write!(formatter, "chat transport returned status {status}: {body}")
            }
            Self::EmptyBody => formatter.write_str("The response body is empty."),
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
    use super::*;
    use serde_json::json;

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
}
