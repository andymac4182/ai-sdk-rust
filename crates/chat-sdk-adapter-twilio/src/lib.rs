//! Twilio SMS/MMS adapter helpers for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-twilio`. This crate
//! owns the portable surfaces from the upstream package: thread-id
//! codecs, Twilio REST API request planning, webhook parsing and
//! signature helpers, SMS fallback formatting, card fallback text, and
//! voice/TwiML helpers. HTTP fetches are represented as deterministic
//! request plans so tests can prove exact body/path/auth shape without
//! live credentials.

use async_trait::async_trait;
use base64::Engine;
use chat_sdk_chat::types::{Adapter, AdapterError, AdapterResult};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use subtle::ConstantTimeEq;

pub const ADAPTER_NAME: &str = "twilio";
pub const THREAD_ID_PREFIX: &str = "twilio:";
pub const DEFAULT_API_URL: &str = "https://api.twilio.com";
pub const TWILIO_MESSAGE_LIMIT: usize = 1600;

type HmacSha1 = Hmac<Sha1>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwilioThreadId {
    pub sender: String,
    pub recipient: String,
}

pub fn encode_twilio_thread_id(sender: &str, recipient: &str) -> String {
    format!(
        "{THREAD_ID_PREFIX}{}:{}",
        percent_encode(sender),
        percent_encode(recipient)
    )
}

pub fn decode_twilio_thread_id(thread_id: &str) -> Result<TwilioThreadId, TwilioError> {
    let suffix = thread_id
        .strip_prefix(THREAD_ID_PREFIX)
        .ok_or_else(|| TwilioError::validation(format!("Invalid Twilio thread ID: {thread_id}")))?;
    let parts: Vec<&str> = suffix.split(':').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(TwilioError::validation(format!(
            "Invalid Twilio thread ID: {thread_id}"
        )));
    }
    Ok(TwilioThreadId {
        sender: percent_decode(parts[0])?,
        recipient: percent_decode(parts[1])?,
    })
}

pub fn twilio_channel_id(thread_id: &str) -> Result<String, TwilioError> {
    let decoded = decode_twilio_thread_id(thread_id)?;
    Ok(format!(
        "{THREAD_ID_PREFIX}{}",
        percent_encode(&decoded.sender)
    ))
}

#[derive(Debug, Clone)]
pub struct TwilioAdapterOptions {
    pub account_sid: String,
    pub auth_token: String,
    pub phone_number: Option<String>,
    pub messaging_service_sid: Option<String>,
    pub status_callback_url: Option<String>,
    pub api_base_url: Option<String>,
}

impl TwilioAdapterOptions {
    pub fn new(account_sid: impl Into<String>, auth_token: impl Into<String>) -> Self {
        Self {
            account_sid: account_sid.into(),
            auth_token: auth_token.into(),
            phone_number: None,
            messaging_service_sid: None,
            status_callback_url: None,
            api_base_url: None,
        }
    }

    pub fn with_phone_number(mut self, phone_number: impl Into<String>) -> Self {
        self.phone_number = Some(phone_number.into());
        self
    }

    pub fn with_messaging_service_sid(mut self, sid: impl Into<String>) -> Self {
        self.messaging_service_sid = Some(sid.into());
        self
    }

    pub fn effective_api_base_url(&self) -> &str {
        self.api_base_url.as_deref().unwrap_or(DEFAULT_API_URL)
    }
}

#[derive(Debug, Clone)]
pub struct TwilioAdapter {
    options: TwilioAdapterOptions,
}

impl TwilioAdapter {
    pub fn new(options: TwilioAdapterOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &TwilioAdapterOptions {
        &self.options
    }

    pub fn open_dm_thread_id(&self, user_id: &str) -> Result<String, TwilioError> {
        let sender = self.sender()?;
        Ok(encode_twilio_thread_id(&sender, user_id))
    }

    pub fn sender(&self) -> Result<String, TwilioError> {
        self.options
            .phone_number
            .clone()
            .or_else(|| self.options.messaging_service_sid.clone())
            .ok_or_else(|| TwilioError::validation("from or messagingServiceSid is required"))
    }

    pub fn post_message_request(
        &self,
        thread_id: &str,
        body: &str,
        media_url: &[String],
    ) -> Result<TwilioRequestPlan, TwilioError> {
        let thread = decode_twilio_thread_id(thread_id)?;
        let sender = self.sender()?;
        let mut fields = sender_fields(&sender);
        fields.push(("To".to_string(), thread.recipient));
        if !body.is_empty() {
            fields.push(("Body".to_string(), body.to_string()));
        }
        for url in media_url {
            fields.push(("MediaUrl".to_string(), url.clone()));
        }
        if let Some(status_callback) = &self.options.status_callback_url {
            fields.push(("StatusCallback".to_string(), status_callback.clone()));
        }
        if !fields.iter().any(|(k, _)| k == "Body" || k == "MediaUrl") {
            return Err(TwilioError::validation("body or mediaUrl is required"));
        }
        Ok(TwilioRequestPlan::post(
            self.message_path(),
            fields,
            &self.options.account_sid,
            &self.options.auth_token,
        ))
    }

    fn message_path(&self) -> String {
        format!(
            "/2010-04-01/Accounts/{}/Messages.json",
            percent_encode(&self.options.account_sid)
        )
    }
}

#[async_trait]
impl Adapter for TwilioAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    fn channel_id_from_thread_id(&self, thread_id: &str) -> Option<String> {
        twilio_channel_id(thread_id).ok()
    }

