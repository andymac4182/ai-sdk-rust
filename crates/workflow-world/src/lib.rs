//! World interface crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/world`. It owns cross-world run, step,
//! event, queue, hook, wait, stream, serialization, and World trait contracts.
//! Local, Postgres, and Vercel worlds implement these traits in their own crates.

#![forbid(unsafe_code)]

pub mod attributes;
pub mod data;
pub mod error;
pub mod events;
pub mod hooks;
pub mod interfaces;
pub mod queue;
pub mod recovery;
pub mod runs;
pub mod serialization;
pub mod spec_version;
pub mod steps;
pub mod ulid;
pub mod waits;

pub use attributes::*;
pub use data::*;
pub use error::*;
pub use events::*;
pub use hooks::*;
pub use interfaces::*;
pub use queue::*;
pub use recovery::*;
pub use runs::*;
pub use serialization::*;
pub use spec_version::*;
pub use steps::*;
pub use ulid::*;
pub use waits::*;

/// Worlds that can clear their backing state for deterministic local tests.
pub trait ClearableWorld {
    /// Error type returned by this world implementation.
    type Error;

    /// Remove persisted workflow data owned by this world.
    fn clear(&self) -> Result<(), Self::Error>;
}

/// Worlds that can recover active runs after a restart.
pub trait RecoverableWorld {
    /// Error type returned by this world implementation.
    type Error;

    /// Re-enqueue persisted pending/running runs.
    fn recover_active_runs(&self) -> Result<usize, Self::Error>;
}

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the initial crate skeleton.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/world";

/// Upstream package version inventoried for this skeleton.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.5";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_world_source_snapshot() {
        assert_eq!(UPSTREAM_PACKAGE, "@workflow/world");
        assert_eq!(UPSTREAM_VERSION, "5.0.0-beta.5");
        assert_eq!(UPSTREAM_HEAD.len(), 40);
    }

    #[test]
    fn exposes_world_local_lifecycle_extension_traits() {
        struct FakeWorld;

        impl ClearableWorld for FakeWorld {
            type Error = core::convert::Infallible;

            fn clear(&self) -> Result<(), Self::Error> {
                Ok(())
            }
        }

        impl RecoverableWorld for FakeWorld {
            type Error = core::convert::Infallible;

            fn recover_active_runs(&self) -> Result<usize, Self::Error> {
                Ok(0)
            }
        }

        let world = FakeWorld;
        assert_eq!(world.recover_active_runs(), Ok(0));
        assert_eq!(world.clear(), Ok(()));
    }
}
