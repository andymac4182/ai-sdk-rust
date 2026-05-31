use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ai_sdk_rust::{FinishReason, UiMessageChunk};
use ai_sdk_workflow::{
    DurableRunAgent, DurableRunAgentContext, DurableRunAgentError, DurableRunAgentOutput,
    DurableRunEngine, DurableRunError, DurableRunEventPayload, DurableRunExecution,
    DurableRunResume, DurableRunStartOptions, DurableRunState, DurableRunStore,
    InMemoryDurableRunStore, chat_message_bridge::open_agent_message_from_stream_chunks,
};
use async_trait::async_trait;
use chat_sdk_adapter_slack::message_bridge::render_open_agent_message_for_slack_with_context;
use chat_sdk_adapter_slack::outbound::{
    SlackOutboundActionKind, SlackOutboundMessage, SlackQuestion, SlackQuestionOption,
    SlackQuestionPrompt, SlackRunContext, SlackRunTerminalStatus, decode_slack_action_id,
    render_progress_update, render_question_prompt, render_run_started, render_run_terminal,
};
use chat_sdk_chat::open_agent_message::{OpenAgentMessagePart, OpenAgentMessageRole};
use chat_sdk_state_memory::create_memory_state;
use open_agents_core::{AgentModelSelection, RemoteAgentIdentity};
use open_agents_persistence::{
    ActiveRunRepository, ChatRepository, CreateChatInput, CreateChatMessageInput, CreateRunInput,
    CreateRunStepInput, CreateSessionInput, CreateSlackThreadMappingInput, LifecycleState,
    MemoryPersistenceStore, MessageRepository, PersistenceError, RunRecord, RunRepository,
    RunStatus, RunStatusUpdate, SandboxState as PersistedSandboxState, SandboxStateRepository,
    SandboxStateUpdate, SessionRepository, SlackThreadKey, SlackThreadMappingRecord,
    SlackThreadMappingRepository,
};
use open_agents_runtime::{DEFAULT_OPEN_AGENT_MODEL_LABEL, RemoteAgentRunRequest};
use open_agents_sandbox::{
    SandboxConnectConfig, SandboxContext, SandboxError, SandboxExecOptions, SandboxState,
    connect_sandbox,
};
use open_agents_slack::{
    SlackHttpRequest, SlackHttpResponse, SlackIngress, SlackIngressOptions, SlackIngressRoute,
    SlackInteractionRequest, SlackRunHandoff, SlackRunRouter, SlackRunRouterResult,
    SlackRunStartRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::config::{OpenAgentsServiceConfig, SandboxMode};
use crate::health::HealthCheck;
use crate::{SLACK_ACTION_ANSWER, SLACK_ACTION_CANCEL};

const SLACK_EVENTS_PATH: &str = "/slack/events";
const SLACK_INTERACTIONS_PATH: &str = "/slack/interactions";
const SLACK_COMMANDS_PATH: &str = "/slack/commands";
const ASK_USER_TOOL_CALL_ID: &str = "ask-user-question";
const SANDBOX_TOOL_CALL_ID: &str = "sandbox-pwd";

/// Deployable Open Agents service with Slack HTTP routes and local runtime
/// wiring.
#[derive(Clone, Debug)]
pub struct OpenAgentsService {
    health: HealthCheck,
    slack_ingress: Arc<SlackIngress>,
    runtime: Arc<LocalRuntimeRouter>,
}

impl OpenAgentsService {
    /// Build the service from validated configuration.
    pub fn from_config(config: OpenAgentsServiceConfig) -> Result<Self, ServiceError> {
        let health = HealthCheck::from_config(&config);
        let state = create_memory_state(None);
        state.connect().map_err(ServiceError::state)?;
        let state = Arc::new(state);
        let runtime = Arc::new(LocalRuntimeRouter::new(&config)?);
        let router: Arc<dyn SlackRunRouter> = runtime.clone();
        let slack_ingress = Arc::new(SlackIngress::new(
            SlackIngressOptions::new(config.slack_signing_secret().to_string()),
            state,
            router,
        ));

        Ok(Self {
            health,
            slack_ingress,
            runtime,
        })
    }

    /// Health state shared by probes and startup.
    pub fn health(&self) -> HealthCheck {
        self.health.clone()
    }

    /// Local runtime handle used by deterministic service E2E tests.
    pub fn local_runtime(&self) -> Arc<LocalRuntimeRouter> {
        self.runtime.clone()
    }

    async fn handle(&self, request: HttpRequest) -> HttpResponse {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/healthz" | "/readyz" | "/status") => {
                let (status, content_type, body) = self.health.response_for_path(&request.path);
                HttpResponse::new(status, content_type, body)
            }
            ("POST", SLACK_EVENTS_PATH) => {
                self.handle_slack(SlackIngressRoute::EventsApi, request)
                    .await
            }
            ("POST", SLACK_INTERACTIONS_PATH) => {
                self.handle_slack(SlackIngressRoute::Interactions, request)
                    .await
            }
            ("POST", SLACK_COMMANDS_PATH) => {
                self.handle_slack(SlackIngressRoute::SlashCommand, request)
                    .await
            }
            (_, SLACK_EVENTS_PATH | SLACK_INTERACTIONS_PATH | SLACK_COMMANDS_PATH) => {
                HttpResponse::text(405, "method not allowed\n")
            }
            _ => HttpResponse::text(404, "not found\n"),
        }
    }

    async fn handle_slack(&self, route: SlackIngressRoute, request: HttpRequest) -> HttpResponse {
        let slack_request = SlackHttpRequest {
            body: request.body,
            headers: request.headers,
        };
        let response = self.slack_ingress.handle(route, slack_request).await;
        HttpResponse::from_slack(response)
    }
}

/// Runtime/outbound kind captured by the local deterministic service route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalOutboundKind {
    Progress,
    Question,
    Final,
    Cancelled,
}

/// Captured Slack outbound message emitted by the local deterministic route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalOutbound {
    pub thread_id: String,
    pub run_id: String,
    pub kind: LocalOutboundKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<serde_json::Value>,
}

