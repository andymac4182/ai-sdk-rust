#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

/// The crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod agent;
pub mod baseten;
pub mod cerebras;
pub mod chat_transport;
pub mod completion_transport;
pub mod deepinfra;
pub mod embed;
pub mod embedding_model;
pub mod embedding_model_middleware;
pub mod file_data;
pub mod files;
pub mod gateway;
pub mod gateway_error;
pub mod gateway_tools;
pub mod generate_image;
pub mod generate_object;
pub mod generate_speech;
pub mod generate_text;
pub mod generate_video;
pub mod headers;
pub mod huggingface;
pub mod image_model;
pub mod image_model_middleware;
pub mod json;
pub mod language_model;
pub mod language_model_middleware;
pub mod logger;
pub mod mock_models;
pub mod object_transport;
pub mod open_responses;
pub mod openai;
pub mod openai_compatible;
pub mod prompt;
pub mod provider;
pub mod provider_middleware;
pub mod provider_utils;
pub mod registry;
pub mod rerank;
pub mod reranking_model;
pub mod resolve_model;
pub mod retry;
pub mod skills;
pub mod speech_model;
pub mod stream_object;
pub mod stream_text;
pub mod telemetry;
pub mod text_stream_response;
pub mod togetherai;
pub mod transcribe;
pub mod transcription_model;
pub mod ui_message_stream;
pub mod upload_file;
pub mod upload_skill;
pub mod util;
pub mod vercel;
pub mod vercel_ai_gateway;
pub mod video_model;
pub mod voyage;
pub mod warning;

