//! WhatsApp Business Cloud API adapter for chat-sdk.
//!
//! 1:1 port (in progress) of `packages/adapter-whatsapp/src/index.ts`.
//!
//! WhatsApp maps each (business phone number, customer phone number)
//! DM pair to one chat-sdk thread. The thread id encoding is
//! `whatsapp:<phone_number_id>:<customer_phone>`.

pub mod cards;
pub mod markdown;
pub mod webhook;

use async_trait::async_trait;
use chat_sdk_chat::types::Adapter;

/// Adapter name discriminator.
pub const ADAPTER_NAME: &str = "whatsapp";

/// Thread-id prefix.
pub const THREAD_ID_PREFIX: &str = "whatsapp:";

/// Default WhatsApp Cloud API base URL (the Meta Graph endpoint).
pub const DEFAULT_GRAPH_BASE: &str = "https://graph.facebook.com";

/// Options for [`WhatsappAdapter::new`].
#[derive(Debug, Clone)]
pub struct WhatsappAdapterOptions {
    /// Business phone-number ID (Meta-issued identifier).
    pub phone_number_id: String,
    /// Permanent access token (Meta business token).
    pub access_token: String,
    /// Webhook verify token.
    pub verify_token: String,
    /// Optional Graph API base URL override.
    pub graph_base: Option<String>,
}

impl WhatsappAdapterOptions {
    /// Construct options. Graph base URL defaults to
    /// [`DEFAULT_GRAPH_BASE`].
    pub fn new(
        phone_number_id: impl Into<String>,
        access_token: impl Into<String>,
        verify_token: impl Into<String>,
    ) -> Self {
        Self {
            phone_number_id: phone_number_id.into(),
            access_token: access_token.into(),
            verify_token: verify_token.into(),
            graph_base: None,
        }
    }

    /// Override the Graph API base URL.
    pub fn with_graph_base(mut self, graph_base: impl Into<String>) -> Self {
        self.graph_base = Some(graph_base.into());
        self
    }

    /// Effective Graph API base URL with default applied.
    pub fn effective_graph_base(&self) -> &str {
        self.graph_base.as_deref().unwrap_or(DEFAULT_GRAPH_BASE)
    }
}

/// WhatsApp Cloud API adapter.
#[derive(Debug, Clone)]
pub struct WhatsappAdapter {
    options: WhatsappAdapterOptions,
    http: chat_sdk_adapter_shared::runtime::reqwest::Client,
}

impl WhatsappAdapter {
    /// 1:1 port of upstream
    /// `new WhatsappAdapter({ phoneNumberId, accessToken, verifyToken, graphBase? })`.
    pub fn new(options: WhatsappAdapterOptions) -> Self {
        Self {
            options,
            http: chat_sdk_adapter_shared::runtime::default_http_client(),
        }
    }

    /// Override the HTTP client.
    pub fn with_http_client(
        mut self,
        client: chat_sdk_adapter_shared::runtime::reqwest::Client,
    ) -> Self {
        self.http = client;
        self
    }

    /// Read the business phone-number ID.
    pub fn phone_number_id(&self) -> &str {
        &self.options.phone_number_id
    }

    /// Read the access token.
    pub fn access_token(&self) -> &str {
        &self.options.access_token
    }

    /// Read the webhook verify token.
    pub fn verify_token(&self) -> &str {
        &self.options.verify_token
    }

    /// Effective Graph API base URL.
    pub fn graph_base(&self) -> &str {
        self.options.effective_graph_base()
    }

    /// Build the Cloud API send URL. 1:1 with upstream's inline
    /// `<graph_base>/v22.0/<phone_number_id>/messages` template.
    fn send_url(&self) -> String {
        format!(
            "{}/v22.0/{}/messages",
            self.graph_base(),
            self.options.phone_number_id
        )
    }
}

