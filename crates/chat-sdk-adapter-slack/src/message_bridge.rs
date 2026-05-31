//! Slack/chat-sdk/Open Agents message bridge helpers.
//!
//! This module is deliberately pure and narrow: webhook parsing stays in
//! [`crate::webhook`], Slack Web API calls stay on [`crate::SlackAdapter`], and
//! this layer only converts already-parsed Slack message events into chat-sdk
//! messages plus deterministic Slack outbound updates for persisted Open
//! Agents UI parts.

use std::sync::Arc;

use chat_sdk_chat::markdown::{Node, paragraph, parse_markdown, root, text};
use chat_sdk_chat::message::Message;
use chat_sdk_chat::open_agent_message::{
    OpenAgentDataPart, OpenAgentGitDataStatus, OpenAgentMessagePart, OpenAgentMessagePartKind,
    OpenAgentToolPart, OpenAgentUiMessage,
};
use chat_sdk_chat::thread::Thread;
use chat_sdk_chat::types::{Adapter, Author, BotStatus, MessageMetadata};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::outbound::{
    SlackApprovalRequest, SlackCommitSummary, SlackGitSummaryStatus, SlackOutboundMessage,
    SlackQuestion, SlackQuestionOption, SlackQuestionPrompt, SlackRunContext, SlackToolEvent,
    SlackToolEventStatus, render_approval_request, render_commit_summary, render_progress_update,
    render_pull_request_summary, render_question_prompt, render_tool_event,
};
use crate::webhook::{SlackEventBase, SlackWebhookPayload};
use crate::{THREAD_ID_PREFIX, encode_thread_id};

/// Chat-sdk representation of a Slack message event.
#[derive(Debug, Clone, PartialEq)]
pub struct SlackInboundChatMessage {
    pub message: Message,
    pub channel_id: String,
    pub thread_ts: String,
    pub thread_id: String,
    pub is_dm: bool,
}

impl SlackInboundChatMessage {
    /// Build a chat-sdk [`Thread`] handle seeded with the converted message.
    pub fn thread(&self, adapter: Arc<dyn Adapter>) -> Thread {
        Thread::new(adapter, self.thread_id.clone())
            .with_channel_id(format!("{THREAD_ID_PREFIX}{}", self.channel_id))
            .with_is_dm(self.is_dm)
            .with_initial_message(self.message.clone())
            .with_current_message(self.message.clone())
    }
}

/// Convert parsed Slack app-mention/direct-message webhook payloads to a
/// chat-sdk message. Non-message Slack payloads return `None`.
pub fn inbound_chat_message_from_payload(
    payload: &SlackWebhookPayload,
) -> Option<SlackInboundChatMessage> {
    match payload {
        SlackWebhookPayload::AppMention(payload) => {
            Some(inbound_chat_message_from_event(&payload.base, true, false))
        }
        SlackWebhookPayload::DirectMessage(payload) => {
            Some(inbound_chat_message_from_event(&payload.base, false, true))
        }
        _ => None,
    }
}

/// Convert a Slack message event base into chat-sdk [`Message`] data.
pub fn inbound_chat_message_from_event(
    event: &SlackEventBase,
    is_mention: bool,
    is_dm: bool,
) -> SlackInboundChatMessage {
    let thread_id = encode_thread_id(&event.channel_id, &event.thread_ts);
    let user_id = event.user_id.clone().unwrap_or_default();
    let author = Author {
        full_name: if user_id.is_empty() {
            "Slack user".to_string()
        } else {
            user_id.clone()
        },
        is_bot: BotStatus::Known(false),
        is_me: false,
        user_id: user_id.clone(),
        user_name: user_id.clone(),
    };

    let mut message = Message::new(
        event.ts.clone(),
        thread_id.clone(),
        event.text.clone(),
        formatted_slack_text(&event.text),
        event.raw.clone(),
        author,
        MessageMetadata {
            date_sent: slack_timestamp_to_iso(&event.ts),
            edited: false,
            edited_at: None,
        },
        Vec::new(),
    );
    message.is_mention = Some(is_mention);
    if !user_id.is_empty() {
        message.user_key = Some(format!("slack:{user_id}"));
    }

    SlackInboundChatMessage {
        message,
        channel_id: event.channel_id.clone(),
        thread_ts: event.thread_ts.clone(),
        thread_id,
        is_dm,
    }
}

