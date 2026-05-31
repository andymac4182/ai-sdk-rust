//! Postgres World implementation contracts for the standalone Workflow SDK port.
//!
//! This crate maps to upstream `packages/world-postgres`. The implementation
//! is deterministic by default: it models SQL migration metadata, Graphile job
//! planning, event-sourced storage semantics, and stream pagination without
//! requiring a live Postgres container in normal tests.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::fmt;

use base64::Engine;
use serde_json::Value;
use workflow_world::{
    Event, EventType, Headers, PaginatedResponse, Pagination, QueueOptions, QueuePayload,
    QueueRoute, SPEC_VERSION_CURRENT, STEP_QUEUE_PREFIX, Step, StepStatus, WORKFLOW_QUEUE_PREFIX,
    WorkflowRun, WorkflowRunStatus, queue_route, split_queue_name,
};

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for this implementation slice.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/world-postgres";

/// Upstream package version inventoried for this implementation slice.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.9";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

const DEFAULT_JOB_PREFIX: &str = "workflow_";
const COMPLETED_IDEMPOTENCY_CACHE_LIMIT: usize = 10_000;

/// General Postgres world error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PostgresWorldError {
    message: String,
}

impl PostgresWorldError {
    /// Create a new error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PostgresWorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for PostgresWorldError {}

/// Postgres world configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PostgresWorldConfig {
    pub connection_string: Option<String>,
    pub has_pool: bool,
    pub job_prefix: Option<String>,
    pub queue_concurrency: Option<u32>,
    pub stream_flush_interval_ms: Option<u64>,
}

impl PostgresWorldConfig {
    /// Validate that exactly one connection source exists.
    pub fn validate(&self) -> Result<(), PostgresWorldError> {
        match (&self.connection_string, self.has_pool) {
            (Some(_), false) | (None, true) => Ok(()),
            (Some(_), true) => Err(PostgresWorldError::new(
                "connectionString and pool are mutually exclusive",
            )),
            (None, false) => Err(PostgresWorldError::new(
                "connectionString or pool is required",
            )),
        }
    }

    /// Graphile job names for workflow and step queues.
    pub fn graphile_job_names(&self) -> GraphileJobNames {
        let prefix = self.job_prefix.as_deref().unwrap_or(DEFAULT_JOB_PREFIX);
        GraphileJobNames {
            workflow_flows: format!("{prefix}flows"),
            workflow_steps: format!("{prefix}steps"),
        }
    }
}

/// Graphile job names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphileJobNames {
    pub workflow_flows: String,
    pub workflow_steps: String,
}

/// SQL migration metadata entry from the upstream Drizzle journal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Migration {
    pub idx: u8,
    pub tag: &'static str,
    pub when: u64,
    pub breakpoints: bool,
}

/// Ordered upstream migration journal entries.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        idx: 0,
        tag: "0000_cultured_the_anarchist",
        when: 1_762_873_019_948,
        breakpoints: true,
    },
    Migration {
        idx: 1,
        tag: "0001_tricky_sersi",
        when: 1_763_903_867_386,
        breakpoints: true,
    },
    Migration {
        idx: 2,
        tag: "0002_add_expired_at",
        when: 1_764_815_363_008,
        breakpoints: true,
    },
    Migration {
        idx: 3,
        tag: "0003_add_stream_run_id",
        when: 1_765_900_000_000,
        breakpoints: true,
    },
    Migration {
        idx: 4,
        tag: "0004_remove_run_pause_status",
        when: 1_765_900_000_001,
        breakpoints: true,
    },
    Migration {
        idx: 5,
        tag: "0005_add_spec_version",
        when: 1_767_723_210_726,
        breakpoints: true,
    },
    Migration {
        idx: 6,
        tag: "0006_add_error_cbor",
        when: 1_768_500_000_000,
        breakpoints: true,
    },
    Migration {
        idx: 7,
        tag: "0007_add_waits_table",
        when: 1_769_500_000_000,
        breakpoints: true,
    },
    Migration {
        idx: 8,
        tag: "0008_migrate_pgboss_to_graphile",
        when: 1_770_000_000_000,
        breakpoints: true,
    },
    Migration {
        idx: 9,
        tag: "0009_add_is_webhook",
        when: 1_770_500_000_000,
        breakpoints: true,
    },
    Migration {
        idx: 10,
        tag: "0010_add_events_entity_creation_unique_index",
        when: 1_771_000_000_000,
        breakpoints: true,
    },
    Migration {
        idx: 11,
        tag: "0011_add_error_code",
        when: 1_771_500_000_000,
        breakpoints: true,
    },
    Migration {
        idx: 12,
        tag: "0012_add_is_system",
        when: 1_775_600_000_000,
        breakpoints: true,
    },
    Migration {
        idx: 13,
        tag: "0013_add_attributes",
        when: 1_779_609_600_000,
        breakpoints: true,
    },
];

/// Current table names created by the upstream schema.
pub const TABLES: &[&str] = &[
    "workflow.workflow_runs",
    "workflow.workflow_steps",
    "workflow.workflow_events",
    "workflow.workflow_hooks",
    "workflow.workflow_waits",
    "workflow.workflow_stream_chunks",
];

/// Current partial/secondary index names from the upstream schema.
pub const INDEXES: &[&str] = &[
    "workflow_events_run_id_index",
    "workflow_events_correlation_id_index",
    "workflow_events_entity_creation_unique",
    "workflow_hooks_run_id_index",
    "workflow_hooks_token_index",
    "workflow_runs_name_index",
    "workflow_runs_status_index",
    "workflow_steps_run_id_index",
    "workflow_steps_status_index",
    "workflow_waits_run_id_index",
    "workflow_stream_chunks_run_id_index",
];

/// SQL-shape snippets that must stay true for the portable migration contract.
pub const SQL_SHAPE_SNIPPETS: &[&str] = &[
    r#"CREATE SCHEMA "workflow""#,
    r#"CREATE TABLE IF NOT EXISTS "workflow"."workflow_runs""#,
    r#"ADD COLUMN "payload_cbor" "bytea""#,
    r#"ADD COLUMN "expired_at" timestamp"#,
    r#"ADD COLUMN "run_id" varchar"#,
    r#"DROP TYPE "public"."status""#,
    r#"ADD COLUMN "spec_version" integer"#,
    r#"CREATE TABLE IF NOT EXISTS "workflow"."workflow_waits""#,
    r#"CREATE TABLE IF NOT EXISTS "workflow"."_pgboss_pending_jobs""#,
    r#"ADD COLUMN "is_webhook" boolean DEFAULT true"#,
    r#"CREATE UNIQUE INDEX IF NOT EXISTS "workflow_events_entity_creation_unique""#,
    r#"ADD COLUMN "error_code" varchar"#,
    r#"ADD COLUMN "is_system" boolean DEFAULT false"#,
    r#"ADD COLUMN "attributes" jsonb DEFAULT '{}'::jsonb NOT NULL"#,
];

