//! `Thread` — the cross-platform thread (one conversation in a channel).
//!
//! 1:1 port (in progress) of `packages/chat/src/thread.ts`.
//!
//! **What this slice ships (slice 127):**
//!
//! - [`Thread`] struct holding an `Arc<dyn Adapter>` + the
//!   platform-encoded `thread_id`. 1:1 with upstream
//!   `class Thread { constructor({ adapter, threadId }) }`.
//! - [`Thread::thread_id`] / [`Thread::adapter_name`] accessors.
//! - [`Thread::post`] — `adapter.post_message(thread_id, text)`.
//! - [`Thread::post_object`] — routed through
//!   [`crate::postable_object::post_postable_object`] so adapters
//!   without typed `post_object` fall back to `post_message` with
//!   the envelope's fallback text.
//! - [`Thread::subject`] — `MessageSubjectResolver`-cached fetch of
//!   the thread's subject via [`crate::types::Adapter::fetch_subject`].
//!
//! **What is deferred:**
//!
//! - `editMessage`, `deleteMessage`, `addReaction`, `removeReaction`,
//!   `startTyping`, `fetchMessages`, `fetchThread`, `fetchMessage`,
//!   `openDm`, `openModal` — each maps to a not-yet-extended `Adapter`
//!   trait method. They land as their consumer call sites get ported.

use std::sync::Arc;

use crate::postable_object::{PostableDispatchError, post_postable_object};
use crate::types::{
    Adapter, AdapterError, AdapterResult, StateAdapter, StateResult, THREAD_STATE_TTL_MS,
};

/// 1:1 with upstream's private
/// `const THREAD_STATE_KEY_PREFIX = "thread-state:"`.
pub const THREAD_STATE_KEY_PREFIX: &str = "thread-state:";

/// Cross-platform thread handle. 1:1 port (in progress) of upstream
/// `class Thread`.
#[derive(Clone)]
pub struct Thread {
    adapter: Arc<dyn Adapter>,
    thread_id: String,
    state_adapter: Option<Arc<dyn StateAdapter>>,
    /// 1:1 with upstream `isSubscribedContext?: boolean`. When
    /// `true`, [`Self::is_subscribed`] short-circuits to `true`
    /// without calling the state adapter — set by the chat event
    /// dispatcher when invoking `onSubscribedMessage` handlers.
    is_subscribed_context: bool,
}

impl std::fmt::Debug for Thread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Thread")
            .field("adapter", &self.adapter)
            .field("thread_id", &self.thread_id)
            .finish()
    }
}

impl Thread {
    /// 1:1 port of upstream `new Thread({ adapter, threadId })`.
    /// Created without a state adapter — `state()` / `set_state()`
    /// require [`Thread::with_state_adapter`] (or upstream's lazy
    /// singleton resolution when ported).
    pub fn new(adapter: Arc<dyn Adapter>, thread_id: impl Into<String>) -> Self {
        Self {
            adapter,
            thread_id: thread_id.into(),
            state_adapter: None,
            is_subscribed_context: false,
        }
    }

    /// 1:1 port of upstream `new ThreadImpl({ adapter, id,
    /// stateAdapter })`. Required when callers want to use
    /// [`Self::state`] / [`Self::set_state`] without the (not yet
    /// ported) singleton fallback path.
    pub fn with_state_adapter(
        adapter: Arc<dyn Adapter>,
        thread_id: impl Into<String>,
        state_adapter: Arc<dyn StateAdapter>,
    ) -> Self {
        Self {
            adapter,
            thread_id: thread_id.into(),
            state_adapter: Some(state_adapter),
            is_subscribed_context: false,
        }
    }

    /// Builder: mark this Thread handle as running inside a
    /// "subscribed" context. 1:1 with upstream
    /// `isSubscribedContext: true` constructor option — the chat
    /// event dispatcher sets this when invoking
    /// `onSubscribedMessage` handlers so [`Self::is_subscribed`]
    /// short-circuits without a state lookup.
    pub fn with_subscribed_context(mut self) -> Self {
        self.is_subscribed_context = true;
        self
    }

    /// Borrow the bound state adapter, if any. Returns `None` when
    /// [`Thread::new`] was used (upstream falls back to the chat
    /// singleton; not yet ported in Rust).
    pub fn state_adapter(&self) -> Option<&Arc<dyn StateAdapter>> {
        self.state_adapter.as_ref()
    }

