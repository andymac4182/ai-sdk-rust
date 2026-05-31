use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::data::{PaginationOptions, ResolveData};
use crate::serialization::SerializedData;
use crate::spec_version::SpecVersion;

/// Hook that can resume a paused workflow run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hook {
    pub run_id: String,
    pub hook_id: String,
    pub token: String,
    pub owner_id: String,
    pub project_id: String,
    pub environment: String,
    pub metadata: Option<SerializedData>,
    pub created_at: OffsetDateTime,
    pub spec_version: Option<SpecVersion>,
    pub is_webhook: Option<bool>,
    pub is_system: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateHookRequest {
    pub hook_id: String,
    pub token: String,
    pub metadata: Option<SerializedData>,
    pub is_webhook: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetHookByTokenParams {
    pub token: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListHooksParams {
    pub run_id: Option<String>,
    pub pagination: Option<PaginationOptions>,
    pub resolve_data: Option<ResolveData>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetHookParams {
    pub resolve_data: Option<ResolveData>,
}