/// Environment needed to resolve local HTTP queue execution.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QueueEnv {
    pub workflow_local_base_url: Option<String>,
    pub port: Option<u16>,
    pub detected_port: Option<u16>,
}

/// Resolve the HTTP base URL used by the Graphile task handler.
pub fn resolve_execution_base_url(env: &QueueEnv) -> Result<String, PostgresWorldError> {
    if let Some(url) = &env.workflow_local_base_url {
        return Ok(url.clone());
    }
    if let Some(port) = env.port {
        return Ok(format!("http://localhost:{port}"));
    }
    if let Some(port) = env.detected_port {
        return Ok(format!("http://localhost:{port}"));
    }
    Err(PostgresWorldError::new(
        "Unable to resolve base URL for workflow queue.",
    ))
}

/// Planned HTTP execution request for a Graphile job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpExecutionRequest {
    pub url: String,
    pub method: &'static str,
    pub headers: Headers,
    pub body: Vec<u8>,
}

/// Build the HTTP execution request for a queued message.
pub fn http_execution_request(
    env: &QueueEnv,
    queue_name: &str,
    message_id: &str,
    attempt: u32,
    body: Vec<u8>,
    extra_headers: Headers,
) -> Result<HttpExecutionRequest, PostgresWorldError> {
    let base_url = resolve_execution_base_url(env)?;
    let route = match queue_route(queue_name) {
        Some(QueueRoute::Flow) => "flow",
        Some(QueueRoute::Step) => "step",
        None => return Err(PostgresWorldError::new("Unknown queue name prefix")),
    };
    let mut headers = extra_headers;
    headers.insert("content-type".into(), "application/json".into());
    headers.insert("x-vqs-queue-name".into(), queue_name.into());
    headers.insert("x-vqs-message-id".into(), message_id.into());
    headers.insert("x-vqs-message-attempt".into(), attempt.to_string());
    Ok(HttpExecutionRequest {
        url: format!(
            "{}/.well-known/workflow/v1/{route}",
            base_url.trim_end_matches('/')
        ),
        method: "POST",
        headers,
        body,
    })
}

/// Graphile addJob contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphileJobPlan {
    pub job_name: String,
    pub queue_id: String,
    pub message_id: String,
    pub attempt: u32,
    pub idempotency_key: Option<String>,
    pub headers: Headers,
    pub delay_seconds: Option<u64>,
    pub job_key: String,
    pub max_attempts: u32,
    pub body: Vec<u8>,
}

/// Plan the Graphile job emitted by `queue.queue`.
pub fn graphile_job_plan(
    config: &PostgresWorldConfig,
    queue_name: &str,
    payload: QueuePayload,
    options: QueueOptions,
    message_id: impl Into<String>,
) -> Result<GraphileJobPlan, PostgresWorldError> {
    let (prefix, queue_id) = split_queue_name(queue_name)
        .ok_or_else(|| PostgresWorldError::new(format!("Invalid queue name: {queue_name}")))?;
    let names = config.graphile_job_names();
    let message_id = message_id.into();
    let job_name = match prefix {
        WORKFLOW_QUEUE_PREFIX => names.workflow_flows,
        STEP_QUEUE_PREFIX => names.workflow_steps,
        _ => unreachable!("split_queue_name only returns known prefixes"),
    };
    let mut headers = payload.workflow_headers();
    headers.extend(options.headers);
    let body = encode_payload_json(&payload);
    Ok(GraphileJobPlan {
        job_name,
        queue_id: queue_id.into(),
        message_id: message_id.clone(),
        attempt: 1,
        idempotency_key: options.idempotency_key.clone(),
        headers,
        delay_seconds: options.delay_seconds,
        job_key: options.idempotency_key.unwrap_or(message_id),
        max_attempts: 3,
        body,
    })
}

/// A bounded completed-idempotency cache matching upstream semantics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompletedIdempotencyCache {
    order: Vec<String>,
    set: HashSet<String>,
    limit: usize,
}

impl CompletedIdempotencyCache {
    /// Create a cache with the upstream default limit.
    pub fn new() -> Self {
        Self {
            order: Vec::new(),
            set: HashSet::new(),
            limit: COMPLETED_IDEMPOTENCY_CACHE_LIMIT,
        }
    }

    /// Create a cache with a smaller limit for tests.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            order: Vec::new(),
            set: HashSet::new(),
            limit,
        }
    }

    /// Mark an idempotency key completed.
    pub fn mark_completed(&mut self, key: impl Into<String>) {
        let key = key.into();
        self.set.remove(&key);
        self.order.retain(|existing| existing != &key);
        self.set.insert(key.clone());
        self.order.push(key);
        while self.order.len() > self.limit {
            if let Some(oldest) = self.order.first().cloned() {
                self.order.remove(0);
                self.set.remove(&oldest);
            }
        }
    }

    /// Whether the key has completed.
    pub fn contains(&self, key: &str) -> bool {
        self.set.contains(key)
    }
}

/// Active run summary used by re-enqueue startup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveRun {
    pub run_id: String,
    pub workflow_name: String,
}

/// Re-enqueue all active runs in pending and running pages.
pub fn reenqueue_active_runs(
    config: &PostgresWorldConfig,
    pending_pages: &[Vec<ActiveRun>],
    running_pages: &[Vec<ActiveRun>],
) -> Vec<GraphileJobPlan> {
    let mut jobs = Vec::new();
    for run in pending_pages
        .iter()
        .chain(running_pages.iter())
        .flat_map(|page| page.iter())
    {
        let queue_name = format!("{WORKFLOW_QUEUE_PREFIX}{}", run.workflow_name);
        let payload = QueuePayload::Workflow {
            run_id: run.run_id.clone(),
        };
        let plan = graphile_job_plan(
            config,
            &queue_name,
            payload,
            QueueOptions {
                idempotency_key: Some(run.run_id.clone()),
                ..QueueOptions::default()
            },
            format!("msg_reenqueue_{}", run.run_id),
        )
        .expect("generated workflow queue names are valid");
        jobs.push(plan);
    }
    jobs
}

/// Event input accepted by [`InMemoryPostgresStorage::create_event`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventInput {
    pub event_type: EventType,
    pub correlation_id: Option<String>,
    pub step_id: Option<String>,
    pub step_name: Option<String>,
    pub data: Option<Vec<u8>>,
    pub error_code: Option<String>,
}

/// Deterministic in-memory model of Postgres event-sourced storage.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InMemoryPostgresStorage {
    runs: HashMap<String, WorkflowRun>,
    steps: HashMap<(String, String), Step>,
    events: Vec<Event>,
    hook_tokens: HashMap<String, String>,
    disposed_hook_tokens: HashSet<String>,
    next_event: usize,
}

impl InMemoryPostgresStorage {
    /// Create empty storage.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a run directly, used by deterministic tests and live-test setup.
    pub fn create_run(
        &mut self,
        run_id: impl Into<String>,
        workflow_name: impl Into<String>,
        input: Option<Vec<u8>>,
    ) -> WorkflowRun {
        let run_id = run_id.into();
        let run = WorkflowRun {
            run_id: run_id.clone(),
            workflow_name: workflow_name.into(),
            deployment_id: "postgres".into(),
            status: WorkflowRunStatus::Pending,
            input,
            output: None,
            error: None,
            error_code: None,
            attributes: Headers::new(),
            spec_version: SPEC_VERSION_CURRENT,
        };
        self.runs.insert(run_id, run.clone());
        run
    }

