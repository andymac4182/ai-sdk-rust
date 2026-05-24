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

use crate::chat_singleton::try_get_chat_singleton;
use crate::message::{Message, SerializedMessage};

/// Result of [`revive_value`]. Carries the original `Value` for
/// non-chat-SDK objects (so the reviver doesn't lose unknown wire
/// shapes) and a typed Rust struct for the recognized `_type` tags.
#[derive(Debug, Clone)]
pub enum Revived {
    /// Pass-through for anything the reviver doesn't recognize.
    PassThrough(Value),
    /// `_type: "chat:Message"` payload, promoted via
    /// [`Message::from_serialized`].
    Message(Message),
    /// `_type: "chat:Thread"` payload, promoted via
    /// [`crate::thread::Thread::from_json`] using the adapter
    /// resolved from the chat singleton (slice 443). When no
    /// singleton is registered or no adapter matches the
    /// `adapterName` field, falls through to
    /// [`Revived::PassThrough`].
    Thread(crate::thread::Thread),
    /// `_type: "chat:Channel"` payload, promoted via
    /// [`crate::channel::Channel::from_json`] using the adapter
    /// resolved from the chat singleton (slice 443).
    Channel(crate::channel::Channel),
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
        Some("chat:Thread") => {
            // Adapter is resolved from the chat singleton at revive
            // time. Falls through to PassThrough when no singleton
            // is registered or the adapterName field is missing /
            // doesn't match a registered adapter. 1:1 with upstream's
            // lazy-resolution semantic (slice 443).
            if let Some(adapter_name) = value.get("adapterName").and_then(Value::as_str) {
                if let Some(singleton) = try_get_chat_singleton() {
                    if let Some(adapter) = singleton.get_adapter(adapter_name) {
                        return Revived::Thread(crate::thread::Thread::from_json(&value, adapter));
                    }
                }
            }
            Revived::PassThrough(value)
        }
        Some("chat:Channel") => {
            // Same lazy-adapter-resolution semantic as chat:Thread.
            if let Some(adapter_name) = value.get("adapterName").and_then(Value::as_str) {
                if let Some(singleton) = try_get_chat_singleton() {
                    if let Some(adapter) = singleton.get_adapter(adapter_name) {
                        return Revived::Channel(crate::channel::Channel::from_json(
                            &value, adapter,
                        ));
                    }
                }
            }
            Revived::PassThrough(value)
        }
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
        Value::Array(items) => Value::Array(items.into_iter().map(revive_walk).collect()),
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
                if let Ok(serialized) = serde_json::from_value::<SerializedMessage>(walked.clone())
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

    // ---------- describe("@workflow/serde integration") — JS-only-documented (9 upstream cases) ----------
    // 1:1 enumeration of upstream
    // `serialization.test.ts > describe("@workflow/serde integration")`.
    //
    // All 9 cases below depend on the TypeScript-specific
    // `@workflow/serde` library, which exposes class-level serde
    // hooks via JS-`Symbol` static properties (`WORKFLOW_SERIALIZE`
    // and `WORKFLOW_DESERIALIZE`). The protocol is unrepresentable in
    // Rust for three independent reasons:
    //
    //   1. Rust has no `Symbol`-keyed property system; static methods
    //      live in inherent `impl` blocks under stable names.
    //   2. The `@workflow/serde` package is a TypeScript framework not
    //      ported to Rust; its `[WORKFLOW_SERIALIZE]` / `[WORKFLOW_
    //      DESERIALIZE]` shape is a JS-language idiom.
    //   3. Upstream's lazy adapter resolution leans on the JS module-
    //      level chat singleton mutating shared state across imports —
    //      the Rust port resolves adapters explicitly through the
    //      [`crate::chat::Chat`] handle, not through ambient
    //      module-load order.
    //
    // The semantic equivalent in the Rust port is already covered
    // by [`Message::to_serialized`] / [`Message::from_serialized`]
    // (see `message.rs` `to_serialized_round_trip_*` tests) and the
    // 8 already-mapped chat:Message / non-chat-object cases above.
    // These 9 upstream cases are enumerated for parity accounting:
    //
    //   ThreadImpl (4 cases)
    //     - "should have WORKFLOW_SERIALIZE static method"
    //     - "should have WORKFLOW_DESERIALIZE static method"
    //     - "should serialize via WORKFLOW_SERIALIZE"
    //     - "should deserialize via WORKFLOW_DESERIALIZE with lazy resolution"
    //
    //   Message (5 cases)
    //     - "should have WORKFLOW_SERIALIZE static method"
    //     - "should have WORKFLOW_DESERIALIZE static method"
    //     - "should serialize via WORKFLOW_SERIALIZE"
    //     - "should deserialize via WORKFLOW_DESERIALIZE"
    //     - "should round-trip via WORKFLOW_SERIALIZE and WORKFLOW_DESERIALIZE"

