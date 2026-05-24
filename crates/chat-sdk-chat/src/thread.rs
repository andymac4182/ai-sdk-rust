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
    Adapter, AdapterError, AdapterResult, Author, EphemeralMessage, PostEphemeralOptions,
    StateAdapter, StateResult, THREAD_STATE_TTL_MS,
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
    /// 1:1 with upstream `recentMessages: Message[]`. Wrapped in
    /// `Arc<Mutex>` so the handle remains `Clone` while allowing
    /// `set_recent_messages` to mutate the underlying buffer.
    recent_messages: Arc<std::sync::Mutex<Vec<crate::message::Message>>>,
    /// 1:1 with upstream `channelId?: string`. The platform-encoded
    /// channel id that owns this thread. Used by `to_json` to
    /// expose the channel relationship in the serialized shape.
    channel_id: Option<String>,
    /// 1:1 with upstream `currentMessage?: Message`. The active
    /// message being processed (set by the chat dispatcher for the
    /// handler call frame; survives serialize/deserialize round-trips).
    current_message: Option<crate::message::Message>,
    /// 1:1 with upstream `isDM: boolean`. Resolved at handle
    /// construction (by `Chat::open_dm` or the event dispatcher)
    /// from `adapter.is_dm(thread_id) ?? false`. The handle field
    /// is the cached value so accessors don't need an async hop.
    is_dm: bool,
    /// 1:1 with upstream `channelVisibility?: ChannelVisibility`.
    /// Defaults to `Unknown` matching upstream's default; set via
    /// [`Thread::with_channel_visibility`] when the adapter or
    /// caller knows the channel's privacy posture. Round-trips
    /// through `to_json` / `from_json` as the `channelVisibility`
    /// field.
    channel_visibility: crate::types::ChannelVisibility,
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
            recent_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
            channel_id: None,
            current_message: None,
            is_dm: false,
            channel_visibility: crate::types::ChannelVisibility::Unknown,
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
            recent_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
            channel_id: None,
            current_message: None,
            is_dm: false,
            channel_visibility: crate::types::ChannelVisibility::Unknown,
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

    /// Builder: seed the handle with an initial message. 1:1 with
    /// upstream `new ThreadImpl({ initialMessage })` constructor
    /// option — the chat dispatcher seeds the incoming message so
    /// `recent_messages` reflects the current event when handlers
    /// fire.
    pub fn with_initial_message(self, message: crate::message::Message) -> Self {
        {
            let mut buf = self.recent_messages.lock().unwrap();
            buf.push(message);
        }
        self
    }

    /// Builder: set the platform-encoded channel id for this thread.
    /// 1:1 with upstream `new ThreadImpl({ channelId })` constructor
    /// option.
    pub fn with_channel_id(mut self, channel_id: impl Into<String>) -> Self {
        self.channel_id = Some(channel_id.into());
        self
    }

    /// Builder: set the active "current" message being processed by
    /// the handler frame. 1:1 with upstream `new ThreadImpl({
    /// currentMessage })` constructor option.
    pub fn with_current_message(mut self, message: crate::message::Message) -> Self {
        self.current_message = Some(message);
        self
    }

    /// Builder: mark this thread as a DM. 1:1 with upstream
    /// `new ThreadImpl({ isDM: true })` constructor option.
    /// `Chat::open_dm` and the chat event dispatcher set this from
    /// `adapter.is_dm(thread_id)`.
    pub fn with_is_dm(mut self, is_dm: bool) -> Self {
        self.is_dm = is_dm;
        self
    }

    /// Builder: set the channel-visibility posture for this thread.
    /// 1:1 with upstream `new ThreadImpl({ channelVisibility })`
    /// constructor option. Defaults to `Unknown` when not set.
    pub fn with_channel_visibility(
        mut self,
        channel_visibility: crate::types::ChannelVisibility,
    ) -> Self {
        self.channel_visibility = channel_visibility;
        self
    }

    /// 1:1 with upstream `get channelVisibility(): ChannelVisibility`.
    pub fn channel_visibility(&self) -> crate::types::ChannelVisibility {
        self.channel_visibility
    }

    /// 1:1 with upstream `get isDM(): boolean`. Returns the cached
    /// DM flag set at construction.
    pub fn is_dm(&self) -> bool {
        self.is_dm
    }

    /// 1:1 with upstream `get channelId(): string | undefined`.
    pub fn channel_id(&self) -> Option<&str> {
        self.channel_id.as_deref()
    }

    /// 1:1 with upstream `get currentMessage(): Message | undefined`.
    pub fn current_message(&self) -> Option<&crate::message::Message> {
        self.current_message.as_ref()
    }

    /// 1:1 port of upstream `Thread.toJSON()`. Wire shape:
    /// `{ _type: "chat:Thread", id, channelId, channelVisibility,
    /// currentMessage?, isDM, adapterName }`. `currentMessage` is
    /// serialized via `Message::to_serialized` when present.
    pub fn to_json(&self) -> serde_json::Value {
        let mut json = serde_json::json!({
            "_type": "chat:Thread",
            "id": self.thread_id,
            "channelId": self.channel_id,
            "channelVisibility": serde_json::to_value(self.channel_visibility)
                .unwrap_or(serde_json::Value::String("unknown".to_string())),
            "currentMessage": serde_json::Value::Null,
            "isDM": self.is_dm,
            "adapterName": self.adapter.name(),
        });
        if let Some(msg) = self.current_message.as_ref() {
            json["currentMessage"] = serde_json::to_value(msg.to_serialized()).unwrap_or_default();
        }
        json
    }

    /// 1:1 port of upstream `static Thread.fromJSON(json, adapter)`.
    /// Reconstructs the handle from its serialized form. The
    /// `adapter` argument is supplied externally (1:1 with upstream's
    /// `adapter` parameter — the adapter isn't serialized).
    pub fn from_json(json: &serde_json::Value, adapter: Arc<dyn Adapter>) -> Self {
        let thread_id = json
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut thread = Self::new(adapter, thread_id);
        if let Some(channel_id) = json.get("channelId").and_then(serde_json::Value::as_str) {
            thread.channel_id = Some(channel_id.to_string());
        }
        if let Some(is_dm) = json.get("isDM").and_then(serde_json::Value::as_bool) {
            thread.is_dm = is_dm;
        }
        if let Some(visibility_value) = json.get("channelVisibility") {
            if let Ok(parsed) =
                serde_json::from_value::<crate::types::ChannelVisibility>(visibility_value.clone())
            {
                thread.channel_visibility = parsed;
            }
        }
        if let Some(serialized) = json.get("currentMessage") {
            if !serialized.is_null() {
                if let Ok(msg) =
                    serde_json::from_value::<crate::message::SerializedMessage>(serialized.clone())
                {
                    thread.current_message = Some(crate::message::Message::from_serialized(msg));
                }
            }
        }
        thread
    }

    /// 1:1 with upstream `get recentMessages(): Message[]`. Returns
    /// a snapshot clone (upstream returns a live array reference; the
    /// Rust port returns a clone to avoid exposing the internal
    /// `Mutex`).
    pub fn recent_messages(&self) -> Vec<crate::message::Message> {
        self.recent_messages.lock().unwrap().clone()
    }

    /// 1:1 with upstream `set recentMessages(value: Message[])`.
    /// Replaces the buffer atomically.
    pub fn set_recent_messages(&self, messages: Vec<crate::message::Message>) {
        let mut buf = self.recent_messages.lock().unwrap();
        *buf = messages;
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

    /// 1:1 port of upstream `Thread.postEphemeral(user, message, options)`.
    /// Tries native ephemeral via [`Adapter::post_ephemeral`]; on
    /// `Unsupported` falls back to DM (open_dm + post_message) when
    /// `options.fallback_to_dm` is `true`, otherwise returns
    /// `Ok(None)`. Returns `Ok(None)` when neither native ephemeral
    /// nor DM are available, matching upstream's
    /// `return null` final branch.
    pub async fn post_ephemeral(
        &self,
        user_id: &str,
        text: &str,
        options: PostEphemeralOptions,
    ) -> AdapterResult<Option<EphemeralMessage>> {
        match self
            .adapter
            .post_ephemeral(&self.thread_id, user_id, text)
            .await
        {
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

    /// 1:1 port of upstream `Thread.postEphemeral(author, message, options)`
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

    /// 1:1 port of upstream `Thread.mentionUser(userId)`. Returns the
    /// Slack-style mention syntax `<@userId>` (upstream hard-codes
    /// the angle-bracket wrapper independent of platform; per-adapter
    /// renderers translate to the platform-native form downstream).
    pub fn mention_user(&self, user_id: &str) -> String {
        format!("<@{user_id}>")
    }

    /// 1:1 port of upstream `Thread.schedule(text, options)`. The
    /// upstream method dispatches to `adapter.scheduleMessage(threadId,
    /// text, postAt)` when the adapter implements it, and throws a
    /// `NotImplementedError("Scheduled messages are not supported by
    /// this adapter", "scheduling")` otherwise.
    ///
    /// Per the slice-380 Unsupported-sentinel pattern, the dispatcher
    /// matches `Err(AdapterError::Unsupported)` and re-surfaces it
    /// as [`crate::errors::ChatError::NotImplemented`]. Other adapter
    /// errors propagate verbatim via [`crate::errors::ChatError::Adapter`].
    ///
    /// Returns a [`ScheduledMessageHandle`] that bundles the adapter-
    /// returned [`crate::types::ScheduledMessage`] with a `cancel()`
    /// method dispatching through [`crate::types::Adapter::cancel_scheduled_message`].
    /// This mirrors upstream's `ScheduledMessage.cancel(): Promise<void>`
    /// closure — the closure can't live on a Serialize+Eq struct so
    /// the cancellation handle lives on the thread-bound wrapper.
    pub async fn schedule(
        &self,
        text: &str,
        post_at_unix_ms: u64,
    ) -> Result<ScheduledMessageHandle, crate::errors::ChatError> {
        match self
            .adapter
            .schedule_message(&self.thread_id, text, post_at_unix_ms)
            .await
        {
            Ok(scheduled) => Ok(ScheduledMessageHandle {
                scheduled,
                adapter: self.adapter.clone(),
            }),
            Err(crate::types::AdapterError::Unsupported(_)) => {
                Err(crate::errors::ChatError::not_implemented_feature(
                    "Scheduled messages are not supported by this adapter",
                    "scheduling",
                ))
            }
            Err(other) => Err(crate::errors::ChatError::new(
                format!("{other}"),
                "ADAPTER_ERROR",
            )),
        }
    }

    /// 1:1 port of upstream `Thread.createSentMessageFromMessage(msg)`.
    /// Wraps an existing [`crate::message::Message`] as a
    /// [`SentMessage`] with edit/delete/add-reaction/remove-reaction
    /// capabilities bound to this thread's adapter. The Message
    /// fields are preserved as-is — accessors on `SentMessage`
    /// delegate to the wrapped Message.
    pub fn create_sent_message_from_message(
        &self,
        message: crate::message::Message,
    ) -> SentMessage {
        SentMessage {
            message,
            adapter: self.adapter.clone(),
        }
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

/// 1:1 port of upstream `interface SentMessage<TRawMessage>`. Wraps
/// a [`crate::message::Message`] with edit / delete / addReaction /
/// removeReaction capabilities bound to the adapter that posted it.
/// Constructed via [`Thread::create_sent_message_from_message`] or
/// returned from `Thread::post` (once that surfaces SentMessage —
/// currently it returns `String` message id; SentMessage construction
/// from a post-result lands in a follow-up slice).
#[derive(Clone)]
/// 1:1 with upstream `ScheduledMessage`-with-`cancel()` shape. The
/// upstream interface embeds the `cancel(): Promise<void>` closure on
/// the value itself; the Rust port keeps [`crate::types::ScheduledMessage`]
/// as a pure Serialize+Eq struct and binds the cancellation closure
/// here via the adapter that produced it.
pub struct ScheduledMessageHandle {
    scheduled: crate::types::ScheduledMessage,
    adapter: Arc<dyn Adapter>,
}

impl std::fmt::Debug for ScheduledMessageHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScheduledMessageHandle")
            .field("scheduled", &self.scheduled)
            .field("adapter", &self.adapter.name())
            .finish()
    }
}

impl ScheduledMessageHandle {
    /// Borrow the wrapped [`crate::types::ScheduledMessage`].
    pub fn scheduled(&self) -> &crate::types::ScheduledMessage {
        &self.scheduled
    }

    /// 1:1 with upstream `scheduled.scheduledMessageId`.
    pub fn scheduled_message_id(&self) -> &str {
        &self.scheduled.scheduled_message_id
    }

    /// 1:1 with upstream `scheduled.channelId`.
    pub fn channel_id(&self) -> &str {
        &self.scheduled.channel_id
    }

    /// 1:1 with upstream `scheduled.postAt` (Date → u64 epoch millis).
    pub fn post_at_unix_ms(&self) -> u64 {
        self.scheduled.post_at_unix_ms
    }

    /// 1:1 with upstream `scheduled.raw`.
    pub fn raw(&self) -> &serde_json::Value {
        &self.scheduled.raw
    }

    /// 1:1 with upstream `scheduled.cancel(): Promise<void>`.
    /// Dispatches through [`crate::types::Adapter::cancel_scheduled_message`].
    pub async fn cancel(&self) -> Result<(), crate::errors::ChatError> {
        match self
            .adapter
            .cancel_scheduled_message(
                &self.scheduled.channel_id,
                &self.scheduled.scheduled_message_id,
            )
            .await
        {
            Ok(()) => Ok(()),
            Err(crate::types::AdapterError::Unsupported(_)) => {
                Err(crate::errors::ChatError::not_implemented_feature(
                    "Scheduled message cancellation is not supported by this adapter",
                    "scheduling",
                ))
            }
            Err(other) => Err(crate::errors::ChatError::new(
                format!("{other}"),
                "ADAPTER_ERROR",
            )),
        }
    }
}

pub struct SentMessage {
    message: crate::message::Message,
    adapter: Arc<dyn Adapter>,
}

impl std::fmt::Debug for SentMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SentMessage")
            .field("message", &self.message)
            .field("adapter", &self.adapter.name())
            .finish()
    }
}

