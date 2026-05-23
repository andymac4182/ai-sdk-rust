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
use crate::types::{Adapter, AdapterResult};

/// Cross-platform thread handle. 1:1 port (in progress) of upstream
/// `class Thread`.
#[derive(Clone)]
pub struct Thread {
    adapter: Arc<dyn Adapter>,
    thread_id: String,
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
    pub fn new(adapter: Arc<dyn Adapter>, thread_id: impl Into<String>) -> Self {
        Self {
            adapter,
            thread_id: thread_id.into(),
        }
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
}
