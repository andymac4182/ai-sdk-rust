//! Utility crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/utils`.

#![forbid(unsafe_code)]

mod check_data_dir;
mod duration;
mod name;
mod plural;
mod port;
mod promise;
mod world_target;

pub use check_data_dir::{
    POSSIBLE_WORKFLOW_DATA_PATHS, WorkflowDataDirInfo, find_workflow_data_dir, get_dir_short_name,
};
pub use duration::{DurationInput, DurationParseError, parse_duration_to_date};
pub use name::{
    ParsedName, format_step_name, format_workflow_name, parse_class_name, parse_step_name,
    parse_workflow_name,
};
pub use plural::pluralize;
pub use port::{ProbeOptions, get_all_ports, get_port, get_workflow_port};
pub use promise::{
    OnceValue, PromiseRecvError, PromiseSendError, PromiseWithResolvers, once, with_resolvers,
};
pub use world_target::{
    is_vercel_world_target, resolve_workflow_target_world, resolve_workflow_target_world_from_env,
    uses_vercel_world,
};

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the current port.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/utils";

/// Upstream package version inventoried for this port.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.3";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_re_exports_cases() {
        let step = parse_step_name("step//./src/workflows/order//processOrder").unwrap();
        assert_eq!(step.short_name, "processOrder");

        let workflow =
            parse_workflow_name("workflow//./src/workflows/pulse//pulseRemoteWorkflow").unwrap();
        assert_eq!(workflow.short_name, "pulseRemoteWorkflow");

        let class = parse_class_name("class//./src/models/point//Point").unwrap();
        assert_eq!(class.short_name, "Point");
    }

    #[test]
    fn parse_step_name_extracts_short_name() {
        let parsed = parse_step_name("step//./src/workflows/pulse//queryKBStep").unwrap();

        assert_eq!(parsed.short_name, "queryKBStep");
        assert_eq!(parsed.module_specifier, "./src/workflows/pulse");
        assert_eq!(parsed.function_name, "queryKBStep");
    }

    #[test]
    fn parse_default_export_uses_module_short_name() {
        let parsed = parse_workflow_name("workflow//point@0.0.1//default").unwrap();

        assert_eq!(parsed.short_name, "point");
    }
}
