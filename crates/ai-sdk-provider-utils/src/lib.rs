//! Provider utility helpers for the Rust port of upstream `@ai-sdk/provider-utils`.

#![forbid(unsafe_code)]

/// The provider-utils crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

mod provider_utils;

pub use provider_utils::*;