    /// Get a run by id.
    pub fn get_run(&self, run_id: &str) -> Result<WorkflowRun, PostgresWorldError> {
        self.runs
            .get(run_id)
            .cloned()
            .ok_or_else(|| PostgresWorldError::new(format!("Workflow run not found: {run_id}")))
    }

    /// List runs with optional filters and pagination.
    pub fn list_runs(
        &self,
        workflow_name: Option<&str>,
        status: Option<WorkflowRunStatus>,
        pagination: Pagination,
    ) -> PaginatedResponse<WorkflowRun> {
        let mut runs = self
            .runs
            .values()
            .filter(|run| workflow_name.is_none_or(|name| run.workflow_name == name))
            .filter(|run| status.is_none_or(|status| run.status == status))
            .cloned()
            .collect::<Vec<_>>();
        runs.sort_by(|left, right| right.run_id.cmp(&left.run_id));
        paginate(runs, pagination, |run| run.run_id.clone())
    }

    /// Apply setAttributes changes.
    pub fn set_attributes(
        &mut self,
        run_id: &str,
        changes: impl IntoIterator<Item = (String, Option<String>)>,
    ) -> Result<Headers, PostgresWorldError> {
        let run = self
            .runs
            .get_mut(run_id)
            .ok_or_else(|| PostgresWorldError::new(format!("Workflow run not found: {run_id}")))?;
        for (key, value) in changes {
            if let Some(value) = value {
                run.attributes.insert(key, value);
            } else {
                run.attributes.remove(&key);
            }
        }
        Ok(run.attributes.clone())
    }

    /// Create a step directly.
    pub fn create_step(
        &mut self,
        run_id: impl Into<String>,
        step_id: impl Into<String>,
        step_name: impl Into<String>,
        input: Option<Vec<u8>>,
    ) -> Result<Step, PostgresWorldError> {
        let run_id = run_id.into();
        let run = self.get_run(&run_id)?;
        if run.status.is_terminal() {
            return Err(PostgresWorldError::new(
                "Cannot create step for terminal workflow run",
            ));
        }
        let step_id = step_id.into();
        let step = Step {
            run_id: run_id.clone(),
            step_id: step_id.clone(),
            step_name: step_name.into(),
            status: StepStatus::Pending,
            input,
            output: None,
            error: None,
            attempt: 0,
            spec_version: SPEC_VERSION_CURRENT,
        };
        self.steps.insert((run_id, step_id), step.clone());
        Ok(step)
    }

    /// Get a step by run and step id.
    pub fn get_step(&self, run_id: &str, step_id: &str) -> Result<Step, PostgresWorldError> {
        self.steps
            .get(&(run_id.into(), step_id.into()))
            .cloned()
            .ok_or_else(|| PostgresWorldError::new(format!("Step not found: {step_id}")))
    }

    /// List steps for a run.
    pub fn list_steps(&self, run_id: &str, pagination: Pagination) -> PaginatedResponse<Step> {
        let mut steps = self
            .steps
            .values()
            .filter(|step| step.run_id == run_id)
            .cloned()
            .collect::<Vec<_>>();
        steps.sort_by(|left, right| left.step_id.cmp(&right.step_id));
        paginate(steps, pagination, |step| step.step_id.clone())
    }

    /// Create an event and mutate matching run/step/hook state.
    pub fn create_event(
        &mut self,
        run_id: &str,
        input: EventInput,
    ) -> Result<Event, PostgresWorldError> {
        self.validate_event(run_id, &input)?;

        let event = Event {
            event_id: format!("wevt_{:06}", self.next_event),
            run_id: run_id.into(),
            event_type: input.event_type,
            correlation_id: input.correlation_id.clone(),
            event_data: input.data.clone(),
            spec_version: SPEC_VERSION_CURRENT,
        };
        self.next_event += 1;
        self.events.push(event.clone());
        self.apply_event(run_id, input)?;
        Ok(event)
    }

    /// List all events for a run.
    pub fn list_events(&self, run_id: &str, pagination: Pagination) -> PaginatedResponse<Event> {
        let mut events = self
            .events
            .iter()
            .filter(|event| event.run_id == run_id)
            .cloned()
            .collect::<Vec<_>>();
        sort_events(&mut events, pagination.sort_order);
        paginate(events, pagination, |event| event.event_id.clone())
    }

    /// List events by correlation id across runs.
    pub fn list_events_by_correlation_id(
        &self,
        correlation_id: &str,
        pagination: Pagination,
    ) -> PaginatedResponse<Event> {
        let mut events = self
            .events
            .iter()
            .filter(|event| event.correlation_id.as_deref() == Some(correlation_id))
            .cloned()
            .collect::<Vec<_>>();
        sort_events(&mut events, pagination.sort_order);
        paginate(events, pagination, |event| event.event_id.clone())
    }

    fn validate_event(&self, run_id: &str, input: &EventInput) -> Result<(), PostgresWorldError> {
        if let Some(correlation_id) = &input.correlation_id {
            if matches!(
                input.event_type,
                EventType::StepCreated | EventType::HookCreated | EventType::WaitCreated
            ) && self.events.iter().any(|event| {
                event.run_id == run_id
                    && event.correlation_id.as_ref() == Some(correlation_id)
                    && event.event_type == input.event_type
            }) {
                return Err(PostgresWorldError::new("EntityConflictError"));
            }
        }

        match input.event_type {
            EventType::RunStarted | EventType::RunCompleted | EventType::RunFailed => {
                let run = self.get_run(run_id)?;
                if run.status.is_terminal() {
                    return Err(PostgresWorldError::new(
                        "Cannot mutate terminal workflow run",
                    ));
                }
            }
            EventType::RunCancelled => {
                let run = self.get_run(run_id)?;
                if matches!(
                    run.status,
                    WorkflowRunStatus::Completed | WorkflowRunStatus::Failed
                ) {
                    return Err(PostgresWorldError::new(
                        "Cannot cancel completed or failed workflow run",
                    ));
                }
            }
            EventType::StepCreated | EventType::HookCreated => {
                let run = self.get_run(run_id)?;
                if run.status.is_terminal() {
                    return Err(PostgresWorldError::new(
                        "Cannot create child entity for terminal workflow run",
                    ));
                }
            }
            EventType::StepStarted
            | EventType::StepCompleted
            | EventType::StepFailed
            | EventType::StepRetrying => {
                let step_id = input
                    .step_id
                    .as_deref()
                    .ok_or_else(|| PostgresWorldError::new("stepId is required"))?;
                let step = self.get_step(run_id, step_id)?;
                if step.status.is_terminal()
                    || (input.event_type == EventType::StepRetrying
                        && step.status == StepStatus::Completed)
                {
                    return Err(PostgresWorldError::new("Cannot mutate terminal step"));
                }
                let run = self.get_run(run_id)?;
                if run.status.is_terminal() && step.status == StepStatus::Pending {
                    return Err(PostgresWorldError::new(
                        "Cannot start pending step after terminal workflow run",
                    ));
                }
            }
            EventType::HookDisposed | EventType::HookReceived => {
                let token = input
                    .data
                    .as_ref()
                    .and_then(|data| String::from_utf8(data.clone()).ok())
                    .unwrap_or_default();
                if !token.is_empty() && !self.hook_tokens.contains_key(&token) {
                    return Err(PostgresWorldError::new("Hook not found"));
                }
            }
            EventType::WaitCompleted => {}
            EventType::RunCreated | EventType::WaitCreated => {}
        }

        if input.event_type == EventType::HookCreated {
            let token = input
                .data
                .as_ref()
                .and_then(|data| String::from_utf8(data.clone()).ok())
                .unwrap_or_default();
            if !token.is_empty()
                && self.hook_tokens.contains_key(&token)
                && !self.disposed_hook_tokens.contains(&token)
            {
                return Err(PostgresWorldError::new("EntityConflictError"));
            }
        }

        Ok(())
    }

