use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ai_sdk_rust::{
    FinishReason, GatewayProvider, GatewayProviderSettings, ToolLoopAgentModelSettings,
    UiMessageChunk,
    open_agents_tools::{
        OpenAgentToolApprovalPolicy, OpenAgentToolsOptions, open_agent_tools_with_options,
    },
    provider_utils::{
        ExperimentalSandbox, SandboxCommandOptions as AiSandboxCommandOptions,
        SandboxCommandResult as AiSandboxCommandResult, SandboxRunCommandFuture,
    },
};
use ai_sdk_workflow::{
    DurableRunAgent, DurableRunAgentContext, DurableRunAgentError, DurableRunAgentOutput,
    DurableRunEngine, DurableRunError, DurableRunEventPayload, DurableRunExecution,
    DurableRunPause, DurableRunRecord, DurableRunResume, DurableRunStartOptions, DurableRunState,
    DurableRunStore, InMemoryDurableRunStore,
    chat_message_bridge::open_agent_message_from_stream_chunks,
};
use async_trait::async_trait;
use chat_sdk_adapter_slack::{
    SlackAdapter, SlackAdapterOptions,
    message_bridge::render_open_agent_message_for_slack_with_context,
    outbound::{
        SlackApprovalRequest, SlackCommitSummary, SlackGitSummaryStatus, SlackOutboundActionKind,
        SlackOutboundMessage, SlackPullRequestSummary, SlackQuestion, SlackQuestionOption,
        SlackQuestionPrompt, SlackRunContext, SlackRunTerminalStatus, decode_slack_action_id,
        render_approval_request, render_commit_summary, render_progress_update,
        render_pull_request_summary, render_question_prompt, render_run_error, render_run_started,
        render_run_terminal,
    },
};
use chat_sdk_chat::{
    open_agent_message::{OpenAgentMessagePart, OpenAgentMessageRole},
    types::Adapter,
};
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
use open_agents_runtime::RemoteAgentRunRequest;
use open_agents_runtime::{
    OpenAgent, OpenAgentCallOptions, OpenAgentPluginMcpServer, OpenAgentSettings,
    OpenAgentSkillMetadata,
};
use open_agents_sandbox::{
    GitCredentials, GitFinishOptions, GitFinishReport, GitFinishStatus, GitSandbox,
    JUST_BASH_DEFAULT_WORKING_DIRECTORY, PullRequestOptions, PushOptions, SandboxConnectConfig,
    SandboxConnectOptions, SandboxContext, SandboxError, SandboxExecOptions, SandboxState,
    connect_sandbox, run_git_finish,
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
use tokio::sync::Mutex as AsyncMutex;

use crate::config::{
    AgentFinishActions, AgentRuntimeMode, AgentToolApprovalMode, OpenAgentsServiceConfig,
    SandboxMode,
};
use crate::health::HealthCheck;
use crate::{SLACK_ACTION_ANSWER, SLACK_ACTION_CANCEL};

const SLACK_EVENTS_PATH: &str = "/slack/events";
const SLACK_INTERACTIONS_PATH: &str = "/slack/interactions";
const SLACK_COMMANDS_PATH: &str = "/slack/commands";
const OPEN_AGENTS_GATEWAY_APP_URL: &str = "https://open-agents.dev";
const OPEN_AGENTS_GATEWAY_APP_NAME: &str = "Open Agents";
const ASK_USER_TOOL_CALL_ID: &str = "ask-user-question";
const SANDBOX_TOOL_CALL_ID: &str = "sandbox-pwd";
const SANDBOX_APPROVAL_ID: &str = "sandbox-pwd-approval";
const JUST_BASH_CONFORMANCE_TOOL_CALL_ID: &str = "just-bash-conformance";

/// Deployable Open Agents service with Slack HTTP routes and local runtime
/// wiring.
#[derive(Clone, Debug)]
pub struct OpenAgentsService {
    health: HealthCheck,
    slack_ingress: Arc<SlackIngress>,
    runtime: Arc<LocalRuntimeRouter>,
    slack_outbound: Option<SlackAdapter>,
    posted_outbounds: Arc<AsyncMutex<usize>>,
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
        let slack_outbound =
            if config.runtime() == AgentRuntimeMode::Gateway || config.slack_api_url().is_some() {
                let mut slack_options = SlackAdapterOptions::new(
                    config.slack_bot_token().to_string(),
                    config.slack_signing_secret().to_string(),
                );
                if let Some(api_base) = config.slack_api_url() {
                    slack_options = slack_options.with_api_base(api_base.to_string());
                }
                Some(SlackAdapter::new(slack_options))
            } else {
                None
            };

        Ok(Self {
            health,
            slack_ingress,
            runtime,
            slack_outbound,
            posted_outbounds: Arc::new(AsyncMutex::new(0)),
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

    /// Handle an HTTP request from an external runtime adapter.
    pub async fn handle_http_request(&self, request: ServiceHttpRequest) -> ServiceHttpResponse {
        ServiceHttpResponse::from(self.handle(HttpRequest::from(request)).await)
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
        if (200..300).contains(&response.status) {
            if let Err(error) = self.flush_slack_outbounds().await {
                return HttpResponse::text(500, format!("{error}\n"));
            }
        }
        HttpResponse::from_slack(response)
    }

    async fn flush_slack_outbounds(&self) -> Result<(), ServiceError> {
        let Some(adapter) = &self.slack_outbound else {
            return Ok(());
        };

        let outbounds = self.runtime.outbound_messages();
        let mut posted = self.posted_outbounds.lock().await;
        for (index, outbound) in outbounds.iter().enumerate().skip(*posted) {
            adapter
                .post_message(&outbound.thread_id, &outbound.text)
                .await
                .map_err(|error| ServiceError::SlackOutbound(error.to_string()))?;
            *posted = index + 1;
        }
        Ok(())
    }
}

/// Runtime-neutral HTTP request passed to the service router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceHttpRequest {
    /// HTTP method such as `GET` or `POST`.
    pub method: String,
    /// Route path as seen by the service, for example `/slack/events`.
    pub path: String,
    /// HTTP headers as name/value pairs. Header names are compared
    /// case-insensitively by the downstream Slack adapter.
    pub headers: Vec<(String, String)>,
    /// UTF-8 request body.
    pub body: String,
}

impl ServiceHttpRequest {
    /// Build a request for the service router.
    pub fn new(
        method: impl Into<String>,
        path: impl Into<String>,
        headers: Vec<(String, String)>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            headers,
            body: body.into(),
        }
    }
}

/// Runtime-neutral HTTP response returned by the service router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceHttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response content type.
    pub content_type: String,
    /// UTF-8 response body.
    pub body: String,
}

/// Runtime/outbound kind captured by the local deterministic service route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalOutboundKind {
    Progress,
    Question,
    Final,
    Cancelled,
    Failed,
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
    engine: Mutex<DurableRunEngine<InMemoryDurableRunStore, ServiceAgent>>,
    scenarios: Arc<Mutex<HashMap<String, ScriptedRunScenario>>>,
    outbounds: Mutex<Vec<LocalOutbound>>,
    sandbox: ServiceSandbox,
    model_id: String,
    finish_actions: AgentFinishActions,
    plugin_skills: Vec<OpenAgentSkillMetadata>,
    plugin_mcp_servers: Vec<OpenAgentPluginMcpServer>,
}

