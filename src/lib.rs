#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

/// The crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod file_data;
pub mod headers;
pub mod json;
pub mod language_model;
pub mod provider;
pub mod warning;

pub use file_data::{FileData, FileDataContent, ProviderReference, ProviderReferenceError};
pub use headers::Headers;
pub use json::{JsonArray, JsonObject, JsonValue, NonNullJsonValue, NullJsonValueError};
pub use language_model::{
    FinishReason, InputTokenUsage, LanguageModelCustomContent, LanguageModelDocumentSource,
    LanguageModelFile, LanguageModelFileData, LanguageModelFinishReason, LanguageModelReasoning,
    LanguageModelReasoningFile, LanguageModelResponseMetadata, LanguageModelSource,
    LanguageModelText, LanguageModelToolApprovalRequest, LanguageModelToolCall,
    LanguageModelToolChoice, LanguageModelToolResult, LanguageModelUrlSource, LanguageModelUsage,
    OutputTokenUsage,
};
pub use provider::{ProviderMetadata, ProviderOptions};
pub use warning::Warning;

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn exposes_crate_version() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    }
}
