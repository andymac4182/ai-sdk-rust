//! Serialization marker crate for the standalone Vercel Workflow SDK Rust port.
//!
//! Upstream `packages/serde` exports two global JavaScript symbols:
//! `Symbol.for("workflow-serialize")` and `Symbol.for("workflow-deserialize")`.
//! Rust has no JavaScript global symbol registry, so this crate owns the stable
//! marker names and keeps the boundary distinct for core serialization code.

#![forbid(unsafe_code)]

/// Stable marker name corresponding to upstream `WORKFLOW_SERIALIZE`.
pub const WORKFLOW_SERIALIZE: &str = "workflow-serialize";

/// Stable marker name corresponding to upstream `WORKFLOW_DESERIALIZE`.
pub const WORKFLOW_DESERIALIZE: &str = "workflow-deserialize";

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the current port.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/serde";

/// Upstream package version inventoried for this port.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.2";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_serde_symbol_boundary() {
        assert_eq!(WORKFLOW_SERIALIZE, "workflow-serialize");
        assert_eq!(WORKFLOW_DESERIALIZE, "workflow-deserialize");
        assert_eq!(UPSTREAM_PACKAGE, "@workflow/serde");
        assert_eq!(UPSTREAM_HEAD.len(), 40);
    }
}
