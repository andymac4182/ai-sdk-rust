//! Standalone JSON reviver for Chat SDK objects.
//!
//! 1:1 port (in progress) of `packages/chat/src/reviver.ts`.
//!
//! Upstream's `reviver(key, value)` is a `JSON.parse(s, reviver)`
//! callback that promotes serialized envelopes back into their typed
//! representations:
//! - `_type: "chat:Message"` -> `Message.fromJSON(value)`
//! - `_type: "chat:Thread"`  -> `ThreadImpl.fromJSON(value)`  *(deferred)*
//! - `_type: "chat:Channel"` -> `ChannelImpl.fromJSON(value)`  *(deferred)*
//!
//! **Rust port shape.** `serde_json::from_str` doesn't take a reviver
//! callback. The equivalent flow is:
//!
//! 1. Parse a JSON string into a [`serde_json::Value`].
//! 2. Walk the tree and replace any object carrying `_type: "chat:*"`
//!    with the corresponding typed struct.
//!
//! This module ships the dispatcher [`revive_value`] for the
//! `chat:Message` branch only — `ThreadImpl` and `ChannelImpl`
//! haven't been ported yet, so the corresponding branches pass
//! through with the `_type` tag intact. The reviver gains a new branch
//! as each class lands.

use serde_json::Value;

use crate::message::{Message, SerializedMessage};

/// Result of [`revive_value`]. Carries the original `Value` for
/// non-chat-SDK objects (so the reviver doesn't lose unknown wire
/// shapes) and a typed Rust struct for the recognized `_type` tags.
#[derive(Debug, Clone, PartialEq)]
pub enum Revived {
    /// Pass-through for anything the reviver doesn't recognize.
    PassThrough(Value),
    /// `_type: "chat:Message"` payload, promoted via
    /// [`Message::from_serialized`].
    Message(Message),
}

impl From<Value> for Revived {
    fn from(value: Value) -> Self {
        Self::PassThrough(value)
    }
}

/// Parse a JSON string and revive the resulting top-level value in
/// one step. 1:1 with upstream's canonical
/// `JSON.parse(text, reviver)` usage at the chat-SDK call sites that
/// rehydrate transcripts / workflow snapshots from disk.
///
/// Returns `Err(serde_json::Error)` if the input is not valid JSON;
/// malformed-but-valid `chat:Message` envelopes fall through to
/// [`Revived::PassThrough`] (matching upstream's permissive
/// try/catch posture inside the reviver itself).
pub fn revive_str(text: &str) -> Result<Revived, serde_json::Error> {
    let value: Value = serde_json::from_str(text)?;
    Ok(revive_value(value))
}

/// Promote a [`serde_json::Value`] into a typed Rust struct when its
/// `_type` field matches a known chat-SDK envelope. 1:1 port (in
/// progress) of upstream `reviver(_key, value)`.
///
/// Returns [`Revived::PassThrough`] for unrecognized values, matching
/// upstream's `return value` fallback.
pub fn revive_value(value: Value) -> Revived {
    let type_tag = value
        .as_object()
        .and_then(|obj| obj.get("_type"))
        .and_then(Value::as_str);

    match type_tag {
        Some("chat:Message") => {
            // Attempt to deserialize; if the shape is malformed, fall
            // back to pass-through so the caller can see the raw value
            // rather than panicking. Mirrors upstream's permissive
            // try/catch posture across `JSON.parse(reviver)`.
            match serde_json::from_value::<SerializedMessage>(value.clone()) {
                Ok(serialized) => Revived::Message(Message::from_serialized(serialized)),
                Err(_) => Revived::PassThrough(value),
            }
        }
        // chat:Thread / chat:Channel pass through until their Rust
        // impls land. The _type tag stays on the wire so a later
        // revive pass can pick them up once the classes ship.
        _ => Revived::PassThrough(value),
    }
}

#[cfg(test)]
mod tests {
    //! Additive coverage. Upstream's `reviver.test.ts` does not exist;
    //! reviver behavior is exercised via integration tests with
    //! `JSON.parse(input, reviver)`. The Rust port locks in the
    //! per-branch dispatch on `_type`.
    use super::*;
    use crate::markdown::root;
    use crate::message::MessageKind;
    use crate::types::{Author, BotStatus, MessageMetadata};
    use serde_json::json;

