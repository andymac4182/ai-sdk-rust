use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::spec_version::SpecVersion;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitStatus {
    Waiting,
    Completed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wait {
    pub wait_id: String,
    pub run_id: String,
    pub status: WaitStatus,
    pub resume_at: Option<OffsetDateTime>,
    pub completed_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub spec_version: Option<SpecVersion>,
}
