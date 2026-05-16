#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

/// The crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod embedding_model;
pub mod file_data;
pub mod files;
pub mod headers;
pub mod image_model;
pub mod json;
pub mod language_model;
pub mod provider;
pub mod reranking_model;
pub mod skills;
pub mod speech_model;
pub mod transcription_model;
pub mod video_model;
pub mod warning;

pub use embedding_model::{
    EmbeddingModelCallOptions, EmbeddingModelEmbedding, EmbeddingModelResponse,
    EmbeddingModelResult, EmbeddingModelUsage,
};
pub use file_data::{
    FileData, FileDataContent, NoSuchProviderReferenceError, ProviderReference,
    ProviderReferenceError,
};
pub use files::{FilesUploadFileCallOptions, FilesUploadFileData, FilesUploadFileResult};
pub use headers::Headers;
pub use image_model::{
    ImageModelCallOptions, ImageModelFile, ImageModelImage, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResult, ImageModelUsage,
};
pub use json::{
    JsonArray, JsonObject, JsonSchema, JsonValue, NonNullJsonValue, NullJsonValueError,
};
pub use language_model::{
    FinishReason, InputTokenUsage, LanguageModelAssistantContentPart,
    LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelContent,
    LanguageModelCustomContent, LanguageModelCustomPart, LanguageModelDocumentSource,
    LanguageModelErrorStreamPart, LanguageModelFile, LanguageModelFileData, LanguageModelFilePart,
    LanguageModelFinishReason, LanguageModelFunctionTool, LanguageModelGenerateResult,
    LanguageModelMessage, LanguageModelPrompt, LanguageModelProviderTool,
    LanguageModelRawStreamPart, LanguageModelReasoning, LanguageModelReasoningDelta,
    LanguageModelReasoningEffort, LanguageModelReasoningEnd, LanguageModelReasoningFile,
    LanguageModelReasoningFilePart, LanguageModelReasoningPart, LanguageModelReasoningStart,
    LanguageModelRequest, LanguageModelResponse, LanguageModelResponseFormat,
    LanguageModelResponseMetadata, LanguageModelSource, LanguageModelStreamFinish,
    LanguageModelStreamPart, LanguageModelStreamResponseMetadata, LanguageModelStreamStart,
    LanguageModelSystemMessage, LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd,
    LanguageModelTextPart, LanguageModelTextStart, LanguageModelTool,
    LanguageModelToolApprovalRequest, LanguageModelToolApprovalResponsePart, LanguageModelToolCall,
    LanguageModelToolCallPart, LanguageModelToolChoice, LanguageModelToolContentPart,
    LanguageModelToolInputDelta, LanguageModelToolInputEnd, LanguageModelToolInputExample,
    LanguageModelToolInputStart, LanguageModelToolMessage, LanguageModelToolResult,
    LanguageModelToolResultContentPart, LanguageModelToolResultCustomContent,
    LanguageModelToolResultOutput, LanguageModelToolResultPart, LanguageModelUrlSource,
    LanguageModelUsage, LanguageModelUserContentPart, LanguageModelUserMessage, OutputTokenUsage,
};
pub use provider::{
    LoadApiKeyError, ModelType, NoSuchModelError, ProviderMetadata, ProviderOptions,
    TooManyEmbeddingValuesForCallError, UnsupportedFunctionalityError,
};
pub use reranking_model::{
    RerankingModelCallOptions, RerankingModelDocuments, RerankingModelRanking,
    RerankingModelResponse, RerankingModelResult,
};
pub use skills::{
    SkillsFile, SkillsFileData, SkillsUploadSkillCallOptions, SkillsUploadSkillResult,
};
pub use speech_model::{
    SpeechModelAudio, SpeechModelCallOptions, SpeechModelRequest, SpeechModelResponse,
    SpeechModelResult,
};
pub use transcription_model::{
    TranscriptionModelCallOptions, TranscriptionModelRequest, TranscriptionModelResponse,
    TranscriptionModelResult, TranscriptionModelSegment,
};
pub use video_model::{
    VideoModelCallOptions, VideoModelFile, VideoModelResponse, VideoModelResult,
    VideoModelVideoData,
};
pub use warning::Warning;

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn exposes_crate_version() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    }
}
