use async_trait::async_trait;
use open_agents_core::RemoteAgentIdentity;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Result type returned by Open Agents persistence repositories.
pub type PersistenceResult<T> = Result<T, PersistenceError>;

/// Errors that can be returned by repository implementations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistenceError {
    /// A record already exists for a key that must be unique.
    Conflict(String),
    /// A record references another record that does not exist.
    InvalidReference { entity: &'static str, id: String },
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict(message) => f.write_str(message),
            Self::InvalidReference { entity, id } => {
                write!(f, "{entity} reference does not exist: {id}")
            }
        }
    }
}

impl std::error::Error for PersistenceError {}

/// User-visible session status, matching Open Agents `sessions.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Running,
    Completed,
    Failed,
    Archived,
}

/// Durable lifecycle state for the remote-agent session and sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Provisioning,
    Running,
    Paused,
    Canceled,
    Failed,
    Finished,
}

/// Durable workflow run state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pending,
    Running,
    Paused,
    Canceled,
    Failed,
    Finished,
}

impl RunStatus {
    /// Whether the run state is terminal.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Canceled | Self::Failed | Self::Finished)
    }
}

/// Persisted chat message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

/// Source surface that produced a usage row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    Slack,
    Web,
}

/// Type of agent that produced usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentType {
    Main,
    Subagent,
}

/// Provider-owned sandbox state stored as structured fields plus raw JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SandboxState {
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_details: Option<String>,
    #[serde(default)]
    pub raw: serde_json::Value,
}

/// Durable Open Agents session record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub status: SessionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clone_url: Option<String>,
    pub is_new_branch: bool,
    pub lifecycle_state: LifecycleState,
    pub lifecycle_version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_provisioning_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_state: Option<SandboxState>,
    #[serde(default)]
    pub metadata: serde_json::Map<String, serde_json::Value>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// Input for creating a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateSessionInput {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub status: SessionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clone_url: Option<String>,
    pub is_new_branch: bool,
    pub lifecycle_state: LifecycleState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_state: Option<SandboxState>,
    #[serde(default)]
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

impl CreateSessionInput {
    /// Construct a default running session in provisioning lifecycle.
    pub fn new(
        id: impl Into<String>,
        user_id: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            user_id: user_id.into(),
            title: title.into(),
            status: SessionStatus::Running,
            repo_owner: None,
            repo_name: None,
            branch: None,
            clone_url: None,
            is_new_branch: false,
            lifecycle_state: LifecycleState::Provisioning,
            sandbox_state: None,
            metadata: serde_json::Map::new(),
        }
    }
}

/// Session lifecycle update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLifecycleUpdate {
    pub lifecycle_state: LifecycleState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<SessionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_error: Option<String>,
}

/// Sandbox state update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SandboxStateUpdate {
    pub lifecycle_state: LifecycleState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_state: Option<SandboxState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_error: Option<String>,
}

/// Durable chat record under a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRecord {
    pub id: String,
    pub session_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_assistant_message_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// Input for creating a chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateChatInput {
    pub id: String,
    pub session_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

impl CreateChatInput {
    /// Construct a chat with no model override.
    pub fn new(
        id: impl Into<String>,
        session_id: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            session_id: session_id.into(),
            title: title.into(),
            model_id: None,
        }
    }
}

/// Persisted chat message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessageRecord {
    pub id: String,
    pub chat_id: String,
    pub role: MessageRole,
    pub parts: serde_json::Value,
    pub sequence: u64,
    pub created_at: OffsetDateTime,
}

/// Input for creating a chat message if absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateChatMessageInput {
    pub id: String,
    pub chat_id: String,
    pub role: MessageRole,
    pub parts: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<OffsetDateTime>,
}

/// Result of an idempotent insert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsertIfAbsent<T> {
    pub record: T,
    pub inserted: bool,
}

/// Durable run record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub session_id: String,
    pub chat_id: String,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub status: RunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    pub started_at: OffsetDateTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl RunRecord {
    /// Returns the shared Open Agents identity for this durable run.
    pub fn identity(&self) -> RemoteAgentIdentity {
        RemoteAgentIdentity::new(self.session_id.clone(), self.chat_id.clone())
            .with_run_id(self.id.clone())
    }
}

