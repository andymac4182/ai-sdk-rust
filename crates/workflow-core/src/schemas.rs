use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// JSON-portable subset used by this bucket for core queue and invoke payloads.
pub type Serializable = serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowInvokePayload {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_carrier: Option<BTreeMap<String, String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepInvokePayload {
    pub workflow_run_id: String,
    pub workflow_started_at: i64,
    pub workflow_name: String,
    pub step_id: String,
}
