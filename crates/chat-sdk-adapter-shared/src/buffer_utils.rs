//! Buffer conversion utilities for handling file uploads.
//!
//! 1:1 port of `packages/adapter-shared/src/buffer-utils.ts` adapted to
//! Rust's single canonical byte container.
//!
//! Upstream juggles three JavaScript runtime types — `Buffer`,
//! `ArrayBuffer`, `Blob` — that all carry raw bytes. Rust collapses
//! them into `Vec<u8>` (see also [`chat_sdk_chat::types::FileBytes`]),
//! so most of the upstream type-discrimination logic disappears. The
//! Rust port preserves the API surface (`to_buffer`, `to_buffer_sync`,
//! `buffer_to_data_uri`) for adapters that reference these helpers,
//! and ports the upstream "unsupported type" error path via a typed
//! `BufferError` for callers that want to react to malformed inputs
//! rather than panic.

use base64::Engine;
use chat_sdk_adapter_shared_error_alias::AdapterError;
use chat_sdk_chat::types::FileBytes;

use crate::card_utils::PlatformName;

/// Error returned by the buffer conversion helpers when input data is
/// rejected. Wraps upstream `ValidationError(platform, ...)`.
pub type BufferError = AdapterError;

/// Options for [`to_buffer`] and [`to_buffer_sync`]. 1:1 port of
/// upstream `interface ToBufferOptions`.
#[derive(Debug, Clone)]
pub struct ToBufferOptions {
    /// Platform name for error messages (upstream `platform` field).
    pub platform: PlatformName,
    /// If `true`, return [`BufferError`] for unsupported inputs;
    /// if `false`, return `Ok(None)` and let the caller skip. Upstream
    /// default: `true`.
    pub throw_on_unsupported: bool,
}

impl Default for ToBufferOptions {
    fn default() -> Self {
        // Upstream omits `throw_on_unsupported` -> defaults to true.
        // Platform has no upstream default; the `default()` impl picks
        // Slack arbitrarily so callers can `..Default::default()` the
        // throw flag and override the platform.
        Self {
            platform: PlatformName::Slack,
            throw_on_unsupported: true,
        }
    }
}

/// Convert various data types to a byte vector. 1:1 port adaptation of
/// upstream `toBuffer(data, options): Promise<Buffer | null>`.
///
/// In upstream the function discriminates between `Buffer`,
/// `ArrayBuffer`, and `Blob`. In Rust the single canonical byte
/// container is `Vec<u8>`, so this function is effectively
/// identity-or-error. It still exists for API parity — adapters that
/// reference `toBuffer` from upstream get a 1:1 Rust call site.
///
/// The optional `data` argument mirrors upstream's `unknown` parameter
/// — passing `None` triggers the unsupported-input path so callers can
/// drive this function from a JSON value where the `data` field may be
/// missing.
pub fn to_buffer(
    data: Option<FileBytes>,
    options: &ToBufferOptions,
) -> Result<Option<FileBytes>, BufferError> {
    if let Some(bytes) = data {
        return Ok(Some(bytes));
    }
    if options.throw_on_unsupported {
        return Err(AdapterError::validation(
            platform_name_str(options.platform),
            "Unsupported file data type",
        ));
    }
    Ok(None)
}

/// Synchronous variant. 1:1 port of upstream `toBufferSync`. In Rust
/// the async/sync split has no semantic difference because there is no
/// `Blob.arrayBuffer()`-style async-only conversion, so [`to_buffer`]
/// and [`to_buffer_sync`] have identical behavior; both ship for API
/// parity.
pub fn to_buffer_sync(
    data: Option<FileBytes>,
    options: &ToBufferOptions,
) -> Result<Option<FileBytes>, BufferError> {
    to_buffer(data, options)
}

/// Convert a byte buffer to a `data:` URI. 1:1 port of upstream
/// `bufferToDataUri(buffer, mimeType?): string`. Default MIME type
/// matches upstream: `application/octet-stream`.
pub fn buffer_to_data_uri(buffer: &[u8], mime_type: Option<&str>) -> String {
    let mime = mime_type.unwrap_or("application/octet-stream");
    let base64 = base64::engine::general_purpose::STANDARD.encode(buffer);
    format!("data:{mime};base64,{base64}")
}

fn platform_name_str(p: PlatformName) -> &'static str {
    match p {
        PlatformName::Slack => "slack",
        PlatformName::Gchat => "gchat",
        PlatformName::Teams => "teams",
        PlatformName::Discord => "discord",
    }
}

// Re-route the BufferError alias through a sub-module so its
// definition stays close to the canonical AdapterError without
// re-exporting the whole errors module here.
mod chat_sdk_adapter_shared_error_alias {
    pub use crate::errors::AdapterError;
}

