//! Linear inbound-message parsing.
//!
//! 1:1 port of upstream `LinearAdapter#parseMessage(raw)` from
//! `packages/adapter-linear/src/index.ts` (function body at index.ts
//! L2026-L2061).
//!
//! Exposing the parser as a pure helper lets the Rust port assert
//! the message-shape contract (id / threadId / author / metadata /
//! edited-detection) without the upstream Vitest `vi.fn()` HTTP
//! infrastructure. The 7 upstream `describe("parseMessage")` cases
//! drive `LinearCommentRawMessage` / `LinearAgentSessionCommentRawMessage`
//! shapes directly — they do not exercise any HTTP / LinearClient
//! typed-client path, so they port 1:1.

use chat_sdk_chat::markdown::{Root, root};
use chat_sdk_chat::message::Message;
use chat_sdk_chat::types::{Author, BotStatus, MessageMetadata};

use crate::markdown::LinearFormatConverter;
use crate::thread_id::{LinearThreadId, encode_thread_id};

/// 1:1 port of upstream `interface LinearActorData` (excerpted —
/// only the fields the parser reads). Other fields (`avatarUrl`,
/// `email`) flow into the `raw` payload via the webhook envelope
/// but aren't surfaced on the parsed [`Author`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearActor {
    /// Actor UUID (Linear user id).
    pub id: String,
    /// Actor's display name (1:1 with upstream `displayName`).
    pub display_name: String,
    /// Actor's full name (1:1 with upstream `fullName`).
    pub full_name: String,
    /// 1:1 with upstream `type: "user" | "bot"`.
    pub user_type: LinearActorType,
}

/// 1:1 with upstream `type: "user" | "bot"` discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinearActorType {
    /// Human actor.
    User,
    /// Bot actor.
    Bot,
}

/// 1:1 port of upstream `interface LinearCommentData`. The parser
/// only needs the fields it actually reads — `url`, `parentId` etc.
/// flow into the `raw` payload via the webhook envelope but aren't
/// surfaced on the parsed [`Message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearComment {
    /// Comment UUID.
    pub id: String,
    /// Comment body in markdown format.
    pub body: String,
    /// Issue UUID the comment is associated with.
    pub issue_id: String,
    /// User who wrote the comment.
    pub user: LinearActor,
    /// ISO 8601 creation date.
    pub created_at: String,
    /// ISO 8601 last update date.
    pub updated_at: String,
}

/// 1:1 port of upstream `type LinearRawMessage = LinearCommentRawMessage
/// | LinearAgentSessionCommentRawMessage`. The discriminator is
/// `kind: "comment" | "agent_session_comment"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinearRawMessage {
    /// 1:1 with upstream `LinearCommentRawMessage` (kind: "comment").
    Comment {
        /// 1:1 with upstream `organizationId`.
        organization_id: String,
        /// 1:1 with upstream `comment`.
        comment: LinearComment,
    },
    /// 1:1 with upstream `LinearAgentSessionCommentRawMessage`.
    AgentSessionComment {
        /// 1:1 with upstream `organizationId`.
        organization_id: String,
        /// 1:1 with upstream `agentSessionId`.
        agent_session_id: String,
        /// 1:1 with upstream `comment`.
        comment: LinearComment,
    },
}

impl LinearRawMessage {
    /// Borrow the inner comment regardless of kind.
    pub fn comment(&self) -> &LinearComment {
        match self {
            Self::Comment { comment, .. } | Self::AgentSessionComment { comment, .. } => comment,
        }
    }