    /// Thread-id accessor. 1:1 with upstream `get threadId(): string`.
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    /// Underlying adapter name. Convenience for `self.adapter.name()`.
    pub fn adapter_name(&self) -> &str {
        self.adapter.name()
    }

    /// Borrow the underlying adapter.
    pub fn adapter(&self) -> &Arc<dyn Adapter> {
        &self.adapter
    }

    /// Post a plain-text message to this thread. 1:1 with upstream
    /// `Thread.post(text)`. Returns the platform-assigned message id.
    pub async fn post(&self, text: &str) -> AdapterResult<String> {
        self.adapter.post_message(&self.thread_id, text).await
    }

    /// Post a postable envelope to this thread. 1:1 with upstream
    /// `Thread.postObject(value)`. Routes through
    /// [`post_postable_object`] for the same fallback-to-text
    /// behavior `Channel::post_object` has.
    pub async fn post_object(
        &self,
        envelope: &serde_json::Value,
    ) -> Result<String, PostableDispatchError> {
        post_postable_object(self.adapter.as_ref(), &self.thread_id, envelope).await
    }

    /// Fetch the thread subject via [`Adapter::fetch_subject`]. 1:1
    /// with upstream's inline `await this.adapter.fetchSubject(this.threadId)`
    /// at handler callsites that don't already have a `Message` to
    /// reuse cached subject from.
    ///
    /// Adapters that don't implement `fetch_subject` return the trait
    /// default `Ok(None)`.
    pub async fn subject(&self) -> AdapterResult<Option<String>> {
        self.adapter.fetch_subject(&self.thread_id).await
    }

    /// 1:1 port of upstream `Thread.startTyping(status?)`. Delegates
    /// to [`Adapter::start_typing`] with the bound thread id.
    pub async fn start_typing(&self, status: Option<&str>) -> AdapterResult<()> {
        self.adapter.start_typing(&self.thread_id, status).await
    }

    /// 1:1 port of upstream `Thread.mentionUser(userId)`. Returns the
    /// Slack-style mention syntax `<@userId>` (upstream hard-codes
    /// the angle-bracket wrapper independent of platform; per-adapter
    /// renderers translate to the platform-native form downstream).
    pub fn mention_user(&self, user_id: &str) -> String {
        format!("<@{user_id}>")
    }

    /// 1:1 port of upstream `Thread.subscribe()`. Records the
    /// subscription in the bound state adapter, then calls
    /// `adapter.on_thread_subscribe(thread_id)` (default no-op).
    /// No-op when the Thread was built without a state adapter
    /// (matches upstream's lazy-singleton fallback which Rust
    /// hasn't ported yet).
    pub async fn subscribe(&self) -> AdapterResult<()> {
        if let Some(state) = self.state_adapter.as_ref() {
            state
                .subscribe(&self.thread_id)
                .await
                .map_err(|e| AdapterError::Io(Box::new(e)))?;
        }
        self.adapter.on_thread_subscribe(&self.thread_id).await
    }

    /// 1:1 port of upstream `Thread.unsubscribe()`. Removes the
    /// subscription record from the bound state adapter. No-op when
    /// no state adapter is bound.
    pub async fn unsubscribe(&self) -> AdapterResult<()> {
        if let Some(state) = self.state_adapter.as_ref() {
            state
                .unsubscribe(&self.thread_id)
                .await
                .map_err(|e| AdapterError::Io(Box::new(e)))?;
        }
        Ok(())
    }

    /// 1:1 port of upstream `Thread.isSubscribed()`. Short-circuits
    /// to `true` when `is_subscribed_context` was set on the handle
    /// (subscribed-message handler optimization); otherwise consults
    /// the bound state adapter. Returns `false` when no state
    /// adapter is bound (matches upstream's "no state → not
    /// subscribed" default).
    pub async fn is_subscribed(&self) -> AdapterResult<bool> {
        if self.is_subscribed_context {
            return Ok(true);
        }
        let Some(state) = self.state_adapter.as_ref() else {
            return Ok(false);
        };
        state
            .is_subscribed(&self.thread_id)
            .await
            .map_err(|e| AdapterError::Io(Box::new(e)))
    }

    fn state_key(&self) -> String {
        format!("{THREAD_STATE_KEY_PREFIX}{}", self.thread_id)
    }