impl LocalOutbound {
    fn new(
        thread_id: impl Into<String>,
        run_id: impl Into<String>,
        kind: LocalOutboundKind,
        message: SlackOutboundMessage,
    ) -> Self {
        Self {
            thread_id: thread_id.into(),
            run_id: run_id.into(),
            kind,
            text: message.text,
            blocks: message.blocks,
        }
    }
}

/// Service-owned local runtime router for Slack ingress tests and local E2E
/// development.
#[derive(Debug)]
pub struct LocalRuntimeRouter {
    persistence: MemoryPersistenceStore,
    engine: Mutex<DurableRunEngine<InMemoryDurableRunStore, LocalScriptedAgent>>,
    scenarios: Arc<Mutex<HashMap<String, ScriptedRunScenario>>>,
    outbounds: Mutex<Vec<LocalOutbound>>,
    sandbox: ServiceSandbox,
}

impl LocalRuntimeRouter {
    fn new(config: &OpenAgentsServiceConfig) -> Result<Self, ServiceError> {
        let scenarios = Arc::new(Mutex::new(HashMap::new()));
        let agent = LocalScriptedAgent {
            scenarios: scenarios.clone(),
        };
        Ok(Self {
            persistence: MemoryPersistenceStore::new(),
            engine: Mutex::new(DurableRunEngine::new(InMemoryDurableRunStore::new(), agent)),
            scenarios,
            outbounds: Mutex::new(Vec::new()),
            sandbox: ServiceSandbox::from_config(config)?,
        })
    }

    /// Return captured outbound messages.
    pub fn outbound_messages(&self) -> Vec<LocalOutbound> {
        self.outbounds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Return captured outbound messages for a single Slack thread.
    pub fn outbound_for_thread(&self, thread_id: &str) -> Vec<LocalOutbound> {
        self.outbound_messages()
            .into_iter()
            .filter(|message| message.thread_id == thread_id)
            .collect()
    }

    /// Load the persisted Open Agents run for a Slack thread.
    pub async fn run_for_thread(&self, thread_id: &str) -> Result<Option<RunRecord>, ServiceError> {
        let Some(mapping) = self.mapping_for_slack_thread_id(thread_id).await? else {
            return Ok(None);
        };
        let Some(chat) = self
            .persistence
            .get_chat(&mapping.chat_id)
            .await
            .map_err(ServiceError::Persistence)?
        else {
            return Ok(None);
        };
        let Some(run_id) = chat.active_run_id else {
            return self.latest_run_for_chat(&mapping.chat_id).await;
        };
        self.persistence
            .get_run(&run_id)
            .await
            .map_err(ServiceError::Persistence)
    }

    /// Return the active run id for a Slack thread.
    pub async fn active_run_id_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<Option<String>, ServiceError> {
        let Some(mapping) = self.mapping_for_slack_thread_id(thread_id).await? else {
            return Ok(None);
        };
        Ok(self
            .persistence
            .get_chat(&mapping.chat_id)
            .await
            .map_err(ServiceError::Persistence)?
            .and_then(|chat| chat.active_run_id))
    }

    /// Load the durable-run state-machine record.
    pub fn durable_run(
        &self,
        run_id: &str,
    ) -> Result<Option<ai_sdk_workflow::DurableRunRecord>, ServiceError> {
        self.engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .store()
            .load_run(run_id)
            .map_err(ServiceError::DurableRun)
    }

    async fn latest_run_for_chat(&self, chat_id: &str) -> Result<Option<RunRecord>, ServiceError> {
        let messages = self
            .persistence
            .list_chat_messages(chat_id)
            .await
            .map_err(ServiceError::Persistence)?;
        let last_run_id = messages
            .iter()
            .rev()
            .find_map(|message| {
                message
                    .parts
                    .get("metadata")
                    .and_then(|metadata| metadata.get("runId"))
                    .and_then(|run_id| run_id.as_str())
            })
            .map(str::to_string);
        if let Some(run_id) = last_run_id {
            return self
                .persistence
                .get_run(&run_id)
                .await
                .map_err(ServiceError::Persistence);
        }
        Ok(None)
    }

    async fn mapping_for_slack_thread_id(
        &self,
        thread_id: &str,
    ) -> Result<Option<SlackThreadMappingRecord>, ServiceError> {
        let Some((channel_id, thread_ts)) = decode_slack_thread_id(thread_id) else {
            return Ok(None);
        };
        let key = SlackThreadKey::new("T123", channel_id, thread_ts);
        self.persistence
            .get_slack_thread_mapping(&key)
            .await
            .map_err(ServiceError::Persistence)
    }

    async fn ensure_mapping(
        &self,
        target: &open_agents_slack::SlackRouteTarget,
        user_id: Option<&str>,
    ) -> Result<SlackThreadMappingRecord, ServiceError> {
        let team_id = target.team_id.as_deref().unwrap_or("T123");
        let user_id = user_id.unwrap_or("slack-user");
        let key = SlackThreadKey {
            team_id: team_id.to_string(),
            channel_id: target.channel_id.clone(),
            thread_ts: target.thread_ts.clone(),
            enterprise_id: target.enterprise_id.clone(),
        };

        if let Some(mapping) = self
            .persistence
            .get_slack_thread_mapping(&key)
            .await
            .map_err(ServiceError::Persistence)?
        {
            return Ok(mapping);
        }

        let ids = durable_ids_for_target(target, team_id);
        let mut session = CreateSessionInput::new(
            ids.session_id.clone(),
            user_id.to_string(),
            format!("Slack {}", target.slack_thread_id),
        );
        session.lifecycle_state = LifecycleState::Running;
        session.sandbox_state = Some(self.sandbox.persistence_state());
        let chat = CreateChatInput {
            id: ids.chat_id.clone(),
            session_id: ids.session_id.clone(),
            title: "Slack remote agent".to_string(),
            model_id: Some(DEFAULT_OPEN_AGENT_MODEL_LABEL.to_string()),
        };
        self.persistence
            .create_session_with_initial_chat(session, chat)
            .await
            .map_err(ServiceError::Persistence)?;
        self.persistence
            .update_sandbox_state(
                &ids.session_id,
                SandboxStateUpdate {
                    lifecycle_state: LifecycleState::Running,
                    sandbox_state: Some(self.sandbox.persistence_state()),
                    lifecycle_error: None,
                },
            )
            .await
            .map_err(ServiceError::Persistence)?;

        self.persistence
            .create_slack_thread_mapping_if_absent(CreateSlackThreadMappingInput {
                key,
                session_id: ids.session_id,
                chat_id: ids.chat_id,
                user_id: user_id.to_string(),
                root_message_ts: Some(target.thread_ts.clone()),
                last_event_ts: None,
            })
            .await
            .map(|inserted| inserted.record)
            .map_err(ServiceError::Persistence)
    }

    async fn start_local_run(
        &self,
        request: SlackRunStartRequest,
    ) -> Result<SlackRunHandoff, ServiceError> {
        let mapping = self
            .ensure_mapping(&request.target, request.user_id.as_deref())
            .await?;
        let run_id = run_id_for_start(&request);
        let user_id = request
            .user_id
            .clone()
            .unwrap_or_else(|| mapping.user_id.clone());

        self.persist_user_message(&mapping, &request).await?;
        self.persistence
            .create_run(CreateRunInput {
                id: run_id.clone(),
                session_id: mapping.session_id.clone(),
                chat_id: mapping.chat_id.clone(),
                user_id: user_id.clone(),
                model_id: Some(DEFAULT_OPEN_AGENT_MODEL_LABEL.to_string()),
                status: RunStatus::Running,
                idempotency_key: request.event_id.clone(),
                started_at: None,
            })
            .await
            .map_err(ServiceError::Persistence)?;
        if !self
            .persistence
            .claim_chat_active_run(&mapping.chat_id, &run_id)
            .await
            .map_err(ServiceError::Persistence)?
        {
            return Err(ServiceError::Conflict(format!(
                "another run is already active for chat {}",
                mapping.chat_id
            )));
        }

        let runtime_request = RemoteAgentRunRequest::new(
            RemoteAgentIdentity::new(mapping.session_id.clone(), mapping.chat_id.clone())
                .with_run_id(run_id.clone()),
            AgentModelSelection::new(DEFAULT_OPEN_AGENT_MODEL_LABEL),
            self.sandbox.runtime_context(),
        )
        .with_message(json!({
            "id": user_message_id(&request),
            "role": "user",
            "parts": [{ "type": "text", "text": request.text }],
        }));
        self.scenarios
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                run_id.clone(),
                ScriptedRunScenario::new(
                    request.text.clone(),
                    runtime_request,
                    self.sandbox.clone(),
                ),
            );

        self.record_outbound(
            &mapping,
            &run_id,
            LocalOutboundKind::Progress,
            render_run_started(Some(&request.text)),
        );

        let execution = {
            let mut engine = self
                .engine
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            engine
                .start(DurableRunStartOptions::new(
                    run_id.clone(),
                    mapping.chat_id.clone(),
                ))
                .map_err(ServiceError::DurableRun)?
        };
        self.apply_execution(&mapping, &run_id, &execution, None)
            .await?;

        Ok(
            SlackRunHandoff::new(Some(run_id), false).with_metadata(json!({
                "sessionId": mapping.session_id,
                "chatId": mapping.chat_id,
                "state": durable_state_label(execution.state),
            })),
        )
    }