    /// Discriminator string (1:1 with upstream `raw.kind`).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Comment { .. } => "comment",
            Self::AgentSessionComment { .. } => "agent_session_comment",
        }
    }

    /// 1:1 with upstream `raw.organizationId`.
    pub fn organization_id(&self) -> &str {
        match self {
            Self::Comment {
                organization_id, ..
            }
            | Self::AgentSessionComment {
                organization_id, ..
            } => organization_id,
        }
    }

    /// Serialize the raw payload back to a JSON `Value` for the
    /// [`Message::raw`] field. 1:1 with upstream's pass-through
    /// `raw` field on the constructed `Message`.
    pub fn to_raw_value(&self) -> serde_json::Value {
        use serde_json::json;
        let comment = self.comment();
        let comment_json = json!({
            "id": comment.id,
            "body": comment.body,
            "issueId": comment.issue_id,
            "user": {
                "id": comment.user.id,
                "displayName": comment.user.display_name,
                "fullName": comment.user.full_name,
                "type": match comment.user.user_type {
                    LinearActorType::User => "user",
                    LinearActorType::Bot => "bot",
                },
            },
            "createdAt": comment.created_at,
            "updatedAt": comment.updated_at,
        });
        match self {
            Self::Comment {
                organization_id, ..
            } => json!({
                "kind": "comment",
                "organizationId": organization_id,
                "comment": comment_json,
            }),
            Self::AgentSessionComment {
                organization_id,
                agent_session_id,
                ..
            } => json!({
                "kind": "agent_session_comment",
                "organizationId": organization_id,
                "agentSessionId": agent_session_id,
                "comment": comment_json,
            }),
        }
    }
}

/// 1:1 port of upstream `LinearAdapter#parseMessage(raw)` (index.ts
/// L2026-L2061). Pure function over [`LinearRawMessage`] — the
/// converter is injected (matches upstream's `this.formatConverter`)
/// and the bot user id is passed explicitly (matches upstream's
/// `this.botUserId`).
///
/// Upstream wire shape:
/// - `id` = `raw.comment.id`
/// - `isMention` = `raw.kind === "agent_session_comment"`
/// - `threadId` = `encodeThreadId({ issueId, commentId, agentSessionId? })`
/// - `text` = `raw.comment.body`
/// - `formatted` = `formatConverter.toAst(text)`
/// - `author.userId` = `raw.comment.user.id`
/// - `author.userName` = `raw.comment.user.displayName`
/// - `author.fullName` = `raw.comment.user.fullName`
/// - `author.isBot` = `raw.comment.user.type === "bot"`
/// - `author.isMe` = `raw.comment.user.id === botUserId`
/// - `metadata.dateSent` = `new Date(raw.comment.createdAt)`
/// - `metadata.edited` = `raw.comment.createdAt !== raw.comment.updatedAt`
/// - `metadata.editedAt` = `edited ? new Date(updatedAt) : undefined`
/// - `attachments` = `[]`
/// - `raw` = pass-through
pub fn parse_message(
    raw: &LinearRawMessage,
    converter: &LinearFormatConverter,
    bot_user_id: Option<&str>,
) -> Message {
    let comment = raw.comment();

    let formatted: Root = match converter.to_ast(&comment.body) {
        Ok(chat_sdk_chat::markdown::Node::Root(r)) => r,
        // Empty-body case (or parse error) — fall back to an empty
        // root, matching upstream's "Hello world" / "" parity on the
        // formatted field.
        _ => root(vec![]),
    };

    let is_mention = matches!(raw, LinearRawMessage::AgentSessionComment { .. });

    let agent_session_id = match raw {
        LinearRawMessage::AgentSessionComment {
            agent_session_id, ..
        } => Some(agent_session_id.clone()),
        _ => None,
    };

    let thread_id = encode_thread_id(&LinearThreadId {
        issue_id: comment.issue_id.clone(),
        comment_id: Some(comment.id.clone()),
        agent_session_id,
    });

    let is_bot = matches!(comment.user.user_type, LinearActorType::Bot);
    let is_me = bot_user_id
        .map(|bid| comment.user.id == bid)
        .unwrap_or(false);

    let edited = comment.created_at != comment.updated_at;
    let edited_at = if edited {
        Some(comment.updated_at.clone())
    } else {
        None
    };

    let mut message = Message::new(
        comment.id.clone(),
        thread_id,
        comment.body.clone(),
        formatted,
        raw.to_raw_value(),
        Author {
            full_name: comment.user.full_name.clone(),
            is_bot: BotStatus::Known(is_bot),
            is_me,
            user_id: comment.user.id.clone(),
            user_name: comment.user.display_name.clone(),
        },
        MessageMetadata {
            date_sent: comment.created_at.clone(),
            edited,
            edited_at,
        },
        Vec::new(),
    );
    message.is_mention = Some(is_mention);
    message
}