#[async_trait]
impl Adapter for WhatsappAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    /// Post a text message via the WhatsApp Cloud API. 1:1 with
    /// upstream's `adapter.postMessage`:
    ///
    /// - Decodes `whatsapp:<phone_number_id>:<customer_phone>`.
    /// - POSTs JSON
    ///   `{messaging_product: "whatsapp", to: <customer_phone>,
    ///   type: "text", text: {body: <text>}}` to
    ///   `<graph_base>/v22.0/<phone_number_id>/messages`.
    /// - Auth via `Authorization: Bearer <access_token>` header.
    /// - Returns the first element of `messages[*].id` (Cloud
    ///   API's envelope).
    async fn post_message(
        &self,
        thread_id: &str,
        text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not WhatsApp-encoded"))
        })?;

        // The thread id's phone_number_id MUST match the adapter's
        // configured phone_number_id (the bot is keyed by phone
        // number on the Meta side).
        if decoded.phone_number_id != self.options.phone_number_id {
            return Err(AdapterError::InvalidPayload(format!(
                "thread_id phone_number_id {:?} does not match adapter's {:?}",
                decoded.phone_number_id, self.options.phone_number_id
            )));
        }

        let url = self.send_url();
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": decoded.customer_phone,
            "type": "text",
            "text": { "body": text },
        });

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.access_token())
            .json(&body)
            .send()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        let status = response.status();
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        if !status.is_success() {
            let error_msg = json["error"]["message"]
                .as_str()
                .unwrap_or("WhatsApp Cloud API call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {error_msg}"
            )));
        }

        json["messages"][0]["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                AdapterError::InvalidPayload(
                    "WhatsApp Cloud API response missing messages[0].id".to_string(),
                )
            })
    }

    /// WhatsApp does not support editing messages. 1:1 with
    /// upstream's `adapter.editMessage`.
    async fn edit_message(
        &self,
        _thread_id: &str,
        _message_id: &str,
        _text: &str,
    ) -> chat_sdk_chat::types::AdapterResult<String> {
        use chat_sdk_chat::types::AdapterError;
        Err(AdapterError::InvalidPayload(
            "WhatsApp does not support editing messages. Use postMessage to send a new message instead.".to_string(),
        ))
    }

    /// WhatsApp does not support deleting messages. 1:1 with
    /// upstream's `adapter.deleteMessage`.
    async fn delete_message(
        &self,
        _thread_id: &str,
        _message_id: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;
        Err(AdapterError::InvalidPayload(
            "WhatsApp does not support deleting messages.".to_string(),
        ))
    }

    /// Add an emoji reaction via WhatsApp Cloud API. 1:1 with
    /// upstream's `adapter.addReaction`: POST `{messaging_product:
    /// "whatsapp", recipient_type: "individual", to: <customer_phone>,
    /// type: "reaction", reaction: {message_id, emoji}}`.
    async fn add_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        use chat_sdk_chat::types::AdapterError;

        let decoded = decode_thread_id(thread_id).ok_or_else(|| {
            AdapterError::InvalidPayload(format!("thread_id {thread_id:?} is not WhatsApp-encoded"))
        })?;

        if decoded.phone_number_id != self.options.phone_number_id {
            return Err(AdapterError::InvalidPayload(format!(
                "thread_id phone_number_id {:?} does not match adapter's {:?}",
                decoded.phone_number_id, self.options.phone_number_id
            )));
        }

        let url = self.send_url();
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": decoded.customer_phone,
            "type": "reaction",
            "reaction": {
                "message_id": message_id,
                "emoji": emoji,
            },
        });

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.access_token())
            .json(&body)
            .send()
            .await
            .map_err(|err| AdapterError::Io(Box::new(err)))?;

        let status = response.status();
        if !status.is_success() {
            let json: serde_json::Value = response.json().await.unwrap_or_default();
            let error_msg = json["error"]["message"]
                .as_str()
                .unwrap_or("WhatsApp Cloud API call failed");
            return Err(AdapterError::InvalidPayload(format!(
                "{status}: {error_msg}"
            )));
        }

        Ok(())
    }

    /// WhatsApp Cloud API does not support typing indicators. 1:1
    /// with upstream's no-op `adapter.startTyping`.
    async fn start_typing(
        &self,
        _thread_id: &str,
        _status: Option<&str>,
    ) -> chat_sdk_chat::types::AdapterResult<()> {
        Ok(())
    }
}

/// Encode a WhatsApp thread id. 1:1 with upstream's inline format:
/// `whatsapp:<phone_number_id>:<customer_phone>`.
pub fn encode_thread_id(phone_number_id: &str, customer_phone: &str) -> String {
    format!("{THREAD_ID_PREFIX}{phone_number_id}:{customer_phone}")
}

/// Components of a decoded WhatsApp thread id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecodedWhatsappThreadId {
    /// Business phone-number ID.
    pub phone_number_id: String,
    /// Customer phone number (E.164 form).
    pub customer_phone: String,
}

/// Decode a WhatsApp thread id.
pub fn decode_thread_id(thread_id: &str) -> Option<DecodedWhatsappThreadId> {
    let suffix = thread_id.strip_prefix(THREAD_ID_PREFIX)?;
    let mut parts = suffix.splitn(2, ':');
    let phone_number_id = parts.next()?;
    let customer_phone = parts.next()?;
    if phone_number_id.is_empty() || customer_phone.is_empty() {
        return None;
    }
    Some(DecodedWhatsappThreadId {
        phone_number_id: phone_number_id.to_string(),
        customer_phone: customer_phone.to_string(),
    })
}

