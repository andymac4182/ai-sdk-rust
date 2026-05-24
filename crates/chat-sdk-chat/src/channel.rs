//! `Channel` — the cross-platform channel/conversation handle.
//!
//! 1:1 port (in progress) of `packages/chat/src/channel.ts`.
//!
//! Upstream `class Channel` is the per-channel API surface exposed to
//! adapters: `post`, `postObject`, `listThreads`, etc. The Rust port
//! lands in stages:
//!
//! **What this slice ships (slice 126):**
//!
//! - [`Channel`] struct holding an `Arc<dyn Adapter>` + the
//!   channel-id-encoded thread root. 1:1 with upstream's
//!   `class Channel { constructor({ adapter, channelId }) }`.
//! - [`Channel::channel_id`] / [`Channel::adapter_name`] accessors.
//! - [`Channel::post`] — convenience wrapper around
//!   `adapter.post_message(channel_id, text)`.
//! - [`Channel::post_object`] — convenience wrapper around
//!   `post_postable_object(adapter, channel_id, envelope)`.
//!
//! **What is deferred:**
//!
//! - `listThreads`, `fetchInfo`, `isDm`, `getVisibility`,
//!   `openModal`, `channelIdFromThreadId`, `fetchChannelMessages` —
//!   each maps to a not-yet-extended `Adapter` trait method. They
//!   land as their consumer call sites get ported.

use std::sync::Arc;

use crate::postable_object::{PostableDispatchError, post_postable_object};
use crate::types::{Adapter, AdapterResult, StateAdapter, StateResult};

/// 1:1 with upstream's private
/// `const CHANNEL_STATE_KEY_PREFIX = "channel-state:"`.
pub const CHANNEL_STATE_KEY_PREFIX: &str = "channel-state:";

/// 1:1 with upstream `export const CHANNEL_STATE_TTL_MS = 30 * 24
/// * 60 * 60 * 1000` (30 days in milliseconds). Channel state and
/// thread state share the same TTL upstream.
pub const CHANNEL_STATE_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// 1:1 port of upstream `deriveChannelId(adapter, threadId): string`.
/// Returns `adapter.channel_id_from_thread_id(thread_id)` when the
/// adapter has overridden the trait method, falling back to
/// `thread_id` otherwise (1:1 with upstream's
/// `adapter.channelIdFromThreadId?.(threadId) ?? threadId`).
pub fn derive_channel_id(adapter: &dyn Adapter, thread_id: &str) -> String {
    adapter
        .channel_id_from_thread_id(thread_id)
        .unwrap_or_else(|| thread_id.to_string())
}

/// Cross-platform channel handle. 1:1 port (in progress) of upstream
/// `class Channel`.
///
/// Holds an `Arc<dyn Adapter>` (the platform adapter that owns this
/// channel) and the platform-encoded `channel_id`. Adapters mint
/// `Channel` instances when handlers need to interact with a channel
/// outside the context of a specific thread/message.
#[derive(Clone)]
pub struct Channel {
    adapter: Arc<dyn Adapter>,
    channel_id: String,
    is_dm: bool,
    state_adapter: Option<Arc<dyn StateAdapter>>,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel")
            .field("adapter", &self.adapter)
            .field("channel_id", &self.channel_id)
            .finish()
    }
}

impl Channel {
    /// 1:1 port of upstream `new Channel({ adapter, channelId })`.
    /// `is_dm` defaults to `false`, matching upstream's `isDM ??
    /// false` fallback. No state adapter bound — call
    /// [`Channel::with_state_adapter`] or use
    /// [`Channel::with_options`] to configure both.
    pub fn new(adapter: Arc<dyn Adapter>, channel_id: impl Into<String>) -> Self {
        Self {
            adapter,
            channel_id: channel_id.into(),
            is_dm: false,
            state_adapter: None,
        }
    }

