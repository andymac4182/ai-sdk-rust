//! Slack outbound rendering for Open Agents-style remote-agent events.
//!
//! This module is intentionally adapter-local: it renders durable run events
//! into Slack fallback text + Block Kit payloads, builds the Slack Web API
//! request bodies used by mocked tests, and offers a small dispatcher that
//! routes fallback text through the existing adapter post/update/delete/
//! reaction/ephemeral/typing methods.

use crate::api::{
    SlackApiError, SlackMessageOptions, slack_delete_body, slack_ephemeral_body,
    slack_message_body, slack_update_body,
};
use chat_sdk_chat::types::{Adapter, AdapterResult, EphemeralMessage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Default minimum time between Slack message updates.
///
/// Slack does not publish a hard per-message edit limit, but the Web API rate
/// limits message updates aggressively enough that streaming should coalesce.
pub const DEFAULT_UPDATE_INTERVAL_MS: u64 = 1000;

/// Reserved assistant markdown href prefix for workspace-file links.
pub const WORKSPACE_FILE_HREF_PREFIX: &str = "#workspace-file=";

/// Context encoded into every interactive Slack action id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackRunContext {
    #[serde(rename = "runId")]
    pub run_id: String,
    #[serde(rename = "messageId")]
    pub message_id: String,
}

impl SlackRunContext {
    pub fn new(run_id: impl Into<String>, message_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            message_id: message_id.into(),
        }
    }
}

/// Supported Slack action kinds emitted by this renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlackOutboundActionKind {
    Approve,
    Deny,
    QuestionAnswer,
    QuestionOther,
    QuestionSelect,
    QuestionSubmit,
    QuestionDecline,
}

impl SlackOutboundActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Deny => "deny",
            Self::QuestionAnswer => "question_answer",
            Self::QuestionOther => "question_other",
            Self::QuestionSelect => "question_select",
            Self::QuestionSubmit => "question_submit",
            Self::QuestionDecline => "question_decline",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "approve" => Some(Self::Approve),
            "deny" => Some(Self::Deny),
            "question_answer" => Some(Self::QuestionAnswer),
            "question_other" => Some(Self::QuestionOther),
            "question_select" => Some(Self::QuestionSelect),
            "question_submit" => Some(Self::QuestionSubmit),
            "question_decline" => Some(Self::QuestionDecline),
            _ => None,
        }
    }
}

/// Decoded form of an Open Agents Slack action id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackOutboundActionId {
    pub kind: SlackOutboundActionKind,
    pub run_id: String,
    pub message_id: String,
    pub target: String,
}

/// Encode an action id with run id, message id, and target.
///
/// Wire shape: `oa:v1:<kind>:<run>:<message>:<target>`, with each segment
/// percent-encoded so ids remain split-safe and parseable.
pub fn encode_slack_action_id(
    kind: SlackOutboundActionKind,
    context: &SlackRunContext,
    target: &str,
) -> String {
    format!(
        "oa:v1:{}:{}:{}:{}",
        kind.as_str(),
        percent_encode_segment(&context.run_id),
        percent_encode_segment(&context.message_id),
        percent_encode_segment(target)
    )
}

/// Decode an Open Agents Slack action id.
pub fn decode_slack_action_id(value: &str) -> Option<SlackOutboundActionId> {
    let mut parts = value.splitn(6, ':');
    if parts.next()? != "oa" || parts.next()? != "v1" {
        return None;
    }
    let kind = SlackOutboundActionKind::parse(parts.next()?)?;
    let run_id = percent_decode_segment(parts.next()?)?;
    let message_id = percent_decode_segment(parts.next()?)?;
    let target = percent_decode_segment(parts.next()?)?;
    Some(SlackOutboundActionId {
        kind,
        run_id,
        message_id,
        target,
    })
}

/// Fully rendered Slack message: fallback text plus optional Block Kit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlackOutboundMessage {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Value>,
}