    async fn resume_local_run(
        &self,
        request: SlackInteractionRequest,
    ) -> Result<SlackRunHandoff, ServiceError> {
        let action = request
            .actions
            .first()
            .ok_or_else(|| ServiceError::Unsupported("empty Slack interaction".to_string()))?;
        let decoded = decode_slack_action_id(&action.action_id);
        let is_cancel = action.action_id == SLACK_ACTION_CANCEL
            || action.value.as_deref() == Some("cancel")
            || decoded.as_ref().is_some_and(|action| {
                action.kind == SlackOutboundActionKind::QuestionDecline
                    || action.kind == SlackOutboundActionKind::Deny
            });
        let mapping = self
            .mapping_for_interaction(&request, decoded.as_ref())
            .await?;
        let active_run_id = self
            .persistence
            .get_chat(&mapping.chat_id)
            .await
            .map_err(ServiceError::Persistence)?
            .and_then(|chat| chat.active_run_id);
        let run_id = decoded
            .as_ref()
            .map(|action| action.run_id.clone())
            .or(active_run_id)
            .ok_or_else(|| ServiceError::NotFound("no active run for Slack thread".to_string()))?;

        if is_cancel {
            let execution = {
                let mut engine = self
                    .engine
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                engine
                    .cancel(&run_id, Some("cancelled from Slack".to_string()))
                    .map_err(ServiceError::DurableRun)?
            };
            self.apply_execution(&mapping, &run_id, &execution, None)
                .await?;
            return Ok(
                SlackRunHandoff::new(Some(run_id), true).with_metadata(json!({
                    "state": "canceled",
                })),
            );
        }

        let answer = action_answer(action, decoded.as_ref());
        if action.action_id != SLACK_ACTION_ANSWER
            && !decoded.as_ref().is_some_and(|action| {
                action.kind == SlackOutboundActionKind::QuestionAnswer
                    || action.kind == SlackOutboundActionKind::QuestionSubmit
                    || action.kind == SlackOutboundActionKind::QuestionSelect
            })
        {
            return Err(ServiceError::Unsupported(format!(
                "unsupported Slack action {}",
                action.action_id
            )));
        }

        let resume = DurableRunResume::ToolInput {
            tool_call_id: ASK_USER_TOOL_CALL_ID.to_string(),
            input: json!({ "answer": answer }),
        };
        let execution = {
            let mut engine = self
                .engine
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            engine
                .resume(&run_id, resume)
                .map_err(ServiceError::DurableRun)?
        };
        self.record_outbound(
            &mapping,
            &run_id,
            LocalOutboundKind::Progress,
            render_progress_update(&format!("Received answer: {answer}")),
        );
        self.apply_execution(&mapping, &run_id, &execution, Some(answer))
            .await?;

        Ok(
            SlackRunHandoff::new(Some(run_id), true).with_metadata(json!({
                "state": durable_state_label(execution.state),
            })),
        )
    }