#[cfg(test)]
mod tests {
    //! 1:1 ports of upstream `describe("parseMessage")` (index.test.ts
    //! L728-L826) — 7 cases. Each upstream case lifts a
    //! `LinearCommentRawMessage` (or `LinearAgentSessionCommentRawMessage`)
    //! through `adapter.parseMessage(raw)` and asserts on the shape of
    //! the returned `Message`. The Rust parser is pure (no HTTP
    //! dependency) so the cases port without any `vi.fn()` mock
    //! infrastructure.
    use super::*;

    const BOT_USER_ID: &str = "bot-user-id";

    fn user_actor() -> LinearActor {
        LinearActor {
            id: "user-456".to_string(),
            display_name: "Test User".to_string(),
            full_name: "Test User".to_string(),
            user_type: LinearActorType::User,
        }
    }

    fn raw_comment(body: &str) -> LinearRawMessage {
        LinearRawMessage::Comment {
            organization_id: "org-123".to_string(),
            comment: LinearComment {
                id: "comment-abc123".to_string(),
                body: body.to_string(),
                issue_id: "issue-123".to_string(),
                user: user_actor(),
                created_at: "2025-01-29T12:00:00.000Z".to_string(),
                updated_at: "2025-01-29T12:00:00.000Z".to_string(),
            },
        }
    }

    // 1:1 with upstream index.test.ts:729 > "should parse a raw Linear message"
    #[test]
    fn should_parse_a_raw_linear_message() {
        let converter = LinearFormatConverter::new();
        let raw = raw_comment("Hello from Linear!");
        let message = parse_message(&raw, &converter, Some(BOT_USER_ID));
        assert_eq!(message.id, "comment-abc123");
        assert_eq!(message.text, "Hello from Linear!");
        assert_eq!(message.author.user_id, "user-456");
    }

    // 1:1 with upstream index.test.ts:738 > "should detect edited messages"
    #[test]
    fn should_detect_edited_messages() {
        let converter = LinearFormatConverter::new();
        let mut raw = raw_comment("Edited message");
        if let LinearRawMessage::Comment { comment, .. } = &mut raw {
            comment.updated_at = "2025-01-29T13:00:00.000Z".to_string();
        }
        let message = parse_message(&raw, &converter, Some(BOT_USER_ID));
        assert!(message.metadata.edited);
    }

    // 1:1 with upstream index.test.ts:748 > "should handle empty body"
    #[test]
    fn should_handle_empty_body() {
        let converter = LinearFormatConverter::new();
        let raw = LinearRawMessage::Comment {
            organization_id: "org-123".to_string(),
            comment: LinearComment {
                id: "comment-empty".to_string(),
                body: String::new(),
                issue_id: "issue-1".to_string(),
                user: LinearActor {
                    id: "user-1".to_string(),
                    display_name: String::new(),
                    full_name: String::new(),
                    user_type: LinearActorType::User,
                },
                created_at: "2025-01-29T12:00:00.000Z".to_string(),
                updated_at: "2025-01-29T12:00:00.000Z".to_string(),
            },
        };
        let message = parse_message(&raw, &converter, Some(BOT_USER_ID));
        assert_eq!(message.text, "");
        assert!(!message.metadata.edited);
    }

    // 1:1 with upstream index.test.ts:761 > "should set editedAt when message is edited"
    #[test]
    fn should_set_edited_at_when_message_is_edited() {
        let converter = LinearFormatConverter::new();
        let raw = LinearRawMessage::Comment {
            organization_id: "org-123".to_string(),
            comment: LinearComment {
                id: "comment-edited".to_string(),
                body: "Updated text".to_string(),
                issue_id: "issue-1".to_string(),
                user: LinearActor {
                    id: "user-1".to_string(),
                    display_name: "User".to_string(),
                    full_name: "User".to_string(),
                    user_type: LinearActorType::User,
                },
                created_at: "2025-01-29T12:00:00.000Z".to_string(),
                updated_at: "2025-01-29T14:30:00.000Z".to_string(),
            },
        };
        let message = parse_message(&raw, &converter, Some(BOT_USER_ID));
        assert!(message.metadata.edited);
        assert_eq!(
            message.metadata.edited_at.as_deref(),
            Some("2025-01-29T14:30:00.000Z")
        );
    }

