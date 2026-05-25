//! GitHub inbound-message parsing.
//!
//! 1:1 port of upstream `GithubAdapter#parseMessage(raw)` +
//! `parseAuthor(user)` + `parseIssueComment(...)` + `parseReviewComment(...)`
//! from `packages/adapter-github/src/index.ts`.
//!
//! These helpers are pure functions over the inbound webhook /
//! REST shapes; the async HTTP layer + `Chat` dispatch live on the
//! adapter. Exposing the parser as a pure helper lets the message-
//! shape contract be asserted without a `vi.fn()` HTTP harness.

use chat_sdk_chat::markdown::{Root, root};
use chat_sdk_chat::message::Message;
use chat_sdk_chat::types::{Author, BotStatus, MessageMetadata};

use crate::markdown::GitHubFormatConverter;
use crate::{
    DecodeThreadIdError, EncodeThreadIdError, GithubThreadId, GithubThreadKind,
    encode_thread_id_full,
};

/// GitHub user sub-shape used in `comment.user` / `pr.user`. 1:1
/// with upstream `interface GitHubUser { id; login; type; }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubUser {
    /// Numeric user id (GitHub assigns integers).
    pub id: u64,
    /// Username/handle.
    pub login: String,
    /// `"User"` or `"Bot"` — upstream relies on this for `isBot`.
    pub user_type: GithubUserType,
}

/// User type discriminator. 1:1 with upstream `"User" | "Bot"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubUserType {
    /// Human user.
    User,
    /// Bot user.
    Bot,
}

/// Repository sub-shape used in raw messages. 1:1 with upstream
/// `raw.repository` (only the `owner.login` + `name` fields are
/// needed by the parser).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryRef {
    /// Repository owner.
    pub owner_login: String,
    /// Repository name (no owner prefix).
    pub name: String,
}

/// Issue comment shape consumed by [`parse_issue_comment`]. 1:1 with
/// upstream `GitHubIssueComment` (only fields needed for parsing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueCommentRef {
    /// Comment id.
    pub id: u64,
    /// Comment body (markdown).
    pub body: String,
    /// Author user block.
    pub user: GithubUser,
    /// ISO-8601 created timestamp.
    pub created_at: String,
    /// ISO-8601 last-update timestamp. When equal to `created_at`,
    /// upstream marks the message as not edited.
    pub updated_at: String,
}

/// Review comment shape consumed by [`parse_review_comment`]. 1:1 with
/// upstream `GitHubReviewComment` (only fields needed for parsing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewCommentRef {
    /// Comment id.
    pub id: u64,
    /// Comment body (markdown).
    pub body: String,
    /// Author user block.
    pub user: GithubUser,
    /// ISO-8601 created timestamp.
    pub created_at: String,
    /// ISO-8601 last-update timestamp.
    pub updated_at: String,
    /// Root review-comment id for reply chains. `None` for root
    /// comments. 1:1 with upstream `comment.in_reply_to_id`.
    pub in_reply_to_id: Option<u64>,
}

/// Raw inbound message variant. 1:1 with upstream's tagged
/// `GitHubRawMessage` union (`type: "issue_comment" | "review_comment"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubRawMessage {
    /// Issue-comment payload (PR-level or issue thread).
    IssueComment {
        /// Comment block.
        comment: IssueCommentRef,
        /// Repository block.
        repository: RepositoryRef,
        /// PR number (or issue number when `thread_type == Issue`).
        pr_number: u64,
        /// Thread variant. Upstream defaults `thread_type` to
        /// `"pr"` when omitted.
        thread_type: GithubThreadKind,
    },
    /// Review-comment payload (Files Changed tab — line-anchored).
    ReviewComment {
        /// Comment block.
        comment: ReviewCommentRef,
        /// Repository block.
        repository: RepositoryRef,
        /// PR number the review comment lives on.
        pr_number: u64,
    },
}

/// Errors returned by [`parse_message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseMessageError {
    /// Thread-id encoding failed (e.g. unsupported variant
    /// combination). Mirrors upstream's `encodeThreadId` throw path.
    Encode(EncodeThreadIdError),
    /// Thread-id decoding failed for a downstream lookup. Mirrors
    /// upstream's `decodeThreadId` throw path.
    Decode(DecodeThreadIdError),
}

