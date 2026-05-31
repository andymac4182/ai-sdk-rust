//! World interface crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/world`. The first concrete contract is
//! intentionally small: enough for local world implementations to advertise the
//! shared lifecycle surface without forcing the rest of the runtime to stabilize
//! before storage and queue parity can land.

#![forbid(unsafe_code)]

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

/// Current upstream workflow world spec version used by new local runs.
pub const SPEC_VERSION_CURRENT: u32 = 2;

/// Minimal lifecycle contract shared by world implementations.
pub trait World {
    /// Error type returned by this world implementation.
    type Error;

    /// Spec version emitted for new entities.
    fn spec_version(&self) -> u32 {
        SPEC_VERSION_CURRENT
    }

    /// Prepare the world for use.
    fn start(&self) -> Result<(), Self::Error>;

    /// Release implementation resources.
    fn close(&self) -> Result<(), Self::Error>;
}

/// Worlds that can clear their backing state for deterministic local tests.
pub trait ClearableWorld: World {
    /// Remove persisted workflow data owned by this world.
    fn clear(&self) -> Result<(), Self::Error>;
}

/// Worlds that can recover active runs after a restart.
pub trait RecoverableWorld: World {
    /// Re-enqueue persisted pending/running runs.
    fn recover_active_runs(&self) -> Result<usize, Self::Error>;
}

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
    fn exposes_world_lifecycle_traits() {
        struct FakeWorld;

        impl World for FakeWorld {
            type Error = core::convert::Infallible;

            fn start(&self) -> Result<(), Self::Error> {
                Ok(())
            }

            fn close(&self) -> Result<(), Self::Error> {
                Ok(())
            }
        }

        impl ClearableWorld for FakeWorld {
            fn clear(&self) -> Result<(), Self::Error> {
                Ok(())
            }
        }

        impl RecoverableWorld for FakeWorld {
            fn recover_active_runs(&self) -> Result<usize, Self::Error> {
                Ok(0)
            }
        }

        let world = FakeWorld;
        assert_eq!(world.spec_version(), SPEC_VERSION_CURRENT);
        assert_eq!(world.recover_active_runs(), Ok(0));
        assert_eq!(world.clear(), Ok(()));
    }
}
