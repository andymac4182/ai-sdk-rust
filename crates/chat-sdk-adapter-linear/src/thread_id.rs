//! Linear thread-id encoding/decoding utilities.
//!
//! 1:1 port of the `encodeThreadId` / `decodeThreadId` methods in
//! `packages/adapter-linear/src/index.ts`. Encodes a Linear issue
//! plus optional comment and agent-session into the canonical
//! `linear:<issueId>[:c:<commentId>][:s:<agentSessionId>]` string
//! used as the chat-sdk thread id by the Linear adapter.

/// Components of a Linear thread id. 1:1 port of upstream
/// `interface LinearThreadId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct LinearThreadId {
    pub issue_id: String,
    pub comment_id: Option<String>,
    pub agent_session_id: Option<String>,
}

/// Error returned by [`decode_thread_id`] when the input isn't a
/// well-formed Linear thread id. 1:1 with upstream's
/// `ValidationError("linear", ...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidLinearThreadId {
    /// Doesn't start with `"linear:"` prefix.
    BadPrefix(String),
    /// Starts with `"linear:"` but the trailing portion is empty.
    EmptyIssueId(String),
}

impl std::fmt::Display for InvalidLinearThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadPrefix(s) => write!(f, "Invalid Linear thread ID: {s}"),
            Self::EmptyIssueId(s) => write!(f, "Invalid Linear thread ID format: {s}"),
        }
    }
}

impl std::error::Error for InvalidLinearThreadId {}

const PREFIX: &str = "linear:";

/// Encode platform-specific data into a Linear thread id string.
/// 1:1 port of upstream `encodeThreadId(platformData)`. Formats:
/// - `linear:<issueId>`
/// - `linear:<issueId>:c:<commentId>`
/// - `linear:<issueId>:s:<agentSessionId>`
/// - `linear:<issueId>:c:<commentId>:s:<agentSessionId>`
pub fn encode_thread_id(data: &LinearThreadId) -> String {
    if let Some(session) = data.agent_session_id.as_deref() {
        if let Some(comment) = data.comment_id.as_deref() {
            return format!("linear:{}:c:{}:s:{}", data.issue_id, comment, session);
        }
        return format!("linear:{}:s:{}", data.issue_id, session);
    }
    if let Some(comment) = data.comment_id.as_deref() {
        return format!("linear:{}:c:{}", data.issue_id, comment);
    }
    format!("linear:{}", data.issue_id)
}

/// Decode a Linear thread id string back into a
/// [`LinearThreadId`]. 1:1 port of upstream `decodeThreadId`.
pub fn decode_thread_id(thread_id: &str) -> Result<LinearThreadId, InvalidLinearThreadId> {
    let Some(rest) = thread_id.strip_prefix(PREFIX) else {
        return Err(InvalidLinearThreadId::BadPrefix(thread_id.to_string()));
    };
    if rest.is_empty() {
        return Err(InvalidLinearThreadId::EmptyIssueId(thread_id.to_string()));
    }

    // Try `<issueId>:c:<commentId>:s:<agentSessionId>`.
    if let Some(parts) = match_three_parts(rest, ":c:", ":s:") {
        return Ok(LinearThreadId {
            issue_id: parts.0,
            comment_id: Some(parts.1),
            agent_session_id: Some(parts.2),
        });
    }
    // Try `<issueId>:s:<agentSessionId>`.
    if let Some(parts) = match_two_parts(rest, ":s:") {
        return Ok(LinearThreadId {
            issue_id: parts.0,
            comment_id: None,
            agent_session_id: Some(parts.1),
        });
    }
    // Try `<issueId>:c:<commentId>`.
    if let Some(parts) = match_two_parts(rest, ":c:") {
        return Ok(LinearThreadId {
            issue_id: parts.0,
            comment_id: Some(parts.1),
            agent_session_id: None,
        });
    }
    // Plain `<issueId>`.
    Ok(LinearThreadId {
        issue_id: rest.to_string(),
        comment_id: None,
        agent_session_id: None,
    })
}

