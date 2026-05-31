//! Facade crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/workflow`. It is intentionally only a
//! skeleton in the inventory pass: package-owned behavior should land in the
//! matching crates and be re-exported here once the Rust contracts exist.

#![forbid(unsafe_code)]

pub use workflow_core as core;
pub use workflow_errors as errors;
pub use workflow_serde as serde;
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
