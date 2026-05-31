use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::data::{PaginationOptions, ResolveData};
use crate::serialization::SerializedData;
use crate::spec_version::SpecVersion;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Materialized workflow step state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub run_id: String,
    pub step_id: String,
    pub step_name: String,
    pub status: StepStatus,
    pub input: Option<SerializedData>,
    pub output: Option<SerializedData>,
    pub error: Option<SerializedData>,
    pub attempt: u32,
    pub started_at: Option<OffsetDateTime>,
    pub completed_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub retry_after: Option<OffsetDateTime>,
    pub spec_version: Option<SpecVersion>,
}

/// Step shape returned when `resolve_data` is `None`.
pub type StepWithoutData = Step;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateStepRequest {
    pub step_id: String,
    pub step_name: String,
    pub input: SerializedData,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStepRequest {
    pub attempt: Option<u32>,
    pub status: Option<StepStatus>,
    pub output: Option<SerializedData>,
    pub error: Option<SerializedData>,
    pub retry_after: Option<OffsetDateTime>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetStepParams {
    pub resolve_data: Option<ResolveData>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListWorkflowRunStepsParams {
    pub run_id: String,
    pub pagination: Option<PaginationOptions>,
    pub resolve_data: Option<ResolveData>,
}