    /// 1:1 port of upstream `Thread.state` getter. Reads the stored
    /// state value via the bound [`StateAdapter`]. Returns
    /// `Ok(None)` when no state has been set, the bound adapter
    /// has expired the key, or when the Thread was built without a
    /// state adapter (matches upstream's `null` resolution).
    pub async fn state(&self) -> StateResult<Option<serde_json::Value>> {
        let Some(state) = self.state_adapter.as_ref() else {
            return Ok(None);
        };
        state.get(&self.state_key()).await
    }

    /// 1:1 port of upstream `Thread.setState(newState, options?)`.
    /// Merges `new_state` with any existing state under the same
    /// key (shallow merge, matching upstream's `{ ...existing,
    /// ...newState }` spread). State persists for
    /// [`THREAD_STATE_TTL_MS`] milliseconds. No-op when the Thread
    /// was built without a state adapter (matches upstream's lazy-
    /// singleton fallback, which Rust hasn't ported yet — the
    /// no-op is the safe default).
    pub async fn set_state(&self, new_state: serde_json::Value) -> StateResult<()> {
        self.set_state_with_options(new_state, false).await
    }

    /// 1:1 port of upstream `Thread.setState(newState, { replace:
    /// true })`. Overwrites any existing state under the key
    /// instead of merging.
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
            return state.set(&key, new_state, Some(THREAD_STATE_TTL_MS)).await;
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
        state.set(&key, merged, Some(THREAD_STATE_TTL_MS)).await
    }
}

