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
    Arrayable, Base64DecodeError, DEFAULT_MAX_DOWNLOAD_SIZE, DownloadError,
    InjectJsonInstructionIntoMessagesOptions, InlineFileDataBytesError, LoadApiKeyOptions,
    LoadOptionalSettingOptions, LoadSettingOptions, ParseJsonError, ParseJsonResult,
    ReasoningLevel, Tool, ToolExecuteFunction, ToolExecuteFuture, ToolExecutionError,
    ToolExecutionOptions, ToolNameMapping, ValidateTypesResult,
    add_additional_properties_to_json_schema, as_array, combine_headers, convert_base64_to_bytes,
    convert_bytes_to_base64, convert_image_model_file_to_data_uri,
    convert_inline_file_data_to_bytes, convert_to_base64, create_tool_name_mapping,
    detect_media_type, extract_response_headers, filter_nullable, get_top_level_media_type,
    inject_json_instruction_into_messages, is_custom_reasoning, is_full_media_type,
    is_non_nullable, is_parsable_json, is_provider_reference, is_url_supported, load_api_key,
    load_optional_setting, load_setting, map_reasoning_to_provider_budget,
    map_reasoning_to_provider_effort, media_type_to_extension, normalize_headers, parse_json,
    parse_provider_options, prepare_tools, read_response_with_size_limit, remove_undefined_entries,
    resolve_full_media_type, resolve_provider_reference, safe_parse_json, safe_validate_types,
    strip_file_extension, validate_download_url, validate_types, with_user_agent_suffix,
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
