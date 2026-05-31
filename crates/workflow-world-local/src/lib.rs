//! Local World crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/world-local`. It currently records the
//! package boundary and re-exports the shared World skeleton until local queue,
//! storage, streaming, telemetry, and filesystem behavior are ported.

#![forbid(unsafe_code)]

pub use workflow_world as world;

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the initial crate skeleton.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/world-local";

/// Upstream package version inventoried for this skeleton.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.11";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
