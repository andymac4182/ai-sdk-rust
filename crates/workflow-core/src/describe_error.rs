use workflow_errors::{RunErrorCode, WorkflowErrorKind};

use crate::classify_error::{RunError, classify_run_error};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ErrorAttribution {
    User,
    Sdk,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorDescription {
    pub attribution: ErrorAttribution,
    pub error_code: RunErrorCode,
    pub hint: Option<&'static str>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PersistedErrorSignal {
    pub error_code: Option<String>,
    pub error_name: Option<String>,
}

pub const SERIALIZATION_ERROR_HINT: &str = "A value passed across a workflow/step boundary could not be serialized. See the error message for the offending path and the Learn More link for details.";
pub const CONTEXT_ERROR_HINT: &str = "A workflow-only or step-only API was called from the wrong context. The error message includes the exact API and how to move the call.";
pub const RUNTIME_ERROR_HINT: &str = "This is an internal workflow SDK error, not a bug in your code. If it keeps happening, please report it with the stack trace and the runId.";
pub const CORRUPTED_EVENT_LOG_HINT: &str = "The workflow event log contains orphaned or mismatched events and cannot be replayed. This is an internal workflow SDK error; please report it with the runId.";
pub const REPLAY_TIMEOUT_HINT: &str = "The workflow replay between step boundaries took too long. This bounds workflow-VM and event-log replay time only \u{2014} step bodies (`\"use step\"` functions) are excluded. This usually means the event log is unusually large or the workflow function is doing heavy synchronous work in workflow code outside of step bodies. Override the default budget via the WORKFLOW_REPLAY_TIMEOUT_MS env var if needed.";
pub const MAX_DELIVERIES_HINT: &str = "The workflow queue exceeded its max-delivery budget. This usually indicates a persistent runtime failure \u{2014} check the most recent stack traces for the underlying cause.";
pub const WORLD_CONTRACT_HINT: &str = "The workflow backend returned data that violated the SDK contract. This is not retryable; please report it with the stack trace and runId.";

fn normalize_error_code(code: Option<&str>) -> RunErrorCode {
    code.and_then(RunErrorCode::from_code)
        .unwrap_or(RunErrorCode::UserError)
}

/// Describe persisted run-failure fields without a live error instance.
#[must_use]
pub fn describe_run_error(signal: &PersistedErrorSignal) -> ErrorDescription {
    let name = signal.error_name.as_deref();
    let error_code = if name == Some("CorruptedEventLogError") {
        RunErrorCode::CorruptedEventLog
    } else {
        normalize_error_code(signal.error_code.as_deref())
    };

    if name == Some("SerializationError") {
        return ErrorDescription {
            attribution: ErrorAttribution::User,
            error_code,
            hint: Some(SERIALIZATION_ERROR_HINT),
        };
    }
    if matches!(
        name,
        Some(
            "NotInWorkflowContextError"
                | "NotInStepContextError"
                | "NotInWorkflowOrStepContextError"
                | "UnavailableInWorkflowContextError"
        )
    ) {
        return ErrorDescription {
            attribution: ErrorAttribution::User,
            error_code,
            hint: Some(CONTEXT_ERROR_HINT),
        };
    }
    if name == Some("CorruptedEventLogError") {
        return ErrorDescription {
            attribution: ErrorAttribution::Sdk,
            error_code,
            hint: Some(CORRUPTED_EVENT_LOG_HINT),
        };
    }
    if error_code == RunErrorCode::ReplayTimeout {
        return ErrorDescription {
            attribution: ErrorAttribution::Sdk,
            error_code,
            hint: Some(REPLAY_TIMEOUT_HINT),
        };
    }
    if error_code == RunErrorCode::MaxDeliveriesExceeded {
        return ErrorDescription {
            attribution: ErrorAttribution::Sdk,
            error_code,
            hint: Some(MAX_DELIVERIES_HINT),
        };
    }
    if error_code == RunErrorCode::CorruptedEventLog {
        return ErrorDescription {
            attribution: ErrorAttribution::Sdk,
            error_code,
            hint: Some(CORRUPTED_EVENT_LOG_HINT),
        };
    }
    if error_code == RunErrorCode::WorldContractError {
        return ErrorDescription {
            attribution: ErrorAttribution::Sdk,
            error_code,
            hint: Some(WORLD_CONTRACT_HINT),
        };
    }
    if matches!(
        name,
        Some("WorkflowRuntimeError" | "StepNotRegisteredError")
    ) || error_code == RunErrorCode::RuntimeError
    {
        return ErrorDescription {
            attribution: ErrorAttribution::Sdk,
            error_code,
            hint: Some(RUNTIME_ERROR_HINT),
        };
    }

    ErrorDescription {
        attribution: ErrorAttribution::User,
        error_code,
        hint: None,
    }
}

/// Describe a live error for user-facing presentation.
#[must_use]
pub fn describe_error(err: &RunError, error_code: Option<RunErrorCode>) -> ErrorDescription {
    let effective_code = error_code.unwrap_or_else(|| classify_run_error(err));

    match err {
        RunError::Workflow(error) if error.kind() == WorkflowErrorKind::Serialization => {
            return ErrorDescription {
                attribution: ErrorAttribution::User,
                error_code: effective_code,
                hint: Some(SERIALIZATION_ERROR_HINT),
            };
        }
        RunError::ContextViolation(_) => {
            return ErrorDescription {
                attribution: ErrorAttribution::User,
                error_code: effective_code,
                hint: Some(CONTEXT_ERROR_HINT),
            };
        }
        RunError::Workflow(error) if error.kind() == WorkflowErrorKind::CorruptedEventLog => {
            return ErrorDescription {
                attribution: ErrorAttribution::Sdk,
                error_code: effective_code,
                hint: Some(CORRUPTED_EVENT_LOG_HINT),
            };
        }
        RunError::Workflow(error)
            if matches!(
                error.kind(),
                WorkflowErrorKind::WorkflowRuntime
                    | WorkflowErrorKind::StepNotRegistered
                    | WorkflowErrorKind::WorkflowNotRegistered
                    | WorkflowErrorKind::RuntimeDecryption
            ) =>
        {
            return ErrorDescription {
                attribution: ErrorAttribution::Sdk,
                error_code: effective_code,
                hint: Some(RUNTIME_ERROR_HINT),
            };
        }
        _ => {}
    }

    match effective_code {
        RunErrorCode::ReplayTimeout => ErrorDescription {
            attribution: ErrorAttribution::Sdk,
            error_code: effective_code,
            hint: Some(REPLAY_TIMEOUT_HINT),
        },
        RunErrorCode::MaxDeliveriesExceeded => ErrorDescription {
            attribution: ErrorAttribution::Sdk,
            error_code: effective_code,
            hint: Some(MAX_DELIVERIES_HINT),
        },
        RunErrorCode::CorruptedEventLog => ErrorDescription {
            attribution: ErrorAttribution::Sdk,
            error_code: effective_code,
            hint: Some(CORRUPTED_EVENT_LOG_HINT),
        },
        RunErrorCode::WorldContractError => ErrorDescription {
            attribution: ErrorAttribution::Sdk,
            error_code: effective_code,
            hint: Some(WORLD_CONTRACT_HINT),
        },
        RunErrorCode::RuntimeError => ErrorDescription {
            attribution: ErrorAttribution::Sdk,
            error_code: effective_code,
            hint: Some(RUNTIME_ERROR_HINT),
        },
        RunErrorCode::UserError => ErrorDescription {
            attribution: ErrorAttribution::User,
            error_code: effective_code,
            hint: None,
        },
    }
}
