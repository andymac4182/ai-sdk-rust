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
use crate::types::{Adapter, AdapterResult};

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
    pub fn new(adapter: Arc<dyn Adapter>, channel_id: impl Into<String>) -> Self {
        Self {
            adapter,
            channel_id: channel_id.into(),
        }
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