    /// 1:1 port of upstream `new ChannelImpl({ adapter, id,
    /// stateAdapter })` with `isDM` defaulted to `false`. Use this
    /// constructor when callers want [`Channel::state`] /
    /// [`Channel::set_state`] without the (not yet ported) chat-
    /// singleton fallback.
    pub fn with_state_adapter(
        adapter: Arc<dyn Adapter>,
        channel_id: impl Into<String>,
        state_adapter: Arc<dyn StateAdapter>,
    ) -> Self {
        Self {
            adapter,
            channel_id: channel_id.into(),
            is_dm: false,
            state_adapter: Some(state_adapter),
        }
    }

    /// 1:1 port of upstream `new ChannelImpl({ adapter, id,
    /// stateAdapter, isDM })`. The most general constructor.
    pub fn with_options(
        adapter: Arc<dyn Adapter>,
        channel_id: impl Into<String>,
        state_adapter: Option<Arc<dyn StateAdapter>>,
        is_dm: bool,
    ) -> Self {
        Self {
            adapter,
            channel_id: channel_id.into(),
            is_dm,
            state_adapter,
        }
    }

    /// 1:1 with upstream `readonly isDM: boolean`. Returns the
    /// `isDM` flag set at construction (defaults to `false`).
    pub fn is_dm(&self) -> bool {
        self.is_dm
    }

    /// Borrow the bound state adapter, if any. Returns `None` when
    /// [`Channel::new`] was used.
    pub fn state_adapter(&self) -> Option<&Arc<dyn StateAdapter>> {
        self.state_adapter.as_ref()
    }

    /// Channel-id accessor. 1:1 with upstream `get channelId(): string`.
    pub fn channel_id(&self) -> &str {
        &self.channel_id
    }

    /// Name of the underlying adapter (its platform tag —
    /// `"slack"`/`"teams"`/…). Convenience wrapper around
    /// `self.adapter.name()`.
    pub fn adapter_name(&self) -> &str {
        self.adapter.name()
    }

    /// Borrow the underlying adapter. Mostly useful when callers need
    /// to invoke a method this `Channel` struct hasn't wrapped yet.
    pub fn adapter(&self) -> &Arc<dyn Adapter> {
        &self.adapter
    }

    /// Post a plain-text message to this channel. 1:1 with upstream
    /// `Channel.post(text)`. Returns the platform-assigned message id.
    pub async fn post(&self, text: &str) -> AdapterResult<String> {
        self.adapter.post_message(&self.channel_id, text).await
    }

    /// Post a postable envelope (cards, modals, plans, polls) to this
    /// channel. 1:1 with upstream `Channel.postObject(value)`. Routes
    /// through [`post_postable_object`] so adapters that lack a typed
    /// `post_object` automatically fall back to `post_message` with the
    /// envelope's fallback text.
    pub async fn post_object(
        &self,
        envelope: &serde_json::Value,
    ) -> Result<String, PostableDispatchError> {
        post_postable_object(self.adapter.as_ref(), &self.channel_id, envelope).await
    }

    fn state_key(&self) -> String {
        format!("{CHANNEL_STATE_KEY_PREFIX}{}", self.channel_id)
    }

    /// 1:1 port of upstream `Channel.state` getter. Reads stored
    /// state via the bound [`StateAdapter`]. Returns `Ok(None)`
    /// when the key is unset, expired, or when the Channel was
    /// built without a state adapter (matches upstream's `null`
    /// resolution).
    pub async fn state(&self) -> StateResult<Option<serde_json::Value>> {
        let Some(state) = self.state_adapter.as_ref() else {
            return Ok(None);
        };
        state.get(&self.state_key()).await
    }

    /// 1:1 port of upstream `Channel.setState(newState, options?)`.
    /// Shallow-merges `new_state` with the existing value under the
    /// channel-state key. State persists for
    /// [`CHANNEL_STATE_TTL_MS`] ms. No-op when the Channel has no
    /// state adapter bound.
    pub async fn set_state(&self, new_state: serde_json::Value) -> StateResult<()> {
        self.set_state_with_options(new_state, false).await
    }