    async fn mapping_for_interaction(
        &self,
        request: &SlackInteractionRequest,
        decoded: Option<&chat_sdk_adapter_slack::outbound::SlackOutboundActionId>,
    ) -> Result<SlackThreadMappingRecord, ServiceError> {
        if let Some(target) = &request.target {
            return self.ensure_mapping(target, Some(&request.user_id)).await;
        }
        if let Some(decoded) = decoded {
            let record = self
                .persistence
                .get_run(&decoded.run_id)
                .await
                .map_err(ServiceError::Persistence)?
                .ok_or_else(|| ServiceError::NotFound(decoded.run_id.clone()))?;
            return self
                .persistence
                .get_slack_thread_mapping_by_chat(&record.chat_id)
                .await
                .map_err(ServiceError::Persistence)?
                .ok_or_else(|| ServiceError::NotFound(record.chat_id));
        }
        Err(ServiceError::MissingSlackTarget)
    }

    async fn persist_user_message(
        &self,
        mapping: &SlackThreadMappingRecord,
        request: &SlackRunStartRequest,
    ) -> Result<(), ServiceError> {
        self.persistence
            .create_message_if_absent(CreateChatMessageInput {
                id: user_message_id(request),
                chat_id: mapping.chat_id.clone(),
                role: open_agents_persistence::MessageRole::User,
                parts: json!({
                    "id": user_message_id(request),
                    "role": "user",
                    "parts": [{ "type": "text", "text": request.text }],
                    "metadata": {
                        "slackThreadId": request.target.slack_thread_id,
                        "slackEventId": request.event_id,
                    }
                }),
                created_at: None,
            })
            .await
            .map(|_| ())
            .map_err(ServiceError::Persistence)
    }

    async fn apply_execution(
        &self,
        mapping: &SlackThreadMappingRecord,
        run_id: &str,
        execution: &DurableRunExecution,
        answer: Option<String>,
    ) -> Result<(), ServiceError> {
        self.persist_run_step(run_id, execution).await?;
        self.persist_assistant_message(mapping, run_id, execution, answer.as_deref())
            .await?;

        match execution.state {
            DurableRunState::Finished => {
                self.persistence
                    .update_run_status(
                        run_id,
                        RunStatusUpdate {
                            status: RunStatus::Finished,
                            finished_at: Some(OffsetDateTime::now_utc()),
                            error: None,
                        },
                    )
                    .await
                    .map_err(ServiceError::Persistence)?;
                self.persistence
                    .compare_and_set_chat_active_run(
                        &mapping.chat_id,
                        Some(run_id.to_string()),
                        None,
                    )
                    .await
                    .map_err(ServiceError::Persistence)?;
                self.record_outbound(
                    mapping,
                    run_id,
                    LocalOutboundKind::Final,
                    render_run_terminal(
                        SlackRunTerminalStatus::Finished,
                        Some("local sandbox proof complete"),
                    ),
                );
            }
            DurableRunState::WaitingForInput => {
                self.persistence
                    .update_run_status(
                        run_id,
                        RunStatusUpdate {
                            status: RunStatus::Paused,
                            finished_at: None,
                            error: None,
                        },
                    )
                    .await
                    .map_err(ServiceError::Persistence)?;
                self.record_outbound(
                    mapping,
                    run_id,
                    LocalOutboundKind::Question,
                    render_question_prompt(
                        &SlackRunContext::new(run_id, format!("{run_id}-question")),
                        &SlackQuestionPrompt {
                            tool_call_id: ASK_USER_TOOL_CALL_ID.to_string(),
                            questions: vec![SlackQuestion {
                                header: "Decision".to_string(),
                                question: "Should the local fixture continue?".to_string(),
                                options: vec![
                                    SlackQuestionOption {
                                        label: "ship it".to_string(),
                                        description: "Resume the deterministic local run."
                                            .to_string(),
                                    },
                                    SlackQuestionOption {
                                        label: "hold".to_string(),
                                        description: "Keep the run paused for inspection."
                                            .to_string(),
                                    },
                                ],
                                multi_select: false,
                            }],
                        },
                    ),
                );
            }
            DurableRunState::Canceled => {
                self.persistence
                    .update_run_status(
                        run_id,
                        RunStatusUpdate {
                            status: RunStatus::Canceled,
                            finished_at: Some(OffsetDateTime::now_utc()),
                            error: None,
                        },
                    )
                    .await
                    .map_err(ServiceError::Persistence)?;
                self.persistence
                    .compare_and_set_chat_active_run(
                        &mapping.chat_id,
                        Some(run_id.to_string()),
                        None,
                    )
                    .await
                    .map_err(ServiceError::Persistence)?;
                self.record_outbound(
                    mapping,
                    run_id,
                    LocalOutboundKind::Cancelled,
                    render_run_terminal(SlackRunTerminalStatus::Canceled, Some("cancelled")),
                );
            }
            DurableRunState::Failed => {
                self.persistence
                    .update_run_status(
                        run_id,
                        RunStatusUpdate {
                            status: RunStatus::Failed,
                            finished_at: Some(OffsetDateTime::now_utc()),
                            error: Some("local runtime failed".to_string()),
                        },
                    )
                    .await
                    .map_err(ServiceError::Persistence)?;
            }
            DurableRunState::Queued | DurableRunState::Running | DurableRunState::Canceling => {}
            DurableRunState::WaitingForApproval => {
                self.persistence
                    .update_run_status(
                        run_id,
                        RunStatusUpdate {
                            status: RunStatus::Paused,
                            finished_at: None,
                            error: None,
                        },
                    )
                    .await
                    .map_err(ServiceError::Persistence)?;
            }
        }

        Ok(())
    }