impl SentMessage {
    /// Borrow the wrapped [`crate::message::Message`].
    pub fn message(&self) -> &crate::message::Message {
        &self.message
    }

    /// 1:1 with upstream `sent.id` field (delegates to wrapped Message).
    pub fn id(&self) -> &str {
        &self.message.id
    }

    /// 1:1 with upstream `sent.text` field.
    pub fn text(&self) -> &str {
        &self.message.text
    }

    /// 1:1 with upstream `sent.threadId` field.
    pub fn thread_id(&self) -> &str {
        &self.message.thread_id
    }

    /// 1:1 with upstream `sent.author` field.
    pub fn author(&self) -> &crate::types::Author {
        &self.message.author
    }

    /// 1:1 with upstream `sent.metadata` field.
    pub fn metadata(&self) -> &crate::types::MessageMetadata {
        &self.message.metadata
    }

    /// 1:1 with upstream `sent.attachments` field.
    pub fn attachments(&self) -> &[crate::types::Attachment] {
        &self.message.attachments
    }

    /// 1:1 port of upstream `SentMessage.delete()`. Delegates to
    /// `adapter.delete_message(thread_id, message_id)`.
    pub async fn delete(&self) -> AdapterResult<()> {
        self.adapter
            .delete_message(&self.message.thread_id, &self.message.id)
            .await
    }