    /// 1:1 port of upstream `Channel.setState(newState, { replace:
    /// true })`. Overwrites any existing state under the key.
    pub async fn set_state_replace(&self, new_state: serde_json::Value) -> StateResult<()> {
        self.set_state_with_options(new_state, true).await
    }

    async fn set_state_with_options(
        &self,
        new_state: serde_json::Value,
        replace: bool,
    ) -> StateResult<()> {
        let Some(state) = self.state_adapter.as_ref() else {
            return Ok(());
        };
        let key = self.state_key();
        if replace {
            return state.set(&key, new_state, Some(CHANNEL_STATE_TTL_MS)).await;
        }
        let merged = match state.get(&key).await? {
            Some(serde_json::Value::Object(mut existing)) => {
                if let serde_json::Value::Object(incoming) = new_state {
                    for (k, v) in incoming {
                        existing.insert(k, v);
                    }
                }
                serde_json::Value::Object(existing)
            }
            _ => new_state,
        };
        state.set(&key, merged, Some(CHANNEL_STATE_TTL_MS)).await
    }
}

#[cfg(test)]
mod tests {
    //! Additive coverage for the [`Channel`] surface. Upstream's
    //! `channel.test.ts` exercises every Adapter method via Channel —
    //! those will land as each method gets ported into the Adapter
    //! trait + Channel wrapper.
    use super::*;
    use crate::postable_object::{POSTABLE_OBJECT_DISCRIMINATOR, postable_envelope};
    use crate::types::{AdapterError, AdapterResult};
    use futures_executor::block_on;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct RecordingAdapter {
        post_message: Mutex<Vec<(String, String)>>,
        post_object: Mutex<Vec<(String, String, serde_json::Value)>>,
        post_object_unsupported: bool,
    }

    #[async_trait::async_trait]
    impl Adapter for RecordingAdapter {
        fn name(&self) -> &str {
            "recording"
        }
        async fn post_message(&self, thread_id: &str, text: &str) -> AdapterResult<String> {
            self.post_message
                .lock()
                .unwrap()
                .push((thread_id.to_string(), text.to_string()));
            Ok("msg-id".to_string())
        }
        async fn post_object(
            &self,
            thread_id: &str,
            kind: &str,
            data: serde_json::Value,
        ) -> AdapterResult<String> {
            if self.post_object_unsupported {
                return Err(AdapterError::Unsupported("post_object"));
            }
            self.post_object
                .lock()
                .unwrap()
                .push((thread_id.to_string(), kind.to_string(), data));
            Ok("obj-id".to_string())
        }
    }

