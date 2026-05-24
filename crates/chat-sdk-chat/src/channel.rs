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
use crate::types::{
    Adapter, AdapterError, AdapterResult, Author, ChannelInfo, EphemeralMessage,
    PostEphemeralOptions, StateAdapter, StateResult,
};

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
    /// 1:1 with upstream `name: string | null`. Lazily populated by
    /// [`Channel::fetch_metadata`] from the adapter's
    /// `fetch_channel_info` result. `Arc<Mutex>` to keep the handle
    /// `Clone`.
    name: Arc<std::sync::Mutex<Option<String>>>,
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
            name: Arc::new(std::sync::Mutex::new(None)),
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
            name: Arc::new(std::sync::Mutex::new(None)),
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
            name: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// 1:1 with upstream `readonly isDM: boolean`. Returns the
    /// `isDM` flag set at construction (defaults to `false`).
    pub fn is_dm(&self) -> bool {
        self.is_dm
    }

    /// 1:1 with upstream `get name(): string | null`. Returns the
    /// cached name populated by the last [`Self::fetch_metadata`]
    /// call. `None` before the first fetch (matches upstream's
    /// `null` initial value).
    pub fn name(&self) -> Option<String> {
        self.name.lock().unwrap().clone()
    }

    /// 1:1 port of upstream `Channel.toJSON()`. Returns the wire
    /// shape `{ _type: "chat:Channel", id, adapterName,
    /// channelVisibility, isDM }`. `channelVisibility` defaults to
    /// `"unknown"` (1:1 with upstream's default when no metadata
    /// has been fetched).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "_type": "chat:Channel",
            "id": self.channel_id,
            "adapterName": self.adapter.name(),
            "channelVisibility": "unknown",
            "isDM": self.is_dm,
        })
    }

    /// 1:1 port of upstream `static Channel.fromJSON(json, adapter)`.
    /// Reconstructs the handle from its serialized form. The
    /// `adapter` argument is supplied externally (1:1 with upstream's
    /// `adapter` parameter — the adapter isn't serialized).
    pub fn from_json(json: &serde_json::Value, adapter: Arc<dyn Adapter>) -> Self {
        let channel_id = json
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let is_dm = json
            .get("isDM")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        Self::with_options(adapter, channel_id, None, is_dm)
    }

    /// 1:1 port of upstream `async fetchMetadata(): Promise<ChannelInfo>`.
    /// Calls the adapter's `fetch_channel_info(channel_id)`; on
    /// `Unsupported` (1:1 with upstream's missing-method optional-
    /// chaining), synthesizes a basic `ChannelInfo` with just the
    /// `id`, `is_dm`, and empty metadata. Caches the resolved `name`
    /// on the handle so [`Self::name`] reflects it.
    pub async fn fetch_metadata(&self) -> AdapterResult<ChannelInfo> {
        let info = match self.adapter.fetch_channel_info(&self.channel_id).await {
            Ok(info) => info,
            Err(AdapterError::Unsupported(_)) => ChannelInfo {
                channel_visibility: None,
                id: self.channel_id.clone(),
                is_dm: Some(self.is_dm),
                member_count: None,
                metadata: serde_json::Map::new(),
                name: None,
            },
            Err(err) => return Err(err),
        };
        if let Some(name) = info.name.as_ref() {
            *self.name.lock().unwrap() = Some(name.clone());
        }
        Ok(info)
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
    /// `Channel.post(text)`. Prefers
    /// [`Adapter::post_channel_message`] when the adapter implements
    /// it (some platforms distinguish channel-level posts from
    /// thread replies); falls back to [`Adapter::post_message`]
    /// when `post_channel_message` returns `Unsupported`. Returns
    /// the platform-assigned message id.
    pub async fn post(&self, text: &str) -> AdapterResult<String> {
        match self
            .adapter
            .post_channel_message(&self.channel_id, text)
            .await
        {
            Ok(id) => Ok(id),
            Err(AdapterError::Unsupported(_)) => {
                self.adapter.post_message(&self.channel_id, text).await
            }
            Err(err) => Err(err),
        }
    }

    /// 1:1 port of upstream `Channel.startTyping(status?)`. Delegates
    /// to [`Adapter::start_typing`] with the bound channel id.
    pub async fn start_typing(&self, status: Option<&str>) -> AdapterResult<()> {
        self.adapter.start_typing(&self.channel_id, status).await
    }

    /// 1:1 port of upstream `Channel.mentionUser(userId)`. Returns
    /// the Slack-style mention syntax `<@userId>` (upstream hard-codes
    /// the angle-bracket wrapper independent of platform; per-adapter
    /// renderers translate to the platform-native form downstream).
    pub fn mention_user(&self, user_id: &str) -> String {
        format!("<@{user_id}>")
    }

    /// 1:1 port of upstream `Channel.postEphemeral(user, message, options)`.
    /// Mirrors [`crate::thread::Thread::post_ephemeral`] semantics:
    /// tries native ephemeral via [`Adapter::post_ephemeral`]; on
    /// `Unsupported` falls back to DM (open_dm + post_message) when
    /// `options.fallback_to_dm` is `true`, otherwise returns
    /// `Ok(None)`. Returns `Ok(None)` when neither native ephemeral
    /// nor DM are available.
    pub async fn post_ephemeral(
        &self,
        user_id: &str,
        text: &str,
        options: PostEphemeralOptions,
    ) -> AdapterResult<Option<EphemeralMessage>> {
        match self.adapter.post_ephemeral(&self.channel_id, user_id, text).await {
            Ok(msg) => return Ok(Some(msg)),
            Err(AdapterError::Unsupported(_)) => {}
            Err(other) => return Err(other),
        }
        if !options.fallback_to_dm {
            return Ok(None);
        }
        let dm_thread_id = match self.adapter.open_dm(user_id).await {
            Ok(id) => id,
            Err(AdapterError::Unsupported(_)) => return Ok(None),
            Err(other) => return Err(other),
        };
        let id = self.adapter.post_message(&dm_thread_id, text).await?;
        Ok(Some(EphemeralMessage {
            id,
            thread_id: dm_thread_id,
            used_fallback: true,
            raw: serde_json::Value::Object(serde_json::Map::new()),
        }))
    }

    /// 1:1 port of upstream `Channel.postEphemeral(author, message, options)`
    /// overload — extracts `user_id` from the [`Author`] and delegates
    /// to [`Self::post_ephemeral`]. Mirrors upstream's runtime
    /// `typeof user === "string" ? user : user.userId` branch.
    pub async fn post_ephemeral_for_author(
        &self,
        author: &Author,
        text: &str,
        options: PostEphemeralOptions,
    ) -> AdapterResult<Option<EphemeralMessage>> {
        self.post_ephemeral(&author.user_id, text, options).await
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
        post_channel_message: Mutex<Vec<(String, String)>>,
        post_channel_message_unsupported: bool,
        start_typing_calls: Mutex<Vec<(String, Option<String>)>>,
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
        async fn post_channel_message(
            &self,
            channel_id: &str,
            text: &str,
        ) -> AdapterResult<String> {
            if self.post_channel_message_unsupported {
                return Err(AdapterError::Unsupported("post_channel_message"));
            }
            self.post_channel_message
                .lock()
                .unwrap()
                .push((channel_id.to_string(), text.to_string()));
            Ok("channel-msg-id".to_string())
        }
        async fn start_typing(
            &self,
            thread_id: &str,
            status: Option<&str>,
        ) -> AdapterResult<()> {
            self.start_typing_calls
                .lock()
                .unwrap()
                .push((thread_id.to_string(), status.map(str::to_string)));
            Ok(())
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
    fn channel_post_delegates_to_adapter_post_message_when_post_channel_message_unsupported() {
        // RecordingAdapter without post_channel_message support
        // exercises the fall-through path. Adapters that don't
        // override `post_channel_message` get the same fall-through
        // via the trait default's `Err(Unsupported)`.
        let adapter = Arc::new(RecordingAdapter {
            post_channel_message_unsupported: true,
            ..Default::default()
        });
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
        // Both clones invoke the same Arc-shared adapter — observe
        // via the post_channel_message recorder since `Channel::post`
        // routes there preferentially.
        block_on(channel.post("a")).unwrap();
        block_on(cloned.post("b")).unwrap();
        let calls = adapter.post_channel_message.lock().unwrap();
        assert_eq!(calls.len(), 2);
    }

    // ---------- basic properties (2 upstream cases) ----------

    #[test]
    fn channel_basic_properties_should_have_correct_id_and_adapter() {
        // 1:1 with upstream "should have correct id and adapter" —
        // asserts id, adapter, isDM=false, name=null on a freshly
        // constructed channel.
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter, "slack:C123");
        assert_eq!(channel.channel_id(), "slack:C123");
        assert_eq!(channel.adapter_name(), "recording");
        // Default: isDM is false (upstream `isDM ?? false`).
        assert!(!channel.is_dm());
        // Default: name is None until fetch_metadata populates it
        // (upstream `channel.name` returns null initially).
        assert!(channel.name().is_none());
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

    // ---------- describe("serialization") (2 upstream cases) ----------
    // 1:1 with upstream `channel.test.ts > describe("serialization")`.

    #[test]
    fn channel_serialization_should_serialize_to_json() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let channel = Channel::with_options(adapter, "slack:C123", None, false);
        let json = channel.to_json();
        assert_eq!(
            json,
            serde_json::json!({
                "_type": "chat:Channel",
                "id": "slack:C123",
                "adapterName": "recording",
                "channelVisibility": "unknown",
                "isDM": false,
            })
        );
    }

    #[test]
    fn channel_serialization_should_deserialize_from_json() {
        let json = serde_json::json!({
            "_type": "chat:Channel",
            "id": "slack:C123",
            "adapterName": "slack",
            "isDM": false,
        });
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let channel = Channel::from_json(&json, adapter.clone());
        assert_eq!(channel.channel_id(), "slack:C123");
        assert!(!channel.is_dm());
        // adapter is the same Arc supplied externally.
        assert!(Arc::ptr_eq(channel.adapter(), &adapter));
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

    // ---------- describe("post") (2 of 3 upstream cases; streaming deferred) ----------
    // 1:1 with upstream `channel.test.ts > describe("post")`. The
    // 3rd case ("should handle streaming by accumulating text")
    // requires async-stream + StreamingMarkdownRenderer integration
    // and is deferred to a follow-up slice.

    #[test]
    fn channel_post_should_use_post_channel_message_when_available() {
        // 1:1 with upstream "should use postChannelMessage when
        // available". The RecordingAdapter implements
        // post_channel_message so Channel::post routes there.
        let adapter = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123");
        let id = block_on(channel.post("Hello channel!")).unwrap();
        assert_eq!(id, "channel-msg-id");
        let calls = adapter.post_channel_message.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "slack:C123");
        assert_eq!(calls[0].1, "Hello channel!");
        // Verify post_message was NOT called.
        assert!(adapter.post_message.lock().unwrap().is_empty());
    }

    #[test]
    fn channel_post_should_fall_back_to_post_message_when_post_channel_message_is_not_available() {
        // 1:1 with upstream "should fall back to postMessage when
        // postChannelMessage is not available".
        let adapter = Arc::new(RecordingAdapter {
            post_channel_message_unsupported: true,
            ..Default::default()
        });
        let channel = Channel::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123");
        let id = block_on(channel.post("Hello!")).unwrap();
        assert_eq!(id, "msg-id");
        let calls = adapter.post_message.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "slack:C123");
        assert_eq!(calls[0].1, "Hello!");
        // Verify post_channel_message recorder is empty (the call
        // returned Unsupported and was not recorded).
        assert!(adapter.post_channel_message.lock().unwrap().is_empty());
    }

    // ---------- describe("fetchMetadata") (2 upstream cases) ----------
    // 1:1 with upstream `channel.test.ts > describe("fetchMetadata")`.

    /// Adapter that returns a fixed `ChannelInfo` from
    /// `fetch_channel_info` (1:1 with upstream's
    /// `createMockAdapter().fetchChannelInfo` default mock that
    /// returns `{ id, name: "#" + id }`).
    #[derive(Debug)]
    struct FetchInfoAdapter;
    #[async_trait::async_trait]
    impl Adapter for FetchInfoAdapter {
        fn name(&self) -> &str {
            "slack"
        }
        async fn fetch_channel_info(&self, channel_id: &str) -> AdapterResult<ChannelInfo> {
            Ok(ChannelInfo {
                channel_visibility: None,
                id: channel_id.to_string(),
                is_dm: Some(false),
                member_count: None,
                metadata: serde_json::Map::new(),
                name: Some(format!("#{channel_id}")),
            })
        }
    }

    #[test]
    fn channel_fetch_metadata_should_fetch_channel_info_and_set_name() {
        let adapter: Arc<dyn Adapter> = Arc::new(FetchInfoAdapter);
        let channel = Channel::new(adapter, "slack:C123");
        // Before fetch: name is None.
        assert!(channel.name().is_none());
        let info = block_on(channel.fetch_metadata()).unwrap();
        assert_eq!(info.id, "slack:C123");
        assert_eq!(info.name.as_deref(), Some("#slack:C123"));
        // After fetch: cached name reflects the fetched value.
        assert_eq!(channel.name().as_deref(), Some("#slack:C123"));
    }

    #[test]
    fn channel_fetch_metadata_returns_basic_info_when_adapter_has_no_fetch_channel_info() {
        // 1:1 with upstream "should return basic info when adapter
        // has no fetchChannelInfo". RecordingAdapter doesn't override
        // `fetch_channel_info` so the default `Err(Unsupported)`
        // triggers the synthesized fallback.
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter, "slack:C123");
        let info = block_on(channel.fetch_metadata()).unwrap();
        assert_eq!(info.id, "slack:C123");
        assert_eq!(info.is_dm, Some(false));
        assert!(info.metadata.is_empty());
        assert!(info.name.is_none());
        // name accessor still None since no fetched name.
        assert!(channel.name().is_none());
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

    // ---------- describe("ChannelImpl.postEphemeral") (5 upstream cases) ----------
    // 1:1 with upstream `channel.test.ts > describe("ChannelImpl.postEphemeral")`.
    // The Rust port uses a dedicated `EphemeralAdapter` test mock with
    // boolean flags (`supports_ephemeral`, `supports_open_dm`) to
    // reproduce upstream's `mockAdapter.postEphemeral = undefined` /
    // `mockAdapter.openDM = undefined` mutation pattern. Mirrors the
    // identical test mock shape used in the Thread postEphemeral
    // describe block.

    #[derive(Debug, Default)]
    struct EphemeralAdapter {
        supports_ephemeral: bool,
        supports_open_dm: bool,
        post_ephemeral_calls: Mutex<Vec<(String, String, String)>>,
        open_dm_calls: Mutex<Vec<String>>,
        post_message_calls: Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl Adapter for EphemeralAdapter {
        fn name(&self) -> &str {
            "slack"
        }
        async fn post_ephemeral(
            &self,
            thread_id: &str,
            user_id: &str,
            text: &str,
        ) -> AdapterResult<EphemeralMessage> {
            if !self.supports_ephemeral {
                return Err(AdapterError::Unsupported("post_ephemeral"));
            }
            self.post_ephemeral_calls.lock().unwrap().push((
                thread_id.to_string(),
                user_id.to_string(),
                text.to_string(),
            ));
            Ok(EphemeralMessage {
                id: "eph-1".to_string(),
                thread_id: thread_id.to_string(),
                used_fallback: false,
                raw: serde_json::Value::Object(serde_json::Map::new()),
            })
        }
        async fn open_dm(&self, user_id: &str) -> AdapterResult<String> {
            if !self.supports_open_dm {
                return Err(AdapterError::Unsupported("open_dm"));
            }
            self.open_dm_calls.lock().unwrap().push(user_id.to_string());
            Ok(format!("slack:D{user_id}:"))
        }
        async fn post_message(&self, thread_id: &str, text: &str) -> AdapterResult<String> {
            self.post_message_calls
                .lock()
                .unwrap()
                .push((thread_id.to_string(), text.to_string()));
            // Upstream default mock returns id "msg-1" for postMessage.
            Ok("msg-1".to_string())
        }
    }

    fn ephemeral_channel(adapter: Arc<EphemeralAdapter>) -> Channel {
        Channel::new(adapter as Arc<dyn Adapter>, "slack:C123")
    }

    #[test]
    fn channel_post_ephemeral_should_use_adapter_post_ephemeral_when_available() {
        let adapter = Arc::new(EphemeralAdapter {
            supports_ephemeral: true,
            supports_open_dm: true,
            ..Default::default()
        });
        let channel = ephemeral_channel(adapter.clone());
        let result = block_on(channel.post_ephemeral(
            "U456",
            "Secret!",
            PostEphemeralOptions { fallback_to_dm: true },
        ))
        .unwrap()
        .expect("Expected Some(EphemeralMessage)");
        let calls = adapter.post_ephemeral_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            (
                "slack:C123".to_string(),
                "U456".to_string(),
                "Secret!".to_string()
            )
        );
        assert_eq!(result.id, "eph-1");
        assert_eq!(result.thread_id, "slack:C123");
        assert!(!result.used_fallback);
    }

    #[test]
    fn channel_post_ephemeral_should_extract_user_id_from_author_object() {
        let adapter = Arc::new(EphemeralAdapter {
            supports_ephemeral: true,
            supports_open_dm: true,
            ..Default::default()
        });
        let channel = ephemeral_channel(adapter.clone());
        let author = Author {
            user_id: "U789".to_string(),
            user_name: "testuser".to_string(),
            full_name: "Test User".to_string(),
            is_bot: crate::types::BotStatus::Known(false),
            is_me: false,
        };
        block_on(channel.post_ephemeral_for_author(
            &author,
            "Hello!",
            PostEphemeralOptions { fallback_to_dm: false },
        ))
        .unwrap()
        .expect("Expected Some(EphemeralMessage)");
        let calls = adapter.post_ephemeral_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            (
                "slack:C123".to_string(),
                "U789".to_string(),
                "Hello!".to_string()
            )
        );
    }

    #[test]
    fn channel_post_ephemeral_should_return_null_when_adapter_has_no_post_ephemeral_and_fallback_to_dm_is_false() {
        let adapter = Arc::new(EphemeralAdapter {
            supports_ephemeral: false,
            supports_open_dm: true,
            ..Default::default()
        });
        let channel = ephemeral_channel(adapter.clone());
        let result = block_on(channel.post_ephemeral(
            "U456",
            "Secret!",
            PostEphemeralOptions { fallback_to_dm: false },
        ))
        .unwrap();
        assert!(result.is_none());
        assert!(adapter.open_dm_calls.lock().unwrap().is_empty());
        assert!(adapter.post_message_calls.lock().unwrap().is_empty());
    }

    #[test]
    fn channel_post_ephemeral_should_fallback_to_dm_when_adapter_has_no_post_ephemeral_and_fallback_to_dm_is_true() {
        let adapter = Arc::new(EphemeralAdapter {
            supports_ephemeral: false,
            supports_open_dm: true,
            ..Default::default()
        });
        let channel = ephemeral_channel(adapter.clone());
        let result = block_on(channel.post_ephemeral(
            "U456",
            "Secret!",
            PostEphemeralOptions { fallback_to_dm: true },
        ))
        .unwrap()
        .expect("Expected Some(EphemeralMessage) via DM fallback");
        let dm_calls = adapter.open_dm_calls.lock().unwrap();
        assert_eq!(dm_calls.as_slice(), &["U456".to_string()]);
        let post_calls = adapter.post_message_calls.lock().unwrap();
        assert_eq!(post_calls.len(), 1);
        assert_eq!(
            post_calls[0],
            ("slack:DU456:".to_string(), "Secret!".to_string())
        );
        assert_eq!(result.id, "msg-1");
        assert_eq!(result.thread_id, "slack:DU456:");
        assert!(result.used_fallback);
    }

    // ---------- describe("ChannelImpl.startTyping") (2 upstream cases) ----------
    // 1:1 with upstream `channel.test.ts > describe("ChannelImpl.startTyping")`.

    #[test]
    fn channel_start_typing_calls_adapter_start_typing_with_channel_id() {
        let adapter = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123");
        block_on(channel.start_typing(None)).unwrap();
        let calls = adapter.start_typing_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], ("slack:C123".to_string(), None));
    }

    #[test]
    fn channel_start_typing_passes_status_to_adapter_start_typing() {
        let adapter = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123");
        block_on(channel.start_typing(Some("thinking..."))).unwrap();
        let calls = adapter.start_typing_calls.lock().unwrap();
        assert_eq!(
            calls[0],
            ("slack:C123".to_string(), Some("thinking...".to_string()))
        );
    }

    // ---------- describe("ChannelImpl.mentionUser") (2 upstream cases) ----------
    // 1:1 with upstream `channel.test.ts > describe("ChannelImpl.mentionUser")`.

    #[test]
    fn channel_mention_user_returns_formatted_mention_string() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter, "slack:C123");
        assert_eq!(channel.mention_user("U456"), "<@U456>");
    }

    #[test]
    fn channel_mention_user_handles_different_user_id_formats() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let channel = Channel::new(adapter, "slack:C123");
        assert_eq!(channel.mention_user("UABC123DEF"), "<@UABC123DEF>");
        assert_eq!(channel.mention_user("bot-user"), "<@bot-user>");
    }

    #[test]
    fn channel_post_ephemeral_should_return_null_when_no_post_ephemeral_no_open_dm_and_fallback_to_dm_is_true() {
        let adapter = Arc::new(EphemeralAdapter {
            supports_ephemeral: false,
            supports_open_dm: false,
            ..Default::default()
        });
        let channel = ephemeral_channel(adapter.clone());
        let result = block_on(channel.post_ephemeral(
            "U456",
            "Secret!",
            PostEphemeralOptions { fallback_to_dm: true },
        ))
        .unwrap();
        assert!(result.is_none());
        assert!(adapter.post_message_calls.lock().unwrap().is_empty());
    }
}
