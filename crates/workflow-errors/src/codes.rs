use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RunErrorCode {
    UserError,
    RuntimeError,
    CorruptedEventLog,
    MaxDeliveriesExceeded,
    ReplayTimeout,
    WorldContractError,
}

impl RunErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UserError => "USER_ERROR",
            Self::RuntimeError => "RUNTIME_ERROR",
            Self::CorruptedEventLog => "CORRUPTED_EVENT_LOG",
            Self::MaxDeliveriesExceeded => "MAX_DELIVERIES_EXCEEDED",
            Self::ReplayTimeout => "REPLAY_TIMEOUT",
            Self::WorldContractError => "WORLD_CONTRACT_ERROR",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunErrorCodes;

pub const RUN_ERROR_CODES: RunErrorCodes = RunErrorCodes;

impl RunErrorCodes {
    pub const USER_ERROR: RunErrorCode = RunErrorCode::UserError;
    pub const RUNTIME_ERROR: RunErrorCode = RunErrorCode::RuntimeError;
    pub const CORRUPTED_EVENT_LOG: RunErrorCode = RunErrorCode::CorruptedEventLog;
    pub const MAX_DELIVERIES_EXCEEDED: RunErrorCode = RunErrorCode::MaxDeliveriesExceeded;
    pub const REPLAY_TIMEOUT: RunErrorCode = RunErrorCode::ReplayTimeout;
    pub const WORLD_CONTRACT_ERROR: RunErrorCode = RunErrorCode::WorldContractError;
}