impl LocalRuntimeRouter {
    fn new(config: &OpenAgentsServiceConfig) -> Result<Self, ServiceError> {
        let scenarios = Arc::new(Mutex::new(HashMap::new()));
        let agent = match config.runtime() {
            AgentRuntimeMode::Fixture => ServiceAgent::Scripted(LocalScriptedAgent {
                scenarios: scenarios.clone(),
            }),
            AgentRuntimeMode::Gateway => ServiceAgent::Gateway(GatewayOpenAgent::new(
                scenarios.clone(),
                GatewayOpenAgentSettings::from_config(config)?,
            )),
        };
        Ok(Self {
            persistence: MemoryPersistenceStore::new(),
            engine: Mutex::new(DurableRunEngine::new(InMemoryDurableRunStore::new(), agent)),
            scenarios,
            outbounds: Mutex::new(Vec::new()),
            sandbox: ServiceSandbox::from_config(config)?,
            model_id: config.model_id().to_string(),
            finish_actions: config.finish_actions().clone(),
            plugin_skills: config.plugin_catalog().runtime_skills(),
            plugin_mcp_servers: config.plugin_catalog().runtime_mcp_servers(),
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

    /// Namespaced Open Plugin skills configured for runtime calls.
    pub fn plugin_skills(&self) -> Vec<OpenAgentSkillMetadata> {
        self.plugin_skills.clone()
    }

    /// Sanitized Open Plugin MCP servers configured for runtime planning.
    pub fn plugin_mcp_servers(&self) -> Vec<OpenAgentPluginMcpServer> {
        self.plugin_mcp_servers.clone()
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

    /// Return persisted stream chunks for a durable run from `start_index`.
    pub fn stream_chunks_since(
        &self,
        run_id: &str,
        start_index: usize,
    ) -> Result<Vec<UiMessageChunk>, ServiceError> {
        self.engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .stream_chunks_since(run_id, start_index)
            .map_err(ServiceError::DurableRun)
    }

    fn execution_for_run(&self, run_id: &str) -> Result<DurableRunExecution, ServiceError> {
        let record = self
            .durable_run(run_id)?
            .ok_or_else(|| ServiceError::NotFound(run_id.to_string()))?;
        Ok(execution_from_record(record))
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
            model_id: Some(self.model_id.clone()),
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
        let run_sandbox = self.sandbox.for_run(&run_id);
        let user_id = request
            .user_id
            .clone()
            .unwrap_or_else(|| mapping.user_id.clone());

        if let Some(existing_run_id) = self
            .persistence
            .get_chat(&mapping.chat_id)
            .await
            .map_err(ServiceError::Persistence)?
            .and_then(|chat| chat.active_run_id)
        {
            if let Some(durable) = self.durable_run(&existing_run_id)? {
                if durable.state.is_terminal() {
                    self.persistence
                        .compare_and_set_chat_active_run(
                            &mapping.chat_id,
                            Some(existing_run_id),
                            None,
                        )
                        .await
                        .map_err(ServiceError::Persistence)?;
                } else {
                    return self
                        .resume_or_reconnect_start(mapping, request, existing_run_id, durable)
                        .await;
                }
            } else {
                self.persistence
                    .compare_and_set_chat_active_run(&mapping.chat_id, Some(existing_run_id), None)
                    .await
                    .map_err(ServiceError::Persistence)?;
            }
        }

        self.persist_user_message(&mapping, &request).await?;
        self.persistence
            .create_run(CreateRunInput {
                id: run_id.clone(),
                session_id: mapping.session_id.clone(),
                chat_id: mapping.chat_id.clone(),
                user_id: user_id.clone(),
                model_id: Some(self.model_id.clone()),
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
        self.persistence
            .update_sandbox_state(
                &mapping.session_id,
                SandboxStateUpdate {
                    lifecycle_state: LifecycleState::Running,
                    sandbox_state: Some(run_sandbox.persistence_state()),
                    lifecycle_error: None,
                },
            )
            .await
            .map_err(ServiceError::Persistence)?;

        let runtime_request = RemoteAgentRunRequest::new(
            RemoteAgentIdentity::new(mapping.session_id.clone(), mapping.chat_id.clone())
                .with_run_id(run_id.clone()),
            AgentModelSelection::new(self.model_id.clone()),
            run_sandbox.runtime_context(),
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
                ScriptedRunScenario::new(request.text.clone(), runtime_request, run_sandbox),
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
            engine.start(DurableRunStartOptions::new(
                run_id.clone(),
                mapping.chat_id.clone(),
            ))
        };
        let execution = match execution {
            Ok(execution) => execution,
            Err(error) => {
                let execution = self.execution_for_run(&run_id)?;
                self.apply_execution(&mapping, &run_id, &execution, None)
                    .await?;
                return Ok(
                    SlackRunHandoff::new(Some(run_id), false).with_metadata(json!({
                        "sessionId": mapping.session_id,
                        "chatId": mapping.chat_id,
                        "state": durable_state_label(execution.state),
                        "error": error.to_string(),
                    })),
                );
            }
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

    async fn resume_or_reconnect_start(
        &self,
        mapping: SlackThreadMappingRecord,
        request: SlackRunStartRequest,
        run_id: String,
        durable: DurableRunRecord,
    ) -> Result<SlackRunHandoff, ServiceError> {
        self.persist_user_message(&mapping, &request).await?;

        if durable.state == DurableRunState::WaitingForInput {
            let tool_call_id = waiting_tool_input_call_id(&durable)
                .unwrap_or_else(|| ASK_USER_TOOL_CALL_ID.into());
            let answer = request.text.trim().to_string();
            return self
                .resume_with_tool_input(&mapping, &run_id, tool_call_id, answer)
                .await;
        }

        Ok(
            SlackRunHandoff::new(Some(run_id), true).with_metadata(json!({
                "sessionId": mapping.session_id,
                "chatId": mapping.chat_id,
                "state": durable_state_label(durable.state),
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
            || decoded
                .as_ref()
                .is_some_and(|action| action.kind == SlackOutboundActionKind::QuestionDecline);
        let is_approval = decoded.as_ref().is_some_and(|action| {
            matches!(
                action.kind,
                SlackOutboundActionKind::Approve | SlackOutboundActionKind::Deny
            )
        }) || matches!(action.action_id.as_str(), "approve" | "deny");
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

        if is_approval {
            let approved = decoded
                .as_ref()
                .is_some_and(|action| action.kind == SlackOutboundActionKind::Approve)
                || action.action_id == "approve";
            let durable = self
                .durable_run(&run_id)?
                .ok_or_else(|| ServiceError::NotFound(run_id.clone()))?;
            let (approval_id, tool_call_id) = waiting_tool_approval(&durable).ok_or_else(|| {
                ServiceError::Unsupported("run is not waiting for approval".to_string())
            })?;
            return self
                .resume_with_tool_approval(&mapping, &run_id, approval_id, tool_call_id, approved)
                .await;
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

        let durable = self
            .durable_run(&run_id)?
            .ok_or_else(|| ServiceError::NotFound(run_id.clone()))?;
        let tool_call_id =
            waiting_tool_input_call_id(&durable).unwrap_or_else(|| ASK_USER_TOOL_CALL_ID.into());
        self.resume_with_tool_input(&mapping, &run_id, tool_call_id, answer)
            .await
    }

    async fn resume_with_tool_input(
        &self,
        mapping: &SlackThreadMappingRecord,
        run_id: &str,
        tool_call_id: String,
        answer: String,
    ) -> Result<SlackRunHandoff, ServiceError> {
        let resume = DurableRunResume::ToolInput {
            tool_call_id,
            input: json!({ "answer": answer }),
        };
        let execution = {
            let mut engine = self
                .engine
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            engine.resume(run_id, resume)
        };
        let execution = match execution {
            Ok(execution) => execution,
            Err(error) => {
                let execution = self.execution_for_run(run_id)?;
                self.apply_execution(mapping, run_id, &execution, None)
                    .await?;
                return Ok(
                    SlackRunHandoff::new(Some(run_id.to_string()), true).with_metadata(json!({
                        "state": durable_state_label(execution.state),
                        "error": error.to_string(),
                    })),
                );
            }
        };
        self.record_outbound(
            mapping,
            run_id,
            LocalOutboundKind::Progress,
            render_progress_update(&format!("Received answer: {answer}")),
        );
        self.apply_execution(mapping, run_id, &execution, Some(answer))
            .await?;

        Ok(
            SlackRunHandoff::new(Some(run_id.to_string()), true).with_metadata(json!({
                "state": durable_state_label(execution.state),
            })),
        )
    }

    async fn resume_with_tool_approval(
        &self,
        mapping: &SlackThreadMappingRecord,
        run_id: &str,
        approval_id: String,
        tool_call_id: String,
        approved: bool,
    ) -> Result<SlackRunHandoff, ServiceError> {
        let resume = DurableRunResume::ToolApproval {
            approval_id,
            tool_call_id,
            approved,
            reason: Some(if approved {
                "approved from Slack".to_string()
            } else {
                "denied from Slack".to_string()
            }),
        };
        let execution = {
            let mut engine = self
                .engine
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            engine.resume(run_id, resume)
        };
        let execution = match execution {
            Ok(execution) => execution,
            Err(error) => {
                let execution = self.execution_for_run(run_id)?;
                self.apply_execution(mapping, run_id, &execution, None)
                    .await?;
                return Ok(
                    SlackRunHandoff::new(Some(run_id.to_string()), true).with_metadata(json!({
                        "state": durable_state_label(execution.state),
                        "error": error.to_string(),
                    })),
                );
            }
        };
        let decision = if approved { "approved" } else { "denied" };
        self.record_outbound(
            mapping,
            run_id,
            LocalOutboundKind::Progress,
            render_progress_update(&format!("Tool approval {decision}")),
        );
        self.apply_execution(mapping, run_id, &execution, None)
            .await?;

        Ok(
            SlackRunHandoff::new(Some(run_id.to_string()), true).with_metadata(json!({
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
                .ok_or(ServiceError::NotFound(record.chat_id));
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
                    render_run_terminal(SlackRunTerminalStatus::Finished, Some("run complete")),
                );
                for message in self.finish_action_messages(run_id) {
                    self.record_outbound(mapping, run_id, LocalOutboundKind::Final, message);
                }
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
                let error = run_failed_message(execution)
                    .unwrap_or_else(|| "local runtime failed".to_string());
                self.persistence
                    .update_run_status(
                        run_id,
                        RunStatusUpdate {
                            status: RunStatus::Failed,
                            finished_at: Some(OffsetDateTime::now_utc()),
                            error: Some(error.clone()),
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
                    LocalOutboundKind::Failed,
                    render_run_error(&error),
                );
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
                let (approval_id, tool_call_id) = waiting_approval_from_execution(execution)
                    .unwrap_or_else(|| {
                        (
                            SANDBOX_APPROVAL_ID.to_string(),
                            SANDBOX_TOOL_CALL_ID.to_string(),
                        )
                    });
                self.record_outbound(
                    mapping,
                    run_id,
                    LocalOutboundKind::Question,
                    render_approval_request(
                        &SlackRunContext::new(run_id, format!("{run_id}-approval")),
                        &SlackApprovalRequest {
                            approval_id,
                            tool_call_id,
                            tool_name: "bash".to_string(),
                            title: "Approve sandbox command execution".to_string(),
                            details: Some("Run `pwd` in the selected sandbox.".to_string()),
                        },
                    ),
                );
            }
        }

        Ok(())
    }

    fn finish_action_messages(&self, run_id: &str) -> Vec<SlackOutboundMessage> {
        if !self.finish_actions.git_enabled {
            return Vec::new();
        }
        let repository = match self.sandbox.git_repository() {
            Ok(Some(repository)) => repository,
            Ok(None) => {
                return vec![render_progress_update(
                    "Finish actions skipped: git finish is only available for local sandbox repositories",
                )];
            }
            Err(error) => {
                return vec![render_run_error(&format!("Finish actions failed: {error}"))];
            }
        };
        match run_git_finish(&repository, &self.git_finish_options(run_id)) {
            Ok(report) => slack_messages_from_git_finish_report(&report),
            Err(error) => vec![render_run_error(&format!("Finish actions failed: {error}"))],
        }
    }

    fn git_finish_options(&self, run_id: &str) -> GitFinishOptions {
        let credentials = self.github_token_credentials();
        GitFinishOptions {
            commit_message: self.finish_actions.commit_message.clone(),
            push: PushOptions {
                mode: self.finish_actions.push_mode,
                remote: "origin".to_string(),
                branch: None,
                credentials: credentials.clone(),
            },
            pull_request: PullRequestOptions {
                mode: self.finish_actions.pull_request_mode,
                base: self.finish_actions.pull_request_base.clone(),
                head: None,
                title: self.finish_actions.pull_request_title.clone(),
                body: format!("{}\n\nRun: {run_id}", self.finish_actions.pull_request_body),
                repository: self.finish_actions.pull_request_repository.clone(),
                credentials,
            },
        }
    }

    fn github_token_credentials(&self) -> Option<GitCredentials> {
        self.sandbox
            .connect
            .options
            .github_token
            .as_ref()
            .map(|token| GitCredentials::new(token.clone()))
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
        if execution.chunks.is_empty()
            && !matches!(
                execution.state,
                DurableRunState::Canceled | DurableRunState::Failed | DurableRunState::Finished
            )
        {
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

        let inserted = self
            .persistence
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
            .map_err(ServiceError::Persistence)?
            .inserted;
        if inserted {
            self.persistence
                .touch_chat(&mapping.chat_id, Some(OffsetDateTime::now_utc()))
                .await
                .map_err(ServiceError::Persistence)?;
        }
        Ok(())
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
            SandboxMode::JustBash => {
                let state = SandboxState::JustBash {
                    workspace_id: "open-agents-service".to_string(),
                    working_directory: JUST_BASH_DEFAULT_WORKING_DIRECTORY.to_string(),
                    current_branch: None,
                    expires_at: None,
                };
                let raw =
                    serde_json::to_value(&state).unwrap_or_else(|_| json!({"type": "just-bash"}));
                let environment_details =
                    "Just Bash virtual filesystem; no host shell or external sandbox provider"
                        .to_string();
                Ok(Self {
                    connect: sandbox_connect_config(state, config),
                    context: SandboxContext::new(raw.clone(), JUST_BASH_DEFAULT_WORKING_DIRECTORY)
                        .with_environment_details(environment_details.clone()),
                    persisted: PersistedSandboxState {
                        provider: "just-bash".to_string(),
                        sandbox_id: None,
                        sandbox_name: None,
                        working_directory: Some(JUST_BASH_DEFAULT_WORKING_DIRECTORY.to_string()),
                        current_branch: None,
                        environment_details: Some(environment_details),
                        raw,
                    },
                })
            }
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
                    connect: sandbox_connect_config(state, config),
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
            SandboxMode::Vercel {
                base_snapshot_id,
                sandbox_name,
            } => {
                let state = SandboxState::Vercel {
                    source: None,
                    sandbox_name: sandbox_name.clone(),
                    sandbox_id: None,
                    snapshot_id: base_snapshot_id.clone(),
                    expires_at: None,
                };
                Ok(Self {
                    connect: sandbox_connect_config(state.clone(), config),
                    context: SandboxContext::new(
                        serde_json::to_value(&state).unwrap_or_else(|_| json!({"type": "vercel"})),
                        "/vercel/sandbox",
                    ),
                    persisted: PersistedSandboxState {
                        provider: "vercel".to_string(),
                        sandbox_id: None,
                        sandbox_name: sandbox_name.clone(),
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

    fn for_run(&self, run_id: &str) -> Self {
        if matches!(&self.connect.state, SandboxState::JustBash { .. }) {
            let mut next = self.clone();
            let state = SandboxState::JustBash {
                workspace_id: format!("run_{run_id}"),
                working_directory: JUST_BASH_DEFAULT_WORKING_DIRECTORY.to_string(),
                current_branch: None,
                expires_at: None,
            };
            next.connect.state = state.clone();
            let raw = serde_json::to_value(&state).unwrap_or_else(|_| json!({"type": "just-bash"}));
            next.context.state = raw.clone();
            next.persisted.raw = raw;
            return next;
        }

        let SandboxState::Vercel {
            sandbox_name: None,
            sandbox_id: None,
            ..
        } = &self.connect.state
        else {
            return self.clone();
        };

        let sandbox_name = run_sandbox_name(run_id);
        let mut connect = self.connect.clone();
        if let SandboxState::Vercel {
            sandbox_name: name, ..
        } = &mut connect.state
        {
            *name = Some(sandbox_name.clone());
        }

        let mut context = self.context.clone();
        context.state = serde_json::to_value(&connect.state).unwrap_or_else(|_| {
            json!({
                "type": "vercel",
                "sandboxName": sandbox_name,
            })
        });

        let mut persisted = self.persisted.clone();
        persisted.sandbox_name = Some(sandbox_name);
        persisted.raw = context.state.clone();

        Self {
            connect,
            context,
            persisted,
        }
    }

    fn runtime_context(&self) -> SandboxContext {
        self.context.clone()
    }

    fn persistence_state(&self) -> PersistedSandboxState {
        self.persisted.clone()
    }

    fn git_repository(&self) -> Result<Option<GitSandbox>, ServiceError> {
        let SandboxState::Local {
            root,
            working_directory,
            ..
        } = &self.connect.state
        else {
            return Ok(None);
        };
        let repository = GitSandbox::open(root, working_directory)
            .map_err(|error| ServiceError::FinishAction(error.to_string()))?;
        Ok(Some(repository))
    }
}

fn sandbox_connect_config(
    state: SandboxState,
    config: &OpenAgentsServiceConfig,
) -> SandboxConnectConfig {
    let mut options = SandboxConnectOptions::new()
        .with_env("GIT_TERMINAL_PROMPT", "0")
        .with_timeout_ms(120_000);
    if let Some(token) = config.github_token() {
        options = options
            .with_env("GITHUB_TOKEN", token)
            .with_env("GH_TOKEN", token)
            .with_github_token(token);
    }
    SandboxConnectConfig::new(state).with_options(options)
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

    fn wants_approval(&self) -> bool {
        self.prompt.to_ascii_lowercase().contains("approval")
    }

    fn wants_model_error(&self) -> bool {
        let prompt = self.prompt.to_ascii_lowercase();
        prompt.contains("model error") || prompt.contains("fail model")
    }

    fn wants_sandbox_error(&self) -> bool {
        self.prompt.to_ascii_lowercase().contains("sandbox error")
    }

    fn wants_just_bash_conformance(&self) -> bool {
        let prompt = self.prompt.to_ascii_lowercase();
        prompt.contains("just bash conformance") || prompt.contains("just-bash conformance")
    }
}

#[derive(Debug, Clone)]
enum ServiceAgent {
    Scripted(LocalScriptedAgent),
    Gateway(GatewayOpenAgent),
}

impl DurableRunAgent for ServiceAgent {
    fn next_turn(
        &mut self,
        context: DurableRunAgentContext,
    ) -> Result<DurableRunAgentOutput, DurableRunAgentError> {
        match self {
            Self::Scripted(agent) => agent.next_turn(context),
            Self::Gateway(agent) => agent.next_turn(context),
        }
    }
}

#[derive(Clone)]
struct GatewayOpenAgentSettings {
    api_key: String,
    model_id: String,
    max_steps: usize,
    max_output_tokens: u64,
    tool_approval: AgentToolApprovalMode,
    plugin_skills: Vec<OpenAgentSkillMetadata>,
    plugin_mcp_servers: Vec<OpenAgentPluginMcpServer>,
}

impl fmt::Debug for GatewayOpenAgentSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayOpenAgentSettings")
            .field("api_key", &"<redacted>")
            .field("model_id", &self.model_id)
            .field("max_steps", &self.max_steps)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("tool_approval", &self.tool_approval)
            .finish()
    }
}

impl GatewayOpenAgentSettings {
    fn from_config(config: &OpenAgentsServiceConfig) -> Result<Self, ServiceError> {
        let api_key = config.model_api_key().ok_or_else(|| {
            ServiceError::BadRequest("AI Gateway runtime requires AI_GATEWAY_API_KEY".to_string())
        })?;
        Ok(Self {
            api_key: api_key.to_string(),
            model_id: config.model_id().to_string(),
            max_steps: config.model_max_steps(),
            max_output_tokens: config.model_max_output_tokens(),
            tool_approval: config.tool_approval(),
            plugin_skills: config.plugin_catalog().runtime_skills(),
            plugin_mcp_servers: config.plugin_catalog().runtime_mcp_servers(),
        })
    }
}

#[derive(Debug, Clone)]
struct GatewayOpenAgent {
    scenarios: Arc<Mutex<HashMap<String, ScriptedRunScenario>>>,
    settings: GatewayOpenAgentSettings,
}

impl GatewayOpenAgent {
    fn new(
        scenarios: Arc<Mutex<HashMap<String, ScriptedRunScenario>>>,
        settings: GatewayOpenAgentSettings,
    ) -> Self {
        Self {
            scenarios,
            settings,
        }
    }

    fn prompt_instructions(&self, scenario: &ScriptedRunScenario) -> String {
        format!(
            "You are Open Agents running from Slack in Vercel cloud. Use the sandbox tools to inspect, clone, edit, test, commit, push, and open pull requests when the user asks for repository work. For GitHub repositories, use sandbox `git` commands for clone, branch, commit, and push; authenticate git over HTTPS with `GIT_TERMINAL_PROMPT=0` and the GH_TOKEN/GITHUB_TOKEN environment without printing the token. Create pull requests with the `github_create_pull_request` tool after the branch is pushed. Do not print tokens or include them in remotes, logs, commits, or Slack messages. Keep Slack responses concise and report branch names, commits, PR URLs, and verification results. The current sandbox working directory is {}.",
            scenario.runtime_request.sandbox.working_directory
        )
    }
}

impl DurableRunAgent for GatewayOpenAgent {
    fn next_turn(
        &mut self,
        context: DurableRunAgentContext,
    ) -> Result<DurableRunAgentOutput, DurableRunAgentError> {
        if let Some(resume) = context.resume {
            return Ok(DurableRunAgentOutput::Finished {
                chunks: chunks_from_gateway_resume(&context.run_id, &resume),
            });
        }

        let scenario = self
            .scenarios
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&context.run_id)
            .cloned()
            .ok_or_else(|| DurableRunAgentError::new("missing Gateway run scenario"))?;
        let settings = self.settings.clone();
        let run_id = context.run_id.clone();
        let instructions = self.prompt_instructions(&scenario);
        let chunks = run_gateway_future_to_completion(move || async move {
            generate_gateway_result(settings, scenario, instructions)
                .await
                .map(|result| chunks_from_gateway_result(&run_id, result))
        })
        .map_err(|error| DurableRunAgentError::new(format!("Gateway Open Agent failed: {error}")))?
        .map_err(|error| {
            DurableRunAgentError::new(format!("Gateway Open Agent failed: {error}"))
        })?;
        Ok(DurableRunAgentOutput::Finished { chunks })
    }
}

async fn generate_gateway_result(
    settings: GatewayOpenAgentSettings,
    scenario: ScriptedRunScenario,
    instructions: String,
) -> Result<ai_sdk_rust::GenerateTextResult, String> {
    let sandbox: Arc<dyn ExperimentalSandbox> = Arc::new(ServiceExperimentalSandbox::new(
        scenario.sandbox.connect.clone(),
    ));
    let provider = GatewayProvider::from_settings(native_gateway_settings(settings.api_key));
    let model = provider.language_model(settings.model_id.clone());
    let tool_options = OpenAgentToolsOptions::new()
        .with_working_directory(scenario.runtime_request.sandbox.working_directory.clone())
        .with_approval_policy(match settings.tool_approval {
            AgentToolApprovalMode::Sensitive => OpenAgentToolApprovalPolicy::Sensitive,
            AgentToolApprovalMode::Never => OpenAgentToolApprovalPolicy::Never,
            AgentToolApprovalMode::Always => OpenAgentToolApprovalPolicy::Always,
        });
    let mut agent_settings = OpenAgentSettings::new(&model)
        .with_model_id(settings.model_id.clone())
        .with_custom_instructions(instructions)
        .with_model_settings(
            ToolLoopAgentModelSettings::new()
                .with_max_output_tokens(settings.max_output_tokens)
                .with_temperature(0.2),
        );
    for tool in open_agent_tools_with_options(tool_options) {
        agent_settings = agent_settings.with_tool(tool);
    }
    let agent = OpenAgent::new(agent_settings);
    let mut call =
        OpenAgentCallOptions::from_prompt(scenario.prompt, scenario.runtime_request.sandbox)
            .with_model(AgentModelSelection::new(settings.model_id))
            .with_skills(settings.plugin_skills)
            .with_plugin_mcp_servers(settings.plugin_mcp_servers);
    call.tool_loop_options = call
        .tool_loop_options
        .with_experimental_sandbox(sandbox)
        .with_max_steps(settings.max_steps);

    agent
        .generate(call)
        .await
        .map_err(|error| error.to_string())
}

fn native_gateway_settings(api_key: String) -> GatewayProviderSettings {
    GatewayProviderSettings::new()
        .with_api_key(api_key)
        .with_header("http-referer", OPEN_AGENTS_GATEWAY_APP_URL)
        .with_header("x-title", OPEN_AGENTS_GATEWAY_APP_NAME)
}

#[derive(Debug, Clone)]
struct ServiceExperimentalSandbox {
    connect: SandboxConnectConfig,
    description: String,
}

impl ServiceExperimentalSandbox {
    fn new(connect: SandboxConnectConfig) -> Self {
        let description = format!("Open Agents {} sandbox", connect.state.sandbox_type());
        Self {
            connect,
            description,
        }
    }
}

impl ExperimentalSandbox for ServiceExperimentalSandbox {
    fn description(&self) -> &str {
        &self.description
    }

    fn run_command(&self, options: AiSandboxCommandOptions) -> SandboxRunCommandFuture {
        let connect = self.connect.clone();
        Box::pin(async move { run_service_sandbox_command(connect, options) })
    }
}

fn run_service_sandbox_command(
    connect: SandboxConnectConfig,
    options: AiSandboxCommandOptions,
) -> AiSandboxCommandResult {
    let sandbox = match connect_sandbox(connect) {
        Ok(sandbox) => sandbox,
        Err(error) => {
            return AiSandboxCommandResult::new(1)
                .with_stderr(format!("failed to connect sandbox: {error}"));
        }
    };
    let mut exec_options = SandboxExecOptions::new(options.command);
    if let Some(cwd) = options.working_directory {
        exec_options = exec_options.with_cwd(cwd);
    }
    match sandbox.exec(exec_options) {
        Ok(result) => AiSandboxCommandResult::new(result.exit_code.unwrap_or(if result.success {
            0
        } else {
            1
        }))
        .with_stdout(result.stdout)
        .with_stderr(result.stderr),
        Err(error) => AiSandboxCommandResult::new(1).with_stderr(error.to_string()),
    }
}

fn chunks_from_gateway_result(
    run_id: &str,
    result: ai_sdk_rust::GenerateTextResult,
) -> Vec<UiMessageChunk> {
    let mut chunks = vec![
        UiMessageChunk::start_with_message_id(assistant_message_id(
            run_id,
            DurableRunState::Finished,
        )),
        UiMessageChunk::start_step(),
    ];

    if !result.text.trim().is_empty() {
        chunks.push(UiMessageChunk::text_start("text-1"));
        chunks.push(UiMessageChunk::text_delta("text-1", result.text.clone()));
        chunks.push(UiMessageChunk::text_end("text-1"));
    }

    for tool_call in &result.tool_calls {
        chunks.push(UiMessageChunk::tool_input_available(
            tool_call.tool_call_id.clone(),
            tool_call.tool_name.clone(),
            tool_call.input.clone(),
        ));
    }
    for tool_result in &result.tool_results {
        chunks.push(UiMessageChunk::tool_output_available(
            tool_result.tool_call_id.clone(),
            tool_result.output.clone(),
        ));
    }

    if result.text.trim().is_empty() && result.tool_results.is_empty() {
        chunks.push(UiMessageChunk::text_start("text-1"));
        chunks.push(UiMessageChunk::text_delta(
            "text-1",
            "Gateway run completed without final text.",
        ));
        chunks.push(UiMessageChunk::text_end("text-1"));
    }

    chunks.push(UiMessageChunk::finish_step());
    chunks.push(UiMessageChunk::finish_with_reason(result.finish_reason));
    chunks
}

fn chunks_from_gateway_resume(run_id: &str, resume: &DurableRunResume) -> Vec<UiMessageChunk> {
    let text = match resume {
        DurableRunResume::ToolInput { input, .. } => format!(
            "Gateway run resumed from Slack answer: {}.",
            input
                .get("answer")
                .and_then(|answer| answer.as_str())
                .unwrap_or("received")
        ),
        DurableRunResume::ToolApproval { approved, .. } => {
            let decision = if *approved { "approved" } else { "denied" };
            format!("Gateway run resumed from Slack approval: {decision}.")
        }
    };
    vec![
        UiMessageChunk::start_with_message_id(assistant_message_id(
            run_id,
            DurableRunState::Finished,
        )),
        UiMessageChunk::start_step(),
        UiMessageChunk::text_start("text-1"),
        UiMessageChunk::text_delta("text-1", text),
        UiMessageChunk::text_end("text-1"),
        UiMessageChunk::finish_step(),
        UiMessageChunk::finish_with_reason(FinishReason::Stop),
    ]
}

fn run_gateway_future_to_completion<T, F, Fut>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = T> + 'static,
{
    let thread = std::thread::Builder::new()
        .name("open-agents-gateway-runtime".to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("failed to start Gateway async runtime: {error}"))?;
            Ok(runtime.block_on(operation()))
        })
        .map_err(|error| format!("failed to spawn Gateway async runtime: {error}"))?;

    thread
        .join()
        .map_err(|_| "Gateway async runtime panicked".to_string())?
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

        if context.resume.is_none() && scenario.wants_model_error() {
            return Err(DurableRunAgentError::new(
                "scripted model error requested by Slack prompt",
            ));
        }

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

        if context.resume.is_none()
            && scenario.wants_approval()
            && !context.previous_events.iter().any(|event| {
                matches!(
                    event.payload,
                    DurableRunEventPayload::WaitingForApproval { .. }
                )
            })
        {
            return Ok(DurableRunAgentOutput::WaitingForApproval {
                approval_id: SANDBOX_APPROVAL_ID.to_string(),
                tool_call_id: SANDBOX_TOOL_CALL_ID.to_string(),
                chunks: vec![
                    UiMessageChunk::start_with_message_id(assistant_message_id(
                        &context.run_id,
                        DurableRunState::WaitingForApproval,
                    )),
                    UiMessageChunk::start_step(),
                    UiMessageChunk::tool_input_available(
                        SANDBOX_TOOL_CALL_ID,
                        "bash",
                        json!({ "command": "pwd" }),
                    ),
                    UiMessageChunk::tool_approval_request(
                        SANDBOX_APPROVAL_ID,
                        SANDBOX_TOOL_CALL_ID,
                    ),
                    UiMessageChunk::finish_step(),
                ],
            });
        }

        let answer = match &context.resume {
            Some(DurableRunResume::ToolInput { input, .. }) => input
                .get("answer")
                .and_then(|answer| answer.as_str())
                .map(str::to_string),
            _ => None,
        };
        let approval = match &context.resume {
            Some(DurableRunResume::ToolApproval { approved, .. }) => Some(*approved),
            _ => None,
        };
        if approval == Some(false) {
            return Ok(DurableRunAgentOutput::Finished {
                chunks: vec![
                    UiMessageChunk::start_with_message_id(assistant_message_id(
                        &context.run_id,
                        DurableRunState::Finished,
                    )),
                    UiMessageChunk::start_step(),
                    UiMessageChunk::tool_approval_response(SANDBOX_APPROVAL_ID, false),
                    UiMessageChunk::tool_output_denied(SANDBOX_TOOL_CALL_ID, "bash"),
                    UiMessageChunk::text_start("text-1"),
                    UiMessageChunk::text_delta(
                        "text-1",
                        "Fixture approval denied; sandbox command was not executed.",
                    ),
                    UiMessageChunk::text_end("text-1"),
                    UiMessageChunk::finish_step(),
                    UiMessageChunk::finish_with_reason(FinishReason::Stop),
                ],
            });
        }

        if scenario.wants_just_bash_conformance() {
            let report = run_just_bash_conformance_probe(&context.run_id, &scenario)?;
            return Ok(DurableRunAgentOutput::Finished {
                chunks: chunks_from_just_bash_conformance_report(&context.run_id, report),
            });
        }

        let sandbox = connect_sandbox(scenario.sandbox.connect.clone())
            .map_err(|error| DurableRunAgentError::new(error.to_string()))?;
        let mut exec_options = SandboxExecOptions::new("pwd");
        if scenario.wants_sandbox_error() {
            exec_options = exec_options.with_cwd("/definitely-outside-open-agents-sandbox");
        }
        let result = sandbox
            .exec(exec_options)
            .map_err(|error| DurableRunAgentError::new(error.to_string()))?;
        let pwd = result.stdout.trim();
        let final_text = match answer.as_deref() {
            Some(answer) => format!(
                "Fixture agent finished after answer `{answer}` in {}.",
                scenario.runtime_request.sandbox.working_directory
            ),
            None if approval == Some(true) => {
                format!("Fixture agent finished after approval in {pwd}.")
            }
            None => format!("Fixture agent finished with local sandbox proof in {pwd}."),
        };

        let mut chunks = vec![
            UiMessageChunk::start_with_message_id(assistant_message_id(
                &context.run_id,
                DurableRunState::Finished,
            )),
            UiMessageChunk::start_step(),
        ];
        if approval == Some(true) {
            chunks.push(UiMessageChunk::tool_approval_response(
                SANDBOX_APPROVAL_ID,
                true,
            ));
        }
        chunks.extend([
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
        ]);

        Ok(DurableRunAgentOutput::Finished { chunks })
    }
}

fn run_just_bash_conformance_probe(
    run_id: &str,
    scenario: &ScriptedRunScenario,
) -> Result<serde_json::Value, DurableRunAgentError> {
    let cwd = scenario.runtime_request.sandbox.working_directory.clone();
    let write = run_probe_command(
        scenario,
        "mkdir -p reports && printf 'alpha' > reports/probe.txt",
        &cwd,
    );
    expect_probe_success("write virtual file", &write)?;

    let append = run_probe_command(scenario, "printf '\\nbeta' >> reports/probe.txt", &cwd);
    expect_probe_success("append virtual file", &append)?;

    let persisted = run_probe_command(scenario, "cat reports/probe.txt", &cwd);
    expect_probe_success("read persisted virtual file", &persisted)?;
    if persisted.stdout != "alpha\nbeta" {
        return Err(DurableRunAgentError::new(format!(
            "Just Bash virtual FS persistence mismatch: {:?}",
            persisted.stdout
        )));
    }

    let mut_cwd_env = run_probe_command(
        scenario,
        "mkdir -p nested && export TEMP_VALUE=present; cd nested; pwd; echo $TEMP_VALUE",
        &cwd,
    );
    expect_probe_success("mutate cwd and env", &mut_cwd_env)?;
    if mut_cwd_env.stdout != "/workspace/nested\npresent\n" {
        return Err(DurableRunAgentError::new(format!(
            "Just Bash cwd/env mutation probe mismatch: {:?}",
            mut_cwd_env.stdout
        )));
    }

    let reset = run_probe_command(scenario, "pwd; echo $TEMP_VALUE", &cwd);
    expect_probe_success("verify cwd/env reset", &reset)?;
    if reset.stdout != "/workspace\n\n" {
        return Err(DurableRunAgentError::new(format!(
            "Just Bash cwd/env reset mismatch: {:?}",
            reset.stdout
        )));
    }

    let missing = run_probe_command(scenario, "cat reports/missing.txt", &cwd);
    if missing.exit_code != 1 || !missing.stderr.contains("No such file or directory") {
        return Err(DurableRunAgentError::new(format!(
            "Just Bash missing-file failure mismatch: exit={} stderr={:?}",
            missing.exit_code, missing.stderr
        )));
    }

    let unsupported = run_probe_command(scenario, "python -c 'print(1)'", &cwd);
    if unsupported.exit_code != 127 || !unsupported.stderr.contains("command not found") {
        return Err(DurableRunAgentError::new(format!(
            "Just Bash unsupported-command failure mismatch: exit={} stderr={:?}",
            unsupported.exit_code, unsupported.stderr
        )));
    }

    let host_shell = run_probe_command(scenario, "/bin/bash -lc 'printf host-fallback'", &cwd);
    if host_shell.exit_code != 127
        || host_shell.stdout.contains("host-fallback")
        || !host_shell.stderr.contains("No such file or directory")
    {
        return Err(DurableRunAgentError::new(format!(
            "Just Bash host-shell fallback probe mismatch: exit={} stdout={:?} stderr={:?}",
            host_shell.exit_code, host_shell.stdout, host_shell.stderr
        )));
    }

    let corpus = [
        ("echo", "echo corpus-ok", "corpus-ok\n"),
        ("pwd", "pwd", "/workspace\n"),
        (
            "redirection",
            "printf 'shared' > corpus.txt; cat corpus.txt",
            "shared",
        ),
        (
            "mkdir-touch-ls",
            "mkdir -p corpus && touch corpus/item.txt && ls corpus",
            "item.txt\n",
        ),
    ];
    let mut corpus_cases = Vec::new();
    for (name, command, expected_stdout) in corpus {
        let result = run_probe_command(scenario, command, &cwd);
        expect_probe_success(name, &result)?;
        if result.stdout != expected_stdout {
            return Err(DurableRunAgentError::new(format!(
                "Just Bash corpus case {name} stdout mismatch: {:?}",
                result.stdout
            )));
        }
        corpus_cases.push(json!({
            "name": name,
            "command": command,
            "stdout": result.stdout,
        }));
    }

    let agent_example_corpus = [
        (
            "agent-search-count",
            "mkdir -p src && printf 'TODO: fix\\nready\\n' > src/app.ts && grep -r TODO src | wc -l",
            "1\n",
        ),
        (
            "agent-csv-filter",
            "printf 'name,role\\nAlice,admin\\nBob,user\\n' > users.csv && awk -F, 'NR>1 {print $2}' users.csv",
            "admin\nuser\n",
        ),
        (
            "agent-stateful-report",
            "mkdir -p reports && printf 'one' > reports/agent.txt && printf '\\ntwo' >> reports/agent.txt && cat reports/agent.txt",
            "one\ntwo",
        ),
    ];
    let mut agent_example_cases = Vec::new();
    for (name, command, expected_stdout) in agent_example_corpus {
        let result = run_probe_command(scenario, command, &cwd);
        expect_probe_success(name, &result)?;
        if result.stdout != expected_stdout {
            return Err(DurableRunAgentError::new(format!(
                "Just Bash agent-example case {name} stdout mismatch: {:?}",
                result.stdout
            )));
        }
        agent_example_cases.push(json!({
            "name": name,
            "command": command,
            "stdout": result.stdout,
        }));
    }

    let false_result = run_probe_command(scenario, "false", &cwd);
    if false_result.exit_code != 1 || !false_result.stdout.is_empty() {
        return Err(DurableRunAgentError::new(format!(
            "Just Bash false corpus case mismatch: exit={} stdout={:?}",
            false_result.exit_code, false_result.stdout
        )));
    }
    corpus_cases.push(json!({
        "name": "false",
        "command": "false",
        "exitCode": false_result.exit_code,
    }));

    Ok(json!({
        "runId": run_id,
        "workingDirectory": cwd,
        "persistedContent": persisted.stdout,
        "cwdEnvMutationStdout": mut_cwd_env.stdout,
        "cwdEnvResetStdout": reset.stdout,
        "missingFileExitCode": missing.exit_code,
        "unsupportedCommandExitCode": unsupported.exit_code,
        "hostShellExitCode": host_shell.exit_code,
        "hostShellStdout": host_shell.stdout,
        "corpusCases": corpus_cases,
        "agentExampleCases": agent_example_cases,
        "adapter": "ServiceExperimentalSandbox",
    }))
}

fn run_probe_command(
    scenario: &ScriptedRunScenario,
    command: impl Into<String>,
    cwd: &str,
) -> AiSandboxCommandResult {
    run_service_sandbox_command(
        scenario.sandbox.connect.clone(),
        AiSandboxCommandOptions::new(command).with_working_directory(cwd.to_string()),
    )
}

fn expect_probe_success(
    label: &str,
    result: &AiSandboxCommandResult,
) -> Result<(), DurableRunAgentError> {
    if result.exit_code == 0 {
        Ok(())
    } else {
        Err(DurableRunAgentError::new(format!(
            "Just Bash probe {label} failed: exit={} stdout={:?} stderr={:?}",
            result.exit_code, result.stdout, result.stderr
        )))
    }
}

fn chunks_from_just_bash_conformance_report(
    run_id: &str,
    report: serde_json::Value,
) -> Vec<UiMessageChunk> {
    vec![
        UiMessageChunk::start_with_message_id(assistant_message_id(
            run_id,
            DurableRunState::Finished,
        )),
        UiMessageChunk::start_step(),
        UiMessageChunk::text_start("text-1"),
        UiMessageChunk::text_delta(
            "text-1",
            "Just Bash conformance probe passed in /workspace: virtual FS persisted, cwd/env reset, failures stayed shell-shaped, and host shell fallback was blocked.",
        ),
        UiMessageChunk::text_end("text-1"),
        UiMessageChunk::tool_input_available(
            JUST_BASH_CONFORMANCE_TOOL_CALL_ID,
            "bash",
            json!({
                "command": "Open Agents Just Bash conformance smoke",
                "cwd": JUST_BASH_DEFAULT_WORKING_DIRECTORY,
            }),
        ),
        UiMessageChunk::tool_output_available(JUST_BASH_CONFORMANCE_TOOL_CALL_ID, report),
        UiMessageChunk::finish_step(),
        UiMessageChunk::finish_with_reason(FinishReason::Stop),
    ]
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
    SlackOutbound(String),
    FinishAction(String),
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
            Self::SlackOutbound(error) => {
                write!(formatter, "service Slack outbound error: {error}")
            }
            Self::FinishAction(error) => write!(formatter, "service finish action error: {error}"),
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

impl From<ServiceHttpRequest> for HttpRequest {
    fn from(request: ServiceHttpRequest) -> Self {
        Self {
            method: request.method,
            path: request.path,
            headers: request.headers,
            body: request.body,
        }
    }
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

impl From<HttpResponse> for ServiceHttpResponse {
    fn from(response: HttpResponse) -> Self {
        Self {
            status: response.status,
            content_type: response.content_type,
            body: response.body,
        }
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

fn run_sandbox_name(run_id: &str) -> String {
    format!("oa-run-{}", stable_id(run_id))
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
        (DurableRunState::Failed, _) => {
            run_failed_message(execution).unwrap_or_else(|| "Run failed".to_string())
        }
        (_, Some(answer)) => format!("Finished after answer: {answer}"),
        _ => "Finished".to_string(),
    };
    chat_sdk_chat::open_agent_message::OpenAgentUiMessage::new(
        message_id.to_string(),
        OpenAgentMessageRole::Assistant,
    )
    .with_part(OpenAgentMessagePart::done_text(text))
}

fn execution_from_record(record: DurableRunRecord) -> DurableRunExecution {
    let chunks = record
        .events
        .iter()
        .filter_map(|event| match &event.payload {
            DurableRunEventPayload::StreamChunk { chunk } => Some(chunk.clone()),
            _ => None,
        })
        .collect();
    DurableRunExecution {
        run_id: record.run_id,
        state: record.state,
        chunks,
        events: record.events,
    }
}

fn waiting_tool_input_call_id(record: &DurableRunRecord) -> Option<String> {
    match &record.pause {
        Some(DurableRunPause::ToolInput { tool_call_id }) => Some(tool_call_id.clone()),
        _ => None,
    }
}

fn waiting_tool_approval(record: &DurableRunRecord) -> Option<(String, String)> {
    match &record.pause {
        Some(DurableRunPause::ToolApproval {
            approval_id,
            tool_call_id,
        }) => Some((approval_id.clone(), tool_call_id.clone())),
        _ => None,
    }
}

fn waiting_approval_from_execution(execution: &DurableRunExecution) -> Option<(String, String)> {
    execution.events.iter().rev().find_map(|event| {
        if let DurableRunEventPayload::WaitingForApproval {
            approval_id,
            tool_call_id,
        } = &event.payload
        {
            Some((approval_id.clone(), tool_call_id.clone()))
        } else {
            None
        }
    })
}

fn run_failed_message(execution: &DurableRunExecution) -> Option<String> {
    execution.events.iter().rev().find_map(|event| {
        if let DurableRunEventPayload::RunFailed { error } = &event.payload {
            Some(error.clone())
        } else {
            None
        }
    })
}

fn slack_messages_from_git_finish_report(report: &GitFinishReport) -> Vec<SlackOutboundMessage> {
    let mut messages = Vec::new();
    let status = slack_git_status_from_finish(report.status);

    if let Some(commit) = &report.commit {
        messages.push(render_commit_summary(&SlackCommitSummary {
            status,
            committed: Some(commit.committed),
            pushed: report.push.as_ref().map(|push| push.pushed),
            commit_message: commit.commit_message.clone(),
            commit_sha: commit.commit_sha.clone(),
            url: None,
            error: report.error.clone(),
        }));
    }

    if let Some(pr) = &report.pull_request {
        messages.push(render_pull_request_summary(&SlackPullRequestSummary {
            status,
            created: Some(pr.created),
            synced_existing: None,
            pr_number: None,
            url: pr.url.clone(),
            error: report.error.clone(),
            skip_reason: if pr.created {
                None
            } else {
                Some("pull request creation did not execute".to_string())
            },
        }));
    }

    if messages.is_empty() {
        let summary = match report.status {
            GitFinishStatus::NoChanges => {
                format!("Finish actions: no git changes on {}", report.branch)
            }
            GitFinishStatus::Skipped => {
                format!(
                    "Finish actions: git changes detected but commit is disabled on {}",
                    report.branch
                )
            }
            GitFinishStatus::Error => format!(
                "Finish actions failed on {}: {}",
                report.branch,
                report.error.as_deref().unwrap_or("unknown error")
            ),
            GitFinishStatus::Committed
            | GitFinishStatus::Pushed
            | GitFinishStatus::PullRequestCreated => {
                format!("Finish actions completed on {}", report.branch)
            }
        };
        messages.push(render_progress_update(&summary));
    }

    messages
}

fn slack_git_status_from_finish(status: GitFinishStatus) -> SlackGitSummaryStatus {
    match status {
        GitFinishStatus::Error => SlackGitSummaryStatus::Error,
        GitFinishStatus::NoChanges | GitFinishStatus::Skipped => SlackGitSummaryStatus::Skipped,
        GitFinishStatus::Committed
        | GitFinishStatus::Pushed
        | GitFinishStatus::PullRequestCreated => SlackGitSummaryStatus::Success,
    }
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
    use chat_sdk_adapter_slack::DEFAULT_API_BASE;
    use chat_sdk_adapter_slack::outbound::{SlackOutboundActionKind, encode_slack_action_id};
    use hmac::{Hmac, Mac};
    use open_agents_slack::SlackThreadAddress;
    use sha2::Sha256;
    use std::env;
    use std::fs;
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
            Self::start_with_config(config).await
        }

        async fn start_with_config(config: OpenAgentsServiceConfig) -> Self {
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

    #[test]
    fn gateway_runtime_uses_native_gateway_provider_settings() {
        use ai_sdk_rust::LanguageModel as _;

        let settings = native_gateway_settings("gateway-key".to_string());
        assert_eq!(settings.api_key.as_deref(), Some("gateway-key"));
        assert_eq!(
            settings.headers.get("http-referer").map(String::as_str),
            Some(OPEN_AGENTS_GATEWAY_APP_URL)
        );
        assert_eq!(
            settings.headers.get("x-title").map(String::as_str),
            Some(OPEN_AGENTS_GATEWAY_APP_NAME)
        );
        assert_eq!(settings.base_url, None);

        let model = GatewayProvider::from_settings(settings).language_model("openai/gpt-4.1");
        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "openai/gpt-4.1");
    }

    #[test]
    fn vercel_sandbox_without_configured_name_gets_stable_run_name() {
        let config = OpenAgentsServiceConfig::from_reader(|name| match name {
            "SLACK_BOT_TOKEN" => Some("xoxb-fixture".to_string()),
            "SLACK_SIGNING_SECRET" => Some("fixture-signing-secret".to_string()),
            "OPEN_AGENTS_SANDBOX" => Some("vercel".to_string()),
            _ => None,
        })
        .unwrap();
        let sandbox = ServiceSandbox::from_config(&config).unwrap();
        let run_sandbox = sandbox.for_run("slack-run-test");
        let expected_name = run_sandbox_name("slack-run-test");

        match &run_sandbox.connect.state {
            SandboxState::Vercel { sandbox_name, .. } => {
                assert_eq!(sandbox_name.as_deref(), Some(expected_name.as_str()));
            }
            state => panic!("expected Vercel sandbox state, got {state:?}"),
        }
        assert_eq!(
            run_sandbox
                .runtime_context()
                .state
                .get("sandboxName")
                .and_then(serde_json::Value::as_str),
            Some(expected_name.as_str())
        );
        assert_eq!(
            run_sandbox.persistence_state().sandbox_name.as_deref(),
            Some(expected_name.as_str())
        );
    }

    #[test]
    fn gateway_async_runner_waits_for_pending_generation_future() {
        let result = run_gateway_future_to_completion(|| async {
            tokio::task::yield_now().await;
            "gateway async complete"
        })
        .unwrap();

        assert_eq!(result, "gateway async complete");
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

    #[tokio::test]
    async fn service_http_request_bridge_traverses_slack_url_verification() {
        let service = OpenAgentsService::from_config(OpenAgentsServiceConfig::fixture()).unwrap();
        service.health().set_ready(true);
        let body = json!({
            "type": "url_verification",
            "challenge": "challenge-token"
        })
        .to_string();
        let timestamp = current_timestamp();
        let signature = sign(&body, &timestamp);

        let response = service
            .handle_http_request(ServiceHttpRequest::new(
                "POST",
                SLACK_EVENTS_PATH,
                vec![
                    ("content-type".to_string(), "application/json".to_string()),
                    ("x-slack-request-timestamp".to_string(), timestamp),
                    ("x-slack-signature".to_string(), signature),
                ],
                body,
            ))
            .await;

        assert_eq!(response.status, 200);
        assert_eq!(response.body, "challenge-token");
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

    fn app_mention_thread_reply_body(
        text: &str,
        event_id: &str,
        ts: &str,
        thread_ts: &str,
    ) -> String {
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
                "ts": ts,
                "thread_ts": thread_ts
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

    fn config_with_slack_api_base(api_addr: SocketAddr) -> OpenAgentsServiceConfig {
        OpenAgentsServiceConfig::from_reader(|name| match name {
            "SLACK_BOT_TOKEN" => Some("xoxb-fixture".to_string()),
            "SLACK_SIGNING_SECRET" => Some("fixture-signing-secret".to_string()),
            "OPEN_AGENTS_BIND_ADDR" => Some("127.0.0.1:0".to_string()),
            "OPEN_AGENTS_SLACK_API_URL" => Some(format!("http://{api_addr}/api")),
            _ => None,
        })
        .unwrap()
    }

    fn config_with_open_plugin_fixture() -> OpenAgentsServiceConfig {
        let plugin_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/open-plugin/minimal")
            .canonicalize()
            .unwrap();
        let data_dir = env::temp_dir().join("open-agents-service-plugin-data-runtime");
        OpenAgentsServiceConfig::from_reader(|name| match name {
            "SLACK_BOT_TOKEN" => Some("xoxb-fixture".to_string()),
            "SLACK_SIGNING_SECRET" => Some("fixture-signing-secret".to_string()),
            "OPEN_AGENTS_BIND_ADDR" => Some("127.0.0.1:0".to_string()),
            crate::plugin::OPEN_AGENTS_PLUGIN_ROOTS_ENV => Some(plugin_root.display().to_string()),
            crate::plugin::OPEN_AGENTS_PLUGIN_DATA_DIR_ENV => Some(data_dir.display().to_string()),
            _ => None,
        })
        .unwrap()
    }

    fn finish_git_config_with_sandbox_root(
        sandbox_root: &std::path::Path,
    ) -> OpenAgentsServiceConfig {
        OpenAgentsServiceConfig::from_reader(|name| match name {
            "SLACK_BOT_TOKEN" => Some("xoxb-fixture".to_string()),
            "SLACK_SIGNING_SECRET" => Some("fixture-signing-secret".to_string()),
            "OPEN_AGENTS_BIND_ADDR" => Some("127.0.0.1:0".to_string()),
            "OPEN_AGENTS_SANDBOX" => Some("local".to_string()),
            "OPEN_AGENTS_SANDBOX_ROOT" => Some(sandbox_root.to_string_lossy().to_string()),
            "OPEN_AGENTS_GIT_FINISH" => Some("report".to_string()),
            _ => None,
        })
        .unwrap()
    }

    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "open-agents-service-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    async fn start_fake_slack_api(
        expected_requests: usize,
    ) -> (SocketAddr, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for index in 0..expected_requests {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = [0_u8; 4096];
                let read = stream.read(&mut buffer).await.unwrap();
                requests.push(String::from_utf8_lossy(&buffer[..read]).to_string());
                let body = json!({
                    "ok": true,
                    "channel": "C123",
                    "ts": format!("1710000000.00020{index}")
                })
                .to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
            requests
        });
        (addr, task)
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
    async fn gateway_service_defaults_slack_outbound_to_real_slack_api() {
        let config = OpenAgentsServiceConfig::from_reader(|name| match name {
            "SLACK_BOT_TOKEN" => Some("xoxb-fixture".to_string()),
            "SLACK_SIGNING_SECRET" => Some("fixture-signing-secret".to_string()),
            "OPEN_AGENTS_RUNTIME" => Some("gateway".to_string()),
            "AI_GATEWAY_API_KEY" => Some("gateway-key".to_string()),
            _ => None,
        })
        .unwrap();
        let service = OpenAgentsService::from_config(config).unwrap();
        let adapter = service
            .slack_outbound
            .as_ref()
            .expect("Slack outbound adapter should be configured");

        assert_eq!(adapter.api_base(), DEFAULT_API_BASE);
    }

    #[tokio::test]
    async fn local_runtime_exposes_open_plugin_components_without_starting_mcp() {
        let service = OpenAgentsService::from_config(config_with_open_plugin_fixture()).unwrap();
        let runtime = service.local_runtime();

        let skills = runtime.plugin_skills();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "hello-plugin:greet");
        assert!(skills[0].path.ends_with("skills/greet"));

        let mcp_servers = runtime.plugin_mcp_servers();
        assert_eq!(mcp_servers.len(), 1);
        assert_eq!(mcp_servers[0].plugin_name, "hello-plugin");
        assert_eq!(mcp_servers[0].server_name, "echo");
        assert_eq!(
            mcp_servers[0].tool_prefix,
            "mcp__plugin_hello-plugin_echo__"
        );
        assert!(mcp_servers[0].has_args);
        assert!(
            mcp_servers[0]
                .command
                .as_deref()
                .is_some_and(|command| command.ends_with("/bin/echo-mcp"))
        );
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
    async fn slack_app_mention_routes_bash_tool_call_through_just_bash_without_vercel_credentials()
    {
        let server = TestServer::start().await;
        let thread_ts = "1710000000.000105";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();

        let response = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body("inspect the repo", "EvJustBashRoute", thread_ts),
            "application/json",
        )
        .await;

        assert_eq!(response.status, 200);
        let runtime = server.service.local_runtime();
        let mapping = runtime
            .mapping_for_slack_thread_id(&thread_id)
            .await
            .unwrap()
            .unwrap();
        let session = runtime
            .persistence
            .get_session(&mapping.session_id)
            .await
            .unwrap()
            .unwrap();
        let sandbox_state = session.sandbox_state.expect("sandbox state");
        assert_eq!(sandbox_state.provider, "just-bash");
        assert_eq!(
            sandbox_state.working_directory.as_deref(),
            Some(JUST_BASH_DEFAULT_WORKING_DIRECTORY)
        );

        let run = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Finished);
        let messages = runtime
            .persistence
            .list_chat_messages(&mapping.chat_id)
            .await
            .unwrap();
        let assistant_json = serde_json::to_string(&messages[1].parts).unwrap();
        assert!(assistant_json.contains("tool-bash"));
        assert!(assistant_json.contains("/workspace"));
        assert!(!assistant_json.contains("/bin/bash"));
        server.stop().await;
    }

    #[tokio::test]
    async fn slack_app_mention_runs_just_bash_conformance_probe_through_service_adapter() {
        let server = TestServer::start().await;
        let thread_ts = "1710000000.000106";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();

        let response = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body(
                "run just bash conformance proof",
                "EvJustBashConformance",
                thread_ts,
            ),
            "application/json",
        )
        .await;

        assert_eq!(response.status, 200);
        let runtime = server.service.local_runtime();
        let mapping = runtime
            .mapping_for_slack_thread_id(&thread_id)
            .await
            .unwrap()
            .unwrap();
        let session = runtime
            .persistence
            .get_session(&mapping.session_id)
            .await
            .unwrap()
            .unwrap();
        let sandbox_state = session.sandbox_state.expect("sandbox state");
        assert_eq!(sandbox_state.provider, "just-bash");

        let run = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Finished);
        let messages = runtime
            .persistence
            .list_chat_messages(&mapping.chat_id)
            .await
            .unwrap();
        let assistant_json = serde_json::to_string(&messages[1].parts).unwrap();
        assert!(assistant_json.contains("Just Bash conformance probe passed"));
        assert!(assistant_json.contains(JUST_BASH_CONFORMANCE_TOOL_CALL_ID));
        assert!(assistant_json.contains("ServiceExperimentalSandbox"));
        assert!(assistant_json.contains("persistedContent"));
        assert!(assistant_json.contains("missingFileExitCode"));
        assert!(assistant_json.contains("hostShellExitCode"));
        assert!(!assistant_json.contains("host-fallback"));

        let state: SandboxState = serde_json::from_value(sandbox_state.raw).unwrap();
        let sandbox = connect_sandbox(SandboxConnectConfig::new(state)).unwrap();
        assert_eq!(
            sandbox.read_file("reports/probe.txt").unwrap(),
            "alpha\nbeta"
        );
        let reset = sandbox
            .exec(SandboxExecOptions::new("pwd; echo $TEMP_VALUE"))
            .unwrap();
        assert_eq!(reset.stdout, "/workspace\n\n");

        server.stop().await;
    }

    #[tokio::test]
    async fn slack_app_mention_runs_agent_example_just_bash_workflow_through_service_adapter() {
        let server = TestServer::start().await;
        let thread_ts = "1710000000.000107";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();

        let response = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body(
                "run just bash conformance for agent examples",
                "EvJustBashAgentExamples",
                thread_ts,
            ),
            "application/json",
        )
        .await;

        assert_eq!(response.status, 200);
        let runtime = server.service.local_runtime();
        let mapping = runtime
            .mapping_for_slack_thread_id(&thread_id)
            .await
            .unwrap()
            .unwrap();
        let session = runtime
            .persistence
            .get_session(&mapping.session_id)
            .await
            .unwrap()
            .unwrap();
        let sandbox_state = session.sandbox_state.expect("sandbox state");
        assert_eq!(sandbox_state.provider, "just-bash");

        let run = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Finished);
        let messages = runtime
            .persistence
            .list_chat_messages(&mapping.chat_id)
            .await
            .unwrap();
        let assistant_json = serde_json::to_string(&messages[1].parts).unwrap();
        assert!(assistant_json.contains("agentExampleCases"));
        assert!(assistant_json.contains("agent-search-count"));
        assert!(assistant_json.contains("agent-csv-filter"));
        assert!(assistant_json.contains("agent-stateful-report"));
        assert!(assistant_json.contains("hostShellExitCode"));
        assert!(!assistant_json.contains("host-fallback"));

        let state: SandboxState = serde_json::from_value(sandbox_state.raw).unwrap();
        let sandbox = connect_sandbox(SandboxConnectConfig::new(state)).unwrap();
        assert_eq!(sandbox.read_file("reports/agent.txt").unwrap(), "one\ntwo");

        server.stop().await;
    }

    #[tokio::test]
    async fn chat_post_route_persists_messages_activity_stream_chunks_and_model_metadata() {
        let server = TestServer::start().await;
        let thread_ts = "1710000000.000110";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();

        let response = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body("inspect the repo", "EvChatPostParity", thread_ts),
            "application/json",
        )
        .await;

        assert_eq!(response.status, 200);
        let runtime = server.service.local_runtime();
        let expected_model_id = runtime.model_id.as_str();
        let mapping = runtime
            .mapping_for_slack_thread_id(&thread_id)
            .await
            .unwrap()
            .unwrap();
        let chat = runtime
            .persistence
            .get_chat(&mapping.chat_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(chat.model_id.as_deref(), Some(expected_model_id));
        assert_eq!(chat.active_run_id, None);
        assert!(chat.last_assistant_message_at.is_some());

        let run = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Finished);
        assert_eq!(run.model_id.as_deref(), Some(expected_model_id));

        let messages = runtime
            .persistence
            .list_chat_messages(&mapping.chat_id)
            .await
            .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, open_agents_persistence::MessageRole::User);
        assert_eq!(
            messages[0].parts.pointer("/metadata/slackEventId"),
            Some(&json!("EvChatPostParity"))
        );
        assert_eq!(
            messages[1].role,
            open_agents_persistence::MessageRole::Assistant
        );
        assert_eq!(
            messages[1].parts.pointer("/metadata/runId"),
            Some(&json!(run.id.clone()))
        );
        let assistant_json = serde_json::to_string(&messages[1].parts).unwrap();
        assert!(assistant_json.contains("Fixture agent finished"));
        assert!(assistant_json.contains("tool-bash"));
        assert!(assistant_json.contains("output"));

        let steps = runtime.persistence.list_run_steps(&run.id).await.unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].finish_reason.as_deref(), Some("finished"));

        let all_chunks = runtime.stream_chunks_since(&run.id, 0).unwrap();
        let tail_chunks = runtime.stream_chunks_since(&run.id, 1).unwrap();
        assert!(!all_chunks.is_empty());
        assert_eq!(tail_chunks.len(), all_chunks.len() - 1);
        server.stop().await;
    }

    #[tokio::test]
    async fn chat_route_thread_reply_resumes_waiting_run_and_starts_new_after_stale_terminal_run() {
        let server = TestServer::start().await;
        let thread_ts = "1710000000.000120";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();

        let start = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body(
                "ask a question before continuing",
                "EvReconnectWaitingStart",
                thread_ts,
            ),
            "application/json",
        )
        .await;
        assert_eq!(start.status, 200);
        let runtime = server.service.local_runtime();
        let waiting = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(waiting.status, RunStatus::Paused);

        let resume = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_thread_reply_body(
                "ship it from another surface",
                "EvReconnectWaitingAgain",
                "1710000000.000121",
                thread_ts,
            ),
            "application/json",
        )
        .await;
        assert_eq!(resume.status, 200);
        let finished = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(finished.id, waiting.id);
        assert_eq!(finished.status, RunStatus::Finished);
        assert_eq!(
            runtime.active_run_id_for_thread(&thread_id).await.unwrap(),
            None
        );

        let new_start = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_thread_reply_body(
                "start a fresh run after terminal state",
                "EvReconnectFreshAfterTerminal",
                "1710000000.000122",
                thread_ts,
            ),
            "application/json",
        )
        .await;
        assert_eq!(new_start.status, 200);
        let fresh = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(fresh.status, RunStatus::Finished);
        assert_ne!(fresh.id, finished.id);
        server.stop().await;
    }

    #[tokio::test]
    async fn chat_stop_route_cancel_persists_abort_snapshot_and_clears_activity() {
        let server = TestServer::start().await;
        let thread_ts = "1710000000.000130";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();

        let start = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body(
                "ask a question before continuing",
                "EvStopSnapshotStart",
                thread_ts,
            ),
            "application/json",
        )
        .await;
        assert_eq!(start.status, 200);
        let runtime = server.service.local_runtime();
        let waiting = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(waiting.status, RunStatus::Paused);

        let cancel = post(
            server.addr,
            SLACK_INTERACTIONS_PATH,
            &action_body(SLACK_ACTION_CANCEL, "cancel", thread_ts),
            "application/x-www-form-urlencoded",
        )
        .await;

        assert_eq!(cancel.status, 200);
        let mapping = runtime
            .mapping_for_slack_thread_id(&thread_id)
            .await
            .unwrap()
            .unwrap();
        let run = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Canceled);
        assert_eq!(
            runtime.active_run_id_for_thread(&thread_id).await.unwrap(),
            None
        );
        let messages = runtime
            .persistence
            .list_chat_messages(&mapping.chat_id)
            .await
            .unwrap();
        assert!(
            messages
                .iter()
                .any(|message| message.parts.pointer("/metadata/durableState")
                    == Some(&json!("waiting_for_input")))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.parts.pointer("/metadata/durableState")
                    == Some(&json!("canceled")))
        );
        let chat = runtime
            .persistence
            .get_chat(&mapping.chat_id)
            .await
            .unwrap()
            .unwrap();
        assert!(chat.last_assistant_message_at.is_some());
        server.stop().await;
    }

    #[tokio::test]
    async fn app_mention_with_slack_api_url_posts_outbounds_to_slack_api() {
        let (api_addr, api_task) = start_fake_slack_api(3).await;
        let server = TestServer::start_with_config(config_with_slack_api_base(api_addr)).await;
        let thread_ts = "1710000000.000150";

        let response = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body("inspect the repo", "EvE2EApiBase", thread_ts),
            "application/json",
        )
        .await;

        assert_eq!(response.status, 200);
        let requests = api_task.await.unwrap();
        assert_eq!(requests.len(), 3);
        assert!(
            requests
                .iter()
                .all(|request| request.contains("/api/chat.postMessage"))
        );
        assert!(
            requests
                .iter()
                .any(|request| request.contains("Fixture agent finished"))
        );
        server.stop().await;
    }

    #[tokio::test]
    #[ignore = "requires AI_GATEWAY_API_KEY or AI_SDK_RUST_AI_GATEWAY_API_KEY and makes a live Gateway model call"]
    async fn live_gateway_runtime_handles_app_mention_without_fixture_text() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!("skipping live Gateway runtime test because no API key is configured");
            return;
        };
        let model = env::var("OPEN_AGENTS_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let config = OpenAgentsServiceConfig::from_reader(|name| match name {
            "SLACK_BOT_TOKEN" => Some("xoxb-fixture".to_string()),
            "SLACK_SIGNING_SECRET" => Some("fixture-signing-secret".to_string()),
            "OPEN_AGENTS_RUNTIME" => Some("gateway".to_string()),
            "AI_GATEWAY_API_KEY" => Some(api_key.clone()),
            "OPEN_AGENTS_MODEL" => Some(model.clone()),
            "OPEN_AGENTS_MODEL_MAX_STEPS" => Some("1".to_string()),
            "OPEN_AGENTS_MODEL_MAX_OUTPUT_TOKENS" => Some("64".to_string()),
            _ => None,
        })
        .unwrap();
        let service = OpenAgentsService::from_config(config).unwrap();
        let thread_ts = "1710000000.000777";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();
        let body = app_mention_body(
            "Reply in exactly two words: gateway ok. Do not use tools.",
            "EvGatewayLive",
            thread_ts,
        );
        let timestamp = current_timestamp();
        let signature = sign(&body, &timestamp);

