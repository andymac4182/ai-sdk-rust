#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

/// The crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod embedding_model;
pub mod embedding_model_middleware;
pub mod file_data;
pub mod files;
pub mod generate_text;
pub mod headers;
pub mod image_model;
pub mod image_model_middleware;
pub mod json;
pub mod language_model;
pub mod language_model_middleware;
pub mod provider;
pub mod provider_utils;
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
pub use embedding_model_middleware::{
    EmbeddingModelDoEmbed, EmbeddingModelMiddleware, EmbeddingModelMiddlewareModelOptions,
    EmbeddingModelTransformParamsOptions, EmbeddingModelWrapEmbedOptions,
};
pub use file_data::{
    FileData, FileDataContent, NoSuchProviderReferenceError, ProviderReference,
    ProviderReferenceError,
};
pub use files::{Files, FilesUploadFileCallOptions, FilesUploadFileData, FilesUploadFileResult};
pub use generate_text::{
    GenerateTextModelInfo, GenerateTextOptions, GenerateTextReasoning, GenerateTextResult,
    GenerateTextStep, GenerateTextTool, GenerateTextToolCall, GenerateTextToolResult,
    generate_text,
};
pub use headers::Headers;
pub use image_model::{
    ImageModel, ImageModelCallOptions, ImageModelFile, ImageModelImage, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResult, ImageModelUsage,
};
pub use image_model_middleware::{
    ImageModelDoGenerate, ImageModelMiddleware, ImageModelMiddlewareModelOptions,
    ImageModelTransformParamsOptions, ImageModelWrapGenerateOptions,
};
pub use json::{
    JsonArray, JsonObject, JsonSchema, JsonValue, NonNullJsonValue, NullJsonValueError,
};
pub use language_model::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelAssistantContentPart,
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
    LanguageModelStreamPart, LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
    LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
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
pub use language_model_middleware::{
    LanguageModelDoGenerate, LanguageModelDoStream, LanguageModelMiddleware,
    LanguageModelMiddlewareCallType, LanguageModelMiddlewareModelOptions,
    LanguageModelTransformParamsOptions, LanguageModelWrapGenerateOptions,
    LanguageModelWrapStreamOptions,
};
pub use provider::{
    ApiCallError, EmptyResponseBodyError, InvalidArgumentError, InvalidPromptError,
    InvalidResponseDataError, JsonParseError, LoadApiKeyError, LoadSettingError, ModelType,
    NoContentGeneratedError, NoSuchModelError, Provider, ProviderMetadata, ProviderOptions,
    ProviderWithFiles, ProviderWithRerankingModel, ProviderWithSkills, ProviderWithSpeechModel,
    ProviderWithTranscriptionModel, SpecificationVersion, TooManyEmbeddingValuesForCallError,
    TypeValidationContext, TypeValidationError, UnsupportedFunctionalityError, get_error_message,
};
pub use provider_utils::{
    Arrayable, InjectJsonInstructionIntoMessagesOptions, LoadApiKeyOptions,
    LoadOptionalSettingOptions, LoadSettingOptions, Tool, ToolExecuteFunction, ToolExecuteFuture,
    ToolExecutionError, ToolExecutionOptions, ToolNameMapping,
    add_additional_properties_to_json_schema, as_array, combine_headers, create_tool_name_mapping,
    detect_media_type, filter_nullable, get_top_level_media_type,
    inject_json_instruction_into_messages, is_full_media_type, is_non_nullable,
    is_provider_reference, load_api_key, load_optional_setting, load_setting,
    media_type_to_extension, normalize_headers, prepare_tools, remove_undefined_entries,
    resolve_provider_reference, strip_file_extension, with_user_agent_suffix,
    without_trailing_slash,
};
pub use reranking_model::{
    RerankingModel, RerankingModelCallOptions, RerankingModelDocuments, RerankingModelRanking,
    RerankingModelResponse, RerankingModelResult,
};
pub use skills::{
    Skills, SkillsFile, SkillsFileData, SkillsUploadSkillCallOptions, SkillsUploadSkillResult,
};
pub use speech_model::{
    SpeechModel, SpeechModelAudio, SpeechModelCallOptions, SpeechModelRequest, SpeechModelResponse,
    SpeechModelResult,
};
pub use transcription_model::{
    TranscriptionModel, TranscriptionModelCallOptions, TranscriptionModelRequest,
    TranscriptionModelResponse, TranscriptionModelResult, TranscriptionModelSegment,
};
pub use video_model::{
    VideoModel, VideoModelCallOptions, VideoModelFile, VideoModelResponse, VideoModelResult,
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
