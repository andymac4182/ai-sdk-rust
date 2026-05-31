use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use time::OffsetDateTime;

use crate::attributes::Attributes;
use crate::data::{PaginationOptions, ResolveData};
use crate::serialization::SerializedData;
use crate::spec_version::SpecVersion;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Materialized workflow run state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRun {
    pub run_id: String,
    pub status: WorkflowRunStatus,
    pub deployment_id: String,
    pub workflow_name: String,
    pub spec_version: Option<SpecVersion>,
    pub execution_context: Option<BTreeMap<String, JsonValue>>,
    pub input: Option<SerializedData>,
    pub output: Option<SerializedData>,
    pub error: Option<SerializedData>,
    pub error_code: Option<String>,
    #[serde(default)]
    pub attributes: Attributes,
    pub expired_at: Option<OffsetDateTime>,
    pub started_at: Option<OffsetDateTime>,
    pub completed_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// Run shape returned when `resolve_data` is `None`.
pub type WorkflowRunWithoutData = WorkflowRun;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkflowRunRequest {
    pub deployment_id: String,
    pub workflow_name: String,
    pub input: SerializedData,
    pub execution_context: Option<SerializedData>,
    pub spec_version: Option<SpecVersion>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetWorkflowRunParams {
    pub resolve_data: Option<ResolveData>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListWorkflowRunsParams {
    pub workflow_name: Option<String>,
    pub status: Option<WorkflowRunStatus>,
    pub pagination: Option<PaginationOptions>,
    pub resolve_data: Option<ResolveData>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelWorkflowRunParams {
    pub resolve_data: Option<ResolveData>,
}