/// Input for creating a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRunInput {
    pub id: String,
    pub session_id: String,
    pub chat_id: String,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub status: RunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<OffsetDateTime>,
}

/// Run status update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStatusUpdate {
    pub status: RunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Durable run step record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunStepRecord {
    pub id: String,
    pub run_id: String,
    pub step_number: u32,
    pub started_at: OffsetDateTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,
    pub created_at: OffsetDateTime,
}

/// Input for creating a run step if absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRunStepInput {
    pub id: String,
    pub run_id: String,
    pub step_number: u32,
    pub started_at: OffsetDateTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,
}

/// Durable usage row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageRecord {
    pub id: String,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub source: UsageSource,
    pub agent_type: AgentType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub tool_call_count: u64,
    pub created_at: OffsetDateTime,
}

/// Input for creating a usage row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateUsageRecordInput {
    pub id: String,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub source: UsageSource,
    pub agent_type: AgentType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub tool_call_count: u64,
}

/// Unique Slack thread identity for durable ownership lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SlackThreadKey {
    pub team_id: String,
    pub channel_id: String,
    pub thread_ts: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enterprise_id: Option<String>,
}

impl SlackThreadKey {
    /// Construct a Slack thread key without an enterprise identifier.
    pub fn new(
        team_id: impl Into<String>,
        channel_id: impl Into<String>,
        thread_ts: impl Into<String>,
    ) -> Self {
        Self {
            team_id: team_id.into(),
            channel_id: channel_id.into(),
            thread_ts: thread_ts.into(),
            enterprise_id: None,
        }
    }
}

/// Durable mapping from Slack thread to Open Agents session/chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackThreadMappingRecord {
    pub key: SlackThreadKey,
    pub session_id: String,
    pub chat_id: String,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_message_ts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_ts: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl SlackThreadMappingRecord {
    /// Returns the shared Open Agents identity resolved from this Slack thread.
    pub fn identity(&self) -> RemoteAgentIdentity {
        RemoteAgentIdentity::new(self.session_id.clone(), self.chat_id.clone())
    }
}

/// Input for creating a Slack thread mapping if absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSlackThreadMappingInput {
    pub key: SlackThreadKey,
    pub session_id: String,
    pub chat_id: String,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_message_ts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_ts: Option<String>,
}

/// Durable idempotency key for Slack retries and API retries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdempotencyRecord {
    pub scope: String,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<serde_json::Value>,
    pub created_at: OffsetDateTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<OffsetDateTime>,
}

/// Input for creating an idempotency record if absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateIdempotencyRecordInput {
    pub scope: String,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<OffsetDateTime>,
}

/// Session repository contract.
#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn create_session_with_initial_chat(
        &self,
        session: CreateSessionInput,
        chat: CreateChatInput,
    ) -> PersistenceResult<(SessionRecord, ChatRecord)>;

    async fn get_session(&self, session_id: &str) -> PersistenceResult<Option<SessionRecord>>;

    async fn update_session_lifecycle(
        &self,
        session_id: &str,
        update: SessionLifecycleUpdate,
    ) -> PersistenceResult<Option<SessionRecord>>;
}

/// Chat repository contract.
#[async_trait]
pub trait ChatRepository: Send + Sync {
    async fn create_chat(&self, chat: CreateChatInput) -> PersistenceResult<ChatRecord>;

    async fn get_chat(&self, chat_id: &str) -> PersistenceResult<Option<ChatRecord>>;

    async fn list_chats_by_session(&self, session_id: &str) -> PersistenceResult<Vec<ChatRecord>>;

    async fn touch_chat(
        &self,
        chat_id: &str,
        assistant_activity_at: Option<OffsetDateTime>,
    ) -> PersistenceResult<Option<ChatRecord>>;
}

/// Chat message repository contract.
#[async_trait]
pub trait MessageRepository: Send + Sync {
    async fn create_message_if_absent(
        &self,
        message: CreateChatMessageInput,
    ) -> PersistenceResult<InsertIfAbsent<ChatMessageRecord>>;

    async fn list_chat_messages(&self, chat_id: &str) -> PersistenceResult<Vec<ChatMessageRecord>>;
}

