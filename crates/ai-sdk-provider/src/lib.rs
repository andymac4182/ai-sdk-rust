//! Provider contracts for the Rust port of upstream `@ai-sdk/provider`.

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
    EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelEmbedding, EmbeddingModelResponse,
    EmbeddingModelResult, EmbeddingModelUsage,
};
pub use file_data::{
    FileData, FileDataContent, NoSuchProviderReferenceError, ProviderReference,
    ProviderReferenceError,
};
pub use files::{Files, FilesUploadFileCallOptions, FilesUploadFileData, FilesUploadFileResult};
pub use headers::Headers;
pub use image_model::{
    ImageModel, ImageModelCallOptions, ImageModelFile, ImageModelImage, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResponseMetadata,
    ImageModelResult, ImageModelUsage, NoImageGeneratedError,
};
pub use json::{
    JsonArray, JsonObject, JsonSchema, JsonValue, NonNullJsonValue, NullJsonValueError,
    is_json_array, is_json_object, is_json_value,
};
pub use language_model::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelAbortController,
    LanguageModelAbortSignal, LanguageModelAssistantContentPart, LanguageModelAssistantMessage,
    LanguageModelCallOptions, LanguageModelContent, LanguageModelCustomContent,
    LanguageModelCustomPart, LanguageModelDocumentSource, LanguageModelErrorStreamPart,
    LanguageModelFile, LanguageModelFileData, LanguageModelFilePart, LanguageModelFinishReason,
    LanguageModelFunctionTool, LanguageModelGenerateResult, LanguageModelMessage,
    LanguageModelPrompt, LanguageModelProviderTool, LanguageModelRawStreamPart,
    LanguageModelReasoning, LanguageModelReasoningDelta, LanguageModelReasoningEffort,
    LanguageModelReasoningEnd, LanguageModelReasoningFile, LanguageModelReasoningFilePart,
    LanguageModelReasoningPart, LanguageModelReasoningStart, LanguageModelRequest,
    LanguageModelResponse, LanguageModelResponseFormat, LanguageModelResponseMetadata,
    LanguageModelSource, LanguageModelStreamFinish, LanguageModelStreamPart,
    LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
    LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
    LanguageModelSystemMessage, LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd,
    LanguageModelTextPart, LanguageModelTextStart, LanguageModelTool,
    LanguageModelToolApprovalRequest, LanguageModelToolApprovalRequestPart,
    LanguageModelToolApprovalResponsePart, LanguageModelToolCall, LanguageModelToolCallPart,
    LanguageModelToolChoice, LanguageModelToolContentPart, LanguageModelToolInputDelta,
    LanguageModelToolInputEnd, LanguageModelToolInputExample, LanguageModelToolInputStart,
    LanguageModelToolMessage, LanguageModelToolResult, LanguageModelToolResultContentPart,
    LanguageModelToolResultCustomContent, LanguageModelToolResultOutput,
    LanguageModelToolResultPart, LanguageModelUrlSource, LanguageModelUsage,
    LanguageModelUserContentPart, LanguageModelUserMessage, OutputTokenUsage,
    ProviderAbortController, ProviderAbortSignal,
};
pub use provider::{
    ApiCallError, EmptyResponseBodyError, InvalidArgumentError, InvalidPromptError,
    InvalidResponseDataError, JsonParseError, LoadApiKeyError, LoadSettingError, ModelType,
    NoContentGeneratedError, NoSuchModelError, Provider, ProviderMetadata, ProviderOptions,
    ProviderWithFiles, ProviderWithRerankingModel, ProviderWithSkills, ProviderWithSpeechModel,
    ProviderWithTranscriptionModel, ProviderWithVideoModel, SpecificationVersion,
    TooManyEmbeddingValuesForCallError, TypeValidationContext, TypeValidationError,
    UnsupportedFunctionalityError, get_error_message,
};
pub use reranking_model::{
    RerankingModel, RerankingModelCallOptions, RerankingModelDocuments, RerankingModelRanking,
    RerankingModelResponse, RerankingModelResult,
};
pub use skills::{
    Skills, SkillsFile, SkillsFileData, SkillsUploadSkillCallOptions, SkillsUploadSkillResult,
};
pub use speech_model::{
    NoSpeechGeneratedError, SpeechModel, SpeechModelAudio, SpeechModelCallOptions,
    SpeechModelRequest, SpeechModelResponse, SpeechModelResponseMetadata, SpeechModelResult,
};
pub use transcription_model::{
    NoTranscriptGeneratedError, TranscriptionModel, TranscriptionModelCallOptions,
    TranscriptionModelRequest, TranscriptionModelResponse, TranscriptionModelResponseMetadata,
    TranscriptionModelResult, TranscriptionModelSegment,
};
pub use video_model::{
    ExperimentalVideoModel, ExperimentalVideoModelCallOptions, ExperimentalVideoModelFile,
    ExperimentalVideoModelResult, ExperimentalVideoModelVideoData, NoVideoGeneratedError,
    VideoModel, VideoModelCallOptions, VideoModelFile, VideoModelResponse,
    VideoModelResponseMetadata, VideoModelResult, VideoModelVideoData,
};
pub use warning::Warning;