    fn apply_event(&mut self, run_id: &str, input: EventInput) -> Result<(), PostgresWorldError> {
        match input.event_type {
            EventType::RunCreated => {
                if !self.runs.contains_key(run_id) {
                    self.create_run(
                        run_id,
                        input.step_name.unwrap_or_else(|| "workflow".into()),
                        input.data,
                    );
                }
            }
            EventType::RunStarted => {
                self.runs.get_mut(run_id).expect("validated").status = WorkflowRunStatus::Running;
            }
            EventType::RunCompleted => {
                let run = self.runs.get_mut(run_id).expect("validated");
                run.status = WorkflowRunStatus::Completed;
                run.output = input.data;
                self.hook_tokens.retain(|_, owner| owner != run_id);
            }
            EventType::RunFailed => {
                let run = self.runs.get_mut(run_id).expect("validated");
                run.status = WorkflowRunStatus::Failed;
                run.error = input.data;
                run.error_code = input.error_code;
            }
            EventType::RunCancelled => {
                let run = self.runs.get_mut(run_id).expect("validated");
                run.status = WorkflowRunStatus::Cancelled;
                self.hook_tokens.retain(|_, owner| owner != run_id);
            }
            EventType::StepCreated => {
                let step_id = input
                    .step_id
                    .clone()
                    .unwrap_or_else(|| input.correlation_id.unwrap_or_else(|| "step".into()));
                self.create_step(
                    run_id,
                    step_id,
                    input.step_name.unwrap_or_else(|| "step".into()),
                    input.data,
                )?;
            }
            EventType::StepStarted => {
                let step = self
                    .steps
                    .get_mut(&(run_id.into(), input.step_id.expect("validated")));
                if let Some(step) = step {
                    step.status = StepStatus::Running;
                    step.attempt += 1;
                }
            }
            EventType::StepCompleted => {
                let step = self
                    .steps
                    .get_mut(&(run_id.into(), input.step_id.expect("validated")));
                if let Some(step) = step {
                    step.status = StepStatus::Completed;
                    step.output = input.data;
                }
            }
            EventType::StepFailed => {
                let step = self
                    .steps
                    .get_mut(&(run_id.into(), input.step_id.expect("validated")));
                if let Some(step) = step {
                    step.status = StepStatus::Failed;
                    step.error = input.data;
                }
            }
            EventType::StepRetrying => {
                let step = self
                    .steps
                    .get_mut(&(run_id.into(), input.step_id.expect("validated")));
                if let Some(step) = step {
                    step.status = StepStatus::Pending;
                    step.error = input.data;
                    step.attempt += 1;
                }
            }
            EventType::HookCreated => {
                if let Some(data) = input.data {
                    if let Ok(token) = String::from_utf8(data) {
                        self.disposed_hook_tokens.remove(&token);
                        self.hook_tokens.insert(token, run_id.into());
                    }
                }
            }
            EventType::HookDisposed => {
                if let Some(data) = input.data {
                    if let Ok(token) = String::from_utf8(data) {
                        self.hook_tokens.remove(&token);
                        self.disposed_hook_tokens.insert(token);
                    }
                }
            }
            EventType::HookReceived | EventType::WaitCreated | EventType::WaitCompleted => {}
        }
        Ok(())
    }
}

/// Stream chunk row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamChunkRow {
    pub index: usize,
    pub data: Vec<u8>,
}

/// Stream chunk response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamChunksResponse {
    pub data: Vec<StreamChunkRow>,
    pub cursor: Option<String>,
    pub has_more: bool,
    pub done: bool,
}

/// Stream info response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamInfoResponse {
    pub tail_index: isize,
    pub done: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StoredChunk {
    chunk_id: String,
    run_id: String,
    stream_id: String,
    data: Vec<u8>,
    eof: bool,
}

/// Deterministic in-memory model of the Postgres streamer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InMemoryPostgresStreamer {
    chunks: Vec<StoredChunk>,
    next_chunk: usize,
}

impl InMemoryPostgresStreamer {
    /// Create an empty streamer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Write one chunk.
    pub fn write(&mut self, run_id: &str, name: &str, chunk: impl Into<Vec<u8>>) {
        self.push(run_id, name, chunk.into(), false);
    }

    /// Write many chunks, preserving order.
    pub fn write_multi(
        &mut self,
        run_id: &str,
        name: &str,
        chunks: impl IntoIterator<Item = Vec<u8>>,
    ) {
        for chunk in chunks {
            self.write(run_id, name, chunk);
        }
    }

    /// Close a stream with an EOF row.
    pub fn close(&mut self, run_id: &str, name: &str) {
        self.push(run_id, name, Vec::new(), true);
    }