/// Active run ownership contract for compare-and-set semantics.
#[async_trait]
pub trait ActiveRunRepository: Send + Sync {
    async fn claim_chat_active_run(&self, chat_id: &str, run_id: &str) -> PersistenceResult<bool>;

    async fn compare_and_set_chat_active_run(
        &self,
        chat_id: &str,
        expected_run_id: Option<String>,
        next_run_id: Option<String>,
    ) -> PersistenceResult<bool>;
}

/// Durable run repository contract.
#[async_trait]
pub trait RunRepository: Send + Sync {
    async fn create_run(&self, run: CreateRunInput) -> PersistenceResult<RunRecord>;

    async fn get_run(&self, run_id: &str) -> PersistenceResult<Option<RunRecord>>;

    async fn update_run_status(
        &self,
        run_id: &str,
        update: RunStatusUpdate,
    ) -> PersistenceResult<Option<RunRecord>>;

    async fn create_run_step_if_absent(
        &self,
        step: CreateRunStepInput,
    ) -> PersistenceResult<InsertIfAbsent<RunStepRecord>>;

    async fn list_run_steps(&self, run_id: &str) -> PersistenceResult<Vec<RunStepRecord>>;
}

/// Usage repository contract.
#[async_trait]
pub trait UsageRepository: Send + Sync {
    async fn create_usage_record(
        &self,
        usage: CreateUsageRecordInput,
    ) -> PersistenceResult<UsageRecord>;

    async fn list_usage_by_run(&self, run_id: &str) -> PersistenceResult<Vec<UsageRecord>>;
}

/// Slack ownership mapping repository contract.
#[async_trait]
pub trait SlackThreadMappingRepository: Send + Sync {
    async fn create_slack_thread_mapping_if_absent(
        &self,
        mapping: CreateSlackThreadMappingInput,
    ) -> PersistenceResult<InsertIfAbsent<SlackThreadMappingRecord>>;

    async fn get_slack_thread_mapping(
        &self,
        key: &SlackThreadKey,
    ) -> PersistenceResult<Option<SlackThreadMappingRecord>>;

    async fn get_slack_thread_mapping_by_chat(
        &self,
        chat_id: &str,
    ) -> PersistenceResult<Option<SlackThreadMappingRecord>>;
}

/// Sandbox lifecycle repository contract.
#[async_trait]
pub trait SandboxStateRepository: Send + Sync {
    async fn update_sandbox_state(
        &self,
        session_id: &str,
        update: SandboxStateUpdate,
    ) -> PersistenceResult<Option<SessionRecord>>;

    async fn claim_session_lifecycle_run(
        &self,
        session_id: &str,
        run_id: &str,
    ) -> PersistenceResult<bool>;

    async fn clear_session_lifecycle_run_if_owned(
        &self,
        session_id: &str,
        run_id: &str,
    ) -> PersistenceResult<bool>;

    async fn claim_sandbox_provisioning_run(
        &self,
        session_id: &str,
        run_id: &str,
    ) -> PersistenceResult<bool>;

    async fn clear_sandbox_provisioning_run_if_owned(
        &self,
        session_id: &str,
        run_id: &str,
    ) -> PersistenceResult<bool>;
}

/// Retry idempotency repository contract.
#[async_trait]
pub trait IdempotencyRepository: Send + Sync {
    async fn create_idempotency_record_if_absent(
        &self,
        record: CreateIdempotencyRecordInput,
    ) -> PersistenceResult<InsertIfAbsent<IdempotencyRecord>>;

    async fn get_idempotency_record(
        &self,
        scope: &str,
        key: &str,
    ) -> PersistenceResult<Option<IdempotencyRecord>>;
}

/// Full store contract used by the remote-agent service.
pub trait RemoteAgentPersistenceStore:
    SessionRepository
    + ChatRepository
    + MessageRepository
    + ActiveRunRepository
    + RunRepository
    + UsageRepository
    + SlackThreadMappingRepository
    + SandboxStateRepository
    + IdempotencyRepository
{
}

impl<T> RemoteAgentPersistenceStore for T where
    T: SessionRepository
        + ChatRepository
        + MessageRepository
        + ActiveRunRepository
        + RunRepository
        + UsageRepository
        + SlackThreadMappingRepository
        + SandboxStateRepository
        + IdempotencyRepository
{
}