/// Match upstream `/^([^:]+)<sep>([^:]+)$/` shape: split on `sep`
/// once, return `(prefix, suffix)` if neither half contains `:`.
fn match_two_parts(input: &str, sep: &str) -> Option<(String, String)> {
    let idx = input.find(sep)?;
    let prefix = &input[..idx];
    let suffix = &input[idx + sep.len()..];
    if prefix.is_empty() || suffix.is_empty() {
        return None;
    }
    if prefix.contains(':') || suffix.contains(':') {
        return None;
    }
    Some((prefix.to_string(), suffix.to_string()))
}

/// Match upstream `/^([^:]+)<sep1>([^:]+)<sep2>([^:]+)$/` shape:
/// returns `(part1, part2, part3)` when all three are non-empty
/// and colon-free.
fn match_three_parts(input: &str, sep1: &str, sep2: &str) -> Option<(String, String, String)> {
    let idx1 = input.find(sep1)?;
    let first = &input[..idx1];
    let after_first = &input[idx1 + sep1.len()..];
    let idx2 = after_first.find(sep2)?;
    let second = &after_first[..idx2];
    let third = &after_first[idx2 + sep2.len()..];
    if first.is_empty() || second.is_empty() || third.is_empty() {
        return None;
    }
    if first.contains(':') || second.contains(':') || third.contains(':') {
        return None;
    }
    Some((first.to_string(), second.to_string(), third.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue_only(id: &str) -> LinearThreadId {
        LinearThreadId {
            issue_id: id.to_string(),
            ..Default::default()
        }
    }

    // ---------- encodeThreadId (6 upstream cases) ----------

    #[test]
    fn encode_issue_level_thread_id() {
        let result = encode_thread_id(&issue_only("abc123-def456-789"));
        assert_eq!(result, "linear:abc123-def456-789");
    }

    #[test]
    fn encode_uuid_issue_level_thread_id() {
        let result = encode_thread_id(&issue_only("2174add1-f7c8-44e3-bbf3-2d60b5ea8bc9"));
        assert_eq!(result, "linear:2174add1-f7c8-44e3-bbf3-2d60b5ea8bc9");
    }

    #[test]
    fn encode_comment_level_thread_id() {
        let result = encode_thread_id(&LinearThreadId {
            issue_id: "issue-123".to_string(),
            comment_id: Some("comment-456".to_string()),
            agent_session_id: None,
        });
        assert_eq!(result, "linear:issue-123:c:comment-456");
    }

    #[test]
    fn encode_comment_level_thread_with_uuids() {
        let result = encode_thread_id(&LinearThreadId {
            issue_id: "2174add1-f7c8-44e3-bbf3-2d60b5ea8bc9".to_string(),
            comment_id: Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string()),
            agent_session_id: None,
        });
        assert_eq!(
            result,
            "linear:2174add1-f7c8-44e3-bbf3-2d60b5ea8bc9:c:a1b2c3d4-e5f6-7890-abcd-ef1234567890"
        );
    }

    #[test]
    fn encode_agent_session_issue_thread_id() {
        let result = encode_thread_id(&LinearThreadId {
            issue_id: "issue-123".to_string(),
            comment_id: None,
            agent_session_id: Some("session-789".to_string()),
        });
        assert_eq!(result, "linear:issue-123:s:session-789");
    }

    #[test]
    fn encode_agent_session_comment_thread_id() {
        let result = encode_thread_id(&LinearThreadId {
            issue_id: "issue-123".to_string(),
            comment_id: Some("comment-456".to_string()),
            agent_session_id: Some("session-789".to_string()),
        });
        assert_eq!(result, "linear:issue-123:c:comment-456:s:session-789");
    }

    // ---------- decodeThreadId (9 upstream cases) ----------

    #[test]
    fn decode_issue_level_thread_id() {
        let r = decode_thread_id("linear:abc123-def456-789").unwrap();
        assert_eq!(r, issue_only("abc123-def456-789"));
    }

    #[test]
    fn decode_uuid_issue_level_thread_id() {
        let r = decode_thread_id("linear:2174add1-f7c8-44e3-bbf3-2d60b5ea8bc9").unwrap();
        assert_eq!(r, issue_only("2174add1-f7c8-44e3-bbf3-2d60b5ea8bc9"));
    }

    #[test]
    fn decode_comment_level_thread_id() {
        let r = decode_thread_id("linear:issue-123:c:comment-456").unwrap();
        assert_eq!(
            r,
            LinearThreadId {
                issue_id: "issue-123".to_string(),
                comment_id: Some("comment-456".to_string()),
                agent_session_id: None,
            }
        );
    }

    #[test]
    fn decode_comment_level_thread_with_uuids() {
        let r = decode_thread_id(
            "linear:2174add1-f7c8-44e3-bbf3-2d60b5ea8bc9:c:a1b2c3d4-e5f6-7890-abcd-ef1234567890",
        )
        .unwrap();
        assert_eq!(
            r,
            LinearThreadId {
                issue_id: "2174add1-f7c8-44e3-bbf3-2d60b5ea8bc9".to_string(),
                comment_id: Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string()),
                agent_session_id: None,
            }
        );
    }

    #[test]
    fn decode_agent_session_issue_thread_id() {
        let r = decode_thread_id("linear:issue-123:s:session-789").unwrap();
        assert_eq!(
            r,
            LinearThreadId {
                issue_id: "issue-123".to_string(),
                comment_id: None,
                agent_session_id: Some("session-789".to_string()),
            }
        );
    }

    #[test]
    fn decode_agent_session_comment_thread_id() {
        let r = decode_thread_id("linear:issue-123:c:comment-456:s:session-789").unwrap();
        assert_eq!(
            r,
            LinearThreadId {
                issue_id: "issue-123".to_string(),
                comment_id: Some("comment-456".to_string()),
                agent_session_id: Some("session-789".to_string()),
            }
        );
    }

    #[test]
    fn decode_throws_on_invalid_prefix() {
        let err = decode_thread_id("slack:C123:ts123").unwrap_err();
        assert!(
            err.to_string().contains("Invalid Linear thread ID"),
            "got: {err}"
        );
    }

    #[test]
    fn decode_throws_on_empty_issue_id() {
        let err = decode_thread_id("linear:").unwrap_err();
        assert!(
            err.to_string().contains("Invalid Linear thread ID format"),
            "got: {err}"
        );
    }

    #[test]
    fn decode_throws_on_completely_wrong_format() {
        let err = decode_thread_id("nonsense").unwrap_err();
        assert!(
            err.to_string().contains("Invalid Linear thread ID"),
            "got: {err}"
        );
    }

    // ---------- roundtrip (3 upstream cases) ----------

    #[test]
    fn round_trip_issue_level_thread_id() {
        let original = issue_only("2174add1-f7c8-44e3-bbf3-2d60b5ea8bc9");
        let encoded = encode_thread_id(&original);
        let decoded = decode_thread_id(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn round_trip_comment_level_thread_id() {
        let original = LinearThreadId {
            issue_id: "2174add1-f7c8-44e3-bbf3-2d60b5ea8bc9".to_string(),
            comment_id: Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string()),
            agent_session_id: None,
        };
        let encoded = encode_thread_id(&original);
        let decoded = decode_thread_id(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn round_trip_agent_session_comment_thread_id() {
        let original = LinearThreadId {
            issue_id: "issue-123".to_string(),
            comment_id: Some("comment-456".to_string()),
            agent_session_id: Some("session-789".to_string()),
        };
        let encoded = encode_thread_id(&original);
        let decoded = decode_thread_id(&encoded).unwrap();
        assert_eq!(decoded, original);
    }
}
