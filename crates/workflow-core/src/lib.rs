//! Core runtime crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/core`. It now owns the portable
//! serialization, encryption, and VM utility contracts used by the standalone
//! workflow runtime. JavaScript-only runtime identity semantics such as
//! function/class constructors and Node `vm.Context` globals are documented in
//! the parity inventory instead of being over-claimed here.

#![forbid(unsafe_code)]

pub mod capabilities;
pub mod classify_error;
pub mod codec;
pub mod context_errors;
pub mod define_hook;
pub mod describe_error;
pub mod encryption;
pub mod error;
pub mod format;
pub mod global;
pub mod log_format;
pub mod logger;
pub mod observability;
pub mod ordering;
pub mod schemas;
pub mod set_attributes;
pub mod source_map;
pub mod stream;
pub mod types;
pub mod util;
pub mod value;
pub mod vm;

pub use workflow_errors as errors;
pub use workflow_serde as serde;
pub use workflow_world as world;

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
