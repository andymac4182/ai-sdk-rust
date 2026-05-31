use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::fmt;

pub const SPEC_VERSION_LEGACY: u32 = 1;
pub const SPEC_VERSION_SUPPORTS_EVENT_SOURCING: u32 = 2;
pub const SPEC_VERSION_SUPPORTS_CBOR_QUEUE_TRANSPORT: u32 = 3;
pub const SPEC_VERSION_CURRENT: u32 = 3;

pub const MAX_QUEUE_DELIVERIES: u32 = 48;
pub const REPLAY_TIMEOUT_MS: u64 = 240_000;
pub const MIN_REPLAY_TIMEOUT_MS: u64 = 30_000;
pub const MAX_REPLAY_TIMEOUT_MS: u64 = 780_000;
pub const REPLAY_TIMEOUT_MAX_RETRIES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RunErrorCode {
    CorruptedEventLog,
    MaxDeliveriesExceeded,
    ReplayTimeout,
    WorkflowRuntimeError,
    WorldContractError,
}

impl RunErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CorruptedEventLog => "CORRUPTED_EVENT_LOG",
            Self::MaxDeliveriesExceeded => "MAX_DELIVERIES_EXCEEDED",
            Self::ReplayTimeout => "REPLAY_TIMEOUT",
            Self::WorkflowRuntimeError => "WORKFLOW_RUNTIME_ERROR",
            Self::WorldContractError => "WORLD_CONTRACT_ERROR",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkflowErrorKind {
    Conflict,
    Fatal,
    NotFound,
    Queue,
    Retryable,
    RunExpired,
    Runtime,
    Throttle,
    TooEarly,
    World,
    WorldContract,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowCoreError {
    pub kind: WorkflowErrorKind,
    pub message: String,
    pub status: Option<u16>,
    pub retry_after_seconds: Option<u64>,
    pub code: Option<RunErrorCode>,
    pub cause: Option<String>,
}

impl WorkflowCoreError {
    pub fn new(kind: WorkflowErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status: None,
            retry_after_seconds: None,
            code: None,
            cause: None,
        }
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self::new(WorkflowErrorKind::Runtime, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(WorkflowErrorKind::Conflict, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(WorkflowErrorKind::NotFound, message)
    }

    pub fn fatal(message: impl Into<String>) -> Self {
        Self::new(WorkflowErrorKind::Fatal, message)
    }

    pub fn retryable(message: impl Into<String>, retry_after_seconds: u64) -> Self {
        let mut error = Self::new(WorkflowErrorKind::Retryable, message);
        error.retry_after_seconds = Some(retry_after_seconds);
        error
    }

    pub fn too_early(message: impl Into<String>, retry_after_seconds: u64) -> Self {
        let mut error = Self::new(WorkflowErrorKind::TooEarly, message);
        error.retry_after_seconds = Some(retry_after_seconds);
        error
    }

    pub fn throttle(message: impl Into<String>, retry_after_seconds: u64) -> Self {
        let mut error = Self::new(WorkflowErrorKind::Throttle, message);
        error.retry_after_seconds = Some(retry_after_seconds);
        error
    }

    pub fn world(message: impl Into<String>, status: Option<u16>) -> Self {
        let mut error = Self::new(WorkflowErrorKind::World, message);
        error.status = status;
        error
    }

    pub fn world_contract(message: impl Into<String>) -> Self {
        let mut error = Self::new(WorkflowErrorKind::WorldContract, message);
        error.code = Some(RunErrorCode::WorldContractError);
        error
    }

    pub fn run_expired(message: impl Into<String>) -> Self {
        Self::new(WorkflowErrorKind::RunExpired, message)
    }

    pub fn queue(message: impl Into<String>) -> Self {
        Self::new(WorkflowErrorKind::Queue, message)
    }

    pub fn with_code(mut self, code: RunErrorCode) -> Self {
        self.code = Some(code);
        self
    }

    pub fn with_cause(mut self, cause: impl Into<String>) -> Self {
        self.cause = Some(cause.into());
        self
    }
}

impl fmt::Display for WorkflowCoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for WorkflowCoreError {}

pub type WorkflowResult<T> = Result<T, WorkflowCoreError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventKind {
    HookConflict,
    HookCreated,
    HookDisposed,
    HookReceived,
    RunCancelled,
    RunCompleted,
    RunCreated,
    RunFailed,
    RunStarted,
    StepCompleted,
    StepCreated,
    StepFailed,
    StepRetrying,
    StepStarted,
    WaitCompleted,
    WaitCreated,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HookConflict => "hook_conflict",
            Self::HookCreated => "hook_created",
            Self::HookDisposed => "hook_disposed",
            Self::HookReceived => "hook_received",
            Self::RunCancelled => "run_cancelled",
            Self::RunCompleted => "run_completed",
            Self::RunCreated => "run_created",
            Self::RunFailed => "run_failed",
            Self::RunStarted => "run_started",
            Self::StepCompleted => "step_completed",
            Self::StepCreated => "step_created",
            Self::StepFailed => "step_failed",
            Self::StepRetrying => "step_retrying",
            Self::StepStarted => "step_started",
            Self::WaitCompleted => "wait_completed",
            Self::WaitCreated => "wait_created",
        }
    }

    pub fn is_terminal_run(self) -> bool {
        matches!(
            self,
            Self::RunCancelled | Self::RunCompleted | Self::RunFailed
        )
    }

    pub fn is_terminal_step(self) -> bool {
        matches!(self, Self::StepCompleted | Self::StepFailed)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub event_id: String,
    pub run_id: String,
    pub event_type: EventKind,
    pub correlation_id: Option<String>,
    pub event_data: Value,
    pub request_id: Option<String>,
    pub spec_version: u32,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub run_id: String,
    pub workflow_name: String,
    pub status: RunStatus,
    pub input: Value,
    pub output: Option<Value>,
    pub error: Option<Value>,
    pub error_code: Option<RunErrorCode>,
    pub deployment_id: Option<String>,
    pub spec_version: u32,
    pub created_at_ms: u64,
    pub started_at_ms: Option<u64>,
    pub completed_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepRecord {
    pub step_id: String,
    pub step_name: String,
    pub status: StepStatus,
    pub input: Value,
    pub result: Option<Value>,
    pub error: Option<Value>,
    pub attempt: u32,
    pub started_at_ms: Option<u64>,
    pub retry_after_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventCreateOptions {
    pub request_id: Option<String>,
    pub v1_compat: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateEventRequest {
    pub event_type: EventKind,
    pub spec_version: u32,
    pub correlation_id: Option<String>,
    pub event_data: Value,
}

impl CreateEventRequest {
    pub fn new(event_type: EventKind) -> Self {
        Self {
            event_type,
            spec_version: SPEC_VERSION_CURRENT,
            correlation_id: None,
            event_data: Value::Null,
        }
    }

    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    pub fn with_data(mut self, event_data: Value) -> Self {
        self.event_data = event_data;
        self
    }

    pub fn with_spec_version(mut self, spec_version: u32) -> Self {
        self.spec_version = spec_version;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventCreateResult {
    pub event: Option<Event>,
    pub run: Option<WorkflowRun>,
    pub step: Option<StepRecord>,
    pub events: Vec<Event>,
    pub cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventListRequest {
    pub run_id: String,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventPage {
    pub data: Vec<Event>,
    pub cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct QueueMessage {
    pub run_id: String,
    pub step_id: Option<String>,
    pub step_name: Option<String>,
    pub requested_at_ms: Option<u64>,
    pub trace_carrier: Option<Value>,
    pub run_input: Option<RunInput>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct QueueOptions {
    pub deployment_id: Option<String>,
    pub spec_version: Option<u32>,
    pub delay_seconds: Option<u64>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct QueueRecord {
    pub queue_name: String,
    pub message: QueueMessage,
    pub options: QueueOptions,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct QueueResult {
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunInput {
    pub input: Value,
    pub deployment_id: Option<String>,
    pub workflow_name: String,
    pub spec_version: u32,
    pub execution_context: Value,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct StartContext {
    pub deployment_id: Option<String>,
}

pub trait RuntimeWorld {
    fn spec_version(&self) -> Option<u32>;
    fn process_exit_triggers_queue_redelivery(&self) -> bool;
    fn generate_run_id(&mut self) -> String;
    fn get_deployment_id(&self) -> Option<String>;
    fn resolve_latest_deployment_id(&mut self) -> WorkflowResult<String>;
    fn get_encryption_key_for_run(
        &mut self,
        run_id: &str,
        context: &StartContext,
    ) -> WorkflowResult<Option<Vec<u8>>>;
    fn events_create(
        &mut self,
        run_id: &str,
        request: CreateEventRequest,
        options: EventCreateOptions,
    ) -> WorkflowResult<EventCreateResult>;
    fn events_list(&mut self, request: EventListRequest) -> WorkflowResult<EventPage>;
    fn queue(
        &mut self,
        queue_name: &str,
        message: QueueMessage,
        options: QueueOptions,
    ) -> WorkflowResult<QueueResult>;
    fn runs_get(&self, run_id: &str) -> WorkflowResult<WorkflowRun>;
    fn now_ms(&self) -> u64;
}

#[derive(Debug, Clone)]
pub struct InMemoryWorld {
    pub spec_version: Option<u32>,
    pub process_exit_triggers_queue_redelivery: bool,
    pub deployment_id: Option<String>,
    pub latest_deployment_id: Option<String>,
    pub resolve_latest_calls: u32,
    pub encryption_contexts: Vec<(String, StartContext)>,
    pub runs: HashMap<String, WorkflowRun>,
    pub steps: HashMap<(String, String), StepRecord>,
    pub events: HashMap<String, Vec<Event>>,
    pub queues: Vec<QueueRecord>,
    pub event_create_failures: VecDeque<(Option<EventKind>, WorkflowCoreError)>,
    pub queue_failures: VecDeque<WorkflowCoreError>,
    pub runs_get_error: Option<WorkflowCoreError>,
    pub scripted_event_pages: VecDeque<EventPage>,
    pub rejected_cursors: HashSet<String>,
    pub force_repeated_cursor: bool,
    pub force_has_more_without_cursor: bool,
    pub active_hook_tokens: HashMap<String, String>,
    next_run_id: u64,
    next_event_id: u64,
    now_ms: u64,
    page_size: usize,
}

impl Default for InMemoryWorld {
    fn default() -> Self {
        Self {
            spec_version: Some(SPEC_VERSION_CURRENT),
            process_exit_triggers_queue_redelivery: false,
            deployment_id: Some("deploy_default".to_string()),
            latest_deployment_id: Some("deploy_latest".to_string()),
            resolve_latest_calls: 0,
            encryption_contexts: Vec::new(),
            runs: HashMap::new(),
            steps: HashMap::new(),
            events: HashMap::new(),
            queues: Vec::new(),
            event_create_failures: VecDeque::new(),
            queue_failures: VecDeque::new(),
            runs_get_error: None,
            scripted_event_pages: VecDeque::new(),
            rejected_cursors: HashSet::new(),
            force_repeated_cursor: false,
            force_has_more_without_cursor: false,
            active_hook_tokens: HashMap::new(),
            next_run_id: 1,
            next_event_id: 1,
            now_ms: 0,
            page_size: 1000,
        }
    }
}

impl InMemoryWorld {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_now_ms(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
    }

    pub fn advance_ms(&mut self, delta_ms: u64) {
        self.now_ms = self.now_ms.saturating_add(delta_ms);
    }

    pub fn set_page_size(&mut self, page_size: usize) {
        self.page_size = page_size.max(1);
    }

    pub fn fail_next_event_create(
        &mut self,
        event_type: Option<EventKind>,
        error: WorkflowCoreError,
    ) {
        self.event_create_failures.push_back((event_type, error));
    }

    pub fn fail_next_queue(&mut self, error: WorkflowCoreError) {
        self.queue_failures.push_back(error);
    }

    pub fn push_scripted_event_page(&mut self, page: EventPage) {
        self.scripted_event_pages.push_back(page);
    }

    pub fn insert_run(&mut self, run: WorkflowRun) {
        self.runs.insert(run.run_id.clone(), run);
    }

    pub fn insert_event(&mut self, run_id: &str, event: Event) {
        self.events
            .entry(run_id.to_string())
            .or_default()
            .push(event);
    }

    pub fn event_kinds(&self, run_id: &str) -> Vec<EventKind> {
        self.events
            .get(run_id)
            .map(|events| events.iter().map(|event| event.event_type).collect())
            .unwrap_or_default()
    }

    pub fn last_event(&self, run_id: &str) -> Option<&Event> {
        self.events.get(run_id).and_then(|events| events.last())
    }

    fn take_event_create_failure(&mut self, event_type: EventKind) -> Option<WorkflowCoreError> {
        let index = self
            .event_create_failures
            .iter()
            .position(|(kind, _)| kind.is_none_or(|kind| kind == event_type))?;
        self.event_create_failures
            .remove(index)
            .map(|(_, error)| error)
    }

    fn append_event(
        &mut self,
        run_id: &str,
        request: &CreateEventRequest,
        options: EventCreateOptions,
    ) -> Event {
        let event = Event {
            event_id: format!("event-{:06}", self.next_event_id),
            run_id: run_id.to_string(),
            event_type: request.event_type,
            correlation_id: request.correlation_id.clone(),
            event_data: request.event_data.clone(),
            request_id: options.request_id,
            spec_version: request.spec_version,
            created_at_ms: self.now_ms,
        };
        self.next_event_id += 1;
        self.events
            .entry(run_id.to_string())
            .or_default()
            .push(event.clone());
        event
    }

    fn ensure_run_not_terminal(&self, run_id: &str) -> WorkflowResult<()> {
        if let Some(run) = self.runs.get(run_id) {
            if matches!(
                run.status,
                RunStatus::Cancelled | RunStatus::Completed | RunStatus::Failed
            ) {
                return Err(WorkflowCoreError::run_expired(format!(
                    "Workflow run \"{run_id}\" has already completed"
                )));
            }
        }
        Ok(())
    }

    fn step_key(run_id: &str, correlation_id: &str) -> (String, String) {
        (run_id.to_string(), correlation_id.to_string())
    }
}

impl RuntimeWorld for InMemoryWorld {
    fn spec_version(&self) -> Option<u32> {
        self.spec_version
    }

    fn process_exit_triggers_queue_redelivery(&self) -> bool {
        self.process_exit_triggers_queue_redelivery
    }

    fn generate_run_id(&mut self) -> String {
        let run_id = format!("wrun_{:06}", self.next_run_id);
        self.next_run_id += 1;
        run_id
    }

    fn get_deployment_id(&self) -> Option<String> {
        self.deployment_id.clone()
    }

    fn resolve_latest_deployment_id(&mut self) -> WorkflowResult<String> {
        self.resolve_latest_calls += 1;
        self.latest_deployment_id.clone().ok_or_else(|| {
            WorkflowCoreError::runtime(
                "deploymentId 'latest' requires a World that implements resolveLatestDeploymentId()",
            )
        })
    }

    fn get_encryption_key_for_run(
        &mut self,
        run_id: &str,
        context: &StartContext,
    ) -> WorkflowResult<Option<Vec<u8>>> {
        self.encryption_contexts
            .push((run_id.to_string(), context.clone()));
        Ok(None)
    }

    fn events_create(
        &mut self,
        run_id: &str,
        request: CreateEventRequest,
        options: EventCreateOptions,
    ) -> WorkflowResult<EventCreateResult> {
        if let Some(error) = self.take_event_create_failure(request.event_type) {
            return Err(error);
        }

        let mut result = EventCreateResult::default();
        match request.event_type {
            EventKind::RunCreated => {
                if self.runs.contains_key(run_id) {
                    return Err(WorkflowCoreError::conflict("run already exists"));
                }
                let workflow_name = request
                    .event_data
                    .get("workflowName")
                    .and_then(Value::as_str)
                    .unwrap_or("workflow")
                    .to_string();
                let deployment_id = request
                    .event_data
                    .get("deploymentId")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let input = request
                    .event_data
                    .get("input")
                    .cloned()
                    .unwrap_or(Value::Null);
                let run = WorkflowRun {
                    run_id: run_id.to_string(),
                    workflow_name,
                    status: RunStatus::Pending,
                    input,
                    output: None,
                    error: None,
                    error_code: None,
                    deployment_id,
                    spec_version: request.spec_version,
                    created_at_ms: self.now_ms,
                    started_at_ms: None,
                    completed_at_ms: None,
                };
                self.runs.insert(run_id.to_string(), run.clone());
                result.run = Some(run);
            }
            EventKind::RunStarted => {
                self.ensure_run_not_terminal(run_id)?;
                if !self.runs.contains_key(run_id) {
                    let data = &request.event_data;
                    let workflow_name = data
                        .get("workflowName")
                        .and_then(Value::as_str)
                        .unwrap_or("workflow")
                        .to_string();
                    let deployment_id = data
                        .get("deploymentId")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let input = data.get("input").cloned().unwrap_or(Value::Null);
                    self.runs.insert(
                        run_id.to_string(),
                        WorkflowRun {
                            run_id: run_id.to_string(),
                            workflow_name,
                            status: RunStatus::Pending,
                            input,
                            output: None,
                            error: None,
                            error_code: None,
                            deployment_id,
                            spec_version: request.spec_version,
                            created_at_ms: self.now_ms,
                            started_at_ms: None,
                            completed_at_ms: None,
                        },
                    );
                }
                let run = self.runs.get_mut(run_id).expect("run inserted");
                run.status = RunStatus::Running;
                run.started_at_ms.get_or_insert(self.now_ms);
                result.run = Some(run.clone());
                result.events = self.events.get(run_id).cloned().unwrap_or_default();
                result.cursor = cursor_for_len(result.events.len());
            }
            EventKind::RunCompleted => {
                self.ensure_run_not_terminal(run_id)?;
                let run = self
                    .runs
                    .get_mut(run_id)
                    .ok_or_else(|| WorkflowCoreError::not_found("run not found"))?;
                run.status = RunStatus::Completed;
                run.output = request.event_data.get("output").cloned();
                run.completed_at_ms = Some(self.now_ms);
                result.run = Some(run.clone());
            }
            EventKind::RunFailed => {
                if let Some(run) = self.runs.get(run_id) {
                    if matches!(
                        run.status,
                        RunStatus::Cancelled | RunStatus::Completed | RunStatus::Failed
                    ) {
                        return Err(WorkflowCoreError::conflict("run already terminal"));
                    }
                }
                let run = self
                    .runs
                    .entry(run_id.to_string())
                    .or_insert_with(|| WorkflowRun {
                        run_id: run_id.to_string(),
                        workflow_name: "workflow".to_string(),
                        status: RunStatus::Pending,
                        input: Value::Null,
                        output: None,
                        error: None,
                        error_code: None,
                        deployment_id: None,
                        spec_version: request.spec_version,
                        created_at_ms: self.now_ms,
                        started_at_ms: None,
                        completed_at_ms: None,
                    });
                run.status = RunStatus::Failed;
                run.error = request.event_data.get("error").cloned();
                run.error_code = request
                    .event_data
                    .get("errorCode")
                    .and_then(Value::as_str)
                    .and_then(parse_run_error_code);
                run.completed_at_ms = Some(self.now_ms);
                result.run = Some(run.clone());
            }
            EventKind::RunCancelled => {
                self.ensure_run_not_terminal(run_id)?;
                let run = self
                    .runs
                    .get_mut(run_id)
                    .ok_or_else(|| WorkflowCoreError::not_found("run not found"))?;
                run.status = RunStatus::Cancelled;
                run.completed_at_ms = Some(self.now_ms);
                result.run = Some(run.clone());
            }
            EventKind::StepCreated => {
                self.ensure_run_not_terminal(run_id)?;
                let correlation_id = request.correlation_id.clone().ok_or_else(|| {
                    WorkflowCoreError::runtime("step_created missing correlation_id")
                })?;
                let key = Self::step_key(run_id, &correlation_id);
                if self.steps.contains_key(&key) {
                    return Err(WorkflowCoreError::conflict("step already exists"));
                }
                let step_name = request
                    .event_data
                    .get("stepName")
                    .and_then(Value::as_str)
                    .unwrap_or("step")
                    .to_string();
                let input = request
                    .event_data
                    .get("input")
                    .cloned()
                    .unwrap_or(Value::Null);
                let step = StepRecord {
                    step_id: correlation_id,
                    step_name,
                    status: StepStatus::Pending,
                    input,
                    result: None,
                    error: None,
                    attempt: 0,
                    started_at_ms: None,
                    retry_after_ms: None,
                };
                self.steps.insert(key, step.clone());
                result.step = Some(step);
            }
            EventKind::StepStarted => {
                self.ensure_run_not_terminal(run_id)?;
                let correlation_id = request.correlation_id.clone().ok_or_else(|| {
                    WorkflowCoreError::runtime("step_started missing correlation_id")
                })?;
                let key = Self::step_key(run_id, &correlation_id);
                let step = self
                    .steps
                    .get_mut(&key)
                    .ok_or_else(|| WorkflowCoreError::not_found("step not found"))?;
                if matches!(step.status, StepStatus::Completed | StepStatus::Failed) {
                    return Err(WorkflowCoreError::conflict("step already terminal"));
                }
                if let Some(retry_after_ms) = step.retry_after_ms {
                    if self.now_ms < retry_after_ms {
                        let delay = ((retry_after_ms - self.now_ms).saturating_add(999)) / 1000;
                        return Err(WorkflowCoreError::too_early(
                            "retryAfter timestamp not reached",
                            delay.max(1),
                        ));
                    }
                }
                step.status = StepStatus::Running;
                step.attempt += 1;
                step.started_at_ms.get_or_insert(self.now_ms);
                result.step = Some(step.clone());
            }
            EventKind::StepCompleted => {
                self.ensure_run_not_terminal(run_id)?;
                let correlation_id = request.correlation_id.clone().ok_or_else(|| {
                    WorkflowCoreError::runtime("step_completed missing correlation_id")
                })?;
                let key = Self::step_key(run_id, &correlation_id);
                let step = self
                    .steps
                    .get_mut(&key)
                    .ok_or_else(|| WorkflowCoreError::not_found("step not found"))?;
                if matches!(step.status, StepStatus::Completed | StepStatus::Failed) {
                    return Err(WorkflowCoreError::conflict("step already terminal"));
                }
                step.status = StepStatus::Completed;
                step.result = request.event_data.get("result").cloned();
                result.step = Some(step.clone());
            }
            EventKind::StepFailed => {
                let correlation_id = request.correlation_id.clone().ok_or_else(|| {
                    WorkflowCoreError::runtime("step_failed missing correlation_id")
                })?;
                let key = Self::step_key(run_id, &correlation_id);
                let step = self.steps.entry(key).or_insert_with(|| StepRecord {
                    step_id: correlation_id.clone(),
                    step_name: request
                        .event_data
                        .get("stepName")
                        .and_then(Value::as_str)
                        .unwrap_or("step")
                        .to_string(),
                    status: StepStatus::Pending,
                    input: Value::Null,
                    result: None,
                    error: None,
                    attempt: 0,
                    started_at_ms: None,
                    retry_after_ms: None,
                });
                if matches!(step.status, StepStatus::Completed | StepStatus::Failed) {
                    return Err(WorkflowCoreError::conflict("step already terminal"));
                }
                step.status = StepStatus::Failed;
                step.error = request.event_data.get("error").cloned();
                result.step = Some(step.clone());
            }
            EventKind::StepRetrying => {
                let correlation_id = request.correlation_id.clone().ok_or_else(|| {
                    WorkflowCoreError::runtime("step_retrying missing correlation_id")
                })?;
                let key = Self::step_key(run_id, &correlation_id);
                let step = self
                    .steps
                    .get_mut(&key)
                    .ok_or_else(|| WorkflowCoreError::not_found("step not found"))?;
                if matches!(step.status, StepStatus::Completed | StepStatus::Failed) {
                    return Err(WorkflowCoreError::conflict("step already terminal"));
                }
                step.status = StepStatus::Pending;
                step.error = request.event_data.get("error").cloned();
                step.retry_after_ms = request
                    .event_data
                    .get("retryAfterMs")
                    .and_then(Value::as_u64);
                result.step = Some(step.clone());
            }
            EventKind::WaitCompleted => {
                let duplicate = self.events.get(run_id).is_some_and(|events| {
                    events.iter().any(|event| {
                        event.event_type == EventKind::WaitCompleted
                            && event.correlation_id == request.correlation_id
                    })
                });
                if duplicate {
                    return Err(WorkflowCoreError::conflict("wait already completed"));
                }
            }
            EventKind::HookCreated => {
                let token = request
                    .event_data
                    .get("token")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if let Some(owner) = self.active_hook_tokens.get(&token) {
                    if owner != run_id {
                        let conflict = CreateEventRequest::new(EventKind::HookConflict)
                            .with_correlation_id(request.correlation_id.clone().unwrap_or_default())
                            .with_data(json!({ "token": token }));
                        let event = self.append_event(run_id, &conflict, options);
                        result.event = Some(event);
                        return Ok(result);
                    }
                }
                self.active_hook_tokens.insert(token, run_id.to_string());
            }
            EventKind::HookDisposed => {
                if let Some(token) = request.event_data.get("token").and_then(Value::as_str) {
                    self.active_hook_tokens.remove(token);
                }
            }
            EventKind::HookReceived | EventKind::WaitCreated | EventKind::HookConflict => {}
        }

        let event = self.append_event(run_id, &request, options);
        result.event = Some(event);
        Ok(result)
    }

    fn events_list(&mut self, request: EventListRequest) -> WorkflowResult<EventPage> {
        if let Some(page) = self.scripted_event_pages.pop_front() {
            return Ok(page);
        }
        if request
            .cursor
            .as_ref()
            .is_some_and(|cursor| self.rejected_cursors.contains(cursor))
        {
            return Err(WorkflowCoreError::world("cursor rejected", Some(400)));
        }

        let events = self
            .events
            .get(&request.run_id)
            .cloned()
            .unwrap_or_default();
        let start = request
            .cursor
            .as_deref()
            .map(parse_cursor)
            .transpose()?
            .unwrap_or(0);
        let limit = request.limit.unwrap_or(self.page_size).max(1);
        let end = start.saturating_add(limit).min(events.len());
        let data = events[start..end].to_vec();
        let has_more = self.force_has_more_without_cursor || end < events.len();
        let cursor = if self.force_has_more_without_cursor {
            None
        } else if self.force_repeated_cursor {
            request
                .cursor
                .clone()
                .or_else(|| Some(format!("cursor_{end}")))
        } else if end == 0 || (data.is_empty() && end >= events.len()) {
            None
        } else {
            Some(format!("cursor_{end}"))
        };
        Ok(EventPage {
            data,
            cursor,
            has_more,
        })
    }

    fn queue(
        &mut self,
        queue_name: &str,
        message: QueueMessage,
        options: QueueOptions,
    ) -> WorkflowResult<QueueResult> {
        if let Some(error) = self.queue_failures.pop_front() {
            return Err(error);
        }
        self.queues.push(QueueRecord {
            queue_name: queue_name.to_string(),
            message,
            options,
        });
        Ok(QueueResult {
            message_id: Some(format!("msg-{:06}", self.queues.len())),
        })
    }

    fn runs_get(&self, run_id: &str) -> WorkflowResult<WorkflowRun> {
        if let Some(error) = &self.runs_get_error {
            return Err(error.clone());
        }
        self.runs
            .get(run_id)
            .cloned()
            .ok_or_else(|| WorkflowCoreError::not_found(format!("Workflow run {run_id} not found")))
    }

    fn now_ms(&self) -> u64 {
        self.now_ms
    }
}

fn cursor_for_len(len: usize) -> Option<String> {
    if len == 0 {
        None
    } else {
        Some(format!("cursor_{len}"))
    }
}

fn parse_cursor(cursor: &str) -> WorkflowResult<usize> {
    cursor
        .strip_prefix("cursor_")
        .ok_or_else(|| WorkflowCoreError::world("invalid cursor", Some(400)))?
        .parse::<usize>()
        .map_err(|_| WorkflowCoreError::world("invalid cursor", Some(400)))
}

fn parse_run_error_code(code: &str) -> Option<RunErrorCode> {
    match code {
        "CORRUPTED_EVENT_LOG" => Some(RunErrorCode::CorruptedEventLog),
        "MAX_DELIVERIES_EXCEEDED" => Some(RunErrorCode::MaxDeliveriesExceeded),
        "REPLAY_TIMEOUT" => Some(RunErrorCode::ReplayTimeout),
        "WORKFLOW_RUNTIME_ERROR" => Some(RunErrorCode::WorkflowRuntimeError),
        "WORLD_CONTRACT_ERROR" => Some(RunErrorCode::WorldContractError),
        _ => None,
    }
}

pub fn is_legacy_spec_version(spec_version: u32) -> bool {
    spec_version <= SPEC_VERSION_LEGACY
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayTimeoutWarnings {
    warned_values: HashSet<String>,
    warnings: Vec<String>,
}

impl ReplayTimeoutWarnings {
    pub fn new() -> Self {
        Self {
            warned_values: HashSet::new(),
            warnings: Vec::new(),
        }
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn warn_once(&mut self, raw: &str, message: String) {
        if self.warned_values.insert(raw.to_string()) {
            self.warnings.push(message);
        }
    }
}

impl Default for ReplayTimeoutWarnings {
    fn default() -> Self {
        Self::new()
    }
}

pub fn get_replay_timeout_ms(raw_value: Option<&str>, warnings: &mut ReplayTimeoutWarnings) -> u64 {
    let Some(raw) = raw_value else {
        return REPLAY_TIMEOUT_MS;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return REPLAY_TIMEOUT_MS;
    }
    let Ok(parsed) = trimmed.parse::<f64>() else {
        warnings.warn_once(trimmed, format!("Invalid replay timeout: {trimmed}"));
        return REPLAY_TIMEOUT_MS;
    };
    if !parsed.is_finite() || parsed <= 0.0 {
        warnings.warn_once(trimmed, format!("Invalid replay timeout: {trimmed}"));
        return REPLAY_TIMEOUT_MS;
    }
    let parsed = parsed as u64;
    if parsed < MIN_REPLAY_TIMEOUT_MS {
        warnings.warn_once(
            trimmed,
            format!("Replay timeout {parsed}ms below minimum; clamping"),
        );
        return MIN_REPLAY_TIMEOUT_MS;
    }
    if parsed > MAX_REPLAY_TIMEOUT_MS {
        warnings.warn_once(
            trimmed,
            format!("Replay timeout {parsed}ms above maximum; clamping"),
        );
        return MAX_REPLAY_TIMEOUT_MS;
    }
    parsed
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayBudget {
    limit_ms: u64,
    elapsed_ms: u64,
    interval_start_ms: Option<u64>,
}

impl ReplayBudget {
    pub fn new(limit_ms: u64, now_ms: u64) -> Self {
        Self {
            limit_ms,
            elapsed_ms: 0,
            interval_start_ms: Some(now_ms),
        }
    }

    pub fn configured_limit_ms(&self) -> u64 {
        self.limit_ms
    }

    pub fn elapsed(&self, now_ms: u64) -> u64 {
        self.elapsed_ms
            + self
                .interval_start_ms
                .map(|start| now_ms.saturating_sub(start))
                .unwrap_or(0)
    }

    pub fn pause(&mut self, now_ms: u64) {
        let Some(start) = self.interval_start_ms.take() else {
            return;
        };
        self.elapsed_ms = self.elapsed_ms.saturating_add(now_ms.saturating_sub(start));
    }

    pub fn resume(&mut self, now_ms: u64) {
        if self.interval_start_ms.is_none() {
            self.interval_start_ms = Some(now_ms);
        }
    }

    pub fn is_exhausted(&self, now_ms: u64) -> bool {
        self.elapsed(now_ms) >= self.limit_ms
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayTimeoutAction {
    Returned,
    ExitForRedelivery,
}

pub fn handle_replay_budget_exhausted<W: RuntimeWorld>(
    world: &mut W,
    run_id: &str,
    workflow_name: &str,
    request_id: Option<String>,
    attempt: u32,
    limit_ms: u64,
) -> WorkflowResult<ReplayTimeoutAction> {
    if world.process_exit_triggers_queue_redelivery() && attempt <= REPLAY_TIMEOUT_MAX_RETRIES {
        return Ok(ReplayTimeoutAction::ExitForRedelivery);
    }

    let message = if attempt > REPLAY_TIMEOUT_MAX_RETRIES {
        format!(
            "Workflow replay exceeded maximum duration ({}s) after {attempt} attempts",
            limit_ms / 1000
        )
    } else {
        format!(
            "Workflow replay exceeded maximum duration ({}s)",
            limit_ms / 1000
        )
    };
    let request = CreateEventRequest::new(EventKind::RunFailed).with_data(json!({
        "workflowName": workflow_name,
        "error": { "name": "FatalError", "message": message },
        "errorCode": RunErrorCode::ReplayTimeout.as_str(),
    }));
    let _ = world.events_create(
        run_id,
        request,
        EventCreateOptions {
            request_id,
            v1_compat: false,
        },
    );

    if world.process_exit_triggers_queue_redelivery() {
        Ok(ReplayTimeoutAction::ExitForRedelivery)
    } else {
        Ok(ReplayTimeoutAction::Returned)
    }
}

pub fn get_workflow_queue_name(workflow_name: &str) -> WorkflowResult<String> {
    if workflow_name.is_empty()
        || !workflow_name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '@'))
    {
        return Err(WorkflowCoreError::runtime(format!(
            "Invalid workflow name \"{workflow_name}\": must only contain alphanumeric characters, underscores, hyphens, dots, forward slashes, or at signs"
        )));
    }
    Ok(format!("__wkf_workflow_{workflow_name}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeploymentId {
    Id(String),
    Latest,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartOptions {
    pub deployment_id: Option<DeploymentId>,
    pub spec_version: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDefinition {
    workflow_id: String,
}

impl WorkflowDefinition {
    pub fn new(workflow_id: impl Into<String>) -> Self {
        Self {
            workflow_id: workflow_id.into(),
        }
    }

    pub fn workflow_id(&self) -> &str {
        &self.workflow_id
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunHandle {
    pub run_id: String,
    pub resilient_start: bool,
}

impl RunHandle {
    pub fn new(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            resilient_start: false,
        }
    }

    pub fn serialize(&self) -> Value {
        json!({
            "runId": self.run_id,
            "resilientStart": self.resilient_start,
        })
    }

    pub fn deserialize(data: &Value) -> WorkflowResult<Self> {
        let run_id = data
            .get("runId")
            .and_then(Value::as_str)
            .ok_or_else(|| WorkflowCoreError::runtime("serialized Run missing runId"))?;
        Ok(Self {
            run_id: run_id.to_string(),
            resilient_start: data
                .get("resilientStart")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    pub fn exists<W: RuntimeWorld>(&self, world: &W) -> WorkflowResult<bool> {
        match world.runs_get(&self.run_id) {
            Ok(_) => Ok(true),
            Err(error) if error.kind == WorkflowErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn status<W: RuntimeWorld>(&self, world: &W) -> WorkflowResult<RunStatus> {
        Ok(world.runs_get(&self.run_id)?.status)
    }

    pub fn wake_up<W: RuntimeWorld>(
        &self,
        world: &mut W,
        options: StopSleepOptions,
    ) -> WorkflowResult<StopSleepResult> {
        wake_up_run(world, &self.run_id, options)
    }

    pub fn return_value<W: RuntimeWorld>(&self, world: &W) -> WorkflowResult<Value> {
        let run = world.runs_get(&self.run_id)?;
        match run.status {
            RunStatus::Completed => Ok(run.output.unwrap_or(Value::Null)),
            RunStatus::Cancelled => Err(WorkflowCoreError::runtime(format!(
                "Workflow run {} was cancelled",
                self.run_id
            ))),
            RunStatus::Failed => {
                let message = run
                    .error
                    .as_ref()
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .or_else(|| {
                        run.error
                            .as_ref()
                            .and_then(|error| error.get("error"))
                            .and_then(|error| error.get("message"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or("Failed to hydrate workflow run error");
                let cause = run
                    .error
                    .as_ref()
                    .and_then(|error| error.get("cause"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let mut error = WorkflowCoreError::fatal(format!(
                    "Workflow run {} failed: {message}",
                    self.run_id
                ));
                error.code = run.error_code;
                error.cause = cause;
                Err(error)
            }
            RunStatus::Pending | RunStatus::Running => Err(WorkflowCoreError::runtime(format!(
                "Workflow run {} has not completed",
                self.run_id
            ))),
        }
    }
}

pub fn start<W: RuntimeWorld>(
    world: &mut W,
    workflow: Option<&WorkflowDefinition>,
    args: Vec<Value>,
    options: StartOptions,
) -> WorkflowResult<RunHandle> {
    let workflow_id = workflow
        .map(WorkflowDefinition::workflow_id)
        .filter(|workflow_id| !workflow_id.is_empty())
        .ok_or_else(|| {
            WorkflowCoreError::runtime(
                "'start' received an invalid workflow function. Ensure the Workflow SDK is configured correctly and the function includes a 'use workflow' directive.",
            )
        })?;

    let mut deployment_id = match options.deployment_id.clone() {
        Some(DeploymentId::Id(deployment_id)) => Some(deployment_id),
        Some(DeploymentId::Latest) => Some(world.resolve_latest_deployment_id()?),
        None => world.get_deployment_id(),
    };
    if matches!(options.deployment_id, Some(DeploymentId::Latest)) && deployment_id.is_none() {
        deployment_id = Some(world.resolve_latest_deployment_id()?);
    }

    let run_id = world.generate_run_id();
    let spec_version = options
        .spec_version
        .or_else(|| world.spec_version())
        .unwrap_or(SPEC_VERSION_SUPPORTS_EVENT_SOURCING);
    let v1_compat = is_legacy_spec_version(spec_version);

    let start_context = StartContext {
        deployment_id: deployment_id.clone(),
    };
    let _ = world.get_encryption_key_for_run(&run_id, &start_context)?;

    let workflow_arguments = Value::Array(args);
    let execution_context = json!({
        "traceCarrier": {},
        "workflowCoreVersion": crate::CRATE_VERSION,
        "features": { "encryption": false },
    });

    let run_created = CreateEventRequest::new(EventKind::RunCreated)
        .with_spec_version(spec_version)
        .with_data(json!({
            "deploymentId": deployment_id,
            "workflowName": workflow_id,
            "input": workflow_arguments,
            "executionContext": execution_context,
        }));

    let run_created_result = world.events_create(
        &run_id,
        run_created,
        EventCreateOptions {
            request_id: None,
            v1_compat,
        },
    );

    let run_input = if spec_version >= SPEC_VERSION_SUPPORTS_CBOR_QUEUE_TRANSPORT {
        Some(RunInput {
            input: Value::Array(Vec::new()),
            deployment_id: start_context.deployment_id.clone(),
            workflow_name: workflow_id.to_string(),
            spec_version,
            execution_context: json!({ "features": { "encryption": false } }),
        })
    } else {
        None
    };
    let queue_result = world.queue(
        &get_workflow_queue_name(workflow_id)?,
        QueueMessage {
            run_id: run_id.clone(),
            requested_at_ms: Some(world.now_ms()),
            run_input,
            ..QueueMessage::default()
        },
        QueueOptions {
            deployment_id: start_context.deployment_id,
            spec_version: Some(spec_version),
            ..QueueOptions::default()
        },
    );

    queue_result?;

    let mut resilient_start = false;
    match run_created_result {
        Ok(result) => {
            if !v1_compat {
                if let Some(run) = result.run {
                    if run.run_id != run_id {
                        return Err(WorkflowCoreError::runtime(format!(
                            "Server returned different runId than requested: expected {run_id}, got {}",
                            run.run_id
                        )));
                    }
                } else {
                    return Err(WorkflowCoreError::runtime(
                        "Missing 'run' in server response for 'run_created' event",
                    ));
                }
            }
        }
        Err(error) if error.kind == WorkflowErrorKind::Conflict => {}
        Err(error) if is_retryable_start_error(&error) => {
            resilient_start = true;
        }
        Err(error) => return Err(error),
    }

    Ok(RunHandle {
        run_id,
        resilient_start,
    })
}

fn is_retryable_start_error(error: &WorkflowCoreError) -> bool {
    error.kind == WorkflowErrorKind::Throttle
        || (error.kind == WorkflowErrorKind::World
            && error.status.is_some_and(|status| status >= 500))
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoadedEvents {
    pub events: Vec<Event>,
    pub cursor: Option<String>,
}

pub fn load_workflow_run_events<W: RuntimeWorld>(
    world: &mut W,
    run_id: &str,
    after_cursor: Option<String>,
) -> WorkflowResult<LoadedEvents> {
    let mut loaded_events = Vec::new();
    let mut loaded_event_ids = HashSet::new();
    let mut requested_cursors = HashSet::new();
    let mut cursor = after_cursor;
    let mut has_more = true;
    let mut retried_without_cursor = false;

    while has_more {
        record_requested_event_cursor(run_id, cursor.as_deref(), &mut requested_cursors)?;
        let requested_cursor = cursor.clone();
        let response = match world.events_list(EventListRequest {
            run_id: run_id.to_string(),
            cursor: requested_cursor.clone(),
            limit: None,
        }) {
            Ok(response) => response,
            Err(error)
                if should_retry_without_event_cursor(
                    &error,
                    requested_cursor.as_deref(),
                    retried_without_cursor,
                ) =>
            {
                loaded_events.clear();
                loaded_event_ids.clear();
                requested_cursors.clear();
                cursor = None;
                retried_without_cursor = true;
                continue;
            }
            Err(error) => return Err(error),
        };

        append_unique_events(&mut loaded_events, &mut loaded_event_ids, response.data);
        has_more = response.has_more;
        assert_event_pagination_progress(
            run_id,
            has_more,
            response.cursor.as_deref(),
            &requested_cursors,
        )?;
        cursor = response.cursor.or(cursor);
    }

    Ok(LoadedEvents {
        events: loaded_events,
        cursor,
    })
}

fn record_requested_event_cursor(
    run_id: &str,
    cursor: Option<&str>,
    requested_cursors: &mut HashSet<String>,
) -> WorkflowResult<()> {
    let Some(cursor) = cursor else {
        return Ok(());
    };
    if !requested_cursors.insert(cursor.to_string()) {
        return Err(event_pagination_contract_error(run_id, "did not advance"));
    }
    Ok(())
}

fn append_unique_events(
    target: &mut Vec<Event>,
    target_ids: &mut HashSet<String>,
    events: Vec<Event>,
) {
    for event in events {
        if target_ids.insert(event.event_id.clone()) {
            target.push(event);
        }
    }
}

fn assert_event_pagination_progress(
    run_id: &str,
    has_more: bool,
    cursor: Option<&str>,
    requested_cursors: &HashSet<String>,
) -> WorkflowResult<()> {
    if !has_more {
        return Ok(());
    }
    let Some(cursor) = cursor else {
        return Err(event_pagination_contract_error(
            run_id,
            "returned more pages without a cursor",
        ));
    };
    if requested_cursors.contains(cursor) {
        return Err(event_pagination_contract_error(run_id, "repeated a cursor"));
    }
    Ok(())
}

fn should_retry_without_event_cursor(
    error: &WorkflowCoreError,
    cursor: Option<&str>,
    already_retried: bool,
) -> bool {
    cursor.is_some()
        && !already_retried
        && error.kind == WorkflowErrorKind::World
        && error.status == Some(400)
}

fn event_pagination_contract_error(run_id: &str, message: &str) -> WorkflowCoreError {
    WorkflowCoreError::world_contract(format!(
        "Event pagination {message} for workflow run \"{run_id}\"."
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StopSleepOptions {
    pub correlation_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StopSleepResult {
    pub stopped_count: usize,
}

pub fn wake_up_run<W: RuntimeWorld>(
    world: &mut W,
    run_id: &str,
    options: StopSleepOptions,
) -> WorkflowResult<StopSleepResult> {
    let run = world.runs_get(run_id)?;
    let events = load_workflow_run_events(world, run_id, None)?.events;
    let completed: HashSet<Option<String>> = events
        .iter()
        .filter(|event| event.event_type == EventKind::WaitCompleted)
        .map(|event| event.correlation_id.clone())
        .collect();
    let targets: Option<HashSet<String>> = options
        .correlation_ids
        .map(|ids| ids.into_iter().collect::<HashSet<_>>());

    let mut stopped_count = 0;
    let mut errors = Vec::new();
    for wait in events
        .iter()
        .filter(|event| event.event_type == EventKind::WaitCreated)
    {
        if completed.contains(&wait.correlation_id) {
            continue;
        }
        let Some(correlation_id) = wait.correlation_id.clone() else {
            continue;
        };
        if targets
            .as_ref()
            .is_some_and(|targets| !targets.contains(&correlation_id))
        {
            continue;
        }
        let request = CreateEventRequest::new(EventKind::WaitCompleted)
            .with_spec_version(run.spec_version)
            .with_correlation_id(correlation_id)
            .with_data(json!({ "resumeAt": wait.event_data.get("resumeAt").cloned().unwrap_or(Value::Null) }));
        match world.events_create(
            run_id,
            request,
            EventCreateOptions {
                request_id: None,
                v1_compat: is_legacy_spec_version(run.spec_version),
            },
        ) {
            Ok(_) => stopped_count += 1,
            Err(error) if error.kind == WorkflowErrorKind::Conflict => stopped_count += 1,
            Err(error) => errors.push(error),
        }
    }

    if stopped_count > 0 {
        world.queue(
            &get_workflow_queue_name(&run.workflow_name)?,
            QueueMessage {
                run_id: run_id.to_string(),
                ..QueueMessage::default()
            },
            QueueOptions {
                deployment_id: run.deployment_id,
                spec_version: Some(run.spec_version),
                ..QueueOptions::default()
            },
        )?;
    }

    if let Some(error) = errors.into_iter().next() {
        return Err(error);
    }
    Ok(StopSleepResult { stopped_count })
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepBehavior {
    Complete(Value),
    PendingOps(Value),
    Fatal(String),
    Retryable {
        message: String,
        retry_after_seconds: u64,
    },
    Abort(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepFunction {
    pub max_retries: u32,
    pub behavior: StepBehavior,
}

impl StepFunction {
    pub fn complete(value: Value) -> Self {
        Self {
            max_retries: 3,
            behavior: StepBehavior::Complete(value),
        }
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RegisteredStep {
    Function(StepFunction),
    NonFunction,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StepRegistry {
    steps: HashMap<String, RegisteredStep>,
}

impl StepRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: impl Into<String>, step: StepFunction) {
        self.steps
            .insert(name.into(), RegisteredStep::Function(step));
    }

    pub fn register_non_function(&mut self, name: impl Into<String>) {
        self.steps.insert(name.into(), RegisteredStep::NonFunction);
    }

    fn get(&self, name: &str) -> Option<&RegisteredStep> {
        self.steps.get(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepExecutionResult {
    Completed { has_pending_ops: bool },
    Failed,
    Retry { timeout_seconds: u64 },
    Skipped,
    Gone,
    Throttled { timeout_seconds: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepExecutorParams {
    pub workflow_run_id: String,
    pub workflow_name: String,
    pub workflow_started_at_ms: u64,
    pub step_id: String,
    pub step_name: String,
    pub request_id: Option<String>,
}

pub fn execute_step<W: RuntimeWorld>(
    world: &mut W,
    registry: &StepRegistry,
    params: StepExecutorParams,
) -> WorkflowResult<StepExecutionResult> {
    let registered = registry.get(&params.step_name);
    if !matches!(registered, Some(RegisteredStep::Function(_))) {
        let error_message = format!(
            "Step \"{}\" is not registered in the current deployment.",
            params.step_name
        );
        let request = CreateEventRequest::new(EventKind::StepFailed)
            .with_correlation_id(params.step_id.clone())
            .with_data(json!({
                "stepName": params.step_name,
                "error": { "name": "FatalError", "message": error_message },
            }));
        match world.events_create(
            &params.workflow_run_id,
            request,
            EventCreateOptions {
                request_id: params.request_id.clone(),
                v1_compat: false,
            },
        ) {
            Ok(_) => return Ok(StepExecutionResult::Failed),
            Err(error) if error.kind == WorkflowErrorKind::Conflict => {
                return Ok(StepExecutionResult::Skipped);
            }
            Err(error) => return Err(error),
        }
    }

    let step_fn = match registered.expect("checked above") {
        RegisteredStep::Function(step_fn) => step_fn,
        RegisteredStep::NonFunction => unreachable!("checked above"),
    };

    let start = CreateEventRequest::new(EventKind::StepStarted)
        .with_correlation_id(params.step_id.clone())
        .with_data(json!({ "stepName": params.step_name }));
    let step = match world.events_create(
        &params.workflow_run_id,
        start,
        EventCreateOptions {
            request_id: params.request_id.clone(),
            v1_compat: false,
        },
    ) {
        Ok(result) => result
            .step
            .ok_or_else(|| WorkflowCoreError::runtime("step_started did not return step"))?,
        Err(error) if error.kind == WorkflowErrorKind::Throttle => {
            return Ok(StepExecutionResult::Throttled {
                timeout_seconds: error.retry_after_seconds.unwrap_or(1).max(1),
            });
        }
        Err(error) if error.kind == WorkflowErrorKind::RunExpired => {
            return Ok(StepExecutionResult::Gone);
        }
        Err(error) if error.kind == WorkflowErrorKind::Conflict => {
            return Ok(StepExecutionResult::Skipped);
        }
        Err(error) if error.kind == WorkflowErrorKind::TooEarly => {
            return Ok(StepExecutionResult::Retry {
                timeout_seconds: error.retry_after_seconds.unwrap_or(1).max(1),
            });
        }
        Err(error) => return Err(error),
    };

    if step.attempt > step_fn.max_retries + 1 && step.error.is_some() {
        return fail_step_for_max_retries(world, &params, step_fn.max_retries, step.error);
    }

    match &step_fn.behavior {
        StepBehavior::Complete(result) | StepBehavior::PendingOps(result) => {
            let request = CreateEventRequest::new(EventKind::StepCompleted)
                .with_correlation_id(params.step_id.clone())
                .with_data(json!({
                    "stepName": params.step_name,
                    "result": result,
                }));
            match world.events_create(
                &params.workflow_run_id,
                request,
                EventCreateOptions {
                    request_id: params.request_id,
                    v1_compat: false,
                },
            ) {
                Ok(_) => Ok(StepExecutionResult::Completed {
                    has_pending_ops: matches!(step_fn.behavior, StepBehavior::PendingOps(_)),
                }),
                Err(error) if error.kind == WorkflowErrorKind::Conflict => {
                    Ok(StepExecutionResult::Skipped)
                }
                Err(error) => Err(error),
            }
        }
        StepBehavior::Fatal(message) | StepBehavior::Abort(message) => {
            let request = CreateEventRequest::new(EventKind::StepFailed)
                .with_correlation_id(params.step_id.clone())
                .with_data(json!({
                    "stepName": params.step_name,
                    "error": { "name": "FatalError", "message": message },
                }));
            match world.events_create(
                &params.workflow_run_id,
                request,
                EventCreateOptions {
                    request_id: params.request_id,
                    v1_compat: false,
                },
            ) {
                Ok(_) => Ok(StepExecutionResult::Failed),
                Err(error) if error.kind == WorkflowErrorKind::Conflict => {
                    Ok(StepExecutionResult::Skipped)
                }
                Err(error) => Err(error),
            }
        }
        StepBehavior::Retryable {
            message,
            retry_after_seconds,
        } => {
            if step.attempt >= step_fn.max_retries + 1 {
                let wrapped = format!(
                    "Step \"{}\" failed after {} retries: {message}",
                    params.step_name, step_fn.max_retries
                );
                let request = CreateEventRequest::new(EventKind::StepFailed)
                    .with_correlation_id(params.step_id.clone())
                    .with_data(json!({
                        "stepName": params.step_name,
                        "error": {
                            "name": "FatalError",
                            "message": wrapped,
                            "cause": message,
                        },
                    }));
                match world.events_create(
                    &params.workflow_run_id,
                    request,
                    EventCreateOptions {
                        request_id: params.request_id,
                        v1_compat: false,
                    },
                ) {
                    Ok(_) => Ok(StepExecutionResult::Failed),
                    Err(error) if error.kind == WorkflowErrorKind::Conflict => {
                        Ok(StepExecutionResult::Skipped)
                    }
                    Err(error) => Err(error),
                }
            } else {
                let retry_after_ms = world
                    .now_ms()
                    .saturating_add(retry_after_seconds.saturating_mul(1000));
                let request = CreateEventRequest::new(EventKind::StepRetrying)
                    .with_correlation_id(params.step_id.clone())
                    .with_data(json!({
                        "stepName": params.step_name,
                        "error": { "name": "Error", "message": message },
                        "retryAfterMs": retry_after_ms,
                    }));
                match world.events_create(
                    &params.workflow_run_id,
                    request,
                    EventCreateOptions {
                        request_id: params.request_id,
                        v1_compat: false,
                    },
                ) {
                    Ok(_) => Ok(StepExecutionResult::Retry {
                        timeout_seconds: *retry_after_seconds,
                    }),
                    Err(error) if error.kind == WorkflowErrorKind::Conflict => {
                        Ok(StepExecutionResult::Skipped)
                    }
                    Err(error) => Err(error),
                }
            }
        }
    }
}

fn fail_step_for_max_retries<W: RuntimeWorld>(
    world: &mut W,
    params: &StepExecutorParams,
    max_retries: u32,
    previous_error: Option<Value>,
) -> WorkflowResult<StepExecutionResult> {
    let retry_count = max_retries + 1;
    let request = CreateEventRequest::new(EventKind::StepFailed)
        .with_correlation_id(params.step_id.clone())
        .with_data(json!({
            "stepName": params.step_name,
            "error": {
                "name": "FatalError",
                "message": format!("Step \"{}\" exceeded max retries ({retry_count} retries)", params.step_name),
                "cause": previous_error,
            },
        }));
    match world.events_create(
        &params.workflow_run_id,
        request,
        EventCreateOptions {
            request_id: params.request_id.clone(),
            v1_compat: false,
        },
    ) {
        Ok(_) => Ok(StepExecutionResult::Failed),
        Err(error) if error.kind == WorkflowErrorKind::Conflict => Ok(StepExecutionResult::Skipped),
        Err(error) => Err(error),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepHandlerMessage {
    pub workflow_run_id: String,
    pub workflow_name: String,
    pub workflow_started_at_ms: u64,
    pub step_id: String,
    pub step_name: String,
    pub attempt: u32,
    pub request_id: Option<String>,
}

pub fn handle_step_message<W: RuntimeWorld>(
    world: &mut W,
    registry: &StepRegistry,
    message: StepHandlerMessage,
) -> WorkflowResult<Option<StepExecutionResult>> {
    if message.attempt > MAX_QUEUE_DELIVERIES {
        let request = CreateEventRequest::new(EventKind::StepFailed)
            .with_correlation_id(message.step_id.clone())
            .with_data(json!({
                "stepName": message.step_name,
                "error": {
                    "name": "FatalError",
                    "message": format!("Step exceeded maximum queue deliveries ({}/{MAX_QUEUE_DELIVERIES})", message.attempt),
                },
            }));
        match world.events_create(
            &message.workflow_run_id,
            request,
            EventCreateOptions {
                request_id: message.request_id,
                v1_compat: false,
            },
        ) {
            Ok(_) => {
                world.queue(
                    &get_workflow_queue_name(&message.workflow_name)?,
                    QueueMessage {
                        run_id: message.workflow_run_id,
                        ..QueueMessage::default()
                    },
                    QueueOptions::default(),
                )?;
            }
            Err(error) if error.kind == WorkflowErrorKind::Conflict => {}
            Err(error) => return Err(error),
        }
        return Ok(None);
    }

    execute_step(
        world,
        registry,
        StepExecutorParams {
            workflow_run_id: message.workflow_run_id,
            workflow_name: message.workflow_name,
            workflow_started_at_ms: message.workflow_started_at_ms,
            step_id: message.step_id,
            step_name: message.step_name,
            request_id: message.request_id,
        },
    )
    .map(Some)
}

#[derive(Debug, Clone, PartialEq)]
pub enum SuspensionItem {
    Step {
        correlation_id: String,
        step_name: String,
        input: Value,
        has_created_event: bool,
    },
    Wait {
        correlation_id: String,
        resume_at_ms: u64,
        has_created_event: bool,
    },
    Hook {
        correlation_id: String,
        token: String,
        metadata: Value,
        has_created_event: bool,
        disposed: bool,
        abort_requested: bool,
        abort_reason: Option<String>,
        is_webhook: bool,
        is_system: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WorkflowSuspension {
    pub items: Vec<SuspensionItem>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SuspensionHandlerResult {
    pub pending_steps: Vec<SuspensionItem>,
    pub created_step_correlation_ids: HashSet<String>,
    pub timeout_seconds: Option<u64>,
    pub has_hook_conflict: bool,
}

pub fn handle_suspension<W: RuntimeWorld>(
    world: &mut W,
    run: &WorkflowRun,
    suspension: &WorkflowSuspension,
    request_id: Option<String>,
) -> WorkflowResult<SuspensionHandlerResult> {
    let mut has_hook_conflict = false;
    let mut created_step_correlation_ids = HashSet::new();
    let mut pending_steps = Vec::new();
    let mut wait_resume_times = Vec::new();

    for item in &suspension.items {
        if let SuspensionItem::Hook {
            correlation_id,
            token,
            metadata,
            has_created_event,
            disposed,
            abort_requested,
            abort_reason,
            is_webhook,
            is_system,
        } = item
        {
            if !has_created_event {
                let result = world.events_create(
                    &run.run_id,
                    CreateEventRequest::new(EventKind::HookCreated)
                        .with_correlation_id(correlation_id.clone())
                        .with_data(json!({
                            "token": token,
                            "metadata": metadata,
                            "isWebhook": is_webhook,
                            "isSystem": is_system,
                        })),
                    EventCreateOptions {
                        request_id: request_id.clone(),
                        v1_compat: false,
                    },
                )?;
                if result
                    .event
                    .as_ref()
                    .is_some_and(|event| event.event_type == EventKind::HookConflict)
                {
                    has_hook_conflict = true;
                }
            }
            if *disposed {
                match world.events_create(
                    &run.run_id,
                    CreateEventRequest::new(EventKind::HookDisposed)
                        .with_correlation_id(correlation_id.clone())
                        .with_data(json!({ "token": token })),
                    EventCreateOptions {
                        request_id: request_id.clone(),
                        v1_compat: false,
                    },
                ) {
                    Ok(_) => {}
                    Err(error)
                        if matches!(
                            error.kind,
                            WorkflowErrorKind::Conflict
                                | WorkflowErrorKind::RunExpired
                                | WorkflowErrorKind::NotFound
                        ) => {}
                    Err(error) => return Err(error),
                }
            }
            if *abort_requested && !disposed {
                let payload = json!({ "aborted": true, "reason": abort_reason });
                match world.events_create(
                    &run.run_id,
                    CreateEventRequest::new(EventKind::HookReceived)
                        .with_correlation_id(correlation_id.clone())
                        .with_data(json!({ "token": token, "payload": payload })),
                    EventCreateOptions {
                        request_id: request_id.clone(),
                        v1_compat: false,
                    },
                ) {
                    Ok(_) => {}
                    Err(error)
                        if matches!(
                            error.kind,
                            WorkflowErrorKind::Conflict | WorkflowErrorKind::RunExpired
                        ) => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }

    for item in &suspension.items {
        match item {
            SuspensionItem::Step {
                correlation_id,
                step_name,
                input,
                has_created_event,
            } => {
                pending_steps.push(item.clone());
                if !has_created_event {
                    match world.events_create(
                        &run.run_id,
                        CreateEventRequest::new(EventKind::StepCreated)
                            .with_correlation_id(correlation_id.clone())
                            .with_data(json!({
                                "stepName": step_name,
                                "input": input,
                            })),
                        EventCreateOptions {
                            request_id: request_id.clone(),
                            v1_compat: false,
                        },
                    ) {
                        Ok(_) => {
                            created_step_correlation_ids.insert(correlation_id.clone());
                        }
                        Err(error) if error.kind == WorkflowErrorKind::Conflict => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            SuspensionItem::Wait {
                correlation_id,
                resume_at_ms,
                has_created_event,
            } => {
                wait_resume_times.push(*resume_at_ms);
                if !has_created_event {
                    match world.events_create(
                        &run.run_id,
                        CreateEventRequest::new(EventKind::WaitCreated)
                            .with_correlation_id(correlation_id.clone())
                            .with_data(json!({ "resumeAt": resume_at_ms })),
                        EventCreateOptions {
                            request_id: request_id.clone(),
                            v1_compat: false,
                        },
                    ) {
                        Ok(_) => {}
                        Err(error) if error.kind == WorkflowErrorKind::Conflict => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            SuspensionItem::Hook { .. } => {}
        }
    }

    let timeout_seconds = if has_hook_conflict {
        Some(0)
    } else {
        wait_resume_times
            .into_iter()
            .map(|resume_at_ms| {
                let delay_ms = resume_at_ms.saturating_sub(world.now_ms()).max(1000);
                delay_ms.div_ceil(1000)
            })
            .min()
    };

    Ok(SuspensionHandlerResult {
        pending_steps,
        created_step_correlation_ids,
        timeout_seconds,
        has_hook_conflict,
    })
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WaitReplayRefresh {
    pub events: Vec<Event>,
    pub cursor: Option<String>,
    pub used_incremental: bool,
    pub used_full_reload: bool,
}

pub fn complete_elapsed_waits_and_refresh<W: RuntimeWorld>(
    world: &mut W,
    run_id: &str,
    events: Vec<Event>,
    cursor: Option<String>,
    now_ms: u64,
    request_id: Option<String>,
) -> WorkflowResult<WaitReplayRefresh> {
    let completed_waits: HashSet<Option<String>> = events
        .iter()
        .filter(|event| event.event_type == EventKind::WaitCompleted)
        .map(|event| event.correlation_id.clone())
        .collect();
    let waits_to_complete: Vec<Event> = events
        .iter()
        .filter(|event| {
            event.event_type == EventKind::WaitCreated
                && event.correlation_id.is_some()
                && !completed_waits.contains(&event.correlation_id)
                && event
                    .event_data
                    .get("resumeAt")
                    .and_then(Value::as_u64)
                    .is_some_and(|resume_at| now_ms >= resume_at)
        })
        .cloned()
        .collect();

    for wait in &waits_to_complete {
        match world.events_create(
            run_id,
            CreateEventRequest::new(EventKind::WaitCompleted)
                .with_correlation_id(wait.correlation_id.clone().unwrap_or_default())
                .with_data(json!({ "resumeAt": wait.event_data.get("resumeAt").cloned().unwrap_or(Value::Null) })),
            EventCreateOptions {
                request_id: request_id.clone(),
                v1_compat: false,
            },
        ) {
            Ok(_) => {}
            Err(error) if error.kind == WorkflowErrorKind::Conflict => {}
            Err(error) => return Err(error),
        }
    }

    if waits_to_complete.is_empty() {
        return Ok(WaitReplayRefresh {
            events,
            cursor,
            used_incremental: false,
            used_full_reload: false,
        });
    }

    if let Some(cursor) = cursor {
        let loaded = load_workflow_run_events(world, run_id, Some(cursor.clone()))?;
        let completed_after_cursor: HashSet<Option<String>> = loaded
            .events
            .iter()
            .filter(|event| event.event_type == EventKind::WaitCompleted)
            .map(|event| event.correlation_id.clone())
            .collect();
        let saw_all = waits_to_complete
            .iter()
            .all(|wait| completed_after_cursor.contains(&wait.correlation_id));
        if saw_all {
            let mut merged = events;
            let mut seen: HashSet<String> =
                merged.iter().map(|event| event.event_id.clone()).collect();
            for event in loaded.events {
                if seen.insert(event.event_id.clone()) {
                    merged.push(event);
                }
            }
            return Ok(WaitReplayRefresh {
                events: merged,
                cursor: loaded.cursor.or(Some(cursor)),
                used_incremental: true,
                used_full_reload: false,
            });
        }
    }

    let loaded = load_workflow_run_events(world, run_id, None)?;
    Ok(WaitReplayRefresh {
        events: loaded.events,
        cursor: loaded.cursor,
        used_incremental: false,
        used_full_reload: true,
    })
}

pub fn validate_replay_event_log(events: &[Event]) -> WorkflowResult<()> {
    let mut waits: HashMap<&str, &Value> = HashMap::new();
    let mut hooks: HashMap<&str, &str> = HashMap::new();
    for event in events {
        match event.event_type {
            EventKind::WaitCreated => {
                if let Some(correlation_id) = event.correlation_id.as_deref() {
                    waits.insert(correlation_id, &event.event_data["resumeAt"]);
                }
            }
            EventKind::WaitCompleted => {
                if let Some(correlation_id) = event.correlation_id.as_deref() {
                    let expected = waits.get(correlation_id).ok_or_else(|| {
                        WorkflowCoreError::runtime("wait_completed without wait_created")
                            .with_code(RunErrorCode::CorruptedEventLog)
                    })?;
                    if *expected != &event.event_data["resumeAt"] {
                        return Err(WorkflowCoreError::runtime(
                            "wait_completed resumeAt does not match wait_created",
                        )
                        .with_code(RunErrorCode::CorruptedEventLog));
                    }
                }
            }
            EventKind::HookCreated => {
                if let (Some(correlation_id), Some(token)) = (
                    event.correlation_id.as_deref(),
                    event.event_data.get("token").and_then(Value::as_str),
                ) {
                    hooks.insert(correlation_id, token);
                }
            }
            EventKind::HookReceived => {
                if let (Some(correlation_id), Some(token)) = (
                    event.correlation_id.as_deref(),
                    event.event_data.get("token").and_then(Value::as_str),
                ) {
                    if hooks
                        .get(correlation_id)
                        .is_some_and(|expected| *expected != token)
                        || !hooks.contains_key(correlation_id)
                    {
                        return Err(WorkflowCoreError::runtime(
                            "hook_received token does not match hook_created",
                        )
                        .with_code(RunErrorCode::CorruptedEventLog));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn record_run_failure<W: RuntimeWorld>(
    world: &mut W,
    run_id: &str,
    request_id: Option<String>,
    code: RunErrorCode,
    message: impl Into<String>,
) -> WorkflowResult<()> {
    let request = CreateEventRequest::new(EventKind::RunFailed).with_data(json!({
        "error": {
            "name": "FatalError",
            "message": message.into(),
        },
        "errorCode": code.as_str(),
    }));
    world
        .events_create(
            run_id,
            request,
            EventCreateOptions {
                request_id,
                v1_compat: false,
            },
        )
        .map(|_| ())
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorldRegistry {
    registered_world: Option<String>,
    fallback_imports: u32,
}

impl WorldRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_get_world(&mut self, world_id: impl Into<String>) {
        if self.registered_world.is_none() {
            self.registered_world = Some(world_id.into());
        }
    }

    pub fn set_world(&mut self, world_id: Option<String>) {
        self.registered_world = world_id;
    }

    pub fn get_world_lazy(&mut self) -> String {
        if let Some(world) = &self.registered_world {
            return world.clone();
        }
        self.fallback_imports += 1;
        "dynamic-world".to_string()
    }

    pub fn fallback_imports(&self) -> u32 {
        self.fallback_imports
    }
}