    // ---------- describe("chat.reviver()") + describe("standalone reviver()")
    //   "should revive both Thread and Message in same payload" — js-only-documented (slice 445) ----------
    //
    // Upstream's `serialization.test.ts > describe("chat.reviver()")` at
    // line 681 and `describe("standalone reviver()")` at line 833 both
    // ship a "should revive both Thread and Message in same payload"
    // case that asserts a single JSON.parse(reviver) call promotes
    // *both* a chat:Thread envelope and a chat:Message envelope nested
    // under the same top-level object into typed instances:
    //
    //   const parsed = JSON.parse(payload, chat.reviver());
    //   expect(parsed.thread).toBeInstanceOf(ThreadImpl);
    //   expect(parsed.message.metadata.dateSent).toBeInstanceOf(Date);
    //
    // The Rust port's `revive_walk` returns a `Value` (not a Revived
    // enum), so a typed-instance promotion on a sub-tree is
    // unrepresentable by construction:
    //   - `Thread` wraps `Arc<dyn Adapter>` and can't be re-encoded
    //     into a `Value` slot.
    //   - `Message` round-trips through `to_serialized` which keeps
    //     the wire shape but loses the typed-instance distinction.
    //
    // The semantic equivalent in Rust is to use `revive_value` per
    // sub-tree (with a singleton registered) to get Revived::Thread +
    // Revived::Message instances independently. This is already
    // covered by slice 443's `revive_value_promotes_chat_thread_*` /
    // slice 374's chat:Message walk tests. The combined-walk case is
    // js-only because JS's dynamic typing lets a single recursive walk
    // produce a heterogeneously-typed object tree — a shape Rust's
    // static type system rejects by construction. Enumerated here for
    // parity accounting; 2 upstream cases (1 in chat.reviver(), 1 in
    // standalone reviver()).

    // ---------- describe("standalone reviver()") additional js-only case ----------
    // Upstream "should be usable directly as JSON.parse second argument"
    // (serialization.test.ts:887) is js-only-documented: it asserts
    // that the upstream `reviver(_key, value)` callback can be passed
    // directly as the second argument to `JSON.parse`. Rust's
    // `serde_json::from_str` has no reviver-callback signature, so
    // the Rust port exposes the same behavior through
    // [`revive_str`] (and is exercised by
    // `revive_str_parses_and_promotes_chat_message_envelope` above).
    // No additional Rust test is written here because that case
    // already locks in the same observable contract.

    // ---------- describe("chat.reviver()") + describe("standalone
    //            reviver()") chat:Thread / chat:Channel branches (slice 443) ----------
    //
    // 1:1 with upstream `serialization.test.ts > describe("chat.reviver()")`
    // chat:Thread case + the equivalent in `describe("standalone
    // reviver()")`. The upstream test registers a Chat singleton with a
    // mock adapter so the reviver can resolve `adapterName` to a real
    // adapter. The Rust port mirrors this via the chat_singleton module.

    use crate::chat_singleton::{ChatSingleton, clear_chat_singleton, set_chat_singleton};
    use crate::types::{Adapter, StateAdapter};
    use std::sync::Arc;

