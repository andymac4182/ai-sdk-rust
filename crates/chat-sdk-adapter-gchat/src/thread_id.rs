//! Google Chat thread-id encoding/decoding helpers.
//!
//! 1:1 port of `packages/adapter-gchat/src/thread-utils.ts`. Encodes a
//! Google Chat space + optional thread + DM marker into the canonical
//! `gchat:<spaceName>[:<base64url(threadName)>][:dm]` string used as
//! the chat-sdk thread id throughout the adapter.

use base64::Engine;

/// Components of a decoded Google Chat thread id. 1:1 port of upstream
/// `interface GoogleChatThreadId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct GoogleChatThreadId {
    /// Whether this is a Direct Message space. When `true`, the
    /// encoded form ends with `:dm`.
    pub is_dm: bool,
    /// Google space resource name (e.g. `"spaces/AAA"`).
    pub space_name: String,
    /// Optional thread resource name (e.g. `"spaces/AAA/threads/xyz"`).
    /// Encoded base64url in the wire format.
    pub thread_name: Option<String>,
}

/// Error returned by [`decode_thread_id`] when the input isn't a
/// well-formed Google Chat thread id. 1:1 with upstream's
/// `ValidationError("gchat", ...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidGoogleChatThreadId(pub String);

impl std::fmt::Display for InvalidGoogleChatThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Invalid Google Chat thread ID: {}", self.0)
    }
}

impl std::error::Error for InvalidGoogleChatThreadId {}

const PREFIX: &str = "gchat:";
const DM_SUFFIX: &str = ":dm";

/// Encode platform-specific data into a chat-sdk thread id string.
/// 1:1 port of upstream `encodeThreadId(platformData)`. Format:
/// `gchat:<spaceName>[:<base64url(threadName)>][:dm]`.
pub fn encode_thread_id(data: &GoogleChatThreadId) -> String {
    let mut out = String::from(PREFIX);
    out.push_str(&data.space_name);
    if let Some(name) = data.thread_name.as_deref().filter(|n| !n.is_empty()) {
        out.push(':');
        out.push_str(&base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(name.as_bytes()));
    }
    if data.is_dm {
        out.push_str(DM_SUFFIX);
    }
    out
}

/// Decode a chat-sdk thread id string back into a
/// [`GoogleChatThreadId`]. 1:1 port of upstream
/// `decodeThreadId(threadId)`. Returns `Err` if the id doesn't start
/// with `"gchat:"` or has no space-name portion.
pub fn decode_thread_id(thread_id: &str) -> Result<GoogleChatThreadId, InvalidGoogleChatThreadId> {
    let is_dm = thread_id.ends_with(DM_SUFFIX);
    let clean = if is_dm {
        &thread_id[..thread_id.len() - DM_SUFFIX.len()]
    } else {
        thread_id
    };

    let parts: Vec<&str> = clean.split(':').collect();
    if parts.len() < 2 || parts[0] != "gchat" {
        return Err(InvalidGoogleChatThreadId(thread_id.to_string()));
    }

    let space_name = parts[1].to_string();
    let thread_name = parts.get(2).filter(|s| !s.is_empty()).and_then(|encoded| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded.as_bytes())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    });

    Ok(GoogleChatThreadId {
        is_dm,
        space_name,
        thread_name,
    })
}