    // 1:1 with upstream index.test.ts:777 > "should not set editedAt when message is not edited"
    #[test]
    fn should_not_set_edited_at_when_message_is_not_edited() {
        let converter = LinearFormatConverter::new();
        let raw = raw_comment("Original text");
        let message = parse_message(&raw, &converter, Some(BOT_USER_ID));
        assert!(message.metadata.edited_at.is_none());
    }

    // 1:1 with upstream index.test.ts:789 >
    // "should set isBot to false and isMe to false for regular users"
    #[test]
    fn should_set_is_bot_false_and_is_me_false_for_regular_users() {
        let converter = LinearFormatConverter::new();
        let raw = raw_comment("test");
        let message = parse_message(&raw, &converter, Some(BOT_USER_ID));
        assert_eq!(message.author.is_bot, BotStatus::Known(false));
        assert!(!message.author.is_me);
    }

    // 1:1 with upstream index.test.ts:802 >
    // "should parse an agent session comment raw message like a comment"
    #[test]
    fn should_parse_an_agent_session_comment_raw_message_like_a_comment() {
        let converter = LinearFormatConverter::new();
        let raw = LinearRawMessage::AgentSessionComment {
            organization_id: "org-123".to_string(),
            agent_session_id: "session-123".to_string(),
            comment: LinearComment {
                id: "comment-abc123".to_string(),
                body: "Hello from an agent session".to_string(),
                issue_id: "issue-123".to_string(),
                user: user_actor(),
                created_at: "2025-01-29T12:00:00.000Z".to_string(),
                updated_at: "2025-01-29T12:00:00.000Z".to_string(),
            },
        };
        let message = parse_message(&raw, &converter, Some(BOT_USER_ID));
        assert_eq!(message.id, "comment-abc123");
        assert_eq!(message.text, "Hello from an agent session");
        assert_eq!(message.author.user_id, "user-456");
        assert_eq!(message.raw["kind"], "agent_session_comment");
    }

    // ---------- additive Rust-side coverage ----------

    #[test]
    fn agent_session_thread_id_carries_session_segment() {
        let converter = LinearFormatConverter::new();
        let raw = LinearRawMessage::AgentSessionComment {
            organization_id: "org-1".to_string(),
            agent_session_id: "sess-1".to_string(),
            comment: LinearComment {
                id: "c-1".to_string(),
                body: String::new(),
                issue_id: "iss-1".to_string(),
                user: user_actor(),
                created_at: "2025-01-29T12:00:00.000Z".to_string(),
                updated_at: "2025-01-29T12:00:00.000Z".to_string(),
            },
        };
        let message = parse_message(&raw, &converter, None);
        assert_eq!(message.thread_id, "linear:iss-1:c:c-1:s:sess-1");
        assert_eq!(message.is_mention, Some(true));
    }

    #[test]
    fn comment_thread_id_omits_session_segment() {
        let converter = LinearFormatConverter::new();
        let raw = raw_comment("hi");
        let message = parse_message(&raw, &converter, None);
        assert_eq!(message.thread_id, "linear:issue-123:c:comment-abc123");
        assert_eq!(message.is_mention, Some(false));
    }

    #[test]
    fn bot_actor_type_sets_is_bot_true() {
        let converter = LinearFormatConverter::new();
        let raw = LinearRawMessage::Comment {
            organization_id: "org-1".to_string(),
            comment: LinearComment {
                id: "c-1".to_string(),
                body: "bot says hi".to_string(),
                issue_id: "iss-1".to_string(),
                user: LinearActor {
                    id: "bot-1".to_string(),
                    display_name: "Bot".to_string(),
                    full_name: "Bot".to_string(),
                    user_type: LinearActorType::Bot,
                },
                created_at: "2025-01-29T12:00:00.000Z".to_string(),
                updated_at: "2025-01-29T12:00:00.000Z".to_string(),
            },
        };
        let message = parse_message(&raw, &converter, Some("bot-1"));
        assert_eq!(message.author.is_bot, BotStatus::Known(true));
        assert!(message.author.is_me);
    }
}