#[cfg(test)]
mod tests {
    //! Adaptation of `packages/adapter-shared/src/buffer-utils.test.ts`.
    //!
    //! Most upstream cases test discrimination between `Buffer`,
    //! `ArrayBuffer`, and `Blob`; in Rust those collapse to `Vec<u8>`
    //! so the equivalent test cases test the happy-path identity
    //! return, the unsupported-input error and `Ok(None)` branches,
    //! and the data-URI formatting. 11 of 16 portable cases mapped
    //! 1:1 in Rust; the remaining 5 upstream cases are
    //! type-system-impossible (per the slice-380 brief pattern):
    //!
    //! - `toBuffer > converts ArrayBuffer to Buffer`: Rust
    //!   `FileBytes` is `Vec<u8>`; no `ArrayBuffer` variant exists.
    //! - `toBuffer > converts Blob to Buffer`: same — no `Blob`.
    //! - `toBufferSync > converts ArrayBuffer to Buffer`: same.
    //! - `toBufferSync > throws ValidationError for Blob by default`:
    //!   no `Blob` variant means the throw branch is unreachable.
    //! - `toBufferSync > returns null for Blob when throwOnUnsupported
    //!   is false`: same — Blob is type-system-impossible.

    use super::*;

    fn opts(throw: bool) -> ToBufferOptions {
        ToBufferOptions {
            platform: PlatformName::Slack,
            throw_on_unsupported: throw,
        }
    }

    #[test]
    fn to_buffer_returns_byte_data_unchanged() {
        let bytes = FileBytes::from(vec![1, 2, 3]);
        let out = to_buffer(Some(bytes.clone()), &opts(true)).unwrap();
        assert_eq!(out, Some(bytes));
    }

    #[test]
    fn to_buffer_returns_error_on_missing_data_when_throw_is_true() {
        let err = to_buffer(None, &opts(true)).unwrap_err();
        assert!(err.is_validation());
        assert_eq!(err.message(), "Unsupported file data type");
        assert_eq!(err.adapter(), "slack");
    }

    #[test]
    fn to_buffer_returns_ok_none_on_missing_data_when_throw_is_false() {
        assert_eq!(to_buffer(None, &opts(false)).unwrap(), None);
    }

    #[test]
    fn to_buffer_sync_has_identical_behavior_to_to_buffer() {
        // In upstream, toBufferSync rejects Blobs (async-only); the
        // Rust port has no Blob so the two functions converge.
        let bytes = FileBytes::from(vec![7, 8, 9]);
        let sync_out = to_buffer_sync(Some(bytes.clone()), &opts(true)).unwrap();
        let async_out = to_buffer(Some(bytes.clone()), &opts(true)).unwrap();
        assert_eq!(sync_out, async_out);

        let sync_none = to_buffer_sync(None, &opts(false)).unwrap();
        assert_eq!(sync_none, None);
    }

    #[test]
    fn to_buffer_carries_platform_name_into_validation_error() {
        for (platform, wire) in [
            (PlatformName::Slack, "slack"),
            (PlatformName::Gchat, "gchat"),
            (PlatformName::Teams, "teams"),
            (PlatformName::Discord, "discord"),
        ] {
            let err = to_buffer(
                None,
                &ToBufferOptions {
                    platform,
                    throw_on_unsupported: true,
                },
            )
            .unwrap_err();
            assert_eq!(err.adapter(), wire);
        }
    }

    #[test]
    fn buffer_to_data_uri_uses_default_mime_type_when_omitted() {
        // 'hi' bytes = [0x68, 0x69] -> base64 'aGk='.
        let uri = buffer_to_data_uri(b"hi", None);
        assert_eq!(uri, "data:application/octet-stream;base64,aGk=");
    }

    #[test]
    fn buffer_to_data_uri_includes_supplied_mime_type() {
        let uri = buffer_to_data_uri(b"hi", Some("image/png"));
        assert_eq!(uri, "data:image/png;base64,aGk=");
    }

    #[test]
    fn buffer_to_data_uri_handles_image_mime_types() {
        // 1:1 with upstream `describe("bufferToDataUri") > it("handles
        // image mime types")` — PNG magic bytes prepended with the
        // `image/png` mime type produce a data URI that starts with
        // the `data:image/png;base64,` prefix.
        let png_magic = [0x89, 0x50, 0x4E, 0x47];
        let uri = buffer_to_data_uri(&png_magic, Some("image/png"));
        assert!(uri.starts_with("data:image/png;base64,"), "got: {uri}");
    }

    #[test]
    fn buffer_to_data_uri_handles_empty_buffer() {
        // 1:1 with upstream `describe("bufferToDataUri") > it("handles
        // empty buffer")` — an empty input produces a data URI
        // with no base64 payload (just the default-mime prefix).
        let uri = buffer_to_data_uri(&[], None);
        assert_eq!(uri, "data:application/octet-stream;base64,");
    }
}