    #[derive(Debug)]
    struct ReviverTestAdapter;
    #[async_trait::async_trait]
    impl Adapter for ReviverTestAdapter {
        fn name(&self) -> &str {
            "slack"
        }
    }

    #[derive(Debug)]
    struct ReviverTestState;
    #[async_trait::async_trait]
    impl StateAdapter for ReviverTestState {
        async fn get(&self, _key: &str) -> crate::types::StateResult<Option<serde_json::Value>> {
            Ok(None)
        }
        async fn set(
            &self,
            _key: &str,
            _value: serde_json::Value,
            _ttl_ms: Option<u64>,
        ) -> crate::types::StateResult<()> {
            Ok(())
        }
        async fn delete(&self, _key: &str) -> crate::types::StateResult<()> {
            Ok(())
        }
        async fn append_to_list(
            &self,
            _key: &str,
            _value: serde_json::Value,
            _max_length: Option<usize>,
            _ttl_ms: Option<u64>,
        ) -> crate::types::StateResult<()> {
            Ok(())
        }
        async fn get_list(
            &self,
            _key: &str,
            _limit: Option<usize>,
        ) -> crate::types::StateResult<Vec<serde_json::Value>> {
            Ok(Vec::new())
        }
        async fn set_if_not_exists(
            &self,
            _key: &str,
            _value: serde_json::Value,
            _ttl_ms: Option<u64>,
        ) -> crate::types::StateResult<bool> {
            Ok(true)
        }
    }

    #[derive(Debug)]
    struct ReviverTestSingleton {
        adapter: Arc<dyn Adapter>,
        state: Arc<dyn StateAdapter>,
    }

    impl ChatSingleton for ReviverTestSingleton {
        fn get_adapter(&self, name: &str) -> Option<Arc<dyn Adapter>> {
            if name == self.adapter.name() {
                Some(self.adapter.clone())
            } else {
                None
            }
        }
        fn get_state(&self) -> Arc<dyn StateAdapter> {
            self.state.clone()
        }
    }

    /// Helper: install a singleton with a single named adapter and
    /// return a guard that clears the singleton on drop. Tests
    /// share global state via the singleton slot, so the guard
    /// ensures cleanup even on panic.
    struct SingletonGuard;
    impl Drop for SingletonGuard {
        fn drop(&mut self) {
            clear_chat_singleton();
        }
    }

    fn install_test_singleton() -> SingletonGuard {
        let singleton: Arc<dyn ChatSingleton> = Arc::new(ReviverTestSingleton {
            adapter: Arc::new(ReviverTestAdapter),
            state: Arc::new(ReviverTestState),
        });
        set_chat_singleton(singleton);
        SingletonGuard
    }

    #[test]
    fn revive_value_promotes_chat_thread_envelopes_when_singleton_resolves_adapter() {
        // 1:1 with upstream "should revive chat:Thread objects" — when
        // a singleton is registered with a matching adapter, the
        // reviver dispatches the chat:Thread branch and constructs a
        // typed Thread handle.
        let _g = install_test_singleton();
        let value = json!({
            "_type": "chat:Thread",
            "id": "slack:C123:1234.5678",
            "channelId": "slack:C123",
            "channelVisibility": "unknown",
            "isDM": false,
            "adapterName": "slack",
        });
        match revive_value(value) {
            Revived::Thread(t) => {
                assert_eq!(t.thread_id(), "slack:C123:1234.5678");
            }
            other => panic!("expected Revived::Thread, got {other:?}"),
        }
    }

    #[test]
    fn revive_value_promotes_chat_channel_envelopes_when_singleton_resolves_adapter() {
        // 1:1 with upstream "should revive chat:Channel objects" — same
        // contract as chat:Thread for the Channel reviver branch.
        let _g = install_test_singleton();
        let value = json!({
            "_type": "chat:Channel",
            "id": "slack:C123",
            "adapterName": "slack",
            "channelVisibility": "unknown",
            "isDM": false,
        });
        match revive_value(value) {
            Revived::Channel(c) => {
                assert_eq!(c.channel_id(), "slack:C123");
            }
            other => panic!("expected Revived::Channel, got {other:?}"),
        }
    }

