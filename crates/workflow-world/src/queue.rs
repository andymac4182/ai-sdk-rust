use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use time::OffsetDateTime;

use crate::spec_version::SpecVersion;

pub const STEP_QUEUE_PREFIX: &str = "__wkf_step_";
pub const WORKFLOW_QUEUE_PREFIX: &str = "__wkf_workflow_";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueueRoute {
    Flow,
    Step,
}

pub fn queue_route(queue_name: &str) -> Option<QueueRoute> {
    if queue_name.starts_with(STEP_QUEUE_PREFIX) {
        Some(QueueRoute::Step)
    } else if queue_name.starts_with(WORKFLOW_QUEUE_PREFIX) {
        Some(QueueRoute::Flow)
    } else {
        None
    }
}

pub fn split_queue_name(queue_name: &str) -> Option<(&'static str, &str)> {
    if let Some(id) = queue_name.strip_prefix(STEP_QUEUE_PREFIX) {
        Some((STEP_QUEUE_PREFIX, id))
    } else {
        queue_name
            .strip_prefix(WORKFLOW_QUEUE_PREFIX)
            .map(|id| (WORKFLOW_QUEUE_PREFIX, id))
    }
}

pub fn sanitize_queue_name(queue_name: &str) -> String {
    queue_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueuePrefix {
    #[serde(rename = "__wkf_step_")]
    Step,
    #[serde(rename = "__wkf_workflow_")]
    Workflow,
}

impl QueuePrefix {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Step => STEP_QUEUE_PREFIX,
            Self::Workflow => WORKFLOW_QUEUE_PREFIX,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidQueueName(String);

impl ValidQueueName {
    pub fn new(value: impl Into<String>) -> Result<Self, QueueNameError> {
        let value = value.into();
        if value.starts_with(STEP_QUEUE_PREFIX) || value.starts_with(WORKFLOW_QUEUE_PREFIX) {
            Ok(Self(value))
        } else {
            Err(QueueNameError(value))
        }
    }

    pub fn step(name: impl AsRef<str>) -> Self {
        Self(format!("{STEP_QUEUE_PREFIX}{}", name.as_ref()))
    }

    pub fn workflow(name: impl AsRef<str>) -> Self {
        Self(format!("{WORKFLOW_QUEUE_PREFIX}{}", name.as_ref()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ValidQueueName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueNameError(String);

impl fmt::Display for QueueNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "queue name {:?} must start with {STEP_QUEUE_PREFIX:?} or {WORKFLOW_QUEUE_PREFIX:?}",
            self.0
        )
    }
}

impl Error for QueueNameError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageId(String);

impl MessageId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub type TraceCarrier = BTreeMap<String, String>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInput {
    pub input: JsonValue,
    pub deployment_id: String,
    pub workflow_name: String,
    pub spec_version: SpecVersion,
    pub execution_context: Option<BTreeMap<String, JsonValue>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowInvokePayload {
    pub run_id: String,
    pub trace_carrier: Option<TraceCarrier>,
    pub requested_at: Option<OffsetDateTime>,
    pub server_error_retry_count: Option<u32>,
    pub step_id: Option<String>,
    pub step_name: Option<String>,
    pub run_input: Option<RunInput>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepInvokePayload {
    pub workflow_name: String,
    pub workflow_run_id: String,
    pub workflow_started_at: i64,
    pub step_id: String,
    pub trace_carrier: Option<TraceCarrier>,
    pub requested_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckPayload {
    #[serde(rename = "__healthCheck")]
    pub health_check: bool,
    pub correlation_id: String,
}

impl HealthCheckPayload {
    pub fn new(correlation_id: impl Into<String>) -> Self {
        Self {
            health_check: true,
            correlation_id: correlation_id.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum QueuePayload {
    Workflow(WorkflowInvokePayload),
    Step(StepInvokePayload),
    HealthCheck(HealthCheckPayload),
}

impl QueuePayload {
    pub fn workflow_headers(&self) -> BTreeMap<String, String> {
        let mut headers = BTreeMap::new();
        match self {
            Self::Workflow(payload) => {
                headers.insert("x-vercel-workflow-run-id".into(), payload.run_id.clone());
            }
            Self::Step(payload) => {
                headers.insert(
                    "x-vercel-workflow-run-id".into(),
                    payload.workflow_run_id.clone(),
                );
                headers.insert("x-vercel-workflow-step-id".into(), payload.step_id.clone());
            }
            Self::HealthCheck(_) => {}
        }
        headers
    }

    pub fn workflow_run_id(&self) -> Option<&str> {
        match self {
            Self::Workflow(payload) => Some(&payload.run_id),
            Self::Step(payload) => Some(&payload.workflow_run_id),
            Self::HealthCheck(_) => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueOptions {
    pub deployment_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub headers: Option<BTreeMap<String, String>>,
    pub delay_seconds: Option<u32>,
    pub spec_version: Option<SpecVersion>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEnvelope {
    pub payload: QueuePayload,
    pub queue_name: String,
    pub deployment_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSendPlan {
    pub deployment_id: String,
    pub topic_name: String,
    pub envelope: QueueEnvelope,
    pub idempotency_key: Option<String>,
    pub delay_seconds: Option<u64>,
    pub headers: BTreeMap<String, String>,
    pub content_type: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueHandlerMetadata {
    pub queue_name: String,
    pub message_id: String,
    pub attempt: u32,
    pub request_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueHandlerResult {
    pub timeout_seconds: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueResult {
    pub message_id: Option<MessageId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueHandlerMeta {
    pub attempt: u32,
    pub queue_name: ValidQueueName,
    pub message_id: MessageId,
    pub request_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QueueHandlerOutcome {
    Complete,
    Timeout { timeout_seconds: u32 },
}