    async fn persist_run_step(
        &self,
        run_id: &str,
        execution: &DurableRunExecution,
    ) -> Result<(), ServiceError> {
        let step_number = self
            .persistence
            .list_run_steps(run_id)
            .await
            .map_err(ServiceError::Persistence)?
            .len() as u32
            + 1;
        self.persistence
            .create_run_step_if_absent(CreateRunStepInput {
                id: format!("{run_id}:step:{step_number}"),
                run_id: run_id.to_string(),
                step_number,
                started_at: OffsetDateTime::now_utc(),
                finished_at: Some(OffsetDateTime::now_utc()),
                duration_ms: Some(0),
                finish_reason: Some(durable_state_label(execution.state).to_string()),
                raw_finish_reason: Some(format!("{:?}", execution.state)),
            })
            .await
            .map(|_| ())
            .map_err(ServiceError::Persistence)
    }

    async fn persist_assistant_message(
        &self,
        mapping: &SlackThreadMappingRecord,
        run_id: &str,
        execution: &DurableRunExecution,
        answer: Option<&str>,
    ) -> Result<(), ServiceError> {
        if execution.chunks.is_empty() {
            return Ok(());
        }

        let message_id = assistant_message_id(run_id, execution.state);
        let message =
            open_agent_message_from_stream_chunks(message_id.clone(), execution.chunks.clone())
                .unwrap_or_else(|_| fallback_assistant_message(&message_id, execution, answer));
        if let Some(outbound) = render_open_agent_message_for_slack_with_context(
            &message,
            &SlackRunContext::new(run_id, message.id.clone()),
        ) && execution.state == DurableRunState::Finished
        {
            self.record_outbound(mapping, run_id, LocalOutboundKind::Progress, outbound);
        }

        self.persistence
            .create_message_if_absent(CreateChatMessageInput {
                id: message.id.clone(),
                chat_id: mapping.chat_id.clone(),
                role: open_agents_persistence::MessageRole::Assistant,
                parts: json!({
                    "id": message.id,
                    "role": message.role,
                    "metadata": {
                        "runId": run_id,
                        "durableState": durable_state_label(execution.state),
                    },
                    "parts": message.parts,
                }),
                created_at: None,
            })
            .await
            .map(|_| ())
            .map_err(ServiceError::Persistence)
    }

    fn record_outbound(
        &self,
        mapping: &SlackThreadMappingRecord,
        run_id: &str,
        kind: LocalOutboundKind,
        message: SlackOutboundMessage,
    ) {
        let thread_id = chat_sdk_adapter_slack::encode_thread_id(
            &mapping.key.channel_id,
            &mapping.key.thread_ts,
        );
        self.outbounds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(LocalOutbound::new(thread_id, run_id, kind, message));
    }
}

#[async_trait]
impl SlackRunRouter for LocalRuntimeRouter {
    async fn start_or_resume(
        &self,
        request: SlackRunStartRequest,
    ) -> SlackRunRouterResult<SlackRunHandoff> {
        self.start_local_run(request)
            .await
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn resume_interaction(
        &self,
        request: SlackInteractionRequest,
    ) -> SlackRunRouterResult<SlackRunHandoff> {
        self.resume_local_run(request)
            .await
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
    }
}

#[derive(Debug, Clone)]
struct ServiceSandbox {
    connect: SandboxConnectConfig,
    context: SandboxContext,
    persisted: PersistedSandboxState,
}

impl ServiceSandbox {
    fn from_config(config: &OpenAgentsServiceConfig) -> Result<Self, ServiceError> {
        match config.sandbox() {
            SandboxMode::Local { root } => {
                let root = absolutize(root)?;
                let root_string = root.to_string_lossy().to_string();
                let state = SandboxState::Local {
                    root: root_string.clone(),
                    working_directory: root_string.clone(),
                    current_branch: None,
                    expires_at: None,
                };
                let context = SandboxContext::new(
                    json!({"type": "local", "root": root_string}),
                    root_string.clone(),
                )
                .with_environment_details("local deterministic Open Agents service route");
                Ok(Self {
                    connect: SandboxConnectConfig::new(state),
                    context,
                    persisted: PersistedSandboxState {
                        provider: "local".to_string(),
                        sandbox_id: None,
                        sandbox_name: None,
                        working_directory: Some(root_string),
                        current_branch: None,
                        environment_details: Some(
                            "local deterministic Open Agents service route".to_string(),
                        ),
                        raw: json!({ "type": "local" }),
                    },
                })
            }
            SandboxMode::Vercel { base_snapshot_id } => {
                let state = SandboxState::Vercel {
                    source: None,
                    sandbox_name: None,
                    sandbox_id: None,
                    snapshot_id: base_snapshot_id.clone(),
                    expires_at: None,
                };
                Ok(Self {
                    connect: SandboxConnectConfig::new(state.clone()),
                    context: SandboxContext::new(
                        serde_json::to_value(&state).unwrap_or_else(|_| json!({"type": "vercel"})),
                        "/vercel/sandbox",
                    ),
                    persisted: PersistedSandboxState {
                        provider: "vercel".to_string(),
                        sandbox_id: None,
                        sandbox_name: None,
                        working_directory: Some("/vercel/sandbox".to_string()),
                        current_branch: None,
                        environment_details: None,
                        raw: serde_json::to_value(&state)
                            .unwrap_or_else(|_| json!({"type": "vercel"})),
                    },
                })
            }
        }
    }

    fn runtime_context(&self) -> SandboxContext {
        self.context.clone()
    }

    fn persistence_state(&self) -> PersistedSandboxState {
        self.persisted.clone()
    }
}

#[derive(Debug, Clone)]
struct ScriptedRunScenario {
    prompt: String,
    runtime_request: RemoteAgentRunRequest,
    sandbox: ServiceSandbox,
}

impl ScriptedRunScenario {
    fn new(
        prompt: String,
        runtime_request: RemoteAgentRunRequest,
        sandbox: ServiceSandbox,
    ) -> Self {
        Self {
            prompt,
            runtime_request,
            sandbox,
        }
    }

