//! Public API re-exports matching upstream `workflow/api.ts`.

pub use workflow_core::runtime::{
    DeploymentId, HealthCheckResult, Run, StartOptions, WorkflowMetadata, WorkflowRuntimeUsageError,
};

/// Workflow-context stub matching upstream `api-workflow.ts`.
pub fn get_run<T>() -> Result<T, WorkflowRuntimeUsageError> {
    workflow_core::runtime::workflow_context_stub("getRun")
}

/// Workflow-context stub matching upstream `api-workflow.ts`.
pub fn get_hook_by_token<T>() -> Result<T, WorkflowRuntimeUsageError> {
    workflow_core::runtime::workflow_context_stub("getHookByToken")
}

/// Workflow-context stub matching upstream `api-workflow.ts`.
pub fn resume_hook<T>() -> Result<T, WorkflowRuntimeUsageError> {
    workflow_core::runtime::workflow_context_stub("resumeHook")
}

/// Workflow-context stub matching upstream `api-workflow.ts`.
pub fn resume_webhook<T>() -> Result<T, WorkflowRuntimeUsageError> {
    workflow_core::runtime::workflow_context_stub("resumeWebhook")
}

/// Workflow-context stub matching upstream `api-workflow.ts`.
pub fn run_step<T>() -> Result<T, WorkflowRuntimeUsageError> {
    workflow_core::runtime::workflow_context_stub("runStep")
}
