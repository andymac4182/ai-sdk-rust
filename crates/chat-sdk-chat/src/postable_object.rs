//! Trait and type guards for objects that can be posted to threads /
//! channels.
//!
//! 1:1 port (in progress) of `packages/chat/src/postable-object.ts`.
//!
//! Upstream uses a `Symbol.for("chat.postable")` discriminator and a
//! `$$typeof` field for runtime identification. The Rust port models
//! this as:
//!
//! - [`POSTABLE_OBJECT_DISCRIMINATOR`] — the string `"chat.postable"`
//!   that appears on the wire as the `$$typeof` field's value (the
//!   Rust port replaces JS Symbols with stable string literals on
//!   serialized payloads).
//! - [`PostableObject`] — the runtime trait that implementors satisfy.
//!   Three methods (`fallback_text`, `kind`, `post_data`) are pure and
//!   ship now; `is_supported` / `on_posted` reference the placeholder
//!   [`crate::types::Adapter`] trait so they ship with the same
//!   placeholder shape and will be filled in once the trait is
//!   extended.
//! - [`is_postable_object`] — shape guard that walks a
//!   [`serde_json::Value`] for the upstream `$$typeof` discriminator.
//!
//! **What is deferred:** [`post_postable_object`] (the dispatch helper
//! that calls into adapter.postObject or falls back to text) requires
//! the `Adapter` trait to carry concrete async `post_object` and
//! `post_message` methods. It lands when those methods are added.

use crate::types::Adapter;

/// String discriminator placed on the wire for objects that are
/// postable. 1:1 with upstream `Symbol.for("chat.postable")`'s
/// description string — JS symbols can't be serialized directly, so
/// the upstream `toJSON` path also lowers the symbol to a string when
/// crossing a network boundary.
pub const POSTABLE_OBJECT_DISCRIMINATOR: &str = "chat.postable";

/// Context provided to a [`PostableObject`] after it has been posted.
/// 1:1 port of upstream `interface PostableObjectContext`.
#[derive(Clone)]
pub struct PostableObjectContext {
    /// The adapter that delivered the message.
    pub adapter: std::sync::Arc<dyn Adapter>,
    /// Thread / channel logger, when one was wired up at post time.
    pub logger: Option<std::sync::Arc<dyn crate::logger::Logger>>,
    /// Platform-side message id assigned to the posted object.
    pub message_id: String,
    /// Thread the object was posted to.
    pub thread_id: String,
}

impl std::fmt::Debug for PostableObjectContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostableObjectContext")
            .field("adapter", &self.adapter)
            .field("logger", &self.logger.as_ref().map(|_| "<Logger>"))
            .field("message_id", &self.message_id)
            .field("thread_id", &self.thread_id)
            .finish()
    }
}

/// Trait implemented by objects that can be posted to threads /
/// channels. 1:1 port of upstream `interface PostableObject<TData>`.
///
/// The associated [`Self::Data`] type mirrors upstream's
/// `TData = unknown` generic; each implementor decides what shape
/// their `post_data()` returns.
pub trait PostableObject: Send + Sync + std::fmt::Debug {
    /// Output of [`Self::post_data`].
    type Data;

    /// Fallback text used by adapters that don't support this kind.
    /// 1:1 with upstream `getFallbackText(): string`.
    fn fallback_text(&self) -> String;

    /// Raw data passed to `adapter.post_object` when supported. 1:1
    /// with upstream `getPostData(): TData`.
    fn post_data(&self) -> Self::Data;

    /// Dispatcher kind used by adapters to route the object. 1:1 with
    /// upstream `readonly kind: string`.
    fn kind(&self) -> &str;

    /// Per-adapter support check. 1:1 with upstream
    /// `isSupported(adapter: Adapter): boolean`. Default is `true`,
    /// matching the upstream class behavior where overridable returns
    /// default true unless the adapter explicitly lacks the kind.
    fn is_supported(&self, _adapter: &dyn Adapter) -> bool {
        true
    }

    /// Lifecycle hook called after a successful post. Default is a
    /// no-op so simple value-only objects (like `StreamingPlan`) don't
    /// need to override it.
    fn on_posted(&self, _context: PostableObjectContext) {}
}

/// Shape guard: returns `true` when `value` is a JSON object whose
/// `"$$typeof"` field equals [`POSTABLE_OBJECT_DISCRIMINATOR`]. 1:1
/// port of upstream `isPostableObject(value): value is PostableObject`.
///
/// The upstream check uses JavaScript object identity on a `Symbol`;
/// the Rust port checks the lowered string discriminator that the
/// wire-format uses. Both behave identically across a JSON boundary.
pub fn is_postable_object(value: &serde_json::Value) -> bool {
    value
        .get("$$typeof")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s == POSTABLE_OBJECT_DISCRIMINATOR)
}

#[cfg(test)]
mod tests {
    //! Additive coverage. Upstream ships no `postable-object.test.ts`;
    //! the canonical implementors (Plan, StreamingPlan, Poll) are
    //! covered by their own test files. These Rust tests lock in the
    //! shape guard and the trait surface.
    use super::*;
    use serde_json::json;

    #[test]
    fn is_postable_object_accepts_objects_with_the_upstream_discriminator() {
        assert!(is_postable_object(&json!({"$$typeof": "chat.postable"})));
    }

    #[test]
    fn is_postable_object_rejects_objects_with_a_different_discriminator() {
        assert!(!is_postable_object(&json!({"$$typeof": "other"})));
        assert!(!is_postable_object(&json!({})));
        assert!(!is_postable_object(&json!({"$$typeof": 42})));
    }

    #[test]
    fn is_postable_object_rejects_non_objects() {
        assert!(!is_postable_object(&json!(null)));
        assert!(!is_postable_object(&json!("string")));
        assert!(!is_postable_object(&json!(42)));
        assert!(!is_postable_object(&json!([1, 2, 3])));
    }

    #[test]
    fn discriminator_matches_upstream_symbol_description() {
        assert_eq!(POSTABLE_OBJECT_DISCRIMINATOR, "chat.postable");
    }
}