    fn wants_question(&self) -> bool {
        self.prompt.to_ascii_lowercase().contains("question")
    }
}

#[derive(Debug, Clone)]
struct LocalScriptedAgent {
    scenarios: Arc<Mutex<HashMap<String, ScriptedRunScenario>>>,
}

impl DurableRunAgent for LocalScriptedAgent {
    fn next_turn(
        &mut self,
        context: DurableRunAgentContext,
    ) -> Result<DurableRunAgentOutput, DurableRunAgentError> {
        let scenario = self
            .scenarios
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&context.run_id)
            .cloned()
            .ok_or_else(|| DurableRunAgentError::new("missing local scripted run scenario"))?;

        if context.resume.is_none()
            && scenario.wants_question()
            && !context.previous_events.iter().any(|event| {
                matches!(
                    event.payload,
                    DurableRunEventPayload::WaitingForInput { .. }
                        | DurableRunEventPayload::WaitingForApproval { .. }
                )
            })
        {
            return Ok(DurableRunAgentOutput::WaitingForInput {
                tool_call_id: ASK_USER_TOOL_CALL_ID.to_string(),
                chunks: vec![
                    UiMessageChunk::start_with_message_id(assistant_message_id(
                        &context.run_id,
                        DurableRunState::WaitingForInput,
                    )),
                    UiMessageChunk::start_step(),
                    UiMessageChunk::tool_input_available(
                        ASK_USER_TOOL_CALL_ID,
                        "ask_user_question",
                        json!({
                            "questions": [{
                                "header": "Decision",
                                "question": "Should the local fixture continue?",
                                "options": [
                                    {
                                        "label": "ship it",
                                        "description": "Resume the deterministic local run."
                                    },
                                    {
                                        "label": "hold",
                                        "description": "Keep the run paused for inspection."
                                    }
                                ]
                            }]
                        }),
                    ),
                    UiMessageChunk::finish_step(),
                ],
            });
        }

        let answer = match context.resume {
            Some(DurableRunResume::ToolInput { input, .. }) => input
                .get("answer")
                .and_then(|answer| answer.as_str())
                .map(str::to_string),
            _ => None,
        };
        let sandbox = connect_sandbox(scenario.sandbox.connect.clone())
            .map_err(|error| DurableRunAgentError::new(error.to_string()))?;
        let result = sandbox
            .exec(SandboxExecOptions::new("pwd"))
            .map_err(|error| DurableRunAgentError::new(error.to_string()))?;
        let pwd = result.stdout.trim();
        let final_text = match answer.as_deref() {
            Some(answer) => format!(
                "Fixture agent finished after answer `{answer}` in {}.",
                scenario.runtime_request.sandbox.working_directory
            ),
            None => format!("Fixture agent finished with local sandbox proof in {pwd}."),
        };

        Ok(DurableRunAgentOutput::Finished {
            chunks: vec![
                UiMessageChunk::start_with_message_id(assistant_message_id(
                    &context.run_id,
                    DurableRunState::Finished,
                )),
                UiMessageChunk::start_step(),
                UiMessageChunk::text_start("text-1"),
                UiMessageChunk::text_delta("text-1", final_text),
                UiMessageChunk::text_end("text-1"),
                UiMessageChunk::tool_input_available(
                    SANDBOX_TOOL_CALL_ID,
                    "bash",
                    json!({ "command": "pwd" }),
                ),
                UiMessageChunk::tool_output_available(
                    SANDBOX_TOOL_CALL_ID,
                    json!({
                        "success": result.success,
                        "stdout": result.stdout,
                        "stderr": result.stderr,
                    }),
                ),
                UiMessageChunk::finish_step(),
                UiMessageChunk::finish_with_reason(FinishReason::Stop),
            ],
        })
    }
}

/// Errors returned by the composed Open Agents service.
#[derive(Debug)]
pub enum ServiceError {
    Io(io::Error),
    State(String),
    Persistence(PersistenceError),
    DurableRun(DurableRunError),
    Sandbox(SandboxError),
    Conflict(String),
    MissingSlackTarget,
    NotFound(String),
    Unsupported(String),
    BadRequest(String),
}

impl ServiceError {
    fn state(error: impl fmt::Display) -> Self {
        Self::State(error.to_string())
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "service I/O error: {error}"),
            Self::State(error) => write!(formatter, "service state error: {error}"),
            Self::Persistence(error) => write!(formatter, "service persistence error: {error}"),
            Self::DurableRun(error) => write!(formatter, "service durable run error: {error}"),
            Self::Sandbox(error) => write!(formatter, "service sandbox error: {error}"),
            Self::Conflict(error) => write!(formatter, "service conflict: {error}"),
            Self::MissingSlackTarget => {
                formatter.write_str("Slack interaction is missing a target")
            }
            Self::NotFound(error) => write!(formatter, "service record not found: {error}"),
            Self::Unsupported(error) => write!(formatter, "unsupported service request: {error}"),
            Self::BadRequest(error) => write!(formatter, "bad service request: {error}"),
        }
    }
}

impl std::error::Error for ServiceError {}

impl From<io::Error> for ServiceError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<SandboxError> for ServiceError {
    fn from(error: SandboxError) -> Self {
        Self::Sandbox(error)
    }
}

/// Bind the full service listener.
pub async fn bind_service_listener(addr: SocketAddr) -> Result<TcpListener, ServiceError> {
    TcpListener::bind(addr).await.map_err(ServiceError::Io)
}

/// Serve health and Slack routes until `shutdown` resolves.
pub async fn serve_open_agents_service(
    listener: TcpListener,
    service: OpenAgentsService,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServiceError> {
    let mut shutdown = Box::pin(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                return Ok(());
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let service = service.clone();
                tokio::spawn(async move {
                    let _ = handle_connection(stream, service).await;
                });
            }
        }
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    service: OpenAgentsService,
) -> Result<(), ServiceError> {
    let request = read_http_request(&mut stream).await?;
    let response = service.handle(request).await;
    stream.write_all(&response.to_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    content_type: String,
    body: String,
}

impl HttpResponse {
    fn new(status: u16, content_type: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: content_type.into(),
            body: body.into(),
        }
    }

    fn text(status: u16, body: impl Into<String>) -> Self {
        Self::new(status, "text/plain; charset=utf-8", body)
    }

    fn from_slack(response: SlackHttpResponse) -> Self {
        Self::new(
            response.status,
            response
                .content_type
                .unwrap_or_else(|| "text/plain; charset=utf-8".to_string()),
            response.body,
        )
    }

    fn to_bytes(&self) -> Vec<u8> {
        let reason = reason_phrase(self.status);
        format!(
            "HTTP/1.1 {} {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            self.status,
            reason,
            self.content_type,
            self.body.len(),
            self.body
        )
        .into_bytes()
    }
}

async fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, ServiceError> {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(ServiceError::BadRequest("connection closed".to_string()));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(end) = find_header_end(&bytes) {
            break end;
        }
        if bytes.len() > 1024 * 1024 {
            return Err(ServiceError::BadRequest(
                "request headers too large".to_string(),
            ));
        }
    };

    let header_bytes = &bytes[..header_end];
    let header_text = std::str::from_utf8(header_bytes)
        .map_err(|_| ServiceError::BadRequest("request headers are not UTF-8".to_string()))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| ServiceError::BadRequest("missing request line".to_string()))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| ServiceError::BadRequest("missing HTTP method".to_string()))?
        .to_string();
    let path = request_parts
        .next()
        .ok_or_else(|| ServiceError::BadRequest("missing HTTP path".to_string()))?
        .to_string();
    let headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_string(), value.trim().to_string()))
        })
        .collect::<Vec<_>>();
    let content_length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    while bytes.len().saturating_sub(body_start) < content_length {
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    let body_bytes = &bytes[body_start..body_start + content_length.min(bytes.len() - body_start)];
    let body = String::from_utf8(body_bytes.to_vec())
        .map_err(|_| ServiceError::BadRequest("request body is not UTF-8".to_string()))?;

    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn durable_ids_for_target(
    target: &open_agents_slack::SlackRouteTarget,
    team_id: &str,
) -> DurableTargetIds {
    let base = stable_id(&format!(
        "{}:{}:{}:{}",
        team_id,
        target.channel_id,
        target.thread_ts,
        target.enterprise_id.as_deref().unwrap_or("")
    ));
    DurableTargetIds {
        session_id: format!("slack-session-{base}"),
        chat_id: format!("slack-chat-{base}"),
    }
}

struct DurableTargetIds {
    session_id: String,
    chat_id: String,
}

fn run_id_for_start(request: &SlackRunStartRequest) -> String {
    let seed = request
        .event_id
        .as_deref()
        .or(request.message_ts.as_deref())
        .unwrap_or(&request.target.slack_thread_id);
    format!("slack-run-{}", stable_id(seed))
}

fn user_message_id(request: &SlackRunStartRequest) -> String {
    let seed = request
        .message_ts
        .as_deref()
        .or(request.event_id.as_deref())
        .unwrap_or(&request.target.slack_thread_id);
    format!("slack-user-{}", stable_id(seed))
}

fn assistant_message_id(run_id: &str, state: DurableRunState) -> String {
    format!("{run_id}:assistant:{}", durable_state_label(state))
}

fn fallback_assistant_message(
    message_id: &str,
    execution: &DurableRunExecution,
    answer: Option<&str>,
) -> chat_sdk_chat::open_agent_message::OpenAgentUiMessage {
    let text = match (execution.state, answer) {
        (DurableRunState::WaitingForInput, _) => "Waiting for answer".to_string(),
        (DurableRunState::Canceled, _) => "Cancelled".to_string(),
        (_, Some(answer)) => format!("Finished after answer: {answer}"),
        _ => "Finished".to_string(),
    };
    chat_sdk_chat::open_agent_message::OpenAgentUiMessage::new(
        message_id.to_string(),
        OpenAgentMessageRole::Assistant,
    )
    .with_part(OpenAgentMessagePart::done_text(text))
}

fn durable_state_label(state: DurableRunState) -> &'static str {
    match state {
        DurableRunState::Queued => "queued",
        DurableRunState::Running => "running",
        DurableRunState::WaitingForInput => "waiting_for_input",
        DurableRunState::WaitingForApproval => "waiting_for_approval",
        DurableRunState::Canceling => "canceling",
        DurableRunState::Canceled => "canceled",
        DurableRunState::Failed => "failed",
        DurableRunState::Finished => "finished",
    }
}

fn action_answer(
    action: &open_agents_slack::SlackInteractionAction,
    decoded: Option<&chat_sdk_adapter_slack::outbound::SlackOutboundActionId>,
) -> String {
    action
        .value
        .clone()
        .or_else(|| action.selected_option_value.clone())
        .or_else(|| decoded.map(|action| answer_from_target(&action.target)))
        .unwrap_or_default()
}

fn answer_from_target(target: &str) -> String {
    target
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(target)
        .to_string()
}

fn decode_slack_thread_id(thread_id: &str) -> Option<(&str, &str)> {
    let without_prefix = thread_id.strip_prefix("slack:")?;
    without_prefix.split_once(':')
}

fn stable_id(input: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in input.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

fn absolutize(path: &Path) -> Result<PathBuf, ServiceError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(ServiceError::Io)
    }
}

trait RemoteAgentRunRequestExt {
    fn with_message(self, message: serde_json::Value) -> Self;
}