impl SlackOutboundMessage {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            blocks: Vec::new(),
        }
    }

    pub fn with_blocks(text: impl Into<String>, blocks: Vec<Value>) -> Self {
        Self {
            text: text.into(),
            blocks,
        }
    }

    pub fn block_value(&self) -> Option<Value> {
        if self.blocks.is_empty() {
            None
        } else {
            Some(Value::Array(self.blocks.clone()))
        }
    }

    pub fn message_options(&self, channel: &str, thread_ts: Option<&str>) -> SlackMessageOptions {
        SlackMessageOptions {
            blocks: self.block_value(),
            channel: channel.to_string(),
            text: Some(self.text.clone()),
            thread_ts: thread_ts.map(str::to_string),
            unfurl_links: Some(false),
            unfurl_media: Some(false),
            ..SlackMessageOptions::default()
        }
    }

    pub fn post_body(
        &self,
        channel: &str,
        thread_ts: Option<&str>,
    ) -> Result<serde_json::Map<String, Value>, SlackApiError> {
        slack_message_body(&self.message_options(channel, thread_ts))
    }

    pub fn update_body(
        &self,
        channel: &str,
        ts: &str,
    ) -> Result<serde_json::Map<String, Value>, SlackApiError> {
        slack_update_body(&self.message_options(channel, None), ts)
    }

    pub fn ephemeral_body(
        &self,
        channel: &str,
        thread_ts: Option<&str>,
        user_id: &str,
    ) -> Result<serde_json::Map<String, Value>, SlackApiError> {
        slack_ephemeral_body(&self.message_options(channel, thread_ts), user_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackApprovalRequest {
    #[serde(rename = "approvalId")]
    pub approval_id: String,
    #[serde(rename = "toolCallId")]
    pub tool_call_id: String,
    #[serde(rename = "toolName")]
    pub tool_name: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackQuestionPrompt {
    #[serde(rename = "toolCallId")]
    pub tool_call_id: String,
    pub questions: Vec<SlackQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackQuestion {
    pub header: String,
    pub question: String,
    pub options: Vec<SlackQuestionOption>,
    #[serde(rename = "multiSelect", default)]
    pub multi_select: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackQuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackToolEventStatus {
    Pending,
    Running,
    WaitingForApproval,
    WaitingForInput,
    Finished,
    Error,
    Denied,
}

impl SlackToolEventStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::WaitingForApproval => "waiting for approval",
            Self::WaitingForInput => "waiting for input",
            Self::Finished => "finished",
            Self::Error => "failed",
            Self::Denied => "denied",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackToolEvent {
    #[serde(rename = "toolCallId")]
    pub tool_call_id: String,
    #[serde(rename = "toolName")]
    pub tool_name: String,
    pub status: SlackToolEventStatus,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SlackPlanItemStatus {
    Todo,
    InProgress,
    Completed,
    Error,
}

impl SlackPlanItemStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in-progress",
            Self::Completed => "completed",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackPlanItem {
    pub content: String,
    pub status: SlackPlanItemStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackPlanUpdate {
    pub title: String,
    pub items: Vec<SlackPlanItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlackGitSummaryStatus {
    Pending,
    Success,
    Error,
    Skipped,
}

impl SlackGitSummaryStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Success => "success",
            Self::Error => "error",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackCommitSummary {
    pub status: SlackGitSummaryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed: Option<bool>,
    #[serde(
        rename = "commitMessage",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub commit_message: Option<String>,
    #[serde(rename = "commitSha", default, skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackPullRequestSummary {
    pub status: SlackGitSummaryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<bool>,
    #[serde(
        rename = "syncedExisting",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub synced_existing: Option<bool>,
    #[serde(rename = "prNumber", default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(
        rename = "skipReason",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub skip_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlackRunTerminalStatus {
    Finished,
    Failed,
    Canceled,
}

impl SlackRunTerminalStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }
}

pub fn render_run_started(summary: Option<&str>) -> SlackOutboundMessage {
    let text = match summary {
        Some(summary) if !summary.trim().is_empty() => format!("Run started: {}", summary.trim()),
        _ => "Run started".to_string(),
    };
    SlackOutboundMessage::with_blocks(
        text.clone(),
        vec![section_block(format!("*{}*", escape_mrkdwn(&text)))],
    )
}

pub fn render_progress_update(text: &str) -> SlackOutboundMessage {
    let fallback = if text.trim().is_empty() {
        "Working...".to_string()
    } else {
        text.trim().to_string()
    };
    SlackOutboundMessage::with_blocks(
        fallback.clone(),
        vec![section_block(escape_mrkdwn(&truncate_chars(
            &fallback, 3000,
        )))],
    )
}

pub fn render_tool_event(event: &SlackToolEvent) -> SlackOutboundMessage {
    let fallback = format!(
        "Tool {}: {} - {}",
        event.status.label(),
        event.tool_name,
        event.summary
    );
    let mut text = format!(
        "*Tool {}: {}*\n{}",
        event.status.label(),
        escape_mrkdwn(&event.tool_name),
        escape_mrkdwn(&event.summary)
    );
    if let Some(details) = event.details.as_deref().filter(|v| !v.trim().is_empty()) {
        text.push('\n');
        text.push_str(&escape_mrkdwn(&truncate_chars(details, 1200)));
    }
    if let Some(output) = event.output.as_deref().filter(|v| !v.trim().is_empty()) {
        text.push('\n');
        text.push_str(&code_block(output));
    }
    if let Some(error) = event.error.as_deref().filter(|v| !v.trim().is_empty()) {
        text.push_str("\n*Error:* ");
        text.push_str(&escape_mrkdwn(&truncate_chars(error, 1200)));
    }
    SlackOutboundMessage::with_blocks(fallback, vec![section_block(text)])
}

pub fn render_approval_request(
    context: &SlackRunContext,
    request: &SlackApprovalRequest,
) -> SlackOutboundMessage {
    let fallback = format!(
        "Approval required: {} - {}",
        request.tool_name, request.title
    );
    let mut blocks = vec![section_block(format!(
        "*Approval required: {}*\n{}",
        escape_mrkdwn(&request.tool_name),
        escape_mrkdwn(&request.title)
    ))];
    if let Some(details) = request
        .details
        .as_deref()
        .filter(|details| !details.trim().is_empty())
    {
        blocks.push(context_block(vec![mrkdwn_text(format!(
            "*Details:* {}",
            escape_mrkdwn(&truncate_chars(details, 900))
        ))]));
    }
    let approve_id = encode_slack_action_id(
        SlackOutboundActionKind::Approve,
        context,
        &request.approval_id,
    );
    let deny_id =
        encode_slack_action_id(SlackOutboundActionKind::Deny, context, &request.approval_id);
    blocks.push(json!({
        "type": "actions",
        "elements": [
            button_element("Approve", &approve_id, &request.approval_id, Some("primary")),
            button_element("Deny", &deny_id, &request.approval_id, Some("danger"))
        ]
    }));
    SlackOutboundMessage::with_blocks(fallback, blocks)
}

pub fn render_question_prompt(
    context: &SlackRunContext,
    prompt: &SlackQuestionPrompt,
) -> SlackOutboundMessage {
    let fallback = question_fallback(prompt);
    let mut blocks = vec![section_block(format!(
        "*{}*",
        if prompt.questions.len() == 1 {
            "Question for you"
        } else {
            "Questions for you"
        }
    ))];

    for (question_index, question) in prompt.questions.iter().enumerate() {
        let target_prefix = format!("{}/q{}", prompt.tool_call_id, question_index);
        let header = if question.header.trim().is_empty() {
            format!("Question {}", question_index + 1)
        } else {
            question.header.clone()
        };
        blocks.push(section_block(format!(
            "*{}*\n{}",
            escape_mrkdwn(&header),
            escape_mrkdwn(&question.question)
        )));

        let option_context: Vec<Value> = question
            .options
            .iter()
            .map(|option| {
                mrkdwn_text(format!(
                    "*{}:* {}",
                    escape_mrkdwn(&option.label),
                    escape_mrkdwn(&truncate_chars(&option.description, 120))
                ))
            })
            .collect();
        if !option_context.is_empty() {
            blocks.push(context_block(option_context));
        }

        if question.multi_select {
            blocks.push(render_multi_select_question_actions(
                context,
                &target_prefix,
                question,
            ));
        } else {
            blocks.push(render_single_select_question_actions(
                context,
                &target_prefix,
                question,
            ));
        }
    }

    let decline_id = encode_slack_action_id(
        SlackOutboundActionKind::QuestionDecline,
        context,
        &prompt.tool_call_id,
    );
    blocks.push(json!({
        "type": "actions",
        "elements": [
            button_element("Decline", &decline_id, &prompt.tool_call_id, Some("danger"))
        ]
    }));
    SlackOutboundMessage::with_blocks(fallback, blocks)
}

pub fn render_plan_update(plan: &SlackPlanUpdate) -> SlackOutboundMessage {
    let fallback = plan_fallback(plan);
    let mut lines = vec![format!("*{}*", escape_mrkdwn(&plan.title))];
    for item in &plan.items {
        lines.push(format!(
            "- `{:>11}` {}",
            item.status.label(),
            escape_mrkdwn(&item.content)
        ));
    }
    SlackOutboundMessage::with_blocks(fallback, vec![section_block(lines.join("\n"))])
}

pub fn render_run_error(error: &str) -> SlackOutboundMessage {
    let fallback = format!("Run failed: {}", error.trim());
    SlackOutboundMessage::with_blocks(
        fallback.clone(),
        vec![section_block(format!(
            "*Run failed*\n{}",
            escape_mrkdwn(&truncate_chars(error.trim(), 2500))
        ))],
    )
}

pub fn render_run_terminal(
    status: SlackRunTerminalStatus,
    summary: Option<&str>,
) -> SlackOutboundMessage {
    let mut fallback = format!("Run {}", status.label());
    if let Some(summary) = summary.filter(|summary| !summary.trim().is_empty()) {
        fallback.push_str(": ");
        fallback.push_str(summary.trim());
    }
    SlackOutboundMessage::with_blocks(
        fallback.clone(),
        vec![section_block(format!("*{}*", escape_mrkdwn(&fallback)))],
    )
}

/// Builds the reserved workspace-file href used by Open Agents assistant text.
pub fn build_workspace_file_href(file_path: &str) -> String {
    format!(
        "{WORKSPACE_FILE_HREF_PREFIX}{}",
        normalize_workspace_file_path(file_path)
    )
}

/// Parses a reserved workspace-file href back to a normalized path.
pub fn parse_workspace_file_href(href: Option<&str>) -> Option<String> {
    let href = href?;
    let encoded = href.strip_prefix(WORKSPACE_FILE_HREF_PREFIX)?;
    let file_path = normalize_workspace_file_path(&percent_decode_lossy(encoded));
    if file_path.is_empty() {
        None
    } else {
        Some(file_path)
    }
}

/// Prompt section that teaches assistants to emit whole-file workspace links.
pub fn assistant_file_link_prompt() -> String {
    [
        "When you mention a workspace file path in assistant text, render it as a markdown link using this exact format:",
        &format!(
            "- `[path/to/file.ts]({})`",
            build_workspace_file_href("path/to/file.ts")
        ),
        "- Use the repo-relative file path as both the visible link text and the path inside the link.",
        "- Whole-file links only for now. Do not include line numbers or ranges.",
        "- Do not use this format for URLs or anything that is not a real workspace file path.",
        "- If you are not sure of the exact file path, do not invent one.",
    ]
    .join("\n")
}

/// Formats token counts for compact Open Agents status rendering.
pub fn format_tokens(tokens: u64) -> String {
    if tokens >= 999_950_000_000 {
        return format!("{:.1}t", tokens as f64 / 1_000_000_000_000.0);
    }
    if tokens >= 999_950_000 {
        return format!("{:.1}b", tokens as f64 / 1_000_000_000.0);
    }
    if tokens >= 999_950 {
        return format!("{:.1}m", tokens as f64 / 1_000_000.0);
    }
    if tokens >= 1_000 {
        return format!("{:.1}k", tokens as f64 / 1_000.0);
    }
    tokens.to_string()
}

pub fn render_commit_summary(summary: &SlackCommitSummary) -> SlackOutboundMessage {
    let mut fallback = format!("Commit {}", summary.status.label());
    if let Some(message) = summary
        .commit_message
        .as_deref()
        .filter(|message| !message.trim().is_empty())
    {
        fallback.push_str(": ");
        fallback.push_str(message.trim());
    }
    let mut lines = vec![format!("*Commit {}*", summary.status.label())];
    if let Some(message) = &summary.commit_message {
        lines.push(format!("Message: {}", escape_mrkdwn(message)));
    }
    if let Some(sha) = &summary.commit_sha {
        lines.push(format!("SHA: `{}`", escape_backticks(sha)));
    }
    if let Some(url) = &summary.url {
        lines.push(format!("<{}|View commit>", url));
    }
    if let Some(error) = &summary.error {
        lines.push(format!("Error: {}", escape_mrkdwn(error)));
    }
    SlackOutboundMessage::with_blocks(fallback, vec![section_block(lines.join("\n"))])
}

pub fn render_pull_request_summary(summary: &SlackPullRequestSummary) -> SlackOutboundMessage {
    let fallback = match summary.pr_number {
        Some(number) => format!("Pull request {}: #{}", summary.status.label(), number),
        None => format!("Pull request {}", summary.status.label()),
    };
    let mut lines = vec![format!("*Pull request {}*", summary.status.label())];
    if let Some(number) = summary.pr_number {
        lines.push(format!("Number: #{}", number));
    }
    if let Some(url) = &summary.url {
        lines.push(format!("<{}|Open pull request>", url));
    }
    if let Some(reason) = &summary.skip_reason {
        lines.push(format!("Skipped: {}", escape_mrkdwn(reason)));
    }
    if let Some(error) = &summary.error {
        lines.push(format!("Error: {}", escape_mrkdwn(error)));
    }
    SlackOutboundMessage::with_blocks(fallback, vec![section_block(lines.join("\n"))])
}

/// Request body for Slack reaction calls.
pub fn slack_reaction_body(
    channel: &str,
    timestamp: &str,
    name: &str,
) -> serde_json::Map<String, Value> {
    let mut body = serde_json::Map::new();
    body.insert("channel".to_string(), Value::String(channel.to_string()));
    body.insert(
        "timestamp".to_string(),
        Value::String(timestamp.to_string()),
    );
    body.insert("name".to_string(), Value::String(name.to_string()));
    body
}

/// Request body for `assistant.threads.setStatus`.
pub fn slack_typing_status_body(
    channel: &str,
    thread_ts: &str,
    status: Option<&str>,
) -> serde_json::Map<String, Value> {
    let display_status = status.unwrap_or("Typing...");
    let mut body = serde_json::Map::new();
    body.insert("channel_id".to_string(), Value::String(channel.to_string()));
    body.insert(
        "thread_ts".to_string(),
        Value::String(thread_ts.to_string()),
    );
    body.insert(
        "status".to_string(),
        Value::String(display_status.to_string()),
    );
    body.insert(
        "loading_messages".to_string(),
        Value::Array(vec![Value::String(display_status.to_string())]),
    );
    body
}

/// Request body for deleting a rendered outbound message.
pub fn slack_outbound_delete_body(channel: &str, ts: &str) -> serde_json::Map<String, Value> {
    slack_delete_body(channel, ts)
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoalescedUpdate {
    SendNow(SlackOutboundMessage),
    Deferred { due_at_ms: u64 },
}

#[derive(Debug, Clone)]
pub struct SlackUpdateCoalescer {
    interval_ms: u64,
    last_sent_at_ms: Option<u64>,
    pending: Option<SlackOutboundMessage>,
}

impl Default for SlackUpdateCoalescer {
    fn default() -> Self {
        Self::new(DEFAULT_UPDATE_INTERVAL_MS)
    }
}

impl SlackUpdateCoalescer {
    pub fn new(interval_ms: u64) -> Self {
        Self {
            interval_ms,
            last_sent_at_ms: None,
            pending: None,
        }
    }

    pub fn push(&mut self, now_ms: u64, message: SlackOutboundMessage) -> CoalescedUpdate {
        if self
            .last_sent_at_ms
            .is_none_or(|last| now_ms.saturating_sub(last) >= self.interval_ms)
        {
            self.last_sent_at_ms = Some(now_ms);
            self.pending = None;
            return CoalescedUpdate::SendNow(message);
        }

        let due_at_ms = self.last_sent_at_ms.unwrap_or(now_ms) + self.interval_ms;
        self.pending = Some(message);
        CoalescedUpdate::Deferred { due_at_ms }
    }

    pub fn take_due(&mut self, now_ms: u64) -> Option<SlackOutboundMessage> {
        let due_at_ms = self.last_sent_at_ms? + self.interval_ms;
        if now_ms < due_at_ms {
            return None;
        }
        let pending = self.pending.take()?;
        self.last_sent_at_ms = Some(now_ms);
        Some(pending)
    }

    pub fn flush(&mut self, now_ms: u64) -> Option<SlackOutboundMessage> {
        let pending = self.pending.take()?;
        self.last_sent_at_ms = Some(now_ms);
        Some(pending)
    }
}

/// Adapter-method dispatcher for fallback-text outbound operations.
#[derive(Debug, Clone, Copy)]
pub struct SlackOutboundDispatcher<'a> {
    adapter: &'a dyn Adapter,
}

impl<'a> SlackOutboundDispatcher<'a> {
    pub fn new(adapter: &'a dyn Adapter) -> Self {
        Self { adapter }
    }

    pub async fn post(
        &self,
        thread_id: &str,
        message: &SlackOutboundMessage,
    ) -> AdapterResult<String> {
        self.adapter.post_message(thread_id, &message.text).await
    }

    pub async fn update(
        &self,
        thread_id: &str,
        message_id: &str,
        message: &SlackOutboundMessage,
    ) -> AdapterResult<String> {
        self.adapter
            .edit_message(thread_id, message_id, &message.text)
            .await
    }

    pub async fn delete(&self, thread_id: &str, message_id: &str) -> AdapterResult<()> {
        self.adapter.delete_message(thread_id, message_id).await
    }

    pub async fn add_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> AdapterResult<()> {
        self.adapter
            .add_reaction(thread_id, message_id, emoji)
            .await
    }

    pub async fn remove_reaction(
        &self,
        thread_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> AdapterResult<()> {
        self.adapter
            .remove_reaction(thread_id, message_id, emoji)
            .await
    }

    pub async fn ephemeral(
        &self,
        thread_id: &str,
        user_id: &str,
        message: &SlackOutboundMessage,
    ) -> AdapterResult<EphemeralMessage> {
        self.adapter
            .post_ephemeral(thread_id, user_id, &message.text)
            .await
    }

    pub async fn typing(&self, thread_id: &str, status: Option<&str>) -> AdapterResult<()> {
        self.adapter.start_typing(thread_id, status).await
    }
}

fn render_single_select_question_actions(
    context: &SlackRunContext,
    target_prefix: &str,
    question: &SlackQuestion,
) -> Value {
    let mut elements: Vec<Value> = question
        .options
        .iter()
        .take(4)
        .enumerate()
        .map(|(option_index, option)| {
            let target = format!("{target_prefix}/option{option_index}");
            let action_id =
                encode_slack_action_id(SlackOutboundActionKind::QuestionAnswer, context, &target);
            button_element(&option.label, &action_id, &target, None)
        })
        .collect();
    let other_target = format!("{target_prefix}/other");
    let other_id = encode_slack_action_id(
        SlackOutboundActionKind::QuestionOther,
        context,
        &other_target,
    );
    elements.push(button_element("Other...", &other_id, &other_target, None));
    json!({
        "type": "actions",
        "elements": elements
    })
}

fn render_multi_select_question_actions(
    context: &SlackRunContext,
    target_prefix: &str,
    question: &SlackQuestion,
) -> Value {
    let select_id = encode_slack_action_id(
        SlackOutboundActionKind::QuestionSelect,
        context,
        target_prefix,
    );
    let submit_id = encode_slack_action_id(
        SlackOutboundActionKind::QuestionSubmit,
        context,
        target_prefix,
    );
    let options: Vec<Value> = question
        .options
        .iter()
        .take(10)
        .enumerate()
        .map(|(option_index, option)| {
            json!({
                "text": plain_text(&option.label),
                "description": plain_text(&truncate_chars(&option.description, 75)),
                "value": format!("{target_prefix}/option{option_index}")
            })
        })
        .collect();
    json!({
        "type": "actions",
        "elements": [
            {
                "type": "checkboxes",
                "action_id": select_id,
                "options": options
            },
            button_element("Submit", &submit_id, target_prefix, Some("primary"))
        ]
    })
}

fn question_fallback(prompt: &SlackQuestionPrompt) -> String {
    let mut lines = vec![if prompt.questions.len() == 1 {
        "Question for you".to_string()
    } else {
        "Questions for you".to_string()
    }];
    for (index, question) in prompt.questions.iter().enumerate() {
        lines.push(format!("{}. {}", index + 1, question.question));
        if !question.options.is_empty() {
            let options = question
                .options
                .iter()
                .map(|option| format!("{} - {}", option.label, option.description))
                .collect::<Vec<_>>()
                .join("; ");
            lines.push(format!("Options: {options}"));
        }
    }
    lines.join("\n")
}

fn plan_fallback(plan: &SlackPlanUpdate) -> String {
    let mut lines = vec![plan.title.clone()];
    for item in &plan.items {
        lines.push(format!("- ({}) {}", item.status.label(), item.content));
    }
    lines.join("\n")
}

fn section_block(text: impl Into<String>) -> Value {
    json!({
        "type": "section",
        "text": mrkdwn_text(text)
    })
}

fn context_block(elements: Vec<Value>) -> Value {
    json!({
        "type": "context",
        "elements": elements
    })
}

fn mrkdwn_text(text: impl Into<String>) -> Value {
    json!({
        "type": "mrkdwn",
        "text": text.into()
    })
}

fn plain_text(text: &str) -> Value {
    json!({
        "type": "plain_text",
        "text": truncate_chars(text, 75),
        "emoji": true
    })
}

fn button_element(label: &str, action_id: &str, value: &str, style: Option<&str>) -> Value {
    let mut button = json!({
        "type": "button",
        "text": plain_text(label),
        "action_id": action_id,
        "value": value
    });
    if let Some(style) = style {
        button["style"] = Value::String(style.to_string());
    }
    button
}

fn code_block(text: &str) -> String {
    format!("```{}```", escape_backticks(&truncate_chars(text, 1800)))
}

fn escape_mrkdwn(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_backticks(text: &str) -> String {
    text.replace('`', "'")
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index >= max_chars {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

fn percent_encode_segment(value: &str) -> String {
    let mut out = String::new();
    for &byte in value.as_bytes() {
        match byte {
            b if b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_' || b == b'~' => {
                out.push(byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn percent_decode_segment(value: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(value.len());
    let input = value.as_bytes();
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let hi = *input.get(index + 1)?;
            let lo = *input.get(index + 2)?;
            bytes.push((hex_value(hi)? << 4) | hex_value(lo)?);
            index += 3;
        } else {
            bytes.push(input[index]);
            index += 1;
        }
    }
    String::from_utf8(bytes).ok()
}

fn percent_decode_lossy(value: &str) -> String {
    let mut out = String::new();
    let input = value.as_bytes();
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let decoded = input
                .get(index + 1)
                .zip(input.get(index + 2))
                .and_then(|(hi, lo)| Some((hex_value(*hi)? << 4) | hex_value(*lo)?));
            if let Some(byte) = decoded {
                out.push(byte as char);
                index += 3;
                continue;
            }
        }
        out.push(input[index] as char);
        index += 1;
    }
    out
}

fn normalize_workspace_file_path(file_path: &str) -> String {
    file_path.replace('\\', "/").trim().to_string()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SlackAdapter, SlackAdapterOptions, encode_thread_id};
    use chat_sdk_chat::types::AdapterError;
    use futures_executor::block_on;
    use std::sync::{Arc, Mutex};

    fn context() -> SlackRunContext {
        SlackRunContext::new("run-1", "assistant-message-1")
    }

    fn fixture_value(path: &str) -> Value {
        serde_json::from_str(match path {
            "approval" => include_str!("fixtures/open-agents-approval-blocks.json"),
            "question" => include_str!("fixtures/open-agents-question-blocks.json"),
            "api-bodies" => include_str!("fixtures/open-agents-slack-api-bodies.json"),
            other => panic!("unknown fixture {other}"),
        })
        .unwrap()
    }

    #[test]
    fn action_ids_round_trip_run_message_and_target() {
        let ctx = SlackRunContext::new("run:123", "msg/456");
        let encoded =
            encode_slack_action_id(SlackOutboundActionKind::QuestionAnswer, &ctx, "ask/q0/A+B");
        assert_eq!(
            encoded,
            "oa:v1:question_answer:run%3A123:msg%2F456:ask%2Fq0%2FA%2BB"
        );
        let decoded = decode_slack_action_id(&encoded).unwrap();
        assert_eq!(decoded.kind, SlackOutboundActionKind::QuestionAnswer);
        assert_eq!(decoded.run_id, "run:123");
        assert_eq!(decoded.message_id, "msg/456");
        assert_eq!(decoded.target, "ask/q0/A+B");
    }

    #[test]
    fn approval_prompt_block_kit_matches_fixture() {
        let request = SlackApprovalRequest {
            approval_id: "approval-1".to_string(),
            tool_call_id: "call-bash-1".to_string(),
            tool_name: "Bash".to_string(),
            title: "Run cargo test --all-features".to_string(),
            details: Some("Command touches the workspace test runner.".to_string()),
        };
        let rendered = render_approval_request(&context(), &request);
        assert_eq!(Value::Array(rendered.blocks), fixture_value("approval"));
        assert_eq!(
            rendered.text,
            "Approval required: Bash - Run cargo test --all-features"
        );
    }

    #[test]
    fn question_prompt_block_kit_matches_fixture() {
        let prompt = SlackQuestionPrompt {
            tool_call_id: "ask-user-call-1".to_string(),
            questions: vec![
                SlackQuestion {
                    header: "Deploy".to_string(),
                    question: "Where should I deploy this?".to_string(),
                    options: vec![
                        SlackQuestionOption {
                            label: "Preview".to_string(),
                            description: "Deploy to a preview environment.".to_string(),
                        },
                        SlackQuestionOption {
                            label: "Production".to_string(),
                            description: "Ship directly to production.".to_string(),
                        },
                    ],
                    multi_select: false,
                },
                SlackQuestion {
                    header: "Checks".to_string(),
                    question: "Which checks should I run?".to_string(),
                    options: vec![
                        SlackQuestionOption {
                            label: "Unit tests".to_string(),
                            description: "Run the fast Rust tests.".to_string(),
                        },
                        SlackQuestionOption {
                            label: "Lint".to_string(),
                            description: "Run clippy and formatting checks.".to_string(),
                        },
                    ],
                    multi_select: true,
                },
            ],
        };
        let rendered = render_question_prompt(&context(), &prompt);
        assert_eq!(Value::Array(rendered.blocks), fixture_value("question"));
        assert!(rendered.text.contains("Questions for you"));
        assert!(rendered.text.contains("Where should I deploy this?"));
    }

    #[test]
    fn slack_api_body_fixtures_cover_post_update_ephemeral_delete_reaction_and_typing() {
        let message = render_run_started(Some("Build the Slack bridge"));
        let post = message
            .post_body("C123", Some("1700000000.000200"))
            .unwrap();
        let update = message.update_body("C123", "1700000000.000300").unwrap();
        let ephemeral = message
            .ephemeral_body("C123", Some("1700000000.000200"), "U123")
            .unwrap();
        let delete = slack_outbound_delete_body("C123", "1700000000.000300");
        let reaction = slack_reaction_body("C123", "1700000000.000300", "hourglass_flowing_sand");
        let typing = slack_typing_status_body("C123", "1700000000.000200", Some("Working"));
        let actual = json!({
            "post": post,
            "update": update,
            "ephemeral": ephemeral,
            "delete": delete,
            "reaction": reaction,
            "typing": typing,
        });
        assert_eq!(actual, fixture_value("api-bodies"));
    }

    #[test]
    fn coalescer_sends_first_update_and_replaces_pending_until_due() {
        let mut coalescer = SlackUpdateCoalescer::new(1000);
        let first = SlackOutboundMessage::text("first");
        let second = SlackOutboundMessage::text("second");
        let third = SlackOutboundMessage::text("third");

        assert_eq!(
            coalescer.push(100, first.clone()),
            CoalescedUpdate::SendNow(first)
        );
        assert_eq!(
            coalescer.push(500, second),
            CoalescedUpdate::Deferred { due_at_ms: 1100 }
        );
        assert_eq!(
            coalescer.push(900, third.clone()),
            CoalescedUpdate::Deferred { due_at_ms: 1100 }
        );
        assert_eq!(coalescer.take_due(1099), None);
        assert_eq!(coalescer.take_due(1100), Some(third));
        assert_eq!(coalescer.take_due(2100), None);
    }

    #[derive(Debug, Default)]
    struct RecordingAdapter {
        calls: Mutex<Vec<Value>>,
    }

    #[async_trait::async_trait]
    impl Adapter for RecordingAdapter {
        fn name(&self) -> &str {
            "recording"
        }

        async fn post_message(&self, thread_id: &str, text: &str) -> AdapterResult<String> {
            self.calls
                .lock()
                .unwrap()
                .push(json!({"method": "post", "thread_id": thread_id, "text": text}));
            Ok("msg-1".to_string())
        }

        async fn edit_message(
            &self,
            thread_id: &str,
            message_id: &str,
            text: &str,
        ) -> AdapterResult<String> {
            self.calls.lock().unwrap().push(json!({
                "method": "update",
                "thread_id": thread_id,
                "message_id": message_id,
                "text": text
            }));
            Ok(message_id.to_string())
        }

        async fn delete_message(&self, thread_id: &str, message_id: &str) -> AdapterResult<()> {
            self.calls.lock().unwrap().push(json!({
                "method": "delete",
                "thread_id": thread_id,
                "message_id": message_id
            }));
            Ok(())
        }

        async fn add_reaction(
            &self,
            thread_id: &str,
            message_id: &str,
            emoji: &str,
        ) -> AdapterResult<()> {
            self.calls.lock().unwrap().push(json!({
                "method": "add_reaction",
                "thread_id": thread_id,
                "message_id": message_id,
                "emoji": emoji
            }));
            Ok(())
        }

        async fn remove_reaction(
            &self,
            thread_id: &str,
            message_id: &str,
            emoji: &str,
        ) -> AdapterResult<()> {
            self.calls.lock().unwrap().push(json!({
                "method": "remove_reaction",
                "thread_id": thread_id,
                "message_id": message_id,
                "emoji": emoji
            }));
            Ok(())
        }

        async fn post_ephemeral(
            &self,
            thread_id: &str,
            user_id: &str,
            text: &str,
        ) -> AdapterResult<EphemeralMessage> {
            self.calls.lock().unwrap().push(json!({
                "method": "ephemeral",
                "thread_id": thread_id,
                "user_id": user_id,
                "text": text
            }));
            Ok(EphemeralMessage {
                id: "eph-1".to_string(),
                thread_id: thread_id.to_string(),
                used_fallback: false,
                raw: Value::Null,
            })
        }

        async fn start_typing(&self, thread_id: &str, status: Option<&str>) -> AdapterResult<()> {
            self.calls.lock().unwrap().push(json!({
                "method": "typing",
                "thread_id": thread_id,
                "status": status
            }));
            Ok(())
        }
    }

    #[test]
    fn dispatcher_uses_existing_adapter_methods_with_fallback_text() {
        let adapter = Arc::new(RecordingAdapter::default());
        let dispatcher = SlackOutboundDispatcher::new(adapter.as_ref());
        let message = render_run_terminal(SlackRunTerminalStatus::Finished, Some("done"));

        assert_eq!(
            block_on(dispatcher.post("slack:C123:1.0", &message)).unwrap(),
            "msg-1"
        );
        assert_eq!(
            block_on(dispatcher.update("slack:C123:1.0", "msg-1", &message)).unwrap(),
            "msg-1"
        );
        block_on(dispatcher.delete("slack:C123:1.0", "msg-1")).unwrap();
        block_on(dispatcher.add_reaction("slack:C123:1.0", "msg-1", "white_check_mark")).unwrap();
        block_on(dispatcher.remove_reaction("slack:C123:1.0", "msg-1", "hourglass")).unwrap();
        block_on(dispatcher.ephemeral("slack:C123:1.0", "U123", &message)).unwrap();
        block_on(dispatcher.typing("slack:C123:1.0", Some("Working"))).unwrap();

        let calls = adapter.calls.lock().unwrap();
        assert_eq!(calls.len(), 7);
        assert_eq!(calls[0]["method"], "post");
        assert_eq!(calls[1]["method"], "update");
        assert_eq!(calls[2]["method"], "delete");
        assert_eq!(calls[3]["method"], "add_reaction");
        assert_eq!(calls[4]["method"], "remove_reaction");
        assert_eq!(calls[5]["method"], "ephemeral");
        assert_eq!(calls[6]["method"], "typing");
    }

    #[test]
    fn renderers_cover_tool_plan_error_commit_and_pr_summaries() {
        let tool = render_tool_event(&SlackToolEvent {
            tool_call_id: "call-1".to_string(),
            tool_name: "bash".to_string(),
            status: SlackToolEventStatus::Error,
            summary: "cargo test".to_string(),
            details: None,
            output: None,
            error: Some("exit 101".to_string()),
        });
        assert!(tool.text.contains("Tool failed"));
        assert!(!tool.blocks.is_empty());

        let plan = render_plan_update(&SlackPlanUpdate {
            title: "Implementation plan".to_string(),
            items: vec![SlackPlanItem {
                content: "Render Slack events".to_string(),
                status: SlackPlanItemStatus::InProgress,
            }],
        });
        assert!(plan.text.contains("Implementation plan"));

        let error = render_run_error("setup failed");
        assert_eq!(error.text, "Run failed: setup failed");

        let commit = render_commit_summary(&SlackCommitSummary {
            status: SlackGitSummaryStatus::Success,
            committed: Some(true),
            pushed: Some(true),
            commit_message: Some("Add Slack outbound renderer".to_string()),
            commit_sha: Some("abc123".to_string()),
            url: Some("https://github.com/acme/repo/commit/abc123".to_string()),
            error: None,
        });
        assert!(commit.text.contains("Commit success"));

        let pr = render_pull_request_summary(&SlackPullRequestSummary {
            status: SlackGitSummaryStatus::Success,
            created: Some(true),
            synced_existing: None,
            pr_number: Some(42),
            url: Some("https://github.com/acme/repo/pull/42".to_string()),
            error: None,
            skip_reason: None,
        });
        assert_eq!(pr.text, "Pull request success: #42");
    }

    #[test]
    fn dispatcher_surfaces_adapter_errors_without_corrupting_fallback_message() {
        #[derive(Debug)]
        struct FailingAdapter;

        #[async_trait::async_trait]
        impl Adapter for FailingAdapter {
            fn name(&self) -> &str {
                "failing"
            }

            async fn post_message(&self, _thread_id: &str, _text: &str) -> AdapterResult<String> {
                Err(AdapterError::InvalidPayload("boom".to_string()))
            }
        }

        let message = SlackOutboundMessage::text("still available");
        let dispatcher = SlackOutboundDispatcher::new(&FailingAdapter);
        let err = block_on(dispatcher.post("slack:C123:1.0", &message)).unwrap_err();
        assert_eq!(err.to_string(), "Adapter parsed an invalid payload: boom");
        assert_eq!(message.text, "still available");
    }

    #[test]
    fn assistant_file_links_build_parse_and_document_workspace_hrefs() {
        let href = build_workspace_file_href("apps/web/app/sessions/[sessionId]/page.tsx");
        assert_eq!(
            href,
            "#workspace-file=apps/web/app/sessions/[sessionId]/page.tsx"
        );
        assert_eq!(
            parse_workspace_file_href(Some(&href)).as_deref(),
            Some("apps/web/app/sessions/[sessionId]/page.tsx")
        );
        assert_eq!(
            parse_workspace_file_href(Some("#workspace-file=apps%5Cweb%5Clib%5Ctest%20file.ts"))
                .as_deref(),
            Some("apps/web/lib/test file.ts")
        );
        assert_eq!(parse_workspace_file_href(Some("https://example.com")), None);
        assert_eq!(parse_workspace_file_href(Some("#workspace-file=")), None);
        assert_eq!(parse_workspace_file_href(None), None);

        let prompt = assistant_file_link_prompt();
        assert!(prompt.contains("[path/to/file.ts](#workspace-file=path/to/file.ts)"));
        assert!(prompt.contains("Whole-file links only for now"));
    }

    #[test]
    fn tool_state_format_tokens_matches_upstream_display_thresholds() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(1), "1");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_000), "1.0k");
        assert_eq!(format_tokens(1_200), "1.2k");
        assert_eq!(format_tokens(15_800), "15.8k");
        assert_eq!(format_tokens(500_000), "500.0k");
        assert_eq!(format_tokens(1_000_000), "1.0m");
        assert_eq!(format_tokens(1_005_000), "1.0m");
        assert_eq!(format_tokens(2_500_000), "2.5m");
        assert_eq!(format_tokens(150_000_000), "150.0m");
        assert_eq!(format_tokens(1_000_000_000), "1.0b");
        assert_eq!(format_tokens(2_500_000_000), "2.5b");
        assert_eq!(format_tokens(10_000_000_000), "10.0b");
        assert_eq!(format_tokens(999_950), "1.0m");
        assert_eq!(format_tokens(999_999), "1.0m");
        assert_eq!(format_tokens(999_950_000), "1.0b");
        assert_eq!(format_tokens(999_999_999), "1.0b");
        assert_eq!(format_tokens(999_949), "999.9k");
        assert_eq!(format_tokens(999_949_999), "999.9m");
        assert!(!format_tokens(1_000_000).contains("1000k"));
        assert!(!format_tokens(1_000_000_000).contains("1000m"));
    }

    #[test]
    #[ignore = "requires live Slack credentials and SLACK_TEST_CHANNEL_ID/SLACK_TEST_USER_ID"]
    fn live_slack_outbound_post_update_react_ephemeral_typing_and_delete() {
        let Some(bot_token) = live_slack_env("SLACK_BOT_TOKEN") else {
            return;
        };
        let Some(signing_secret) = live_slack_env("SLACK_SIGNING_SECRET") else {
            return;
        };
        let Some(channel_id) = live_slack_env("SLACK_TEST_CHANNEL_ID") else {
            return;
        };
        let Some(user_id) = live_slack_env("SLACK_TEST_USER_ID") else {
            return;
        };
        let adapter = SlackAdapter::new(SlackAdapterOptions::new(bot_token, signing_secret));
        let dispatcher = SlackOutboundDispatcher::new(&adapter);
        let channel_thread_id = encode_thread_id(&channel_id, "");
        let initial = render_run_started(Some("ignored live Slack outbound smoke"));

        let message_id = block_on(dispatcher.post(&channel_thread_id, &initial)).unwrap();
        let thread_id = encode_thread_id(&channel_id, &message_id);
        let updated = render_progress_update("Updated by ignored live Slack outbound smoke");

        let result = (|| -> AdapterResult<()> {
            block_on(dispatcher.update(&thread_id, &message_id, &updated))?;
            block_on(dispatcher.add_reaction(&thread_id, &message_id, "hourglass_flowing_sand"))?;
            block_on(dispatcher.remove_reaction(
                &thread_id,
                &message_id,
                "hourglass_flowing_sand",
            ))?;
            block_on(dispatcher.ephemeral(
                &thread_id,
                &user_id,
                &render_run_error("private smoke"),
            ))?;
            block_on(dispatcher.typing(&thread_id, Some("Working")))?;
            Ok(())
        })();

        let delete_result = block_on(dispatcher.delete(&thread_id, &message_id));
        result.unwrap();
        delete_result.unwrap();
    }

    fn live_slack_env(name: &'static str) -> Option<String> {
        match std::env::var(name) {
            Ok(value) if !value.trim().is_empty() => Some(value),
            _ => {
                eprintln!("skipping live Slack outbound smoke because {name} is missing");
                None
            }
        }
    }
}
