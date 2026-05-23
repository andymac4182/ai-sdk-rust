//! Google Workspace Events API helpers for the Google Chat adapter.
//!
//! Partial 1:1 port of
//! `packages/adapter-gchat/src/workspace-events.ts`. This slice
//! covers the pure `decodePubSubMessage` helper + supporting wire
//! types. The HTTP-and-googleapis-SDK-heavy
//! `createSpaceSubscription` / `listSpaceSubscriptions` /
//! `deleteSpaceSubscription` paths are deferred to a follow-up
//! slice that wires up reqwest + Google OAuth.

use std::collections::HashMap;

use base64::Engine;
use serde::{Deserialize, Serialize};

/// Pub/Sub push message wrapper. 1:1 port of upstream
/// `interface PubSubPushMessage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PubSubPushMessage {
    pub message: PubSubMessageBody,
    pub subscription: String,
}

/// Inner body of a Pub/Sub push wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PubSubMessageBody {
    /// Base64-encoded event JSON.
    pub data: String,
    #[serde(rename = "messageId")]
    pub message_id: String,
    #[serde(rename = "publishTime")]
    pub publish_time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attributes: Option<HashMap<String, String>>,
}

/// Decoded Workspace Events notification payload. 1:1 port of
/// upstream `interface WorkspaceEventNotification`. `message` and
/// `reaction` are kept as opaque `serde_json::Value` here — full
/// `GoogleChatMessage` / `GoogleChatReaction` typing lands when the
/// adapter's index.ts is ported.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceEventNotification {
    #[serde(rename = "eventTime")]
    pub event_time: String,
    #[serde(rename = "eventType")]
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction: Option<serde_json::Value>,
    pub subscription: String,
    #[serde(rename = "targetResource")]
    pub target_resource: String,
}

/// Decode a Pub/Sub push message into a
/// [`WorkspaceEventNotification`]. 1:1 port of upstream
/// `decodePubSubMessage(pushMessage)`:
///
/// 1. Base64-decode `message.data` and parse it as JSON.
/// 2. Pull `message` / `reaction` out of the parsed payload.
/// 3. Map CloudEvents-shaped attributes (`ce-subject`, `ce-type`,
///    `ce-time`) to top-level `target_resource` / `event_type` /
///    `event_time`. Falls back to `publishTime` when `ce-time` is
///    absent.
pub fn decode_pub_sub_message(push: &PubSubPushMessage) -> WorkspaceEventNotification {
    let data_bytes = base64::engine::general_purpose::STANDARD
        .decode(push.message.data.as_bytes())
        .unwrap_or_default();
    let payload: serde_json::Value =
        serde_json::from_slice(&data_bytes).unwrap_or(serde_json::Value::Null);

    let message = payload.get("message").filter(|v| !v.is_null()).cloned();
    let reaction = payload.get("reaction").filter(|v| !v.is_null()).cloned();

    let attrs = push.message.attributes.as_ref();
    let target_resource = attrs
        .and_then(|a| a.get("ce-subject"))
        .cloned()
        .unwrap_or_default();
    let event_type = attrs
        .and_then(|a| a.get("ce-type"))
        .cloned()
        .unwrap_or_default();
    let event_time = attrs
        .and_then(|a| a.get("ce-time"))
        .cloned()
        .unwrap_or_else(|| push.message.publish_time.clone());

    WorkspaceEventNotification {
        subscription: push.subscription.clone(),
        target_resource,
        event_type,
        event_time,
        message,
        reaction,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pub_sub_message(
        payload: serde_json::Value,
        attributes: Option<HashMap<String, String>>,
    ) -> PubSubPushMessage {
        let data =
            base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&payload).unwrap());
        PubSubPushMessage {
            message: PubSubMessageBody {
                data,
                message_id: "msg-123".to_string(),
                publish_time: "2024-01-15T10:00:00Z".to_string(),
                attributes,
            },
            subscription: "projects/my-project/subscriptions/my-sub".to_string(),
        }
    }

    // ---------- decodePubSubMessage (4 upstream cases) ----------

    #[test]
    fn should_decode_base64_message_payload() {
        let push = make_pub_sub_message(
            serde_json::json!({
                "message": {"text": "Hello world", "name": "spaces/ABC/messages/123"}
            }),
            None,
        );

        let result = decode_pub_sub_message(&push);
        assert_eq!(
            result
                .message
                .as_ref()
                .and_then(|m| m.get("text"))
                .and_then(|t| t.as_str()),
            Some("Hello world")
        );
        assert_eq!(
            result.subscription,
            "projects/my-project/subscriptions/my-sub"
        );
    }

    #[test]
    fn should_extract_cloud_events_attributes() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "ce-type".to_string(),
            "google.workspace.chat.message.v1.created".to_string(),
        );
        attrs.insert(
            "ce-subject".to_string(),
            "//chat.googleapis.com/spaces/ABC".to_string(),
        );
        attrs.insert("ce-time".to_string(), "2024-01-15T10:00:00Z".to_string());

        let push = make_pub_sub_message(
            serde_json::json!({"message": {"text": "test"}}),
            Some(attrs),
        );

        let result = decode_pub_sub_message(&push);
        assert_eq!(
            result.event_type,
            "google.workspace.chat.message.v1.created"
        );
        assert_eq!(result.target_resource, "//chat.googleapis.com/spaces/ABC");
        assert_eq!(result.event_time, "2024-01-15T10:00:00Z");
    }

    #[test]
    fn should_handle_missing_attributes() {
        let push = make_pub_sub_message(serde_json::json!({"message": {"text": "test"}}), None);

        let result = decode_pub_sub_message(&push);
        assert_eq!(result.event_type, "");
        assert_eq!(result.target_resource, "");
        // Falls back to publishTime.
        assert_eq!(result.event_time, "2024-01-15T10:00:00Z");
    }

    #[test]
    fn should_decode_reaction_payload() {
        let mut attrs = HashMap::new();
        attrs.insert(
            "ce-type".to_string(),
            "google.workspace.chat.reaction.v1.created".to_string(),
        );

        let push = make_pub_sub_message(
            serde_json::json!({
                "reaction": {
                    "name": "spaces/ABC/messages/123/reactions/456",
                    "emoji": {"unicode": "\u{1F44D}"}
                }
            }),
            Some(attrs),
        );

        let result = decode_pub_sub_message(&push);
        let reaction = result.reaction.as_ref().expect("reaction present");
        assert_eq!(
            reaction.get("name").and_then(|n| n.as_str()),
            Some("spaces/ABC/messages/123/reactions/456")
        );
        assert_eq!(
            reaction
                .get("emoji")
                .and_then(|e| e.get("unicode"))
                .and_then(|u| u.as_str()),
            Some("\u{1F44D}")
        );
    }
}
