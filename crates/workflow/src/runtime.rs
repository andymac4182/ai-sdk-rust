//! Runtime-facing re-exports matching upstream `workflow/runtime.ts`.

pub use workflow_core::runtime::{
    DeploymentId, HealthCheckResult, Run, StartOptions, WorkflowMetadata, WorkflowRuntimeUsageError,
};

/// Runtime entrypoint descriptor.
///
/// Full request handling belongs to `workflow-core`; the facade only exposes
/// the callable shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowEntrypoint {
    workflow_code: String,
}

impl WorkflowEntrypoint {
    /// Creates a runtime entrypoint descriptor from bundled workflow code.
    pub fn new(workflow_code: impl Into<String>) -> Self {
        Self {
            workflow_code: workflow_code.into(),
        }
    }

    /// Bundled workflow code used by the runtime handler.
    pub fn workflow_code(&self) -> &str {
        &self.workflow_code
    }
}
