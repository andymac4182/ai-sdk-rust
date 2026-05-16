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
pub mod prompt;
pub mod provider;
pub mod provider_utils;
pub mod registry;
pub mod reranking_model;
pub mod retry;
pub mod skills;
pub mod speech_model;
pub mod transcription_model;
pub mod util;
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
    ActiveTools, CollectToolApprovalsError, CollectedToolApproval, CollectedToolApprovals,
    ContentPart, DefaultGeneratedFile, DynamicToolCall, DynamicToolError, DynamicToolResult,
    ExperimentalGeneratedImage, GenerateTextContentPart, GenerateTextEndEvent,
    GenerateTextFileContent, GenerateTextFinishEvent, GenerateTextInclude, GenerateTextModelInfo,
    GenerateTextOnFinish, GenerateTextOnFinishCallback, GenerateTextOnFinishFunction,
    GenerateTextOnFinishFuture, GenerateTextOnLanguageModelCallEnd,
    GenerateTextOnLanguageModelCallEndFunction, GenerateTextOnLanguageModelCallEndFuture,
    GenerateTextOnLanguageModelCallStart, GenerateTextOnLanguageModelCallStartFunction,
    GenerateTextOnLanguageModelCallStartFuture, GenerateTextOnStart, GenerateTextOnStartCallback,
    GenerateTextOnStartFunction, GenerateTextOnStartFuture, GenerateTextOnStepFinish,
    GenerateTextOnStepFinishCallback, GenerateTextOnStepFinishFunction,
    GenerateTextOnStepFinishFuture, GenerateTextOnStepStart, GenerateTextOnStepStartCallback,
    GenerateTextOnStepStartFunction, GenerateTextOnStepStartFuture, GenerateTextOnToolExecutionEnd,
    GenerateTextOnToolExecutionEndFunction, GenerateTextOnToolExecutionEndFuture,
    GenerateTextOnToolExecutionStart, GenerateTextOnToolExecutionStartFunction,
    GenerateTextOnToolExecutionStartFuture, GenerateTextOptions, GenerateTextReasoning,
    GenerateTextResult, GenerateTextStartEvent, GenerateTextStep, GenerateTextStepEndEvent,
    GenerateTextStepPerformance, GenerateTextStepStartEvent, GenerateTextTool,
    GenerateTextToolCall, GenerateTextToolError, GenerateTextToolExecutionEndEvent,
    GenerateTextToolExecutionStartEvent, GenerateTextToolOutputDenied, GenerateTextToolResult,
    GeneratedFile, GenericToolApprovalFunction, GenericToolApprovalOptions, InvalidStreamPartError,
    InvalidToolApprovalError, InvalidToolInputError, LanguageModelCallEndEvent,
    LanguageModelCallPerformance, LanguageModelCallStartEvent, MissingToolResultsError, ModelInfo,
    NoObjectGeneratedError, NoOutputGeneratedError, NoSuchToolError, NormalizedToolApprovalStatus,
    OnLanguageModelCallEndCallback, OnLanguageModelCallStartCallback, OnToolExecutionEndCallback,
    OnToolExecutionStartCallback, PrepareStep, PrepareStepFunction, PrepareStepFuture,
    PrepareStepOptions, PrepareStepResult, PruneEmptyMessages, PruneMessagesOptions,
    PruneReasoning, PruneToolCallRule, PruneToolCallRuleMode, PruneToolCalls, ReasoningFileOutput,
    ReasoningOutput, ResolveToolApprovalOptions, SingleToolApprovalFunction,
    SingleToolApprovalOptions, StaticToolCall, StaticToolError, StaticToolOutputDenied,
    StaticToolResult, StopCondition, ToolApprovalConfiguration, ToolApprovalFuture,
    ToolApprovalRequestOutput, ToolApprovalResponseOutput, ToolApprovalStatus,
    ToolApprovalStatusKind, ToolCallNotFoundForApprovalError, ToolCallRepair, ToolCallRepairError,
    ToolCallRepairFunction, ToolCallRepairFuture, ToolCallRepairOptions,
    ToolCallRepairOriginalError, ToolExecutionEndEvent, ToolExecutionStartEvent,
    ToolInputRefinement, ToolInputRefinementError, ToolInputRefinementFunction,
    ToolInputRefinementFuture, TypedToolCall, TypedToolError, TypedToolOutputDenied,
    TypedToolResult, UiMessageStreamError, UnsupportedModelVersionError, collect_tool_approvals,
    experimental_filter_active_tools, filter_active_tools, generate_text, has_tool_call,
    is_loop_finished, is_step_count, is_stop_condition_met, normalize_tool_approval_status,
    prune_messages, resolve_tool_approval, step_count_is,
};
#[allow(deprecated)]
pub use generate_text::{
    OnFinishEvent, OnStartEvent, OnStepFinishEvent, OnStepStartEvent, OnToolCallFinishEvent,
    OnToolCallStartEvent,
};
pub use headers::Headers;
pub use image_model::{
    ImageModel, ImageModelCallOptions, ImageModelFile, ImageModelImage, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResult, ImageModelUsage,
    NoImageGeneratedError,
};
pub use image_model_middleware::{
    ImageModelDoGenerate, ImageModelMiddleware, ImageModelMiddlewareModelOptions,
    ImageModelTransformParamsOptions, ImageModelWrapGenerateOptions,
};
pub use json::{
    JsonArray, JsonObject, JsonSchema, JsonValue, NonNullJsonValue, NullJsonValueError,
    is_json_array, is_json_object, is_json_value,
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
    LanguageModelToolApprovalRequest, LanguageModelToolApprovalRequestPart,
    LanguageModelToolApprovalResponsePart, LanguageModelToolCall, LanguageModelToolCallPart,
    LanguageModelToolChoice, LanguageModelToolContentPart, LanguageModelToolInputDelta,
    LanguageModelToolInputEnd, LanguageModelToolInputExample, LanguageModelToolInputStart,
    LanguageModelToolMessage, LanguageModelToolResult, LanguageModelToolResultContentPart,
    LanguageModelToolResultCustomContent, LanguageModelToolResultOutput,
    LanguageModelToolResultPart, LanguageModelUrlSource, LanguageModelUsage,
    LanguageModelUserContentPart, LanguageModelUserMessage, OutputTokenUsage,
};
pub use language_model_middleware::{
    LanguageModelDoGenerate, LanguageModelDoStream, LanguageModelMiddleware,
    LanguageModelMiddlewareCallType, LanguageModelMiddlewareModelOptions,
    LanguageModelTransformParamsOptions, LanguageModelWrapGenerateOptions,
    LanguageModelWrapStreamOptions,
};
pub use prompt::{
    Instructions, InvalidDataContentError, InvalidMessageRoleError, MessageConversionError, Prompt,
    PromptInput, PromptSource, RequestOptions, StandardizedPrompt, TimeoutConfiguration,
    TimeoutConfigurationOptions, convert_data_content_to_base64_string, get_chunk_timeout_ms,
    get_step_timeout_ms, get_tool_timeout_ms, get_total_timeout_ms, standardize_prompt,
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
pub use provider_utils::{
    Arrayable, Base64DecodeError, BinaryResponseHandlerOptions, ConvertToFormDataOptions,
    DEFAULT_MAX_DOWNLOAD_SIZE, DelayedPromise, DelayedPromiseFuture, DownloadBlobOptions,
    DownloadBlobResponse, DownloadError, DownloadedBlob, EventSourceResponseHandlerOptions,
    FetchErrorInfo, FlexibleSchema, FormData, FormDataEntry, FormDataInputValue, FormDataValue,
    GetFromApiOptions, HandledFetchError, IdGeneratorOptions,
    InjectJsonInstructionIntoMessagesOptions, InlineFileDataBytesError,
    JsonErrorResponseHandlerOptions, JsonResponseHandlerOptions, LazySchema, LoadApiKeyOptions,
    LoadOptionalSettingOptions, LoadSettingOptions, ParseJsonError, ParseJsonResult,
    PostJsonToApiOptions, PostToApiOptions, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, ProviderApiResponseBody,
    ProviderApiResponseHandlerError, ProviderDefinedToolFactory, ProviderExecutedToolFactory,
    ReasoningLevel, Resolvable, ResolvableFunction, ResolvableFuture, ResponseHandlerResult,
    RuntimeEnvironment, Schema, SerializedModelOptions, StatusCodeErrorResponseHandlerOptions,
    StreamingToolCallDelta, StreamingToolCallDeltaFunction, StreamingToolCallTracker,
    StreamingToolCallTrackerOptions, StreamingToolCallTypeValidation, Tool, ToolExecuteFunction,
    ToolExecuteFuture, ToolExecutionError, ToolExecutionOptions, ToolNameMapping,
    ValidateTypesResult, ValidationResult, add_additional_properties_to_json_schema, as_array,
    as_flexible_schema, as_schema, combine_headers, convert_base64_to_bytes,
    convert_bytes_to_base64, convert_image_model_file_to_data_uri,
    convert_inline_file_data_to_bytes, convert_to_base64, convert_to_form_data,
    create_binary_response_handler, create_event_source_response_handler, create_id_generator,
    create_json_error_response_handler, create_json_response_handler,
    create_provider_defined_tool_factory, create_provider_defined_tool_factory_with_output_schema,
    create_provider_executed_tool_factory, create_status_code_error_response_handler,
    create_tool_name_mapping, delay, detect_media_type, download_blob, dynamic_tool,
    execute_provider_api_request, extract_response_headers, filter_nullable, generate_id,
    get_from_api, get_runtime_environment_user_agent, get_top_level_media_type, handle_fetch_error,
    handle_provider_api_response, inject_json_instruction_into_messages, is_abort_error,
    is_custom_reasoning, is_full_media_type, is_non_nullable, is_parsable_json,
    is_provider_reference, is_url_supported, json_schema, lazy_json_schema, lazy_schema,
    load_api_key, load_optional_setting, load_setting, map_reasoning_to_provider_budget,
    map_reasoning_to_provider_effort, media_type_to_extension, normalize_headers, parse_json,
    parse_json_event_stream, parse_json_with_schema, parse_provider_options, post_json_to_api,
    post_to_api, prepare_get_from_api_request, prepare_post_json_to_api_request,
    prepare_post_to_api_request, prepare_tools, read_response_with_size_limit,
    remove_undefined_entries, resolve, resolve_full_media_type, resolve_provider_reference,
    safe_parse_json, safe_parse_json_with_schema, safe_validate_types, serialize_model_options,
    strip_file_extension, validate_download_url, validate_types, with_provider_utils_user_agent,
    with_user_agent_suffix, without_trailing_slash,
};
pub use registry::{
    NoSuchProviderError, ProviderRegistry, ProviderRegistryError, ProviderRegistryOptions,
    create_provider_registry, create_provider_registry_with_options, split_registry_model_id,
};
pub use reranking_model::{
    RerankingModel, RerankingModelCallOptions, RerankingModelDocuments, RerankingModelRanking,
    RerankingModelResponse, RerankingModelResult,
};
pub use retry::{
    DEFAULT_INITIAL_RETRY_DELAY_MS, DEFAULT_MAX_RETRIES, DEFAULT_RETRY_BACKOFF_FACTOR,
    RetryAttemptError, RetryError, RetryErrorReason, RetryFailure,
    RetryWithExponentialBackoffOptions, get_retry_delay_in_ms, retry_delay_from_response_headers,
    retry_with_exponential_backoff_respecting_retry_headers,
};
pub use skills::{
    Skills, SkillsFile, SkillsFileData, SkillsUploadSkillCallOptions, SkillsUploadSkillResult,
};
pub use speech_model::{
    NoSpeechGeneratedError, SpeechModel, SpeechModelAudio, SpeechModelCallOptions,
    SpeechModelRequest, SpeechModelResponse, SpeechModelResult,
};
pub use transcription_model::{
    NoTranscriptGeneratedError, TranscriptionModel, TranscriptionModelCallOptions,
    TranscriptionModelRequest, TranscriptionModelResponse, TranscriptionModelResult,
    TranscriptionModelSegment,
};
pub use util::{
    DataUrlTextError, InvalidArgumentError as AiInvalidArgumentError, ParsePartialJsonResult,
    ParsePartialJsonState, cosine_similarity, get_text_from_data_url, is_deep_equal_data,
    parse_partial_json,
};
pub use video_model::{
    NoVideoGeneratedError, VideoModel, VideoModelCallOptions, VideoModelFile, VideoModelResponse,
    VideoModelResponseMetadata, VideoModelResult, VideoModelVideoData,
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