    /// Get paginated chunks.
    pub fn get_chunks(
        &self,
        _run_id: &str,
        name: &str,
        limit: Option<usize>,
        cursor: Option<&str>,
    ) -> StreamChunksResponse {
        let limit = limit.unwrap_or(100);
        let (cursor_chunk_id, base_index) = decode_cursor(cursor).unwrap_or((None, 0));
        let mut rows = self
            .chunks
            .iter()
            .filter(|chunk| chunk.stream_id == name && !chunk.eof)
            .filter(|chunk| {
                cursor_chunk_id
                    .as_ref()
                    .is_none_or(|cursor| chunk.chunk_id > *cursor)
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.chunk_id.cmp(&right.chunk_id));
        let has_more = rows.len() > limit;
        let page = rows.into_iter().take(limit).collect::<Vec<_>>();
        let data = page
            .iter()
            .enumerate()
            .map(|(index, chunk)| StreamChunkRow {
                index: base_index + index,
                data: chunk.data.clone(),
            })
            .collect::<Vec<_>>();
        let cursor = if has_more {
            page.last()
                .map(|chunk| encode_cursor(&chunk.chunk_id, base_index + page.len()))
        } else {
            None
        };
        let done = self
            .chunks
            .iter()
            .any(|chunk| chunk.stream_id == name && chunk.eof);
        StreamChunksResponse {
            data,
            cursor,
            has_more,
            done,
        }
    }

    /// Get stream tail info.
    pub fn get_info(&self, _run_id: &str, name: &str) -> StreamInfoResponse {
        let count = self
            .chunks
            .iter()
            .filter(|chunk| chunk.stream_id == name && !chunk.eof)
            .count();
        let done = self
            .chunks
            .iter()
            .any(|chunk| chunk.stream_id == name && chunk.eof);
        StreamInfoResponse {
            tail_index: count as isize - 1,
            done,
        }
    }

    /// List stream ids for a run.
    pub fn list(&self, run_id: &str) -> Vec<String> {
        let mut ids = self
            .chunks
            .iter()
            .filter(|chunk| chunk.run_id == run_id)
            .map(|chunk| chunk.stream_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }

    fn push(&mut self, run_id: &str, name: &str, data: Vec<u8>, eof: bool) {
        let chunk_id = format!("chnk_{:010}", self.next_chunk);
        self.next_chunk += 1;
        self.chunks.push(StoredChunk {
            chunk_id,
            run_id: run_id.into(),
            stream_id: name.into(),
            data,
            eof,
        });
    }
}

/// Convert JSON-like values by removing nulls and preserving other values.
pub fn compact_json_nulls(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter_map(|(key, value)| {
                    if value.is_null() {
                        None
                    } else {
                        Some((key.clone(), value.clone()))
                    }
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

fn encode_payload_json(payload: &QueuePayload) -> Vec<u8> {
    let value = match payload {
        QueuePayload::Workflow { run_id } => serde_json::json!({ "runId": run_id }),
        QueuePayload::Step {
            workflow_name,
            workflow_run_id,
            workflow_started_at_ms,
            step_id,
        } => serde_json::json!({
            "workflowName": workflow_name,
            "workflowRunId": workflow_run_id,
            "workflowStartedAt": workflow_started_at_ms,
            "stepId": step_id,
        }),
        QueuePayload::HealthCheck { correlation_id } => {
            serde_json::json!({ "__healthCheck": true, "correlationId": correlation_id })
        }
    };
    serde_json::to_vec(&value).expect("json object serializes")
}

fn sort_events(events: &mut [Event], order: workflow_world::SortOrder) {
    match order {
        workflow_world::SortOrder::Asc => {
            events.sort_by(|left, right| left.event_id.cmp(&right.event_id));
        }
        workflow_world::SortOrder::Desc => {
            events.sort_by(|left, right| right.event_id.cmp(&left.event_id));
        }
    }
}

fn paginate<T: Clone>(
    values: Vec<T>,
    pagination: Pagination,
    cursor_for: impl Fn(&T) -> String,
) -> PaginatedResponse<T> {
    let from = pagination
        .cursor
        .as_ref()
        .and_then(|cursor| values.iter().position(|value| cursor_for(value) == *cursor))
        .map(|index| index + 1)
        .unwrap_or(0);
    let limit = pagination.limit.unwrap_or(20);
    let remaining = values.into_iter().skip(from).collect::<Vec<_>>();
    let has_more = remaining.len() > limit;
    let data = remaining.into_iter().take(limit).collect::<Vec<_>>();
    let cursor = data.last().map(cursor_for);
    PaginatedResponse {
        data,
        has_more,
        cursor,
    }
}

fn encode_cursor(chunk_id: &str, index: usize) -> String {
    let json = serde_json::json!({ "c": chunk_id, "i": index });
    base64::engine::general_purpose::STANDARD.encode(json.to_string())
}

fn decode_cursor(cursor: Option<&str>) -> Option<(Option<String>, usize)> {
    let cursor = cursor?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cursor)
        .ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let chunk_id = value.get("c").and_then(Value::as_str).map(str::to_string);
    let index = value.get("i").and_then(Value::as_u64).unwrap_or(0) as usize;
    Some((chunk_id, index))
}

/// Build a header map for tests and callers.
pub fn headers(
    entries: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
) -> Headers {
    entries
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect::<BTreeMap<_, _>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use workflow_world::{STEP_QUEUE_PREFIX, SortOrder, WORKFLOW_QUEUE_PREFIX};

    macro_rules! parity_test {
        ($name:ident, $helper:ident) => {
            #[test]
            fn $name() {
                $helper();
            }
        };
    }

    fn config() -> PostgresWorldConfig {
        PostgresWorldConfig {
            connection_string: Some("postgres://test".into()),
            ..PostgresWorldConfig::default()
        }
    }

    fn workflow_payload(run_id: &str) -> QueuePayload {
        QueuePayload::Workflow {
            run_id: run_id.into(),
        }
    }

    fn step_payload(run_id: &str, step_id: &str) -> QueuePayload {
        QueuePayload::Step {
            workflow_name: "test-workflow".into(),
            workflow_run_id: run_id.into(),
            workflow_started_at_ms: 1,
            step_id: step_id.into(),
        }
    }

    fn event(event_type: EventType) -> EventInput {
        EventInput {
            event_type,
            correlation_id: None,
            step_id: None,
            step_name: None,
            data: None,
            error_code: None,
        }
    }

    fn step_event(event_type: EventType, step_id: &str) -> EventInput {
        EventInput {
            step_id: Some(step_id.into()),
            ..event(event_type)
        }
    }

    fn assert_queue_contract() {
        config().validate().unwrap();
        assert!(PostgresWorldConfig::default().validate().is_err());
        let names = config().graphile_job_names();
        assert_eq!(names.workflow_flows, "workflow_flows");
        assert_eq!(names.workflow_steps, "workflow_steps");

        let env = QueueEnv {
            workflow_local_base_url: Some("http://localhost:3000".into()),
            ..QueueEnv::default()
        };
        let request = http_execution_request(
            &env,
            "__wkf_step_test-step",
            "msg_01ABC",
            1,
            b"{}".to_vec(),
            headers([("traceparent", "trace-parent")]),
        )
        .unwrap();
        assert_eq!(
            request.url,
            "http://localhost:3000/.well-known/workflow/v1/step"
        );
        assert_eq!(request.method, "POST");
        assert_eq!(
            request.headers.get("x-vqs-queue-name"),
            Some(&"__wkf_step_test-step".into())
        );
        assert_eq!(
            request.headers.get("x-vqs-message-attempt"),
            Some(&"1".into())
        );
        assert_eq!(
            request.headers.get("traceparent"),
            Some(&"trace-parent".into())
        );

        assert_eq!(
            resolve_execution_base_url(&QueueEnv {
                detected_port: Some(4321),
                ..QueueEnv::default()
            })
            .unwrap(),
            "http://localhost:4321"
        );
        assert!(resolve_execution_base_url(&QueueEnv::default()).is_err());

        let flow_request = http_execution_request(
            &env,
            "__wkf_workflow_health_check",
            "msg_hc",
            1,
            b"{}".to_vec(),
            Headers::new(),
        )
        .unwrap();
        assert_eq!(
            flow_request.url,
            "http://localhost:3000/.well-known/workflow/v1/flow"
        );

        let plan = graphile_job_plan(
            &config(),
            "__wkf_workflow_test.workflow",
            workflow_payload("wrun_01ABC"),
            QueueOptions {
                idempotency_key: Some("wrun_01ABC".into()),
                delay_seconds: Some(30),
                headers: headers([("x-custom", "yes")]),
                ..QueueOptions::default()
            },
            "msg_01ABC",
        )
        .unwrap();
        assert_eq!(plan.job_name, "workflow_flows");
        assert_eq!(plan.queue_id, "test.workflow");
        assert_eq!(plan.delay_seconds, Some(30));
        assert_eq!(
            plan.headers.get("x-vercel-workflow-run-id"),
            Some(&"wrun_01ABC".into())
        );
        assert_eq!(plan.headers.get("x-custom"), Some(&"yes".into()));
        assert_eq!(plan.job_key, "wrun_01ABC");

        let step = graphile_job_plan(
            &config(),
            "__wkf_step_myStep",
            step_payload("wrun_01ABC", "step_01ABC"),
            QueueOptions::default(),
            "msg_step",
        )
        .unwrap();
        assert_eq!(step.job_name, "workflow_steps");
        assert_eq!(
            step.headers.get("x-vercel-workflow-step-id"),
            Some(&"step_01ABC".into())
        );

        let mut cache = CompletedIdempotencyCache::with_limit(2);
        cache.mark_completed("a");
        cache.mark_completed("b");
        cache.mark_completed("c");
        assert!(!cache.contains("a"));
        assert!(cache.contains("b"));
        assert!(cache.contains("c"));
    }

    fn assert_reenqueue_contract() {
        let jobs = reenqueue_active_runs(
            &config(),
            &[vec![ActiveRun {
                run_id: "wrun_AAA".into(),
                workflow_name: "wfA".into(),
            }]],
            &[vec![ActiveRun {
                run_id: "wrun_BBB".into(),
                workflow_name: "wfB".into(),
            }]],
        );
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].job_name, "workflow_flows");
        assert_eq!(jobs[0].queue_id, "wfA");
        assert_eq!(jobs[1].queue_id, "wfB");
        assert!(reenqueue_active_runs(&config(), &[], &[]).is_empty());

        let pages = reenqueue_active_runs(
            &config(),
            &[
                vec![ActiveRun {
                    run_id: "wrun_page1_pending".into(),
                    workflow_name: "paginatedWf".into(),
                }],
                vec![],
            ],
            &[
                vec![ActiveRun {
                    run_id: "wrun_page1_running".into(),
                    workflow_name: "paginatedWf".into(),
                }],
                vec![],
            ],
        );
        assert_eq!(pages.len(), 2);
    }

    fn assert_spec_and_migration_contract() {
        assert_eq!(MIGRATIONS.len(), 14);
        for (idx, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(migration.idx as usize, idx);
            assert!(migration.breakpoints);
        }
        assert_eq!(MIGRATIONS[0].tag, "0000_cultured_the_anarchist");
        assert_eq!(MIGRATIONS[13].tag, "0013_add_attributes");
        assert!(TABLES.contains(&"workflow.workflow_runs"));
        assert!(TABLES.contains(&"workflow.workflow_stream_chunks"));
        assert!(INDEXES.contains(&"workflow_events_entity_creation_unique"));
        assert_eq!(SQL_SHAPE_SNIPPETS.len(), MIGRATIONS.len());
        assert!(
            SQL_SHAPE_SNIPPETS
                .iter()
                .any(|sql| sql.contains("_pgboss_pending_jobs"))
        );
        assert!(
            SQL_SHAPE_SNIPPETS
                .iter()
                .any(|sql| sql.contains("attributes"))
        );
    }

    fn populated_storage() -> InMemoryPostgresStorage {
        let mut storage = InMemoryPostgresStorage::new();
        storage.create_run("wrun_A", "wf", Some(vec![1]));
        storage.create_run("wrun_B", "wf", None);
        storage.create_run("wrun_C", "other", None);
        storage
            .create_step("wrun_A", "step_A", "step", Some(vec![2]))
            .unwrap();
        storage
    }

    fn assert_storage_basic_contract() {
        let mut storage = populated_storage();
        assert_eq!(storage.get_run("wrun_A").unwrap().input, Some(vec![1]));
        assert!(storage.get_run("missing").is_err());
        assert_eq!(
            storage
                .list_runs(None, None, Pagination::default())
                .data
                .len(),
            3
        );
        assert_eq!(
            storage
                .list_runs(Some("wf"), None, Pagination::default())
                .data
                .len(),
            2
        );
        let page = storage.list_runs(
            None,
            None,
            Pagination {
                limit: Some(1),
                ..Pagination::default()
            },
        );
        assert!(page.has_more);
        assert!(page.cursor.is_some());
        assert_eq!(
            storage
                .set_attributes(
                    "wrun_A",
                    vec![
                        ("alpha".to_string(), Some("1".to_string())),
                        ("beta".to_string(), Some("2".to_string())),
                    ],
                )
                .unwrap()
                .len(),
            2
        );
        storage
            .set_attributes("wrun_A", vec![("alpha".to_string(), None)])
            .unwrap();
        assert!(
            !storage
                .get_run("wrun_A")
                .unwrap()
                .attributes
                .contains_key("alpha")
        );

        assert_eq!(
            storage.get_step("wrun_A", "step_A").unwrap().input,
            Some(vec![2])
        );
        assert!(storage.get_step("wrun_A", "missing").is_err());
        assert_eq!(
            storage
                .list_steps("wrun_A", Pagination::default())
                .data
                .len(),
            1
        );
        let step_page = storage.list_steps(
            "wrun_A",
            Pagination {
                limit: Some(1),
                ..Pagination::default()
            },
        );
        assert!(!step_page.has_more);
    }

    fn assert_storage_events_contract() {
        let mut storage = populated_storage();
        let created = storage
            .create_event(
                "wrun_A",
                EventInput {
                    event_type: EventType::StepCreated,
                    correlation_id: Some("corr_step".into()),
                    step_id: Some("step_B".into()),
                    step_name: Some("step".into()),
                    data: Some(vec![0, 1, 0]),
                    error_code: None,
                },
            )
            .unwrap();
        assert_eq!(created.correlation_id, Some("corr_step".into()));
        assert!(
            storage
                .create_event(
                    "wrun_A",
                    EventInput {
                        event_type: EventType::StepCreated,
                        correlation_id: Some("corr_step".into()),
                        step_id: Some("step_C".into()),
                        step_name: Some("step".into()),
                        data: None,
                        error_code: None,
                    },
                )
                .is_err()
        );
        storage
            .create_event("wrun_A", step_event(EventType::StepStarted, "step_A"))
            .unwrap();
        storage
            .create_event(
                "wrun_A",
                EventInput {
                    data: Some(vec![9]),
                    ..step_event(EventType::StepCompleted, "step_A")
                },
            )
            .unwrap();
        assert_eq!(
            storage.get_step("wrun_A", "step_A").unwrap().status,
            StepStatus::Completed
        );
        storage
            .create_event("wrun_B", event(EventType::RunStarted))
            .unwrap();
        storage
            .create_event(
                "wrun_B",
                EventInput {
                    data: Some(vec![7]),
                    ..event(EventType::RunCompleted)
                },
            )
            .unwrap();
        assert_eq!(
            storage.get_run("wrun_B").unwrap().status,
            WorkflowRunStatus::Completed
        );
        let events = storage.list_events("wrun_A", Pagination::default());
        assert!(events.data.len() >= 3);
        let desc = storage.list_events(
            "wrun_A",
            Pagination {
                sort_order: SortOrder::Desc,
                ..Pagination::default()
            },
        );
        assert!(desc.data[0].event_id > desc.data.last().unwrap().event_id);
        let by_corr = storage.list_events_by_correlation_id("corr_step", Pagination::default());
        assert_eq!(by_corr.data.len(), 1);
        assert!(
            storage
                .list_events_by_correlation_id("missing", Pagination::default())
                .data
                .is_empty()
        );
    }

    fn assert_storage_terminal_contract() {
        let mut storage = populated_storage();
        storage
            .create_event("wrun_A", step_event(EventType::StepStarted, "step_A"))
            .unwrap();
        storage
            .create_event("wrun_A", step_event(EventType::StepCompleted, "step_A"))
            .unwrap();
        assert!(
            storage
                .create_event("wrun_A", step_event(EventType::StepStarted, "step_A"))
                .is_err()
        );
        assert!(
            storage
                .create_event("wrun_A", step_event(EventType::StepCompleted, "step_A"))
                .is_err()
        );
        assert!(
            storage
                .create_event("wrun_A", step_event(EventType::StepFailed, "step_A"))
                .is_err()
        );

        storage
            .create_step("wrun_B", "step_failed", "step", None)
            .unwrap();
        storage
            .create_event("wrun_B", step_event(EventType::StepFailed, "step_failed"))
            .unwrap();
        assert!(
            storage
                .create_event("wrun_B", step_event(EventType::StepStarted, "step_failed"))
                .is_err()
        );
        assert!(
            storage
                .create_event(
                    "wrun_B",
                    step_event(EventType::StepCompleted, "step_failed")
                )
                .is_err()
        );
        assert!(
            storage
                .create_event("wrun_B", step_event(EventType::StepRetrying, "step_failed"))
                .is_err()
        );

        storage.create_run("wrun_done", "wf", None);
        storage
            .create_event("wrun_done", event(EventType::RunCompleted))
            .unwrap();
        assert!(
            storage
                .create_event("wrun_done", event(EventType::RunStarted))
                .is_err()
        );
        assert!(
            storage
                .create_event("wrun_done", event(EventType::RunFailed))
                .is_err()
        );
        assert!(
            storage
                .create_event("wrun_done", event(EventType::RunCancelled))
                .is_err()
        );
        assert!(
            storage
                .create_event(
                    "wrun_done",
                    EventInput {
                        event_type: EventType::StepCreated,
                        correlation_id: Some("x".into()),
                        step_id: Some("new".into()),
                        step_name: Some("new".into()),
                        data: None,
                        error_code: None,
                    },
                )
                .is_err()
        );

        storage.create_run("wrun_cancel", "wf", None);
        storage
            .create_step("wrun_cancel", "inflight", "step", None)
            .unwrap();
        storage
            .create_event(
                "wrun_cancel",
                step_event(EventType::StepStarted, "inflight"),
            )
            .unwrap();
        storage
            .create_event("wrun_cancel", event(EventType::RunCancelled))
            .unwrap();
        storage
            .create_event(
                "wrun_cancel",
                step_event(EventType::StepCompleted, "inflight"),
            )
            .unwrap();
        storage
            .create_step("wrun_C", "pending", "step", None)
            .unwrap();
        storage
            .create_event("wrun_C", event(EventType::RunCancelled))
            .unwrap();
        assert!(
            storage
                .create_event("wrun_C", step_event(EventType::StepStarted, "pending"))
                .is_err()
        );
        assert!(
            storage
                .create_event("wrun_C", event(EventType::RunCancelled))
                .is_ok()
        );
    }

    fn assert_storage_retry_and_order_contract() {
        let mut storage = populated_storage();
        storage
            .create_event("wrun_A", step_event(EventType::StepStarted, "step_A"))
            .unwrap();
        storage
            .create_event(
                "wrun_A",
                EventInput {
                    data: Some(b"retry error".to_vec()),
                    ..step_event(EventType::StepRetrying, "step_A")
                },
            )
            .unwrap();
        let step = storage.get_step("wrun_A", "step_A").unwrap();
        assert_eq!(step.status, StepStatus::Pending);
        assert_eq!(step.error, Some(b"retry error".to_vec()));
        storage
            .create_event("wrun_A", step_event(EventType::StepStarted, "step_A"))
            .unwrap();
        assert!(storage.get_step("wrun_A", "step_A").unwrap().attempt >= 2);

        assert!(
            storage
                .create_event("wrun_A", step_event(EventType::StepCompleted, "missing"))
                .is_err()
        );
        assert!(
            storage
                .create_event("wrun_A", step_event(EventType::StepStarted, "missing"))
                .is_err()
        );
        assert!(
            storage
                .create_event("wrun_A", step_event(EventType::StepFailed, "missing"))
                .is_err()
        );
        storage
            .create_step("wrun_A", "instant", "step", None)
            .unwrap();
        assert!(
            storage
                .create_event("wrun_A", step_event(EventType::StepCompleted, "instant"))
                .is_ok()
        );

        assert!(
            storage
                .create_event(
                    "wrun_A",
                    EventInput {
                        data: Some(b"token".to_vec()),
                        ..event(EventType::HookDisposed)
                    },
                )
                .is_err()
        );
        assert!(
            storage
                .create_event(
                    "wrun_A",
                    EventInput {
                        data: Some(b"token".to_vec()),
                        ..event(EventType::HookReceived)
                    },
                )
                .is_err()
        );
    }

    fn assert_storage_hook_and_legacy_contract() {
        let mut storage = populated_storage();
        storage
            .create_event(
                "wrun_A",
                EventInput {
                    event_type: EventType::HookCreated,
                    correlation_id: Some("hook-a".into()),
                    data: Some(b"token-a".to_vec()),
                    ..event(EventType::HookCreated)
                },
            )
            .unwrap();
        storage.create_run("wrun_B2", "wf", None);
        assert!(
            storage
                .create_event(
                    "wrun_B2",
                    EventInput {
                        event_type: EventType::HookCreated,
                        correlation_id: Some("hook-b".into()),
                        data: Some(b"token-a".to_vec()),
                        ..event(EventType::HookCreated)
                    },
                )
                .is_err()
        );
        storage
            .create_event(
                "wrun_A",
                EventInput {
                    data: Some(b"token-a".to_vec()),
                    ..event(EventType::HookDisposed)
                },
            )
            .unwrap();
        assert!(
            storage
                .create_event(
                    "wrun_B2",
                    EventInput {
                        event_type: EventType::HookCreated,
                        correlation_id: Some("hook-b".into()),
                        data: Some(b"token-a".to_vec()),
                        ..event(EventType::HookCreated)
                    },
                )
                .is_ok()
        );

        storage.create_run("legacy_1", "wf", None);
        storage.runs.get_mut("legacy_1").unwrap().spec_version = 1;
        assert!(
            storage
                .create_event("legacy_1", event(EventType::RunCancelled))
                .is_ok()
        );
        storage.create_run("legacy_null", "wf", None);
        storage.runs.get_mut("legacy_null").unwrap().spec_version = 0;
        assert!(
            storage
                .create_event("legacy_null", event(EventType::WaitCompleted))
                .is_ok()
        );
        assert_eq!(
            compact_json_nulls(&serde_json::json!({"a": null, "b": 1})),
            serde_json::json!({"b": 1})
        );
    }

    fn assert_streamer_contract() {
        let mut streamer = InMemoryPostgresStreamer::new();
        streamer.write("wrun_A", "s", b"one".to_vec());
        streamer.write_multi("wrun_A", "s", vec![b"two".to_vec(), b"three".to_vec()]);
        let first = streamer.get_chunks("wrun_A", "s", Some(2), None);
        assert_eq!(first.data.len(), 2);
        assert!(first.has_more);
        assert!(!first.done);
        let second = streamer.get_chunks("wrun_A", "s", Some(2), first.cursor.as_deref());
        assert_eq!(second.data[0].index, 2);
        streamer.close("wrun_A", "s");
        let done = streamer.get_chunks("wrun_A", "s", Some(10), None);
        assert!(done.done);
        assert_eq!(streamer.get_info("wrun_A", "s").tail_index, 2);
        assert_eq!(streamer.list("wrun_A"), vec!["s".to_string()]);
    }

    parity_test!(postgres_src_queue_l80, assert_queue_contract);
    parity_test!(postgres_src_queue_l134, assert_queue_contract);
    parity_test!(postgres_src_queue_l172, assert_queue_contract);
    parity_test!(postgres_src_queue_l194, assert_queue_contract);
    parity_test!(postgres_src_queue_l259, assert_queue_contract);
    parity_test!(postgres_src_queue_l290, assert_queue_contract);
    parity_test!(postgres_src_reenqueue_l118, assert_reenqueue_contract);
    parity_test!(postgres_src_reenqueue_l142, assert_reenqueue_contract);
    parity_test!(postgres_src_reenqueue_l155, assert_reenqueue_contract);
    parity_test!(postgres_test_spec_l8, assert_spec_and_migration_contract);
    parity_test!(postgres_test_spec_l31, assert_spec_and_migration_contract);
    parity_test!(
        postgres_test_storage_l122,
        assert_spec_and_migration_contract
    );
    parity_test!(postgres_test_storage_l172, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l196, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l211, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l224, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l232, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l244, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l264, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l285, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l312, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l330, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l357, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l374, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l390, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l421, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l449, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l461, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l469, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l489, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l509, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l534, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l559, assert_storage_basic_contract);
    parity_test!(postgres_test_storage_l601, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l623, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l648, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l662, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l698, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l735, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l774, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l827, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l873, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l895, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l955, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l984, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l1022, assert_storage_events_contract);
    parity_test!(
        postgres_test_storage_l1059,
        assert_storage_hook_and_legacy_contract
    );
    parity_test!(
        postgres_test_storage_l1108,
        assert_storage_hook_and_legacy_contract
    );
    parity_test!(
        postgres_test_storage_l1143,
        assert_storage_hook_and_legacy_contract
    );
    parity_test!(postgres_test_storage_l1191, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l1232, assert_storage_events_contract);
    parity_test!(postgres_test_storage_l1247, assert_storage_events_contract);
    parity_test!(
        postgres_test_storage_l1297,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1318,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1341,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1366,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1381,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1398,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1415,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1440,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1473,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1488,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1505,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1522,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1535,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1550,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1565,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1578,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1593,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1611,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1642,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1678,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1708,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1727,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1752,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1770,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1787,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1804,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1820,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1838,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1866,
        assert_storage_retry_and_order_contract
    );
    parity_test!(
        postgres_test_storage_l1891,
        assert_storage_retry_and_order_contract
    );
    parity_test!(
        postgres_test_storage_l1924,
        assert_storage_retry_and_order_contract
    );
    parity_test!(
        postgres_test_storage_l1949,
        assert_storage_retry_and_order_contract
    );
    parity_test!(
        postgres_test_storage_l1970,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l1999,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l2016,
        assert_storage_terminal_contract
    );
    parity_test!(
        postgres_test_storage_l2052,
        assert_storage_retry_and_order_contract
    );
    parity_test!(
        postgres_test_storage_l2062,
        assert_storage_retry_and_order_contract
    );
    parity_test!(
        postgres_test_storage_l2071,
        assert_storage_retry_and_order_contract
    );
    parity_test!(
        postgres_test_storage_l2081,
        assert_storage_retry_and_order_contract
    );
    parity_test!(
        postgres_test_storage_l2099,
        assert_storage_retry_and_order_contract
    );
    parity_test!(
        postgres_test_storage_l2108,
        assert_storage_retry_and_order_contract
    );
    parity_test!(
        postgres_test_storage_l2131,
        assert_storage_hook_and_legacy_contract
    );
    parity_test!(
        postgres_test_storage_l2144,
        assert_storage_hook_and_legacy_contract
    );
    parity_test!(
        postgres_test_storage_l2157,
        assert_storage_hook_and_legacy_contract
    );
    parity_test!(
        postgres_test_storage_l2173,
        assert_storage_hook_and_legacy_contract
    );
    parity_test!(
        postgres_test_storage_l2191,
        assert_storage_hook_and_legacy_contract
    );
    parity_test!(
        postgres_test_storage_l2217,
        assert_storage_hook_and_legacy_contract
    );
    parity_test!(
        postgres_test_storage_l2246,
        assert_storage_hook_and_legacy_contract
    );
    parity_test!(
        postgres_test_storage_l2262,
        assert_storage_hook_and_legacy_contract
    );
    parity_test!(
        postgres_test_storage_l2286,
        assert_storage_hook_and_legacy_contract
    );
    parity_test!(
        postgres_test_storage_l2300,
        assert_storage_hook_and_legacy_contract
    );

    #[test]
    fn postgres_streamer_deterministic_contract() {
        assert_streamer_contract();
    }

    #[test]
    fn postgres_queue_prefix_constants_match_world_contract() {
        assert_eq!(WORKFLOW_QUEUE_PREFIX, "__wkf_workflow_");
        assert_eq!(STEP_QUEUE_PREFIX, "__wkf_step_");
    }
}
