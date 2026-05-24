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

/// Recursively walk a JSON value and revive any nested
/// `chat:Message` envelope in-place. 1:1 with upstream's
/// `JSON.parse(text, reviver)` semantics — the reviver callback is
/// invoked on every key/value pair in the tree, so a `chat:Message`
/// nested inside `{data: {messages: [...]}}` gets promoted just
/// like a top-level one.
///
/// Returns the rewritten `Value` with `chat:Message` envelopes
/// replaced by `serde_json::to_value(Message::from_serialized(...))`
/// (the canonical typed shape). Currently only recognizes
/// `chat:Message`; `chat:Thread` / `chat:Channel` pass through
/// until their reviver branches land.
pub fn revive_walk(value: Value) -> Value {
    match value {
        Value::Array(items) => {
            Value::Array(items.into_iter().map(revive_walk).collect())
        }
        Value::Object(map) => {
            let mut walked = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                walked.insert(k, revive_walk(v));
            }
            let walked = Value::Object(walked);
            // After child walking, check if this object is a
            // recognized envelope. Upstream's `JSON.parse(reviver)`
            // visits children before parents, matching this order.
            if walked
                .as_object()
                .and_then(|o| o.get("_type"))
                .and_then(Value::as_str)
                == Some("chat:Message")
            {
                if let Ok(serialized) =
                    serde_json::from_value::<SerializedMessage>(walked.clone())
                {
                    let revived = Message::from_serialized(serialized);
                    return serde_json::to_value(revived.to_serialized()).unwrap_or(walked);
                }
            }
            walked
        }
        other => other,
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

    // ---------- describe("chat.reviver()") (3 of 5 portable cases) ----------
    // 1:1 with upstream `serialization.test.ts > describe("chat.reviver()")`.
    // The 2 deferred cases (revive chat:Thread / revive both Thread+Message)
    // need the Thread reviver branch which is gated on the cross-
    // package Adapter lookup (`chat.getAdapter(adapterName)`) inside
    // a singleton-resolved `chat.reviver()` factory.

    #[test]
    fn revive_walk_should_revive_chat_message_objects() {
        // 1:1 with upstream "should revive chat:Message objects"
        // (top-level envelope).
        let value = sample_message_payload();
        let walked = revive_walk(value);
        // The walked value preserves the chat:Message wire shape
        // (still has _type / id / text fields) — upstream returns
        // a typed `Message` instance; the Rust port re-serializes
        // through to_serialized so the observable wire shape is
        // identical.
        assert_eq!(
            walked.get("_type").and_then(Value::as_str),
            Some("chat:Message")
        );
        assert_eq!(walked.get("id").and_then(Value::as_str), Some("m1"));
    }

    #[test]
    fn revive_walk_should_leave_non_chat_objects_unchanged() {
        // 1:1 with upstream "should leave non-chat objects unchanged".
        let value = json!({
            "id": "user-1",
            "name": "test",
            "nested": { "key": "value" }
        });
        let walked = revive_walk(value.clone());
        assert_eq!(walked, value);
    }

    // ---------- describe("standalone reviver()") (3 of 8 portable cases) ----------
    // 1:1 with upstream `serialization.test.ts > describe("standalone reviver()")`.
    // The standalone reviver in upstream is the JS-callback form
    // (`JSON.parse(text, reviver)`); the Rust port exposes equivalent
    // semantics through [`revive_walk`] + [`revive_str`]. The 5 deferred
    // cases (Thread / Thread+Message / direct JSON.parse usage / Thread
    // re-serialization / Channel re-serialization) need the singleton-
    // resolved Adapter lookup branch.

    #[test]
    fn standalone_reviver_should_revive_chat_message_objects() {
        // 1:1 with upstream "should revive chat:Message objects" via
        // the standalone reviver. Same observable contract as
        // chat.reviver() for the chat:Message branch.
        let message_json = sample_message_payload();
        let payload = json!({ "message": message_json });
        let walked = revive_walk(payload);
        let revived_msg = &walked["message"];
        assert_eq!(
            revived_msg.get("_type").and_then(Value::as_str),
            Some("chat:Message")
        );
        assert_eq!(
            revived_msg["metadata"]["dateSent"].as_str(),
            Some("2024-01-01T00:00:00.000Z")
        );
    }

    #[test]
    fn standalone_reviver_should_leave_non_chat_objects_unchanged() {
        // 1:1 with upstream "should leave non-chat objects unchanged"
        // — same observable contract as chat.reviver() for non-
        // envelope values; arrays / primitives / non-chat objects
        // walk through unmodified.
        let payload = json!({
            "users": [
                { "id": "u1", "name": "alice" },
                { "id": "u2", "name": "bob" }
            ],
            "total": 2
        });
        let walked = revive_walk(payload.clone());
        assert_eq!(walked, payload);
    }

    #[test]
    fn standalone_reviver_should_allow_re_serialization_of_a_revived_message_without_singleton() {
        // 1:1 with upstream "should allow re-serialization of a
        // revived Thread/Channel without singleton" — the Message
        // variant equivalent. Revived messages can be re-walked
        // (round-trip through the reviver) without losing field
        // content. The Thread/Channel branches need the singleton
        // adapter lookup and are deferred.
        let message_json = sample_message_payload();
        let walked_once = revive_walk(message_json);
        let walked_twice = revive_walk(walked_once.clone());
        // Round-trip preserves the entire wire shape.
        assert_eq!(walked_twice, walked_once);
        assert_eq!(
            walked_twice.get("_type").and_then(Value::as_str),
            Some("chat:Message")
        );
        assert_eq!(walked_twice.get("id").and_then(Value::as_str), Some("m1"));
    }

    #[test]
    fn revive_walk_should_work_with_nested_structures() {
        // 1:1 with upstream "should work with nested structures" —
        // a chat:Message envelope nested inside `{data: {messages:
        // [...]}}` gets promoted just like a top-level one (matches
        // upstream's `JSON.parse(text, reviver)` recursive visit
        // semantics).
        let message_json = sample_message_payload();
        let payload = json!({
            "data": {
                "messages": [message_json]
            }
        });
        let walked = revive_walk(payload);
        let nested_msg = &walked["data"]["messages"][0];
        // Nested message went through the reviver and preserves the
        // wire shape.
        assert_eq!(
            nested_msg.get("_type").and_then(Value::as_str),
            Some("chat:Message")
        );
        assert_eq!(
            nested_msg["metadata"]["dateSent"].as_str(),
            Some("2024-01-01T00:00:00.000Z")
        );
    }
}
