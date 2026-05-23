//! Workspace runtime + HTTP client re-exports for chat-sdk adapters.
//!
//! This module exists so per-adapter crates depend on
//! `chat-sdk-adapter-shared` rather than reaching for `tokio` /
//! `reqwest` directly. Pinning the workspace runtime + HTTP-client
//! choice in one place keeps the adapter ecosystem coherent:
//!
//! - **Runtime**: `tokio` (single-threaded `current_thread` or
//!   multi-threaded `rt-multi-thread` both work; adapters that need
//!   `current_thread` can opt in via the upstream `tokio` crate
//!   directly when needed).
//! - **HTTP client**: `reqwest` (json + rustls features; no
//!   native-tls / native-tls-alpn / default-tls deps). 1:1 with
//!   upstream's `fetch`/`undici` semantic shape on the
//!   request/response surface.
//!
//! Adopters that need a different runtime or HTTP client can still
//! reach the underlying crates through their own Cargo.toml — these
//! re-exports are a convention, not a constraint.
//!
//! See the slice 143 refinement in
//! [`docs/chat/goal-refinements.md`](../../../docs/chat/goal-refinements.md)
//! for the rationale.

pub use reqwest;
pub use tokio;

/// Build a `reqwest::Client` with the chat-sdk defaults. 1:1 with
/// upstream's `new fetch()` helper at adapter callsites:
///
/// - 30-second total request timeout (matches upstream's
///   `fetch.timeout = 30_000`).
/// - Identifies as `chat-sdk-rust/<version>` via User-Agent.
/// - Connection pooling defaults to reqwest's defaults (good for
///   per-adapter request volume; production deployments tune via
///   their own [`reqwest::ClientBuilder`]).
pub fn default_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("chat-sdk-rust/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("reqwest::Client::build with chat-sdk defaults should never fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_http_client_builds_without_panicking() {
        let _client = default_http_client();
    }

    #[test]
    fn reqwest_reexport_is_at_the_expected_path() {
        // Compile-time check: the re-export gives access to
        // reqwest::Client without per-adapter Cargo.toml entries.
        let _: fn() -> reqwest::ClientBuilder = reqwest::Client::builder;
    }

    #[test]
    fn tokio_reexport_is_at_the_expected_path() {
        // Compile-time check: tokio::runtime is reachable.
        let _: fn() -> Result<tokio::runtime::Runtime, std::io::Error> = || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
        };
    }
}