/// Render a persisted Open Agents UI message into one Slack outbound update.
///
/// The default context is deterministic for fixture and persistence
/// round-trips. Runtime callers that know the durable run id should use
/// [`render_open_agent_message_for_slack_with_context`] so interactive
/// question/approval action ids route back to the correct run.
pub fn render_open_agent_message_for_slack(
    message: &OpenAgentUiMessage,
) -> Option<SlackOutboundMessage> {
    let context = SlackRunContext::new(message.id.clone(), message.id.clone());
    render_open_agent_message_for_slack_with_context(message, &context)
}

/// Render a persisted Open Agents UI message with an explicit Slack action
/// context.
pub fn render_open_agent_message_for_slack_with_context(
    message: &OpenAgentUiMessage,
    context: &SlackRunContext,
) -> Option<SlackOutboundMessage> {
    let updates = message
        .parts
        .iter()
        .filter_map(|part| render_open_agent_part_for_slack(part, context));

    combine_slack_updates(updates)
}

/// Render one persisted Open Agents UI part into a Slack outbound update.
pub fn render_open_agent_part_for_slack(
    part: &OpenAgentMessagePart,
    context: &SlackRunContext,
) -> Option<SlackOutboundMessage> {
    match part.classify() {
        OpenAgentMessagePartKind::Text(part) => Some(render_progress_update(&part.text)),
        OpenAgentMessagePartKind::Tool(tool) => render_tool_part_for_slack(&tool),
        OpenAgentMessagePartKind::Question(question) => {
            render_question_part_for_slack(context, &question.tool)
        }
        OpenAgentMessagePartKind::Approval(approval) => {
            render_approval_part_for_slack(context, &approval.tool)
        }
        OpenAgentMessagePartKind::Data(data) => render_data_part_for_slack(data),
        OpenAgentMessagePartKind::Other(_) => None,
    }
}

fn combine_slack_updates(
    updates: impl IntoIterator<Item = SlackOutboundMessage>,
) -> Option<SlackOutboundMessage> {
    let mut text = Vec::new();
    let mut blocks = Vec::new();

    for update in updates {
        if !update.text.trim().is_empty() {
            text.push(update.text);
        }
        blocks.extend(update.blocks);
    }

    if text.is_empty() && blocks.is_empty() {
        None
    } else {
        Some(SlackOutboundMessage {
            text: text.join("\n"),
            blocks,
        })
    }
}

fn formatted_slack_text(markdown_text: &str) -> chat_sdk_chat::markdown::Root {
    match parse_markdown(markdown_text) {
        Ok(Node::Root(root)) => root,
        Ok(node) => root(vec![node]),
        Err(_) => root(vec![Node::Paragraph(paragraph(vec![Node::Text(text(
            markdown_text,
        ))]))]),
    }
}

fn slack_timestamp_to_iso(ts: &str) -> String {
    let mut parts = ts.splitn(2, '.');
    let Some(seconds) = parts.next().and_then(|value| value.parse::<i128>().ok()) else {
        return ts.to_string();
    };
    let nanos = parts
        .next()
        .map(|fraction| {
            let mut padded = fraction.chars().take(9).collect::<String>();
            while padded.len() < 9 {
                padded.push('0');
            }
            padded.parse::<i128>().unwrap_or(0)
        })
        .unwrap_or(0);
    let total_nanos = seconds.saturating_mul(1_000_000_000).saturating_add(nanos);

    match OffsetDateTime::from_unix_timestamp_nanos(total_nanos) {
        Ok(timestamp) => timestamp
            .format(&Rfc3339)
            .unwrap_or_else(|_| ts.to_string()),
        Err(_) => ts.to_string(),
    }
}