impl std::fmt::Display for ParseMessageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encode(e) => write!(f, "{e}"),
            Self::Decode(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ParseMessageError {}

/// Parse a GitHub user into an [`Author`]. 1:1 with upstream
/// `protected parseAuthor(user: GitHubUser): Author`.
///
/// `bot_user_id` is the adapter's configured bot id (numeric). When
/// `Some(id)` and `user.id == id`, `is_me` is `true`.
pub fn parse_author(user: &GithubUser, bot_user_id: Option<u64>) -> Author {
    let is_bot = matches!(user.user_type, GithubUserType::Bot);
    Author {
        user_id: user.id.to_string(),
        user_name: user.login.clone(),
        // GitHub doesn't always expose real names — upstream uses
        // `login` for `fullName` too.
        full_name: user.login.clone(),
        is_bot: if is_bot {
            BotStatus::TRUE
        } else {
            BotStatus::FALSE
        },
        is_me: bot_user_id.map(|id| user.id == id).unwrap_or(false),
    }
}

/// Parse an issue comment into a normalized [`Message`]. 1:1 with
/// upstream `protected parseIssueComment(comment, repository,
/// prNumber, threadId, threadType)`.
///
/// `thread_id` is the already-encoded thread id (caller is expected
/// to derive it via [`encode_thread_id_full`] for the right variant).
pub fn parse_issue_comment(
    comment: &IssueCommentRef,
    thread_id: &str,
    bot_user_id: Option<u64>,
) -> Message {
    let author = parse_author(&comment.user, bot_user_id);
    let converter = GitHubFormatConverter::new();
    let text = converter.extract_plain_text(&comment.body);
    let formatted: Root = match converter.to_ast(&comment.body) {
        Ok(chat_sdk_chat::markdown::Node::Root(r)) => r,
        Ok(other) => root(vec![other]),
        Err(_) => root(vec![]),
    };
    let edited = comment.created_at != comment.updated_at;
    let metadata = MessageMetadata {
        date_sent: comment.created_at.clone(),
        edited,
        edited_at: if edited {
            Some(comment.updated_at.clone())
        } else {
            None
        },
    };
    Message::new(
        comment.id.to_string(),
        thread_id.to_string(),
        text,
        formatted,
        serde_json::Value::Null,
        author,
        metadata,
        Vec::new(),
    )
}

/// Parse a review comment into a normalized [`Message`]. 1:1 with
/// upstream `protected parseReviewComment(comment, repository,
/// prNumber, threadId)`.
pub fn parse_review_comment(
    comment: &ReviewCommentRef,
    thread_id: &str,
    bot_user_id: Option<u64>,
) -> Message {
    let author = parse_author(&comment.user, bot_user_id);
    let converter = GitHubFormatConverter::new();
    let text = converter.extract_plain_text(&comment.body);
    let formatted: Root = match converter.to_ast(&comment.body) {
        Ok(chat_sdk_chat::markdown::Node::Root(r)) => r,
        Ok(other) => root(vec![other]),
        Err(_) => root(vec![]),
    };
    let edited = comment.created_at != comment.updated_at;
    let metadata = MessageMetadata {
        date_sent: comment.created_at.clone(),
        edited,
        edited_at: if edited {
            Some(comment.updated_at.clone())
        } else {
            None
        },
    };
    Message::new(
        comment.id.to_string(),
        thread_id.to_string(),
        text,
        formatted,
        serde_json::Value::Null,
        author,
        metadata,
        Vec::new(),
    )
}

/// Parse any [`GithubRawMessage`] into a normalized [`Message`]. 1:1
/// with upstream `parseMessage(raw)` — dispatches on `raw.type`,
/// derives the thread id via [`encode_thread_id_full`], and routes
/// to the right per-variant parser.
pub fn parse_message(
    raw: &GithubRawMessage,
    bot_user_id: Option<u64>,
) -> Result<Message, ParseMessageError> {
    match raw {
        GithubRawMessage::IssueComment {
            comment,
            repository,
            pr_number,
            thread_type,
        } => {
            let thread = GithubThreadId {
                owner: repository.owner_login.clone(),
                repo: repository.name.clone(),
                pr_number: *pr_number,
                kind: *thread_type,
                review_comment_id: None,
            };
            let thread_id = encode_thread_id_full(&thread).map_err(ParseMessageError::Encode)?;
            Ok(parse_issue_comment(comment, &thread_id, bot_user_id))
        }
        GithubRawMessage::ReviewComment {
            comment,
            repository,
            pr_number,
        } => {
            let root_comment_id = comment.in_reply_to_id.unwrap_or(comment.id);
            let thread = GithubThreadId {
                owner: repository.owner_login.clone(),
                repo: repository.name.clone(),
                pr_number: *pr_number,
                kind: GithubThreadKind::Pr,
                review_comment_id: Some(root_comment_id),
            };
            let thread_id = encode_thread_id_full(&thread).map_err(ParseMessageError::Encode)?;
            Ok(parse_review_comment(comment, &thread_id, bot_user_id))
        }
    }
}

#[cfg(test)]
mod tests {
    //! 1:1 with upstream `index.test.ts > describe("parseMessage")` (7
    //! upstream cases) + `describe("parseAuthor (via parseMessage)")` (3
    //! upstream cases) — all 10 portable cases mapped here.
    use super::*;

    fn user(id: u64, login: &str, is_bot: bool) -> GithubUser {
        GithubUser {
            id,
            login: login.to_string(),
            user_type: if is_bot {
                GithubUserType::Bot
            } else {
                GithubUserType::User
            },
        }
    }

    fn repo() -> RepositoryRef {
        RepositoryRef {
            owner_login: "acme".to_string(),
            name: "app".to_string(),
        }
    }

    fn issue_comment(id: u64, body: &str, user: GithubUser) -> IssueCommentRef {
        IssueCommentRef {
            id,
            body: body.to_string(),
            user,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    fn review_comment(id: u64, body: &str, user: GithubUser) -> ReviewCommentRef {
        ReviewCommentRef {
            id,
            body: body.to_string(),
            user,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            in_reply_to_id: None,
        }
    }

    // ---------- describe("parseMessage") ----------

    #[test]
    fn should_parse_an_issue_comment_raw_message() {
        // 1:1 with upstream index.test.ts:1570 > "should parse an issue_comment raw message".
        let raw = GithubRawMessage::IssueComment {
            comment: issue_comment(100, "Test comment", user(1, "testuser", false)),
            repository: repo(),
            pr_number: 42,
            thread_type: GithubThreadKind::Pr,
        };
        let message = parse_message(&raw, None).unwrap();
        assert_eq!(message.id, "100");
        assert_eq!(message.thread_id, "github:acme/app:42");
        assert_eq!(message.text, "Test comment");
        assert_eq!(message.author.user_name, "testuser");
        assert_eq!(message.author.is_bot, BotStatus::FALSE);
    }

    #[test]
    fn should_parse_an_issue_comment_raw_message_from_an_issue_thread() {
        // 1:1 with upstream index.test.ts:1598 > "should parse an
        // issue_comment raw message from an issue thread".
        let raw = GithubRawMessage::IssueComment {
            comment: issue_comment(100, "Issue comment", user(1, "testuser", false)),
            repository: repo(),
            pr_number: 10,
            thread_type: GithubThreadKind::Issue,
        };
        let message = parse_message(&raw, None).unwrap();
        assert_eq!(message.id, "100");
        assert_eq!(message.thread_id, "github:acme/app:issue:10");
        assert_eq!(message.text, "Issue comment");
    }

    #[test]
    fn should_default_to_pr_thread_format_when_thread_type_is_omitted() {
        // 1:1 with upstream index.test.ts:1629 > "should default to PR
        // thread format when threadType is omitted".
        // The Rust port uses `GithubThreadKind::default()` to model
        // upstream's `threadType ?? "pr"` fallback.
        let raw = GithubRawMessage::IssueComment {
            comment: issue_comment(100, "Test comment", user(1, "testuser", false)),
            repository: repo(),
            pr_number: 42,
            thread_type: GithubThreadKind::default(),
        };
        let message = parse_message(&raw, None).unwrap();
        assert_eq!(message.thread_id, "github:acme/app:42");
        assert_eq!(GithubThreadKind::default(), GithubThreadKind::Pr);
    }

    #[test]
    fn should_parse_a_review_comment_raw_message_root_comment() {
        // 1:1 with upstream index.test.ts:1654 > "should parse a
        // review_comment raw message (root comment)".
        let raw = GithubRawMessage::ReviewComment {
            comment: review_comment(200, "Line comment", user(2, "reviewer", false)),
            repository: repo(),
            pr_number: 42,
        };
        let message = parse_message(&raw, None).unwrap();
        assert_eq!(message.id, "200");
        // Root comment -> reviewCommentId = comment.id
        assert_eq!(message.thread_id, "github:acme/app:42:rc:200");
    }

    #[test]
    fn should_parse_a_review_comment_raw_message_reply() {
        // 1:1 with upstream index.test.ts:1684 > "should parse a
        // review_comment raw message (reply)".
        let mut rc = review_comment(300, "Reply", user(2, "reviewer", false));
        rc.in_reply_to_id = Some(200);
        let raw = GithubRawMessage::ReviewComment {
            comment: rc,
            repository: repo(),
            pr_number: 42,
        };
        let message = parse_message(&raw, None).unwrap();
        assert_eq!(message.id, "300");
        // Reply -> uses in_reply_to_id as root.
        assert_eq!(message.thread_id, "github:acme/app:42:rc:200");
    }

    #[test]
    fn should_mark_edited_messages() {
        // 1:1 with upstream index.test.ts:1715 > "should mark edited
        // messages". Upstream model: `edited = created_at !== updated_at`,
        // with `editedAt = updated_at` when edited.
        let mut comment = issue_comment(100, "Edited", user(1, "testuser", false));
        comment.updated_at = "2024-01-02T00:00:00Z".to_string();
        let raw = GithubRawMessage::IssueComment {
            comment,
            repository: repo(),
            pr_number: 42,
            thread_type: GithubThreadKind::Pr,
        };
        let message = parse_message(&raw, None).unwrap();
        assert!(message.metadata.edited);
        assert_eq!(
            message.metadata.edited_at.as_deref(),
            Some("2024-01-02T00:00:00Z")
        );
    }

    #[test]
    fn should_not_mark_unedited_messages_as_edited() {
        // 1:1 with upstream index.test.ts:1742 > "should not mark
        // unedited messages as edited".
        let raw = GithubRawMessage::IssueComment {
            comment: issue_comment(100, "Not edited", user(1, "testuser", false)),
            repository: repo(),
            pr_number: 42,
            thread_type: GithubThreadKind::Pr,
        };
        let message = parse_message(&raw, None).unwrap();
        assert!(!message.metadata.edited);
        assert!(message.metadata.edited_at.is_none());
    }

    // ---------- describe("parseAuthor (via parseMessage)") ----------

    #[test]
    fn should_identify_bot_users() {
        // 1:1 with upstream index.test.ts:1769 > "should identify bot
        // users".
        let raw = GithubRawMessage::IssueComment {
            comment: issue_comment(100, "Automated comment", user(50, "dependabot[bot]", true)),
            repository: repo(),
            pr_number: 42,
            thread_type: GithubThreadKind::Pr,
        };
        let message = parse_message(&raw, None).unwrap();
        assert_eq!(message.author.is_bot, BotStatus::TRUE);
        assert_eq!(message.author.user_name, "dependabot[bot]");
        assert_eq!(message.author.user_id, "50");
    }

    #[test]
    fn should_detect_is_me_when_bot_user_id_matches() {
        // 1:1 with upstream index.test.ts:1795 > "should detect isMe
        // when botUserId matches".
        let raw = GithubRawMessage::IssueComment {
            comment: issue_comment(100, "My comment", user(50, "test-bot", true)),
            repository: repo(),
            pr_number: 42,
            thread_type: GithubThreadKind::Pr,
        };
        let message = parse_message(&raw, Some(50)).unwrap();
        assert!(message.author.is_me);
    }

    #[test]
    fn should_set_is_me_to_false_when_user_is_not_the_bot() {
        // 1:1 with upstream index.test.ts:1827 > "should set isMe to
        // false when user is not the bot".
        let raw = GithubRawMessage::IssueComment {
            comment: issue_comment(100, "Someone else", user(999, "someone", false)),
            repository: repo(),
            pr_number: 42,
            thread_type: GithubThreadKind::Pr,
        };
        let message = parse_message(&raw, None).unwrap();
        assert!(!message.author.is_me);
    }
}
