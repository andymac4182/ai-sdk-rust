//! Upstream-shape Teams thread-id codec.
//!
//! 1:1 port of `packages/adapter-teams/src/thread-id.ts`:
//!
//! - [`TeamsThreadId`] - struct with the two upstream fields
//!   (`conversation_id`, `service_url`).
//! - [`encode_thread_id`] - `teams:<base64url(conv)>:<base64url(url)>`.
//! - [`decode_thread_id`] - parse + base64url-decode, requiring
//!   exactly three colon-separated segments with `teams` as the
//!   first.
//! - [`is_dm_thread`] - true iff `conversation_id` doesn't start
//!   with `19:` (Teams convention: group/channel ids start with
//!   `19:`, DM conversations don't).
//!
//! This new struct-based API coexists with the simpler
//! `encode_thread_id(conversation_id, message_id)` form in `lib.rs`,
//! which is the form the adapter's HTTP code currently uses.
//! Migration is deferred (see `goal-refinements.md`).

use base64::Engine;
use chat_sdk_adapter_shared::errors::AdapterError;

const THREAD_ID_PREFIX: &str = "teams:";

/// Decoded Teams thread-id components. 1:1 with upstream
/// `interface TeamsThreadId { conversationId; serviceUrl }`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TeamsThreadId {
    /// Bot Framework conversation id (e.g.
    /// `19:abc@thread.tacv2` or `19:abc@thread.tacv2;messageid=…`
    /// for channel threads, or `a]8:orgid:user-id-here` for DMs).
    pub conversation_id: String,
    /// Bot Framework service URL (regional Microsoft Teams endpoint).
    pub service_url: String,
}

/// Encode a [`TeamsThreadId`] as `teams:<b64(conv)>:<b64(svc)>`.
/// 1:1 port of upstream `encodeThreadId(platformData)` which
/// base64url-encodes each field.
pub fn encode_thread_id(data: &TeamsThreadId) -> String {
    let conv_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&data.conversation_id);
    let svc_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&data.service_url);
    format!("{THREAD_ID_PREFIX}{conv_b64}:{svc_b64}")
}

/// Decode a `teams:<b64(conv)>:<b64(svc)>` thread id back into a
/// [`TeamsThreadId`]. 1:1 port of upstream `decodeThreadId(threadId)`
/// — requires exactly three `":"`-separated parts (`teams`, conv,
/// svc), returns `AdapterError::Validation("teams", ...)` for any
/// other shape or for non-UTF-8 base64 contents.
pub fn decode_thread_id(thread_id: &str) -> Result<TeamsThreadId, AdapterError> {
    let parts: Vec<&str> = thread_id.split(':').collect();
    if parts.len() != 3 || parts[0] != "teams" {
        return Err(AdapterError::validation(
            "teams",
            format!("Invalid Teams thread ID: {thread_id}"),
        ));
    }
    let conv = decode_b64_utf8(parts[1])
        .ok_or_else(|| AdapterError::validation("teams", "Invalid Teams thread ID: conversation"))?;
    let svc = decode_b64_utf8(parts[2])
        .ok_or_else(|| AdapterError::validation("teams", "Invalid Teams thread ID: serviceUrl"))?;
    Ok(TeamsThreadId {
        conversation_id: conv,
        service_url: svc,
    })
}

/// Check whether `thread_id` encodes a Direct Message conversation.
/// 1:1 port of upstream `isDM(threadId)`: decodes the thread, then
/// tests `!conversation_id.starts_with("19:")` (Teams convention:
/// group chats and channel threads use the `19:` prefix; DMs don't).
/// Returns `false` for thread ids that fail to decode.
pub fn is_dm_thread(thread_id: &str) -> bool {
    match decode_thread_id(thread_id) {
        Ok(decoded) => !decoded.conversation_id.starts_with("19:"),
        Err(_) => false,
    }
}

fn decode_b64_utf8(encoded: &str) -> Option<String> {
    // Accept both URL-safe (no-pad and padded) and standard b64,
    // mirroring Node `Buffer.from(_, "base64url")` permissiveness.
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(encoded))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(encoded))
        .ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- 6 ported upstream cases ----------
    // Upstream tests live in `index.test.ts` under
    // `describe("Thread ID Encoding")` (3 cases) and
    // `describe("isDM")` (3 cases).

    #[test]
    fn encodes_and_decodes_thread_ids() {
        let original = TeamsThreadId {
            conversation_id: "19:abc123@thread.tacv2".to_string(),
            service_url: "https://smba.trafficmanager.net/teams/".to_string(),
        };
        let encoded = encode_thread_id(&original);
        assert!(encoded.starts_with("teams:"));
        let decoded = decode_thread_id(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn preserves_messageid_in_thread_context_for_channel_threads() {
        let original = TeamsThreadId {
            conversation_id:
                "19:d441d38c655c47a085215b2726e76927@thread.tacv2;messageid=1767297849909"
                    .to_string(),
            service_url: "https://smba.trafficmanager.net/amer/".to_string(),
        };
        let encoded = encode_thread_id(&original);
        let decoded = decode_thread_id(&encoded).unwrap();
        assert_eq!(decoded.conversation_id, original.conversation_id);
        assert!(decoded.conversation_id.contains(";messageid="));
    }

    #[test]
    fn throws_validation_error_for_invalid_thread_ids() {
        for bad in ["invalid", "slack:abc:def", "teams"] {
            let err = decode_thread_id(bad).unwrap_err();
            assert!(err.is_validation(), "expected validation error for {bad:?}, got {err}");
        }
    }

    #[test]
    fn handles_special_characters_in_conversation_id_and_service_url() {
        let original = TeamsThreadId {
            conversation_id: "19:meeting_MDE4OWI4N2UtNzEzNC00ZGE2LTkxMGEtNDM3@thread.v2".to_string(),
            service_url: "https://smba.trafficmanager.net/amer/?special=chars&foo=bar"
                .to_string(),
        };
        let encoded = encode_thread_id(&original);
        let decoded = decode_thread_id(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn is_dm_returns_false_for_group_chats_with_19_prefix() {
        let thread_id = encode_thread_id(&TeamsThreadId {
            conversation_id: "19:abc@thread.tacv2".to_string(),
            service_url: "https://smba.trafficmanager.net/teams/".to_string(),
        });
        assert!(!is_dm_thread(&thread_id));
    }

    #[test]
    fn is_dm_returns_true_for_dm_conversations() {
        let thread_id = encode_thread_id(&TeamsThreadId {
            conversation_id: "a]8:orgid:user-id-here".to_string(),
            service_url: "https://smba.trafficmanager.net/teams/".to_string(),
        });
        assert!(is_dm_thread(&thread_id));
    }

    #[test]
    fn is_dm_returns_false_for_channel_threads_with_messageid() {
        let thread_id = encode_thread_id(&TeamsThreadId {
            conversation_id: "19:abc@thread.tacv2;messageid=1767297849909".to_string(),
            service_url: "https://smba.trafficmanager.net/teams/".to_string(),
        });
        assert!(!is_dm_thread(&thread_id));
    }
}
