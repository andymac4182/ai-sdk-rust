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

/// Build the wire envelope upstream's `PostableObject.toJSON()`
/// produces. 1:1 with the upstream object literal:
///
/// ```text
/// { $$typeof: Symbol.for("chat.postable"), kind, data, fallbackText }
/// ```
///
/// `$$typeof` is lowered from a JS Symbol to the matching string
/// discriminator on the wire (see [`POSTABLE_OBJECT_DISCRIMINATOR`]).
pub fn postable_envelope(
    kind: &str,
    data: serde_json::Value,
    fallback_text: &str,
) -> serde_json::Value {
    serde_json::json!({
        "$$typeof": POSTABLE_OBJECT_DISCRIMINATOR,
        "kind": kind,
        "data": data,
        "fallbackText": fallback_text,
    })
}

/// Read the `kind` discriminator off a postable envelope. 1:1 with
/// upstream's `value.kind` access on the deserialized envelope used
/// by `adapter.postObject` to route the object.
///
/// Returns `None` for any value that isn't a postable envelope or
/// that's missing the `kind` field.
pub fn postable_envelope_kind(value: &serde_json::Value) -> Option<&str> {
    if !is_postable_object(value) {
        return None;
    }
    value.get("kind").and_then(serde_json::Value::as_str)
}

/// Read the `data` payload off a postable envelope. 1:1 with
/// upstream's `value.data` access used by `adapter.postObject` to
/// hand off the typed payload to platform-specific renderers.
///
/// Returns `None` for any value that isn't a postable envelope.
pub fn postable_envelope_data(value: &serde_json::Value) -> Option<&serde_json::Value> {
    if !is_postable_object(value) {
        return None;
    }
    value.get("data")
}

/// Read the `fallbackText` field off a postable envelope. 1:1 with
/// upstream's `value.fallbackText` access, used by adapters that
/// don't natively support the envelope's `kind` and fall back to a
/// plain-text post.
///
/// Returns `None` for any value that isn't a postable envelope or
/// is missing the field.
pub fn postable_envelope_fallback_text(value: &serde_json::Value) -> Option<&str> {
    if !is_postable_object(value) {
        return None;
    }
    value
        .get("fallbackText")
        .and_then(serde_json::Value::as_str)
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

    // ---------- slice 109: envelope builder + accessors ----------

    #[test]
    fn postable_envelope_builds_the_upstream_to_json_shape() {
        let envelope = postable_envelope("plan", json!({"title": "T"}), "Plan: T");
        assert!(is_postable_object(&envelope));
        assert_eq!(envelope.get("kind").and_then(|v| v.as_str()), Some("plan"));
        assert_eq!(envelope.get("data"), Some(&json!({"title": "T"})));
        assert_eq!(
            envelope.get("fallbackText").and_then(|v| v.as_str()),
            Some("Plan: T")
        );
    }

    #[test]
    fn postable_envelope_kind_reads_the_kind_field() {
        let envelope = postable_envelope("poll", json!({}), "");
        assert_eq!(postable_envelope_kind(&envelope), Some("poll"));
    }

    #[test]
    fn postable_envelope_kind_rejects_non_envelopes() {
        assert!(postable_envelope_kind(&json!({"kind": "plan"})).is_none());
        assert!(postable_envelope_kind(&json!(null)).is_none());
        assert!(postable_envelope_kind(&json!("string")).is_none());
    }

    #[test]
    fn postable_envelope_data_returns_the_payload_for_a_valid_envelope() {
        let data = json!({"items": [1, 2, 3]});
        let envelope = postable_envelope("kind", data.clone(), "fb");
        assert_eq!(postable_envelope_data(&envelope), Some(&data));
    }

    #[test]
    fn postable_envelope_data_returns_none_for_non_envelopes() {
        assert!(postable_envelope_data(&json!({"data": {"x": 1}})).is_none());
        assert!(postable_envelope_data(&json!([1, 2, 3])).is_none());
    }

    #[test]
    fn postable_envelope_fallback_text_reads_the_fallback_field() {
        let envelope = postable_envelope("plan", json!({}), "Plan: Title");
        assert_eq!(
            postable_envelope_fallback_text(&envelope),
            Some("Plan: Title")
        );
    }

    #[test]
    fn postable_envelope_round_trips_through_serde_json() {
        let envelope = postable_envelope("plan", json!({"title": "T"}), "Plan: T");
        let text = serde_json::to_string(&envelope).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(is_postable_object(&parsed));
        assert_eq!(postable_envelope_kind(&parsed), Some("plan"));
    }
}
