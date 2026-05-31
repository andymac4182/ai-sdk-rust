//! Facade crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/workflow`. It keeps behavior in the
//! owning package crates when possible and exposes the public `workflow`
//! facade expected by the standalone SDK.

#![forbid(unsafe_code)]

pub mod api;
pub mod host;
pub mod internal;
pub mod observability;
pub mod runtime;
pub mod stdlib;

pub use stdlib::fetch;
pub use workflow_core as core;
pub use workflow_errors as errors;
pub use workflow_utils as workflow_utilities;

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the initial crate skeleton.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "workflow";

/// Upstream package version inventoried for this skeleton.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.10";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;
    use internal::builtins::{
        __builtin_set_attributes, __builtin_set_attributes_max_retries, AttributeChange,
        AttributeWorld, AttributeWorldDispatch, BuiltinSetAttributesState, BuiltinStepContext,
        SetAttributesError, SetAttributesLogger, SetAttributesOptions,
    };
    use serde_json::json;

    #[derive(Default)]
    struct TestLogger {
        warnings: Vec<String>,
        errors: Vec<String>,
    }

    impl SetAttributesLogger for TestLogger {
        fn warn(&mut self, message: &str) {
            self.warnings.push(message.to_string());
        }

        fn error(&mut self, message: &str) {
            self.errors.push(message.to_string());
        }
    }

    struct FailingWorld {
        calls: Vec<(String, Vec<AttributeChange>, SetAttributesOptions)>,
    }

    impl FailingWorld {
        fn new() -> Self {
            Self { calls: Vec::new() }
        }
    }

    impl AttributeWorld for FailingWorld {
        fn experimental_set_attributes(
            &mut self,
            run_id: &str,
            changes: &[AttributeChange],
            options: SetAttributesOptions,
        ) -> Result<(), SetAttributesError> {
            self.calls
                .push((run_id.to_string(), changes.to_vec(), options));
            Err(SetAttributesError::World("world unavailable".to_string()))
        }
    }

    #[test]
    fn workflow_builtins_set_attributes_rethrows_before_third_attempt() {
        let changes = [AttributeChange::set("$tag.kind", "agent")];
        let options = SetAttributesOptions {
            allow_reserved_attributes: true,
        };
        let mut state = BuiltinSetAttributesState::default();
        let mut logger = TestLogger::default();
        let mut world = FailingWorld::new();

        for attempt in [1, 2] {
            let context = BuiltinStepContext::new(attempt, "run_123");
            let error = __builtin_set_attributes(
                &changes,
                options,
                &context,
                AttributeWorldDispatch::Supported(&mut world),
                &mut state,
                &mut logger,
            )
            .expect_err("attempts before the third should rethrow");

            assert_eq!(
                error,
                SetAttributesError::World("world unavailable".to_string())
            );
        }

        assert_eq!(world.calls.len(), 2);
        assert!(logger.errors.is_empty());
    }

    #[test]
    fn workflow_builtins_set_attributes_logs_after_third_failed_attempt() {
        let changes = [AttributeChange::set("$tag.kind", "agent")];
        let options = SetAttributesOptions {
            allow_reserved_attributes: true,
        };
        let context = BuiltinStepContext::new(3, "run_123");
        let mut state = BuiltinSetAttributesState::default();
        let mut logger = TestLogger::default();
        let mut world = FailingWorld::new();

        __builtin_set_attributes(
            &changes,
            options,
            &context,
            AttributeWorldDispatch::Supported(&mut world),
            &mut state,
            &mut logger,
        )
        .expect("the third failed attempt is logged and completed");

        assert_eq!(
            world.calls,
            vec![("run_123".to_string(), changes.to_vec(), options)]
        );
        assert_eq!(logger.errors.len(), 1);
        assert!(logger.errors[0].contains("failed to post tags after 3 attempts"));
        assert_eq!(__builtin_set_attributes_max_retries(), 2);
    }

    #[test]
    fn workflow_observability_reexports_parse_step_name_and_it_works() {
        let result =
            observability::parse_step_name("step//./src/workflows/pulse//queryKBStep").unwrap();

        assert_eq!(result.short_name, "queryKBStep");
    }

    #[test]
    fn workflow_observability_reexports_parse_workflow_name_and_it_works() {
        let result = observability::parse_workflow_name(
            "workflow//./src/workflows/pulse//pulseRemoteWorkflow",
        )
        .unwrap();

        assert_eq!(result.short_name, "pulseRemoteWorkflow");
    }

    #[test]
    fn workflow_observability_reexports_parse_class_name_and_it_works() {
        let result = observability::parse_class_name("class//./src/models//MyModel").unwrap();

        assert_eq!(result.short_name, "MyModel");
    }

    #[test]
    fn workflow_observability_reexports_observability_revivers() {
        let revivers = observability::observability_revivers();

        assert!(revivers.contains("ReadableStream"));
        assert!(revivers.contains("WritableStream"));
        assert!(revivers.contains("StepFunction"));
    }

    #[test]
    fn workflow_observability_reexports_hydrate_resource_io_and_handles_plain_values() {
        let step = json!({ "stepId": "test", "input": "hello", "output": 42 });
        let result =
            observability::hydrate_resource_io(step, observability::observability_revivers());

        assert_eq!(result["input"], json!("hello"));
        assert_eq!(result["output"], json!(42));
    }

    #[test]
    fn workflow_observability_reexports_hydrate_data_and_passes_through_plain_values() {
        let revivers = observability::observability_revivers();

        assert_eq!(
            observability::hydrate_data(json!("hello"), revivers),
            json!("hello")
        );
        assert_eq!(observability::hydrate_data(json!(42), revivers), json!(42));
        assert_eq!(
            observability::hydrate_data(json!(null), revivers),
            json!(null)
        );
    }

    #[test]
    fn workflow_stdlib_fetch_has_the_correct_name() {
        assert_eq!(fetch().name(), "fetch");
    }
}
