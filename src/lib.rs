#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

/// The crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod file_data;
pub mod json;
pub mod language_model;
pub mod provider;

pub use file_data::{FileData, FileDataContent, ProviderReference, ProviderReferenceError};
pub use json::{JsonArray, JsonObject, JsonValue};
pub use language_model::{
    FinishReason, InputTokenUsage, LanguageModelFinishReason, LanguageModelUsage, OutputTokenUsage,
};
pub use provider::{ProviderMetadata, ProviderOptions};

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn exposes_crate_version() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    }
}
