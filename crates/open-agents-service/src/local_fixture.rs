use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use chat_sdk_adapter_slack::webhook::{
    SlackBlockActionsPayload, SlackEventBase, SlackParseOptions, SlackWebhookPayload,
    parse_slack_webhook_body,
};
use chat_sdk_adapter_slack::{SlackAdapter, SlackAdapterOptions};
use chat_sdk_chat::types::Adapter;
use chat_sdk_state_memory::MemoryStateAdapter;
use open_agents_slack::SlackThreadAddress;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Slack action id used by answer/resume buttons.
pub const SLACK_ACTION_ANSWER: &str = "open_agents_answer";
/// Slack action id used by cancel buttons.
pub const SLACK_ACTION_CANCEL: &str = "open_agents_cancel";

/// State of a fixture run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureRunStatus {
    Running,
    WaitingForAnswer,
    Completed,
    Cancelled,
}

/// Persisted fixture run record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureRun {
    pub run_id: String,
    pub thread_id: String,
    pub channel_id: String,
    pub user_id: Option<String>,
    pub prompt: String,
    pub status: FixtureRunStatus,
    pub sandbox_steps: Vec<String>,
    pub answer: Option<String>,
    pub final_text: Option<String>,
}

/// Captured Slack outbound message emitted by the fixture harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureOutbound {
    pub thread_id: String,
    pub kind: FixtureOutboundKind,
    pub text: String,
}

/// Fixture outbound message kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureOutboundKind {
    Progress,
    Question,
    Final,
    Cancelled,
}

impl FixtureOutboundKind {
    /// Stable operator-facing label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Progress => "progress",
            Self::Question => "question",
            Self::Final => "final",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Immediate HTTP-style response that Slack expects from event callbacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureReply {
    pub status: u16,
    pub body: String,
}

impl FixtureReply {
    fn accepted() -> Self {
        Self {
            status: 200,
            body: String::new(),
        }
    }

    fn challenge(challenge: String) -> Self {
        Self {
            status: 200,
            body: challenge,
        }
    }
}

/// Deterministic Slack event harness backed by state-memory.
#[derive(Debug)]
pub struct FixtureHarness {
    state: Arc<MemoryStateAdapter>,
    outbounds: Mutex<Vec<FixtureOutbound>>,
    active_runs: Mutex<HashMap<String, String>>,
}

impl FixtureHarness {
    /// Create a connected in-memory fixture harness.
    pub fn new() -> Result<Self, FixtureError> {
        let state = Arc::new(MemoryStateAdapter::new());
        state.connect().map_err(FixtureError::state)?;
        Ok(Self {
            state,
            outbounds: Mutex::new(Vec::new()),
            active_runs: Mutex::new(HashMap::new()),
        })
    }

    /// Parse and handle a Slack callback body.
    pub fn handle_slack_body(
        &self,
        body: &str,
        content_type: &str,
    ) -> Result<FixtureReply, FixtureError> {
        let options = SlackParseOptions {
            content_type: Some(content_type),
            headers: None,
        };
        let payload = parse_slack_webhook_body(body, &options)
            .map_err(|err| FixtureError::SlackParse(err.to_string()))?;
        self.handle_payload(payload)
    }

