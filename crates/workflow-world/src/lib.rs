//! World interface crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/world`. It provides the portable
//! queue, storage, stream, and pagination contracts used by package-owned world
//! implementations.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the initial crate skeleton.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/world";

/// Upstream package version inventoried for this skeleton.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.5";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Current upstream workflow spec version.
pub const SPEC_VERSION_CURRENT: u16 = 3;

/// First spec version that uses CBOR queue transport upstream.
pub const SPEC_VERSION_SUPPORTS_CBOR_QUEUE_TRANSPORT: u16 = 3;

/// Workflow queue name prefix used by upstream.
pub const WORKFLOW_QUEUE_PREFIX: &str = "__wkf_workflow_";

/// Step queue name prefix used by upstream.
pub const STEP_QUEUE_PREFIX: &str = "__wkf_step_";

/// Shared string header map used by queue and HTTP contracts.
pub type Headers = BTreeMap<String, String>;

/// Queue route selected from a queue name prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueRoute {
    /// Workflow run callback route.
    Flow,
    /// Workflow step callback route.
    Step,
}

/// Return the queue route implied by an upstream queue name.
pub fn queue_route(queue_name: &str) -> Option<QueueRoute> {
    if queue_name.starts_with(STEP_QUEUE_PREFIX) {
        Some(QueueRoute::Step)
    } else if queue_name.starts_with(WORKFLOW_QUEUE_PREFIX) {
        Some(QueueRoute::Flow)
    } else {
        None
    }
}

/// Split an upstream queue name into its prefix and id suffix.
pub fn split_queue_name(queue_name: &str) -> Option<(&'static str, &str)> {
    if let Some(id) = queue_name.strip_prefix(STEP_QUEUE_PREFIX) {
        Some((STEP_QUEUE_PREFIX, id))
    } else {
        queue_name
            .strip_prefix(WORKFLOW_QUEUE_PREFIX)
            .map(|id| (WORKFLOW_QUEUE_PREFIX, id))
    }
}

/// Sanitize a queue name to the alphanumeric, dash, and underscore topic shape.
pub fn sanitize_queue_name(queue_name: &str) -> String {
    queue_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Upstream queue payload variants with identifiers used for observability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueuePayload {
    /// Workflow invocation payload.
    Workflow { run_id: String },
    /// Step invocation payload.
    Step {
        workflow_name: String,
        workflow_run_id: String,
        workflow_started_at_ms: u64,
        step_id: String,
    },
    /// Health-check payload, intentionally without workflow headers.
    HealthCheck { correlation_id: String },
}

impl QueuePayload {
    /// Build the workflow/step headers upstream injects into VQS messages.
    pub fn workflow_headers(&self) -> Headers {
        let mut headers = Headers::new();
        match self {
            QueuePayload::Workflow { run_id } => {
                headers.insert("x-vercel-workflow-run-id".into(), run_id.clone());
            }
            QueuePayload::Step {
                workflow_run_id,
                step_id,
                ..
            } => {
                headers.insert("x-vercel-workflow-run-id".into(), workflow_run_id.clone());
                headers.insert("x-vercel-workflow-step-id".into(), step_id.clone());
            }
            QueuePayload::HealthCheck { .. } => {}
        }
        headers
    }

    /// Workflow run id used to serialize replay execution.
    pub fn workflow_run_id(&self) -> Option<&str> {
        match self {
            QueuePayload::Workflow { run_id } => Some(run_id),
            QueuePayload::Step {
                workflow_run_id, ..
            } => Some(workflow_run_id),
            QueuePayload::HealthCheck { .. } => None,
        }
    }
}

/// Options accepted by queue send operations.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QueueOptions {
    pub deployment_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub delay_seconds: Option<u64>,
    pub headers: Headers,
    pub spec_version: Option<u16>,
}

/// Queue message wrapper persisted by upstream Vercel queue messages.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueEnvelope {
    pub payload: QueuePayload,
    pub queue_name: String,
    pub deployment_id: Option<String>,
}

/// Deterministic description of a message send.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueSendPlan {
    pub deployment_id: String,
    pub topic_name: String,
    pub envelope: QueueEnvelope,
    pub idempotency_key: Option<String>,
    pub delay_seconds: Option<u64>,
    pub headers: Headers,
    pub content_type: &'static str,
}

/// Metadata passed to queue handlers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueHandlerMetadata {
    pub queue_name: String,
    pub message_id: String,
    pub attempt: u32,
    pub request_id: Option<String>,
}

/// Queue handler result used to request delayed re-enqueue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueHandlerResult {
    pub timeout_seconds: Option<u64>,
}

/// Sort direction for list APIs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

/// Pagination request shape used by storage list APIs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Pagination {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub sort_order: SortOrder,
}

/// Generic paginated response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub has_more: bool,
    pub cursor: Option<String>,
}

/// Workflow run status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl WorkflowRunStatus {
    /// Whether this status is terminal.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// Workflow step status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl StepStatus {
    /// Whether this status is terminal.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// Minimal portable workflow run model shared by world implementations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowRun {
    pub run_id: String,
    pub workflow_name: String,
    pub deployment_id: String,
    pub status: WorkflowRunStatus,
    pub input: Option<Vec<u8>>,
    pub output: Option<Vec<u8>>,
    pub error: Option<Vec<u8>>,
    pub error_code: Option<String>,
    pub attributes: Headers,
    pub spec_version: u16,
}

/// Minimal portable step model shared by world implementations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Step {
    pub run_id: String,
    pub step_id: String,
    pub step_name: String,
    pub status: StepStatus,
    pub input: Option<Vec<u8>>,
    pub output: Option<Vec<u8>>,
    pub error: Option<Vec<u8>>,
    pub attempt: u32,
    pub spec_version: u16,
}

/// Portable event kind used by Postgres event-sourced storage tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventType {
    RunCreated,
    RunStarted,
    RunCompleted,
    RunFailed,
    RunCancelled,
    StepCreated,
    StepStarted,
    StepCompleted,
    StepFailed,
    StepRetrying,
    HookCreated,
    HookDisposed,
    HookReceived,
    WaitCreated,
    WaitCompleted,
}

/// Minimal portable event model shared by world implementations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    pub event_id: String,
    pub run_id: String,
    pub event_type: EventType,
    pub correlation_id: Option<String>,
    pub event_data: Option<Vec<u8>>,
    pub spec_version: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_world_source_snapshot() {
        assert_eq!(UPSTREAM_PACKAGE, "@workflow/world");
        assert_eq!(UPSTREAM_VERSION, "5.0.0-beta.5");
        assert_eq!(UPSTREAM_HEAD.len(), 40);
    }

    #[test]
    fn queue_payload_headers_match_upstream_world_contract() {
        assert_eq!(
            QueuePayload::Workflow {
                run_id: "wrun_123".into()
            }
            .workflow_headers()
            .get("x-vercel-workflow-run-id"),
            Some(&"wrun_123".into())
        );
        assert!(
            QueuePayload::HealthCheck {
                correlation_id: "hc_123".into()
            }
            .workflow_headers()
            .is_empty()
        );
    }
}