#[cfg(test)]
mod tests {
    //! Additive coverage for the [`Thread`] surface. Upstream's
    //! `thread.test.ts` exercises every Adapter method via Thread —
    //! those will land as each method gets ported into the Adapter
    //! trait + Thread wrapper.
    use super::*;
    use crate::postable_object::postable_envelope;
    use crate::types::{AdapterError, AdapterResult};
    use futures_executor::block_on;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Default)]
    struct RecordingAdapter {
        post_message: Mutex<Vec<(String, String)>>,
        post_object: Mutex<Vec<(String, String, serde_json::Value)>>,
        post_object_unsupported: bool,
        fetch_subject_calls: AtomicUsize,
        subject_result: Option<String>,
        start_typing: Mutex<Vec<(String, Option<String>)>>,
        on_thread_subscribe: Mutex<Vec<String>>,
        on_thread_subscribe_unsupported: bool,
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
        async fn fetch_subject(&self, _thread_id: &str) -> AdapterResult<Option<String>> {
            self.fetch_subject_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.subject_result.clone())
        }
        async fn start_typing(
            &self,
            thread_id: &str,
            status: Option<&str>,
        ) -> AdapterResult<()> {
            self.start_typing
                .lock()
                .unwrap()
                .push((thread_id.to_string(), status.map(str::to_string)));
            Ok(())
        }
        async fn on_thread_subscribe(&self, thread_id: &str) -> AdapterResult<()> {
            if self.on_thread_subscribe_unsupported {
                return Err(AdapterError::Unsupported("on_thread_subscribe"));
            }
            self.on_thread_subscribe
                .lock()
                .unwrap()
                .push(thread_id.to_string());
            Ok(())
        }
    }

    #[test]
    fn thread_new_holds_adapter_and_thread_id() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter, "slack:C123:1.0");
        assert_eq!(thread.thread_id(), "slack:C123:1.0");
        assert_eq!(thread.adapter_name(), "recording");
    }

    #[test]
    fn thread_post_delegates_to_adapter_post_message() {
        let adapter = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "T1");
        let id = block_on(thread.post("hello")).unwrap();
        assert_eq!(id, "msg-id");
        let calls = adapter.post_message.lock().unwrap();
        assert_eq!(calls[0].0, "T1");
        assert_eq!(calls[0].1, "hello");
    }

    #[test]
    fn thread_post_object_dispatches_through_post_postable_object() {
        let adapter = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "T1");
        let envelope = postable_envelope("plan", serde_json::json!({"title": "T"}), "Plan: T");
        let id = block_on(thread.post_object(&envelope)).unwrap();
        assert_eq!(id, "obj-id");
        let calls = adapter.post_object.lock().unwrap();
        assert_eq!(calls[0].0, "T1");
        assert_eq!(calls[0].1, "plan");
    }

    #[test]
    fn thread_post_object_falls_back_to_post_message_when_unsupported() {
        let adapter = Arc::new(RecordingAdapter {
            post_object_unsupported: true,
            ..Default::default()
        });
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "T1");
        let envelope = postable_envelope("plan", serde_json::json!({}), "Plan-fb");
        let id = block_on(thread.post_object(&envelope)).unwrap();
        assert_eq!(id, "msg-id");
        let calls = adapter.post_message.lock().unwrap();
        assert_eq!(calls[0].0, "T1");
        assert_eq!(calls[0].1, "Plan-fb");
    }

    #[test]
    fn thread_subject_returns_value_from_adapter() {
        let adapter = Arc::new(RecordingAdapter {
            subject_result: Some("General".to_string()),
            ..Default::default()
        });
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "T1");
        let subject = block_on(thread.subject()).unwrap();
        assert_eq!(subject.as_deref(), Some("General"));
        assert_eq!(adapter.fetch_subject_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn thread_subject_returns_none_when_adapter_returns_none() {
        let adapter = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "T1");
        let subject = block_on(thread.subject()).unwrap();
        assert!(subject.is_none());
    }

    #[test]
    fn thread_clone_shares_adapter_arc() {
        let adapter = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "T1");
        let cloned = thread.clone();
        block_on(thread.post("a")).unwrap();
        block_on(cloned.post("b")).unwrap();
        assert_eq!(adapter.post_message.lock().unwrap().len(), 2);
    }

    // ---------- Per-thread state (8 upstream cases) ----------
    // 1:1 with upstream `thread.test.ts > describe("Per-thread state")`.

    use crate::types::StateAdapter;
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;

    /// 1:1 with upstream `createMockState()` (vi.fn()-backed
    /// HashMap). Records all `get` / `set` calls so tests can
    /// assert on call shape.
    #[derive(Debug, Default)]
    struct MockState {
        cache: StdMutex<HashMap<String, serde_json::Value>>,
        get_calls: StdMutex<Vec<String>>,
        set_calls: StdMutex<Vec<(String, serde_json::Value, Option<u64>)>>,
    }

    #[async_trait::async_trait]
    impl StateAdapter for MockState {
        async fn get(&self, key: &str) -> StateResult<Option<serde_json::Value>> {
            self.get_calls.lock().unwrap().push(key.to_string());
            Ok(self.cache.lock().unwrap().get(key).cloned())
        }
        async fn set(
            &self,
            key: &str,
            value: serde_json::Value,
            ttl_ms: Option<u64>,
        ) -> StateResult<()> {
            self.set_calls
                .lock()
                .unwrap()
                .push((key.to_string(), value.clone(), ttl_ms));
            self.cache.lock().unwrap().insert(key.to_string(), value);
            Ok(())
        }
        async fn delete(&self, key: &str) -> StateResult<()> {
            self.cache.lock().unwrap().remove(key);
            Ok(())
        }
        async fn append_to_list(
            &self,
            _key: &str,
            _value: serde_json::Value,
            _max_length: Option<usize>,
            _ttl_ms: Option<u64>,
        ) -> StateResult<()> {
            Ok(())
        }
        async fn get_list(
            &self,
            _key: &str,
            _limit: Option<usize>,
        ) -> StateResult<Vec<serde_json::Value>> {
            Ok(Vec::new())
        }
    }

    fn thread_with_state() -> (Thread, Arc<MockState>) {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let state = Arc::new(MockState::default());
        let thread = Thread::with_state_adapter(
            adapter,
            "slack:C123:1234.5678",
            state.clone() as Arc<dyn StateAdapter>,
        );
        (thread, state)
    }

    #[test]
    fn per_thread_state_returns_none_when_no_state_has_been_set() {
        let (thread, _state) = thread_with_state();
        let value = block_on(thread.state()).unwrap();
        assert!(value.is_none());
    }

    #[test]
    fn per_thread_state_returns_stored_state() {
        let (thread, state) = thread_with_state();
        state
            .cache
            .lock()
            .unwrap()
            .insert(
                "thread-state:slack:C123:1234.5678".to_string(),
                serde_json::json!({ "aiMode": true }),
            );
        let value = block_on(thread.state()).unwrap();
        assert_eq!(value, Some(serde_json::json!({ "aiMode": true })));
    }

    #[test]
    fn per_thread_state_sets_state_and_retrieves_it() {
        let (thread, _state) = thread_with_state();
        block_on(thread.set_state(serde_json::json!({ "aiMode": true }))).unwrap();
        let value = block_on(thread.state()).unwrap();
        assert_eq!(value, Some(serde_json::json!({ "aiMode": true })));
    }

    #[test]
    fn per_thread_state_merges_state_by_default() {
        let (thread, _state) = thread_with_state();
        block_on(thread.set_state(serde_json::json!({ "aiMode": true }))).unwrap();
        block_on(thread.set_state(serde_json::json!({ "counter": 5 }))).unwrap();
        let value = block_on(thread.state()).unwrap();
        assert_eq!(value, Some(serde_json::json!({ "aiMode": true, "counter": 5 })));
    }

    #[test]
    fn per_thread_state_overwrites_existing_keys_when_merging() {
        let (thread, _state) = thread_with_state();
        block_on(thread.set_state(serde_json::json!({ "aiMode": true, "counter": 1 }))).unwrap();
        block_on(thread.set_state(serde_json::json!({ "counter": 10 }))).unwrap();
        let value = block_on(thread.state()).unwrap();
        assert_eq!(value, Some(serde_json::json!({ "aiMode": true, "counter": 10 })));
    }

    #[test]
    fn per_thread_state_replaces_entire_state_when_replace_option_is_true() {
        let (thread, _state) = thread_with_state();
        block_on(thread.set_state(serde_json::json!({ "aiMode": true, "counter": 5 }))).unwrap();
        block_on(thread.set_state_replace(serde_json::json!({ "counter": 10 }))).unwrap();
        let value = block_on(thread.state()).unwrap();
        assert_eq!(value, Some(serde_json::json!({ "counter": 10 })));
    }

    #[test]
    fn per_thread_state_uses_correct_key_prefix_for_state_storage() {
        let (thread, state) = thread_with_state();
        block_on(thread.set_state(serde_json::json!({ "aiMode": true }))).unwrap();
        let set_calls = state.set_calls.lock().unwrap();
        let last = set_calls.last().unwrap();
        assert_eq!(last.0, "thread-state:slack:C123:1234.5678");
        assert_eq!(last.1, serde_json::json!({ "aiMode": true }));
        assert_eq!(last.2, Some(THREAD_STATE_TTL_MS));
    }

    #[test]
    fn per_thread_state_calls_get_with_correct_key() {
        let (thread, state) = thread_with_state();
        block_on(thread.state()).unwrap();
        let get_calls = state.get_calls.lock().unwrap();
        assert_eq!(get_calls.last().unwrap(), "thread-state:slack:C123:1234.5678");
    }

    // ---------- describe("startTyping") (2 upstream cases) + describe("mentionUser") (2 cases) ----------
    // 1:1 with upstream `thread.test.ts > describe("startTyping")` +
    // `describe("mentionUser")`.

    #[test]
    fn thread_start_typing_calls_adapter_start_typing_with_thread_id() {
        let adapter = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123:1234.5678");
        block_on(thread.start_typing(None)).unwrap();
        let calls = adapter.start_typing.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "slack:C123:1234.5678");
        assert!(calls[0].1.is_none());
    }

    #[test]
    fn thread_start_typing_passes_status_to_adapter_start_typing() {
        let adapter = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123:1234.5678");
        block_on(thread.start_typing(Some("thinking..."))).unwrap();
        let calls = adapter.start_typing.lock().unwrap();
        assert_eq!(calls[0].1.as_deref(), Some("thinking..."));
    }

    #[test]
    fn thread_mention_user_returns_formatted_mention_string() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter, "slack:C123:1234.5678");
        assert_eq!(thread.mention_user("U456"), "<@U456>");
    }

    #[test]
    fn thread_mention_user_handles_various_user_id_formats() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter, "slack:C123:1234.5678");
        assert_eq!(thread.mention_user("UABC123"), "<@UABC123>");
        assert_eq!(thread.mention_user("bot-user-id"), "<@bot-user-id>");
    }

    // ---------- describe("subscribe and unsubscribe") (4 cases) +
    //            describe("isSubscribed") (4 cases) ----------
    // 1:1 with upstream `thread.test.ts > describe("subscribe and
    // unsubscribe")` + `describe("isSubscribed")`.

    #[test]
    fn thread_subscribe_writes_subscription_via_state_adapter() {
        let (thread, state) = thread_with_state();
        block_on(thread.subscribe()).unwrap();
        // Default trait subscribe writes Bool(true) under
        // "subscribed:<thread_id>" via set.
        let set_calls = state.set_calls.lock().unwrap();
        let last = set_calls.last().unwrap();
        assert_eq!(last.0, "subscribed:slack:C123:1234.5678");
        assert_eq!(last.1, serde_json::Value::Bool(true));
    }

    #[test]
    fn thread_subscribe_calls_adapter_on_thread_subscribe_when_available() {
        let adapter = Arc::new(RecordingAdapter::default());
        let state = Arc::new(MockState::default());
        let thread = Thread::with_state_adapter(
            adapter.clone() as Arc<dyn Adapter>,
            "slack:C123:1234.5678",
            state as Arc<dyn StateAdapter>,
        );
        block_on(thread.subscribe()).unwrap();
        let calls = adapter.on_thread_subscribe.lock().unwrap();
        assert_eq!(calls.as_slice(), &["slack:C123:1234.5678".to_string()]);
    }

    #[test]
    fn thread_subscribe_does_not_error_when_adapter_has_no_on_thread_subscribe() {
        // 1:1 with upstream "should not error when adapter has no
        // onThreadSubscribe". Adapters that don't override the
        // optional trait method get the default Ok(()) impl, and
        // state.subscribe still runs.
        #[derive(Debug, Default)]
        struct NoSubscribeAdapter;
        #[async_trait::async_trait]
        impl Adapter for NoSubscribeAdapter {
            fn name(&self) -> &str {
                "no-sub"
            }
        }
        let adapter: Arc<dyn Adapter> = Arc::new(NoSubscribeAdapter);
        let state = Arc::new(MockState::default());
        let thread = Thread::with_state_adapter(
            adapter,
            "slack:C123:1234.5678",
            state.clone() as Arc<dyn StateAdapter>,
        );
        block_on(thread.subscribe()).unwrap();
        let set_calls = state.set_calls.lock().unwrap();
        assert_eq!(set_calls.last().unwrap().0, "subscribed:slack:C123:1234.5678");
    }

    #[test]
    fn thread_unsubscribe_removes_subscription_via_state_adapter() {
        let (thread, state) = thread_with_state();
        block_on(thread.subscribe()).unwrap();
        block_on(thread.unsubscribe()).unwrap();
        // Default trait unsubscribe deletes the key.
        let value = state
            .cache
            .lock()
            .unwrap()
            .get("subscribed:slack:C123:1234.5678")
            .cloned();
        assert!(value.is_none());
    }

    #[test]
    fn thread_is_subscribed_returns_false_when_not_subscribed() {
        let (thread, _state) = thread_with_state();
        let result = block_on(thread.is_subscribed()).unwrap();
        assert!(!result);
    }

    #[test]
    fn thread_is_subscribed_returns_true_after_subscribing() {
        let (thread, _state) = thread_with_state();
        block_on(thread.subscribe()).unwrap();
        let result = block_on(thread.is_subscribed()).unwrap();
        assert!(result);
    }

    #[test]
    fn thread_is_subscribed_returns_false_after_unsubscribing() {
        let (thread, _state) = thread_with_state();
        block_on(thread.subscribe()).unwrap();
        block_on(thread.unsubscribe()).unwrap();
        let result = block_on(thread.is_subscribed()).unwrap();
        assert!(!result);
    }

    #[test]
    fn thread_is_subscribed_short_circuits_when_is_subscribed_context_is_set() {
        // 1:1 with upstream "should short-circuit and return true when
        // isSubscribedContext is set". Verify the state adapter is
        // NOT called (no get_calls entries for the subscribed: key).
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let state = Arc::new(MockState::default());
        let thread = Thread::with_state_adapter(
            adapter,
            "slack:C123:1234.5678",
            state.clone() as Arc<dyn StateAdapter>,
        )
        .with_subscribed_context();
        let result = block_on(thread.is_subscribed()).unwrap();
        assert!(result);
        let get_calls = state.get_calls.lock().unwrap();
        assert!(
            !get_calls.iter().any(|k| k == "subscribed:slack:C123:1234.5678"),
            "state.get should NOT be called when subscribed context is set"
        );
    }
}