    /// Return all captured outbound messages.
    pub fn outbound_messages(&self) -> Vec<FixtureOutbound> {
        self.outbounds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Return captured outbound messages for a single Slack thread.
    pub fn outbound_for_thread(&self, thread_id: &str) -> Vec<FixtureOutbound> {
        self.outbound_messages()
            .into_iter()
            .filter(|message| message.thread_id == thread_id)
            .collect()
    }

    /// Load the persisted run for a thread.
    pub fn run_for_thread(&self, thread_id: &str) -> Result<Option<FixtureRun>, FixtureError> {
        let Some(value) = self
            .state
            .get(&run_key(thread_id))
            .map_err(FixtureError::state)?
        else {
            return Ok(None);
        };
        serde_json::from_value(value)
            .map(Some)
            .map_err(FixtureError::json)
    }

    /// Return the active run id for a thread, if any.
    pub fn active_run_id_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<Option<String>, FixtureError> {
        let Some(value) = self
            .state
            .get(&active_key(thread_id))
            .map_err(FixtureError::state)?
        else {
            return Ok(None);
        };
        serde_json::from_value(value)
            .map(Some)
            .map_err(FixtureError::json)
    }

    fn handle_payload(&self, payload: SlackWebhookPayload) -> Result<FixtureReply, FixtureError> {
        match payload {
            SlackWebhookPayload::UrlVerification(payload) => {
                Ok(FixtureReply::challenge(payload.challenge))
            }
            SlackWebhookPayload::AppMention(payload) => {
                self.start_run_from_event(&payload.base)?;
                Ok(FixtureReply::accepted())
            }
            SlackWebhookPayload::DirectMessage(payload) => {
                self.start_run_from_event(&payload.base)?;
                Ok(FixtureReply::accepted())
            }
            SlackWebhookPayload::BlockActions(payload) => {
                self.handle_block_actions(&payload)?;
                Ok(FixtureReply::accepted())
            }
            other => Err(FixtureError::UnsupportedPayload(other.kind().to_string())),
        }
    }

    fn start_run_from_event(&self, event: &SlackEventBase) -> Result<(), FixtureError> {
        let thread_id =
            SlackThreadAddress::new(&event.channel_id, &event.thread_ts).chat_thread_id();
        let run_id = format!("fixture-{}-{}", event.channel_id, event.ts.replace('.', ""));
        let mut run = FixtureRun {
            run_id: run_id.clone(),
            thread_id: thread_id.clone(),
            channel_id: event.channel_id.clone(),
            user_id: event.user_id.clone(),
            prompt: event.text.clone(),
            status: FixtureRunStatus::Running,
            sandbox_steps: Vec::new(),
            answer: None,
            final_text: None,
        };

        self.state
            .subscribe(&thread_id)
            .map_err(FixtureError::state)?;
        self.set_active(&thread_id, &run_id)?;
        self.record(
            &thread_id,
            FixtureOutboundKind::Progress,
            "Starting Open Agents fixture run",
        );

        if event.text.to_ascii_lowercase().contains("question") {
            run.status = FixtureRunStatus::WaitingForAnswer;
            self.persist_run(&run)?;
            self.record(
                &thread_id,
                FixtureOutboundKind::Question,
                "Fixture agent needs an answer before continuing",
            );
            return Ok(());
        }

        self.finish_fake_agent(run, None)
    }

    fn finish_fake_agent(
        &self,
        mut run: FixtureRun,
        answer: Option<String>,
    ) -> Result<(), FixtureError> {
        let thread_id = run.thread_id.clone();
        self.record(
            &thread_id,
            FixtureOutboundKind::Progress,
            "Running sandbox command: pwd",
        );
        run.sandbox_steps.push("sandbox.exec pwd".to_string());
        run.answer = answer;
        run.status = FixtureRunStatus::Completed;
        run.final_text = Some(match &run.answer {
            Some(answer) => format!("Fixture agent finished after answer: {answer}"),
            None => "Fixture agent finished with local sandbox proof".to_string(),
        });
        self.persist_run(&run)?;
        self.clear_active(&thread_id)?;
        self.record(
            &thread_id,
            FixtureOutboundKind::Final,
            run.final_text
                .as_deref()
                .unwrap_or("Fixture agent finished"),
        );
        Ok(())
    }

    fn handle_block_actions(&self, payload: &SlackBlockActionsPayload) -> Result<(), FixtureError> {
        let thread_id = thread_id_from_block_actions(payload).ok_or(FixtureError::MissingThread)?;
        let action = payload
            .actions
            .first()
            .ok_or_else(|| FixtureError::UnsupportedPayload("empty block_actions".to_string()))?;

        if action.action_id == SLACK_ACTION_CANCEL || action.value.as_deref() == Some("cancel") {
            return self.cancel_run(&thread_id);
        }

        if action.action_id == SLACK_ACTION_ANSWER {
            let answer = action
                .value
                .clone()
                .or_else(|| action.selected_option_value.clone())
                .unwrap_or_default();
            let run = self
                .run_for_thread(&thread_id)?
                .ok_or_else(|| FixtureError::RunNotFound(thread_id.clone()))?;
            return self.finish_fake_agent(run, Some(answer));
        }

        Err(FixtureError::UnsupportedPayload(action.action_id.clone()))
    }

    fn cancel_run(&self, thread_id: &str) -> Result<(), FixtureError> {
        let mut run = self
            .run_for_thread(thread_id)?
            .ok_or_else(|| FixtureError::RunNotFound(thread_id.to_string()))?;
        run.status = FixtureRunStatus::Cancelled;
        run.final_text = Some("Fixture run cancelled".to_string());
        self.persist_run(&run)?;
        self.clear_active(thread_id)?;
        self.record(
            thread_id,
            FixtureOutboundKind::Cancelled,
            "Fixture run cancelled",
        );
        Ok(())
    }

    fn persist_run(&self, run: &FixtureRun) -> Result<(), FixtureError> {
        self.state
            .set(
                &run_key(&run.thread_id),
                serde_json::to_value(run).map_err(FixtureError::json)?,
                None,
            )
            .map_err(FixtureError::state)
    }

    fn set_active(&self, thread_id: &str, run_id: &str) -> Result<(), FixtureError> {
        self.active_runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(thread_id.to_string(), run_id.to_string());
        self.state
            .set(&active_key(thread_id), json!(run_id), None)
            .map_err(FixtureError::state)
    }

    fn clear_active(&self, thread_id: &str) -> Result<(), FixtureError> {
        self.active_runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(thread_id);
        self.state
            .delete(&active_key(thread_id))
            .map_err(FixtureError::state)
    }

    fn record(&self, thread_id: &str, kind: FixtureOutboundKind, text: &str) {
        self.outbounds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(FixtureOutbound {
                thread_id: thread_id.to_string(),
                kind,
                text: text.to_string(),
            });
    }
}

/// Fixture harness error.
#[derive(Debug)]
pub enum FixtureError {
    SlackParse(String),
    State(String),
    Json(serde_json::Error),
    MissingThread,
    RunNotFound(String),
    UnsupportedPayload(String),
}

impl FixtureError {
    fn state(err: impl fmt::Display) -> Self {
        Self::State(err.to_string())
    }

