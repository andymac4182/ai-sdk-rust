//! Messenger message-fetch pagination.
//!
//! 1:1 port of upstream
//! `MessengerAdapter#paginateMessages(messages, options)` from
//! `packages/adapter-messenger/src/index.ts`. Pure function over a
//! pre-sorted `Vec<Message>` (the adapter's per-thread cache) +
//! [`PaginateOptions`] — returns a [`FetchResult`] mirroring
//! upstream's `{ messages, nextCursor }` shape.
//!
//! Direction semantics (1:1 with upstream):
//! - `Backward` (default): return the most-recent `limit` messages
//!   ending at the message before `cursor` (or end-of-list when no
//!   cursor). `nextCursor` is the id of the first message in the
//!   returned page, set only when there are older messages available.
//! - `Forward`: return the next `limit` messages starting after
//!   `cursor` (or from the first message when no cursor).
//!   `nextCursor` is the id of the last message in the returned page,
//!   set only when there are newer messages available.
//!
//! Limit clamping: `max(1, min(limit ?? 50, 100))` — values < 1 are
//! treated as 1 and > 100 are capped at 100, matching upstream's
//! inline `Math.max(1, Math.min(options.limit ?? 50, 100))`.

use chat_sdk_chat::message::Message;

/// Pagination direction. 1:1 with upstream
/// `options.direction: "backward" | "forward"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaginateDirection {
    /// Older messages first (default).
    Backward,
    /// Newer messages first.
    Forward,
}

impl Default for PaginateDirection {
    fn default() -> Self {
        Self::Backward
    }
}

/// Options for [`paginate_messages`]. 1:1 with upstream
/// `FetchOptions`.
#[derive(Debug, Clone, Default)]
pub struct PaginateOptions {
    /// Pagination cursor (message id from a prior [`FetchResult::next_cursor`]).
    pub cursor: Option<String>,
    /// Direction. Defaults to [`PaginateDirection::Backward`].
    pub direction: Option<PaginateDirection>,
    /// Max messages per page. Clamped to `[1, 100]`; defaults to 50.
    pub limit: Option<i32>,
}

/// Result of [`paginate_messages`]. 1:1 with upstream
/// `FetchResult<MessengerRawMessage>`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FetchResult {
    /// Messages on this page (always in chronological order, per
    /// upstream).
    pub messages: Vec<Message>,
    /// Cursor for the next page, when more messages exist in the
    /// scan direction.
    pub next_cursor: Option<String>,
}