/// Check whether `thread_id` encodes a Direct Message conversation.
/// 1:1 port of upstream `isDMThread(threadId)`: just checks for the
/// `:dm` suffix.
pub fn is_dm_thread(thread_id: &str) -> bool {
    thread_id.ends_with(DM_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- encodeThreadId (4 upstream cases) ----------

    #[test]
    fn should_encode_space_name_only() {
        let id = encode_thread_id(&GoogleChatThreadId {
            space_name: "spaces/ABC123".to_string(),
            ..Default::default()
        });
        assert_eq!(id, "gchat:spaces/ABC123");
    }

    #[test]
    fn should_encode_space_name_with_thread_name() {
        let id = encode_thread_id(&GoogleChatThreadId {
            space_name: "spaces/ABC123".to_string(),
            thread_name: Some("spaces/ABC123/threads/xyz".to_string()),
            is_dm: false,
        });
        assert!(id.starts_with("gchat:spaces/ABC123:"), "got: {id}");
        let parts: Vec<&str> = id.split(':').collect();
        assert!(parts.len() >= 3, "got: {id}");
    }

    #[test]
    fn should_add_dm_suffix_for_dm_threads() {
        let id = encode_thread_id(&GoogleChatThreadId {
            space_name: "spaces/DM123".to_string(),
            is_dm: true,
            ..Default::default()
        });
        assert_eq!(id, "gchat:spaces/DM123:dm");
    }

    #[test]
    fn should_add_dm_suffix_with_thread_name() {
        let id = encode_thread_id(&GoogleChatThreadId {
            space_name: "spaces/DM123".to_string(),
            thread_name: Some("spaces/DM123/threads/t1".to_string()),
            is_dm: true,
        });
        assert!(id.ends_with(":dm"), "got: {id}");
    }

    // ---------- decodeThreadId (4 upstream cases) ----------

    #[test]
    fn should_decode_space_only_thread_id() {
        let r = decode_thread_id("gchat:spaces/ABC123").unwrap();
        assert_eq!(r.space_name, "spaces/ABC123");
        assert!(r.thread_name.is_none());
        assert!(!r.is_dm);
    }

    #[test]
    fn should_decode_dm_thread_id() {
        let r = decode_thread_id("gchat:spaces/DM123:dm").unwrap();
        assert_eq!(r.space_name, "spaces/DM123");
        assert!(r.is_dm);
    }

    #[test]
    fn should_err_on_invalid_format() {
        let err = decode_thread_id("invalid").unwrap_err();
        assert!(
            err.to_string().contains("Invalid Google Chat thread ID"),
            "got: {err}"
        );
    }

    #[test]
    fn should_err_on_wrong_prefix() {
        let err = decode_thread_id("slack:C123:1234").unwrap_err();
        assert!(
            err.to_string().contains("Invalid Google Chat thread ID"),
            "got: {err}"
        );
    }

    // ---------- round-trip (3 upstream cases) ----------

    #[test]
    fn round_trip_space_only() {
        let original = GoogleChatThreadId {
            space_name: "spaces/ABC".to_string(),
            ..Default::default()
        };
        let encoded = encode_thread_id(&original);
        let decoded = decode_thread_id(&encoded).unwrap();
        assert_eq!(decoded.space_name, original.space_name);
    }

    #[test]
    fn round_trip_with_thread_name() {
        let original = GoogleChatThreadId {
            space_name: "spaces/ABC".to_string(),
            thread_name: Some("spaces/ABC/threads/xyz".to_string()),
            is_dm: false,
        };
        let encoded = encode_thread_id(&original);
        let decoded = decode_thread_id(&encoded).unwrap();
        assert_eq!(decoded.space_name, original.space_name);
        assert_eq!(decoded.thread_name, original.thread_name);
    }

    #[test]
    fn round_trip_dm() {
        let original = GoogleChatThreadId {
            space_name: "spaces/DM1".to_string(),
            is_dm: true,
            ..Default::default()
        };
        let encoded = encode_thread_id(&original);
        let decoded = decode_thread_id(&encoded).unwrap();
        assert_eq!(decoded.space_name, original.space_name);
        assert!(decoded.is_dm);
    }

    // ---------- isDMThread (3 upstream cases) ----------

    #[test]
    fn is_dm_thread_returns_true_for_dm_thread_ids() {
        assert!(is_dm_thread("gchat:spaces/DM123:dm"));
    }

    #[test]
    fn is_dm_thread_returns_false_for_non_dm_thread_ids() {
        assert!(!is_dm_thread("gchat:spaces/ABC123"));
    }

    #[test]
    fn is_dm_thread_returns_false_for_thread_ids_with_dm_in_middle() {
        assert!(!is_dm_thread("gchat:dm:spaces/ABC"));
    }
}