    fn json(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

impl fmt::Display for FixtureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SlackParse(err) => write!(formatter, "Slack payload parse failed: {err}"),
            Self::State(err) => write!(formatter, "fixture state failed: {err}"),
            Self::Json(err) => write!(formatter, "fixture JSON failed: {err}"),
            Self::MissingThread => {
                formatter.write_str("Slack interaction did not include a thread")
            }
            Self::RunNotFound(thread_id) => {
                write!(formatter, "no fixture run exists for thread {thread_id}")
            }
            Self::UnsupportedPayload(kind) => {
                write!(formatter, "unsupported Slack fixture payload {kind}")
            }
        }
    }
}

impl std::error::Error for FixtureError {}

fn run_key(thread_id: &str) -> String {
    format!("open-agents:fixture:run:{thread_id}")
}

fn active_key(thread_id: &str) -> String {
    format!("open-agents:fixture:active:{thread_id}")
}

fn thread_id_from_block_actions(payload: &SlackBlockActionsPayload) -> Option<String> {
    if let Some(continuation) = &payload.continuation {
        return Some(
            SlackThreadAddress::new(&continuation.channel_id, &continuation.thread_ts)
                .chat_thread_id(),
        );
    }
    let channel_id = payload.channel_id.as_ref()?;
    let thread_ts = payload
        .thread_ts
        .as_deref()
        .or(payload.message_ts.as_deref())?;
    Some(SlackThreadAddress::new(channel_id, thread_ts).chat_thread_id())
}

/// Post a live probe message when all Slack smoke environment variables exist.
pub async fn run_live_slack_smoke_from_env() -> Result<Option<String>, FixtureError> {
    let config = match crate::OpenAgentsServiceConfig::from_env() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("skipping live Slack smoke: {err}");
            return Ok(None);
        }
    };
    let channel_id = match std::env::var(open_agents_slack::SLACK_TEST_CHANNEL_ID_ENV) {
        Ok(channel_id) if !channel_id.trim().is_empty() => channel_id,
        _ => {
            eprintln!("skipping live Slack smoke: SLACK_TEST_CHANNEL_ID is missing");
            return Ok(None);
        }
    };

    let adapter = SlackAdapter::new(SlackAdapterOptions::new(
        config.slack_bot_token().to_string(),
        config.slack_signing_secret().to_string(),
    ));
    let thread_id = SlackThreadAddress::new(channel_id.trim(), "").chat_thread_id();
    let message_id = adapter
        .post_message(
            &thread_id,
            "Open Agents Rust service live smoke: fixture probe.",
        )
        .await
        .map_err(FixtureError::state)?;
    Ok(Some(message_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_mention_body(text: &str, ts: &str) -> String {
        json!({
            "type": "event_callback",
            "team_id": "T123",
            "api_app_id": "A123",
            "event_id": "Ev123",
            "event_time": 1710000000,
            "event": {
                "type": "app_mention",
                "user": "U123",
                "text": text,
                "channel": "C123",
                "ts": ts
            }
        })
        .to_string()
    }

    fn action_body(action_id: &str, value: &str, thread_ts: &str) -> String {
        let payload = json!({
            "type": "block_actions",
            "user": { "id": "U123", "username": "andrew" },
            "channel": { "id": "C123" },
            "message": { "ts": thread_ts, "thread_ts": thread_ts },
            "actions": [{
                "type": "button",
                "action_id": action_id,
                "value": value
            }]
        })
        .to_string();
        format!("payload={}", form_encode(&payload))
    }

    fn form_encode(value: &str) -> String {
        let mut out = String::new();
        for byte in value.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(byte as char);
                }
                b' ' => out.push('+'),
                other => out.push_str(&format!("%{other:02X}")),
            }
        }
        out
    }

    #[test]
    fn url_verification_returns_challenge() {
        let harness = FixtureHarness::new().unwrap();
        let body = json!({
            "type": "url_verification",
            "challenge": "challenge-token"
        })
        .to_string();

        let reply = harness
            .handle_slack_body(&body, "application/json")
            .unwrap();

        assert_eq!(reply.status, 200);
        assert_eq!(reply.body, "challenge-token");
    }

    #[test]
    fn app_mention_creates_session_uses_sandbox_and_finishes() {
        let harness = FixtureHarness::new().unwrap();
        let thread_id = SlackThreadAddress::new("C123", "1710000000.000100").chat_thread_id();
        let reply = harness
            .handle_slack_body(
                &app_mention_body("inspect the repo", "1710000000.000100"),
                "application/json",
            )
            .unwrap();

        assert_eq!(reply.status, 200);
        let run = harness.run_for_thread(&thread_id).unwrap().unwrap();
        assert_eq!(run.status, FixtureRunStatus::Completed);
        assert_eq!(run.sandbox_steps, vec!["sandbox.exec pwd"]);
        assert_eq!(harness.active_run_id_for_thread(&thread_id).unwrap(), None);

        let outbounds = harness.outbound_for_thread(&thread_id);
        assert!(
            outbounds
                .iter()
                .any(|message| message.kind == FixtureOutboundKind::Progress)
        );
        assert!(
            outbounds
                .iter()
                .any(|message| message.kind == FixtureOutboundKind::Final)
        );
    }

    #[test]
    fn block_action_answer_resumes_waiting_run_in_same_thread() {
        let harness = FixtureHarness::new().unwrap();
        let thread_ts = "1710000000.000200";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();
        harness
            .handle_slack_body(
                &app_mention_body("ask a question before continuing", thread_ts),
                "application/json",
            )
            .unwrap();

        let waiting = harness.run_for_thread(&thread_id).unwrap().unwrap();
        assert_eq!(waiting.status, FixtureRunStatus::WaitingForAnswer);
        assert_eq!(
            harness.active_run_id_for_thread(&thread_id).unwrap(),
            Some(waiting.run_id.clone())
        );

        harness
            .handle_slack_body(
                &action_body(SLACK_ACTION_ANSWER, "ship it", thread_ts),
                "application/x-www-form-urlencoded",
            )
            .unwrap();

        let completed = harness.run_for_thread(&thread_id).unwrap().unwrap();
        assert_eq!(completed.status, FixtureRunStatus::Completed);
        assert_eq!(completed.answer.as_deref(), Some("ship it"));
        assert_eq!(completed.sandbox_steps, vec!["sandbox.exec pwd"]);
        assert_eq!(harness.active_run_id_for_thread(&thread_id).unwrap(), None);
    }

    #[test]
    fn block_action_cancel_clears_active_state() {
        let harness = FixtureHarness::new().unwrap();
        let thread_ts = "1710000000.000300";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();
        harness
            .handle_slack_body(
                &app_mention_body("ask a question before continuing", thread_ts),
                "application/json",
            )
            .unwrap();

        harness
            .handle_slack_body(
                &action_body(SLACK_ACTION_CANCEL, "cancel", thread_ts),
                "application/x-www-form-urlencoded",
            )
            .unwrap();

        let run = harness.run_for_thread(&thread_id).unwrap().unwrap();
        assert_eq!(run.status, FixtureRunStatus::Cancelled);
        assert_eq!(harness.active_run_id_for_thread(&thread_id).unwrap(), None);
        let outbounds = harness.outbound_for_thread(&thread_id);
        assert!(
            outbounds
                .iter()
                .any(|message| message.kind == FixtureOutboundKind::Cancelled)
        );
    }

    #[tokio::test]
    #[ignore = "requires SLACK_BOT_TOKEN, SLACK_SIGNING_SECRET, and SLACK_TEST_CHANNEL_ID"]
    async fn live_slack_smoke_posts_probe_message_when_env_present() {
        let message_id = run_live_slack_smoke_from_env().await.unwrap();
        if let Some(message_id) = message_id {
            assert!(!message_id.is_empty());
        }
    }
}