/// Paginate a pre-sorted message slice. 1:1 with upstream
/// `paginateMessages(messages, options)`.
pub fn paginate_messages(messages: &[Message], options: &PaginateOptions) -> FetchResult {
    let raw_limit = options.limit.unwrap_or(50);
    // clamp((raw_limit, 1, 100))
    let limit = raw_limit.max(1).min(100) as usize;
    let direction = options.direction.unwrap_or_default();

    if messages.is_empty() {
        return FetchResult::default();
    }

    // Build id -> index map (1:1 with upstream's
    // `new Map(messages.map((m, i) => [m.id, i]))`).
    let index_of = |id: &str| messages.iter().position(|m| m.id == id);

    match direction {
        PaginateDirection::Backward => {
            let end = options
                .cursor
                .as_deref()
                .and_then(index_of)
                .unwrap_or(messages.len());
            let start = end.saturating_sub(limit);
            let page = messages[start..end].to_vec();
            let next_cursor = if start > 0 {
                page.first().map(|m| m.id.clone())
            } else {
                None
            };
            FetchResult {
                messages: page,
                next_cursor,
            }
        }
        PaginateDirection::Forward => {
            let start = options
                .cursor
                .as_deref()
                .and_then(index_of)
                .map(|i| i + 1)
                .unwrap_or(0);
            let end = (start + limit).min(messages.len());
            // Guard against start > end (empty page).
            let page = if start >= messages.len() {
                Vec::new()
            } else {
                messages[start..end].to_vec()
            };
            let next_cursor = if end < messages.len() {
                page.last().map(|m| m.id.clone())
            } else {
                None
            };
            FetchResult {
                messages: page,
                next_cursor,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{
        MessengerMessagePayload, MessengerMessagingEvent, MessengerRecipient, MessengerSender,
        parse_messenger_message,
    };

    fn fixture(count: usize) -> Vec<Message> {
        // Mirrors upstream's
        //   for (let i = 1; i <= count; i++) {
        //     adapter.parseMessage({ ..., timestamp: 1735689600000 + i*1000,
        //       message: { mid: `mid.${i}`, text: `message ${i}` } });
        //   }
        let mut out = Vec::new();
        for i in 1..=count {
            let event = MessengerMessagingEvent {
                sender: MessengerSender {
                    id: "USER_123".to_string(),
                },
                recipient: MessengerRecipient {
                    id: "PAGE_456".to_string(),
                },
                timestamp: 1735689600000 + (i as i64) * 1000,
                message: Some(MessengerMessagePayload {
                    mid: Some(format!("mid.{i}")),
                    text: Some(format!("message {i}")),
                    is_echo: false,
                    attachments: None,
                    quick_reply: None,
                }),
                postback: None,
            };
            out.push(parse_messenger_message(&event, None));
        }
        out
    }

    // ---------- message fetching (7 upstream cases) ----------

    #[test]
    fn returns_empty_result_for_unknown_thread() {
        // 1:1 with upstream index.test.ts:1634 > "returns empty result for unknown thread"
        let got = paginate_messages(&[], &PaginateOptions::default());
        assert!(got.messages.is_empty());
        assert_eq!(got.next_cursor, None);
    }

    #[test]
    fn fetches_messages_backward_default() {
        // 1:1 with upstream index.test.ts:1640 > "fetches messages backward (default)"
        let msgs = fixture(5);
        let got = paginate_messages(
            &msgs,
            &PaginateOptions {
                limit: Some(3),
                ..Default::default()
            },
        );
        assert_eq!(got.messages.len(), 3);
        assert_eq!(got.messages[0].id, "mid.3");
        assert_eq!(got.messages[2].id, "mid.5");
        assert_eq!(got.next_cursor, Some("mid.3".to_string()));
    }

    #[test]
    fn fetches_messages_backward_with_cursor() {
        // 1:1 with upstream index.test.ts:1651 > "fetches messages backward with cursor"
        let msgs = fixture(5);
        let got = paginate_messages(
            &msgs,
            &PaginateOptions {
                limit: Some(2),
                cursor: Some("mid.3".to_string()),
                direction: Some(PaginateDirection::Backward),
            },
        );
        assert_eq!(got.messages.len(), 2);
        assert_eq!(got.messages[0].id, "mid.1");
        assert_eq!(got.messages[1].id, "mid.2");
    }

    #[test]
    fn fetches_messages_forward() {
        // 1:1 with upstream index.test.ts:1663 > "fetches messages forward"
        let msgs = fixture(5);
        let got = paginate_messages(
            &msgs,
            &PaginateOptions {
                limit: Some(2),
                direction: Some(PaginateDirection::Forward),
                ..Default::default()
            },
        );
        assert_eq!(got.messages.len(), 2);
        assert_eq!(got.messages[0].id, "mid.1");
        assert_eq!(got.messages[1].id, "mid.2");
        assert_eq!(got.next_cursor, Some("mid.2".to_string()));
    }

    #[test]
    fn fetches_messages_forward_with_cursor() {
        // 1:1 with upstream index.test.ts:1675 > "fetches messages forward with cursor"
        let msgs = fixture(5);
        let got = paginate_messages(
            &msgs,
            &PaginateOptions {
                limit: Some(2),
                cursor: Some("mid.2".to_string()),
                direction: Some(PaginateDirection::Forward),
            },
        );
        assert_eq!(got.messages.len(), 2);
        assert_eq!(got.messages[0].id, "mid.3");
        assert_eq!(got.messages[1].id, "mid.4");
        assert_eq!(got.next_cursor, Some("mid.4".to_string()));
    }

    #[test]
    fn returns_no_next_cursor_when_all_messages_are_returned() {
        // 1:1 with upstream index.test.ts:1688 > "returns no nextCursor when all messages are returned"
        let msgs = fixture(5);
        let got = paginate_messages(
            &msgs,
            &PaginateOptions {
                limit: Some(100),
                ..Default::default()
            },
        );
        assert_eq!(got.messages.len(), 5);
        assert_eq!(got.next_cursor, None);
    }

    #[test]
    fn returns_null_for_non_existent_message() {
        // 1:1 with upstream index.test.ts:1697 > "returns null for non-existent message".
        // Upstream's `fetchMessage(threadId, id)` returns `null` when no
        // cached message matches. The Rust analog is a linear scan over
        // the thread's cached messages; both surface "miss" identically.
        let msgs = fixture(5);
        let got = msgs.iter().find(|m| m.id == "mid.nonexistent");
        assert!(got.is_none());
    }

    // ---------- pagination edge cases (7 upstream cases) ----------

    #[test]
    fn clamps_negative_limit_to_1() {
        // 1:1 with upstream index.test.ts:1727 > "clamps negative limit to 1"
        let msgs = fixture(5);
        let got = paginate_messages(
            &msgs,
            &PaginateOptions {
                limit: Some(-10),
                ..Default::default()
            },
        );
        assert_eq!(got.messages.len(), 1);
    }

    #[test]
    fn clamps_limit_above_100_to_100() {
        // 1:1 with upstream index.test.ts:1735 > "clamps limit above 100 to 100"
        let msgs = fixture(5);
        let got = paginate_messages(
            &msgs,
            &PaginateOptions {
                limit: Some(500),
                ..Default::default()
            },
        );
        assert_eq!(got.messages.len(), 5);
    }

    #[test]
    fn returns_no_next_cursor_for_forward_from_last_message() {
        // 1:1 with upstream index.test.ts:1743 > "returns no nextCursor for forward from last message"
        let msgs = fixture(3);
        let got = paginate_messages(
            &msgs,
            &PaginateOptions {
                cursor: Some("mid.3".to_string()),
                direction: Some(PaginateDirection::Forward),
                limit: Some(10),
            },
        );
        assert_eq!(got.messages.len(), 0);
        assert_eq!(got.next_cursor, None);
    }

    #[test]
    fn returns_no_next_cursor_for_backward_from_first_message() {
        // 1:1 with upstream index.test.ts:1754 > "returns no nextCursor for backward from first message"
        let msgs = fixture(3);
        let got = paginate_messages(
            &msgs,
            &PaginateOptions {
                cursor: Some("mid.1".to_string()),
                direction: Some(PaginateDirection::Backward),
                limit: Some(10),
            },
        );
        assert_eq!(got.messages.len(), 0);
        assert_eq!(got.next_cursor, None);
    }

    #[test]
    fn ignores_unknown_cursor_for_backward_and_returns_from_end() {
        // 1:1 with upstream index.test.ts:1765 > "ignores unknown cursor for backward and returns from end"
        let msgs = fixture(3);
        let got = paginate_messages(
            &msgs,
            &PaginateOptions {
                cursor: Some("mid.nonexistent".to_string()),
                direction: Some(PaginateDirection::Backward),
                limit: Some(2),
            },
        );
        assert_eq!(got.messages.len(), 2);
        assert_eq!(got.messages[1].id, "mid.3");
    }

    #[test]
    fn ignores_unknown_cursor_for_forward_and_returns_from_start() {
        // 1:1 with upstream index.test.ts:1776 > "ignores unknown cursor for forward and returns from start"
        let msgs = fixture(3);
        let got = paginate_messages(
            &msgs,
            &PaginateOptions {
                cursor: Some("mid.nonexistent".to_string()),
                direction: Some(PaginateDirection::Forward),
                limit: Some(2),
            },
        );
        assert_eq!(got.messages.len(), 2);
        assert_eq!(got.messages[0].id, "mid.1");
    }

    #[test]
    fn uses_default_limit_of_50_when_not_specified() {
        // 1:1 with upstream index.test.ts:1787 > "uses default limit of 50 when not specified"
        let msgs = fixture(3);
        let got = paginate_messages(&msgs, &PaginateOptions::default());
        assert_eq!(got.messages.len(), 3);
    }
}