    fn sample_message_payload() -> Value {
        serde_json::to_value(SerializedMessage {
            kind: MessageKind::Message,
            id: "m1".to_string(),
            thread_id: "t1".to_string(),
            text: "hi".to_string(),
            formatted: root(vec![]),
            raw: json!({}),
            author: Author {
                user_id: "U".to_string(),
                user_name: "u".to_string(),
                full_name: "U".to_string(),
                is_bot: BotStatus::Known(false),
                is_me: false,
            },
            metadata: MessageMetadata {
                date_sent: "2024-01-01T00:00:00.000Z".to_string(),
                edited: false,
                edited_at: None,
            },
            attachments: vec![],
            is_mention: None,
            links: None,
        })
        .unwrap()
    }

    #[test]
    fn revive_value_promotes_chat_message_envelopes_to_typed_messages() {
        let value = sample_message_payload();
        match revive_value(value) {
            Revived::Message(m) => assert_eq!(m.id, "m1"),
            other => panic!("expected Revived::Message, got {other:?}"),
        }
    }

    #[test]
    fn revive_value_passes_through_objects_with_no_type_tag() {
        let value = json!({"name": "no type tag"});
        match revive_value(value.clone()) {
            Revived::PassThrough(v) => assert_eq!(v, value),
            other => panic!("expected PassThrough, got {other:?}"),
        }
    }

    #[test]
    fn revive_value_passes_through_chat_thread_until_thread_impl_lands() {
        // Documented in the module header: chat:Thread / chat:Channel
        // pass through until ThreadImpl / ChannelImpl ship.
        let value = json!({"_type": "chat:Thread", "id": "t1"});
        match revive_value(value.clone()) {
            Revived::PassThrough(v) => assert_eq!(v, value),
            other => panic!("expected PassThrough, got {other:?}"),
        }
    }

    #[test]
    fn revive_value_passes_through_chat_channel_until_channel_impl_lands() {
        let value = json!({"_type": "chat:Channel", "id": "c1"});
        match revive_value(value.clone()) {
            Revived::PassThrough(v) => assert_eq!(v, value),
            other => panic!("expected PassThrough, got {other:?}"),
        }
    }

    #[test]
    fn revive_value_passes_through_non_objects() {
        for raw in [json!(null), json!("string"), json!(42), json!([1, 2])] {
            match revive_value(raw.clone()) {
                Revived::PassThrough(v) => assert_eq!(v, raw),
                other => panic!("expected PassThrough, got {other:?}"),
            }
        }
    }

    #[test]
    fn revive_value_falls_back_to_passthrough_on_malformed_message_shape() {
        // `_type` says chat:Message but the body is missing required
        // fields. Mirror upstream's permissive fall-through.
        let value = json!({"_type": "chat:Message", "id": "incomplete"});
        match revive_value(value.clone()) {
            Revived::PassThrough(v) => assert_eq!(v, value),
            other => panic!("expected PassThrough, got {other:?}"),
        }
    }

    // ---------- slice 107: revive_str helper ----------

    #[test]
    fn revive_str_parses_and_promotes_chat_message_envelope() {
        let value = sample_message_payload();
        let text = serde_json::to_string(&value).unwrap();
        match revive_str(&text).unwrap() {
            Revived::Message(m) => assert_eq!(m.id, "m1"),
            other => panic!("expected Revived::Message, got {other:?}"),
        }
    }

    #[test]
    fn revive_str_passes_through_objects_with_unknown_type_tags() {
        let text = r#"{"_type":"chat:Thread","id":"t1"}"#;
        match revive_str(text).unwrap() {
            Revived::PassThrough(v) => {
                assert_eq!(v.get("_type").and_then(Value::as_str), Some("chat:Thread"));
            }
            other => panic!("expected PassThrough, got {other:?}"),
        }
    }

    #[test]
    fn revive_str_passes_through_primitive_json_values() {
        // The reviver is shaped around envelope objects; bare strings /
        // numbers / arrays parse and fall through verbatim.
        match revive_str("\"hello\"").unwrap() {
            Revived::PassThrough(v) => assert_eq!(v, json!("hello")),
            other => panic!("expected PassThrough, got {other:?}"),
        }
        match revive_str("42").unwrap() {
            Revived::PassThrough(v) => assert_eq!(v, json!(42)),
            other => panic!("expected PassThrough, got {other:?}"),
        }
        match revive_str("[1,2,3]").unwrap() {
            Revived::PassThrough(v) => assert_eq!(v, json!([1, 2, 3])),
            other => panic!("expected PassThrough, got {other:?}"),
        }
    }

    #[test]
    fn revive_str_returns_err_for_invalid_json() {
        assert!(revive_str("not-json").is_err());
        assert!(revive_str("").is_err());
        assert!(revive_str("{").is_err());
    }
}