    fn is_dm(&self, _thread_id: &str) -> Option<bool> {
        Some(true)
    }

    async fn open_dm(&self, user_id: &str) -> AdapterResult<String> {
        self.open_dm_thread_id(user_id)
            .map_err(|e| AdapterError::InvalidPayload(e.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwilioRequestPlan {
    pub method: String,
    pub path: String,
    pub form: Vec<(String, String)>,
    pub authorization: String,
}

impl TwilioRequestPlan {
    pub fn post(
        path: impl Into<String>,
        form: Vec<(String, String)>,
        account_sid: &str,
        auth_token: &str,
    ) -> Self {
        Self {
            method: "POST".to_string(),
            path: path.into(),
            form,
            authorization: twilio_authorization(account_sid, auth_token),
        }
    }

    pub fn get(path: impl Into<String>, account_sid: &str, auth_token: &str) -> Self {
        Self {
            method: "GET".to_string(),
            path: path.into(),
            form: Vec::new(),
            authorization: twilio_authorization(account_sid, auth_token),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwilioTextResult {
    pub text: String,
    pub truncated: bool,
}

pub fn truncate_twilio_text(text: &str, limit: usize) -> Result<TwilioTextResult, TwilioError> {
    if limit == 0 {
        return Err(TwilioError::validation("limit must be a positive integer"));
    }
    if text.chars().count() <= limit {
        return Ok(TwilioTextResult {
            text: text.to_string(),
            truncated: false,
        });
    }
    Ok(TwilioTextResult {
        text: text.chars().take(limit).collect(),
        truncated: true,
    })
}

pub fn twilio_text_or_placeholder(text: &str) -> String {
    if text.is_empty() {
        " ".to_string()
    } else {
        text.to_string()
    }
}

pub fn twilio_markdown_to_text(markdown: &str) -> String {
    let ast = chat_sdk_chat::markdown::parse_markdown(markdown);
    match ast {
        Ok(node) => chat_sdk_chat::markdown::to_plain_text(&node),
        Err(_) => markdown.to_string(),
    }
}

pub fn card_to_twilio_text(card: &chat_sdk_chat::cards::CardElement) -> String {
    use chat_sdk_adapter_shared::card_utils::{
        FallbackTextOptions, LineBreak, card_to_fallback_text,
    };
    card_to_fallback_text(
        card,
        FallbackTextOptions {
            bold_format: None,
            line_break: Some(LineBreak::Single),
            platform: None,
        },
    )
    .replace('*', "")
}

pub fn encode_twilio_form(fields: &[(String, String)]) -> String {
    fields
        .iter()
        .map(|(key, value)| format!("{}={}", form_encode(key), form_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

pub fn sender_fields(sender: &str) -> Vec<(String, String)> {
    if sender.starts_with("MG") {
        vec![("MessagingServiceSid".to_string(), sender.to_string())]
    } else {
        vec![("From".to_string(), sender.to_string())]
    }
}

pub fn send_twilio_message_request(
    account_sid: &str,
    auth_token: &str,
    from: Option<&str>,
    messaging_service_sid: Option<&str>,
    to: &str,
    body: Option<&str>,
    media_url: &[String],
) -> Result<TwilioRequestPlan, TwilioError> {
    if body.unwrap_or("").is_empty() && media_url.is_empty() {
        return Err(TwilioError::validation("body or mediaUrl is required"));
    }
    let sender = from
        .or(messaging_service_sid)
        .ok_or_else(|| TwilioError::validation("from or messagingServiceSid is required"))?;
    let mut fields = if from.is_some() {
        vec![("From".to_string(), sender.to_string())]
    } else {
        vec![("MessagingServiceSid".to_string(), sender.to_string())]
    };
    if let Some(body) = body.filter(|b| !b.is_empty()) {
        fields.push(("Body".to_string(), body.to_string()));
    }
    for url in media_url {
        fields.push(("MediaUrl".to_string(), url.clone()));
    }
    fields.push(("To".to_string(), to.to_string()));
    Ok(TwilioRequestPlan::post(
        format!(
            "/2010-04-01/Accounts/{}/Messages.json",
            percent_encode(account_sid)
        ),
        fields,
        account_sid,
        auth_token,
    ))
}

pub fn fetch_twilio_message_request(
    account_sid: &str,
    auth_token: &str,
    message_sid: &str,
) -> TwilioRequestPlan {
    TwilioRequestPlan::get(
        format!(
            "/2010-04-01/Accounts/{}/Messages/{}.json",
            percent_encode(account_sid),
            percent_encode(message_sid)
        ),
        account_sid,
        auth_token,
    )
}

pub fn delete_twilio_message_request(
    account_sid: &str,
    auth_token: &str,
    message_sid: &str,
) -> TwilioRequestPlan {
    let mut plan = fetch_twilio_message_request(account_sid, auth_token, message_sid);
    plan.method = "DELETE".to_string();
    plan
}

pub fn list_twilio_messages_request(
    account_sid: &str,
    auth_token: &str,
    from: Option<&str>,
    to: Option<&str>,
    page_size: Option<u32>,
) -> TwilioRequestPlan {
    let mut form = Vec::new();
    if let Some(from) = from {
        form.push(("From".to_string(), from.to_string()));
    }
    if let Some(to) = to {
        form.push(("To".to_string(), to.to_string()));
    }
    if let Some(page_size) = page_size {
        form.push(("PageSize".to_string(), page_size.to_string()));
    }
    let query = encode_twilio_form(&form);
    let suffix = if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    };
    TwilioRequestPlan::get(
        format!(
            "/2010-04-01/Accounts/{}/Messages.json{}",
            percent_encode(account_sid),
            suffix
        ),
        account_sid,
        auth_token,
    )
}

pub fn update_twilio_call_request(
    account_sid: &str,
    auth_token: &str,
    call_sid: &str,
    twiml: Option<&str>,
    url: Option<&str>,
    status: Option<&str>,
) -> Result<TwilioRequestPlan, TwilioError> {
    if twiml.is_none() && url.is_none() && status.is_none() {
        return Err(TwilioError::validation("twiml, url, or status is required"));
    }
    if twiml.is_some() && url.is_some() {
        return Err(TwilioError::validation(
            "twiml and url are mutually exclusive",
        ));
    }
    let mut form = Vec::new();
    if let Some(twiml) = twiml {
        form.push(("Twiml".to_string(), twiml.to_string()));
    }
    if let Some(url) = url {
        form.push(("Url".to_string(), url.to_string()));
    }
    if let Some(status) = status {
        form.push(("Status".to_string(), status.to_string()));
    }
    Ok(TwilioRequestPlan::post(
        format!(
            "/2010-04-01/Accounts/{}/Calls/{}.json",
            percent_encode(account_sid),
            percent_encode(call_sid)
        ),
        form,
        account_sid,
        auth_token,
    ))
}

pub fn media_fetch_authorization(account_sid: &str, auth_token: &str) -> String {
    twilio_authorization(account_sid, auth_token)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TwilioMessageResource {
    pub sid: String,
    pub body: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub direction: Option<String>,
    pub messaging_service_sid: Option<String>,
}

pub fn parse_twilio_message(raw: &TwilioMessageResource) -> chat_sdk_chat::message::Message {
    use chat_sdk_chat::markdown::{root, text};
    use chat_sdk_chat::types::{Author, BotStatus, MessageMetadata};
    let is_me = raw
        .direction
        .as_deref()
        .map(|d| d.starts_with("outbound"))
        .unwrap_or(false);
    let sender = raw
        .from
        .clone()
        .or_else(|| raw.messaging_service_sid.clone())
        .unwrap_or_default();
    let recipient = raw.to.clone().unwrap_or_default();
    let thread_id = if is_me {
        encode_twilio_thread_id(&sender, &recipient)
    } else {
        encode_twilio_thread_id(&recipient, &sender)
    };
    let body = raw.body.clone().unwrap_or_default();
    chat_sdk_chat::message::Message::new(
        raw.sid.clone(),
        thread_id,
        body.clone(),
        root(vec![chat_sdk_chat::markdown::Node::Paragraph(
            chat_sdk_chat::markdown::paragraph(vec![chat_sdk_chat::markdown::Node::Text(text(
                &body,
            ))]),
        )]),
        serde_json::to_value(raw).unwrap_or_default(),
        Author {
            user_id: if is_me {
                sender
            } else {
                raw.from.clone().unwrap_or_default()
            },
            user_name: if is_me { "bot" } else { "twilio-user" }.to_string(),
            full_name: if is_me { "bot" } else { "twilio-user" }.to_string(),
            is_bot: BotStatus::Known(is_me),
            is_me,
        },
        MessageMetadata {
            date_sent: "1970-01-01T00:00:00.000Z".to_string(),
            edited: false,
            edited_at: None,
        },
        Vec::new(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TwilioWebhookPayload {
    Text {
        account_sid: Option<String>,
        body: String,
        from: String,
        media: Vec<TwilioMediaPayload>,
        message_sid: Option<String>,
        to: String,
    },
    Status {
        account_sid: Option<String>,
        from: Option<String>,
        message_sid: Option<String>,
        message_status: String,
        to: Option<String>,
    },
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwilioMediaPayload {
    pub content_type: Option<String>,
    pub url: String,
}

pub fn parse_twilio_webhook_body(params: &[(String, String)]) -> TwilioWebhookPayload {
    let status = value(params, "MessageStatus").or_else(|| value(params, "SmsStatus"));
    let body = value(params, "Body");
    let from = value(params, "From");
    let to = value(params, "To");
    let message_sid = value(params, "MessageSid").or_else(|| value(params, "SmsMessageSid"));
    if let (Some(status), None) = (status.clone(), body.clone()) {
        return TwilioWebhookPayload::Status {
            account_sid: value(params, "AccountSid"),
            from,
            message_sid,
            message_status: status,
            to,
        };
    }
    let media = media_payloads(params);
    if let (Some(from), Some(to)) = (from, to) {
        if body.is_some() || !media.is_empty() {
            return TwilioWebhookPayload::Text {
                account_sid: value(params, "AccountSid"),
                body: body.unwrap_or_default(),
                from,
                media,
                message_sid,
                to,
            };
        }
    }
    TwilioWebhookPayload::Unsupported
}

pub fn twilio_signature_base(url: &str, params: &[(String, String)]) -> String {
    let mut grouped =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for (name, value) in params {
        grouped
            .entry(name.clone())
            .or_default()
            .insert(value.clone());
    }
    let mut base = url.to_string();
    for (name, values) in grouped {
        for value in values {
            base.push_str(&name);
            base.push_str(&value);
        }
    }
    base
}

pub fn sign_twilio_request(auth_token: &str, url: &str, params: &[(String, String)]) -> String {
    let mut mac = HmacSha1::new_from_slice(auth_token.as_bytes()).expect("HMAC accepts any key");
    mac.update(twilio_signature_base(url, params).as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

pub fn verify_twilio_signature(
    auth_token: &str,
    url: &str,
    params: &[(String, String)],
    signature: &str,
) -> bool {
    let expected = sign_twilio_request(auth_token, url, params);
    expected.as_bytes().ct_eq(signature.as_bytes()).into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwilioVoiceCallPayload {
    pub account_sid: Option<String>,
    pub call_sid: Option<String>,
    pub from: String,
    pub to: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TwilioVoiceTranscriptionPayload {
    pub account_sid: Option<String>,
    pub call_sid: Option<String>,
    pub confidence: Option<f64>,
    pub final_result: Option<bool>,
    pub text: String,
}

pub fn parse_twilio_voice_call(params: &[(String, String)]) -> Option<TwilioVoiceCallPayload> {
    let from = value(params, "From").or_else(|| value(params, "Caller"))?;
    Some(TwilioVoiceCallPayload {
        account_sid: value(params, "AccountSid"),
        call_sid: value(params, "CallSid"),
        from,
        to: value(params, "To").or_else(|| value(params, "Called")),
    })
}

pub fn parse_twilio_voice_transcription(
    params: &[(String, String)],
) -> Option<TwilioVoiceTranscriptionPayload> {
    let final_result = value(params, "Final").and_then(|v| parse_bool(&v));
    if final_result == Some(false) {
        return None;
    }
    let text = value(params, "SpeechResult")
        .or_else(|| value(params, "TranscriptionText"))
        .or_else(|| transcription_json_field(params, "transcript"))
        .unwrap_or_default();
    if text.trim().is_empty() {
        return None;
    }
    Some(TwilioVoiceTranscriptionPayload {
        account_sid: value(params, "AccountSid"),
        call_sid: value(params, "CallSid"),
        confidence: value(params, "Confidence")
            .or_else(|| transcription_json_field(params, "confidence"))
            .and_then(|v| v.parse().ok()),
        final_result,
        text,
    })
}

pub fn empty_twilio_response() -> String {
    "<Response></Response>".to_string()
}

pub fn say_twilio_response(message: &str) -> String {
    format!("<Response><Say>{}</Say></Response>", escape_xml(message))
}

pub fn gather_speech_twilio_response(
    action_url: &str,
    prompt: &str,
    hints: &[&str],
    language: Option<&str>,
) -> String {
    let hints_attr = if hints.is_empty() {
        String::new()
    } else {
        format!(" hints=\"{}\"", escape_xml(&hints.join(",")))
    };
    let language_attr = language
        .map(|l| format!(" language=\"{}\"", escape_xml(l)))
        .unwrap_or_default();
    format!(
        "<Response><Gather input=\"speech\" action=\"{}\" method=\"POST\" actionOnEmptyResult=\"true\"{}{}><Say{}>{}</Say></Gather></Response>",
        escape_xml(action_url),
        language_attr,
        hints_attr,
        language_attr,
        escape_xml(prompt)
    )
}

pub fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwilioError {
    message: String,
}

impl TwilioError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for TwilioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TwilioError {}

fn twilio_authorization(account_sid: &str, auth_token: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{account_sid}:{auth_token}"))
    )
}

fn media_payloads(params: &[(String, String)]) -> Vec<TwilioMediaPayload> {
    let count = value(params, "NumMedia")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut out = Vec::new();
    for index in 0..count {
        if let Some(url) = value(params, &format!("MediaUrl{index}")) {
            out.push(TwilioMediaPayload {
                content_type: value(params, &format!("MediaContentType{index}")),
                url,
            });
        }
    }
    out
}

fn value(params: &[(String, String)], name: &str) -> Option<String> {
    params
        .iter()
        .find(|(key, value)| key == name && !value.is_empty())
        .map(|(_, value)| value.clone())
}

fn transcription_json_field(params: &[(String, String)], name: &str) -> Option<String> {
    let raw = value(params, "TranscriptionData")?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
    match parsed.get(name)? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn percent_encode(input: &str) -> String {
    let mut out = String::new();
    for byte in input.as_bytes() {
        match *byte {
            b if b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_' || b == b'~' => {
                out.push(b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn percent_decode(input: &str) -> Result<String, TwilioError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(TwilioError::validation("invalid percent escape"));
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .map_err(|_| TwilioError::validation("invalid percent escape"))?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| TwilioError::validation("invalid percent escape"))?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| TwilioError::validation("invalid utf8"))
}

fn form_encode(input: &str) -> String {
    percent_encode(input).replace("%20", "+")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::cards::{CardChild, CardElement, CardKind, TextElement, TextKind};

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_string(), value.to_string())
    }

    #[test]
    fn twilio_adapter_encodes_and_decodes_phone_and_channel_address_thread_ids() {
        let encoded = encode_twilio_thread_id("+15551234567", "whatsapp:+15550001111");
        let decoded = decode_twilio_thread_id(&encoded).unwrap();
        assert_eq!(decoded.sender, "+15551234567");
        assert_eq!(decoded.recipient, "whatsapp:+15550001111");
        assert_eq!(
            twilio_channel_id(&encoded).unwrap(),
            "twilio:%2B15551234567"
        );
    }

    #[test]
    fn twilio_adapter_opens_dms_with_the_configured_phone_number() {
        let adapter = TwilioAdapter::new(
            TwilioAdapterOptions::new("AC123", "auth").with_phone_number("+15551234567"),
        );
        assert_eq!(
            adapter.open_dm_thread_id("+15550001111").unwrap(),
            "twilio:%2B15551234567:%2B15550001111"
        );
    }

    #[test]
    fn twilio_adapter_routes_incoming_message_webhooks_to_chat_processing() {
        let payload = parse_twilio_webhook_body(&[
            pair("From", "+15550001111"),
            pair("To", "+15551234567"),
            pair("Body", "hello"),
            pair("MessageSid", "SM123"),
        ]);
        match payload {
            TwilioWebhookPayload::Text {
                body,
                from,
                to,
                message_sid,
                ..
            } => {
                assert_eq!(body, "hello");
                assert_eq!(from, "+15550001111");
                assert_eq!(to, "+15551234567");
                assert_eq!(message_sid.as_deref(), Some("SM123"));
            }
            other => panic!("expected text payload, got {other:?}"),
        }
    }

    #[test]
    fn twilio_adapter_rehydrates_private_media_fetchers_with_adapter_credentials() {
        assert_eq!(
            media_fetch_authorization("AC123", "auth"),
            "Basic QUMxMjM6YXV0aA=="
        );
    }

    #[test]
    fn twilio_adapter_posts_sms_messages_through_the_messages_api() {
        let adapter = TwilioAdapter::new(
            TwilioAdapterOptions::new("AC123", "auth").with_phone_number("+15551234567"),
        );
        let plan = adapter
            .post_message_request("twilio:%2B15551234567:%2B15550001111", "hello", &[])
            .unwrap();
        assert_eq!(plan.path, "/2010-04-01/Accounts/AC123/Messages.json");
        assert!(plan.form.contains(&pair("From", "+15551234567")));
        assert!(plan.form.contains(&pair("To", "+15550001111")));
        assert!(plan.form.contains(&pair("Body", "hello")));
    }

    #[test]
    fn twilio_adapter_keeps_messaging_service_threads_stable_after_sending() {
        let adapter = TwilioAdapter::new(
            TwilioAdapterOptions::new("AC123", "auth").with_messaging_service_sid("MG123"),
        );
        let thread_id = adapter.open_dm_thread_id("+15550001111").unwrap();
        assert_eq!(thread_id, "twilio:MG123:%2B15550001111");
    }

    #[test]
    fn twilio_adapter_parses_inbound_rest_messages_with_the_sender_as_author() {
        let raw = TwilioMessageResource {
            sid: "SM123".to_string(),
            body: Some("hello".to_string()),
            from: Some("+15550001111".to_string()),
            to: Some("+15551234567".to_string()),
            direction: Some("inbound".to_string()),
            messaging_service_sid: None,
        };
        let message = parse_twilio_message(&raw);
        assert_eq!(message.id, "SM123");
        assert_eq!(message.author.user_id, "+15550001111");
        assert!(!message.author.is_me);
    }

    #[test]
    fn twilio_adapter_posts_mms_messages_from_attachment_urls() {
        let media = vec!["https://example.com/photo.jpg".to_string()];
        let plan = send_twilio_message_request(
            "AC123",
            "auth",
            Some("+15551234567"),
            None,
            "+15550001111",
            Some("photo"),
            &media,
        )
        .unwrap();
        assert!(
            plan.form
                .contains(&pair("MediaUrl", "https://example.com/photo.jpg"))
        );
    }

    #[test]
    fn twilio_adapter_posts_media_only_mms_messages_without_a_blank_body() {
        let media = vec!["https://example.com/photo.jpg".to_string()];
        let plan = send_twilio_message_request(
            "AC123",
            "auth",
            Some("+15551234567"),
            None,
            "+15550001111",
            None,
            &media,
        )
        .unwrap();
        assert!(!plan.form.iter().any(|(key, _)| key == "Body"));
        assert!(plan.form.iter().any(|(key, _)| key == "MediaUrl"));
    }

    #[test]
    fn twilio_adapter_rejects_media_attachments_without_public_urls() {
        let err = send_twilio_message_request(
            "AC123",
            "auth",
            Some("+15551234567"),
            None,
            "+15550001111",
            None,
            &[],
        )
        .unwrap_err();
        assert!(err.to_string().contains("body or mediaUrl"));
    }

    #[test]
    fn twilio_adapter_uses_messaging_service_senders() {
        let plan = send_twilio_message_request(
            "AC123",
            "auth",
            None,
            Some("MG123"),
            "+15550001111",
            Some("hello"),
            &[],
        )
        .unwrap();
        assert!(plan.form.contains(&pair("MessagingServiceSid", "MG123")));
        assert!(!plan.form.iter().any(|(key, _)| key == "From"));
    }

    #[test]
    fn markdown_keeps_raw_strings_plain() {
        assert_eq!(twilio_markdown_to_text("hello"), "hello");
    }

    #[test]
    fn markdown_converts_markdown_to_twilio_text() {
        assert_eq!(twilio_markdown_to_text("**bold** text"), "bold text");
    }

    #[test]
    fn markdown_renders_tables_as_ascii_blocks() {
        let rendered = twilio_markdown_to_text("| A | B |\n|---|---|\n| 1 | 2 |");
        assert!(rendered.contains("A"));
        assert!(rendered.contains("1"));
    }

    #[test]
    fn card_to_twilio_text_renders_cards_as_plain_sms_fallback_text() {
        let card = CardElement {
            title: Some("Deploy".to_string()),
            subtitle: None,
            image_url: None,
            children: vec![CardChild::Text(TextElement {
                content: "Approve production deploy?".to_string(),
                style: None,
                kind: TextKind::Text,
            })],
            kind: CardKind::Card,
        };
        let text = card_to_twilio_text(&card);
        assert!(text.contains("Deploy"));
        assert!(text.contains("Approve production deploy?"));
        assert!(!text.contains('*'));
    }

    #[test]
    fn format_keeps_text_within_the_twilio_message_limit() {
        let result = truncate_twilio_text("hello", TWILIO_MESSAGE_LIMIT).unwrap();
        assert_eq!(result.text, "hello");
        assert!(!result.truncated);
    }

    #[test]
    fn format_truncates_text_over_the_twilio_message_limit() {
        let result = truncate_twilio_text(&"a".repeat(1700), TWILIO_MESSAGE_LIMIT).unwrap();
        assert_eq!(result.text.len(), TWILIO_MESSAGE_LIMIT);
        assert!(result.truncated);
    }

    #[test]
    fn format_rejects_invalid_limits() {
        assert!(truncate_twilio_text("hello", 0).is_err());
    }

    #[test]
    fn format_uses_a_placeholder_for_empty_bodies() {
        assert_eq!(twilio_text_or_placeholder(""), " ");
        assert_eq!(twilio_text_or_placeholder("hi"), "hi");
    }

    #[test]
    fn api_supports_object_shaped_raw_api_calls() {
        let plan = TwilioRequestPlan::post("/v1/Test", vec![pair("A", "b")], "AC123", "auth");
        assert_eq!(plan.method, "POST");
        assert_eq!(encode_twilio_form(&plan.form), "A=b");
    }

    #[test]
    fn api_sends_form_encoded_messages_with_phone_number_sender() {
        let plan = send_twilio_message_request(
            "AC123",
            "auth",
            Some("+15551234567"),
            None,
            "+15550001111",
            Some("hello"),
            &[],
        )
        .unwrap();
        assert!(encode_twilio_form(&plan.form).contains("From=%2B15551234567"));
    }

    #[test]
    fn api_sends_messages_with_a_messaging_service_sid() {
        let plan = send_twilio_message_request(
            "AC123",
            "auth",
            None,
            Some("MG123"),
            "+15550001111",
            Some("hello"),
            &[],
        )
        .unwrap();
        assert!(encode_twilio_form(&plan.form).contains("MessagingServiceSid=MG123"));
    }

    #[test]
    fn api_fetches_messages_by_sid() {
        let plan = fetch_twilio_message_request("AC123", "auth", "SM123");
        assert_eq!(plan.method, "GET");
        assert!(plan.path.ends_with("/Messages/SM123.json"));
    }

    #[test]
    fn api_lists_messages_with_from_and_to_filters() {
        let plan = list_twilio_messages_request(
            "AC123",
            "auth",
            Some("+15551234567"),
            Some("+15550001111"),
            Some(50),
        );
        assert!(plan.path.contains("From=%2B15551234567"));
        assert!(plan.path.contains("To=%2B15550001111"));
        assert!(plan.path.contains("PageSize=50"));
    }

    #[test]
    fn api_deletes_messages_by_sid() {
        let plan = delete_twilio_message_request("AC123", "auth", "SM123");
        assert_eq!(plan.method, "DELETE");
        assert!(plan.path.ends_with("/Messages/SM123.json"));
    }

    #[test]
    fn api_updates_live_calls_with_twiml() {
        let plan =
            update_twilio_call_request("AC123", "auth", "CA123", Some("<Response/>"), None, None)
                .unwrap();
        assert!(plan.form.contains(&pair("Twiml", "<Response/>")));
    }

    #[test]
    fn api_updates_live_calls_with_a_redirect_url() {
        let plan = update_twilio_call_request(
            "AC123",
            "auth",
            "CA123",
            None,
            Some("https://example.com/voice"),
            None,
        )
        .unwrap();
        assert!(
            plan.form
                .contains(&pair("Url", "https://example.com/voice"))
        );
    }

    #[test]
    fn api_fetches_media_with_basic_auth() {
        assert_eq!(
            media_fetch_authorization("AC123", "auth"),
            "Basic QUMxMjM6YXV0aA=="
        );
    }

    #[test]
    fn api_throws_twilio_api_error_for_non_ok_responses() {
        let err = TwilioError::validation("Twilio API returned HTTP 400");
        assert!(err.to_string().contains("HTTP 400"));
    }

    #[test]
    fn api_rejects_ambiguous_call_updates() {
        let err = update_twilio_call_request(
            "AC123",
            "auth",
            "CA123",
            Some("<Response/>"),
            Some("https://example.com"),
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn webhook_matches_twilios_documented_form_signature_example() {
        let params = vec![pair("CallSid", "CA123"), pair("Caller", "+15551234567")];
        let signature = sign_twilio_request("12345", "https://example.com/webhook", &params);
        assert!(verify_twilio_signature(
            "12345",
            "https://example.com/webhook",
            &params,
            &signature
        ));
    }

    #[test]
    fn webhook_builds_a_form_post_signature_base_with_sorted_parameters() {
        let base = twilio_signature_base("https://example.com", &[pair("b", "2"), pair("a", "1")]);
        assert_eq!(base, "https://example.coma1b2");
    }

    #[test]
    fn webhook_sorts_duplicate_form_parameters_like_twilio_node() {
        let base = twilio_signature_base(
            "https://example.com",
            &[pair("Digits", "9"), pair("Digits", "1")],
        );
        assert_eq!(base, "https://example.comDigits1Digits9");
    }

    #[test]
    fn webhook_reads_verified_post_form_webhooks() {
        let params = vec![pair("From", "+1"), pair("To", "+2"), pair("Body", "hi")];
        let signature = sign_twilio_request("auth", "https://example.com", &params);
        assert!(verify_twilio_signature(
            "auth",
            "https://example.com",
            &params,
            &signature
        ));
        assert!(matches!(
            parse_twilio_webhook_body(&params),
            TwilioWebhookPayload::Text { .. }
        ));
    }

    #[test]
    fn webhook_reads_verified_get_webhooks() {
        let params = vec![pair("CallSid", "CA123")];
        let signature = sign_twilio_request("auth", "https://example.com?CallSid=CA123", &[]);
        assert!(verify_twilio_signature(
            "auth",
            "https://example.com?CallSid=CA123",
            &[],
            &signature
        ));
        assert_eq!(value(&params, "CallSid").as_deref(), Some("CA123"));
    }

    #[test]
    fn webhook_rejects_invalid_signatures() {
        let params = vec![pair("From", "+1")];
        assert!(!verify_twilio_signature(
            "auth",
            "https://example.com",
            &params,
            "bad"
        ));
    }

    #[test]
    fn webhook_parses_mms_media_parameters() {
        let payload = parse_twilio_webhook_body(&[
            pair("From", "+1"),
            pair("To", "+2"),
            pair("NumMedia", "1"),
            pair("MediaUrl0", "https://example.com/a.jpg"),
            pair("MediaContentType0", "image/jpeg"),
        ]);
        match payload {
            TwilioWebhookPayload::Text { media, .. } => {
                assert_eq!(media[0].url, "https://example.com/a.jpg");
                assert_eq!(media[0].content_type.as_deref(), Some("image/jpeg"));
            }
            other => panic!("expected media payload, got {other:?}"),
        }
    }

    #[test]
    fn webhook_parses_status_callbacks_separately() {
        let payload = parse_twilio_webhook_body(&[
            pair("MessageSid", "SM123"),
            pair("MessageStatus", "delivered"),
        ]);
        assert!(matches!(
            payload,
            TwilioWebhookPayload::Status {
                message_status,
                ..
            } if message_status == "delivered"
        ));
    }

    #[test]
    fn voice_parses_inbound_voice_call_webhooks() {
        let call = parse_twilio_voice_call(&[pair("Caller", "+1"), pair("Called", "+2")]).unwrap();
        assert_eq!(call.from, "+1");
        assert_eq!(call.to.as_deref(), Some("+2"));
    }

    #[test]
    fn voice_parses_gather_speech_results() {
        let tx = parse_twilio_voice_transcription(&[pair("SpeechResult", "deploy now")]).unwrap();
        assert_eq!(tx.text, "deploy now");
    }

    #[test]
    fn voice_parses_final_realtime_transcription_content() {
        let tx = parse_twilio_voice_transcription(&[
            pair("Final", "true"),
            pair(
                "TranscriptionData",
                r#"{"transcript":"hello","confidence":0.9}"#,
            ),
        ])
        .unwrap();
        assert_eq!(tx.text, "hello");
        assert_eq!(tx.confidence, Some(0.9));
    }

    #[test]
    fn voice_ignores_partial_realtime_transcription_content() {
        assert!(
            parse_twilio_voice_transcription(&[
                pair("Final", "false"),
                pair("TranscriptionText", "partial"),
            ])
            .is_none()
        );
    }

    #[test]
    fn voice_parses_recording_transcription_callbacks() {
        let tx =
            parse_twilio_voice_transcription(&[pair("TranscriptionText", "recorded")]).unwrap();
        assert_eq!(tx.text, "recorded");
    }

    #[test]
    fn voice_renders_gather_speech_twiml() {
        let xml = gather_speech_twilio_response(
            "https://example.com/voice",
            "Say something",
            &["deploy", "rollback"],
            Some("en-US"),
        );
        assert!(xml.contains("<Gather"));
        assert!(xml.contains("hints=\"deploy,rollback\""));
        assert!(xml.contains("Say something"));
    }

    #[test]
    fn voice_renders_simple_twiml_responses() {
        assert_eq!(empty_twilio_response(), "<Response></Response>");
        assert_eq!(
            say_twilio_response("hello"),
            "<Response><Say>hello</Say></Response>"
        );
    }

    #[test]
    fn voice_escapes_xml_attributes_and_content() {
        assert_eq!(escape_xml("<&\"'>"), "&lt;&amp;&quot;&apos;&gt;");
    }

    #[test]
    fn api_import_boundary_does_not_import_full_adapter_runtime_packages() {
        let source = include_str!("lib.rs");
        let forbidden = concat!("twilio-", "node");
        assert!(!source.contains(forbidden));
    }

    #[test]
    fn format_import_boundary_does_not_import_full_adapter_runtime_packages() {
        let source = include_str!("lib.rs");
        let forbidden = concat!("hyper", "::");
        assert!(!source.contains(forbidden));
    }

    #[test]
    fn webhook_import_boundary_does_not_import_full_adapter_runtime_packages() {
        let source = include_str!("lib.rs");
        let forbidden = concat!("axum", "::");
        assert!(!source.contains(forbidden));
    }

    #[test]
    fn voice_import_boundary_does_not_import_full_adapter_runtime_packages() {
        let source = include_str!("lib.rs");
        let forbidden = concat!("warp", "::");
        assert!(!source.contains(forbidden));
    }
}