fn render_tool_part_for_slack(tool: &OpenAgentToolPart) -> Option<SlackOutboundMessage> {
    let name = tool.tool_name.as_deref().unwrap_or("tool");
    let event = match tool.state.as_deref() {
        Some("output-available") => Some(SlackToolEvent {
            tool_call_id: tool_call_id(tool),
            tool_name: name.to_string(),
            status: SlackToolEventStatus::Finished,
            summary: "completed".to_string(),
            details: tool.input.as_ref().map(json_to_pretty_string),
            output: tool.output.as_ref().map(json_to_pretty_string),
            error: None,
        }),
        Some("output-error") => Some(SlackToolEvent {
            tool_call_id: tool_call_id(tool),
            tool_name: name.to_string(),
            status: SlackToolEventStatus::Error,
            summary: "failed".to_string(),
            details: tool.input.as_ref().map(json_to_pretty_string),
            output: None,
            error: tool
                .error_text
                .clone()
                .or_else(|| tool.output.as_ref().map(json_to_pretty_string)),
        }),
        Some("output-denied") => Some(SlackToolEvent {
            tool_call_id: tool_call_id(tool),
            tool_name: name.to_string(),
            status: SlackToolEventStatus::Denied,
            summary: "denied".to_string(),
            details: tool.input.as_ref().map(json_to_pretty_string),
            output: None,
            error: None,
        }),
        Some("input-available") | Some("input-streaming") => Some(SlackToolEvent {
            tool_call_id: tool_call_id(tool),
            tool_name: name.to_string(),
            status: SlackToolEventStatus::WaitingForInput,
            summary: "waiting for input".to_string(),
            details: tool.input.as_ref().map(json_to_pretty_string),
            output: None,
            error: None,
        }),
        _ => None,
    }?;

    Some(render_tool_event(&event))
}

fn render_question_part_for_slack(
    context: &SlackRunContext,
    tool: &OpenAgentToolPart,
) -> Option<SlackOutboundMessage> {
    let questions = tool
        .input
        .as_ref()
        .and_then(|input| input.get("questions"))
        .and_then(|questions| questions.as_array())?
        .iter()
        .map(slack_question_from_value)
        .collect::<Option<Vec<_>>>()?;

    if questions.is_empty() {
        return None;
    }

    Some(render_question_prompt(
        context,
        &SlackQuestionPrompt {
            tool_call_id: tool_call_id(tool),
            questions,
        },
    ))
}

fn render_approval_part_for_slack(
    context: &SlackRunContext,
    tool: &OpenAgentToolPart,
) -> Option<SlackOutboundMessage> {
    let name = tool.tool_name.as_deref().unwrap_or("tool");
    let approval_id = tool
        .approval
        .as_ref()
        .map(|approval| approval.id.as_str())
        .unwrap_or("approval");

    Some(render_approval_request(
        context,
        &SlackApprovalRequest {
            approval_id: approval_id.to_string(),
            tool_call_id: tool_call_id(tool),
            tool_name: name.to_string(),
            title: format!("Approve `{name}` execution"),
            details: tool.input.as_ref().map(json_to_pretty_string),
        },
    ))
}

fn render_data_part_for_slack(data: OpenAgentDataPart) -> Option<SlackOutboundMessage> {
    match data {
        OpenAgentDataPart::Commit(commit) => Some(render_commit_summary(&SlackCommitSummary {
            status: slack_git_status(commit.status),
            committed: commit.committed,
            pushed: commit.pushed,
            commit_message: commit.commit_message,
            commit_sha: commit.commit_sha,
            url: commit.url,
            error: commit.error,
        })),
        OpenAgentDataPart::PullRequest(pr) => Some(render_pull_request_summary(
            &crate::outbound::SlackPullRequestSummary {
                status: slack_git_status(pr.status),
                created: pr.created,
                synced_existing: pr.synced_existing,
                pr_number: pr.pr_number,
                url: pr.url,
                error: pr.error,
                skip_reason: pr.skip_reason,
            },
        )),
        OpenAgentDataPart::WorkspaceStatus(status) => Some(render_progress_update(&status.message)),
        OpenAgentDataPart::Snippet(snippet) => Some(render_progress_update(&format!(
            "Snippet `{}`",
            snippet.filename
        ))),
        OpenAgentDataPart::Other { .. } => None,
    }
}

