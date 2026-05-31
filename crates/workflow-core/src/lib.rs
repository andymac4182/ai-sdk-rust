//! Core runtime crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/core`. Package-owned runtime and
//! serialization contracts live here and are re-exported by the `workflow`
//! facade.

#![forbid(unsafe_code)]

pub use workflow_errors as errors;
pub use workflow_world as world;

/// Runtime-facing types re-exported by upstream `workflow/api.ts` and
/// `workflow/runtime.ts`.
pub mod runtime {
    use std::{error::Error, fmt};

    /// Deployment selector accepted when starting a workflow run.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum DeploymentId {
        /// Resolve the latest deployment for the active environment.
        Latest,
        /// Start the run against a concrete deployment id.
        Id(String),
    }

    /// Options accepted by the public workflow `start` API.
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub struct StartOptions {
        /// Optional world adapter name or handle chosen by the caller.
        pub world: Option<String>,
        /// Optional workflow spec version override.
        pub spec_version: Option<u32>,
        /// Optional deployment selector.
        pub deployment_id: Option<DeploymentId>,
    }

    impl StartOptions {
        /// Creates empty start options.
        pub fn new() -> Self {
            Self::default()
        }

        /// Sets the world adapter name or handle.
        pub fn with_world(mut self, world: impl Into<String>) -> Self {
            self.world = Some(world.into());
            self
        }

        /// Sets the workflow spec version.
        pub fn with_spec_version(mut self, spec_version: u32) -> Self {
            self.spec_version = Some(spec_version);
            self
        }

        /// Resolves the workflow against the latest deployment.
        pub fn with_latest_deployment(mut self) -> Self {
            self.deployment_id = Some(DeploymentId::Latest);
            self
        }

        /// Resolves the workflow against a concrete deployment id.
        pub fn with_deployment_id(mut self, deployment_id: impl Into<String>) -> Self {
            self.deployment_id = Some(DeploymentId::Id(deployment_id.into()));
            self
        }
    }

    /// Metadata emitted by the upstream transform for an imported workflow.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct WorkflowMetadata {
        /// Machine-readable upstream workflow id.
        pub workflow_id: String,
    }

    impl WorkflowMetadata {
        /// Creates workflow metadata from a machine-readable workflow id.
        pub fn new(workflow_id: impl Into<String>) -> Self {
            Self {
                workflow_id: workflow_id.into(),
            }
        }
    }

    /// Handle returned by the public workflow start API.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Run {
        /// Unique workflow run id.
        pub run_id: String,
    }

    impl Run {
        /// Creates a run handle.
        pub fn new(run_id: impl Into<String>) -> Self {
            Self {
                run_id: run_id.into(),
            }
        }
    }

    /// Result returned by a runtime health-check endpoint.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct HealthCheckResult {
        /// Whether the runtime endpoint is healthy.
        pub ok: bool,
        /// Runtime or world implementation name.
        pub runtime: String,
        /// World spec version observed by the endpoint.
        pub spec_version: Option<u32>,
    }

    /// Error used by workflow-context API stubs.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct WorkflowRuntimeUsageError {
        item: String,
    }

    impl WorkflowRuntimeUsageError {
        /// Creates an error for an API that cannot run in the workflow body
        /// context.
        pub fn new(item: impl Into<String>) -> Self {
            Self { item: item.into() }
        }

        /// API item that was rejected.
        pub fn item(&self) -> &str {
            &self.item
        }
    }

    impl fmt::Display for WorkflowRuntimeUsageError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "The workflow environment doesn't allow this runtime usage of {}. Move this call to a step function (\"use step\") or call it outside the workflow context.",
                self.item
            )
        }
    }

    impl Error for WorkflowRuntimeUsageError {}

    /// Returns the same workflow-context rejection used by upstream
    /// `api-workflow.ts` stubs.
    pub fn workflow_context_stub<T>(
        item: impl Into<String>,
    ) -> Result<T, WorkflowRuntimeUsageError> {
        Err(WorkflowRuntimeUsageError::new(item))
    }
}

/// Browser-safe serialization-format helpers used by observability.
pub mod serialization_format {
    use serde_json::{Map, Value};

    /// Length of the upstream serialization format prefix.
    pub const FORMAT_PREFIX_LENGTH: usize = 4;