    #[test]
    fn channel_new_holds_adapter_and_channel_id() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter, "C123");
        assert_eq!(channel.channel_id(), "C123");
        assert_eq!(channel.adapter_name(), "recording");
    }

    #[test]
    fn channel_post_delegates_to_adapter_post_message() {
        let adapter = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter.clone() as Arc<dyn Adapter>, "C123");
        let id = block_on(channel.post("hello")).unwrap();
        assert_eq!(id, "msg-id");
        let calls = adapter.post_message.lock().unwrap();
        assert_eq!(calls[0].0, "C123");
        assert_eq!(calls[0].1, "hello");
    }

    #[test]
    fn channel_post_object_dispatches_through_post_postable_object() {
        let adapter = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter.clone() as Arc<dyn Adapter>, "C123");
        let envelope = postable_envelope("plan", serde_json::json!({"title": "T"}), "Plan: T");
        let id = block_on(channel.post_object(&envelope)).unwrap();
        assert_eq!(id, "obj-id");
        let calls = adapter.post_object.lock().unwrap();
        assert_eq!(calls[0].0, "C123");
        assert_eq!(calls[0].1, "plan");
    }

    #[test]
    fn channel_post_object_falls_back_to_post_message_when_unsupported() {
        let adapter = Arc::new(RecordingAdapter {
            post_object_unsupported: true,
            ..Default::default()
        });
        let channel = Channel::new(adapter.clone() as Arc<dyn Adapter>, "C123");
        let envelope = postable_envelope("plan", serde_json::json!({}), "Plan-fallback");
        let id = block_on(channel.post_object(&envelope)).unwrap();
        assert_eq!(id, "msg-id");
        let calls = adapter.post_message.lock().unwrap();
        assert_eq!(calls[0].0, "C123");
        assert_eq!(calls[0].1, "Plan-fallback");
    }

    #[test]
    fn channel_post_object_returns_not_a_postable_envelope_for_plain_json() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter, "C1");
        match block_on(channel.post_object(&serde_json::json!({"kind": "plan"}))) {
            Err(PostableDispatchError::NotAPostableEnvelope) => {}
            other => panic!("expected NotAPostableEnvelope, got {other:?}"),
        }
    }

    #[test]
    fn channel_clone_shares_adapter_arc() {
        let adapter = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter.clone() as Arc<dyn Adapter>, "C123");
        let cloned = channel.clone();
        // Both clones invoke the same Arc-shared adapter.
        block_on(channel.post("a")).unwrap();
        block_on(cloned.post("b")).unwrap();
        let calls = adapter.post_message.lock().unwrap();
        assert_eq!(calls.len(), 2);
    }

    // ---------- basic properties (2 upstream cases) ----------

    #[test]
    fn channel_basic_properties_should_have_correct_id_and_adapter() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter, "C123");
        assert_eq!(channel.channel_id(), "C123");
        assert_eq!(channel.adapter_name(), "recording");
        // Default: isDM is false (upstream `isDM ?? false`).
        assert!(!channel.is_dm());
    }

    #[test]
    fn channel_basic_properties_should_set_is_dm_when_configured() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let channel = Channel::with_options(adapter, "U123", None, true);
        assert!(channel.is_dm());
    }

    // ---------- state management (5 upstream cases) ----------

    use crate::types::StateAdapter;
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;

    #[derive(Debug, Default)]
    struct MockState {
        cache: StdMutex<HashMap<String, serde_json::Value>>,
        set_calls: StdMutex<Vec<(String, serde_json::Value, Option<u64>)>>,
    }

    #[async_trait::async_trait]
    impl StateAdapter for MockState {
        async fn get(&self, key: &str) -> crate::types::StateResult<Option<serde_json::Value>> {
            Ok(self.cache.lock().unwrap().get(key).cloned())
        }
        async fn set(
            &self,
            key: &str,
            value: serde_json::Value,
            ttl_ms: Option<u64>,
        ) -> crate::types::StateResult<()> {
            self.set_calls
                .lock()
                .unwrap()
                .push((key.to_string(), value.clone(), ttl_ms));
            self.cache.lock().unwrap().insert(key.to_string(), value);
            Ok(())
        }
        async fn delete(&self, key: &str) -> crate::types::StateResult<()> {
            self.cache.lock().unwrap().remove(key);
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
    }

    fn channel_with_state() -> (Channel, Arc<MockState>) {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let state = Arc::new(MockState::default());
        let channel = Channel::with_state_adapter(
            adapter,
            "C123",
            state.clone() as Arc<dyn StateAdapter>,
        );
        (channel, state)
    }

    #[test]
    fn channel_state_should_return_none_when_no_state_has_been_set() {
        let (channel, _state) = channel_with_state();
        let value = block_on(channel.state()).unwrap();
        assert!(value.is_none());
    }

    #[test]
    fn channel_state_should_set_and_retrieve_state() {
        let (channel, _state) = channel_with_state();
        block_on(channel.set_state(serde_json::json!({ "topic": "hello" }))).unwrap();
        let value = block_on(channel.state()).unwrap();
        assert_eq!(value, Some(serde_json::json!({ "topic": "hello" })));
    }

    #[test]
    fn channel_state_should_merge_state_by_default() {
        let (channel, _state) = channel_with_state();
        block_on(channel.set_state(serde_json::json!({ "topic": "x" }))).unwrap();
        block_on(channel.set_state(serde_json::json!({ "members": 5 }))).unwrap();
        let value = block_on(channel.state()).unwrap();
        assert_eq!(value, Some(serde_json::json!({ "topic": "x", "members": 5 })));
    }

    #[test]
    fn channel_state_should_replace_state_when_option_is_set() {
        let (channel, _state) = channel_with_state();
        block_on(channel.set_state(serde_json::json!({ "topic": "x", "members": 5 }))).unwrap();
        block_on(channel.set_state_replace(serde_json::json!({ "members": 10 }))).unwrap();
        let value = block_on(channel.state()).unwrap();
        assert_eq!(value, Some(serde_json::json!({ "members": 10 })));
    }

    #[test]
    fn channel_state_should_use_channel_state_key_prefix() {
        let (channel, state) = channel_with_state();
        block_on(channel.set_state(serde_json::json!({ "topic": "x" }))).unwrap();
        let last = state.set_calls.lock().unwrap().last().unwrap().clone();
        assert_eq!(last.0, "channel-state:C123");
        assert_eq!(last.2, Some(CHANNEL_STATE_TTL_MS));
    }

    // ---------- describe("deriveChannelId") (2 upstream cases) ----------
    // 1:1 with upstream `channel.test.ts > describe("deriveChannelId")`.

    /// Adapter whose `name` and `channel_id_from_thread_id` override
    /// match the upstream mock — strips any `:thread-suffix` from
    /// `<prefix>:<channel>:<rest>` and returns `<prefix>:<channel>`.
    #[derive(Debug)]
    struct PlatformAdapter {
        name: &'static str,
    }
    #[async_trait::async_trait]
    impl Adapter for PlatformAdapter {
        fn name(&self) -> &str {
            self.name
        }
        fn channel_id_from_thread_id(&self, thread_id: &str) -> Option<String> {
            // Collapse e.g. "slack:C123:1234.5678" -> "slack:C123"
            // (matches the upstream mock's behavior of returning
            // the prefix + first channel segment).
            let mut parts = thread_id.splitn(3, ':');
            let prefix = parts.next()?;
            let channel = parts.next()?;
            if prefix.is_empty() || channel.is_empty() {
                return None;
            }
            Some(format!("{prefix}:{channel}"))
        }
    }

    #[test]
    fn derive_channel_id_uses_adapter_channel_id_from_thread_id_when_available() {
        let adapter = PlatformAdapter { name: "slack" };
        let channel_id = derive_channel_id(&adapter, "slack:C123:1234.5678");
        assert_eq!(channel_id, "slack:C123");
    }

    #[test]
    fn derive_channel_id_works_with_different_adapters() {
        let adapter = PlatformAdapter { name: "gchat" };
        let channel_id = derive_channel_id(&adapter, "gchat:spaces/ABC123:dGhyZWFk");
        assert_eq!(channel_id, "gchat:spaces/ABC123");
    }

    #[test]
    fn derive_channel_id_falls_back_to_thread_id_when_adapter_returns_none() {
        // 1:1 with upstream `adapter.channelIdFromThreadId?.(threadId)
        // ?? threadId` — adapters without the override (e.g.
        // Messenger / WhatsApp where channel === thread) return the
        // thread_id verbatim.
        let adapter = RecordingAdapter::default();
        let channel_id = derive_channel_id(&adapter, "wa:PNID:E164");
        assert_eq!(channel_id, "wa:PNID:E164");
    }

    #[test]
    fn channel_post_object_serializes_envelope_through_round_trip() {
        let adapter = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter.clone() as Arc<dyn Adapter>, "C123");
        let envelope = postable_envelope("poll", serde_json::json!({"q": "?"}), "Poll");
        let text = serde_json::to_string(&envelope).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            parsed.get("$$typeof").and_then(|v| v.as_str()),
            Some(POSTABLE_OBJECT_DISCRIMINATOR)
        );
        block_on(channel.post_object(&parsed)).unwrap();
        let calls = adapter.post_object.lock().unwrap();
        assert_eq!(calls[0].1, "poll");
    }
}
