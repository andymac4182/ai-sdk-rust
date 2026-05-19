//! Open Responses provider helpers for the Rust port of upstream
//! `@ai-sdk/open-responses`.

#![forbid(unsafe_code)]

/// The Open Responses crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod open_responses;

pub use open_responses::{
    OpenResponsesLanguageModel, OpenResponsesProvider, OpenResponsesProviderSettings,
    OpenResponsesTransport, OpenResponsesTransportFuture, create_open_responses,
};
