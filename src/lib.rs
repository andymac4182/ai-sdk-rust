#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

/// The crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod embedding_model;
pub mod file_data;
pub mod headers;
pub mod json;
pub mod language_model;
pub mod provider;
pub mod warning;

pub use embedding_model::{
    EmbeddingModelCallOptions, EmbeddingModelEmbedding, EmbeddingModelResponse,
    EmbeddingModelResult, EmbeddingModelUsage,
};
pub use file_data::{FileData, FileDataContent, ProviderReference, ProviderReferenceError};
pub use headers::Headers;
pub use json::{
    JsonArray, JsonObject, JsonSchema, JsonValue, NonNullJsonValue, NullJsonValueError,
};
pub use language_model::{
    FinishReason, InputTokenUsage, LanguageModelAssistantContentPart,
    LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelContent,
    LanguageModelCustomContent, LanguageModelCustomPart, LanguageModelDocumentSource,
    LanguageModelFile, LanguageModelFileData, LanguageModelFilePart, LanguageModelFinishReason,
    LanguageModelFunctionTool, LanguageModelMessage, LanguageModelPrompt,
    LanguageModelProviderTool, LanguageModelReasoning, LanguageModelReasoningEffort,
    LanguageModelReasoningFile, LanguageModelReasoningFilePart, LanguageModelReasoningPart,
    LanguageModelResponseFormat, LanguageModelResponseMetadata, LanguageModelSource,
    LanguageModelSystemMessage, LanguageModelText, LanguageModelTextPart, LanguageModelTool,
    LanguageModelToolApprovalRequest, LanguageModelToolApprovalResponsePart, LanguageModelToolCall,
    LanguageModelToolCallPart, LanguageModelToolChoice, LanguageModelToolContentPart,
    LanguageModelToolInputExample, LanguageModelToolMessage, LanguageModelToolResult,
    LanguageModelToolResultContentPart, LanguageModelToolResultCustomContent,
    LanguageModelToolResultOutput, LanguageModelToolResultPart, LanguageModelUrlSource,
    LanguageModelUsage, LanguageModelUserContentPart, LanguageModelUserMessage, OutputTokenUsage,
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
