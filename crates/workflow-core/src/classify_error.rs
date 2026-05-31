use workflow_errors::{RunErrorCode, WorkflowError, WorkflowErrorKind};

use crate::context_errors::ContextViolationError;

/// Rust representation of JavaScript's `unknown` thrown values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunError {
    Workflow(WorkflowError),
    ContextViolation(ContextViolationError),
    String(String),
    Null,
    Undefined,
}

impl From<WorkflowError> for RunError {
    fn from(value: WorkflowError) -> Self {
        Self::Workflow(value)
    }
}

impl From<ContextViolationError> for RunError {
    fn from(value: ContextViolationError) -> Self {
        Self::ContextViolation(value)
    }
}

#[must_use]
pub fn is_world_contract_error(err: &WorkflowError) -> bool {
    if err.kind() != WorkflowErrorKind::WorkflowWorld || err.status().is_some() {
        return false;
    }

    matches!(
        err.code(),
        Some("PARSE_ERROR" | "SCHEMA_VALIDATION" | "WORLD_CONTRACT_ERROR")
    ) || err
        .message()
        .starts_with("Failed to parse response body for ")
        || err.message().starts_with("Schema validation failed for ")
        || err.cause_name() == Some("ZodError")
}

/// Classify an error that caused a workflow run to fail.
#[must_use]
pub fn classify_run_error(err: &RunError) -> RunErrorCode {
    match err {
        RunError::Workflow(error) if error.kind() == WorkflowErrorKind::CorruptedEventLog => {
            RunErrorCode::CorruptedEventLog
        }
        RunError::Workflow(error) if is_world_contract_error(error) => {
            RunErrorCode::WorldContractError
        }
        RunError::Workflow(error) if error.is_runtime_error_family() => RunErrorCode::RuntimeError,
        RunError::Workflow(_)
        | RunError::ContextViolation(_)
        | RunError::String(_)
        | RunError::Null
        | RunError::Undefined => RunErrorCode::UserError,
    }
}