impl RemoteAgentRunRequestExt for RemoteAgentRunRequest {
    fn with_message(mut self, message: serde_json::Value) -> Self {
        self.messages.push(message);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::{Hmac, Mac};
    use open_agents_slack::SlackThreadAddress;
    use sha2::Sha256;
    use tokio::sync::oneshot;

    #[derive(Debug)]
    struct TestServer {
        addr: SocketAddr,
        service: OpenAgentsService,
        stop: oneshot::Sender<()>,
        server: tokio::task::JoinHandle<Result<(), ServiceError>>,
    }

    impl TestServer {
        async fn start() -> Self {
            let config = OpenAgentsServiceConfig::fixture();
            let service = OpenAgentsService::from_config(config.clone()).unwrap();
            let listener = bind_service_listener(config.bind_addr()).await.unwrap();
            let addr = listener.local_addr().unwrap();
            service.health().set_ready(true);
            let (stop, stop_rx) = oneshot::channel::<()>();
            let server = tokio::spawn(serve_open_agents_service(
                listener,
                service.clone(),
                async move {
                    let _ = stop_rx.await;
                },
            ));
            Self {
                addr,
                service,
                stop,
                server,
            }
        }

        async fn stop(self) {
            let _ = self.stop.send(());
            self.server.await.unwrap().unwrap();
        }
    }

    #[derive(Debug)]
    struct TestResponse {
        status: u16,
        body: String,
    }

    async fn post(addr: SocketAddr, path: &str, body: &str, content_type: &str) -> TestResponse {
        let timestamp = current_timestamp();
        let signature = sign(body, &timestamp);
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let request = format!(
            "POST {path} HTTP/1.1\r\nhost: localhost\r\ncontent-type: {content_type}\r\nx-slack-request-timestamp: {timestamp}\r\nx-slack-signature: {signature}\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        parse_response(&response)
    }

    fn parse_response(response: &str) -> TestResponse {
        let (head, body) = response.split_once("\r\n\r\n").unwrap();
        let status = head
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse::<u16>()
            .unwrap();
        TestResponse {
            status,
            body: body.to_string(),
        }
    }

    fn current_timestamp() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string()
    }

    fn sign(body: &str, timestamp: &str) -> String {
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(b"fixture-signing-secret").unwrap();
        mac.update(b"v0:");
        mac.update(timestamp.as_bytes());
        mac.update(b":");
        mac.update(body.as_bytes());
        let bytes = mac.finalize().into_bytes();
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        format!("v0={hex}")
    }

    fn app_mention_body(text: &str, event_id: &str, ts: &str) -> String {
        json!({
            "type": "event_callback",
            "team_id": "T123",
            "api_app_id": "A123",
            "event_id": event_id,
            "event_time": 1710000000,
            "event": {
                "type": "app_mention",
                "user": "U123",
                "text": text,
                "channel": "C123",
                "team": "T123",
                "ts": ts
            }
        })
        .to_string()
    }

    fn action_body(action_id: &str, value: &str, thread_ts: &str) -> String {
        let payload = json!({
            "type": "block_actions",
            "team": { "id": "T123" },
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

    #[tokio::test]
    async fn slack_events_url_verification_traverses_service_http_route() {
        let server = TestServer::start().await;
        let body = json!({
            "type": "url_verification",
            "challenge": "challenge-token"
        })
        .to_string();

        let response = post(server.addr, SLACK_EVENTS_PATH, &body, "application/json").await;

        assert_eq!(response.status, 200);
        assert_eq!(response.body, "challenge-token");
        server.stop().await;
    }

    #[tokio::test]
    async fn app_mention_accepts_persists_run_and_records_outbound() {
        let server = TestServer::start().await;
        let thread_ts = "1710000000.000100";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();

        let response = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body("inspect the repo", "EvE2EStart", thread_ts),
            "application/json",
        )
        .await;

        assert_eq!(response.status, 200);
        assert_eq!(response.body, "");
        let runtime = server.service.local_runtime();
        let run = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Finished);
        assert_eq!(
            runtime.active_run_id_for_thread(&thread_id).await.unwrap(),
            None
        );
        let durable = runtime.durable_run(&run.id).unwrap().unwrap();
        assert_eq!(durable.state, DurableRunState::Finished);

        let outbounds = runtime.outbound_for_thread(&thread_id);
        assert!(
            outbounds
                .iter()
                .any(|message| message.kind == LocalOutboundKind::Progress)
        );
        assert!(
            outbounds
                .iter()
                .any(|message| message.kind == LocalOutboundKind::Final)
        );
        server.stop().await;
    }

    #[tokio::test]
    async fn block_action_answer_resumes_waiting_run_to_completion() {
        let server = TestServer::start().await;
        let thread_ts = "1710000000.000200";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();

        let start = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body(
                "ask a question before continuing",
                "EvE2EQuestion",
                thread_ts,
            ),
            "application/json",
        )
        .await;
        assert_eq!(start.status, 200);

        let runtime = server.service.local_runtime();
        let waiting = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(waiting.status, RunStatus::Paused);
        assert_eq!(
            runtime.active_run_id_for_thread(&thread_id).await.unwrap(),
            Some(waiting.id.clone())
        );
        assert!(
            runtime
                .outbound_for_thread(&thread_id)
                .iter()
                .any(|message| message.kind == LocalOutboundKind::Question)
        );

        let answer = post(
            server.addr,
            SLACK_INTERACTIONS_PATH,
            &action_body(SLACK_ACTION_ANSWER, "ship it", thread_ts),
            "application/x-www-form-urlencoded",
        )
        .await;

        assert_eq!(answer.status, 200);
        let completed = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(completed.status, RunStatus::Finished);
        assert_eq!(
            runtime.active_run_id_for_thread(&thread_id).await.unwrap(),
            None
        );
        let durable = runtime.durable_run(&completed.id).unwrap().unwrap();
        assert_eq!(durable.state, DurableRunState::Finished);
        assert!(
            durable
                .events
                .iter()
                .any(|event| matches!(event.payload, DurableRunEventPayload::RunResumed { .. }))
        );
        server.stop().await;
    }

    #[tokio::test]
    async fn block_action_cancel_cancels_waiting_run() {
        let server = TestServer::start().await;
        let thread_ts = "1710000000.000300";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();

        let start = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body("ask a question before continuing", "EvE2ECancel", thread_ts),
            "application/json",
        )
        .await;
        assert_eq!(start.status, 200);

        let cancel = post(
            server.addr,
            SLACK_INTERACTIONS_PATH,
            &action_body(SLACK_ACTION_CANCEL, "cancel", thread_ts),
            "application/x-www-form-urlencoded",
        )
        .await;

        assert_eq!(cancel.status, 200);
        let runtime = server.service.local_runtime();
        let run = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Canceled);
        assert_eq!(
            runtime.active_run_id_for_thread(&thread_id).await.unwrap(),
            None
        );
        assert!(
            runtime
                .outbound_for_thread(&thread_id)
                .iter()
                .any(|message| message.kind == LocalOutboundKind::Cancelled)
        );
        server.stop().await;
    }
}