pub use agent::{
    AgentUiStreamResponseOptions, TOOL_LOOP_AGENT_VERSION, ToolLoopAgent, ToolLoopAgentCallOptions,
    ToolLoopAgentModelSettings, ToolLoopAgentPrepareCall, ToolLoopAgentPreparedCall,
    ToolLoopAgentSettings, create_agent_ui_stream_response,
};
pub use baseten::{
    BasetenProvider, BasetenProviderSettings, DEFAULT_BASETEN_BASE_URL, baseten, create_baseten,
};
pub use cerebras::{
    CerebrasProvider, CerebrasProviderSettings, DEFAULT_CEREBRAS_BASE_URL, cerebras,
    create_cerebras,
};
pub use chat_transport::{
    Chat, ChatError, ChatMessageInput, ChatRequestOptions, ChatStatus, ChatTransport,
    ChatTransportError, ChatTransportReconnectOptions, ChatTransportSendOptions,
    ChatTransportTrigger, DefaultChatTransport, DirectChatTransport, DirectChatTransportOptions,
    HttpChatTransport, HttpChatTransportMethod, HttpChatTransportOptions, HttpChatTransportRequest,
    PrepareReconnectToStreamRequestOptions, PrepareSendMessagesRequestOptions,
    PreparedReconnectToStreamRequest, PreparedSendMessagesRequest, RequestCredentials,
    TextStreamChatTransport, convert_ui_messages_to_model_messages,
    convert_ui_messages_to_model_messages_with_tools,
};
pub use completion_transport::{
    CompletionRequestOptions, CompletionStreamProtocol, CompletionTransport,
    CompletionTransportError, CompletionTransportMethod, CompletionTransportOptions,
    CompletionTransportRequest, process_completion_data_event_stream,
    process_completion_text_stream,
};
pub use deepinfra::{
    DEFAULT_DEEPINFRA_BASE_URL, DeepInfraChatLanguageModel, DeepInfraImageModel, DeepInfraProvider,
    DeepInfraProviderSettings, create_deepinfra, deepinfra,
};
pub use embed::{
    EmbedEndEvent, EmbedEventEmbedding, EmbedEventResponse, EmbedEventValue, EmbedManyOptions,
    EmbedManyResult, EmbedOnEnd, EmbedOnEndCallback, EmbedOnEndFunction, EmbedOnEndFuture,
    EmbedOnStart, EmbedOnStartCallback, EmbedOnStartFunction, EmbedOnStartFuture, EmbedOptions,
    EmbedResult, EmbedStartEvent, Embedding, EmbeddingModelCallEndEvent,
    EmbeddingModelCallStartEvent, embed, embed_many,
};
pub use embedding_model::{
    EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelEmbedding, EmbeddingModelResponse,
    EmbeddingModelResult, EmbeddingModelUsage,
};
pub use embedding_model_middleware::{
    DefaultEmbeddingSettingsMiddleware, EmbeddingModelDefaultSettings, EmbeddingModelDoEmbed,
    EmbeddingModelMiddleware, EmbeddingModelMiddlewareModelOptions,
    EmbeddingModelTransformParamsOptions, EmbeddingModelWrapEmbedOptions, WrappedEmbeddingModel,
    default_embedding_settings_middleware, wrap_embedding_model,
};
pub use file_data::{
    FileData, FileDataContent, NoSuchProviderReferenceError, ProviderReference,
    ProviderReferenceError,
};
pub use files::{Files, FilesUploadFileCallOptions, FilesUploadFileData, FilesUploadFileResult};
pub use gateway::{
    DEFAULT_GATEWAY_BASE_URL, GatewayAuthToken, GatewayCredentialType, GatewayCreditsResponse,
    GatewayEmbeddingModel, GatewayFetchMetadataResponse, GatewayGenerationInfo,
    GatewayGenerationInfoParams, GatewayImageModel, GatewayLanguageModel,
    GatewayLanguageModelEntry, GatewayLanguageModelPricing, GatewayLanguageModelSpecification,
    GatewayModelType, GatewayProvider, GatewayProviderOptions, GatewayProviderOptionsSort,
    GatewayProviderOptionsValidationError, GatewayProviderSettings, GatewayProviderTimeouts,
    GatewayRerankingModel, GatewaySpendReportDatePart, GatewaySpendReportGroupBy,
    GatewaySpendReportParams, GatewaySpendReportResponse, GatewaySpendReportRow, GatewayTransport,
    GatewayTransportFuture, GatewayVideoModel, create_gateway, create_gateway_provider, gateway,
    gateway_observability_headers, gateway_provider_options, get_gateway_auth_token,
    try_gateway_provider_options,
};
pub use gateway_error::{
    GATEWAY_AUTH_METHOD_HEADER, GatewayAuthMethod, GatewayAuthenticationError, GatewayError,
    GatewayErrorResponse, GatewayErrorResponseError, GatewayInternalServerError,
    GatewayInvalidRequestError, GatewayModelNotFoundError, GatewayRateLimitError,
    GatewayResponseError, GatewayTimeoutError, as_gateway_error,
    create_gateway_error_from_api_call, create_gateway_error_from_response,
    extract_gateway_api_call_response, gateway_headers_from_auth_method, parse_gateway_auth_method,
};
pub use gateway_tools::{
    GatewayTools, ParallelSearchConfig, ParallelSearchError, ParallelSearchErrorType,
    ParallelSearchExcerpts, ParallelSearchFetchPolicy, ParallelSearchInput,
    ParallelSearchInputExcerpts, ParallelSearchInputFetchPolicy, ParallelSearchInputSourcePolicy,
    ParallelSearchMode, ParallelSearchOutput, ParallelSearchResponse, ParallelSearchResult,
    ParallelSearchSourcePolicy, PerplexitySearchConfig, PerplexitySearchError,
    PerplexitySearchErrorType, PerplexitySearchInput, PerplexitySearchOutput,
    PerplexitySearchQuery, PerplexitySearchRecencyFilter, PerplexitySearchResponse,
    PerplexitySearchResult, gateway_tools, parallel_search, parallel_search_tool_factory,
    perplexity_search, perplexity_search_tool_factory,
};
pub use generate_image::{
    ExperimentalGenerateImageResult, GenerateImageOptions, GenerateImagePrompt,
    GenerateImagePromptImage, GenerateImagePromptImages, GenerateImageResult,
    experimental_generate_image, generate_image,
};
pub use generate_object::{
    GenerateObjectEndEvent, GenerateObjectOnFinish, GenerateObjectOnFinishCallback,
    GenerateObjectOnFinishFunction, GenerateObjectOnFinishFuture, GenerateObjectOnStart,
    GenerateObjectOnStartCallback, GenerateObjectOnStartFunction, GenerateObjectOnStartFuture,
    GenerateObjectOnStepFinish, GenerateObjectOnStepFinishCallback,
    GenerateObjectOnStepFinishFunction, GenerateObjectOnStepFinishFuture,
    GenerateObjectOnStepStart, GenerateObjectOnStepStartCallback,
    GenerateObjectOnStepStartFunction, GenerateObjectOnStepStartFuture, GenerateObjectOptions,
    GenerateObjectOutputKind, GenerateObjectRepairText, GenerateObjectRepairTextFunction,
    GenerateObjectRepairTextFuture, GenerateObjectRepairTextOptions, GenerateObjectRequest,
    GenerateObjectResponse, GenerateObjectResult, GenerateObjectStartEvent,
    GenerateObjectStepEndEvent, GenerateObjectStepStartEvent, RepairTextFunction, generate_object,
};
pub use generate_speech::{
    DefaultGeneratedAudioFile, ExperimentalSpeechResult, GenerateSpeechOptions, GeneratedAudioFile,
    SpeechResult, experimental_generate_speech, generate_speech,
};
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
    ToolInputRefinementFuture, ToolModelOutputErrorMode, TypedToolCall, TypedToolError,
    TypedToolOutputDenied, TypedToolResult, UiMessageStreamError, UnsupportedModelVersionError,
    collect_tool_approvals, create_tool_model_output, experimental_filter_active_tools,
    filter_active_tools, generate_text, has_tool_call, is_loop_finished, is_step_count,
    is_stop_condition_met, normalize_tool_approval_status, prune_messages, resolve_tool_approval,
    step_count_is,
};
#[allow(deprecated)]
pub use generate_text::{
    OnFinishEvent, OnStartEvent, OnStepFinishEvent, OnStepStartEvent, OnToolCallFinishEvent,
    OnToolCallStartEvent,
};
pub use generate_video::{
    ExperimentalGenerateVideoResult, GenerateVideoDownload, GenerateVideoDownloadFunction,
    GenerateVideoDownloadFuture, GenerateVideoDownloadOptions, GenerateVideoError,
    GenerateVideoOptions, GenerateVideoPrompt, GenerateVideoPromptImage, GenerateVideoResult,
    experimental_generate_video, generate_video,
};
pub use headers::Headers;
pub use huggingface::{
    DEFAULT_HUGGINGFACE_BASE_URL, HuggingFaceProvider, HuggingFaceProviderSettings,
    HuggingFaceResponsesLanguageModel, HuggingFaceTransport, HuggingFaceTransportFuture,
    create_huggingface, huggingface,
};
pub use image_model::{
    ImageModel, ImageModelCallOptions, ImageModelFile, ImageModelImage, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResponseMetadata,
    ImageModelResult, ImageModelUsage, NoImageGeneratedError,
};
pub use image_model_middleware::{
    ImageModelDoGenerate, ImageModelMiddleware, ImageModelMiddlewareModelOptions,
    ImageModelTransformParamsOptions, ImageModelWrapGenerateOptions, WrappedImageModel,
    wrap_image_model,
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
pub use language_model_middleware::{
    AddToolInputExamplesMiddleware, DefaultSettingsMiddleware, ExtractJsonMiddleware,
    ExtractJsonTransformFunction, ExtractReasoningMiddleware, LanguageModelDefaultSettings,
    LanguageModelDoGenerate, LanguageModelDoStream, LanguageModelMiddleware,
    LanguageModelMiddlewareCallType, LanguageModelMiddlewareModelOptions,
    LanguageModelTransformParamsOptions, LanguageModelWrapGenerateOptions,
    LanguageModelWrapStreamOptions, SimulateStreamingMiddleware, ToolInputExampleFormatFunction,
    WrappedLanguageModel, add_tool_input_examples_middleware, default_extract_json_transform,
    default_format_tool_input_example, default_settings_middleware, extract_json_middleware,
    extract_reasoning_middleware, simulate_streaming_middleware, wrap_language_model,
};
pub use logger::{
    FIRST_WARNING_INFO_MESSAGE, LogWarningsOptions, WarningLogKind, WarningLogRecord,
    WarningLogger, format_warning, log_warnings, log_warnings_with_custom_logger,
    reset_log_warnings_state, set_log_warnings_enabled,
};
pub use mock_models::{
    MockEmbeddingModel, MockImageModel, MockLanguageModel, MockProvider, MockRerankingModel,
    MockSpeechModel, MockTranscriptionModel, MockVideoModel,
};
pub use object_transport::{
    ObjectRequestOptions, ObjectStreamResult, ObjectStreamUpdate, ObjectTransport,
    ObjectTransportMethod, ObjectTransportOptions, ObjectTransportRequest,
    parse_object_stream_final_json, process_object_text_stream,
};
pub use open_responses::{
    OpenResponsesLanguageModel, OpenResponsesProvider, OpenResponsesProviderSettings,
    OpenResponsesTransport, OpenResponsesTransportFuture, create_open_responses,
};
pub use openai::{
    DEFAULT_OPENAI_BASE_URL, OpenAIProvider, OpenAIProviderSettings, create_openai, openai,
    openai_local_shell_tool, openai_web_search_tool, openai_web_search_tool_with_args,
};
pub use openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleCompletionLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel, OpenAICompatibleModelEntry,
    OpenAICompatibleModelListResponse, OpenAICompatibleProvider, OpenAICompatibleProviderSettings,
    OpenAICompatibleTransport, OpenAICompatibleTransportFuture, create_openai_compatible,
};
pub use prompt::{
    ConvertedLanguageModelV4FilePart, Instructions, InvalidDataContentError,
    InvalidMessageRoleError, LanguageModelCallSettings, MessageConversionError, Prompt,
    PromptInput, PromptSource, RequestOptions, StandardizedPrompt, TimeoutConfiguration,
    TimeoutConfigurationOptions, convert_data_content_to_base64_string,
    convert_to_language_model_prompt, convert_to_language_model_v4_file_part, get_chunk_timeout_ms,
    get_step_timeout_ms, get_tool_timeout_ms, get_total_timeout_ms,
    prepare_language_model_call_options, prepare_tool_choice, standardize_prompt,
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
pub use provider_middleware::{
    WrappedProvider, WrappedProviderWithImageMiddleware, WrappedProviderWithImageModelMiddleware,
    wrap_provider, wrap_provider_with_image_middleware, wrap_provider_with_image_model_middleware,
};
pub use provider_utils::{
    Arrayable, Base64DecodeError, BinaryResponseHandlerOptions, ConvertToFormDataOptions,
    DEFAULT_MAX_DOWNLOAD_SIZE, DelayedPromise, DelayedPromiseFuture, DownloadBlobOptions,
    DownloadBlobResponse, DownloadError, DownloadedBlob, EventSourceResponseHandlerOptions,
    ExecuteToolOutput, ExperimentalSandbox, FetchErrorInfo, FilePart, FilePartData, FlexibleSchema,
    FormData, FormDataEntry, FormDataInputValue, FormDataValue, GetFromApiOptions,
    HandledFetchError, IdGeneratorOptions, InjectJsonInstructionIntoMessagesOptions,
    InlineFileDataBytesError, JsonErrorResponseHandlerOptions, JsonResponseHandlerOptions,
    LazySchema, LoadApiKeyOptions, LoadOptionalSettingOptions, LoadSettingOptions, ParseJsonError,
    ParseJsonResult, PostFormDataToApiOptions, PostJsonToApiOptions, PostToApiOptions,
    ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseBody, ProviderApiResponseHandlerError, ProviderDefinedToolFactory,
    ProviderExecutedToolFactory, ProviderReferenceOrString, ReasoningFilePart,
    ReasoningFilePartData, ReasoningLevel, Resolvable, ResolvableFunction, ResolvableFuture,
    ResponseHandlerResult, RuntimeEnvironment, SandboxCommandOptions, SandboxCommandResult,
    SandboxRunCommandFuture, Schema, SerializedModelOptions, StatusCodeErrorResponseHandlerOptions,
    StreamingToolCallDelta, StreamingToolCallDeltaFunction, StreamingToolCallTracker,
    StreamingToolCallTrackerOptions, StreamingToolCallTypeValidation, Tool, ToolApprovalRequest,
    ToolApprovalResponse, ToolCall, ToolDescriptionFunction, ToolDescriptionOptions,
    ToolExecuteFunction, ToolExecuteFuture, ToolExecutionError, ToolExecutionOptions,
    ToolInputAvailableFunction, ToolInputAvailableOptions, ToolInputCallbackFuture,
    ToolInputDeltaFunction, ToolInputDeltaOptions, ToolInputStartFunction, ToolModelOutputFunction,
    ToolModelOutputFuture, ToolModelOutputOptions, ToolNameMapping, ToolNeedsApprovalFunction,
    ToolNeedsApprovalFuture, ToolNeedsApprovalOptions, ToolResult, ToolResultContentPart,
    ToolResultOutput, ValidateTypesResult, ValidationResult,
    add_additional_properties_to_json_schema, as_array, as_flexible_schema, as_schema,
    combine_headers, convert_base64_to_bytes, convert_bytes_to_base64,
    convert_image_model_file_to_data_uri, convert_inline_file_data_to_bytes, convert_to_base64,
    convert_to_form_data, create_binary_response_handler, create_event_source_response_handler,
    create_id_generator, create_json_error_response_handler, create_json_response_handler,
    create_provider_defined_tool_factory, create_provider_defined_tool_factory_with_output_schema,
    create_provider_executed_tool_factory, create_status_code_error_response_handler,
    create_tool_name_mapping, delay, detect_media_type, download_blob, dynamic_tool,
    execute_provider_api_request, execute_tool, extract_response_headers, filter_nullable,
    generate_id, get_from_api, get_runtime_environment_user_agent, get_top_level_media_type,
    handle_fetch_error, handle_provider_api_response, inject_json_instruction_into_messages,
    is_abort_error, is_custom_reasoning, is_executable_tool, is_full_media_type, is_non_nullable,
    is_parsable_json, is_provider_reference, is_url_supported, json_schema, lazy_json_schema,
    lazy_schema, load_api_key, load_optional_setting, load_setting,
    map_reasoning_to_provider_budget, map_reasoning_to_provider_effort, media_type_to_extension,
    normalize_headers, parse_json, parse_json_event_stream, parse_json_with_schema,
    parse_provider_options, post_form_data_to_api, post_json_to_api, post_to_api,
    prepare_get_from_api_request, prepare_post_form_data_to_api_request,
    prepare_post_json_to_api_request, prepare_post_to_api_request, prepare_tools,
    prepare_tools_with_context, read_response_with_size_limit, remove_undefined_entries, resolve,
    resolve_full_media_type, resolve_provider_reference, safe_parse_json,
    safe_parse_json_with_schema, safe_validate_types, serialize_model_options,
    strip_file_extension, tool, validate_download_url, validate_types,
    with_provider_utils_user_agent, with_user_agent_suffix, without_trailing_slash,
};
pub use registry::{
    CustomProvider, CustomProviderWithFiles, CustomProviderWithRerankingModel,
    CustomProviderWithSkills, CustomProviderWithSpeechModel, CustomProviderWithTranscriptionModel,
    CustomProviderWithVideoModel, NoSuchProviderError, ProviderRegistry, ProviderRegistryError,
    ProviderRegistryOptions, create_provider_registry,
    create_provider_registry_with_image_model_middleware,
    create_provider_registry_with_language_model_middleware, create_provider_registry_with_options,
    custom_provider, experimental_create_provider_registry,
    experimental_create_provider_registry_with_options, split_registry_model_id,
};
pub use rerank::{
    RerankDocument, RerankDocuments, RerankEndEvent, RerankOnEnd, RerankOnEndCallback,
    RerankOnEndFunction, RerankOnEndFuture, RerankOnStart, RerankOnStartCallback,
    RerankOnStartFunction, RerankOnStartFuture, RerankOptions, RerankRanking, RerankResponse,
    RerankResult, RerankStartEvent, RerankingModelCallEndEvent, RerankingModelCallStartEvent,
    rerank,
};
pub use reranking_model::{
    RerankingModel, RerankingModelCallOptions, RerankingModelDocuments, RerankingModelRanking,
    RerankingModelResponse, RerankingModelResult,
};
pub use resolve_model::{
    ModelSource, ResolvedModel, resolve_embedding_model, resolve_image_model,
    resolve_language_model, resolve_reranking_model, resolve_speech_model,
    resolve_transcription_model, resolve_video_model,
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
    SpeechModelRequest, SpeechModelResponse, SpeechModelResponseMetadata, SpeechModelResult,
};
pub use stream_object::{
    ObjectStreamFinishPart, ObjectStreamPart, StreamObjectAbortController, StreamObjectAbortSignal,
    StreamObjectOptions, StreamObjectResponseMetadata, StreamObjectResult, stream_object,
};
pub use stream_text::{
    SmoothStreamChunkDetector, SmoothStreamChunking, SmoothStreamError, SmoothStreamOptions,
    StreamTextAbortController, StreamTextAbortSignal, StreamTextMessageMetadata,
    StreamTextMessageMetadataFunction, StreamTextOnAbortEvent, StreamTextOptions,
    StreamTextResponseMetadata, StreamTextResult, StreamTextStep, StreamTextStepPerformance,
    StreamTextTransform, StreamTextTransformFunction, StreamTextUiMessageStreamOptions,
    TextStreamFilePart, TextStreamFinishPart, TextStreamFinishStepPart, TextStreamPart,
    TextStreamReasoningDeltaPart, TextStreamReasoningFilePart, TextStreamStartPart,
    TextStreamStartStepPart, TextStreamTextDeltaPart, smooth_stream, stream_text,
};
pub use telemetry::{
    AI_SDK_TELEMETRY_DIAGNOSTIC_CHANNEL, LegacyOpenTelemetryRecorder, OpenTelemetryRecorder,
    TelemetryDiagnosticMessage, TelemetryDiagnosticSubscription, TelemetryDispatcher,
    TelemetryEvent, TelemetryEventKind, TelemetryExecuteToolOptions, TelemetryIntegration,
    TelemetryOptions, create_legacy_open_telemetry_integration, create_open_telemetry_integration,
    create_telemetry_dispatcher, get_global_telemetry_integrations, register_telemetry,
    register_telemetry_integration, subscribe_telemetry_diagnostics,
};
pub use text_stream_response::{
    TEXT_STREAM_CONTENT_TYPE, TextStreamResponse, TextStreamResponseInit,
    TextStreamResponseOptions, TextStreamResponseWriter, create_text_stream_response,
    pipe_text_stream_to_response,
};
pub use togetherai::{
    DEFAULT_TOGETHERAI_BASE_URL, TogetherAIImageModel, TogetherAIProvider,
    TogetherAIProviderSettings, TogetherAIRerankingModel, create_togetherai, togetherai,
};
pub use transcribe::{
    ExperimentalTranscriptionResult, TranscribeAudio, TranscribeDownload,
    TranscribeDownloadFunction, TranscribeDownloadFuture, TranscribeDownloadOptions,
    TranscribeError, TranscribeOptions, TranscriptionResult, experimental_transcribe, transcribe,
};
pub use transcription_model::{
    NoTranscriptGeneratedError, TranscriptionModel, TranscriptionModelCallOptions,
    TranscriptionModelRequest, TranscriptionModelResponse, TranscriptionModelResponseMetadata,
    TranscriptionModelResult, TranscriptionModelSegment,
};
pub use ui_message_stream::{
    CreateUiMessageStreamOptions, HandleUiMessageStreamFinishOptions, ReadUiMessageStreamOptions,
    ResponseUiMessageId, SafeValidateUiMessagesResult, StreamingUiMessageState,
    UI_MESSAGE_STREAM_CONTENT_TYPE, UI_MESSAGE_STREAM_VERSION, UI_MESSAGE_STREAM_VERSION_HEADER,
    UiMessage, UiMessageChunk, UiMessageRole, UiMessageStreamCreateErrorFunction,
    UiMessageStreamCreateErrorHandler, UiMessageStreamFinishCallback,
    UiMessageStreamFinishCallbackEvent, UiMessageStreamFinishCallbackFunction,
    UiMessageStreamFinishEvent, UiMessageStreamOnFinish, UiMessageStreamOnFinishFunction,
    UiMessageStreamProcessError, UiMessageStreamResponse, UiMessageStreamResponseInit,
    UiMessageStreamResponseOptions, UiMessageStreamResponseWriter,
    UiMessageStreamStepFinishCallback, UiMessageStreamStepFinishCallbackEvent,
    UiMessageStreamStepFinishCallbackFunction, UiMessageStreamWriter, UiMessageValidationError,
    UiMessageValidationOptions, UiMessageValidationTool, create_ui_message_stream,
    create_ui_message_stream_response, create_ui_message_stream_with_result,
    get_response_ui_message_id, get_static_tool_name, handle_ui_message_stream_finish,
    is_custom_content_ui_part, is_data_ui_part, is_dynamic_tool_ui_part, is_static_tool_ui_part,
    is_tool_ui_part, last_assistant_message_is_complete_with_approval_responses,
    last_assistant_message_is_complete_with_tool_calls, pipe_ui_message_stream_to_response,
    process_text_stream, process_ui_message_stream, read_ui_message_stream,
    safe_validate_ui_messages, transform_text_to_ui_message_stream, validate_ui_messages,
};
pub use upload_file::{
    UploadFileData, UploadFileOptions, UploadFileResult, upload_file, upload_file_with_provider,
};
pub use upload_skill::{
    UploadSkillFile, UploadSkillFileData, UploadSkillOptions, UploadSkillResult, upload_skill,
    upload_skill_with_provider,
};
pub use util::{
    AbortSignalSource, AbortTimeoutHandle, AbortTimeoutOptions, AsyncIterableStream,
    AsyncIterableStreamError, AsyncIterableStreamIterator, AsyncIterableStreamSource, Callback,
    CallbackFunction, CallbackFuture, CallbackResult, CallbackSettleFuture, CreateDownloadOptions,
    DataUrlTextError, DownloadFunction, DownloadTransportRequest, DownloadUrlOptions,
    InvalidArgumentError as AiInvalidArgumentError, NotifyCallbacks, NotifyFuture,
    ParsePartialJsonResult, ParsePartialJsonState, PrepareRetriesOptions, PreparedRetries,
    SerialJobError, SerialJobExecutor, SerialJobHandle, SerialJobResult, ServerResponseWriter,
    SimulateReadableStreamDelayFunction, SimulateReadableStreamError,
    SimulateReadableStreamOptions, SimulateReadableStreamResult, SimulatedReadableStream,
    SplitArrayError, StitchableStream, StitchableStreamError, StitchableStreamRead,
    VecAsyncIterableStreamSource, WriteToServerResponseOptions, cosine_similarity,
    create_async_iterable_stream, create_async_iterable_stream_from_source, create_download,
    create_stitchable_stream, download, download_with_transport, fix_json,
    get_potential_start_index, get_text_from_data_url, is_deep_equal_data, merge_abort_signals,
    merge_callbacks, merge_objects, notify, parse_partial_json, prepare_headers, prepare_retries,
    set_abort_timeout, simulate_readable_stream, simulate_readable_stream_with_delay, split_array,
    write_to_server_response,
};
pub use vercel::{
    DEFAULT_VERCEL_BASE_URL, VercelProvider, VercelProviderSettings, create_vercel, vercel,
};
pub use vercel_ai_gateway::{
    VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL, VercelAiGatewayOpenAICompatibleProvider,
    VercelAiGatewayOpenAICompatibleSettings, create_vercel_ai_gateway_openai_compatible,
    vercel_ai_gateway_openai_compatible, vercel_ai_gateway_openai_compatible_embedding,
    vercel_ai_gateway_openai_compatible_image, vercel_ai_gateway_openai_responses,
};
pub use video_model::{
    ExperimentalVideoModel, ExperimentalVideoModelCallOptions, ExperimentalVideoModelFile,
    ExperimentalVideoModelResult, ExperimentalVideoModelVideoData, NoVideoGeneratedError,
    VideoModel, VideoModelCallOptions, VideoModelFile, VideoModelResponse,
    VideoModelResponseMetadata, VideoModelResult, VideoModelVideoData,
};
pub use voyage::{
    DEFAULT_VOYAGE_BASE_URL, VoyageEmbeddingModel, VoyageProvider, VoyageProviderSettings,
    VoyageRerankingModel, VoyageTransport, VoyageTransportFuture, create_voyage, voyage,
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