fn slack_question_from_value(value: &serde_json::Value) -> Option<SlackQuestion> {
    let question = value.get("question")?.as_str()?.to_string();
    let header = value
        .get("header")
        .and_then(|header| header.as_str())
        .unwrap_or("Question")
        .to_string();
    let options = value
        .get("options")
        .and_then(|options| options.as_array())
        .into_iter()
        .flatten()
        .map(|option| {
            Some(SlackQuestionOption {
                label: option.get("label")?.as_str()?.to_string(),
                description: option
                    .get("description")
                    .and_then(|description| description.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(SlackQuestion {
        header,
        question,
        options,
        multi_select: value
            .get("multiSelect")
            .and_then(|multi_select| multi_select.as_bool())
            .unwrap_or(false),
    })
}

fn tool_call_id(tool: &OpenAgentToolPart) -> String {
    tool.tool_call_id
        .clone()
        .unwrap_or_else(|| "tool-call".to_string())
}

fn slack_git_status(status: OpenAgentGitDataStatus) -> SlackGitSummaryStatus {
    match status {
        OpenAgentGitDataStatus::Pending => SlackGitSummaryStatus::Pending,
        OpenAgentGitDataStatus::Success => SlackGitSummaryStatus::Success,
        OpenAgentGitDataStatus::Error => SlackGitSummaryStatus::Error,
        OpenAgentGitDataStatus::Skipped => SlackGitSummaryStatus::Skipped,
    }
}

fn json_to_pretty_string(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use chat_sdk_chat::open_agent_message::OpenAgentUiMessage;

    use crate::message_bridge::{
        inbound_chat_message_from_payload, render_open_agent_message_for_slack,
    };
    use crate::webhook::{SlackParseOptions, parse_slack_webhook_body};

    #[test]
    fn slack_app_mention_fixture_converts_to_chat_message_and_thread() {
        let payload = parse_slack_webhook_body(
            include_str!("fixtures/slack-app-mention.json"),
            &SlackParseOptions {
                content_type: Some("application/json"),
                headers: None,
            },
        )
        .unwrap();
        let inbound = inbound_chat_message_from_payload(&payload).unwrap();

        assert_eq!(inbound.channel_id, "C123");
        assert_eq!(inbound.thread_ts, "1717113600.000100");
        assert_eq!(inbound.thread_id, "slack:C123:1717113600.000100");
        assert_eq!(inbound.message.id, "1717113600.000100");
        assert_eq!(inbound.message.thread_id, inbound.thread_id);
        assert_eq!(inbound.message.text, "<@U999BOT> run cargo test");
        assert_eq!(inbound.message.is_mention, Some(true));
        assert_eq!(inbound.message.user_key.as_deref(), Some("slack:U123"));
        assert_eq!(
            inbound.message.metadata.date_sent,
            "2024-05-31T00:00:00.0001Z"
        );
    }

    #[test]
    fn slack_dm_fixture_converts_to_chat_message_and_thread() {
        let payload = parse_slack_webhook_body(
            include_str!("fixtures/slack-dm.json"),
            &SlackParseOptions {
                content_type: Some("application/json"),
                headers: None,
            },
        )
        .unwrap();
        let inbound = inbound_chat_message_from_payload(&payload).unwrap();

        assert!(inbound.is_dm);
        assert_eq!(inbound.channel_id, "D123");
        assert_eq!(inbound.thread_ts, "1717113660.000200");
        assert_eq!(inbound.thread_id, "slack:D123:1717113660.000200");
        assert_eq!(inbound.message.text, "please summarize the branch");
        assert_eq!(inbound.message.is_mention, Some(false));
    }

    #[test]
    fn open_agent_persisted_fixture_renders_tool_question_approval_and_final_parts_for_slack() {
        let messages: Vec<OpenAgentUiMessage> = serde_json::from_str(include_str!(
            "../../chat-sdk-chat/src/fixtures/open-agent-persisted-messages.json"
        ))
        .unwrap();

        let tool_update = render_open_agent_message_for_slack(&messages[1]).unwrap();
        assert!(tool_update.text.contains("Tool finished: bash - completed"));
        assert!(tool_update.text.contains("Question for you"));
        assert!(
            tool_update
                .blocks
                .iter()
                .any(|block| { block.to_string().contains("Which branch should I use?") })
        );

        let approval_update = render_open_agent_message_for_slack(&messages[2]).unwrap();
        assert!(approval_update.text.contains("Approval required: bash"));
        assert!(
            approval_update
                .blocks
                .iter()
                .any(|block| { block.to_string().contains("approval-1") })
        );

        let final_update = render_open_agent_message_for_slack(&messages[3]).unwrap();
        assert!(final_update.text.contains("Done. Tests passed."));
        assert!(final_update.text.contains("Commit success"));
        assert!(final_update.text.contains("Pull request success: #42"));
        assert!(final_update.blocks.iter().any(|block| {
            block
                .to_string()
                .contains("https://github.com/example/repo/commit/abc123")
        }));
        assert!(final_update.blocks.iter().any(|block| {
            block
                .to_string()
                .contains("https://github.com/example/repo/pull/42")
        }));
    }
}