    /// 1:1 port of upstream `SentMessage.addReaction(emoji)`.
    pub async fn add_reaction(&self, emoji: &str) -> AdapterResult<()> {
        self.adapter
            .add_reaction(&self.message.thread_id, &self.message.id, emoji)
            .await
    }

    /// 1:1 port of upstream `SentMessage.removeReaction(emoji)`.
    pub async fn remove_reaction(&self, emoji: &str) -> AdapterResult<()> {
        self.adapter
            .remove_reaction(&self.message.thread_id, &self.message.id, emoji)
            .await
    }

    /// 1:1 port of upstream `SentMessage.edit(text)`. Returns the
    /// updated message id from the adapter's `edit_message` call.
    pub async fn edit(&self, text: &str) -> AdapterResult<String> {
        self.adapter
            .edit_message(&self.message.thread_id, &self.message.id, text)
            .await
    }
}

#[cfg(test)]
mod tests {
    //! Additive coverage for the [`Thread`] surface. Upstream's
    //! `thread.test.ts` exercises every Adapter method via Thread —
    //! those will land as each method gets ported into the Adapter
    //! trait + Thread wrapper.
    //!
    //! ---------- upstream js-only-documented cases (2) ----------
    //!
    //! Per the slice-380 type-system-impossible pattern, the
    //! following upstream `thread.test.ts > describe("schedule()")`
    //! cases are enumerated as js-only-documented here because they
    //! exercise a JS-only authoring surface unrepresentable in the
    //! Rust port by construction (slice 449):
    //!
    //! 1. `should convert JSX Card elements to CardElement before
    //!    passing to adapter` (thread.test.ts:2809) — asserts the
    //!    upstream `Card(...)` JSX-element factory is rewritten to
    //!    a plain `CardElement` object before being passed to
    //!    `adapter.scheduleMessage`. The Rust port has no JSX
    //!    runtime; `card(CardOptions { ... })` is already a builder
    //!    that returns the `CardElement` struct directly, so the
    //!    "convert JSX -> CardElement" branch is a no-op by
    //!    construction. See [`crate::cards::card`].
    //!
    //! 2. `should convert Card JSX with children to CardElement`
    //!    (thread.test.ts:2826) — same JSX-element factory, this
    //!    time with nested children. Same Rust-equivalent: the
    //!    builder takes children as a typed `Vec<CardChild>` and
    //!    produces a `CardElement` directly.
    //!
    //! These 2 cases are part of the 24-case upstream schedule()
    //! describe block; 18 are already 1:1 ported (slices 385,
    //! 403..405) and the other 4 are deferred behind PostableMessage
    //! input shapes that require the `from_full_stream` integration.
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
        edit_message: Mutex<Vec<(String, String, String)>>,
        delete_message: Mutex<Vec<(String, String)>>,
        add_reaction: Mutex<Vec<(String, String, String)>>,
        remove_reaction: Mutex<Vec<(String, String, String)>>,
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
        async fn start_typing(&self, thread_id: &str, status: Option<&str>) -> AdapterResult<()> {
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
        async fn edit_message(
            &self,
            thread_id: &str,
            message_id: &str,
            text: &str,
        ) -> AdapterResult<String> {
            self.edit_message.lock().unwrap().push((
                thread_id.to_string(),
                message_id.to_string(),
                text.to_string(),
            ));
            Ok(message_id.to_string())
        }
        async fn delete_message(&self, thread_id: &str, message_id: &str) -> AdapterResult<()> {
            self.delete_message
                .lock()
                .unwrap()
                .push((thread_id.to_string(), message_id.to_string()));
            Ok(())
        }
        async fn add_reaction(
            &self,
            thread_id: &str,
            message_id: &str,
            emoji: &str,
        ) -> AdapterResult<()> {
            self.add_reaction.lock().unwrap().push((
                thread_id.to_string(),
                message_id.to_string(),
                emoji.to_string(),
            ));
            Ok(())
        }
        async fn remove_reaction(
            &self,
            thread_id: &str,
            message_id: &str,
            emoji: &str,
        ) -> AdapterResult<()> {
            self.remove_reaction.lock().unwrap().push((
                thread_id.to_string(),
                message_id.to_string(),
                emoji.to_string(),
            ));
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
        state.cache.lock().unwrap().insert(
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
        assert_eq!(
            value,
            Some(serde_json::json!({ "aiMode": true, "counter": 5 }))
        );
    }

    #[test]
    fn per_thread_state_overwrites_existing_keys_when_merging() {
        let (thread, _state) = thread_with_state();
        block_on(thread.set_state(serde_json::json!({ "aiMode": true, "counter": 1 }))).unwrap();
        block_on(thread.set_state(serde_json::json!({ "counter": 10 }))).unwrap();
        let value = block_on(thread.state()).unwrap();
        assert_eq!(
            value,
            Some(serde_json::json!({ "aiMode": true, "counter": 10 }))
        );
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
        assert_eq!(
            get_calls.last().unwrap(),
            "thread-state:slack:C123:1234.5678"
        );
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
        assert_eq!(
            set_calls.last().unwrap().0,
            "subscribed:slack:C123:1234.5678"
        );
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

    // ---------- describe("createSentMessageFromMessage") (1 of 4+ cases) ----------
    // 1:1 with upstream `thread.test.ts > describe("createSentMessageFromMessage")
    // > it("should wrap a Message as a SentMessage with same fields")`.
    // The remaining cases (edit/delete/addReaction/removeReaction
    // capabilities) require HTTP mocking infrastructure to assert
    // adapter calls — deferred to a follow-up slice.

    #[test]
    fn thread_create_sent_message_from_message_wraps_with_same_fields() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter, "slack:C123:1234.5678");
        let msg = sample_message("msg-1", "Hello world");
        let sent = thread.create_sent_message_from_message(msg.clone());
        assert_eq!(sent.id(), "msg-1");
        assert_eq!(sent.text(), "Hello world");
        assert_eq!(sent.thread_id(), msg.thread_id);
        assert_eq!(sent.author(), &msg.author);
        assert_eq!(sent.metadata(), &msg.metadata);
        assert_eq!(sent.attachments(), &msg.attachments[..]);
    }

    #[test]
    fn sent_message_edit_delegates_to_adapter_edit_message() {
        // 1:1 with upstream "should provide edit capability".
        let adapter = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123:1234.5678");
        let msg = sample_message("msg-1", "Hello");
        let sent = thread.create_sent_message_from_message(msg);
        let new_id = block_on(sent.edit("edited content")).unwrap();
        assert_eq!(new_id, "msg-1");
        let calls = adapter.edit_message.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            &[(
                "slack:C123:1234.5678".to_string(),
                "msg-1".to_string(),
                "edited content".to_string()
            )]
        );
    }