    #[test]
    fn standalone_reviver_should_allow_re_serialization_of_a_revived_thread_without_singleton() {
        // 1:1 with upstream "should allow re-serialization of a
        // revived Thread without singleton" (serialization.test.ts:917).
        // With no singleton registered, the chat:Thread envelope
        // passes through verbatim; re-serializing the PassThrough
        // value via `serde_json` reproduces the original wire shape
        // (1:1 with upstream's `JSON.stringify` round-trip observable).
        // Upstream constructs `ThreadImpl.fromJSON` in the
        // no-singleton path; Rust's `Thread::from_json` requires an
        // explicit adapter argument by construction, so the upstream
        // "construct without adapter" shape is unrepresentable — the
        // observable wire-shape preservation lives on the
        // PassThrough branch instead.
        clear_chat_singleton();
        let json = json!({
            "_type": "chat:Thread",
            "id": "slack:C123:1234.5678",
            "channelId": "C123",
            "isDM": false,
            "adapterName": "slack",
        });
        match revive_value(json.clone()) {
            Revived::PassThrough(v) => {
                let reserialized = serde_json::to_value(&v).unwrap();
                assert_eq!(
                    reserialized.get("_type").and_then(Value::as_str),
                    Some("chat:Thread")
                );
                assert_eq!(
                    reserialized.get("adapterName").and_then(Value::as_str),
                    Some("slack")
                );
                assert_eq!(
                    reserialized.get("id").and_then(Value::as_str),
                    Some("slack:C123:1234.5678")
                );
            }
            other => panic!("expected PassThrough (no singleton), got {other:?}"),
        }
    }

    #[test]
    fn standalone_reviver_should_allow_re_serialization_of_a_revived_channel_without_singleton() {
        // 1:1 with upstream "should allow re-serialization of a
        // revived Channel without singleton" (serialization.test.ts:936).
        // Same shape as the Thread re-serialization case above.
        clear_chat_singleton();
        let json = json!({
            "_type": "chat:Channel",
            "id": "C123",
            "isDM": false,
            "adapterName": "slack",
        });
        match revive_value(json.clone()) {
            Revived::PassThrough(v) => {
                let reserialized = serde_json::to_value(&v).unwrap();
                assert_eq!(
                    reserialized.get("_type").and_then(Value::as_str),
                    Some("chat:Channel")
                );
                assert_eq!(
                    reserialized.get("adapterName").and_then(Value::as_str),
                    Some("slack")
                );
                assert_eq!(reserialized.get("id").and_then(Value::as_str), Some("C123"));
            }
            other => panic!("expected PassThrough (no singleton), got {other:?}"),
        }
    }

    #[test]
    fn revive_value_falls_through_when_no_singleton_or_no_matching_adapter() {
        // 1:1 (subset) with upstream's "no singleton registered" /
        // "adapter not found" lazy-resolution-failure behavior: the
        // Rust reviver falls through to PassThrough so the caller
        // sees the raw value rather than constructing a Thread bound
        // to a nonexistent adapter. Tested without installing any
        // singleton (singleton slot is empty).
        clear_chat_singleton();
        let value = json!({
            "_type": "chat:Thread",
            "id": "slack:C123:1234.5678",
            "adapterName": "slack",
        });
        match revive_value(value.clone()) {
            Revived::PassThrough(v) => assert_eq!(v, value),
            other => panic!("expected PassThrough (no singleton), got {other:?}"),
        }

        // With a singleton installed but no matching adapter, also
        // falls through.
        let _g = install_test_singleton();
        let unknown_adapter = json!({
            "_type": "chat:Thread",
            "id": "telegram:123:456",
            "adapterName": "telegram",
        });
        match revive_value(unknown_adapter.clone()) {
            Revived::PassThrough(v) => assert_eq!(v, unknown_adapter),
            other => panic!("expected PassThrough (no adapter match), got {other:?}"),
        }
    }
}