        let response = service
            .handle_http_request(ServiceHttpRequest::new(
                "POST",
                SLACK_EVENTS_PATH,
                vec![
                    ("content-type".to_string(), "application/json".to_string()),
                    ("x-slack-request-timestamp".to_string(), timestamp),
                    ("x-slack-signature".to_string(), signature),
                ],
                body,
            ))
            .await;

        assert_eq!(response.status, 200);
        let outbounds = service.local_runtime().outbound_for_thread(&thread_id);
        assert!(
            outbounds.iter().any(|message| {
                message.kind == LocalOutboundKind::Final
                    && !message.text.contains("Fixture agent finished")
            }),
            "expected a non-fixture final outbound, got {outbounds:#?}"
        );
    }

    fn live_gateway_api_key() -> Option<String> {
        env::var("AI_GATEWAY_API_KEY")
            .or_else(|_| env::var("AI_SDK_RUST_AI_GATEWAY_API_KEY"))
            .ok()
            .filter(|value| !value.trim().is_empty())
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
    async fn threaded_message_resumes_waiting_run_without_starting_duplicate() {
        let server = TestServer::start().await;
        let thread_ts = "1710000000.000250";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();

        let start = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body(
                "ask a question before continuing",
                "EvE2EResumeStart",
                thread_ts,
            ),
            "application/json",
        )
        .await;
        assert_eq!(start.status, 200);
        let runtime = server.service.local_runtime();
        let waiting = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(waiting.status, RunStatus::Paused);

        let resume = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_thread_reply_body(
                "ship it from the thread",
                "EvE2EResumeReply",
                "1710000000.000251",
                thread_ts,
            ),
            "application/json",
        )
        .await;

        assert_eq!(resume.status, 200);
        let completed = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(completed.id, waiting.id);
        assert_eq!(completed.status, RunStatus::Finished);
        assert_eq!(
            runtime.active_run_id_for_thread(&thread_id).await.unwrap(),
            None
        );
        server.stop().await;
    }

    #[tokio::test]
    async fn block_action_approval_resumes_waiting_run_to_completion() {
        let server = TestServer::start().await;
        let thread_ts = "1710000000.000260";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();

        let start = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body(
                "ask for approval before running pwd",
                "EvE2EApproval",
                thread_ts,
            ),
            "application/json",
        )
        .await;
        assert_eq!(start.status, 200);
        let runtime = server.service.local_runtime();
        let waiting = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(waiting.status, RunStatus::Paused);
        assert!(
            runtime
                .outbound_for_thread(&thread_id)
                .iter()
                .any(|message| {
                    message.kind == LocalOutboundKind::Question
                        && message.text.contains("Approval required")
                })
        );

        let context = SlackRunContext::new(waiting.id.clone(), format!("{}-approval", waiting.id));
        let action_id = encode_slack_action_id(
            SlackOutboundActionKind::Approve,
            &context,
            SANDBOX_APPROVAL_ID,
        );
        let approve = post(
            server.addr,
            SLACK_INTERACTIONS_PATH,
            &action_body(&action_id, SANDBOX_APPROVAL_ID, thread_ts),
            "application/x-www-form-urlencoded",
        )
        .await;

        assert_eq!(approve.status, 200);
        let completed = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(completed.status, RunStatus::Finished);
        let durable = runtime.durable_run(&completed.id).unwrap().unwrap();
        assert_eq!(durable.state, DurableRunState::Finished);
        assert!(
            runtime
                .outbound_for_thread(&thread_id)
                .iter()
                .any(|message| message.text.contains("after approval"))
        );
        server.stop().await;
    }

    #[tokio::test]
    async fn scripted_runtime_failure_is_persisted_and_reported_to_slack() {
        let server = TestServer::start().await;
        let thread_ts = "1710000000.000270";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();

        let response = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body(
                "please fail model for coverage",
                "EvE2EModelError",
                thread_ts,
            ),
            "application/json",
        )
        .await;

        assert_eq!(response.status, 200);
        let runtime = server.service.local_runtime();
        let run = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(
            runtime.active_run_id_for_thread(&thread_id).await.unwrap(),
            None
        );
        assert!(
            runtime
                .outbound_for_thread(&thread_id)
                .iter()
                .any(|message| {
                    message.kind == LocalOutboundKind::Failed && message.text.contains("Run failed")
                })
        );
        server.stop().await;
    }

    #[tokio::test]
    async fn finish_action_errors_are_reported_without_failing_finished_run() {
        let sandbox_root = unique_temp_dir("finish-action-error");
        let server =
            TestServer::start_with_config(finish_git_config_with_sandbox_root(&sandbox_root)).await;
        let thread_ts = "1710000000.000280";
        let thread_id = SlackThreadAddress::new("C123", thread_ts).chat_thread_id();

        let response = post(
            server.addr,
            SLACK_EVENTS_PATH,
            &app_mention_body("inspect the repo", "EvE2EFinishError", thread_ts),
            "application/json",
        )
        .await;

        assert_eq!(response.status, 200);
        let runtime = server.service.local_runtime();
        let run = runtime.run_for_thread(&thread_id).await.unwrap().unwrap();
        assert_eq!(run.status, RunStatus::Finished);
        assert!(
            runtime
                .outbound_for_thread(&thread_id)
                .iter()
                .any(|message| {
                    message.kind == LocalOutboundKind::Final
                        && message.text.contains("Finish actions failed")
                })
        );
        server.stop().await;
        fs::remove_dir_all(sandbox_root).unwrap();
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