    #[test]
    fn sent_message_delete_delegates_to_adapter_delete_message() {
        // 1:1 with upstream "should provide delete capability".
        let adapter = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123:1234.5678");
        let msg = sample_message("msg-1", "Hello");
        let sent = thread.create_sent_message_from_message(msg);
        block_on(sent.delete()).unwrap();
        let calls = adapter.delete_message.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            &[("slack:C123:1234.5678".to_string(), "msg-1".to_string())]
        );
    }

    #[test]
    fn sent_message_add_reaction_delegates_to_adapter_add_reaction() {
        // 1:1 with upstream "should provide addReaction capability".
        let adapter = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123:1234.5678");
        let msg = sample_message("msg-1", "Hello");
        let sent = thread.create_sent_message_from_message(msg);
        block_on(sent.add_reaction("thumbsup")).unwrap();
        let calls = adapter.add_reaction.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            &[(
                "slack:C123:1234.5678".to_string(),
                "msg-1".to_string(),
                "thumbsup".to_string()
            )]
        );
    }

    #[test]
    fn sent_message_remove_reaction_delegates_to_adapter_remove_reaction() {
        // 1:1 with upstream "should provide removeReaction capability".
        let adapter = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123:1234.5678");
        let msg = sample_message("msg-1", "Hello");
        let sent = thread.create_sent_message_from_message(msg);
        block_on(sent.remove_reaction("thumbsup")).unwrap();
        let calls = adapter.remove_reaction.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            &[(
                "slack:C123:1234.5678".to_string(),
                "msg-1".to_string(),
                "thumbsup".to_string()
            )]
        );
    }

    // ---------- describe("recentMessages getter/setter") (4 cases) ----------
    // 1:1 with upstream `thread.test.ts > describe("recentMessages
    // getter/setter")`.

    fn sample_message(id: &str, text: &str) -> crate::message::Message {
        use crate::markdown::root;
        use crate::types::{Author, BotStatus, MessageMetadata};
        crate::message::Message::new(
            id,
            "slack:C123:1234.5678",
            text,
            root(vec![]),
            serde_json::json!({}),
            Author {
                user_id: "U_AUTHOR".to_string(),
                user_name: "author".to_string(),
                full_name: "Author".to_string(),
                is_bot: BotStatus::Known(false),
                is_me: false,
            },
            MessageMetadata {
                date_sent: "2024-01-15T10:30:00.000Z".to_string(),
                edited: false,
                edited_at: None,
            },
            Vec::new(),
        )
    }

    #[test]
    fn thread_recent_messages_should_start_with_empty_array_by_default() {
        let (thread, _state) = thread_with_state();
        assert!(thread.recent_messages().is_empty());
    }

    #[test]
    fn thread_recent_messages_should_initialize_with_initial_message_when_provided() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let state = Arc::new(MockState::default());
        let thread = Thread::with_state_adapter(
            adapter,
            "slack:C123:1234.5678",
            state as Arc<dyn StateAdapter>,
        )
        .with_initial_message(sample_message("msg-1", "Initial"));
        let msgs = thread.recent_messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "Initial");
    }

    #[test]
    fn thread_recent_messages_should_allow_setting() {
        let (thread, _state) = thread_with_state();
        thread.set_recent_messages(vec![
            sample_message("msg-1", "First"),
            sample_message("msg-2", "Second"),
        ]);
        let msgs = thread.recent_messages();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].text, "First");
        assert_eq!(msgs[1].text, "Second");
    }

    #[test]
    fn thread_recent_messages_should_allow_replacing() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let state = Arc::new(MockState::default());
        let thread = Thread::with_state_adapter(
            adapter,
            "slack:C123:1234.5678",
            state as Arc<dyn StateAdapter>,
        )
        .with_initial_message(sample_message("msg-1", "Initial"));
        thread.set_recent_messages(vec![sample_message("msg-2", "Replaced")]);
        let msgs = thread.recent_messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "Replaced");
    }

    // ---------- describe("serialization") (4 upstream cases) ----------
    // 1:1 with upstream `thread.test.ts > describe("serialization")`.

    #[test]
    fn thread_serialization_should_serialize_to_json() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let state = Arc::new(MockState::default());
        let thread = Thread::with_state_adapter(
            adapter,
            "slack:C123:1234.5678",
            state as Arc<dyn StateAdapter>,
        )
        .with_channel_id("C123");
        let json = thread.to_json();
        assert_eq!(json["_type"], "chat:Thread");
        assert_eq!(json["id"], "slack:C123:1234.5678");
        assert_eq!(json["channelId"], "C123");
        assert_eq!(json["channelVisibility"], "unknown");
        assert!(json["currentMessage"].is_null());
        assert_eq!(json["adapterName"], "recording");
    }

    #[test]
    fn thread_serialization_should_reconstruct_dm_thread() {
        // 1:1 with upstream `describe("ThreadImpl.fromJSON()") >
        // it("should reconstruct DM thread")` — `isDM: true` is
        // preserved when reading a serialized DM thread back.
        let json = serde_json::json!({
            "_type": "chat:Thread",
            "id": "slack:DU456:",
            "channelId": "DU456",
            "isDM": true,
            "adapterName": "slack",
        });
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::from_json(&json, adapter);
        assert!(thread.is_dm());
        assert_eq!(thread.thread_id(), "slack:DU456:");
    }

    #[test]
    fn thread_serialization_should_round_trip_correctly() {
        // 1:1 with upstream `describe("ThreadImpl.fromJSON()") >
        // it("should round-trip correctly")` — toJSON+fromJSON
        // preserves id / channelId / isDM / adapter.name.
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let state = Arc::new(MockState::default());
        let original = Thread::with_state_adapter(
            adapter.clone(),
            "slack:C123:1234.5678",
            state as Arc<dyn StateAdapter>,
        )
        .with_channel_id("C123")
        .with_is_dm(true);
        let json = original.to_json();
        let restored = Thread::from_json(&json, adapter);
        assert_eq!(restored.thread_id(), original.thread_id());
        assert_eq!(restored.channel_id(), original.channel_id());
        assert_eq!(restored.is_dm(), original.is_dm());
        assert_eq!(restored.adapter_name(), original.adapter_name());
    }

    #[test]
    fn thread_serialization_should_produce_json_serializable_output() {
        // 1:1 with upstream `describe("ThreadImpl.toJSON()") >
        // it("should produce JSON-serializable output")` — the
        // serialized form round-trips through `JSON.stringify` +
        // `JSON.parse` losslessly (Rust equivalent:
        // `serde_json::to_string` → `serde_json::from_str`).
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter, "slack:C123:1234.5678").with_channel_id("C123");
        let json = thread.to_json();
        let text = serde_json::to_string(&json).expect("serialize");
        let _parsed: serde_json::Value = serde_json::from_str(&text).expect("re-parse round-trips");
    }

    #[test]
    fn thread_serialization_should_serialize_dm_thread_correctly() {
        // 1:1 with upstream `describe("ThreadImpl.toJSON()") >
        // it("should serialize DM thread correctly")` — `isDM: true`
        // round-trips through the JSON shape.
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let state = Arc::new(MockState::default());
        let thread =
            Thread::with_state_adapter(adapter, "slack:DU123:", state as Arc<dyn StateAdapter>)
                .with_channel_id("DU123")
                .with_is_dm(true);
        let json = thread.to_json();
        assert_eq!(json["_type"], "chat:Thread");
        assert_eq!(json["id"], "slack:DU123:");
        assert_eq!(json["channelId"], "DU123");
        assert_eq!(json["isDM"], true);
    }

    #[test]
    fn thread_serialization_should_serialize_external_channel_thread_correctly() {
        // 1:1 with upstream `describe("ThreadImpl.toJSON()") >
        // it("should serialize external channel thread correctly")`
        // — `channelVisibility: "external"` round-trips through the
        // JSON shape.
        use crate::types::ChannelVisibility;
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let state = Arc::new(MockState::default());
        let thread = Thread::with_state_adapter(
            adapter,
            "slack:C123:1234.5678",
            state as Arc<dyn StateAdapter>,
        )
        .with_channel_id("C123")
        .with_channel_visibility(ChannelVisibility::External);
        let json = thread.to_json();
        assert_eq!(json["_type"], "chat:Thread");
        assert_eq!(json["channelVisibility"], "external");
    }

    #[test]
    fn thread_serialization_should_serialize_private_channel_thread_correctly() {
        // 1:1 with upstream `describe("ThreadImpl.toJSON()") >
        // it("should serialize private channel thread correctly")`
        // — `channelVisibility: "private"` round-trips through the
        // JSON shape.
        use crate::types::ChannelVisibility;
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let state = Arc::new(MockState::default());
        let thread = Thread::with_state_adapter(
            adapter,
            "slack:C123:1234.5678",
            state as Arc<dyn StateAdapter>,
        )
        .with_channel_id("C123")
        .with_channel_visibility(ChannelVisibility::Private);
        let json = thread.to_json();
        assert_eq!(json["_type"], "chat:Thread");
        assert_eq!(json["channelVisibility"], "private");
    }

    #[test]
    fn thread_serialization_should_serialize_workspace_channel_thread_correctly() {
        // 1:1 with upstream `describe("ThreadImpl.toJSON()") >
        // it("should serialize workspace channel thread correctly")`
        // — `channelVisibility: "workspace"` round-trips through the
        // JSON shape.
        use crate::types::ChannelVisibility;
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let state = Arc::new(MockState::default());
        let thread = Thread::with_state_adapter(
            adapter,
            "slack:C123:1234.5678",
            state as Arc<dyn StateAdapter>,
        )
        .with_channel_id("C123")
        .with_channel_visibility(ChannelVisibility::Workspace);
        let json = thread.to_json();
        assert_eq!(json["channelVisibility"], "workspace");
    }

    #[test]
    fn thread_serialization_should_default_isdm_to_false_when_omitted() {
        // 1:1 with upstream "should default isDM to false when not
        // provided" — Thread constructor defaults `is_dm` to false
        // when no `with_is_dm(_)` builder call is made.
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter, "slack:C1:1.0");
        let json = thread.to_json();
        assert_eq!(json["isDM"], false);
    }

    #[test]
    fn thread_serialization_fromjson_should_default_isdm_to_false_when_omitted() {
        // 1:1 with upstream `describe("ThreadImpl.fromJSON()") >
        // it("should default isDM to false when not present in JSON")`.
        let json = serde_json::json!({
            "_type": "chat:Thread",
            "id": "slack:C1:1.0",
            "channelId": "C1",
            "adapterName": "recording",
        });
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::from_json(&json, adapter);
        assert!(!thread.is_dm());
    }

    #[test]
    fn thread_serialization_fromjson_should_set_isdm_to_true_when_present() {
        // 1:1 with upstream `describe("ThreadImpl.fromJSON()") >
        // it("should set isDM to true when present in JSON")`.
        let json = serde_json::json!({
            "_type": "chat:Thread",
            "id": "slack:DU1:",
            "channelId": "DU1",
            "isDM": true,
            "adapterName": "recording",
        });
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::from_json(&json, adapter);
        assert!(thread.is_dm());
    }

    #[test]
    fn thread_serialization_should_serialize_with_current_message() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let state = Arc::new(MockState::default());
        let thread = Thread::with_state_adapter(
            adapter,
            "slack:C123:1234.5678",
            state as Arc<dyn StateAdapter>,
        )
        .with_channel_id("C123")
        .with_current_message(sample_message("msg-1", "Current"));
        let json = thread.to_json();
        let current = &json["currentMessage"];
        assert!(!current.is_null());
        assert_eq!(current["_type"], "chat:Message");
        assert_eq!(current["text"], "Current");
    }

    #[test]
    fn thread_serialization_should_deserialize_from_json_with_explicit_adapter() {
        let json = serde_json::json!({
            "_type": "chat:Thread",
            "id": "slack:C123:1234.5678",
            "channelId": "C123",
            "isDM": false,
            "adapterName": "slack",
        });
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::from_json(&json, adapter.clone());
        assert_eq!(thread.thread_id(), "slack:C123:1234.5678");
        assert_eq!(thread.channel_id(), Some("C123"));
        assert!(Arc::ptr_eq(thread.adapter(), &adapter));
    }

    #[test]
    fn thread_serialization_should_deserialize_with_current_message() {
        // Serialize a message, embed it, deserialize the thread,
        // then re-serialize and observe the message text round-trips.
        let msg = sample_message("msg-1", "Serialized");
        let serialized_msg = serde_json::to_value(msg.to_serialized()).unwrap();
        let json = serde_json::json!({
            "_type": "chat:Thread",
            "id": "slack:C123:1234.5678",
            "channelId": "C123",
            "currentMessage": serialized_msg,
            "isDM": false,
            "adapterName": "slack",
        });
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::from_json(&json, adapter);
        let round_tripped = thread.to_json();
        assert_eq!(round_tripped["currentMessage"]["text"], "Serialized");
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
            !get_calls
                .iter()
                .any(|k| k == "subscribed:slack:C123:1234.5678"),
            "state.get should NOT be called when subscribed context is set"
        );
    }

    // ---------- describe("schedule()") (3 upstream cases) ----------
    // 1:1 with upstream `thread.test.ts > describe("schedule()")`.
    // Upstream's 3 NotImplementedError default cases all assert that
    // calling schedule on an adapter without scheduleMessage support
    // throws NotImplementedError with the right feature/message
    // fields. The Rust port currently always returns
    // ChatError::NotImplemented since no adapter ships
    // schedule_message yet. The remaining upstream cases
    // (mockResolvedValue scenarios for native scheduling) need
    // Adapter::schedule_message trait extension first.

    const FUTURE_UNIX_MS: u64 = 1_893_456_000_000; // 2030-01-01T00:00:00Z

    #[test]
    fn thread_schedule_should_throw_not_implemented_error_when_adapter_has_no_schedule_message() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter, "slack:C123:1234.5678");
        let err =
            block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).expect_err("expected ChatError");
        assert!(err.is_not_implemented(), "expected NotImplemented variant");
    }

    #[test]
    fn thread_schedule_should_include_scheduling_as_the_feature_in_not_implemented_error() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter, "slack:C123:1234.5678");
        let err =
            block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).expect_err("expected ChatError");
        assert_eq!(err.feature(), Some("scheduling"));
    }

    #[test]
    fn thread_schedule_should_include_descriptive_message_in_not_implemented_error() {
        let adapter: Arc<dyn Adapter> = Arc::new(RecordingAdapter::default());
        let thread = Thread::new(adapter, "slack:C123:1234.5678");
        let err =
            block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).expect_err("expected ChatError");
        assert!(
            err.message()
                .contains("Scheduled messages are not supported by this adapter"),
            "got: {}",
            err.message()
        );
    }

    // ---------- describe("schedule()") (additional 5 of 24 upstream cases) ----------
    // Slice 403 adds Adapter::schedule_message trait method + a
    // SchedulingAdapter test mock. The basic-delegation + return-
    // shape upstream cases are now mapped; the cancel(), JSX-Card
    // conversion, AsyncIterable, and adapter-error-propagation cases
    // remain deferred behind ScheduledMessage::cancel closure
    // wiring and JSX-runtime / Stream / SentMessage infrastructure.

    #[derive(Debug, Default)]
    struct SchedulingAdapter {
        schedule_calls: Mutex<Vec<(String, String, u64)>>,
        scheduled_message_id: String,
        channel_id: String,
        post_at_unix_ms: u64,
        raw: serde_json::Value,
    }

    #[async_trait::async_trait]
    impl Adapter for SchedulingAdapter {
        fn name(&self) -> &str {
            "scheduling"
        }
        async fn schedule_message(
            &self,
            thread_id: &str,
            text: &str,
            post_at_unix_ms: u64,
        ) -> AdapterResult<crate::types::ScheduledMessage> {
            self.schedule_calls.lock().unwrap().push((
                thread_id.to_string(),
                text.to_string(),
                post_at_unix_ms,
            ));
            Ok(crate::types::ScheduledMessage {
                scheduled_message_id: self.scheduled_message_id.clone(),
                channel_id: self.channel_id.clone(),
                post_at_unix_ms: self.post_at_unix_ms,
                raw: self.raw.clone(),
            })
        }
    }

    #[test]
    fn thread_schedule_should_delegate_to_adapter_schedule_message_with_correct_threadid() {
        // 1:1 with upstream "should delegate to adapter.scheduleMessage
        // with correct threadId".
        let adapter = Arc::new(SchedulingAdapter {
            scheduled_message_id: "Q123".to_string(),
            channel_id: "C123".to_string(),
            post_at_unix_ms: FUTURE_UNIX_MS,
            ..Default::default()
        });
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123:1234.5678");
        block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).unwrap();
        let calls = adapter.schedule_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "slack:C123:1234.5678");
        assert_eq!(calls[0].1, "Hello");
        assert_eq!(calls[0].2, FUTURE_UNIX_MS);
    }

    #[test]
    fn thread_schedule_should_return_scheduled_message_id_from_adapter() {
        // 1:1 with upstream "should return scheduledMessageId from adapter".
        let adapter = Arc::new(SchedulingAdapter {
            scheduled_message_id: "Q999".to_string(),
            channel_id: "C123".to_string(),
            post_at_unix_ms: FUTURE_UNIX_MS,
            ..Default::default()
        });
        let thread = Thread::new(adapter as Arc<dyn Adapter>, "slack:C123:1234.5678");
        let result = block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).unwrap();
        assert_eq!(result.scheduled_message_id(), "Q999");
    }

    #[test]
    fn thread_schedule_should_return_channel_id_from_adapter() {
        // 1:1 with upstream "should return channelId from adapter".
        let adapter = Arc::new(SchedulingAdapter {
            scheduled_message_id: "Q123".to_string(),
            channel_id: "C456".to_string(),
            post_at_unix_ms: FUTURE_UNIX_MS,
            ..Default::default()
        });
        let thread = Thread::new(adapter as Arc<dyn Adapter>, "slack:C123:1234.5678");
        let result = block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).unwrap();
        assert_eq!(result.channel_id(), "C456");
    }

    #[test]
    fn thread_schedule_should_return_post_at_from_adapter() {
        // 1:1 with upstream "should return postAt from adapter".
        const CUSTOM_UNIX_MS: u64 = 2_065_516_800_000; // 2035-06-15T12:00:00Z
        let adapter = Arc::new(SchedulingAdapter {
            scheduled_message_id: "Q123".to_string(),
            channel_id: "C123".to_string(),
            post_at_unix_ms: CUSTOM_UNIX_MS,
            ..Default::default()
        });
        let thread = Thread::new(adapter as Arc<dyn Adapter>, "slack:C123:1234.5678");
        let result = block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).unwrap();
        assert_eq!(result.post_at_unix_ms(), CUSTOM_UNIX_MS);
    }

    #[test]
    fn thread_schedule_should_return_raw_platform_response_from_adapter() {
        // 1:1 with upstream "should return raw platform response from
        // adapter".
        let raw = serde_json::json!({
            "ok": true,
            "scheduled_message_id": "Q123",
            "post_at": 123,
        });
        let adapter = Arc::new(SchedulingAdapter {
            scheduled_message_id: "Q123".to_string(),
            channel_id: "C123".to_string(),
            post_at_unix_ms: FUTURE_UNIX_MS,
            raw: raw.clone(),
            ..Default::default()
        });
        let thread = Thread::new(adapter as Arc<dyn Adapter>, "slack:C123:1234.5678");
        let result = block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).unwrap();
        assert_eq!(result.raw(), &raw);
    }

    // ---------- describe("schedule()") — additional slice 404 cases ----------

    /// SchedulingAdapter variant that rejects with an IO error, so we
    /// can assert that non-Unsupported adapter errors propagate as
    /// ChatError::Base rather than NotImplemented.
    #[derive(Debug, Default)]
    struct FailingSchedulingAdapter;

    #[async_trait::async_trait]
    impl Adapter for FailingSchedulingAdapter {
        fn name(&self) -> &str {
            "failing-scheduling"
        }
        async fn schedule_message(
            &self,
            _thread_id: &str,
            _text: &str,
            _post_at_unix_ms: u64,
        ) -> AdapterResult<crate::types::ScheduledMessage> {
            Err(AdapterError::Io(
                std::io::Error::other("Slack API error").into(),
            ))
        }
    }

    /// SchedulingAdapter that also records postMessage so we can
    /// assert post_message is not invoked during schedule().
    #[derive(Debug, Default)]
    struct SchedulingAndPostingAdapter {
        post_message: Mutex<Vec<(String, String)>>,
        schedule_calls: Mutex<Vec<(String, String, u64)>>,
    }

    #[async_trait::async_trait]
    impl Adapter for SchedulingAndPostingAdapter {
        fn name(&self) -> &str {
            "scheduling-and-posting"
        }
        async fn post_message(&self, thread_id: &str, text: &str) -> AdapterResult<String> {
            self.post_message
                .lock()
                .unwrap()
                .push((thread_id.to_string(), text.to_string()));
            Ok("msg-id".to_string())
        }
        async fn schedule_message(
            &self,
            thread_id: &str,
            text: &str,
            post_at_unix_ms: u64,
        ) -> AdapterResult<crate::types::ScheduledMessage> {
            self.schedule_calls.lock().unwrap().push((
                thread_id.to_string(),
                text.to_string(),
                post_at_unix_ms,
            ));
            Ok(crate::types::ScheduledMessage {
                scheduled_message_id: "Q123".to_string(),
                channel_id: "C123".to_string(),
                post_at_unix_ms,
                raw: serde_json::Value::Null,
            })
        }
    }

    #[test]
    fn thread_schedule_should_propagate_errors_thrown_by_adapter_schedule_message() {
        // 1:1 with upstream "should propagate errors thrown by
        // adapter.scheduleMessage". Non-Unsupported adapter errors
        // surface as ChatError::Base with the upstream message
        // contained in the formatted string, NOT as NotImplemented.
        let adapter: Arc<dyn Adapter> = Arc::new(FailingSchedulingAdapter);
        let thread = Thread::new(adapter, "slack:C123:1234.5678");
        let err =
            block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).expect_err("expected ChatError");
        assert!(
            !err.is_not_implemented(),
            "Io error must not be coerced into NotImplemented"
        );
        assert!(
            err.message().contains("Slack API error"),
            "expected adapter message in error; got: {}",
            err.message()
        );
    }

    #[test]
    fn thread_schedule_should_not_call_adapter_post_message_when_scheduling() {
        // 1:1 with upstream "should not call adapter.postMessage when
        // scheduling".
        let adapter = Arc::new(SchedulingAndPostingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123:1234.5678");
        block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).unwrap();
        assert!(
            adapter.post_message.lock().unwrap().is_empty(),
            "post_message must not be invoked during schedule()"
        );
        assert_eq!(adapter.schedule_calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn thread_schedule_should_use_the_threads_own_id_for_scheduling() {
        // 1:1 with upstream "should use the thread's own ID for
        // scheduling".
        let adapter = Arc::new(SchedulingAndPostingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C999:9999.0000");
        block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).unwrap();
        let calls = adapter.schedule_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "slack:C999:9999.0000");
        assert_eq!(calls[0].1, "Hello");
        assert_eq!(calls[0].2, FUTURE_UNIX_MS);
    }

    #[test]
    fn thread_schedule_should_allow_scheduling_multiple_messages_on_the_same_thread() {
        // 1:1 with upstream "should allow scheduling multiple
        // messages on the same thread".
        let adapter = Arc::new(SchedulingAndPostingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123:1234.5678");
        let s1 = block_on(thread.schedule("First", FUTURE_UNIX_MS)).unwrap();
        let s2 = block_on(thread.schedule("Second", FUTURE_UNIX_MS)).unwrap();
        let s3 = block_on(thread.schedule("Third", FUTURE_UNIX_MS)).unwrap();
        // The SchedulingAndPostingAdapter returns the same id every
        // call (matching the simplest mock — upstream's
        // mockResolvedValueOnce gives Q1/Q2/Q3 distinct ids; the Rust
        // mock doesn't need that to verify multi-schedule per-thread
        // dispatch, which is the upstream invariant).
        assert_eq!(s1.scheduled_message_id(), "Q123");
        assert_eq!(s2.scheduled_message_id(), "Q123");
        assert_eq!(s3.scheduled_message_id(), "Q123");
        assert_eq!(adapter.schedule_calls.lock().unwrap().len(), 3);
    }

    #[test]
    fn thread_schedule_should_pass_string_messages_through_directly() {
        // 1:1 with upstream "should pass string messages through
        // directly". Rust port currently only accepts &str, which
        // matches this case exactly — the other 3 upstream cases
        // (raw/markdown/ast message-object passthrough) require a
        // PostableMessage input enum and are deferred.
        let adapter = Arc::new(SchedulingAndPostingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123:1234.5678");
        block_on(thread.schedule("Plain text", FUTURE_UNIX_MS)).unwrap();
        let calls = adapter.schedule_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "Plain text");
    }

    // ---------- describe("schedule()") cancel() — slice 405 ----------
    //
    // Slice 405 adds [`ScheduledMessageHandle`] (Thread::schedule
    // return type) which bundles the adapter-returned ScheduledMessage
    // with a cancel() method dispatching through
    // Adapter::cancel_scheduled_message. The 4 upstream cancel()
    // describe cases are now mapped via a CancelingAdapter test mock
    // with serial scheduledMessageIds + a per-id cancel counter and
    // optional rejection mode.

    #[derive(Debug, Default)]
    struct CancelingAdapter {
        next_id: AtomicUsize,
        schedule_calls: Mutex<Vec<(String, String, u64)>>,
        cancel_calls: Mutex<Vec<(String, String)>>,
        cancel_should_fail_with: Option<&'static str>,
    }

    #[async_trait::async_trait]
    impl Adapter for CancelingAdapter {
        fn name(&self) -> &str {
            "canceling"
        }
        async fn schedule_message(
            &self,
            thread_id: &str,
            text: &str,
            post_at_unix_ms: u64,
        ) -> AdapterResult<crate::types::ScheduledMessage> {
            self.schedule_calls.lock().unwrap().push((
                thread_id.to_string(),
                text.to_string(),
                post_at_unix_ms,
            ));
            let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(crate::types::ScheduledMessage {
                scheduled_message_id: format!("Q{id}"),
                channel_id: "C123".to_string(),
                post_at_unix_ms,
                raw: serde_json::Value::Null,
            })
        }
        async fn cancel_scheduled_message(
            &self,
            channel_id: &str,
            scheduled_message_id: &str,
        ) -> AdapterResult<()> {
            self.cancel_calls
                .lock()
                .unwrap()
                .push((channel_id.to_string(), scheduled_message_id.to_string()));
            if let Some(msg) = self.cancel_should_fail_with {
                return Err(AdapterError::Io(std::io::Error::other(msg).into()));
            }
            Ok(())
        }
    }

    #[test]
    fn thread_schedule_should_return_a_cancel_function() {
        // 1:1 with upstream "should return a cancel function". The
        // Rust equivalent is "cancel() is a method on the handle that
        // returns a Future"; we verify by being able to call it.
        let adapter = Arc::new(CancelingAdapter::default());
        let thread = Thread::new(adapter as Arc<dyn Adapter>, "slack:C123:1234.5678");
        let handle = block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).unwrap();
        // .cancel() exists as a callable async method.
        block_on(handle.cancel()).unwrap();
    }

    #[test]
    fn thread_schedule_should_invoke_cancel_without_errors() {
        // 1:1 with upstream "should invoke cancel without errors".
        let adapter = Arc::new(CancelingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123:1234.5678");
        let handle = block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).unwrap();
        block_on(handle.cancel()).unwrap();
        let cancel_calls = adapter.cancel_calls.lock().unwrap();
        assert_eq!(cancel_calls.len(), 1);
        assert_eq!(cancel_calls[0].0, "C123");
        assert_eq!(cancel_calls[0].1, "Q1");
    }

    #[test]
    fn thread_schedule_should_propagate_errors_from_cancel() {
        // 1:1 with upstream "should propagate errors from cancel".
        let adapter = Arc::new(CancelingAdapter {
            cancel_should_fail_with: Some("already sent"),
            ..Default::default()
        });
        let thread = Thread::new(adapter as Arc<dyn Adapter>, "slack:C123:1234.5678");
        let handle = block_on(thread.schedule("Hello", FUTURE_UNIX_MS)).unwrap();
        let err = block_on(handle.cancel()).expect_err("expected cancel error");
        assert!(
            err.message().contains("already sent"),
            "got: {}",
            err.message()
        );
        assert!(
            !err.is_not_implemented(),
            "Io errors must not coerce into NotImplemented"
        );
    }

    #[test]
    fn thread_schedule_should_cancel_individual_messages_independently() {
        // 1:1 with upstream "should cancel individual messages
        // independently". Two scheduled messages on the same thread;
        // cancelling one only invokes cancel for that specific id.
        let adapter = Arc::new(CancelingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123:1234.5678");
        let s1 = block_on(thread.schedule("First", FUTURE_UNIX_MS)).unwrap();
        let _s2 = block_on(thread.schedule("Second", FUTURE_UNIX_MS)).unwrap();
        block_on(s1.cancel()).unwrap();
        let cancel_calls = adapter.cancel_calls.lock().unwrap();
        assert_eq!(cancel_calls.len(), 1);
        assert_eq!(cancel_calls[0].1, "Q1");
        // Q2 was never cancelled.
        assert!(cancel_calls.iter().all(|(_, id)| id != "Q2"));
    }

    #[test]
    fn thread_schedule_should_pass_the_exact_post_at_unix_ms_to_adapter() {
        // 1:1 with upstream "should pass the exact Date object to
        // adapter". The Rust port uses u64 epoch millis instead of
        // a Date object, so the equivalent assertion is exact
        // unix-ms passthrough.
        const SPECIFIC_UNIX_MS: u64 = 1_861_488_000_000; // 2028-12-25T08:00:00Z
        let adapter = Arc::new(SchedulingAndPostingAdapter::default());
        let thread = Thread::new(adapter.clone() as Arc<dyn Adapter>, "slack:C123:1234.5678");
        block_on(thread.schedule("Merry Christmas!", SPECIFIC_UNIX_MS)).unwrap();
        let calls = adapter.schedule_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].2, SPECIFIC_UNIX_MS);
    }

    // ---------- describe("postEphemeral") (5 upstream cases) ----------
    // 1:1 with upstream `thread.test.ts > describe("postEphemeral")`.
    // The Rust port uses a dedicated `EphemeralAdapter` test mock with
    // boolean flags (`supports_ephemeral`, `supports_open_dm`) to
    // reproduce upstream's `mockAdapter.postEphemeral = undefined` /
    // `mockAdapter.openDM = undefined` mutation pattern.

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
                id: "ephemeral-1".to_string(),
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

    fn ephemeral_thread(adapter: Arc<EphemeralAdapter>) -> Thread {
        Thread::new(adapter as Arc<dyn Adapter>, "slack:C123:1234.5678")
    }

    #[test]
    fn thread_post_ephemeral_should_use_adapter_post_ephemeral_when_available() {
        let adapter = Arc::new(EphemeralAdapter {
            supports_ephemeral: true,
            supports_open_dm: true,
            ..Default::default()
        });
        let thread = ephemeral_thread(adapter.clone());
        let result = block_on(thread.post_ephemeral(
            "U456",
            "Secret message",
            PostEphemeralOptions {
                fallback_to_dm: true,
            },
        ))
        .unwrap()
        .expect("Expected Some(EphemeralMessage)");
        let calls = adapter.post_ephemeral_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            (
                "slack:C123:1234.5678".to_string(),
                "U456".to_string(),
                "Secret message".to_string()
            )
        );
        assert_eq!(result.id, "ephemeral-1");
        assert_eq!(result.thread_id, "slack:C123:1234.5678");
        assert!(!result.used_fallback);
        // open_dm and post_message NOT called when native ephemeral succeeded.
        assert!(adapter.open_dm_calls.lock().unwrap().is_empty());
        assert!(adapter.post_message_calls.lock().unwrap().is_empty());
    }

    #[test]
    fn thread_post_ephemeral_should_extract_user_id_from_author_object() {
        let adapter = Arc::new(EphemeralAdapter {
            supports_ephemeral: true,
            supports_open_dm: true,
            ..Default::default()
        });
        let thread = ephemeral_thread(adapter.clone());
        let author = Author {
            user_id: "U789".to_string(),
            user_name: "testuser".to_string(),
            full_name: "Test User".to_string(),
            is_bot: crate::types::BotStatus::Known(false),
            is_me: false,
        };
        block_on(thread.post_ephemeral_for_author(
            &author,
            "Secret message",
            PostEphemeralOptions {
                fallback_to_dm: true,
            },
        ))
        .unwrap()
        .expect("Expected Some(EphemeralMessage)");
        let calls = adapter.post_ephemeral_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            (
                "slack:C123:1234.5678".to_string(),
                "U789".to_string(),
                "Secret message".to_string()
            )
        );
    }

    #[test]
    fn thread_post_ephemeral_should_fallback_to_dm_when_adapter_has_no_post_ephemeral_and_fallback_to_dm_is_true()
     {
        let adapter = Arc::new(EphemeralAdapter {
            supports_ephemeral: false,
            supports_open_dm: true,
            ..Default::default()
        });
        let thread = ephemeral_thread(adapter.clone());
        let result = block_on(thread.post_ephemeral(
            "U456",
            "Secret message",
            PostEphemeralOptions {
                fallback_to_dm: true,
            },
        ))
        .unwrap()
        .expect("Expected Some(EphemeralMessage) via DM fallback");
        // open_dm called with the user id.
        let dm_calls = adapter.open_dm_calls.lock().unwrap();
        assert_eq!(dm_calls.as_slice(), &["U456".to_string()]);
        // post_message called to the DM thread id.
        let post_calls = adapter.post_message_calls.lock().unwrap();
        assert_eq!(post_calls.len(), 1);
        assert_eq!(
            post_calls[0],
            ("slack:DU456:".to_string(), "Secret message".to_string())
        );
        // Result reflects fallback usage.
        assert_eq!(result.id, "msg-1");
        assert_eq!(result.thread_id, "slack:DU456:");
        assert!(result.used_fallback);
    }

    #[test]
    fn thread_post_ephemeral_should_return_null_when_adapter_has_no_post_ephemeral_and_fallback_to_dm_is_false()
     {
        let adapter = Arc::new(EphemeralAdapter {
            supports_ephemeral: false,
            supports_open_dm: true,
            ..Default::default()
        });
        let thread = ephemeral_thread(adapter.clone());
        let result = block_on(thread.post_ephemeral(
            "U456",
            "Secret message",
            PostEphemeralOptions {
                fallback_to_dm: false,
            },
        ))
        .unwrap();
        assert!(result.is_none());
        // Neither open_dm nor post_message should have been called.
        assert!(adapter.open_dm_calls.lock().unwrap().is_empty());
        assert!(adapter.post_message_calls.lock().unwrap().is_empty());
    }

    #[test]
    fn thread_post_ephemeral_should_return_null_when_adapter_has_no_post_ephemeral_or_open_dm() {
        let adapter = Arc::new(EphemeralAdapter {
            supports_ephemeral: false,
            supports_open_dm: false,
            ..Default::default()
        });
        let thread = ephemeral_thread(adapter.clone());
        let result = block_on(thread.post_ephemeral(
            "U456",
            "Secret message",
            PostEphemeralOptions {
                fallback_to_dm: true,
            },
        ))
        .unwrap();
        assert!(result.is_none());
        // post_message should NOT have been called.
        assert!(adapter.post_message_calls.lock().unwrap().is_empty());
    }
}