/// Predicate: does this thread id belong to the WhatsApp adapter?
pub fn is_whatsapp_thread_id(thread_id: &str) -> bool {
    thread_id.starts_with(THREAD_ID_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_executor::block_on;

    #[test]
    fn adapter_name_is_whatsapp() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("PNID", "access", "verify"));
        assert_eq!(adapter.name(), "whatsapp");
        assert_eq!(ADAPTER_NAME, "whatsapp");
    }

    #[test]
    fn options_new_stores_credentials_and_defaults_graph_base() {
        let opts = WhatsappAdapterOptions::new("PNID", "access", "verify");
        assert_eq!(opts.phone_number_id, "PNID");
        assert_eq!(opts.access_token, "access");
        assert_eq!(opts.verify_token, "verify");
        assert_eq!(opts.effective_graph_base(), DEFAULT_GRAPH_BASE);
    }

    #[test]
    fn options_with_graph_base_overrides_the_default() {
        let opts = WhatsappAdapterOptions::new("p", "a", "v")
            .with_graph_base("https://graph.example.test/v20.0");
        assert_eq!(
            opts.effective_graph_base(),
            "https://graph.example.test/v20.0"
        );
    }

    #[test]
    fn encode_thread_id_builds_the_upstream_format() {
        assert_eq!(
            encode_thread_id("PNID123", "15551234567"),
            "whatsapp:PNID123:15551234567"
        );
    }

    #[test]
    fn decode_thread_id_parses_phone_number_id_and_customer_phone() {
        let decoded = decode_thread_id("whatsapp:PNID123:15551234567").unwrap();
        assert_eq!(decoded.phone_number_id, "PNID123");
        assert_eq!(decoded.customer_phone, "15551234567");
    }

    #[test]
    fn decode_thread_id_returns_none_for_other_prefixes() {
        assert!(decode_thread_id("messenger:PAGE:USER").is_none());
        assert!(decode_thread_id("telegram:123").is_none());
        assert!(decode_thread_id("").is_none());
    }

    #[test]
    fn decode_thread_id_returns_none_for_missing_components() {
        assert!(decode_thread_id("whatsapp:onlyone").is_none());
        assert!(decode_thread_id("whatsapp::15551234567").is_none());
        assert!(decode_thread_id("whatsapp:PNID:").is_none());
    }

    #[test]
    fn is_whatsapp_thread_id_detects_the_prefix() {
        assert!(is_whatsapp_thread_id("whatsapp:PNID:CUST"));
        assert!(!is_whatsapp_thread_id("messenger:1:2"));
        assert!(!is_whatsapp_thread_id(""));
    }

    #[test]
    fn encode_decode_round_trip() {
        for (p, c) in [
            ("PNID", "15551234567"),
            ("a", "b"),
            ("with-dash", "with.dot"),
        ] {
            let encoded = encode_thread_id(p, c);
            let decoded = decode_thread_id(&encoded).unwrap();
            assert_eq!(decoded.phone_number_id, p);
            assert_eq!(decoded.customer_phone, c);
        }
    }

    #[test]
    fn adapter_post_message_rejects_non_whatsapp_thread_ids() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("p", "a", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("slack:C1:1.0", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not WhatsApp-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_post_message_rejects_thread_id_with_mismatched_phone_number_id() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("PNID1", "a", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.post_message("whatsapp:PNID2:15551234567", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("does not match"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_edit_message_is_unsupported() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("P", "a", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.edit_message("whatsapp:P:1234567890", "msg", "hi"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("does not support editing"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_delete_message_is_unsupported() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("P", "a", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.delete_message("whatsapp:P:1234567890", "msg"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("does not support deleting"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_add_reaction_rejects_non_whatsapp_thread_ids() {
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("P", "a", "v"));
        use chat_sdk_chat::types::AdapterError;
        let err = block_on(adapter.add_reaction("slack:C1:1.0", "msg", "👍"));
        match err {
            Err(AdapterError::InvalidPayload(msg)) => {
                assert!(msg.contains("not WhatsApp-encoded"));
            }
            other => panic!("expected InvalidPayload, got {other:?}"),
        }
    }

    #[test]
    fn adapter_start_typing_is_a_noop() {
        // WhatsApp Cloud API doesn't expose typing indicators -
        // upstream's body is empty.
        let adapter = WhatsappAdapter::new(WhatsappAdapterOptions::new("P", "a", "v"));
        assert!(block_on(adapter.start_typing("anything", None)).is_ok());
        assert!(block_on(adapter.start_typing("anything", Some("s"))).is_ok());
    }

    #[test]
    fn adapter_send_url_builds_the_upstream_endpoint() {
        let adapter = WhatsappAdapter::new(
            WhatsappAdapterOptions::new("PNID123", "a", "v")
                .with_graph_base("https://graph.example.test"),
        );
        assert_eq!(
            adapter.send_url(),
            "https://graph.example.test/v22.0/PNID123/messages"
        );
    }

    #[test]
    fn adapter_credential_accessors() {
        let adapter = WhatsappAdapter::new(
            WhatsappAdapterOptions::new("PNID", "access-tok", "verify-tok")
                .with_graph_base("https://example.test"),
        );
        assert_eq!(adapter.phone_number_id(), "PNID");
        assert_eq!(adapter.access_token(), "access-tok");
        assert_eq!(adapter.verify_token(), "verify-tok");
        assert_eq!(adapter.graph_base(), "https://example.test");
    }
}