    /// Upstream devalue serialization prefix.
    pub const DEVALUE_V1_FORMAT: &str = "devl";

    /// Upstream encrypted payload prefix.
    pub const ENCRYPTED_FORMAT: &str = "encr";

    /// Placeholder displayed for encrypted data when no decryption key is
    /// supplied.
    pub const ENCRYPTED_PLACEHOLDER: &str = "\u{1F512} Encrypted";

    /// Display-friendly observability reviver registry.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct ObservabilityRevivers;

    impl ObservabilityRevivers {
        /// Returns true when the upstream observability registry exposes a
        /// reviver for the given serialized type.
        pub fn contains(&self, name: &str) -> bool {
            OBSERVABILITY_REVIVER_NAMES.contains(&name)
        }

        /// Names exposed by upstream `observabilityRevivers`.
        pub fn names(&self) -> &'static [&'static str] {
            OBSERVABILITY_REVIVER_NAMES
        }
    }

    /// Shared reviver type for the Rust observability facade.
    pub type Revivers = ObservabilityRevivers;

    /// Upstream observability reviver names.
    pub const OBSERVABILITY_REVIVER_NAMES: &[&str] = &[
        "ReadableStream",
        "WritableStream",
        "TransformStream",
        "AbortController",
        "AbortSignal",
        "DOMException",
        "StepFunction",
        "WorkflowFunction",
        "Instance",
        "Class",
    ];

    /// Upstream `observabilityRevivers` registry.
    pub const OBSERVABILITY_REVIVERS: ObservabilityRevivers = ObservabilityRevivers;

    /// Returns the observability reviver registry.
    pub fn observability_revivers() -> ObservabilityRevivers {
        OBSERVABILITY_REVIVERS
    }

    /// Hydrates serialized data for observability.
    ///
    /// The initial Rust surface faithfully preserves already-plain values. Full
    /// devalue and encrypted payload handling belongs to the broader
    /// `workflow-core` serialization bucket.
    pub fn hydrate_data(value: Value, _revivers: Revivers) -> Value {
        value
    }

    /// Hydrates the input/output-style fields of a workflow resource.
    pub fn hydrate_resource_io(resource: Value, revivers: Revivers) -> Value {
        let Value::Object(mut object) = resource else {
            return resource;
        };

        if object.contains_key("stepId") {
            hydrate_fields(&mut object, &["input", "output", "error"], revivers);
        } else if object.contains_key("hookId") {
            hydrate_fields(&mut object, &["metadata"], revivers);
        } else if object.contains_key("eventId") {
            hydrate_event_data(&mut object, revivers);
        } else {
            hydrate_fields(&mut object, &["input", "output", "error"], revivers);
        }

        strip_execution_context(&mut object);

        Value::Object(object)
    }

    fn hydrate_fields(object: &mut Map<String, Value>, fields: &[&str], revivers: Revivers) {
        for field in fields {
            let Some(value) = object.get(*field) else {
                continue;
            };
            if value.is_null() {
                continue;
            }
            object.insert((*field).to_string(), hydrate_data(value.clone(), revivers));
        }
    }

    fn hydrate_event_data(object: &mut Map<String, Value>, revivers: Revivers) {
        let Some(Value::Object(mut event_data)) = object.get("eventData").cloned() else {
            return;
        };

        hydrate_fields(
            &mut event_data,
            &["result", "input", "output", "metadata", "payload", "error"],
            revivers,
        );
        object.insert("eventData".to_string(), Value::Object(event_data));
    }

    fn strip_execution_context(object: &mut Map<String, Value>) {
        let Some(execution_context) = object.remove("executionContext") else {
            return;
        };

        let Some(workflow_core_version) = execution_context
            .as_object()
            .and_then(|context| context.get("workflowCoreVersion"))
            .filter(|value| !value.is_null())
            .cloned()
        else {
            return;
        };

        object.insert("workflowCoreVersion".to_string(), workflow_core_version);
    }
}

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the initial crate skeleton.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/core";

/// Upstream package version inventoried for this skeleton.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.10";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_core_source_snapshot() {
        assert_eq!(UPSTREAM_PACKAGE, "@workflow/core");
        assert_eq!(UPSTREAM_VERSION, "5.0.0-beta.10");
        assert_eq!(UPSTREAM_HEAD.len(), 40);
    }
}
