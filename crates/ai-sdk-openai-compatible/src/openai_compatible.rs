use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use url::Url;

use ai_sdk_provider::embedding_model::{
    EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResponse, EmbeddingModelResult,
    EmbeddingModelUsage,
};
use ai_sdk_provider::file_data::{FileData, FileDataContent};
use ai_sdk_provider::headers::Headers;
use ai_sdk_provider::image_model::{
    ImageModel, ImageModelCallOptions, ImageModelFile, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResult,
};
use ai_sdk_provider::json::{JsonArray, JsonObject, JsonValue};
use ai_sdk_provider::language_model::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelAssistantContentPart,
    LanguageModelCallOptions, LanguageModelContent, LanguageModelErrorStreamPart,
    LanguageModelFile, LanguageModelFileData, LanguageModelFinishReason,
    LanguageModelGenerateResult, LanguageModelMessage, LanguageModelRawStreamPart,
    LanguageModelReasoning, LanguageModelReasoningDelta, LanguageModelReasoningEffort,
    LanguageModelReasoningEnd, LanguageModelReasoningStart, LanguageModelRequest,
    LanguageModelResponse, LanguageModelResponseFormat, LanguageModelStreamFinish,
    LanguageModelStreamPart, LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
    LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
    LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextStart,
    LanguageModelTool, LanguageModelToolCall, LanguageModelToolChoice, LanguageModelUsage,
    OutputTokenUsage,
};
use ai_sdk_provider::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
use ai_sdk_provider::warning::Warning;
use ai_sdk_provider_utils::{
    ConvertToFormDataOptions, FetchErrorInfo, FormDataInputValue, FormDataValue, GetFromApiOptions,
    HandledFetchError, InjectJsonInstructionIntoMessagesOptions, ParseJsonResult,
    PostFormDataToApiOptions, PostJsonToApiOptions, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, ProviderApiResponseHandlerError,
    RuntimeEnvironment, StreamingToolCallDelta, StreamingToolCallDeltaFunction,
    StreamingToolCallTracker, combine_headers, convert_base64_to_bytes, convert_to_base64,
    convert_to_form_data, create_event_source_response_handler, create_json_error_response_handler,
    create_json_response_handler, detect_media_type, generate_id, get_from_api,
    inject_json_instruction_into_messages, post_form_data_to_api, post_json_to_api,
    with_user_agent_suffix, without_trailing_slash,
};

/// Future returned by an injected OpenAI-compatible HTTP transport.
pub type OpenAICompatibleTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by OpenAI-compatible provider models.
pub type OpenAICompatibleTransport =
    Arc<dyn Fn(ProviderApiRequest) -> OpenAICompatibleTransportFuture + Send + Sync>;

/// Future returned by an OpenAI-compatible metadata extractor.
pub type OpenAICompatibleExtractMetadataFuture =
    Pin<Box<dyn Future<Output = Option<ProviderMetadata>> + Send>>;

type OpenAICompatibleExtractMetadataCallback = dyn Fn(OpenAICompatibleExtractMetadataArgs) -> OpenAICompatibleExtractMetadataFuture
    + Send
    + Sync;
type OpenAICompatibleCreateStreamMetadataExtractorCallback =
    dyn Fn() -> OpenAICompatibleStreamMetadataExtractor + Send + Sync;
type OpenAICompatibleProcessStreamMetadataChunkCallback = dyn Fn(&JsonValue) + Send + Sync;
type OpenAICompatibleBuildStreamMetadataCallback =
    dyn Fn() -> Option<ProviderMetadata> + Send + Sync;
type OpenAICompatibleTransformRequestBodyCallback = dyn Fn(JsonValue) -> JsonValue + Send + Sync;

/// Chat request body transformer for OpenAI-compatible proxy providers.
#[derive(Clone)]
pub struct OpenAICompatibleRequestBodyTransformer {
    transform_request_body: Arc<OpenAICompatibleTransformRequestBodyCallback>,
}

impl OpenAICompatibleRequestBodyTransformer {
    /// Creates a request body transformer from a callback.
    pub fn new<F>(transform_request_body: F) -> Self
    where
        F: Fn(JsonValue) -> JsonValue + Send + Sync + 'static,
    {
        Self {
            transform_request_body: Arc::new(transform_request_body),
        }
    }

    /// Transforms the JSON request body before it is sent.
    pub fn transform_request_body(&self, request_body: JsonValue) -> JsonValue {
        (self.transform_request_body)(request_body)
    }
}

impl fmt::Debug for OpenAICompatibleRequestBodyTransformer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenAICompatibleRequestBodyTransformer")
            .finish_non_exhaustive()
    }
}

impl PartialEq for OpenAICompatibleRequestBodyTransformer {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.transform_request_body, &other.transform_request_body)
    }
}

impl Eq for OpenAICompatibleRequestBodyTransformer {}

/// Arguments passed to a complete-response metadata extractor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAICompatibleExtractMetadataArgs {
    /// Parsed provider response JSON body.
    pub parsed_body: JsonValue,
}

impl OpenAICompatibleExtractMetadataArgs {
    /// Creates metadata extraction arguments from a parsed response body.
    pub fn new(parsed_body: JsonValue) -> Self {
        Self { parsed_body }
    }
}

/// Provider-specific metadata extraction callbacks for OpenAI-compatible chat models.
#[derive(Clone)]
pub struct OpenAICompatibleMetadataExtractor {
    extract_metadata: Option<Arc<OpenAICompatibleExtractMetadataCallback>>,
    create_stream_extractor: Option<Arc<OpenAICompatibleCreateStreamMetadataExtractorCallback>>,
}

impl OpenAICompatibleMetadataExtractor {
    /// Creates an empty metadata extractor.
    pub fn new() -> Self {
        Self {
            extract_metadata: None,
            create_stream_extractor: None,
        }
    }

    /// Sets the callback used to extract metadata from complete JSON responses.
    pub fn with_extract_metadata<F, Fut>(mut self, extract_metadata: F) -> Self
    where
        F: Fn(OpenAICompatibleExtractMetadataArgs) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<ProviderMetadata>> + Send + 'static,
    {
        self.extract_metadata = Some(Arc::new(move |args| Box::pin(extract_metadata(args))));
        self
    }

    /// Sets the callback used to create one extractor for each streaming response.
    pub fn with_stream_extractor<F>(mut self, create_stream_extractor: F) -> Self
    where
        F: Fn() -> OpenAICompatibleStreamMetadataExtractor + Send + Sync + 'static,
    {
        self.create_stream_extractor = Some(Arc::new(create_stream_extractor));
        self
    }

    async fn extract_metadata(&self, parsed_body: JsonValue) -> Option<ProviderMetadata> {
        let extract_metadata = self.extract_metadata.as_ref()?;
        extract_metadata(OpenAICompatibleExtractMetadataArgs::new(parsed_body)).await
    }

    fn create_stream_extractor(&self) -> Option<OpenAICompatibleStreamMetadataExtractor> {
        self.create_stream_extractor
            .as_ref()
            .map(|create_stream_extractor| create_stream_extractor())
    }
}

impl Default for OpenAICompatibleMetadataExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for OpenAICompatibleMetadataExtractor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenAICompatibleMetadataExtractor")
            .field("extract_metadata", &self.extract_metadata.is_some())
            .field(
                "create_stream_extractor",
                &self.create_stream_extractor.is_some(),
            )
            .finish()
    }
}

impl PartialEq for OpenAICompatibleMetadataExtractor {
    fn eq(&self, other: &Self) -> bool {
        option_arc_ptr_eq(&self.extract_metadata, &other.extract_metadata)
            && option_arc_ptr_eq(
                &self.create_stream_extractor,
                &other.create_stream_extractor,
            )
    }
}

impl Eq for OpenAICompatibleMetadataExtractor {}

/// Stateful metadata extractor for one OpenAI-compatible chat stream.
#[derive(Clone)]
pub struct OpenAICompatibleStreamMetadataExtractor {
    process_chunk: Arc<OpenAICompatibleProcessStreamMetadataChunkCallback>,
    build_metadata: Arc<OpenAICompatibleBuildStreamMetadataCallback>,
}

impl OpenAICompatibleStreamMetadataExtractor {
    /// Creates a stream metadata extractor from chunk and finalization callbacks.
    pub fn new<P, B>(process_chunk: P, build_metadata: B) -> Self
    where
        P: Fn(&JsonValue) + Send + Sync + 'static,
        B: Fn() -> Option<ProviderMetadata> + Send + Sync + 'static,
    {
        Self {
            process_chunk: Arc::new(process_chunk),
            build_metadata: Arc::new(build_metadata),
        }
    }

    /// Processes a parsed stream chunk.
    pub fn process_chunk(&self, parsed_chunk: &JsonValue) {
        (self.process_chunk)(parsed_chunk);
    }

    /// Builds final provider metadata after all chunks have been processed.
    pub fn build_metadata(&self) -> Option<ProviderMetadata> {
        (self.build_metadata)()
    }
}

impl fmt::Debug for OpenAICompatibleStreamMetadataExtractor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenAICompatibleStreamMetadataExtractor")
            .finish_non_exhaustive()
    }
}

fn option_arc_ptr_eq<T: ?Sized>(left: &Option<Arc<T>>, right: &Option<Arc<T>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

/// Settings for an OpenAI-compatible provider instance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAICompatibleProviderSettings {
    /// Base URL for API calls, without the endpoint path.
    pub base_url: String,

    /// Provider name used as the provider id prefix.
    pub name: String,

    /// API key used to build a `Bearer` authorization header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom headers included in model requests.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,

    /// Custom query parameters appended to model request URLs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub query_params: BTreeMap<String, String>,

    /// Provider ids for model types whose upstream package uses a non-default suffix.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_provider_names: BTreeMap<String, String>,

    /// Include usage information in streaming responses when supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_usage: Option<bool>,

    /// Whether chat models support structured JSON schema outputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_structured_outputs: Option<bool>,

    /// Whether chat models support the OpenAI JSON object response_format body field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_json_object_response_format: Option<bool>,

    /// User-agent suffix for wrappers built on the OpenAI-compatible transport.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent_suffix: Option<String>,

    /// Chat-only request body transformer.
    #[serde(skip)]
    pub transform_request_body: Option<OpenAICompatibleRequestBodyTransformer>,

    /// Chat-only metadata extraction callbacks.
    #[serde(skip)]
    pub metadata_extractor: Option<OpenAICompatibleMetadataExtractor>,
}

impl OpenAICompatibleProviderSettings {
    /// Creates OpenAI-compatible provider settings.
    pub fn new(name: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            base_url: base_url.into(),
            api_key: None,
            headers: Headers::new(),
            query_params: BTreeMap::new(),
            model_provider_names: BTreeMap::new(),
            include_usage: None,
            supports_structured_outputs: None,
            supports_json_object_response_format: None,
            user_agent_suffix: None,
            transform_request_body: None,
            metadata_extractor: None,
        }
    }

    /// Sets the API key used for bearer authentication.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Adds a custom request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Adds a custom URL query parameter.
    pub fn with_query_param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.query_params.insert(name.into(), value.into());
        self
    }

    /// Overrides the provider id used for a model type.
    pub fn with_model_provider_name(
        mut self,
        model_type: impl Into<String>,
        provider_name: impl Into<String>,
    ) -> Self {
        self.model_provider_names
            .insert(model_type.into(), provider_name.into());
        self
    }

    /// Sets whether streamed requests should include usage when supported.
    pub fn with_include_usage(mut self, include_usage: bool) -> Self {
        self.include_usage = Some(include_usage);
        self
    }

    /// Sets whether chat models support structured JSON schema outputs.
    pub fn with_supports_structured_outputs(mut self, supports_structured_outputs: bool) -> Self {
        self.supports_structured_outputs = Some(supports_structured_outputs);
        self
    }

    /// Sets whether chat models support the OpenAI JSON object response_format body field.
    pub fn with_supports_json_object_response_format(
        mut self,
        supports_json_object_response_format: bool,
    ) -> Self {
        self.supports_json_object_response_format = Some(supports_json_object_response_format);
        self
    }

    /// Sets the request user-agent suffix for wrappers built on this provider.
    pub fn with_user_agent_suffix(mut self, user_agent_suffix: impl Into<String>) -> Self {
        self.user_agent_suffix = Some(user_agent_suffix.into());
        self
    }

    /// Sets a chat request body transformer for OpenAI-compatible proxy providers.
    pub fn with_transform_request_body<F>(mut self, transform_request_body: F) -> Self
    where
        F: Fn(JsonValue) -> JsonValue + Send + Sync + 'static,
    {
        self.transform_request_body = Some(OpenAICompatibleRequestBodyTransformer::new(
            transform_request_body,
        ));
        self
    }

    /// Sets provider-specific metadata extraction callbacks for chat models.
    pub fn with_metadata_extractor(
        mut self,
        metadata_extractor: OpenAICompatibleMetadataExtractor,
    ) -> Self {
        self.metadata_extractor = Some(metadata_extractor);
        self
    }
}

/// A model entry returned by an OpenAI-compatible `/models` response.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OpenAICompatibleModelEntry {
    /// Provider-specific model id.
    pub id: String,

    /// OpenAI-compatible object type, usually `model`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,

    /// Creation timestamp when the provider includes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<i64>,

    /// Owning provider or organization when the provider includes it.
    #[serde(
        default,
        rename = "owned_by",
        alias = "ownedBy",
        skip_serializing_if = "Option::is_none"
    )]
    pub owned_by: Option<String>,

    /// Display name when the provider includes richer model metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Model description when the provider includes richer model metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Release timestamp when the provider includes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released: Option<i64>,

    /// Maximum context length in tokens when the provider includes it.
    #[serde(
        default,
        rename = "context_window",
        alias = "contextWindow",
        skip_serializing_if = "Option::is_none"
    )]
    pub context_window: Option<u64>,

    /// Maximum output tokens when the provider includes it.
    #[serde(
        default,
        rename = "max_tokens",
        alias = "maxTokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_tokens: Option<u64>,

    /// Model category such as `language`, `embedding`, `image`, or `video`.
    #[serde(
        default,
        rename = "type",
        alias = "modelType",
        skip_serializing_if = "Option::is_none"
    )]
    pub model_type: Option<String>,

    /// Capability tags when the provider includes them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    /// Pricing metadata when the provider includes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<JsonObject>,

    /// Additional provider-specific model metadata.
    #[serde(default, flatten)]
    pub metadata: JsonObject,
}

/// OpenAI-compatible `/models` response.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OpenAICompatibleModelListResponse {
    /// OpenAI-compatible object type, usually `list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,

    /// Models returned by the provider.
    pub data: Vec<OpenAICompatibleModelEntry>,
}

impl OpenAICompatibleModelListResponse {
    /// Iterates over returned model ids.
    pub fn model_ids(&self) -> impl Iterator<Item = &str> {
        self.data.iter().map(|model| model.id.as_str())
    }
}

/// OpenAI-compatible provider.
#[derive(Clone)]
pub struct OpenAICompatibleProvider {
    settings: OpenAICompatibleProviderSettings,
    transport: OpenAICompatibleTransport,
}

impl OpenAICompatibleProvider {
    /// Creates a provider from explicit OpenAI-compatible settings.
    pub fn from_settings(settings: OpenAICompatibleProviderSettings) -> Self {
        Self {
            settings,
            transport: default_openai_compatible_transport(),
        }
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Creates the default OpenAI-compatible chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates an OpenAI-compatible chat language model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        OpenAICompatibleChatLanguageModel::new(
            model_id,
            openai_compatible_model_config("chat", &self.settings, &self.transport),
        )
    }

    /// Creates an OpenAI-compatible completion language model.
    pub fn completion_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleCompletionLanguageModel {
        OpenAICompatibleCompletionLanguageModel::new(
            model_id,
            openai_compatible_model_config("completion", &self.settings, &self.transport),
        )
    }

    /// Creates an OpenAI-compatible embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        OpenAICompatibleEmbeddingModel::new(
            model_id,
            openai_compatible_model_config("embedding", &self.settings, &self.transport),
        )
    }

    /// Deprecated upstream alias for [`OpenAICompatibleProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates an OpenAI-compatible image model.
    pub fn image_model(&self, model_id: impl Into<String>) -> OpenAICompatibleImageModel {
        OpenAICompatibleImageModel::new(
            model_id,
            openai_compatible_model_config("image", &self.settings, &self.transport),
        )
    }

    /// Lists models from an OpenAI-compatible `/models` endpoint.
    pub async fn list_models(
        &self,
    ) -> Result<OpenAICompatibleModelListResponse, HandledFetchError> {
        let url = openai_compatible_url(&self.settings, "/models")
            .map_err(openai_compatible_url_fetch_error)?;
        let request_headers = openai_compatible_provider_headers(&self.settings)
            .into_iter()
            .map(|(name, value)| (name, Some(value)))
            .collect::<BTreeMap<_, _>>();
        let get_options = GetFromApiOptions::new(url)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        get_from_api(
            get_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    openai_compatible_model_list_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    openai_compatible_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        .map(|response| response.value)
    }

    /// Retrieves one model from an OpenAI-compatible `/models/{model}` endpoint.
    pub async fn retrieve_model(
        &self,
        model_id: impl AsRef<str>,
    ) -> Result<OpenAICompatibleModelEntry, HandledFetchError> {
        let url = openai_compatible_retrieve_model_url(&self.settings, model_id.as_ref())
            .map_err(openai_compatible_url_fetch_error)?;
        let request_headers = openai_compatible_provider_headers(&self.settings)
            .into_iter()
            .map(|(name, value)| (name, Some(value)))
            .collect::<BTreeMap<_, _>>();
        let get_options = GetFromApiOptions::new(url)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        get_from_api(
            get_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    openai_compatible_model_entry_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    openai_compatible_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        .map(|response| response.value)
    }
}

/// Creates an OpenAI-compatible provider.
pub fn create_openai_compatible(
    settings: OpenAICompatibleProviderSettings,
) -> OpenAICompatibleProvider {
    OpenAICompatibleProvider::from_settings(settings)
}

#[derive(Clone)]
struct OpenAICompatibleModelConfig {
    provider: String,
    settings: OpenAICompatibleProviderSettings,
    transport: OpenAICompatibleTransport,
}

fn openai_compatible_model_config(
    model_type: &str,
    settings: &OpenAICompatibleProviderSettings,
    transport: &OpenAICompatibleTransport,
) -> OpenAICompatibleModelConfig {
    let provider = settings
        .model_provider_names
        .get(model_type)
        .cloned()
        .unwrap_or_else(|| format!("{}.{}", settings.name, model_type));

    let mut model_settings = settings.clone();
    if model_type != "chat" {
        model_settings.supports_structured_outputs = None;
        model_settings.supports_json_object_response_format = None;
        model_settings.transform_request_body = None;
        model_settings.metadata_extractor = None;
    }

    OpenAICompatibleModelConfig {
        provider,
        settings: model_settings,
        transport: Arc::clone(transport),
    }
}

/// OpenAI-compatible chat language model.
#[derive(Clone)]
pub struct OpenAICompatibleChatLanguageModel {
    model_id: String,
    config: OpenAICompatibleModelConfig,
}

impl OpenAICompatibleChatLanguageModel {
    fn new(model_id: impl Into<String>, config: OpenAICompatibleModelConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        &self.config.provider
    }

    /// Returns whether structured outputs are enabled for this chat model.
    pub fn supports_structured_outputs(&self) -> bool {
        self.config
            .settings
            .supports_structured_outputs
            .unwrap_or(false)
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let (request_body, warnings) = match openai_compatible_chat_request_body(
            &self.model_id,
            &self.config.provider,
            &self.config.settings,
            &options,
        ) {
            Ok(result) => result,
            Err(message) => {
                return openai_compatible_error_generate_result(
                    &self.config.settings.name,
                    message,
                    json!({ "model": self.model_id }),
                );
            }
        };
        let request_body = self.transform_request_body(request_body);
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let url = match self.model_url("/chat/completions") {
            Ok(url) => url,
            Err(message) => {
                return openai_compatible_error_generate_result(
                    &self.config.settings.name,
                    message,
                    request_body_for_error,
                );
            }
        };
        let post_options = PostJsonToApiOptions::new(url, request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.config.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    clone_json_value,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    openai_compatible_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => {
                self.generate_result_from_response(
                    response.value,
                    response.raw_value,
                    response.response_headers,
                    request_body_for_response,
                    warnings,
                )
                .await
            }
            Err(error) => self.generate_result_from_error(error, request_body_for_error),
        }
    }

    async fn do_stream_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let include_raw_chunks = options.include_raw_chunks.unwrap_or(false);
        let (request_body, warnings) = match openai_compatible_chat_stream_request_body(
            &self.model_id,
            &self.config.provider,
            &self.config.settings,
            &options,
        ) {
            Ok(result) => result,
            Err(message) => {
                return openai_compatible_error_stream_result(
                    message,
                    json!({ "model": self.model_id }),
                    None,
                    None,
                );
            }
        };
        let request_body = self.transform_request_body(request_body);
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let url = match self.model_url("/chat/completions") {
            Ok(url) => url,
            Err(message) => {
                return openai_compatible_error_stream_result(
                    message,
                    request_body_for_error,
                    None,
                    None,
                );
            }
        };
        let post_options = PostJsonToApiOptions::new(url, request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.config.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |_request, response| {
                create_event_source_response_handler(
                    response.event_source_response_handler_options(),
                    clone_json_value,
                )
                .map_err(|error| ProviderApiResponseHandlerError::other(error.to_string()))
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    openai_compatible_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => openai_compatible_stream_result_from_response(
                &self.config.settings.name,
                response.value,
                response.response_headers,
                request_body_for_response,
                warnings,
                include_raw_chunks,
                self.config
                    .settings
                    .metadata_extractor
                    .as_ref()
                    .and_then(OpenAICompatibleMetadataExtractor::create_stream_extractor),
            ),
            Err(error) => self.stream_result_from_error(error, request_body_for_error),
        }
    }

    fn model_url(&self, path: &str) -> Result<String, String> {
        openai_compatible_url(&self.config.settings, path)
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(
                openai_compatible_provider_headers(&self.config.settings)
                    .into_iter()
                    .map(|(name, value)| (name, Some(value)))
                    .collect::<Vec<_>>(),
            ),
            optional_headers(call_headers),
        ])
    }

    fn transform_request_body(&self, request_body: JsonValue) -> JsonValue {
        match self.config.settings.transform_request_body.as_ref() {
            Some(transformer) => transformer.transform_request_body(request_body),
            None => request_body,
        }
    }

    async fn generate_result_from_response(
        &self,
        response: JsonValue,
        raw_response: Option<JsonValue>,
        response_headers: Option<Headers>,
        request_body: JsonValue,
        warnings: Vec<Warning>,
    ) -> LanguageModelGenerateResult {
        let choice = response
            .get("choices")
            .and_then(JsonValue::as_array)
            .and_then(|choices| choices.first());
        let message = choice.and_then(|choice| choice.get("message"));
        let content = openai_compatible_response_content(message, &self.config.settings.name);
        let finish_reason = openai_compatible_finish_reason(
            choice
                .and_then(|choice| choice.get("finish_reason"))
                .or_else(|| choice.and_then(|choice| choice.get("finishReason"))),
        );
        let usage = openai_compatible_usage(response.get("usage"));
        let raw_body = raw_response.unwrap_or_else(|| response.clone());

        let mut result = LanguageModelGenerateResult::new(content, finish_reason, usage)
            .with_request(LanguageModelRequest::new().with_body(request_body));
        let mut response_metadata = LanguageModelResponse::new().with_body(raw_body.clone());

        if let Some(id) = json_string(response.get("id")) {
            response_metadata = response_metadata.with_id(id);
        }

        if let Some(timestamp) = openai_compatible_response_timestamp(response.get("created")) {
            response_metadata = response_metadata.with_timestamp(timestamp);
        }

        if let Some(model_id) = json_string(response.get("model")) {
            response_metadata = response_metadata.with_model_id(model_id);
        }

        if let Some(headers) = response_headers {
            response_metadata = with_response_headers(response_metadata, headers);
        }

        let metadata = openai_compatible_provider_metadata(
            &self.config.settings.name,
            &raw_body,
            self.config.settings.metadata_extractor.as_ref(),
        )
        .await;
        if !metadata.is_empty() {
            result = result.with_provider_metadata(metadata);
        }

        for warning in warnings {
            result = result.with_warning(warning);
        }

        result.with_response(response_metadata)
    }

    fn generate_result_from_error(
        &self,
        error: HandledFetchError,
        request_body: JsonValue,
    ) -> LanguageModelGenerateResult {
        let message = match error {
            HandledFetchError::Original { error } => error.message().to_string(),
            HandledFetchError::ApiCall { error } => error.message().to_string(),
        };

        openai_compatible_error_generate_result(&self.config.settings.name, message, request_body)
    }

    fn stream_result_from_error(
        &self,
        error: HandledFetchError,
        request_body: JsonValue,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let (message, headers, body) = match error {
            HandledFetchError::Original { error } => (error.message().to_string(), None, None),
            HandledFetchError::ApiCall { error } => (
                error.message().to_string(),
                error.response_headers().cloned(),
                error.response_body().map(String::from),
            ),
        };

        openai_compatible_error_stream_result(message, request_body, headers, body.as_deref())
    }
}

impl LanguageModel for OpenAICompatibleChatLanguageModel {
    type SupportedUrlsFuture<'a>
        = Ready<LanguageModelSupportedUrls>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
    where
        Self: 'a;

    type Stream = Vec<LanguageModelStreamPart>;

    type StreamFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelStreamResult<Self::Stream>> + Send + 'a>>
    where
        Self: 'a;

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(LanguageModelSupportedUrls::new())
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(self.do_stream_result(options))
    }
}

/// OpenAI-compatible completion language model.
#[derive(Clone)]
pub struct OpenAICompatibleCompletionLanguageModel {
    model_id: String,
    config: OpenAICompatibleModelConfig,
}

impl OpenAICompatibleCompletionLanguageModel {
    fn new(model_id: impl Into<String>, config: OpenAICompatibleModelConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        &self.config.provider
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let (request_body, warnings) = match openai_compatible_completion_request_body(
            &self.model_id,
            &self.config.provider,
            &options,
        ) {
            Ok(result) => result,
            Err(message) => {
                return openai_compatible_error_generate_result(
                    &self.config.settings.name,
                    message,
                    json!({ "model": self.model_id }),
                );
            }
        };
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let url = match self.model_url("/completions") {
            Ok(url) => url,
            Err(message) => {
                return openai_compatible_error_generate_result(
                    &self.config.settings.name,
                    message,
                    request_body_for_error,
                );
            }
        };
        let post_options = PostJsonToApiOptions::new(url, request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.config.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    clone_json_value,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    openai_compatible_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => self.generate_result_from_response(
                response.value,
                response.raw_value,
                response.response_headers,
                request_body_for_response,
                warnings,
            ),
            Err(error) => self.generate_result_from_error(error, request_body_for_error),
        }
    }

    async fn do_stream_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let include_raw_chunks = options.include_raw_chunks.unwrap_or(false);
        let (request_body, warnings) = match openai_compatible_completion_stream_request_body(
            &self.model_id,
            &self.config.provider,
            &self.config.settings,
            &options,
        ) {
            Ok(result) => result,
            Err(message) => {
                return openai_compatible_error_stream_result(
                    message,
                    json!({ "model": self.model_id }),
                    None,
                    None,
                );
            }
        };
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let url = match self.model_url("/completions") {
            Ok(url) => url,
            Err(message) => {
                return openai_compatible_error_stream_result(
                    message,
                    request_body_for_error,
                    None,
                    None,
                );
            }
        };
        let post_options = PostJsonToApiOptions::new(url, request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.config.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |_request, response| {
                create_event_source_response_handler(
                    response.event_source_response_handler_options(),
                    clone_json_value,
                )
                .map_err(|error| ProviderApiResponseHandlerError::other(error.to_string()))
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    openai_compatible_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => openai_compatible_completion_stream_result_from_response(
                response.value,
                response.response_headers,
                request_body_for_response,
                warnings,
                include_raw_chunks,
            ),
            Err(error) => self.stream_result_from_error(error, request_body_for_error),
        }
    }

    fn model_url(&self, path: &str) -> Result<String, String> {
        openai_compatible_url(&self.config.settings, path)
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(
                openai_compatible_provider_headers(&self.config.settings)
                    .into_iter()
                    .map(|(name, value)| (name, Some(value)))
                    .collect::<Vec<_>>(),
            ),
            optional_headers(call_headers),
        ])
    }

    fn generate_result_from_response(
        &self,
        response: JsonValue,
        raw_response: Option<JsonValue>,
        response_headers: Option<Headers>,
        request_body: JsonValue,
        warnings: Vec<Warning>,
    ) -> LanguageModelGenerateResult {
        let choice = response
            .get("choices")
            .and_then(JsonValue::as_array)
            .and_then(|choices| choices.first());
        let mut content = Vec::new();

        if let Some(text) = choice
            .and_then(|choice| choice.get("text"))
            .and_then(JsonValue::as_str)
            .filter(|text| !text.is_empty())
        {
            content.push(LanguageModelContent::Text(LanguageModelText::new(text)));
        }

        let finish_reason = openai_compatible_finish_reason(
            choice
                .and_then(|choice| choice.get("finish_reason"))
                .or_else(|| choice.and_then(|choice| choice.get("finishReason"))),
        );
        let usage = openai_compatible_usage(response.get("usage"));
        let raw_body = raw_response.unwrap_or_else(|| response.clone());
        let mut result = LanguageModelGenerateResult::new(content, finish_reason, usage)
            .with_request(LanguageModelRequest::new().with_body(request_body));
        let mut response_metadata = LanguageModelResponse::new().with_body(raw_body);

        if let Some(id) = json_string(response.get("id")) {
            response_metadata = response_metadata.with_id(id);
        }

        if let Some(timestamp) = openai_compatible_response_timestamp(response.get("created")) {
            response_metadata = response_metadata.with_timestamp(timestamp);
        }

        if let Some(model_id) = json_string(response.get("model")) {
            response_metadata = response_metadata.with_model_id(model_id);
        }

        if let Some(headers) = response_headers {
            response_metadata = with_response_headers(response_metadata, headers);
        }

        for warning in warnings {
            result = result.with_warning(warning);
        }

        result.with_response(response_metadata)
    }

    fn generate_result_from_error(
        &self,
        error: HandledFetchError,
        request_body: JsonValue,
    ) -> LanguageModelGenerateResult {
        let message = match error {
            HandledFetchError::Original { error } => error.message().to_string(),
            HandledFetchError::ApiCall { error } => error.message().to_string(),
        };

        openai_compatible_error_generate_result(&self.config.settings.name, message, request_body)
    }

    fn stream_result_from_error(
        &self,
        error: HandledFetchError,
        request_body: JsonValue,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let (message, headers, body) = match error {
            HandledFetchError::Original { error } => (error.message().to_string(), None, None),
            HandledFetchError::ApiCall { error } => (
                error.message().to_string(),
                error.response_headers().cloned(),
                error.response_body().map(String::from),
            ),
        };

        openai_compatible_error_stream_result(message, request_body, headers, body.as_deref())
    }
}

impl LanguageModel for OpenAICompatibleCompletionLanguageModel {
    type SupportedUrlsFuture<'a>
        = Ready<LanguageModelSupportedUrls>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
    where
        Self: 'a;

    type Stream = Vec<LanguageModelStreamPart>;

    type StreamFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelStreamResult<Self::Stream>> + Send + 'a>>
    where
        Self: 'a;

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(LanguageModelSupportedUrls::new())
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(self.do_stream_result(options))
    }
}

/// OpenAI-compatible embedding model.
#[derive(Clone)]
pub struct OpenAICompatibleEmbeddingModel {
    model_id: String,
    config: OpenAICompatibleModelConfig,
}

impl OpenAICompatibleEmbeddingModel {
    fn new(model_id: impl Into<String>, config: OpenAICompatibleModelConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        &self.config.provider
    }

    async fn do_embed_result(&self, options: EmbeddingModelCallOptions) -> EmbeddingModelResult {
        let (request_body, warnings) = openai_compatible_embedding_request_body(
            &self.model_id,
            &self.config.provider,
            &options,
        );
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let url = match self.model_url("/embeddings") {
            Ok(url) => url,
            Err(message) => {
                return openai_compatible_embedding_error_result(
                    &self.config.settings.name,
                    message,
                    request_body_for_error,
                    None,
                    None,
                );
            }
        };
        let post_options = PostJsonToApiOptions::new(url, request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.config.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    openai_compatible_embedding_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    openai_compatible_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => openai_compatible_embedding_result_from_response(
                response.value,
                response.raw_value,
                response.response_headers,
                request_body_for_response,
                warnings,
            ),
            Err(error) => openai_compatible_embedding_result_from_error(
                &self.config.settings.name,
                error,
                request_body_for_error,
            ),
        }
    }

    fn model_url(&self, path: &str) -> Result<String, String> {
        openai_compatible_url(&self.config.settings, path)
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(
                openai_compatible_provider_headers(&self.config.settings)
                    .into_iter()
                    .map(|(name, value)| (name, Some(value)))
                    .collect::<Vec<_>>(),
            ),
            optional_headers(call_headers),
        ])
    }
}

impl EmbeddingModel for OpenAICompatibleEmbeddingModel {
    type MaxEmbeddingsPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type SupportsParallelCallsFuture<'a>
        = Ready<bool>
    where
        Self: 'a;

    type EmbedFuture<'a>
        = Pin<Box<dyn Future<Output = EmbeddingModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_> {
        ready(Some(2048))
    }

    fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
        ready(true)
    }

    fn do_embed(&self, options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
        Box::pin(self.do_embed_result(options))
    }
}

/// OpenAI-compatible image model.
#[derive(Clone)]
pub struct OpenAICompatibleImageModel {
    model_id: String,
    config: OpenAICompatibleModelConfig,
}

impl OpenAICompatibleImageModel {
    fn new(model_id: impl Into<String>, config: OpenAICompatibleModelConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        &self.config.provider
    }

    async fn do_generate_result(&self, options: ImageModelCallOptions) -> ImageModelResult {
        let mut warnings = openai_compatible_image_warnings(&options);
        let provider_options = openai_compatible_image_provider_options(
            &self.config.provider,
            &options.provider_options,
            &mut warnings,
        );
        let request_headers = self.request_headers(options.headers.as_ref());
        let response = if options
            .files
            .as_ref()
            .is_some_and(|files| !files.is_empty())
        {
            self.do_generate_edit_result(options, provider_options, request_headers, warnings)
                .await
        } else {
            self.do_generate_image_result(options, provider_options, request_headers, warnings)
                .await
        };

        match response {
            Ok((response, response_headers, warnings)) => {
                openai_compatible_image_result_from_response(
                    &self.model_id,
                    response,
                    response_headers,
                    warnings,
                )
            }
            Err((error, warnings)) => openai_compatible_image_result_from_error(
                &self.model_id,
                &self.config.settings.name,
                error,
                warnings,
            ),
        }
    }

    async fn do_generate_image_result(
        &self,
        options: ImageModelCallOptions,
        provider_options: JsonObject,
        request_headers: BTreeMap<String, Option<String>>,
        warnings: Vec<Warning>,
    ) -> Result<
        (OpenAICompatibleImageResponse, Option<Headers>, Vec<Warning>),
        (HandledFetchError, Vec<Warning>),
    > {
        let request_body = openai_compatible_image_generation_request_body(
            &self.model_id,
            &options,
            provider_options,
        );
        let url = match self.model_url("/images/generations") {
            Ok(url) => url,
            Err(message) => {
                return Err((
                    HandledFetchError::Original {
                        error: FetchErrorInfo::new(message),
                    },
                    warnings,
                ));
            }
        };
        let post_options = PostJsonToApiOptions::new(url, request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.config.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    openai_compatible_image_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    openai_compatible_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => Ok((response.value, response.response_headers, warnings)),
            Err(error) => Err((error, warnings)),
        }
    }

    async fn do_generate_edit_result(
        &self,
        options: ImageModelCallOptions,
        provider_options: JsonObject,
        request_headers: BTreeMap<String, Option<String>>,
        warnings: Vec<Warning>,
    ) -> Result<
        (OpenAICompatibleImageResponse, Option<Headers>, Vec<Warning>),
        (HandledFetchError, Vec<Warning>),
    > {
        let form_data =
            openai_compatible_image_edit_form_data(&self.model_id, &options, provider_options);
        let url = match self.model_url("/images/edits") {
            Ok(url) => url,
            Err(message) => {
                return Err((
                    HandledFetchError::Original {
                        error: FetchErrorInfo::new(message),
                    },
                    warnings,
                ));
            }
        };
        let post_options = PostFormDataToApiOptions::new(url, form_data)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.config.transport);

        match post_form_data_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    openai_compatible_image_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    openai_compatible_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => Ok((response.value, response.response_headers, warnings)),
            Err(error) => Err((error, warnings)),
        }
    }

    fn model_url(&self, path: &str) -> Result<String, String> {
        openai_compatible_url(&self.config.settings, path)
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(
                openai_compatible_provider_headers(&self.config.settings)
                    .into_iter()
                    .map(|(name, value)| (name, Some(value)))
                    .collect::<Vec<_>>(),
            ),
            optional_headers(call_headers),
        ])
    }
}

impl ImageModel for OpenAICompatibleImageModel {
    type MaxImagesPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        ready(Some(10))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

fn openai_compatible_url(
    settings: &OpenAICompatibleProviderSettings,
    path: &str,
) -> Result<String, String> {
    let base_url = without_trailing_slash(Some(settings.base_url.as_str()))
        .unwrap_or(settings.base_url.as_str());
    let mut url = Url::parse(&format!("{base_url}{path}"))
        .map_err(|error| format!("invalid OpenAI-compatible base URL: {error}"))?;

    if !settings.query_params.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (name, value) in &settings.query_params {
            pairs.append_pair(name, value);
        }
    }

    Ok(url.to_string())
}

fn openai_compatible_retrieve_model_url(
    settings: &OpenAICompatibleProviderSettings,
    model_id: &str,
) -> Result<String, String> {
    let mut url = Url::parse(&openai_compatible_url(settings, "/models")?)
        .map_err(|error| format!("invalid OpenAI-compatible model URL: {error}"))?;

    url.path_segments_mut()
        .map_err(|_| "OpenAI-compatible model URL cannot be a base URL".to_string())?
        .push(model_id);

    Ok(url.to_string())
}

fn openai_compatible_provider_headers(settings: &OpenAICompatibleProviderSettings) -> Headers {
    let mut headers = Vec::new();

    if let Some(api_key) = settings
        .api_key
        .as_ref()
        .filter(|api_key| !api_key.is_empty())
    {
        headers.push((
            "authorization".to_string(),
            Some(format!("Bearer {api_key}")),
        ));
    }

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    let user_agent_suffix = settings
        .user_agent_suffix
        .clone()
        .unwrap_or_else(|| format!("ai-sdk/openai-compatible/{}", crate::VERSION));

    with_user_agent_suffix(Some(headers), [user_agent_suffix])
}

fn optional_headers(headers: Option<&Headers>) -> Option<Vec<(String, Option<String>)>> {
    headers.map(|headers| {
        headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone())))
            .collect()
    })
}

fn openai_compatible_chat_request_body(
    model_id: &str,
    provider: &str,
    settings: &OpenAICompatibleProviderSettings,
    options: &LanguageModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), String> {
    let mut body = JsonObject::new();
    let mut warnings = Vec::new();
    let OpenAICompatibleChatProviderOptions {
        user,
        reasoning_effort,
        text_verbosity,
        strict_json_schema,
        additional_body_options,
    } = openai_compatible_chat_provider_options(provider, options, &mut warnings);

    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));

    if let Some(max_output_tokens) = options.max_output_tokens {
        body.insert("max_tokens".to_string(), json!(max_output_tokens));
    }

    if let Some(temperature) = options.temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }

    if let Some(top_p) = options.top_p {
        body.insert("top_p".to_string(), json!(top_p));
    }

    if let Some(presence_penalty) = options.presence_penalty {
        body.insert("presence_penalty".to_string(), json!(presence_penalty));
    }

    if let Some(frequency_penalty) = options.frequency_penalty {
        body.insert("frequency_penalty".to_string(), json!(frequency_penalty));
    }

    if let Some(stop_sequences) = &options.stop_sequences {
        body.insert("stop".to_string(), json!(stop_sequences));
    }

    if let Some(seed) = options.seed {
        body.insert("seed".to_string(), json!(seed));
    }

    if options.top_k.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "topK".to_string(),
            details: None,
        });
    }

    let mut prompt = options.prompt.clone();
    if let Some(response_format) = &options.response_format {
        if let Some(value) = openai_compatible_response_format(
            response_format,
            settings,
            strict_json_schema.unwrap_or(true),
            &mut warnings,
        ) {
            body.insert("response_format".to_string(), value);
        } else if let Some(json_instruction_options) =
            openai_compatible_json_instruction_options(response_format, prompt.clone())
        {
            prompt = inject_json_instruction_into_messages(json_instruction_options);
        }
    }

    body.extend(additional_body_options);
    merge_vercel_ai_gateway_provider_options(
        &settings.name,
        options.provider_options.as_ref(),
        &mut body,
    );

    if let Some(user) = user {
        body.insert("user".to_string(), JsonValue::String(user));
    }

    if let Some(reasoning_effort) =
        reasoning_effort.or_else(|| openai_compatible_reasoning_effort(options.reasoning.as_ref()))
    {
        body.insert(
            "reasoning_effort".to_string(),
            JsonValue::String(reasoning_effort),
        );
    }

    if let Some(text_verbosity) = text_verbosity {
        body.insert("verbosity".to_string(), JsonValue::String(text_verbosity));
    }

    body.insert(
        "messages".to_string(),
        JsonValue::Array(openai_compatible_messages(&prompt)?),
    );

    let (tools, tool_choice) =
        openai_compatible_prepare_tools(&options.tools, &options.tool_choice, &mut warnings);
    if let Some(tools) = tools {
        body.insert("tools".to_string(), JsonValue::Array(tools));
    }
    if let Some(tool_choice) = tool_choice {
        body.insert("tool_choice".to_string(), tool_choice);
    }

    Ok((JsonValue::Object(body), warnings))
}

fn openai_compatible_chat_stream_request_body(
    model_id: &str,
    provider: &str,
    settings: &OpenAICompatibleProviderSettings,
    options: &LanguageModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), String> {
    let (mut body, warnings) =
        openai_compatible_chat_request_body(model_id, provider, settings, options)?;

    if let Some(body) = body.as_object_mut() {
        body.insert("stream".to_string(), JsonValue::Bool(true));

        if settings.include_usage == Some(true) {
            body.insert(
                "stream_options".to_string(),
                json!({
                    "include_usage": true
                }),
            );
        }
    }

    Ok((body, warnings))
}

fn openai_compatible_completion_request_body(
    model_id: &str,
    provider: &str,
    options: &LanguageModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), String> {
    let mut body = JsonObject::new();
    let mut warnings = Vec::new();
    let (completion_prompt, mut stop_sequences) =
        openai_compatible_completion_prompt(&options.prompt)?;

    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));

    if let Some(max_output_tokens) = options.max_output_tokens {
        body.insert("max_tokens".to_string(), json!(max_output_tokens));
    }

    if let Some(temperature) = options.temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }

    if let Some(top_p) = options.top_p {
        body.insert("top_p".to_string(), json!(top_p));
    }

    if let Some(presence_penalty) = options.presence_penalty {
        body.insert("presence_penalty".to_string(), json!(presence_penalty));
    }

    if let Some(frequency_penalty) = options.frequency_penalty {
        body.insert("frequency_penalty".to_string(), json!(frequency_penalty));
    }

    if let Some(seed) = options.seed {
        body.insert("seed".to_string(), json!(seed));
    }

    if options.top_k.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "topK".to_string(),
            details: None,
        });
    }

    if options
        .tools
        .as_ref()
        .is_some_and(|tools| !tools.is_empty())
    {
        warnings.push(Warning::Unsupported {
            feature: "tools".to_string(),
            details: None,
        });
    }

    if options.tool_choice.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "toolChoice".to_string(),
            details: None,
        });
    }

    if let Some(response_format) = &options.response_format
        && !matches!(response_format, LanguageModelResponseFormat::Text)
    {
        warnings.push(Warning::Unsupported {
            feature: "responseFormat".to_string(),
            details: Some("JSON response format is not supported.".to_string()),
        });
    }

    if let Some(provider_options) = &options.provider_options {
        for warning in
            openai_compatible_completion_provider_options(provider, provider_options, &mut body)
        {
            warnings.push(warning);
        }
    }

    if let Some(user_stop_sequences) = &options.stop_sequences {
        stop_sequences.extend(user_stop_sequences.clone());
    }

    body.insert("prompt".to_string(), JsonValue::String(completion_prompt));

    if !stop_sequences.is_empty() {
        body.insert("stop".to_string(), json!(stop_sequences));
    }

    Ok((JsonValue::Object(body), warnings))
}

fn openai_compatible_completion_stream_request_body(
    model_id: &str,
    provider: &str,
    settings: &OpenAICompatibleProviderSettings,
    options: &LanguageModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), String> {
    let (mut body, warnings) =
        openai_compatible_completion_request_body(model_id, provider, options)?;

    if let Some(body) = body.as_object_mut() {
        body.insert("stream".to_string(), JsonValue::Bool(true));

        if settings.include_usage == Some(true) {
            body.insert(
                "stream_options".to_string(),
                json!({
                    "include_usage": true
                }),
            );
        }
    }

    Ok((body, warnings))
}

fn openai_compatible_completion_prompt(
    prompt: &[LanguageModelMessage],
) -> Result<(String, Vec<String>), String> {
    let mut text = String::new();
    let mut start_index = 0;

    if let Some(LanguageModelMessage::System(message)) = prompt.first() {
        text.push_str(&message.content);
        text.push_str("\n\n");
        start_index = 1;
    }

    for message in &prompt[start_index..] {
        match message {
            LanguageModelMessage::System(message) => {
                return Err(format!(
                    "Unexpected system message in completion prompt: {}",
                    message.content
                ));
            }
            LanguageModelMessage::User(message) => {
                let user_message = message
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        ai_sdk_provider::language_model::LanguageModelUserContentPart::Text(
                            text,
                        ) => Some(text.text.as_str()),
                        ai_sdk_provider::language_model::LanguageModelUserContentPart::File(_) => {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("");
                text.push_str("user:\n");
                text.push_str(&user_message);
                text.push_str("\n\n");
            }
            LanguageModelMessage::Assistant(message) => {
                let mut assistant_message = String::new();
                for part in &message.content {
                    match part {
                        LanguageModelAssistantContentPart::Text(text) => {
                            assistant_message.push_str(&text.text);
                        }
                        LanguageModelAssistantContentPart::ToolCall(_) => {
                            return Err(
                                "OpenAI-compatible completion models do not support tool-call messages"
                                    .to_string(),
                            );
                        }
                        _ => {}
                    }
                }
                text.push_str("assistant:\n");
                text.push_str(&assistant_message);
                text.push_str("\n\n");
            }
            LanguageModelMessage::Tool(_) => {
                return Err(
                    "OpenAI-compatible completion models do not support tool messages".to_string(),
                );
            }
        }
    }

    text.push_str("assistant:\n");

    Ok((text, vec!["\nuser:".to_string()]))
}

fn openai_compatible_completion_provider_options(
    provider: &str,
    provider_options: &ProviderOptions,
    body: &mut JsonObject,
) -> Vec<Warning> {
    let mut warnings = Vec::new();
    let provider_options_name = openai_compatible_provider_options_name(provider);
    warn_if_deprecated_openai_compatible_provider_options_key(
        provider_options_name,
        Some(provider_options),
        &mut warnings,
    );

    if let Some(options) = provider_options.get(provider_options_name) {
        add_openai_compatible_completion_body_options(body, options);
    }

    let resolved_provider_options_name = resolve_openai_compatible_provider_options_key(
        provider_options_name,
        Some(provider_options),
    );
    if resolved_provider_options_name != provider_options_name
        && let Some(options) = provider_options.get(&resolved_provider_options_name)
    {
        add_openai_compatible_completion_body_options(body, options);
    }

    warnings
}

fn add_openai_compatible_completion_body_options(body: &mut JsonObject, options: &JsonObject) {
    if let Some(echo) = options.get("echo").and_then(JsonValue::as_bool) {
        body.insert("echo".to_string(), JsonValue::Bool(echo));
    }

    if let Some(logit_bias) = options.get("logitBias").filter(|value| value.is_object()) {
        body.insert("logit_bias".to_string(), logit_bias.clone());
    }

    if let Some(suffix) = options.get("suffix").and_then(JsonValue::as_str) {
        body.insert("suffix".to_string(), JsonValue::String(suffix.to_string()));
    }

    if let Some(user) = options.get("user").and_then(JsonValue::as_str) {
        body.insert("user".to_string(), JsonValue::String(user.to_string()));
    }

    for (key, value) in options {
        body.insert(key.clone(), value.clone());
    }
}

fn openai_compatible_image_warnings(options: &ImageModelCallOptions) -> Vec<Warning> {
    let mut warnings = Vec::new();

    if options.aspect_ratio.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "aspectRatio".to_string(),
            details: Some(
                "This model does not support aspect ratio. Use `size` instead.".to_string(),
            ),
        });
    }

    if options.seed.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "seed".to_string(),
            details: None,
        });
    }

    warnings
}

fn openai_compatible_image_provider_options(
    provider: &str,
    provider_options: &ProviderOptions,
    warnings: &mut Vec<Warning>,
) -> JsonObject {
    let provider_options_name = openai_compatible_provider_options_name(provider);
    let mut body_options = JsonObject::new();
    warn_if_deprecated_openai_compatible_provider_options_key(
        provider_options_name,
        Some(provider_options),
        warnings,
    );

    if let Some(options) = provider_options.get(provider_options_name) {
        body_options.extend(options.clone());
    }

    let resolved_provider_options_name = resolve_openai_compatible_provider_options_key(
        provider_options_name,
        Some(provider_options),
    );
    if resolved_provider_options_name != provider_options_name
        && let Some(options) = provider_options.get(&resolved_provider_options_name)
    {
        body_options.extend(options.clone());
    }

    merge_vercel_ai_gateway_provider_options(provider, Some(provider_options), &mut body_options);

    body_options
}

fn merge_vercel_ai_gateway_provider_options(
    provider_name: &str,
    provider_options: Option<&ProviderOptions>,
    body: &mut JsonObject,
) {
    if provider_name != "vercel-ai-gateway" {
        return;
    }

    let Some(gateway_options) = provider_options.and_then(|options| options.get("gateway")) else {
        return;
    };

    let request_provider_options = body
        .entry("providerOptions".to_string())
        .or_insert_with(|| JsonValue::Object(JsonObject::new()));

    if let JsonValue::Object(request_provider_options) = request_provider_options {
        request_provider_options
            .entry("gateway".to_string())
            .or_insert_with(|| JsonValue::Object(gateway_options.clone()));
    }
}

fn openai_compatible_image_generation_request_body(
    model_id: &str,
    options: &ImageModelCallOptions,
    provider_options: JsonObject,
) -> JsonValue {
    let mut body = JsonObject::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));

    if let Some(prompt) = &options.prompt {
        body.insert("prompt".to_string(), JsonValue::String(prompt.clone()));
    }

    body.insert("n".to_string(), json!(options.n));

    if let Some(size) = &options.size {
        body.insert("size".to_string(), JsonValue::String(size.clone()));
    }

    body.extend(provider_options);
    body.insert(
        "response_format".to_string(),
        JsonValue::String("b64_json".to_string()),
    );

    JsonValue::Object(body)
}

fn openai_compatible_image_edit_form_data(
    model_id: &str,
    options: &ImageModelCallOptions,
    provider_options: JsonObject,
) -> ai_sdk_provider_utils::FormData {
    let mut input = vec![
        (
            "model".to_string(),
            Some(FormDataInputValue::text(model_id.to_string())),
        ),
        (
            "prompt".to_string(),
            options.prompt.clone().map(FormDataInputValue::text),
        ),
        (
            "image".to_string(),
            options.files.as_ref().map(|files| {
                FormDataInputValue::array(
                    files
                        .iter()
                        .map(|file| FormDataValue::bytes(openai_compatible_image_file_bytes(file)))
                        .collect(),
                )
            }),
        ),
        (
            "mask".to_string(),
            options
                .mask
                .as_ref()
                .map(|mask| FormDataInputValue::bytes(openai_compatible_image_file_bytes(mask))),
        ),
        (
            "n".to_string(),
            Some(FormDataInputValue::text(options.n.to_string())),
        ),
        (
            "size".to_string(),
            options.size.clone().map(FormDataInputValue::text),
        ),
    ];

    for (key, value) in provider_options {
        input.push((key, openai_compatible_image_form_value(value)));
    }

    convert_to_form_data(input, ConvertToFormDataOptions::new())
}

fn openai_compatible_image_file_bytes(file: &ImageModelFile) -> Vec<u8> {
    match file {
        ImageModelFile::File { data, .. } => match data {
            FileDataContent::Bytes(bytes) => bytes.clone(),
            FileDataContent::Base64(base64) => {
                convert_base64_to_bytes(base64).unwrap_or_else(|_| base64.as_bytes().to_vec())
            }
        },
        ImageModelFile::Url { url, .. } => url.as_str().as_bytes().to_vec(),
    }
}

fn openai_compatible_image_form_value(value: JsonValue) -> Option<FormDataInputValue> {
    match value {
        JsonValue::Null => None,
        JsonValue::String(value) => Some(FormDataInputValue::text(value)),
        JsonValue::Bool(value) => Some(FormDataInputValue::text(value.to_string())),
        JsonValue::Number(value) => Some(FormDataInputValue::text(value.to_string())),
        JsonValue::Array(values) => Some(FormDataInputValue::array(
            values
                .into_iter()
                .filter_map(|value| {
                    openai_compatible_image_form_value(value).and_then(|value| match value {
                        FormDataInputValue::Text { value } => Some(FormDataValue::text(value)),
                        FormDataInputValue::Bytes { value } => Some(FormDataValue::bytes(value)),
                        FormDataInputValue::Array { .. } => None,
                    })
                })
                .collect(),
        )),
        JsonValue::Object(value) => Some(FormDataInputValue::text(
            JsonValue::Object(value).to_string(),
        )),
    }
}

fn openai_compatible_embedding_request_body(
    model_id: &str,
    provider: &str,
    options: &EmbeddingModelCallOptions,
) -> (JsonValue, Vec<Warning>) {
    let mut body = JsonObject::new();
    let mut warnings = Vec::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    body.insert("input".to_string(), json!(&options.values));
    body.insert(
        "encoding_format".to_string(),
        JsonValue::String("float".to_string()),
    );

    if let Some(provider_options) = &options.provider_options {
        for warning in
            openai_compatible_embedding_provider_options(provider, provider_options, &mut body)
        {
            warnings.push(warning);
        }
    }

    (JsonValue::Object(body), warnings)
}

fn openai_compatible_embedding_provider_options(
    provider: &str,
    provider_options: &ProviderOptions,
    body: &mut JsonObject,
) -> Vec<Warning> {
    let mut warnings = Vec::new();

    if let Some(options) = provider_options.get("openai-compatible") {
        warnings.push(Warning::Deprecated {
            setting: "providerOptions key 'openai-compatible'".to_string(),
            message: "Use 'openaiCompatible' instead.".to_string(),
        });
        add_openai_compatible_embedding_body_options(body, options);
    }

    if let Some(options) = provider_options.get("openaiCompatible") {
        add_openai_compatible_embedding_body_options(body, options);
    }

    let provider_options_name = openai_compatible_provider_options_name(provider);
    warn_if_deprecated_openai_compatible_provider_options_key(
        provider_options_name,
        Some(provider_options),
        &mut warnings,
    );

    if let Some(options) = provider_options.get(provider_options_name) {
        add_openai_compatible_embedding_body_options(body, options);
    }

    let resolved_provider_options_name = resolve_openai_compatible_provider_options_key(
        provider_options_name,
        Some(provider_options),
    );
    if resolved_provider_options_name != provider_options_name
        && let Some(options) = provider_options.get(&resolved_provider_options_name)
    {
        add_openai_compatible_embedding_body_options(body, options);
    }

    merge_vercel_ai_gateway_provider_options(provider, Some(provider_options), body);

    warnings
}

fn add_openai_compatible_embedding_body_options(body: &mut JsonObject, options: &JsonObject) {
    if let Some(dimensions) = options.get("dimensions").filter(|value| value.is_number()) {
        body.insert("dimensions".to_string(), dimensions.clone());
    }

    if let Some(user) = options.get("user").and_then(JsonValue::as_str) {
        body.insert("user".to_string(), JsonValue::String(user.to_string()));
    }
}

fn to_openai_compatible_camel_case(value: &str) -> String {
    let mut output = String::new();
    let mut uppercase_next = false;

    for character in value.chars() {
        if matches!(character, '-' | '_') {
            uppercase_next = true;
            continue;
        }

        if uppercase_next {
            output.extend(character.to_uppercase());
            uppercase_next = false;
        } else {
            output.push(character);
        }
    }

    output
}

fn openai_compatible_provider_options_name(provider: &str) -> &str {
    provider.split('.').next().unwrap_or(provider).trim()
}

fn resolve_openai_compatible_provider_options_key(
    raw_name: &str,
    provider_options: Option<&ProviderOptions>,
) -> String {
    let camel_name = to_openai_compatible_camel_case(raw_name);

    if camel_name != raw_name
        && provider_options
            .and_then(|provider_options| provider_options.get(&camel_name))
            .is_some()
    {
        camel_name
    } else {
        raw_name.to_string()
    }
}

fn warn_if_deprecated_openai_compatible_provider_options_key(
    raw_name: &str,
    provider_options: Option<&ProviderOptions>,
    warnings: &mut Vec<Warning>,
) {
    let camel_name = to_openai_compatible_camel_case(raw_name);

    if camel_name != raw_name
        && provider_options
            .and_then(|provider_options| provider_options.get(raw_name))
            .is_some()
    {
        warnings.push(Warning::Deprecated {
            setting: format!("providerOptions key '{raw_name}'"),
            message: format!("Use '{camel_name}' instead."),
        });
    }
}

#[derive(Clone, Debug, Default)]
struct OpenAICompatibleChatProviderOptions {
    user: Option<String>,
    reasoning_effort: Option<String>,
    text_verbosity: Option<String>,
    strict_json_schema: Option<bool>,
    additional_body_options: JsonObject,
}

fn openai_compatible_chat_provider_options(
    provider_name: &str,
    options: &LanguageModelCallOptions,
    warnings: &mut Vec<Warning>,
) -> OpenAICompatibleChatProviderOptions {
    let Some(provider_options) = options.provider_options.as_ref() else {
        return OpenAICompatibleChatProviderOptions::default();
    };

    let mut resolved = OpenAICompatibleChatProviderOptions::default();

    if let Some(options) = provider_options.get("openai-compatible") {
        warnings.push(Warning::Deprecated {
            setting: "providerOptions key 'openai-compatible'".to_string(),
            message: "Use 'openaiCompatible' instead.".to_string(),
        });
        merge_openai_compatible_chat_known_options(&mut resolved, options);
    }

    if let Some(options) = provider_options.get("openaiCompatible") {
        merge_openai_compatible_chat_known_options(&mut resolved, options);
    }

    let provider_options_name = openai_compatible_provider_options_name(provider_name);
    warn_if_deprecated_openai_compatible_provider_options_key(
        provider_options_name,
        Some(provider_options),
        warnings,
    );

    if let Some(options) = provider_options.get(provider_options_name) {
        merge_openai_compatible_chat_known_options(&mut resolved, options);
        merge_openai_compatible_chat_additional_options(
            &mut resolved.additional_body_options,
            options,
        );
    }

    let resolved_provider_options_name = resolve_openai_compatible_provider_options_key(
        provider_options_name,
        Some(provider_options),
    );
    if resolved_provider_options_name != provider_options_name
        && let Some(options) = provider_options.get(&resolved_provider_options_name)
    {
        merge_openai_compatible_chat_known_options(&mut resolved, options);
        merge_openai_compatible_chat_additional_options(
            &mut resolved.additional_body_options,
            options,
        );
    }

    resolved
}

fn merge_openai_compatible_chat_known_options(
    resolved: &mut OpenAICompatibleChatProviderOptions,
    options: &JsonObject,
) {
    if let Some(user) = options.get("user").and_then(JsonValue::as_str) {
        resolved.user = Some(user.to_string());
    }

    if let Some(reasoning_effort) = options.get("reasoningEffort").and_then(JsonValue::as_str) {
        resolved.reasoning_effort = Some(reasoning_effort.to_string());
    }

    if let Some(text_verbosity) = options.get("textVerbosity").and_then(JsonValue::as_str) {
        resolved.text_verbosity = Some(text_verbosity.to_string());
    }

    if let Some(strict_json_schema) = options.get("strictJsonSchema").and_then(JsonValue::as_bool) {
        resolved.strict_json_schema = Some(strict_json_schema);
    }
}

fn merge_openai_compatible_chat_additional_options(body: &mut JsonObject, options: &JsonObject) {
    for (key, value) in options {
        if !matches!(
            key.as_str(),
            "user" | "reasoningEffort" | "textVerbosity" | "strictJsonSchema"
        ) {
            body.insert(key.clone(), value.clone());
        }
    }
}

fn openai_compatible_reasoning_effort(
    reasoning: Option<&LanguageModelReasoningEffort>,
) -> Option<String> {
    match reasoning? {
        LanguageModelReasoningEffort::ProviderDefault | LanguageModelReasoningEffort::None => None,
        LanguageModelReasoningEffort::Minimal => Some("minimal".to_string()),
        LanguageModelReasoningEffort::Low => Some("low".to_string()),
        LanguageModelReasoningEffort::Medium => Some("medium".to_string()),
        LanguageModelReasoningEffort::High => Some("high".to_string()),
        LanguageModelReasoningEffort::Xhigh => Some("xhigh".to_string()),
    }
}

fn openai_compatible_prepare_tools(
    tools: &Option<Vec<LanguageModelTool>>,
    tool_choice: &Option<LanguageModelToolChoice>,
    warnings: &mut Vec<Warning>,
) -> (Option<Vec<JsonValue>>, Option<JsonValue>) {
    let Some(tools) = tools.as_ref().filter(|tools| !tools.is_empty()) else {
        return (None, None);
    };

    let prepared_tools = tools
        .iter()
        .filter_map(|tool| match tool {
            LanguageModelTool::Function(tool) => {
                let mut function = JsonObject::new();
                function.insert("name".to_string(), JsonValue::String(tool.name.clone()));

                if let Some(description) = &tool.description {
                    function.insert(
                        "description".to_string(),
                        JsonValue::String(description.clone()),
                    );
                }

                function.insert(
                    "parameters".to_string(),
                    JsonValue::Object(tool.input_schema.clone()),
                );

                if let Some(strict) = tool.strict {
                    function.insert("strict".to_string(), JsonValue::Bool(strict));
                }

                Some(json!({
                    "type": "function",
                    "function": function
                }))
            }
            LanguageModelTool::Provider(tool) => {
                warnings.push(Warning::Unsupported {
                    feature: format!("provider-defined tool {}", tool.id),
                    details: None,
                });
                None
            }
        })
        .collect::<Vec<_>>();

    let prepared_tool_choice = tool_choice.as_ref().map(|choice| match choice {
        LanguageModelToolChoice::Auto => JsonValue::String("auto".to_string()),
        LanguageModelToolChoice::None => JsonValue::String("none".to_string()),
        LanguageModelToolChoice::Required => JsonValue::String("required".to_string()),
        LanguageModelToolChoice::Tool { tool_name } => json!({
            "type": "function",
            "function": {
                "name": tool_name
            }
        }),
    });

    (Some(prepared_tools), prepared_tool_choice)
}

fn openai_compatible_response_format(
    response_format: &LanguageModelResponseFormat,
    settings: &OpenAICompatibleProviderSettings,
    strict_json_schema: bool,
    warnings: &mut Vec<Warning>,
) -> Option<JsonValue> {
    match response_format {
        LanguageModelResponseFormat::Text => None,
        LanguageModelResponseFormat::Json {
            schema,
            name,
            description,
        } => {
            if let Some(schema) = schema
                && settings.supports_structured_outputs == Some(true)
            {
                let mut json_schema = JsonObject::new();
                json_schema.insert("schema".to_string(), JsonValue::Object(schema.clone()));
                json_schema.insert("strict".to_string(), JsonValue::Bool(strict_json_schema));
                json_schema.insert(
                    "name".to_string(),
                    JsonValue::String(name.clone().unwrap_or_else(|| "response".to_string())),
                );

                if let Some(description) = description {
                    json_schema.insert(
                        "description".to_string(),
                        JsonValue::String(description.clone()),
                    );
                }

                return Some(json!({
                    "type": "json_schema",
                    "json_schema": json_schema
                }));
            }

            if schema.is_some() {
                warnings.push(Warning::Unsupported {
                    feature: "responseFormat".to_string(),
                    details: Some(
                        "JSON response format schema is only supported with structuredOutputs"
                            .to_string(),
                    ),
                });
            }

            if settings.supports_json_object_response_format == Some(false) {
                warnings.push(Warning::Unsupported {
                    feature: "responseFormat".to_string(),
                    details: Some(
                        "JSON response_format is not supported; JSON instructions were injected into the prompt."
                            .to_string(),
                    ),
                });
                return None;
            }

            Some(json!({
                "type": "json_object"
            }))
        }
    }
}

fn openai_compatible_json_instruction_options(
    response_format: &LanguageModelResponseFormat,
    messages: Vec<LanguageModelMessage>,
) -> Option<InjectJsonInstructionIntoMessagesOptions> {
    match response_format {
        LanguageModelResponseFormat::Text => None,
        LanguageModelResponseFormat::Json { schema, .. } => {
            let mut options = InjectJsonInstructionIntoMessagesOptions::new(messages);
            if let Some(schema) = schema {
                options = options.with_schema(schema.clone());
            }
            Some(options)
        }
    }
}

fn openai_compatible_messages(prompt: &[LanguageModelMessage]) -> Result<Vec<JsonValue>, String> {
    let mut messages = Vec::new();

    for message in prompt {
        match message {
            LanguageModelMessage::System(message) => {
                let mut object = JsonObject::new();
                object.insert("role".to_string(), JsonValue::String("system".to_string()));
                object.insert(
                    "content".to_string(),
                    JsonValue::String(message.content.clone()),
                );
                openai_compatible_insert_metadata(&mut object, message.provider_options.as_ref());
                messages.push(JsonValue::Object(object));
            }
            LanguageModelMessage::User(message) => {
                messages.push(openai_compatible_user_message(message)?);
            }
            LanguageModelMessage::Assistant(message) => {
                messages.push(openai_compatible_assistant_message(message));
            }
            LanguageModelMessage::Tool(message) => {
                for part in &message.content {
                    if let ai_sdk_provider::language_model::LanguageModelToolContentPart::ToolResult(
                        tool_result,
                    ) = part
                    {
                        messages.push(openai_compatible_tool_message(tool_result));
                    }
                }
            }
        }
    }

    Ok(messages)
}

fn openai_compatible_user_message(
    message: &ai_sdk_provider::language_model::LanguageModelUserMessage,
) -> Result<JsonValue, String> {
    if let [ai_sdk_provider::language_model::LanguageModelUserContentPart::Text(text_part)] =
        message.content.as_slice()
    {
        let mut object = JsonObject::new();
        object.insert("role".to_string(), JsonValue::String("user".to_string()));
        object.insert(
            "content".to_string(),
            JsonValue::String(text_part.text.clone()),
        );
        openai_compatible_insert_metadata(&mut object, text_part.provider_options.as_ref());
        return Ok(JsonValue::Object(object));
    }

    let mut object = JsonObject::new();
    object.insert("role".to_string(), JsonValue::String("user".to_string()));
    object.insert(
        "content".to_string(),
        JsonValue::Array(
            message
                .content
                .iter()
                .map(openai_compatible_user_content_part)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    openai_compatible_insert_metadata(&mut object, message.provider_options.as_ref());
    Ok(JsonValue::Object(object))
}

fn openai_compatible_user_content_part(
    part: &ai_sdk_provider::language_model::LanguageModelUserContentPart,
) -> Result<JsonValue, String> {
    match part {
        ai_sdk_provider::language_model::LanguageModelUserContentPart::Text(text) => {
            let mut object = JsonObject::new();
            object.insert("type".to_string(), JsonValue::String("text".to_string()));
            object.insert("text".to_string(), JsonValue::String(text.text.clone()));
            openai_compatible_insert_metadata(&mut object, text.provider_options.as_ref());
            Ok(JsonValue::Object(object))
        }
        ai_sdk_provider::language_model::LanguageModelUserContentPart::File(file) => {
            openai_compatible_user_file_part(file)
        }
    }
}

fn openai_compatible_user_file_part(
    part: &ai_sdk_provider::language_model::LanguageModelFilePart,
) -> Result<JsonValue, String> {
    match &part.data {
        FileData::Reference { .. } => {
            return Err(openai_compatible_unsupported_functionality(
                "file parts with provider references",
            ));
        }
        FileData::Text { .. } => {
            return Err(openai_compatible_unsupported_functionality(
                "text file parts",
            ));
        }
        FileData::Url { .. } | FileData::Data { .. } => {}
    }

    let top_level = openai_compatible_top_level_media_type(&part.media_type);
    match top_level {
        "image" => openai_compatible_image_part(part),
        "audio" => openai_compatible_audio_part(part),
        "application" => openai_compatible_application_part(part),
        "text" => openai_compatible_text_part(part),
        _ => Err(openai_compatible_unsupported_functionality(format!(
            "file part media type {}",
            part.media_type
        ))),
    }
}

fn openai_compatible_image_part(
    part: &ai_sdk_provider::language_model::LanguageModelFilePart,
) -> Result<JsonValue, String> {
    let url = match &part.data {
        FileData::Url { url } => url.to_string(),
        FileData::Data { data } => format!(
            "data:{};base64,{}",
            openai_compatible_resolve_full_media_type(part),
            convert_to_base64(data)
        ),
        FileData::Reference { .. } | FileData::Text { .. } => unreachable!(),
    };
    let mut object = JsonObject::new();
    object.insert(
        "type".to_string(),
        JsonValue::String("image_url".to_string()),
    );
    object.insert(
        "image_url".to_string(),
        json!({
            "url": url
        }),
    );
    openai_compatible_insert_metadata(&mut object, part.provider_options.as_ref());
    Ok(JsonValue::Object(object))
}

fn openai_compatible_audio_part(
    part: &ai_sdk_provider::language_model::LanguageModelFilePart,
) -> Result<JsonValue, String> {
    let FileData::Data { data } = &part.data else {
        return Err(openai_compatible_unsupported_functionality(
            "audio file parts with URLs",
        ));
    };
    let full_media_type = openai_compatible_resolve_full_media_type(part);
    let Some(format) = openai_compatible_audio_format(&full_media_type) else {
        return Err(openai_compatible_unsupported_functionality(format!(
            "audio media type {full_media_type}"
        )));
    };

    let mut object = JsonObject::new();
    object.insert(
        "type".to_string(),
        JsonValue::String("input_audio".to_string()),
    );
    object.insert(
        "input_audio".to_string(),
        json!({
            "data": convert_to_base64(data),
            "format": format
        }),
    );
    openai_compatible_insert_metadata(&mut object, part.provider_options.as_ref());
    Ok(JsonValue::Object(object))
}

fn openai_compatible_application_part(
    part: &ai_sdk_provider::language_model::LanguageModelFilePart,
) -> Result<JsonValue, String> {
    let FileData::Data { data } = &part.data else {
        return Err(openai_compatible_unsupported_functionality(
            "PDF file parts with URLs",
        ));
    };
    let full_media_type = openai_compatible_resolve_full_media_type(part);
    if full_media_type != "application/pdf" {
        return Err(openai_compatible_unsupported_functionality(format!(
            "file part media type {full_media_type}"
        )));
    }

    let mut object = JsonObject::new();
    object.insert("type".to_string(), JsonValue::String("file".to_string()));
    object.insert(
        "file".to_string(),
        json!({
            "filename": part
                .filename
                .clone()
                .unwrap_or_else(|| "document.pdf".to_string()),
            "file_data": format!("data:application/pdf;base64,{}", convert_to_base64(data))
        }),
    );
    openai_compatible_insert_metadata(&mut object, part.provider_options.as_ref());
    Ok(JsonValue::Object(object))
}

fn openai_compatible_text_part(
    part: &ai_sdk_provider::language_model::LanguageModelFilePart,
) -> Result<JsonValue, String> {
    let text = match &part.data {
        FileData::Url { url } => url.to_string(),
        FileData::Data { data } => openai_compatible_text_file_content(data)?,
        FileData::Reference { .. } | FileData::Text { .. } => unreachable!(),
    };
    let mut object = JsonObject::new();
    object.insert("type".to_string(), JsonValue::String("text".to_string()));
    object.insert("text".to_string(), JsonValue::String(text));
    openai_compatible_insert_metadata(&mut object, part.provider_options.as_ref());
    Ok(JsonValue::Object(object))
}

fn openai_compatible_text_file_content(data: &FileDataContent) -> Result<String, String> {
    match data {
        FileDataContent::Bytes(bytes) => Ok(String::from_utf8_lossy(bytes).into_owned()),
        FileDataContent::Base64(base64) => {
            let bytes = convert_base64_to_bytes(base64).map_err(|_| {
                openai_compatible_unsupported_functionality("invalid base64 text file parts")
            })?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
    }
}

fn openai_compatible_assistant_message(
    message: &ai_sdk_provider::language_model::LanguageModelAssistantMessage,
) -> JsonValue {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();

    for part in &message.content {
        match part {
            LanguageModelAssistantContentPart::Text(text_part) => {
                text.push_str(&text_part.text);
            }
            LanguageModelAssistantContentPart::Reasoning(reasoning_part) => {
                reasoning.push_str(&reasoning_part.text);
            }
            LanguageModelAssistantContentPart::ToolCall(tool_call) => {
                tool_calls.push(openai_compatible_tool_call_prompt_part(tool_call));
            }
            LanguageModelAssistantContentPart::File(_)
            | LanguageModelAssistantContentPart::Custom(_)
            | LanguageModelAssistantContentPart::ReasoningFile(_)
            | LanguageModelAssistantContentPart::ToolResult(_)
            | LanguageModelAssistantContentPart::ToolApprovalRequest(_) => {}
        }
    }

    let mut object = JsonObject::new();
    object.insert(
        "role".to_string(),
        JsonValue::String("assistant".to_string()),
    );
    object.insert(
        "content".to_string(),
        if tool_calls.is_empty() || !text.is_empty() {
            JsonValue::String(text)
        } else {
            JsonValue::Null
        },
    );
    if !reasoning.is_empty() {
        object.insert(
            "reasoning_content".to_string(),
            JsonValue::String(reasoning),
        );
    }
    if !tool_calls.is_empty() {
        object.insert("tool_calls".to_string(), JsonValue::Array(tool_calls));
    }
    openai_compatible_insert_metadata(&mut object, message.provider_options.as_ref());
    JsonValue::Object(object)
}

fn openai_compatible_tool_call_prompt_part(
    tool_call: &ai_sdk_provider::language_model::LanguageModelToolCallPart,
) -> JsonValue {
    let mut object = JsonObject::new();
    object.insert(
        "id".to_string(),
        JsonValue::String(tool_call.tool_call_id.clone()),
    );
    object.insert(
        "type".to_string(),
        JsonValue::String("function".to_string()),
    );
    object.insert(
        "function".to_string(),
        json!({
            "name": tool_call.tool_name.clone(),
            "arguments": tool_call.input.to_string()
        }),
    );
    openai_compatible_insert_metadata(&mut object, tool_call.provider_options.as_ref());
    if let Some(thought_signature) =
        openai_compatible_google_thought_signature(tool_call.provider_options.as_ref())
    {
        object.insert(
            "extra_content".to_string(),
            json!({
                "google": {
                    "thought_signature": thought_signature
                }
            }),
        );
    }
    JsonValue::Object(object)
}

fn openai_compatible_tool_message(
    tool_result: &ai_sdk_provider::language_model::LanguageModelToolResultPart,
) -> JsonValue {
    let mut object = JsonObject::new();
    object.insert("role".to_string(), JsonValue::String("tool".to_string()));
    object.insert(
        "content".to_string(),
        JsonValue::String(openai_compatible_tool_result_content(&tool_result.output)),
    );
    object.insert(
        "tool_call_id".to_string(),
        JsonValue::String(tool_result.tool_call_id.clone()),
    );
    openai_compatible_insert_metadata(&mut object, tool_result.provider_options.as_ref());
    JsonValue::Object(object)
}

fn openai_compatible_tool_result_content(
    output: &ai_sdk_provider::language_model::LanguageModelToolResultOutput,
) -> String {
    match output {
        ai_sdk_provider::language_model::LanguageModelToolResultOutput::Text { value, .. }
        | ai_sdk_provider::language_model::LanguageModelToolResultOutput::ErrorText {
            value, ..
        } => value.clone(),
        ai_sdk_provider::language_model::LanguageModelToolResultOutput::ExecutionDenied {
            reason,
            ..
        } => reason
            .clone()
            .unwrap_or_else(|| "Tool call execution denied.".to_string()),
        ai_sdk_provider::language_model::LanguageModelToolResultOutput::Json { value, .. }
        | ai_sdk_provider::language_model::LanguageModelToolResultOutput::ErrorJson {
            value, ..
        } => value.to_string(),
        ai_sdk_provider::language_model::LanguageModelToolResultOutput::Content { value } => {
            serde_json::to_string(value).unwrap_or_else(|_| "[]".to_string())
        }
    }
}

fn openai_compatible_top_level_media_type(media_type: &str) -> &str {
    media_type
        .split_once('/')
        .map_or(media_type, |(top_level, _)| top_level)
}

fn openai_compatible_resolve_full_media_type(
    part: &ai_sdk_provider::language_model::LanguageModelFilePart,
) -> String {
    let top_level = openai_compatible_top_level_media_type(&part.media_type);
    if !part.media_type.ends_with("/*") && part.media_type.contains('/') {
        return part.media_type.clone();
    }
    if let FileData::Data { data } = &part.data
        && let Some(detected_media_type) = detect_media_type(data, Some(top_level))
    {
        return detected_media_type.to_string();
    }
    part.media_type.clone()
}

fn openai_compatible_audio_format(media_type: &str) -> Option<&'static str> {
    match media_type {
        "audio/wav" => Some("wav"),
        "audio/mp3" | "audio/mpeg" => Some("mp3"),
        _ => None,
    }
}

fn openai_compatible_insert_metadata(
    object: &mut JsonObject,
    provider_options: Option<&ProviderOptions>,
) {
    if let Some(metadata) =
        provider_options.and_then(|provider_options| provider_options.get("openaiCompatible"))
    {
        object.extend(metadata.clone());
    }
}

fn openai_compatible_google_thought_signature(
    provider_options: Option<&ProviderOptions>,
) -> Option<String> {
    let value = provider_options
        .and_then(|provider_options| provider_options.get("google"))
        .and_then(|google| google.get("thoughtSignature"))?;
    Some(match value {
        JsonValue::String(value) => value.clone(),
        other => other.to_string(),
    })
}

fn openai_compatible_unsupported_functionality(functionality: impl AsRef<str>) -> String {
    format!("'{}' functionality not supported", functionality.as_ref())
}

fn openai_compatible_tool_call_metadata(
    provider_name: &str,
    tool_call: &JsonValue,
) -> Option<ProviderMetadata> {
    let thought_signature = tool_call
        .get("extra_content")
        .and_then(|extra| extra.get("google"))
        .and_then(|google| google.get("thought_signature"))
        .and_then(JsonValue::as_str)?;
    let mut metadata = ProviderMetadata::new();
    metadata.insert(
        provider_name.to_string(),
        json!({
            "thoughtSignature": thought_signature
        })
        .as_object()
        .expect("metadata is an object")
        .clone(),
    );
    Some(metadata)
}

fn openai_compatible_response_content(
    message: Option<&JsonValue>,
    provider_name: &str,
) -> Vec<LanguageModelContent> {
    let mut content = Vec::new();
    let Some(message) = message else {
        return content;
    };

    if let Some(text) = message.get("content").and_then(JsonValue::as_str)
        && !text.is_empty()
    {
        content.push(LanguageModelContent::Text(LanguageModelText::new(text)));
    }

    if let Some(reasoning) = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(JsonValue::as_str)
        && !reasoning.is_empty()
    {
        content.push(LanguageModelContent::Reasoning(
            LanguageModelReasoning::new(reasoning),
        ));
    }

    for file in openai_compatible_image_files(message.get("images")) {
        content.push(LanguageModelContent::File(file));
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(JsonValue::as_array) {
        for (index, tool_call) in tool_calls.iter().enumerate() {
            let Some(function) = tool_call.get("function") else {
                continue;
            };
            let Some(tool_name) = function.get("name").and_then(JsonValue::as_str) else {
                continue;
            };
            let input = function
                .get("arguments")
                .and_then(JsonValue::as_str)
                .unwrap_or_default();
            let tool_call_id = tool_call
                .get("id")
                .and_then(JsonValue::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if index == 0 {
                        generate_id()
                    } else {
                        format!("{}-{index}", generate_id())
                    }
                });
            let mut content_part =
                LanguageModelToolCall::new(tool_call_id, tool_name.to_string(), input.to_string());

            if let Some(provider_metadata) =
                openai_compatible_tool_call_metadata(provider_name, tool_call)
            {
                content_part = content_part.with_provider_metadata(provider_metadata);
            }

            content.push(LanguageModelContent::ToolCall(content_part));
        }
    }

    content
}

fn openai_compatible_image_files(value: Option<&JsonValue>) -> Vec<LanguageModelFile> {
    value
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(openai_compatible_image_file)
        .collect()
}

fn openai_compatible_image_file(value: &JsonValue) -> Option<LanguageModelFile> {
    if value.get("type").and_then(JsonValue::as_str) != Some("image_url") {
        return None;
    }

    let url = value
        .get("image_url")
        .and_then(|image_url| image_url.get("url"))
        .and_then(JsonValue::as_str)?;

    Some(openai_compatible_image_url_file(url))
}

fn openai_compatible_image_url_file(url: &str) -> LanguageModelFile {
    if let Some((header, base64)) = url.split_once(',')
        && let Some(media_type) = header
            .strip_prefix("data:")
            .and_then(|header| header.split(';').next())
            .filter(|media_type| !media_type.is_empty())
    {
        return LanguageModelFile::new(
            media_type,
            LanguageModelFileData::Data {
                data: FileDataContent::Base64(base64.to_string()),
            },
        );
    }

    if let Ok(url) = Url::parse(url) {
        return LanguageModelFile::new("image/*", LanguageModelFileData::Url { url });
    }

    LanguageModelFile::new(
        "image/*",
        LanguageModelFileData::Data {
            data: FileDataContent::Base64(url.to_string()),
        },
    )
}

fn openai_compatible_finish_reason(value: Option<&JsonValue>) -> LanguageModelFinishReason {
    let raw = json_string(value).unwrap_or_else(|| "unknown".to_string());
    let unified = match raw.as_str() {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "content_filter" => FinishReason::ContentFilter,
        "tool_calls" => FinishReason::ToolCalls,
        "error" => FinishReason::Error,
        _ => FinishReason::Other,
    };

    LanguageModelFinishReason {
        unified,
        raw: Some(raw),
    }
}

fn openai_compatible_usage(value: Option<&JsonValue>) -> LanguageModelUsage {
    let Some(value) = value else {
        return LanguageModelUsage::default();
    };

    let input_total = json_u64(
        value
            .get("prompt_tokens")
            .or_else(|| value.get("promptTokens"))
            .or_else(|| value.get("input_tokens"))
            .or_else(|| value.get("inputTokens")),
    );
    let output_total = json_u64(
        value
            .get("completion_tokens")
            .or_else(|| value.get("completionTokens"))
            .or_else(|| value.get("output_tokens"))
            .or_else(|| value.get("outputTokens")),
    );
    let cache_read = json_u64(value.get("prompt_tokens_details").and_then(|details| {
        details
            .get("cached_tokens")
            .or_else(|| details.get("cachedTokens"))
    }));
    let reasoning_tokens = json_u64(
        value
            .get("completion_tokens_details")
            .and_then(|details| {
                details
                    .get("reasoning_tokens")
                    .or_else(|| details.get("reasoningTokens"))
            })
            .or_else(|| value.get("reasoning_tokens"))
            .or_else(|| value.get("reasoningTokens")),
    );
    let raw = value.as_object().cloned();

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: input_total,
            no_cache: input_total
                .zip(cache_read)
                .map(|(total, cached)| total.saturating_sub(cached)),
            cache_read,
            cache_write: None,
        },
        output_tokens: OutputTokenUsage {
            total: output_total,
            text: output_total
                .map(|total| total.saturating_sub(reasoning_tokens.unwrap_or_default())),
            reasoning: reasoning_tokens,
        },
        raw,
    }
}

async fn openai_compatible_provider_metadata(
    provider_name: &str,
    response: &JsonValue,
    metadata_extractor: Option<&OpenAICompatibleMetadataExtractor>,
) -> ProviderMetadata {
    let mut metadata = if let Some(metadata_extractor) = metadata_extractor {
        metadata_extractor
            .extract_metadata(response.clone())
            .await
            .unwrap_or_default()
    } else {
        ProviderMetadata::new()
    };

    add_openai_compatible_provider_prediction_metadata(
        provider_name,
        &mut metadata,
        response.get("usage"),
        true,
    );

    metadata
}

fn openai_compatible_completion_stream_result_from_response(
    events: Vec<ParseJsonResult<JsonValue>>,
    response_headers: Option<Headers>,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    include_raw_chunks: bool,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let mut stream = vec![LanguageModelStreamPart::StreamStart(
        LanguageModelStreamStart::new(warnings),
    )];
    let mut finish_reason = LanguageModelFinishReason {
        unified: FinishReason::Other,
        raw: None,
    };
    let mut usage = None::<JsonValue>;
    let mut is_first_chunk = true;
    let mut is_active_text = false;

    for event in events {
        match event {
            ParseJsonResult::Success { value, raw_value } => {
                if include_raw_chunks {
                    stream.push(LanguageModelStreamPart::Raw(
                        LanguageModelRawStreamPart::new(raw_value.clone()),
                    ));
                }

                if value.get("error").is_some() {
                    finish_reason = LanguageModelFinishReason {
                        unified: FinishReason::Error,
                        raw: Some("openai-compatible-error".to_string()),
                    };
                    stream.push(openai_compatible_stream_error(
                        openai_compatible_error_message(&value),
                        Some(&raw_value.to_string()),
                    ));
                    continue;
                }

                if is_first_chunk {
                    is_first_chunk = false;
                    stream.push(LanguageModelStreamPart::ResponseMetadata(
                        openai_compatible_stream_response_metadata(&value),
                    ));
                    stream.push(LanguageModelStreamPart::TextStart(
                        LanguageModelTextStart::new("0"),
                    ));
                    is_active_text = true;
                }

                if let Some(event_usage) = value.get("usage") {
                    usage = Some(event_usage.clone());
                }

                let Some(choice) = value
                    .get("choices")
                    .and_then(JsonValue::as_array)
                    .and_then(|choices| choices.first())
                else {
                    continue;
                };

                if let Some(raw_finish_reason) =
                    choice.get("finish_reason").filter(|value| !value.is_null())
                {
                    finish_reason = openai_compatible_finish_reason(Some(raw_finish_reason));
                }

                if let Some(text) = choice.get("text").and_then(JsonValue::as_str) {
                    stream.push(LanguageModelStreamPart::TextDelta(
                        LanguageModelTextDelta::new("0", text),
                    ));
                }
            }
            ParseJsonResult::Failure { error, raw_value } => {
                finish_reason = LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("openai-compatible-parse-error".to_string()),
                };
                stream.push(openai_compatible_stream_error(
                    error.to_string(),
                    raw_value.as_ref().map(JsonValue::to_string).as_deref(),
                ));
            }
        }
    }

    if is_active_text {
        stream.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
            "0",
        )));
    }

    stream.push(LanguageModelStreamPart::Finish(
        LanguageModelStreamFinish::new(openai_compatible_usage(usage.as_ref()), finish_reason),
    ));

    let mut result = LanguageModelStreamResult::new(stream)
        .with_request(LanguageModelRequest::new().with_body(request_body));

    if let Some(headers) = response_headers {
        result = result.with_response(with_stream_response_headers(
            LanguageModelStreamResultResponse::new(),
            headers,
        ));
    }

    result
}

fn openai_compatible_stream_result_from_response(
    provider_name: &str,
    events: Vec<ParseJsonResult<JsonValue>>,
    response_headers: Option<Headers>,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    include_raw_chunks: bool,
    metadata_extractor: Option<OpenAICompatibleStreamMetadataExtractor>,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let mut stream = vec![LanguageModelStreamPart::StreamStart(
        LanguageModelStreamStart::new(warnings),
    )];
    let mut finish_reason = LanguageModelFinishReason {
        unified: FinishReason::Other,
        raw: None,
    };
    let mut usage = None::<JsonValue>;
    let mut is_first_chunk = true;
    let mut is_active_reasoning = false;
    let mut is_active_text = false;
    let tool_metadata_provider_name = provider_name.to_string();
    let mut tool_call_tracker = StreamingToolCallTracker::new()
        .with_generate_id(generate_id)
        .with_extract_metadata(move |delta| {
            openai_compatible_streaming_tool_call_metadata(&tool_metadata_provider_name, delta)
        })
        .with_tool_call_provider_metadata(|metadata| metadata.cloned());
    let mut pending_tool_calls = BTreeMap::<usize, PendingOpenAICompatibleToolCall>::new();
    let mut forwarded_tool_call_indices = BTreeSet::<usize>::new();

    for event in events {
        match event {
            ParseJsonResult::Success { value, raw_value } => {
                if include_raw_chunks {
                    stream.push(LanguageModelStreamPart::Raw(
                        LanguageModelRawStreamPart::new(raw_value.clone()),
                    ));
                }

                if let Some(metadata_extractor) = metadata_extractor.as_ref() {
                    metadata_extractor.process_chunk(&raw_value);
                }

                if value.get("error").is_some() {
                    finish_reason = LanguageModelFinishReason {
                        unified: FinishReason::Error,
                        raw: Some("openai-compatible-error".to_string()),
                    };
                    stream.push(openai_compatible_stream_error(
                        openai_compatible_error_message(&value),
                        Some(&raw_value.to_string()),
                    ));
                    continue;
                }

                if is_first_chunk {
                    is_first_chunk = false;
                    stream.push(LanguageModelStreamPart::ResponseMetadata(
                        openai_compatible_stream_response_metadata(&value),
                    ));
                }

                if let Some(event_usage) = value.get("usage") {
                    usage = Some(event_usage.clone());
                }

                let Some(choice) = value
                    .get("choices")
                    .and_then(JsonValue::as_array)
                    .and_then(|choices| choices.first())
                else {
                    continue;
                };

                if let Some(raw_finish_reason) = choice.get("finish_reason") {
                    finish_reason = openai_compatible_finish_reason(Some(raw_finish_reason));
                }

                let Some(delta) = choice.get("delta") else {
                    continue;
                };

                let reasoning = delta
                    .get("reasoning_content")
                    .or_else(|| delta.get("reasoning"))
                    .and_then(JsonValue::as_str)
                    .filter(|reasoning| !reasoning.is_empty());
                if let Some(reasoning) = reasoning {
                    if !is_active_reasoning {
                        stream.push(LanguageModelStreamPart::ReasoningStart(
                            LanguageModelReasoningStart::new("reasoning-0"),
                        ));
                        is_active_reasoning = true;
                    }

                    stream.push(LanguageModelStreamPart::ReasoningDelta(
                        LanguageModelReasoningDelta::new("reasoning-0", reasoning),
                    ));
                }

                let text = delta
                    .get("content")
                    .and_then(JsonValue::as_str)
                    .filter(|text| !text.is_empty());
                if let Some(text) = text {
                    if is_active_reasoning {
                        stream.push(LanguageModelStreamPart::ReasoningEnd(
                            LanguageModelReasoningEnd::new("reasoning-0"),
                        ));
                        is_active_reasoning = false;
                    }

                    if !is_active_text {
                        stream.push(LanguageModelStreamPart::TextStart(
                            LanguageModelTextStart::new("txt-0"),
                        ));
                        is_active_text = true;
                    }

                    stream.push(LanguageModelStreamPart::TextDelta(
                        LanguageModelTextDelta::new("txt-0", text),
                    ));
                }

                let files = openai_compatible_image_files(delta.get("images"));
                if !files.is_empty() {
                    if is_active_reasoning {
                        stream.push(LanguageModelStreamPart::ReasoningEnd(
                            LanguageModelReasoningEnd::new("reasoning-0"),
                        ));
                        is_active_reasoning = false;
                    }

                    if is_active_text {
                        stream.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
                            "txt-0",
                        )));
                        is_active_text = false;
                    }

                    for file in files {
                        stream.push(LanguageModelStreamPart::File(file));
                    }
                }

                if let Some(tool_calls) = delta.get("tool_calls").and_then(JsonValue::as_array) {
                    if is_active_reasoning {
                        stream.push(LanguageModelStreamPart::ReasoningEnd(
                            LanguageModelReasoningEnd::new("reasoning-0"),
                        ));
                        is_active_reasoning = false;
                    }

                    for tool_call in tool_calls {
                        match serde_json::from_value::<StreamingToolCallDelta>(tool_call.clone())
                            .map_err(|error| error.to_string())
                            .and_then(|delta| {
                                process_openai_compatible_streaming_tool_call_delta(
                                    delta,
                                    &mut pending_tool_calls,
                                    &mut forwarded_tool_call_indices,
                                    &mut tool_call_tracker,
                                )
                                .map_err(|error| error.to_string())
                            }) {
                            Ok(parts) => stream.extend(parts),
                            Err(error) => {
                                finish_reason = LanguageModelFinishReason {
                                    unified: FinishReason::Error,
                                    raw: Some("openai-compatible-tool-call-error".to_string()),
                                };
                                stream.push(openai_compatible_stream_error(
                                    error,
                                    Some(&raw_value.to_string()),
                                ));
                            }
                        }
                    }
                }
            }
            ParseJsonResult::Failure { error, raw_value } => {
                finish_reason = LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("openai-compatible-parse-error".to_string()),
                };
                stream.push(openai_compatible_stream_error(
                    error.to_string(),
                    raw_value.as_ref().map(JsonValue::to_string).as_deref(),
                ));
            }
        }
    }

    if is_active_reasoning {
        stream.push(LanguageModelStreamPart::ReasoningEnd(
            LanguageModelReasoningEnd::new("reasoning-0"),
        ));
    }

    if is_active_text {
        stream.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
            "txt-0",
        )));
    }

    for (index, pending) in pending_tool_calls {
        let mut delta = StreamingToolCallDelta::new()
            .with_index(index)
            .with_function(
                StreamingToolCallDeltaFunction::new().with_arguments(pending.buffered_arguments),
            );
        if let Some(id) = pending.id {
            delta = delta.with_id(id);
        }
        for (key, value) in pending.extra {
            delta = delta.with_extra_value(key, value);
        }
        match tool_call_tracker.process_delta(delta) {
            Ok(parts) => stream.extend(parts),
            Err(error) => {
                finish_reason = LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("openai-compatible-tool-call-error".to_string()),
                };
                stream.push(openai_compatible_stream_error(error.to_string(), None));
            }
        }
    }

    stream.extend(tool_call_tracker.flush());
    let extracted_metadata = metadata_extractor
        .as_ref()
        .and_then(OpenAICompatibleStreamMetadataExtractor::build_metadata);

    stream.push(LanguageModelStreamPart::Finish(
        LanguageModelStreamFinish::new(openai_compatible_usage(usage.as_ref()), finish_reason)
            .with_provider_metadata(openai_compatible_stream_provider_metadata(
                provider_name,
                usage.as_ref(),
                extracted_metadata,
            )),
    ));

    let mut result = LanguageModelStreamResult::new(stream)
        .with_request(LanguageModelRequest::new().with_body(request_body));

    if let Some(headers) = response_headers {
        result = result.with_response(with_stream_response_headers(
            LanguageModelStreamResultResponse::new(),
            headers,
        ));
    }

    result
}

#[derive(Clone, Debug, Default)]
struct PendingOpenAICompatibleToolCall {
    id: Option<String>,
    buffered_arguments: String,
    extra: JsonObject,
}

fn process_openai_compatible_streaming_tool_call_delta(
    delta: StreamingToolCallDelta,
    pending_tool_calls: &mut BTreeMap<usize, PendingOpenAICompatibleToolCall>,
    forwarded_tool_call_indices: &mut BTreeSet<usize>,
    tool_call_tracker: &mut StreamingToolCallTracker,
) -> Result<Vec<LanguageModelStreamPart>, ai_sdk_provider::provider::InvalidResponseDataError> {
    let Some(index) = delta.index else {
        return tool_call_tracker.process_delta(delta);
    };

    if forwarded_tool_call_indices.contains(&index) {
        return tool_call_tracker.process_delta(delta);
    }

    let pending = pending_tool_calls.entry(index).or_default();

    if pending.id.is_none() {
        pending.id = delta.id.clone();
    }

    if pending.extra.is_empty() {
        pending.extra = delta.extra.clone();
    }

    if let Some(arguments) = delta
        .function
        .as_ref()
        .and_then(|function| function.arguments.as_ref())
    {
        pending.buffered_arguments.push_str(arguments);
    }

    let Some(name) = delta
        .function
        .as_ref()
        .and_then(|function| function.name.clone())
    else {
        return Ok(Vec::new());
    };

    let pending = pending_tool_calls
        .remove(&index)
        .expect("pending tool call entry exists");
    let mut forward_delta = StreamingToolCallDelta::new()
        .with_index(index)
        .with_function(
            StreamingToolCallDeltaFunction::new()
                .with_name(name)
                .with_arguments(pending.buffered_arguments),
        );

    if let Some(id) = pending.id {
        forward_delta = forward_delta.with_id(id);
    }

    for (key, value) in pending.extra {
        forward_delta = forward_delta.with_extra_value(key, value);
    }

    forwarded_tool_call_indices.insert(index);
    tool_call_tracker.process_delta(forward_delta)
}

fn openai_compatible_streaming_tool_call_metadata(
    provider_name: &str,
    delta: &StreamingToolCallDelta,
) -> Option<ProviderMetadata> {
    let thought_signature = delta
        .extra
        .get("extra_content")
        .and_then(|extra| extra.get("google"))
        .and_then(|google| google.get("thought_signature"))
        .and_then(JsonValue::as_str)?;
    let mut metadata = ProviderMetadata::new();
    metadata.insert(
        provider_name.to_string(),
        json!({
            "thoughtSignature": thought_signature
        })
        .as_object()
        .expect("metadata is an object")
        .clone(),
    );
    Some(metadata)
}

fn openai_compatible_stream_response_metadata(
    value: &JsonValue,
) -> LanguageModelStreamResponseMetadata {
    let mut metadata = LanguageModelStreamResponseMetadata::new();

    if let Some(id) = json_string(value.get("id")) {
        metadata = metadata.with_id(id);
    }

    if let Some(timestamp) = openai_compatible_response_timestamp(value.get("created")) {
        metadata = metadata.with_timestamp(timestamp);
    }

    if let Some(model_id) = json_string(value.get("model")) {
        metadata = metadata.with_model_id(model_id);
    }

    metadata
}

fn openai_compatible_stream_provider_metadata(
    provider_name: &str,
    usage: Option<&JsonValue>,
    extracted_metadata: Option<ProviderMetadata>,
) -> ProviderMetadata {
    let mut metadata = extracted_metadata.unwrap_or_default();
    add_openai_compatible_provider_prediction_metadata(provider_name, &mut metadata, usage, true);
    metadata
}

fn add_openai_compatible_provider_prediction_metadata(
    provider_name: &str,
    metadata: &mut ProviderMetadata,
    usage: Option<&JsonValue>,
    include_empty_provider: bool,
) {
    {
        let provider_metadata = metadata.entry(provider_name.to_string()).or_default();
        add_openai_compatible_prediction_metadata(provider_metadata, usage);
    }

    if !include_empty_provider
        && metadata
            .get(provider_name)
            .is_some_and(|provider_metadata| provider_metadata.is_empty())
    {
        metadata.remove(provider_name);
    }
}

fn add_openai_compatible_prediction_metadata(
    provider_metadata: &mut JsonObject,
    usage: Option<&JsonValue>,
) {
    if let Some(completion_token_details) =
        usage.and_then(|usage| usage.get("completion_tokens_details"))
    {
        if let Some(accepted_prediction_tokens) = json_u64(
            completion_token_details
                .get("accepted_prediction_tokens")
                .or_else(|| completion_token_details.get("acceptedPredictionTokens")),
        ) {
            provider_metadata.insert(
                "acceptedPredictionTokens".to_string(),
                json!(accepted_prediction_tokens),
            );
        }

        if let Some(rejected_prediction_tokens) = json_u64(
            completion_token_details
                .get("rejected_prediction_tokens")
                .or_else(|| completion_token_details.get("rejectedPredictionTokens")),
        ) {
            provider_metadata.insert(
                "rejectedPredictionTokens".to_string(),
                json!(rejected_prediction_tokens),
            );
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct OpenAICompatibleImageResponse {
    data: Vec<OpenAICompatibleImageData>,
}

#[derive(Clone, Debug, Deserialize)]
struct OpenAICompatibleImageData {
    b64_json: String,
}

fn openai_compatible_image_response(
    value: &JsonValue,
) -> Result<OpenAICompatibleImageResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn openai_compatible_image_result_from_response(
    model_id: &str,
    response: OpenAICompatibleImageResponse,
    response_headers: Option<Headers>,
    warnings: Vec<Warning>,
) -> ImageModelResult {
    let mut result = ImageModelResult::new(
        response
            .data
            .into_iter()
            .map(|image| FileDataContent::Base64(image.b64_json))
            .collect(),
        openai_compatible_image_response_metadata(model_id, response_headers),
    );

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn openai_compatible_image_result_from_error(
    model_id: &str,
    provider_name: &str,
    error: HandledFetchError,
    warnings: Vec<Warning>,
) -> ImageModelResult {
    let (message, headers) = match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
        ),
    };
    let mut result = ImageModelResult::new(
        Vec::new(),
        openai_compatible_image_response_metadata(model_id, headers),
    )
    .with_provider_metadata(openai_compatible_image_error_metadata(
        provider_name,
        message,
    ));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn openai_compatible_image_response_metadata(
    model_id: &str,
    headers: Option<Headers>,
) -> ImageModelResponse {
    let mut response = ImageModelResponse::new(OffsetDateTime::now_utc(), model_id);

    if let Some(headers) = headers {
        response = with_image_response_headers(response, headers);
    }

    response
}

fn openai_compatible_image_error_metadata(
    provider_name: &str,
    message: String,
) -> ImageModelProviderMetadata {
    let mut metadata = ImageModelProviderMetadata::new();
    let mut extra = JsonObject::new();
    extra.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert(
        provider_name.to_string(),
        ImageModelProviderMetadataEntry {
            images: JsonArray::new(),
            extra,
        },
    );
    metadata
}

#[derive(Clone, Debug, Deserialize)]
struct OpenAICompatibleEmbeddingResponse {
    data: Vec<OpenAICompatibleEmbeddingData>,
    #[serde(default)]
    usage: Option<OpenAICompatibleEmbeddingUsage>,
    #[serde(default, alias = "providerMetadata")]
    provider_metadata: Option<ProviderMetadata>,
}

#[derive(Clone, Debug, Deserialize)]
struct OpenAICompatibleEmbeddingData {
    embedding: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize)]
struct OpenAICompatibleEmbeddingUsage {
    prompt_tokens: u64,
}

fn openai_compatible_embedding_response(
    value: &JsonValue,
) -> Result<OpenAICompatibleEmbeddingResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn openai_compatible_embedding_result_from_response(
    response: OpenAICompatibleEmbeddingResponse,
    raw_response: Option<JsonValue>,
    response_headers: Option<Headers>,
    request_body: JsonValue,
    warnings: Vec<Warning>,
) -> EmbeddingModelResult {
    let mut result = EmbeddingModelResult::new(
        response
            .data
            .into_iter()
            .map(|item| item.embedding)
            .collect(),
    );

    if let Some(usage) = response.usage {
        result = result.with_usage(EmbeddingModelUsage::new(usage.prompt_tokens));
    }

    if let Some(provider_metadata) = response.provider_metadata {
        result = result.with_provider_metadata(provider_metadata);
    }

    for warning in warnings {
        result = result.with_warning(warning);
    }

    let mut response_metadata =
        EmbeddingModelResponse::new().with_body(raw_response.unwrap_or(request_body));

    if let Some(headers) = response_headers {
        response_metadata = with_embedding_response_headers(response_metadata, headers);
    }

    result.with_response(response_metadata)
}

fn openai_compatible_embedding_result_from_error(
    provider_name: &str,
    error: HandledFetchError,
    request_body: JsonValue,
) -> EmbeddingModelResult {
    let (message, headers, body) = match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None, None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
            error.response_body().map(String::from),
        ),
    };
    openai_compatible_embedding_error_result(
        provider_name,
        message,
        request_body,
        headers,
        body.as_deref(),
    )
}

fn openai_compatible_embedding_error_result(
    provider_name: &str,
    message: String,
    request_body: JsonValue,
    response_headers: Option<Headers>,
    raw_body: Option<&str>,
) -> EmbeddingModelResult {
    let response_body = raw_body
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| raw_body.map(|body| JsonValue::String(body.to_string())))
        .unwrap_or(request_body);
    let mut response = EmbeddingModelResponse::new().with_body(response_body);

    if let Some(headers) = response_headers {
        response = with_embedding_response_headers(response, headers);
    }

    EmbeddingModelResult::new(Vec::new())
        .with_provider_metadata(openai_compatible_error_metadata(provider_name, message))
        .with_response(response)
}

fn openai_compatible_error_generate_result(
    provider_name: &str,
    message: String,
    request_body: JsonValue,
) -> LanguageModelGenerateResult {
    LanguageModelGenerateResult::new(
        Vec::new(),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("openai-compatible-error".to_string()),
        },
        LanguageModelUsage::default(),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_provider_metadata(openai_compatible_error_metadata(provider_name, message))
}

fn openai_compatible_error_stream_result(
    message: String,
    request_body: JsonValue,
    response_headers: Option<Headers>,
    raw_body: Option<&str>,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let mut result =
        LanguageModelStreamResult::new(vec![openai_compatible_stream_error(message, raw_body)])
            .with_request(LanguageModelRequest::new().with_body(request_body));

    if let Some(headers) = response_headers {
        result = result.with_response(with_stream_response_headers(
            LanguageModelStreamResultResponse::new(),
            headers,
        ));
    }

    result
}

fn openai_compatible_stream_error(
    message: impl Into<String>,
    raw_body: Option<&str>,
) -> LanguageModelStreamPart {
    let mut error = JsonObject::new();
    error.insert("message".to_string(), JsonValue::String(message.into()));

    if let Some(raw_body) = raw_body {
        error.insert("body".to_string(), JsonValue::String(raw_body.to_string()));
    }

    LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(JsonValue::Object(error)))
}

fn openai_compatible_error_metadata(provider_name: &str, message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert(provider_name.to_string(), provider);
    metadata
}

fn openai_compatible_error_message(error: &JsonValue) -> String {
    error
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(JsonValue::as_str)
        .or_else(|| error.get("message").and_then(JsonValue::as_str))
        .map_or_else(|| error.to_string(), String::from)
}

fn clone_json_value(value: &JsonValue) -> Result<JsonValue, &'static str> {
    Ok(value.clone())
}

fn json_string(value: Option<&JsonValue>) -> Option<String> {
    match value {
        Some(JsonValue::String(value)) => Some(value.clone()),
        Some(JsonValue::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn json_u64(value: Option<&JsonValue>) -> Option<u64> {
    match value {
        Some(JsonValue::Number(value)) => value.as_u64(),
        Some(JsonValue::String(value)) => value.parse::<u64>().ok(),
        _ => None,
    }
}

fn openai_compatible_response_timestamp(value: Option<&JsonValue>) -> Option<OffsetDateTime> {
    match value {
        Some(JsonValue::Number(value)) => value
            .as_i64()
            .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok()),
        Some(JsonValue::String(value)) => value
            .parse::<i64>()
            .ok()
            .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok()),
        _ => None,
    }
}

fn openai_compatible_model_list_response(
    value: &JsonValue,
) -> Result<OpenAICompatibleModelListResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn openai_compatible_model_entry_response(
    value: &JsonValue,
) -> Result<OpenAICompatibleModelEntry, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn openai_compatible_url_fetch_error(message: String) -> HandledFetchError {
    HandledFetchError::Original {
        error: FetchErrorInfo::new(message),
    }
}

fn with_response_headers(
    mut response: LanguageModelResponse,
    headers: Headers,
) -> LanguageModelResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
}

fn with_stream_response_headers(
    mut response: LanguageModelStreamResultResponse,
    headers: Headers,
) -> LanguageModelStreamResultResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
}

fn with_embedding_response_headers(
    mut response: EmbeddingModelResponse,
    headers: Headers,
) -> EmbeddingModelResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
}

fn with_image_response_headers(
    mut response: ImageModelResponse,
    headers: Headers,
) -> ImageModelResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
}

fn default_openai_compatible_transport() -> OpenAICompatibleTransport {
    Arc::new(|request| Box::pin(ready(execute_openai_compatible_request(request))))
}

fn execute_openai_compatible_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_openai_compatible_get_request(request),
        ProviderApiRequestMethod::Post => execute_openai_compatible_post_request(request),
    }
}

fn execute_openai_compatible_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    provider_api_response(response)
}

fn execute_openai_compatible_post_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::post(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let builder = builder.config().http_status_as_error(false).build();
    let response = match request.body {
        Some(ProviderApiRequestBody::Text { content }) => builder.send(content),
        Some(ProviderApiRequestBody::Bytes { content }) => builder.send(content),
        Some(ProviderApiRequestBody::FormData { .. }) => {
            return Err(FetchErrorInfo::new(
                "multipart form data is not supported by the OpenAI-compatible transport",
            ));
        }
        None => builder.send_empty(),
    };

    provider_api_response(response)
}

fn provider_api_response(
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut response = response.map_err(|error| {
        FetchErrorInfo::new("fetch failed")
            .with_name("Error")
            .with_cause_message(error.to_string())
    })?;
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or("").to_string();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect::<Headers>();
    let body = response.body_mut().read_to_string().map_err(|error| {
        FetchErrorInfo::new("failed to read response body")
            .with_name("Error")
            .with_cause_message(error.to_string())
    })?;

    Ok(ProviderApiResponse::text(status.as_u16(), status_text, body).with_headers(headers))
}

#[cfg(test)]
mod tests {
    use super::{
        OpenAICompatibleEmbeddingModel, OpenAICompatibleMetadataExtractor,
        OpenAICompatibleProvider, OpenAICompatibleProviderSettings,
        OpenAICompatibleStreamMetadataExtractor, OpenAICompatibleTransport,
        OpenAICompatibleTransportFuture, create_openai_compatible, openai_compatible_prepare_tools,
        openai_compatible_provider_options_name, resolve_openai_compatible_provider_options_key,
        to_openai_compatible_camel_case, warn_if_deprecated_openai_compatible_provider_options_key,
    };
    use ai_sdk_provider::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions};
    use ai_sdk_provider::file_data::{FileData, FileDataContent};
    use ai_sdk_provider::headers::Headers;
    use ai_sdk_provider::image_model::{ImageModel, ImageModelCallOptions, ImageModelFile};
    use ai_sdk_provider::json::{JsonObject, JsonValue};
    use ai_sdk_provider::language_model::{
        FinishReason, LanguageModel, LanguageModelAbortController,
        LanguageModelAssistantContentPart, LanguageModelAssistantMessage, LanguageModelCallOptions,
        LanguageModelContent, LanguageModelFilePart, LanguageModelFunctionTool,
        LanguageModelGenerateResult, LanguageModelMessage, LanguageModelProviderTool,
        LanguageModelReasoningEffort, LanguageModelReasoningPart, LanguageModelResponseFormat,
        LanguageModelStreamPart, LanguageModelSystemMessage, LanguageModelTextPart,
        LanguageModelTool, LanguageModelToolCallPart, LanguageModelToolChoice,
        LanguageModelToolContentPart, LanguageModelToolMessage, LanguageModelToolResultOutput,
        LanguageModelToolResultPart, LanguageModelUserContentPart, LanguageModelUserMessage,
    };
    use ai_sdk_provider::provider::{ProviderMetadata, ProviderOptions};
    use ai_sdk_provider::warning::Warning;
    use ai_sdk_provider_utils::{
        ProviderApiRequest, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use serde_json::json;
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use url::Url;

    fn assert_request_tracks_abort_signal(
        request: &ProviderApiRequest,
        abort_controller: &LanguageModelAbortController,
    ) {
        let request_signal = request.abort_signal.clone().expect("abort signal set");
        assert!(!request_signal.is_aborted());

        abort_controller.abort_with_reason("client-disconnected");
        assert!(request_signal.is_aborted());
        assert_eq!(request_signal.reason(), Some(json!("client-disconnected")));
    }

    fn test_provider_options(value: JsonValue) -> ProviderOptions {
        serde_json::from_value(value).expect("provider options deserialize")
    }

    fn openai_compatible_prepare_tools_for_test(
        tools: Option<Vec<LanguageModelTool>>,
        tool_choice: Option<LanguageModelToolChoice>,
    ) -> (Option<Vec<JsonValue>>, Option<JsonValue>, Vec<Warning>) {
        let mut warnings = Vec::new();
        let (prepared_tools, prepared_tool_choice) =
            openai_compatible_prepare_tools(&tools, &tool_choice, &mut warnings);

        (prepared_tools, prepared_tool_choice, warnings)
    }

    fn openai_compatible_test_object_schema() -> JsonObject {
        serde_json::from_value(json!({
            "type": "object",
            "properties": {}
        }))
        .expect("test schema deserializes")
    }

    fn openai_compatible_test_function_tool(
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> LanguageModelTool {
        LanguageModelTool::Function(
            LanguageModelFunctionTool::new(name, openai_compatible_test_object_schema())
                .with_description(description),
        )
    }

    fn openai_compatible_default_provider_settings() -> OpenAICompatibleProviderSettings {
        OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
            .with_api_key("test-api-key")
            .with_header("custom-header", "value")
            .with_query_param("Custom-Param", "value")
    }

    fn assert_openai_compatible_default_request_headers(
        headers: &std::collections::BTreeMap<String, Option<String>>,
    ) {
        assert_eq!(
            headers.get("authorization").and_then(Option::as_deref),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            headers.get("custom-header").and_then(Option::as_deref),
            Some("value")
        );
        assert_eq!(
            headers.get("user-agent").and_then(Option::as_deref),
            Some("ai-sdk/openai-compatible/0.1.0")
        );
    }

    fn openai_compatible_test_provider_metadata(value: impl Into<String>) -> ProviderMetadata {
        let mut provider_metadata = JsonObject::new();
        provider_metadata.insert("value".to_string(), json!(value.into()));
        ProviderMetadata::from([("test-provider".to_string(), provider_metadata)])
    }

    fn openai_compatible_chat_generate_result_with_usage(
        usage: JsonValue,
    ) -> LanguageModelGenerateResult {
        let transport: OpenAICompatibleTransport =
            Arc::new(move |_request| -> OpenAICompatibleTransportFuture {
                let usage = usage.clone();
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "chatcmpl-usage",
                        "object": "chat.completion",
                        "created": 1711115037,
                        "model": "grok-3",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello!"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": usage
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("grok-3");

        poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
            ])),
        ])))
    }

    fn openai_compatible_test_provider_metadata_entry<'a>(
        result: &'a LanguageModelGenerateResult,
    ) -> &'a JsonObject {
        result
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("test-provider"))
            .expect("test-provider metadata is present")
    }

    fn openai_compatible_embedding_response_body(usage_prompt_tokens: u64) -> JsonValue {
        json!({
            "object": "list",
            "data": [
                {
                    "object": "embedding",
                    "index": 0,
                    "embedding": [0.1, 0.2]
                },
                {
                    "object": "embedding",
                    "index": 1,
                    "embedding": [0.3, 0.4]
                }
            ],
            "model": "text-embedding-3-large",
            "usage": {
                "prompt_tokens": usage_prompt_tokens,
                "total_tokens": usage_prompt_tokens
            }
        })
    }

    fn openai_compatible_embedding_test_model(
        response_body: JsonValue,
        response_headers: Headers,
    ) -> (
        OpenAICompatibleEmbeddingModel,
        Arc<Mutex<Option<ProviderApiRequest>>>,
    ) {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());
                let response_body = response_body.clone();
                let response_headers = response_headers.clone();

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    response_body.to_string(),
                )
                .with_headers(response_headers))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://my.api.com/v1/")
                .with_api_key("test-api-key")
                .with_header("Custom-Provider-Header", "provider-header-value"),
        )
        .with_transport(transport)
        .embedding_model("text-embedding-3-large");

        (model, captured_request)
    }

    fn openai_compatible_embedding_test_model_without_headers() -> (
        OpenAICompatibleEmbeddingModel,
        Arc<Mutex<Option<ProviderApiRequest>>>,
    ) {
        openai_compatible_embedding_test_model(
            openai_compatible_embedding_response_body(8),
            Headers::new(),
        )
    }

    fn captured_openai_compatible_embedding_request_body(
        captured_request: &Arc<Mutex<Option<ProviderApiRequest>>>,
    ) -> JsonValue {
        captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured")
            .body
            .and_then(|body| body.as_text().map(str::to_string))
            .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok())
            .expect("request body is JSON")
    }

    fn openai_compatible_embedding_test_values() -> Vec<String> {
        vec!["sunny day".to_string(), "rainy night".to_string()]
    }

    fn openai_compatible_stream_request_bodies_for_include_usage(
        include_usage: Option<bool>,
    ) -> Vec<JsonValue> {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_requests_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned")
                    .push(request.clone());

                let response_body = if request.url.contains("/chat/completions") {
                    sse_body([json!({
                        "id": "chatcmpl-include-usage",
                        "created": 1711115037,
                        "model": "chat-model",
                        "choices": [
                            {
                                "index": 0,
                                "delta": {},
                                "finish_reason": "stop"
                            }
                        ]
                    })])
                } else {
                    sse_body([json!({
                        "id": "cmpl-include-usage",
                        "object": "text_completion",
                        "created": 1711115037,
                        "model": "completion-model",
                        "choices": [
                            {
                                "text": "",
                                "index": 0,
                                "logprobs": null,
                                "finish_reason": "stop"
                            }
                        ]
                    })])
                };

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    response_body,
                ))))
            });
        let mut settings =
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com");

        if let Some(include_usage) = include_usage {
            settings = settings.with_include_usage(include_usage);
        }

        let provider = OpenAICompatibleProvider::from_settings(settings).with_transport(transport);

        let prompt = || {
            LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new("Hello"),
                )]),
            )])
        };

        let _chat_result = poll_ready(provider.chat_model("chat-model").do_stream(prompt()));
        let _language_result = poll_ready(
            provider
                .language_model("language-model")
                .do_stream(prompt()),
        );
        let _completion_result = poll_ready(
            provider
                .completion_model("completion-model")
                .do_stream(prompt()),
        );

        captured_requests
            .lock()
            .expect("captured request mutex is not poisoned")
            .iter()
            .map(openai_compatible_request_body_json)
            .collect()
    }

    fn openai_compatible_request_body_json(request: &ProviderApiRequest) -> JsonValue {
        request
            .body
            .clone()
            .and_then(|body| body.as_text().map(str::to_string))
            .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok())
            .expect("request body is JSON")
    }

    fn assert_openai_compatible_include_usage(
        request_bodies: &[JsonValue],
        expected_stream_options: Option<JsonValue>,
    ) {
        assert_eq!(request_bodies.len(), 3);

        for body in request_bodies {
            assert_eq!(body.get("stream_options").cloned(), expected_stream_options);
        }
    }

    #[test]
    fn to_camel_case_upstream_should_convert_hyphenated_names_to_camel_case() {
        assert_eq!(
            to_openai_compatible_camel_case("provider-name"),
            "providerName"
        );
    }

    #[test]
    fn to_camel_case_upstream_should_convert_underscored_names_to_camel_case() {
        assert_eq!(
            to_openai_compatible_camel_case("provider_name"),
            "providerName"
        );
    }

    #[test]
    fn to_camel_case_upstream_should_handle_multiple_separators() {
        assert_eq!(
            to_openai_compatible_camel_case("my-provider-name"),
            "myProviderName"
        );
    }

    #[test]
    fn to_camel_case_upstream_should_return_same_string_when_already_camel_case() {
        assert_eq!(
            to_openai_compatible_camel_case("providerName"),
            "providerName"
        );
    }

    #[test]
    fn to_camel_case_upstream_should_return_same_string_when_no_separators() {
        assert_eq!(to_openai_compatible_camel_case("openai"), "openai");
    }

    #[test]
    fn to_camel_case_upstream_should_handle_empty_string() {
        assert_eq!(to_openai_compatible_camel_case(""), "");
    }

    #[test]
    fn openai_compatible_chat_config_extracts_base_name_from_provider_string() {
        assert_eq!(
            openai_compatible_provider_options_name("anthropic.beta"),
            "anthropic"
        );
    }

    #[test]
    fn openai_compatible_chat_config_handles_provider_without_dot_notation() {
        assert_eq!(openai_compatible_provider_options_name("openai"), "openai");
    }

    #[test]
    fn openai_compatible_chat_config_returns_empty_for_empty_provider() {
        assert_eq!(openai_compatible_provider_options_name(""), "");
    }

    #[test]
    fn resolve_provider_options_key_upstream_should_return_camel_case_key_when_camel_case_options_present()
     {
        let provider_options = test_provider_options(json!({
            "providerName": {
                "someOption": "value"
            }
        }));

        assert_eq!(
            resolve_openai_compatible_provider_options_key(
                "provider-name",
                Some(&provider_options),
            ),
            "providerName"
        );
    }

    #[test]
    fn resolve_provider_options_key_upstream_should_return_raw_key_when_only_raw_options_present() {
        let provider_options = test_provider_options(json!({
            "provider-name": {
                "someOption": "value"
            }
        }));

        assert_eq!(
            resolve_openai_compatible_provider_options_key(
                "provider-name",
                Some(&provider_options),
            ),
            "provider-name"
        );
    }

    #[test]
    fn resolve_provider_options_key_upstream_should_return_camel_case_key_when_both_are_present() {
        let provider_options = test_provider_options(json!({
            "provider-name": {
                "a": 1
            },
            "providerName": {
                "b": 2
            }
        }));

        assert_eq!(
            resolve_openai_compatible_provider_options_key(
                "provider-name",
                Some(&provider_options),
            ),
            "providerName"
        );
    }

    #[test]
    fn resolve_provider_options_key_upstream_should_return_raw_key_when_no_options_are_present() {
        let provider_options = test_provider_options(json!({}));

        assert_eq!(
            resolve_openai_compatible_provider_options_key(
                "provider-name",
                Some(&provider_options),
            ),
            "provider-name"
        );
    }

    #[test]
    fn resolve_provider_options_key_upstream_should_return_raw_key_when_provider_options_is_undefined()
     {
        assert_eq!(
            resolve_openai_compatible_provider_options_key("provider-name", None),
            "provider-name"
        );
    }

    #[test]
    fn resolve_provider_options_key_upstream_should_return_raw_key_when_name_has_no_separators() {
        let provider_options = test_provider_options(json!({
            "openai": {
                "a": 1
            }
        }));

        assert_eq!(
            resolve_openai_compatible_provider_options_key("openai", Some(&provider_options)),
            "openai"
        );
    }

    #[test]
    fn deprecated_provider_options_key_upstream_should_push_warning_when_raw_key_is_used_and_differs()
     {
        let provider_options = test_provider_options(json!({
            "black-forest-labs": {
                "style": "hd"
            }
        }));
        let mut warnings = Vec::new();

        warn_if_deprecated_openai_compatible_provider_options_key(
            "black-forest-labs",
            Some(&provider_options),
            &mut warnings,
        );

        assert!(matches!(
            warnings.as_slice(),
            [Warning::Deprecated { setting, message }]
                if setting == "providerOptions key 'black-forest-labs'"
                    && message == "Use 'blackForestLabs' instead."
        ));
    }

    #[test]
    fn deprecated_provider_options_key_upstream_should_not_warn_when_only_camel_case_key_is_used() {
        let provider_options = test_provider_options(json!({
            "blackForestLabs": {
                "style": "hd"
            }
        }));
        let mut warnings = Vec::new();

        warn_if_deprecated_openai_compatible_provider_options_key(
            "black-forest-labs",
            Some(&provider_options),
            &mut warnings,
        );

        assert!(warnings.is_empty());
    }

    #[test]
    fn deprecated_provider_options_key_upstream_should_not_warn_when_raw_name_is_already_camel_case()
     {
        let provider_options = test_provider_options(json!({
            "openai": {
                "user": "test"
            }
        }));
        let mut warnings = Vec::new();

        warn_if_deprecated_openai_compatible_provider_options_key(
            "openai",
            Some(&provider_options),
            &mut warnings,
        );

        assert!(warnings.is_empty());
    }

    #[test]
    fn deprecated_provider_options_key_upstream_should_not_warn_when_raw_key_is_not_present() {
        let provider_options = test_provider_options(json!({}));
        let mut warnings = Vec::new();

        warn_if_deprecated_openai_compatible_provider_options_key(
            "black-forest-labs",
            Some(&provider_options),
            &mut warnings,
        );

        assert!(warnings.is_empty());
    }

    #[test]
    fn deprecated_provider_options_key_upstream_should_not_warn_when_provider_options_is_undefined()
    {
        let mut warnings = Vec::new();

        warn_if_deprecated_openai_compatible_provider_options_key(
            "black-forest-labs",
            None,
            &mut warnings,
        );

        assert!(warnings.is_empty());
    }

    #[test]
    fn openai_compatible_prepare_tools_returns_undefined_tools_and_tool_choice_when_tools_are_null()
    {
        let (tools, tool_choice, warnings) = openai_compatible_prepare_tools_for_test(None, None);

        assert_eq!(tools, None);
        assert_eq!(tool_choice, None);
        assert_eq!(warnings, Vec::<Warning>::new());
    }

    #[test]
    fn openai_compatible_prepare_tools_returns_undefined_tools_and_tool_choice_when_tools_are_empty()
     {
        let (tools, tool_choice, warnings) =
            openai_compatible_prepare_tools_for_test(Some(Vec::new()), None);

        assert_eq!(tools, None);
        assert_eq!(tool_choice, None);
        assert_eq!(warnings, Vec::<Warning>::new());
    }

    #[test]
    fn openai_compatible_prepare_tools_prepares_function_tools() {
        let (tools, tool_choice, warnings) = openai_compatible_prepare_tools_for_test(
            Some(vec![openai_compatible_test_function_tool(
                "testFunction",
                "A test function",
            )]),
            None,
        );

        assert_eq!(
            tools,
            Some(vec![json!({
                "type": "function",
                "function": {
                    "name": "testFunction",
                    "description": "A test function",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    }
                }
            })])
        );
        assert_eq!(tool_choice, None);
        assert_eq!(warnings, Vec::<Warning>::new());
    }

    #[test]
    fn openai_compatible_prepare_tools_warns_for_unsupported_provider_defined_tools() {
        let (tools, tool_choice, warnings) = openai_compatible_prepare_tools_for_test(
            Some(vec![LanguageModelTool::Provider(
                LanguageModelProviderTool::new(
                    "some.unsupported_tool",
                    "unsupported_tool",
                    JsonObject::new(),
                ),
            )]),
            None,
        );

        assert_eq!(tools, Some(vec![]));
        assert_eq!(tool_choice, None);
        assert_eq!(
            warnings,
            vec![Warning::Unsupported {
                feature: "provider-defined tool some.unsupported_tool".to_string(),
                details: None,
            }]
        );
    }

    #[test]
    fn openai_compatible_prepare_tools_handles_auto_tool_choice() {
        let (_tools, tool_choice, warnings) = openai_compatible_prepare_tools_for_test(
            Some(vec![openai_compatible_test_function_tool(
                "testFunction",
                "Test",
            )]),
            Some(LanguageModelToolChoice::Auto),
        );

        assert_eq!(tool_choice, Some(json!("auto")));
        assert_eq!(warnings, Vec::<Warning>::new());
    }

    #[test]
    fn openai_compatible_prepare_tools_handles_required_tool_choice() {
        let (_tools, tool_choice, warnings) = openai_compatible_prepare_tools_for_test(
            Some(vec![openai_compatible_test_function_tool(
                "testFunction",
                "Test",
            )]),
            Some(LanguageModelToolChoice::Required),
        );

        assert_eq!(tool_choice, Some(json!("required")));
        assert_eq!(warnings, Vec::<Warning>::new());
    }

    #[test]
    fn openai_compatible_prepare_tools_handles_none_tool_choice() {
        let (_tools, tool_choice, warnings) = openai_compatible_prepare_tools_for_test(
            Some(vec![openai_compatible_test_function_tool(
                "testFunction",
                "Test",
            )]),
            Some(LanguageModelToolChoice::None),
        );

        assert_eq!(tool_choice, Some(json!("none")));
        assert_eq!(warnings, Vec::<Warning>::new());
    }

    #[test]
    fn openai_compatible_prepare_tools_handles_specific_tool_choice() {
        let (_tools, tool_choice, warnings) = openai_compatible_prepare_tools_for_test(
            Some(vec![openai_compatible_test_function_tool(
                "testFunction",
                "Test",
            )]),
            Some(LanguageModelToolChoice::Tool {
                tool_name: "testFunction".to_string(),
            }),
        );

        assert_eq!(
            tool_choice,
            Some(json!({
                "type": "function",
                "function": {
                    "name": "testFunction"
                }
            }))
        );
        assert_eq!(warnings, Vec::<Warning>::new());
    }

    #[test]
    fn openai_compatible_prepare_tools_passes_through_strict_true() {
        let (tools, tool_choice, warnings) = openai_compatible_prepare_tools_for_test(
            Some(vec![LanguageModelTool::Function(
                LanguageModelFunctionTool::new(
                    "testFunction",
                    openai_compatible_test_object_schema(),
                )
                .with_description("A test function")
                .with_strict(true),
            )]),
            None,
        );

        assert_eq!(
            tools,
            Some(vec![json!({
                "type": "function",
                "function": {
                    "name": "testFunction",
                    "description": "A test function",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    },
                    "strict": true
                }
            })])
        );
        assert_eq!(tool_choice, None);
        assert_eq!(warnings, Vec::<Warning>::new());
    }

    #[test]
    fn openai_compatible_prepare_tools_passes_through_strict_false() {
        let (tools, tool_choice, warnings) = openai_compatible_prepare_tools_for_test(
            Some(vec![LanguageModelTool::Function(
                LanguageModelFunctionTool::new(
                    "testFunction",
                    openai_compatible_test_object_schema(),
                )
                .with_description("A test function")
                .with_strict(false),
            )]),
            None,
        );

        assert_eq!(
            tools,
            Some(vec![json!({
                "type": "function",
                "function": {
                    "name": "testFunction",
                    "description": "A test function",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    },
                    "strict": false
                }
            })])
        );
        assert_eq!(tool_choice, None);
        assert_eq!(warnings, Vec::<Warning>::new());
    }

    #[test]
    fn openai_compatible_prepare_tools_omits_undefined_strict() {
        let (tools, tool_choice, warnings) = openai_compatible_prepare_tools_for_test(
            Some(vec![openai_compatible_test_function_tool(
                "testFunction",
                "A test function",
            )]),
            None,
        );

        assert_eq!(
            tools,
            Some(vec![json!({
                "type": "function",
                "function": {
                    "name": "testFunction",
                    "description": "A test function",
                    "parameters": {
                        "type": "object",
                        "properties": {}
                    }
                }
            })])
        );
        assert_eq!(tool_choice, None);
        assert_eq!(warnings, Vec::<Warning>::new());
    }

    #[test]
    fn openai_compatible_prepare_tools_passes_mixed_strict_settings() {
        let (tools, tool_choice, warnings) = openai_compatible_prepare_tools_for_test(
            Some(vec![
                LanguageModelTool::Function(
                    LanguageModelFunctionTool::new(
                        "strictTool",
                        openai_compatible_test_object_schema(),
                    )
                    .with_description("A strict tool")
                    .with_strict(true),
                ),
                LanguageModelTool::Function(
                    LanguageModelFunctionTool::new(
                        "nonStrictTool",
                        openai_compatible_test_object_schema(),
                    )
                    .with_description("A non-strict tool")
                    .with_strict(false),
                ),
                LanguageModelTool::Function(
                    LanguageModelFunctionTool::new(
                        "defaultTool",
                        openai_compatible_test_object_schema(),
                    )
                    .with_description("A tool without strict setting"),
                ),
            ]),
            None,
        );

        assert_eq!(
            tools,
            Some(vec![
                json!({
                    "type": "function",
                    "function": {
                        "name": "strictTool",
                        "description": "A strict tool",
                        "parameters": {
                            "type": "object",
                            "properties": {}
                        },
                        "strict": true
                    }
                }),
                json!({
                    "type": "function",
                    "function": {
                        "name": "nonStrictTool",
                        "description": "A non-strict tool",
                        "parameters": {
                            "type": "object",
                            "properties": {}
                        },
                        "strict": false
                    }
                }),
                json!({
                    "type": "function",
                    "function": {
                        "name": "defaultTool",
                        "description": "A tool without strict setting",
                        "parameters": {
                            "type": "object",
                            "properties": {}
                        }
                    }
                })
            ])
        );
        assert_eq!(tool_choice, None);
        assert_eq!(warnings, Vec::<Warning>::new());
    }

    #[test]
    fn openai_compatible_provider_lists_models() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "object": "list",
                        "data": [
                            {
                                "id": "provider/chat-model",
                                "object": "model",
                                "created": 1711115037,
                                "owned_by": "provider",
                                "contextWindow": 128000,
                                "max_tokens": 4096,
                                "type": "language",
                                "tags": ["tool-use"]
                            },
                            {
                                "id": "provider/embedding-model",
                                "object": "model",
                                "ownedBy": "provider"
                            }
                        ]
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "models_req".to_string(),
                )])))))
            });
        let provider = create_openai_compatible(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com/v1/")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value")
                .with_query_param("catalog", "current"),
        )
        .with_transport(transport);

        let result = poll_ready(provider.list_models()).expect("model list succeeds");
        assert_eq!(result.object.as_deref(), Some("list"));
        assert_eq!(
            result.model_ids().collect::<Vec<_>>(),
            vec!["provider/chat-model", "provider/embedding-model"]
        );
        assert_eq!(result.data[0].created, Some(1711115037));
        assert_eq!(result.data[0].owned_by.as_deref(), Some("provider"));
        assert_eq!(result.data[0].context_window, Some(128000));
        assert_eq!(result.data[0].max_tokens, Some(4096));
        assert_eq!(result.data[0].model_type.as_deref(), Some("language"));
        assert_eq!(result.data[0].tags, vec!["tool-use"]);
        assert_eq!(result.data[1].owned_by.as_deref(), Some("provider"));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Get);
        assert_eq!(
            request.url,
            "https://api.example.com/v1/models?catalog=current"
        );
        assert!(request.body.is_none());
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/openai-compatible/0.1.0"))
        );
    }

    #[test]
    fn openai_compatible_provider_retrieves_model_by_id() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "provider/chat-model",
                        "object": "model",
                        "created": 1711115037,
                        "owned_by": "provider",
                        "contextWindow": 128000,
                        "maxTokens": 4096,
                        "modelType": "language",
                        "tags": ["tool-use"]
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "model_req".to_string(),
                )])))))
            });
        let provider = create_openai_compatible(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com/v1/")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value")
                .with_query_param("catalog", "current"),
        )
        .with_transport(transport);

        let result = poll_ready(provider.retrieve_model("provider/chat-model"))
            .expect("model retrieval succeeds");
        assert_eq!(result.id, "provider/chat-model");
        assert_eq!(result.object.as_deref(), Some("model"));
        assert_eq!(result.created, Some(1711115037));
        assert_eq!(result.owned_by.as_deref(), Some("provider"));
        assert_eq!(result.context_window, Some(128000));
        assert_eq!(result.max_tokens, Some(4096));
        assert_eq!(result.model_type.as_deref(), Some("language"));
        assert_eq!(result.tags, vec!["tool-use"]);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Get);
        assert_eq!(
            request.url,
            "https://api.example.com/v1/models/provider%2Fchat-model?catalog=current"
        );
        assert!(request.body.is_none());
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
    }

    #[test]
    fn openai_compatible_embedding_extracts_embedding() {
        let (model, _captured_request) = openai_compatible_embedding_test_model_without_headers();

        let result = poll_ready(model.do_embed(EmbeddingModelCallOptions::new(
            openai_compatible_embedding_test_values(),
        )));

        assert_eq!(result.embeddings, vec![vec![0.1, 0.2], vec![0.3, 0.4]]);
    }

    #[test]
    fn openai_compatible_embedding_exposes_raw_response_headers() {
        let (model, _captured_request) = openai_compatible_embedding_test_model(
            openai_compatible_embedding_response_body(8),
            Headers::from([
                ("content-length".to_string(), "236".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
                ("test-header".to_string(), "test-value".to_string()),
            ]),
        );

        let result = poll_ready(model.do_embed(EmbeddingModelCallOptions::new(
            openai_compatible_embedding_test_values(),
        )));

        assert_eq!(
            result.response.and_then(|response| response.headers),
            Some(Headers::from([
                ("content-length".to_string(), "236".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
                ("test-header".to_string(), "test-value".to_string())
            ]))
        );
    }

    #[test]
    fn openai_compatible_embedding_extracts_usage() {
        let (model, _captured_request) = openai_compatible_embedding_test_model(
            openai_compatible_embedding_response_body(20),
            Headers::new(),
        );

        let result = poll_ready(model.do_embed(EmbeddingModelCallOptions::new(
            openai_compatible_embedding_test_values(),
        )));

        assert_eq!(result.usage.map(|usage| usage.tokens), Some(20),);
    }

    #[test]
    fn openai_compatible_embedding_passes_model_and_values() {
        let (model, captured_request) = openai_compatible_embedding_test_model_without_headers();
        let values = openai_compatible_embedding_test_values();

        let _result = poll_ready(model.do_embed(EmbeddingModelCallOptions::new(values.clone())));

        assert_eq!(
            captured_openai_compatible_embedding_request_body(&captured_request),
            json!({
                "model": "text-embedding-3-large",
                "input": values,
                "encoding_format": "float"
            })
        );
    }

    #[test]
    fn openai_compatible_embedding_passes_dimensions_setting() {
        let (model, captured_request) = openai_compatible_embedding_test_model_without_headers();
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "dimensions": 64
            }
        }))
        .expect("provider options deserialize");

        let _result = poll_ready(
            model.do_embed(
                EmbeddingModelCallOptions::new(openai_compatible_embedding_test_values())
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(
            captured_openai_compatible_embedding_request_body(&captured_request),
            json!({
                "model": "text-embedding-3-large",
                "input": openai_compatible_embedding_test_values(),
                "encoding_format": "float",
                "dimensions": 64
            })
        );
    }

    #[test]
    fn openai_compatible_embedding_passes_deprecated_openai_compatible_key_and_warns() {
        let (model, captured_request) = openai_compatible_embedding_test_model_without_headers();
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai-compatible": {
                "dimensions": 64
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_embed(
                EmbeddingModelCallOptions::new(openai_compatible_embedding_test_values())
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(
            captured_openai_compatible_embedding_request_body(&captured_request),
            json!({
                "model": "text-embedding-3-large",
                "input": openai_compatible_embedding_test_values(),
                "encoding_format": "float",
                "dimensions": 64
            })
        );
        assert_eq!(
            result.warnings,
            vec![Warning::Deprecated {
                setting: "providerOptions key 'openai-compatible'".to_string(),
                message: "Use 'openaiCompatible' instead.".to_string()
            }]
        );
    }

    #[test]
    fn openai_compatible_embedding_warns_when_raw_provider_name_key_is_used() {
        let (model, _captured_request) = openai_compatible_embedding_test_model_without_headers();
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "test-provider": {
                "dimensions": 64
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_embed(
                EmbeddingModelCallOptions::new(openai_compatible_embedding_test_values())
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(
            result.warnings,
            vec![Warning::Deprecated {
                setting: "providerOptions key 'test-provider'".to_string(),
                message: "Use 'testProvider' instead.".to_string()
            }]
        );
    }

    #[test]
    fn openai_compatible_embedding_does_not_warn_when_camel_case_provider_name_key_is_used() {
        let (model, _captured_request) = openai_compatible_embedding_test_model_without_headers();
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "testProvider": {
                "dimensions": 64
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_embed(
                EmbeddingModelCallOptions::new(openai_compatible_embedding_test_values())
                    .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
    }

    #[test]
    fn openai_compatible_embedding_passes_headers() {
        let (model, captured_request) = openai_compatible_embedding_test_model_without_headers();

        let _result = poll_ready(
            model.do_embed(
                EmbeddingModelCallOptions::new(openai_compatible_embedding_test_values())
                    .with_header("Custom-Request-Header", "request-header-value"),
            ),
        );

        let headers = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured")
            .headers;
        assert_eq!(
            headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            headers.get("custom-provider-header").map(String::as_str),
            Some("provider-header-value")
        );
        assert_eq!(
            headers.get("custom-request-header").map(String::as_str),
            Some("request-header-value")
        );
        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn openai_compatible_embedding_model_passes_options_and_errors() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "data": [
                            {
                                "embedding": [0.1, 0.2]
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 3
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .embedding_model("text-embedding-3-small");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai-compatible": {
                "dimensions": 64,
                "user": "user-123"
            },
            "openaiCompatible": {
                "dimensions": 32
            },
            "test-provider": {
                "user": "user-456"
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(
            model.do_embed(
                EmbeddingModelCallOptions::new(vec!["hello".to_string()])
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.embeddings, vec![vec![0.1, 0.2]]);
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Deprecated { setting, .. }
                    if setting == "providerOptions key 'openai-compatible'"
            )
        }));
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Deprecated { setting, .. }
                    if setting == "providerOptions key 'test-provider'"
            )
        }));
        assert_eq!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .clone()
                .expect("request is captured")
                .body
                .and_then(|body| body.as_text().map(str::to_string))
                .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok()),
            Some(json!({
                "model": "text-embedding-3-small",
                "input": ["hello"],
                "encoding_format": "float",
                "dimensions": 32,
                "user": "user-456"
            }))
        );

        let error_transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    401,
                    "Unauthorized",
                    json!({
                        "error": {
                            "message": "Invalid API key"
                        }
                    })
                    .to_string(),
                ))))
            });
        let error_model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com"),
        )
        .with_transport(error_transport)
        .embedding_model("text-embedding-3-small");
        let error_result = poll_ready(
            error_model.do_embed(EmbeddingModelCallOptions::new(vec!["hello".to_string()])),
        );

        assert!(error_result.embeddings.is_empty());
        assert_eq!(
            error_result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test-provider"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Invalid API key")
        );
    }

    #[test]
    fn openai_compatible_embedding_passes_abort_signal_to_provider_api_request() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let abort_controller = LanguageModelAbortController::new();
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "data": [
                            {
                                "embedding": [0.1, 0.2]
                            }
                        ]
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .embedding_model("text-embedding-3-small");

        let result = poll_ready(
            model.do_embed(
                EmbeddingModelCallOptions::new(vec!["hello".to_string()])
                    .with_abort_signal(abort_controller.signal()),
            ),
        );

        assert_eq!(result.embeddings, vec![vec![0.1, 0.2]]);
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_request_tracks_abort_signal(&request, &abort_controller);
    }

    #[test]
    fn openai_compatible_image_model_passes_options_warnings_and_errors() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "data": [
                            {
                                "b64_json": "aW1hZ2U="
                            }
                        ]
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "black-forest-labs",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .image_model("flux-pro");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "black-forest-labs": {
                "quality": "standard"
            },
            "blackForestLabs": {
                "quality": "hd"
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A forest")
                    .with_aspect_ratio("16:9")
                    .with_seed(123)
                    .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Deprecated { setting, .. }
                    if setting == "providerOptions key 'black-forest-labs'"
            )
        }));
        assert_eq!(
            result
                .warnings
                .iter()
                .filter(|warning| matches!(warning, Warning::Unsupported { .. }))
                .count(),
            2
        );
        assert_eq!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .clone()
                .expect("request is captured")
                .body
                .and_then(|body| body.as_text().map(str::to_string))
                .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok()),
            Some(json!({
                "model": "flux-pro",
                "prompt": "A forest",
                "n": 1,
                "quality": "hd",
                "response_format": "b64_json"
            }))
        );

        let error_transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({
                        "error": {
                            "message": "Invalid image prompt"
                        }
                    })
                    .to_string(),
                ))))
            });
        let error_model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com"),
        )
        .with_transport(error_transport)
        .image_model("dall-e-3");
        let error_result = poll_ready(
            error_model.do_generate(ImageModelCallOptions::new(1).with_prompt("bad prompt")),
        );

        assert!(error_result.images.is_empty());
        assert_eq!(
            error_result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test-provider"))
                .map(|metadata| &metadata.extra)
                .and_then(|extra| extra.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Invalid image prompt")
        );
    }

    #[test]
    fn openai_compatible_image_generation_passes_abort_signal_to_provider_api_request() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let abort_controller = LanguageModelAbortController::new();
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "data": [
                            {
                                "b64_json": "aW1hZ2U="
                            }
                        ]
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .image_model("dall-e-3");

        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A forest")
                    .with_abort_signal(abort_controller.signal()),
            ),
        );

        assert_eq!(result.images.len(), 1);
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_request_tracks_abort_signal(&request, &abort_controller);
    }

    #[test]
    fn openai_compatible_image_edit_passes_abort_signal_to_provider_api_request() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let abort_controller = LanguageModelAbortController::new();
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "data": [
                            {
                                "b64_json": "ZWRpdGVkLWltYWdl"
                            }
                        ]
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .image_model("dall-e-3");

        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("Add a flamingo to the pool")
                    .with_files(vec![ImageModelFile::file(
                        "image/png",
                        FileDataContent::Bytes(vec![137, 80, 78, 71]),
                    )])
                    .with_abort_signal(abort_controller.signal()),
            ),
        );

        assert_eq!(result.images.len(), 1);
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_request_tracks_abort_signal(&request, &abort_controller);
    }

    #[test]
    fn openai_compatible_completion_passes_options_warnings_and_errors() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "choices": [
                            {
                                "text": "ok",
                                "finish_reason": "length"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .completion_model("gpt-3.5-turbo-instruct");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "test-provider": {
                "echo": true,
                "logitBias": {
                    "7": 42
                },
                "suffix": "raw-suffix",
                "someCustomOption": "raw-value",
                "user": "raw-user"
            },
            "testProvider": {
                "someCustomOption": "camel-value",
                "user": "camel-user"
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Hello"),
                    )]),
                )])
                .with_top_k(5)
                .with_response_format(
                    LanguageModelResponseFormat::json().with_schema(
                        serde_json::from_value(json!({
                            "type": "object",
                            "properties": {}
                        }))
                        .expect("schema deserializes"),
                    ),
                )
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Length);
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Deprecated { setting, .. }
                    if setting == "providerOptions key 'test-provider'"
            )
        }));
        assert_eq!(
            result
                .warnings
                .iter()
                .filter(|warning| matches!(warning, Warning::Unsupported { .. }))
                .count(),
            2
        );
        assert_eq!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .clone()
                .expect("request is captured")
                .body
                .and_then(|body| body.as_text().map(str::to_string))
                .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok()),
            Some(json!({
                "model": "gpt-3.5-turbo-instruct",
                "echo": true,
                "logitBias": {
                    "7": 42
                },
                "logit_bias": {
                    "7": 42
                },
                "suffix": "raw-suffix",
                "someCustomOption": "camel-value",
                "user": "camel-user",
                "prompt": "user:\nHello\n\nassistant:\n",
                "stop": ["\nuser:"]
            }))
        );

        let error_transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    429,
                    "Too Many Requests",
                    json!({
                        "error": {
                            "message": "Rate limited"
                        }
                    })
                    .to_string(),
                ))))
            });
        let error_model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com"),
        )
        .with_transport(error_transport)
        .completion_model("gpt-3.5-turbo-instruct");
        let error_result =
            poll_ready(error_model.do_generate(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(error_result.finish_reason.unified, FinishReason::Error);
        assert_eq!(
            error_result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test-provider"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Rate limited")
        );
    }

    #[test]
    fn openai_compatible_chat_maps_response_formats_and_warnings() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "choices": [
                            {
                                "message": {
                                    "content": "{}",
                                    "reasoning_content": "reasoning"
                                },
                                "finish_reason": "length"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("JSON only"),
                )])
                .with_top_k(4)
                .with_response_format(
                    LanguageModelResponseFormat::json().with_schema(
                        serde_json::from_value(json!({
                            "type": "object",
                            "properties": {}
                        }))
                        .expect("schema deserializes"),
                    ),
                ),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::Length);
        assert_eq!(result.content.len(), 2);
        assert_eq!(
            result
                .warnings
                .iter()
                .filter(|warning| matches!(warning, Warning::Unsupported { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn openai_compatible_chat_extracts_detailed_token_usage_when_available() {
        let usage = json!({
            "prompt_tokens": 20,
            "completion_tokens": 30,
            "total_tokens": 50,
            "prompt_tokens_details": {
                "cached_tokens": 5
            },
            "completion_tokens_details": {
                "reasoning_tokens": 10,
                "accepted_prediction_tokens": 15,
                "rejected_prediction_tokens": 5
            }
        });
        let result = openai_compatible_chat_generate_result_with_usage(usage.clone());

        assert_eq!(result.usage.input_tokens.total, Some(20));
        assert_eq!(result.usage.input_tokens.cache_read, Some(5));
        assert_eq!(result.usage.input_tokens.no_cache, Some(15));
        assert_eq!(result.usage.input_tokens.cache_write, None);
        assert_eq!(result.usage.output_tokens.total, Some(30));
        assert_eq!(result.usage.output_tokens.reasoning, Some(10));
        assert_eq!(result.usage.output_tokens.text, Some(20));
        assert_eq!(result.usage.raw, usage.as_object().cloned());

        let provider_metadata = openai_compatible_test_provider_metadata_entry(&result);
        assert_eq!(
            provider_metadata.get("acceptedPredictionTokens"),
            Some(&json!(15))
        );
        assert_eq!(
            provider_metadata.get("rejectedPredictionTokens"),
            Some(&json!(5))
        );
    }

    #[test]
    fn openai_compatible_chat_handles_missing_token_details() {
        let result = openai_compatible_chat_generate_result_with_usage(json!({
            "prompt_tokens": 20,
            "completion_tokens": 30
        }));

        assert_eq!(result.usage.input_tokens.total, Some(20));
        assert_eq!(result.usage.input_tokens.cache_read, None);
        assert_eq!(result.usage.input_tokens.no_cache, None);
        assert_eq!(result.usage.output_tokens.total, Some(30));
        assert_eq!(result.usage.output_tokens.reasoning, None);
        assert_eq!(result.usage.output_tokens.text, Some(30));
        assert!(openai_compatible_test_provider_metadata_entry(&result).is_empty());
    }

    #[test]
    fn openai_compatible_chat_handles_partial_token_details() {
        let usage = json!({
            "prompt_tokens": 20,
            "completion_tokens": 30,
            "total_tokens": 50,
            "prompt_tokens_details": {
                "cached_tokens": 5
            },
            "completion_tokens_details": {
                "reasoning_tokens": 10
            }
        });
        let result = openai_compatible_chat_generate_result_with_usage(usage.clone());

        assert_eq!(result.usage.input_tokens.total, Some(20));
        assert_eq!(result.usage.input_tokens.cache_read, Some(5));
        assert_eq!(result.usage.input_tokens.no_cache, Some(15));
        assert_eq!(result.usage.output_tokens.total, Some(30));
        assert_eq!(result.usage.output_tokens.reasoning, Some(10));
        assert_eq!(result.usage.output_tokens.text, Some(20));
        assert_eq!(result.usage.raw, usage.as_object().cloned());
        assert!(openai_compatible_test_provider_metadata_entry(&result).is_empty());
    }

    #[test]
    fn openai_compatible_chat_preserves_extra_usage_fields_from_provider_specific_responses() {
        let usage = json!({
            "prompt_tokens": 18,
            "completion_tokens": 439,
            "total_tokens": 457,
            "queue_time": 0.061348671,
            "prompt_time": 0.000211569,
            "completion_time": 0.798181818,
            "total_time": 0.798393387
        });
        let result = openai_compatible_chat_generate_result_with_usage(usage.clone());

        assert_eq!(result.usage.input_tokens.total, Some(18));
        assert_eq!(result.usage.output_tokens.total, Some(439));
        assert_eq!(result.usage.raw, usage.as_object().cloned());
    }

    #[test]
    fn openai_compatible_chat_injects_json_instruction_when_response_format_body_is_disabled() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "choices": [
                            {
                                "message": {
                                    "content": "{\"answer\":\"ok\"}"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_supports_json_object_response_format(false),
        )
        .with_transport(transport)
        .chat_model("test-chat-model");
        let response_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "answer": {
                    "type": "string"
                }
            },
            "required": ["answer"]
        }))
        .expect("schema deserializes");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Return an answer."),
                    )]),
                )])
                .with_response_format(
                    LanguageModelResponseFormat::json().with_schema(response_schema),
                ),
            ),
        );

        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Unsupported { feature, .. } if feature == "responseFormat"
            )
        }));

        let request_body = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured")
            .body
            .and_then(|body| body.as_text().map(str::to_string))
            .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok())
            .expect("request body is JSON");
        assert!(request_body.get("response_format").is_none());
        let messages = request_body
            .get("messages")
            .and_then(JsonValue::as_array)
            .expect("messages are sent");
        assert_eq!(messages[0]["role"], "system");
        assert!(
            messages[0]["content"]
                .as_str()
                .is_some_and(|content| content.contains("JSON schema:"))
        );
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Return an answer.");
    }

    #[test]
    fn openai_compatible_chat_passes_tools_tool_choice_and_provider_options() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let abort_controller = LanguageModelAbortController::new();
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "choices": [
                            {
                                "message": {
                                    "content": "ok"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_supports_structured_outputs(true),
        )
        .with_transport(transport)
        .chat_model("test-chat-model");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai-compatible": {
                "user": "deprecated-user",
                "reasoningEffort": "low"
            },
            "openaiCompatible": {
                "textVerbosity": "low"
            },
            "test-provider": {
                "reasoningEffort": "medium",
                "someCustomOption": "raw-value",
                "user": "raw-user"
            },
            "testProvider": {
                "someCustomOption": "camel-value",
                "strictJsonSchema": false,
                "user": "camel-user"
            }
        }))
        .expect("provider options deserialize");
        let input_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string"
                }
            },
            "required": ["city"]
        }))
        .expect("schema deserializes");
        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use the weather tool"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Function(
                    LanguageModelFunctionTool::new("weather", input_schema.clone())
                        .with_description("Get weather")
                        .with_strict(false),
                ))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "gateway.unsupported",
                    "unsupported",
                    JsonObject::new(),
                )))
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "weather".to_string(),
                })
                .with_reasoning(LanguageModelReasoningEffort::High)
                .with_response_format(
                    LanguageModelResponseFormat::json().with_schema(input_schema.clone()),
                )
                .with_provider_options(provider_options)
                .with_abort_signal(abort_controller.signal()),
            ),
        );

        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Deprecated { setting, .. }
                    if setting == "providerOptions key 'openai-compatible'"
            )
        }));
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Deprecated { setting, .. }
                    if setting == "providerOptions key 'test-provider'"
            )
        }));
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Unsupported { feature, .. }
                    if feature == "provider-defined tool gateway.unsupported"
            )
        }));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let request_signal = request.abort_signal.clone().expect("abort signal set");
        abort_controller.abort_with_reason("client-disconnected");
        assert!(request_signal.is_aborted());
        assert_eq!(request_signal.reason(), Some(json!("client-disconnected")));

        assert_eq!(
            request
                .body
                .and_then(|body| body.as_text().map(str::to_string))
                .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok()),
            Some(json!({
                "model": "test-chat-model",
                "messages": [
                    {
                        "role": "user",
                        "content": "Use the weather tool"
                    }
                ],
                "user": "camel-user",
                "reasoning_effort": "medium",
                "verbosity": "low",
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "schema": input_schema,
                        "strict": false,
                        "name": "response"
                    }
                },
                "someCustomOption": "camel-value",
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "weather",
                            "description": "Get weather",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "city": {
                                        "type": "string"
                                    }
                                },
                                "required": ["city"]
                            },
                            "strict": false
                        }
                    }
                ],
                "tool_choice": {
                    "type": "function",
                    "function": {
                        "name": "weather"
                    }
                }
            }))
        );
    }

    #[test]
    fn openai_compatible_chat_converts_multimodal_user_messages() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "choices": [
                            {
                                "message": {
                                    "content": "ok"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let message_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "priority": "high"
            },
            "ignoredProvider": {
                "ignored": true
            }
        }))
        .expect("metadata deserializes");
        let text_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "sentiment": "positive"
            }
        }))
        .expect("metadata deserializes");
        let image_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "alt_text": "A sample image"
            }
        }))
        .expect("metadata deserializes");
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new("Hello").with_provider_options(text_metadata),
                )])
                .with_provider_options(message_metadata.clone()),
            ),
            LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Summarize these inputs")
                            .with_provider_options(message_metadata.clone()),
                    ),
                    LanguageModelUserContentPart::File(
                        LanguageModelFilePart::new(
                            FileData::Data {
                                data: FileDataContent::Bytes(vec![0, 1, 2, 3]),
                            },
                            "image/png",
                        )
                        .with_provider_options(image_metadata),
                    ),
                    LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                        FileData::Url {
                            url: Url::parse("https://example.com/image.jpg")
                                .expect("url parses"),
                        },
                        "image/*",
                    )),
                    LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                        FileData::Data {
                            data: FileDataContent::Base64("AAECAw==".to_string()),
                        },
                        "audio/wav",
                    )),
                    LanguageModelUserContentPart::File(
                        LanguageModelFilePart::new(
                            FileData::Data {
                                data: FileDataContent::Bytes(vec![0, 1, 2, 3]),
                            },
                            "application/pdf",
                        )
                        .with_filename("report.pdf"),
                    ),
                    LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                        FileData::Data {
                            data: FileDataContent::Base64("SGVsbG8=".to_string()),
                        },
                        "text/markdown",
                    )),
                    LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                        FileData::Url {
                            url: Url::parse("https://example.com/readme.md")
                                .expect("url parses"),
                        },
                        "text/markdown",
                    )),
                ])
                .with_provider_options(message_metadata),
            ),
        ])));

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);
        assert_eq!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .clone()
                .expect("request is captured")
                .body
                .and_then(|body| body.as_text().map(str::to_string))
                .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok()),
            Some(json!({
                "model": "test-chat-model",
                "messages": [
                    {
                        "role": "user",
                        "content": "Hello",
                        "sentiment": "positive"
                    },
                    {
                        "role": "user",
                        "priority": "high",
                        "content": [
                            {
                                "type": "text",
                                "text": "Summarize these inputs",
                                "priority": "high"
                            },
                            {
                                "type": "image_url",
                                "image_url": {
                                    "url": "data:image/png;base64,AAECAw=="
                                },
                                "alt_text": "A sample image"
                            },
                            {
                                "type": "image_url",
                                "image_url": {
                                    "url": "https://example.com/image.jpg"
                                }
                            },
                            {
                                "type": "input_audio",
                                "input_audio": {
                                    "data": "AAECAw==",
                                    "format": "wav"
                                }
                            },
                            {
                                "type": "file",
                                "file": {
                                    "filename": "report.pdf",
                                    "file_data": "data:application/pdf;base64,AAECAw=="
                                }
                            },
                            {
                                "type": "text",
                                "text": "Hello"
                            },
                            {
                                "type": "text",
                                "text": "https://example.com/readme.md"
                            }
                        ]
                    }
                ]
            }))
        );
    }

    #[test]
    fn openai_compatible_chat_rejects_unsupported_file_messages_before_transport() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                panic!("transport should not be called for unsupported prompt conversion")
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Bytes(vec![0, 1, 2, 3]),
                    },
                    "video/mp4",
                )),
            ])),
        ])));

        assert_eq!(result.finish_reason.unified, FinishReason::Error);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("test-provider"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("'file part media type video/mp4' functionality not supported")
        );
    }

    #[test]
    fn openai_compatible_chat_converts_assistant_tool_history() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "choices": [
                            {
                                "message": {
                                    "content": "ok"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 1,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let assistant_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "globalPriority": "high"
            }
        }))
        .expect("metadata deserializes");
        let tool_call_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "function_call_reason": "user request"
            },
            "google": {
                "thoughtSignature": "<Signature A>"
            }
        }))
        .expect("metadata deserializes");
        let tool_result_metadata: ProviderOptions = serde_json::from_value(json!({
            "openaiCompatible": {
                "partial": true
            }
        }))
        .expect("metadata deserializes");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::Assistant(
                LanguageModelAssistantMessage::new(vec![
                    LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                        "Checking that now...",
                    )),
                    LanguageModelAssistantContentPart::Reasoning(
                        LanguageModelReasoningPart::new("Need weather data."),
                    ),
                    LanguageModelAssistantContentPart::ToolCall(
                        LanguageModelToolCallPart::new(
                            "call_1",
                            "weather",
                            json!({ "city": "Brisbane" }),
                        )
                        .with_provider_options(tool_call_metadata),
                    ),
                ])
                .with_provider_options(assistant_metadata),
            ),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(
                    LanguageModelToolResultPart::new(
                        "call_1",
                        "weather",
                        LanguageModelToolResultOutput::json(json!({
                            "temperature": 24
                        })),
                    )
                    .with_provider_options(tool_result_metadata),
                ),
            ])),
        ])));

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);
        assert_eq!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .clone()
                .expect("request is captured")
                .body
                .and_then(|body| body.as_text().map(str::to_string))
                .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok()),
            Some(json!({
                "model": "test-chat-model",
                "messages": [
                    {
                        "role": "assistant",
                        "content": "Checking that now...",
                        "reasoning_content": "Need weather data.",
                        "globalPriority": "high",
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "weather",
                                    "arguments": "{\"city\":\"Brisbane\"}"
                                },
                                "function_call_reason": "user request",
                                "extra_content": {
                                    "google": {
                                        "thought_signature": "<Signature A>"
                                    }
                                }
                            }
                        ]
                    },
                    {
                        "role": "tool",
                        "content": "{\"temperature\":24}",
                        "tool_call_id": "call_1",
                        "partial": true
                    }
                ]
            }))
        );
    }

    #[test]
    fn openai_compatible_chat_streams_reasoning_raw_chunks_and_parse_errors() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    sse_body([
                        json!({
                            "id": "chatcmpl-stream-test",
                            "created": 1711357598,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "role": "assistant",
                                        "reasoning_content": "Let me think"
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-stream-test",
                            "created": 1711357598,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "reasoning": " about this"
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-stream-test",
                            "created": 1711357598,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "content": "Here's my response"
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-stream-test",
                            "created": 1711357598,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {},
                                    "finish_reason": "stop"
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 2,
                                "completion_tokens": 3
                            }
                        }),
                    ]),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(
            model.do_stream(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("Think first"),
                )])
                .with_include_raw_chunks(true),
            ),
        );

        assert!(matches!(
            result.stream.first(),
            Some(LanguageModelStreamPart::StreamStart(_))
        ));
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Raw(_)))
        );
        assert_eq!(
            result
                .stream
                .iter()
                .filter_map(|part| match part {
                    LanguageModelStreamPart::ReasoningDelta(part) => Some(part.delta.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["Let me think", " about this"]
        );
        assert_eq!(
            result
                .stream
                .iter()
                .filter_map(|part| match part {
                    LanguageModelStreamPart::TextDelta(part) => Some(part.delta.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["Here's my response"]
        );
        assert!(matches!(
            result.stream.last(),
            Some(LanguageModelStreamPart::Finish(finish))
                if finish.finish_reason.unified == FinishReason::Stop
                    && finish.usage.input_tokens.total == Some(2)
        ));

        let parse_error_transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    "data: {not json}\n\ndata: [DONE]\n\n",
                ))))
            });
        let parse_error_model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com"),
        )
        .with_transport(parse_error_transport)
        .chat_model("test-chat-model");
        let parse_error_result =
            poll_ready(parse_error_model.do_stream(LanguageModelCallOptions::new(Vec::new())));

        assert!(
            parse_error_result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Error(_)))
        );
        assert!(matches!(
            parse_error_result.stream.last(),
            Some(LanguageModelStreamPart::Finish(finish))
                if finish.finish_reason.unified == FinishReason::Error
        ));
    }

    #[test]
    fn openai_compatible_chat_maps_tool_calls_from_generate() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "choices": [
                            {
                                "message": {
                                    "role": "assistant",
                                    "content": null,
                                    "tool_calls": [
                                        {
                                            "id": "call_1",
                                            "type": "function",
                                            "function": {
                                                "name": "weather",
                                                "arguments": "{\"city\":\"Brisbane\"}"
                                            },
                                            "extra_content": {
                                                "google": {
                                                    "thought_signature": "signature-1"
                                                }
                                            }
                                        }
                                    ]
                                },
                                "finish_reason": "tool_calls"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 2,
                            "completion_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                    "What is the weather?",
                )),
            ])),
        ])));

        assert_eq!(result.finish_reason.unified, FinishReason::ToolCalls);
        assert!(matches!(
            result.content.first(),
            Some(LanguageModelContent::ToolCall(tool_call))
                if tool_call.tool_call_id == "call_1"
                    && tool_call.tool_name == "weather"
                    && tool_call.input == "{\"city\":\"Brisbane\"}"
                    && tool_call
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("test-provider"))
                        .and_then(|metadata| metadata.get("thoughtSignature"))
                        .and_then(JsonValue::as_str)
                        == Some("signature-1")
        ));
    }

    #[test]
    fn openai_compatible_chat_streams_tool_calls() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    sse_body([
                        json!({
                            "id": "chatcmpl-tool-stream",
                            "created": 1711115037,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "tool_calls": [
                                            {
                                                "index": 0,
                                                "id": "call_1",
                                                "type": "function",
                                                "function": {
                                                    "name": "weather",
                                                    "arguments": "{\"city\""
                                                },
                                                "extra_content": {
                                                    "google": {
                                                        "thought_signature": "signature-1"
                                                    }
                                                }
                                            }
                                        ]
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-tool-stream",
                            "created": 1711115037,
                            "model": "test-chat-model",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "tool_calls": [
                                            {
                                                "index": 0,
                                                "function": {
                                                    "arguments": ":\"Brisbane\"}"
                                                }
                                            }
                                        ]
                                    },
                                    "finish_reason": "tool_calls"
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 2,
                                "completion_tokens": 1
                            }
                        }),
                    ]),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("test-chat-model");
        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                    "What is the weather?",
                )),
            ])),
        ])));

        assert!(result.stream.iter().any(|part| {
            matches!(
                part,
                LanguageModelStreamPart::ToolInputStart(start)
                    if start.id == "call_1" && start.tool_name == "weather"
            )
        }));
        assert_eq!(
            result
                .stream
                .iter()
                .filter_map(|part| match part {
                    LanguageModelStreamPart::ToolInputDelta(delta) => Some(delta.delta.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["{\"city\"", ":\"Brisbane\"}"]
        );
        assert!(result.stream.iter().any(|part| {
            matches!(
                part,
                LanguageModelStreamPart::ToolInputEnd(end) if end.id == "call_1"
            )
        }));
        assert!(result.stream.iter().any(|part| {
            matches!(
                part,
                LanguageModelStreamPart::ToolCall(tool_call)
                    if tool_call.tool_call_id == "call_1"
                        && tool_call.tool_name == "weather"
                        && tool_call.input == "{\"city\":\"Brisbane\"}"
                        && tool_call
                            .provider_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.get("test-provider"))
                            .and_then(|metadata| metadata.get("thoughtSignature"))
                            .and_then(JsonValue::as_str)
                            == Some("signature-1")
            )
        }));
        assert!(matches!(
            result.stream.last(),
            Some(LanguageModelStreamPart::Finish(finish))
                if finish.finish_reason.unified == FinishReason::ToolCalls
        ));
    }

    #[test]
    fn openai_compatible_provider_configures_headers_urls_and_model_aliases() {
        let provider = create_openai_compatible(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com/")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value")
                .with_query_param("Custom-Param", "value")
                .with_include_usage(true)
                .with_supports_structured_outputs(true),
        );

        let chat = provider.chat_model("chat-model");
        let language = provider.language_model("language-model");
        let completion = provider.completion_model("completion-model");
        let embedding = provider.embedding_model("embedding-model");
        let text_embedding = provider.text_embedding_model("embedding-model");
        let image = provider.image_model("image-model");

        assert_eq!(chat.provider(), "test-provider.chat");
        assert_eq!(language.model_id(), "language-model");
        assert_eq!(completion.provider(), "test-provider.completion");
        assert_eq!(embedding.provider(), "test-provider.embedding");
        assert_eq!(text_embedding.model_id(), "embedding-model");
        assert_eq!(image.provider(), "test-provider.image");
        assert_eq!(poll_ready(image.max_images_per_call()), Some(10));
        assert!(chat.supports_structured_outputs());
        assert_eq!(
            chat.model_url("/v1/chat").expect("url is valid"),
            "https://api.example.com/v1/chat?Custom-Param=value"
        );
        assert_eq!(
            completion
                .model_url("/v1/completions")
                .expect("url is valid"),
            "https://api.example.com/v1/completions?Custom-Param=value"
        );
        assert_eq!(
            embedding.model_url("/v1/embeddings").expect("url is valid"),
            "https://api.example.com/v1/embeddings?Custom-Param=value"
        );
        assert_eq!(
            image.model_url("/v1/images").expect("url is valid"),
            "https://api.example.com/v1/images?Custom-Param=value"
        );

        let headers = chat.request_headers(None);
        assert_eq!(
            headers.get("authorization").and_then(Option::as_deref),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            headers.get("custom-header").and_then(Option::as_deref),
            Some("value")
        );
        assert_eq!(
            headers.get("user-agent").and_then(Option::as_deref),
            Some("ai-sdk/openai-compatible/0.1.0")
        );
        assert_eq!(
            completion
                .request_headers(None)
                .get("user-agent")
                .and_then(Option::as_deref),
            Some("ai-sdk/openai-compatible/0.1.0")
        );
        assert_eq!(
            embedding
                .request_headers(None)
                .get("user-agent")
                .and_then(Option::as_deref),
            Some("ai-sdk/openai-compatible/0.1.0")
        );
        assert_eq!(
            image
                .request_headers(None)
                .get("user-agent")
                .and_then(Option::as_deref),
            Some("ai-sdk/openai-compatible/0.1.0")
        );
    }

    #[test]
    fn openai_compatible_provider_creates_provider_with_correct_configuration() {
        let provider = create_openai_compatible(openai_compatible_default_provider_settings());
        let model = provider.language_model("model-id");

        assert_openai_compatible_default_request_headers(&model.request_headers(None));
        assert_eq!(model.provider(), "test-provider.chat");
        assert_eq!(
            model.model_url("/v1/chat").expect("url is valid"),
            "https://api.example.com/v1/chat?Custom-Param=value"
        );
    }

    #[test]
    fn openai_compatible_provider_creates_headers_without_authorization_when_no_api_key_provided() {
        let provider = create_openai_compatible(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_header("custom-header", "value"),
        );
        let model = provider.language_model("model-id");
        let headers = model.request_headers(None);

        assert_eq!(headers.get("authorization"), None);
        assert_eq!(
            headers.get("custom-header").and_then(Option::as_deref),
            Some("value")
        );
        assert_eq!(
            headers.get("user-agent").and_then(Option::as_deref),
            Some("ai-sdk/openai-compatible/0.1.0")
        );
    }

    #[test]
    fn openai_compatible_provider_creates_chat_model_with_correct_configuration() {
        let provider = create_openai_compatible(openai_compatible_default_provider_settings());
        let model = provider.chat_model("chat-model");

        assert_eq!(model.model_id(), "chat-model");
        assert_openai_compatible_default_request_headers(&model.request_headers(None));
        assert_eq!(model.provider(), "test-provider.chat");
        assert_eq!(
            model.model_url("/v1/chat").expect("url is valid"),
            "https://api.example.com/v1/chat?Custom-Param=value"
        );
    }

    #[test]
    fn openai_compatible_provider_creates_completion_model_with_correct_configuration() {
        let provider = create_openai_compatible(openai_compatible_default_provider_settings());
        let model = provider.completion_model("completion-model");

        assert_eq!(model.model_id(), "completion-model");
        assert_openai_compatible_default_request_headers(&model.request_headers(None));
        assert_eq!(model.provider(), "test-provider.completion");
        assert_eq!(
            model.model_url("/v1/completions").expect("url is valid"),
            "https://api.example.com/v1/completions?Custom-Param=value"
        );
    }

    #[test]
    fn openai_compatible_provider_creates_embedding_model_with_correct_configuration() {
        let provider = create_openai_compatible(openai_compatible_default_provider_settings());
        let model = provider.embedding_model("embedding-model");

        assert_eq!(model.model_id(), "embedding-model");
        assert_openai_compatible_default_request_headers(&model.request_headers(None));
        assert_eq!(model.provider(), "test-provider.embedding");
        assert_eq!(
            model.model_url("/v1/embeddings").expect("url is valid"),
            "https://api.example.com/v1/embeddings?Custom-Param=value"
        );
    }

    #[test]
    fn openai_compatible_provider_uses_language_model_as_default_chat_model_alias() {
        let provider = create_openai_compatible(openai_compatible_default_provider_settings());
        let model = provider.language_model("model-id");

        assert_eq!(model.model_id(), "model-id");
        assert_eq!(model.provider(), "test-provider.chat");
    }

    #[test]
    fn openai_compatible_provider_creates_url_without_query_parameters_when_unspecified() {
        let provider = create_openai_compatible(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_api_key("test-api-key"),
        );
        let model = provider.language_model("model-id");

        assert_eq!(
            model.model_url("/v1/chat").expect("url is valid"),
            "https://api.example.com/v1/chat"
        );
    }

    #[test]
    fn openai_compatible_provider_passes_structured_outputs_to_chat_and_language_models_only() {
        let provider = create_openai_compatible(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_supports_structured_outputs(true),
        );

        let default_model = provider.language_model("model-id");
        let chat_model = provider.chat_model("chat-model");
        let language_model = provider.language_model("language-model");
        let completion_model = provider.completion_model("completion-model");
        let embedding_model = provider.embedding_model("embedding-model");
        let image_model = provider.image_model("image-model");

        assert!(default_model.supports_structured_outputs());
        assert!(chat_model.supports_structured_outputs());
        assert!(language_model.supports_structured_outputs());
        assert_eq!(
            completion_model.config.settings.supports_structured_outputs,
            None
        );
        assert_eq!(
            embedding_model.config.settings.supports_structured_outputs,
            None
        );
        assert_eq!(
            image_model.config.settings.supports_structured_outputs,
            None
        );
    }

    #[test]
    fn openai_compatible_provider_passes_metadata_extractor_to_chat_model() {
        let provider = create_openai_compatible(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_metadata_extractor(OpenAICompatibleMetadataExtractor::new()),
        );

        assert!(
            provider
                .chat_model("chat-model")
                .config
                .settings
                .metadata_extractor
                .is_some()
        );
        assert!(
            provider
                .language_model("language-model")
                .config
                .settings
                .metadata_extractor
                .is_some()
        );
        assert!(
            provider
                .completion_model("completion-model")
                .config
                .settings
                .metadata_extractor
                .is_none()
        );
        assert!(
            provider
                .embedding_model("embedding-model")
                .config
                .settings
                .metadata_extractor
                .is_none()
        );
        assert!(
            provider
                .image_model("image-model")
                .config
                .settings
                .metadata_extractor
                .is_none()
        );
    }

    #[test]
    fn openai_compatible_chat_processes_metadata_from_complete_response() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "chatcmpl-123",
                        "object": "chat.completion",
                        "created": 1711115037,
                        "model": "gpt-5",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "test_field": "test_value"
                    })
                    .to_string(),
                ))))
            });
        let metadata_extractor =
            OpenAICompatibleMetadataExtractor::new().with_extract_metadata(|args| {
                ready(
                    args.parsed_body
                        .get("test_field")
                        .and_then(JsonValue::as_str)
                        .map(openai_compatible_test_provider_metadata),
                )
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_metadata_extractor(metadata_extractor),
        )
        .with_transport(transport)
        .chat_model("gpt-5");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
            ])),
        ])));

        assert_eq!(
            result.provider_metadata,
            Some(openai_compatible_test_provider_metadata("test_value"))
        );
        assert_eq!(
            result
                .request
                .as_ref()
                .and_then(|request| request.body.clone()),
            Some(json!({
                "model": "gpt-5",
                "messages": [
                    {
                        "role": "user",
                        "content": "Hello"
                    }
                ]
            }))
        );
    }

    #[test]
    fn openai_compatible_chat_processes_metadata_from_streaming_response() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    sse_body([
                        json!({
                            "choices": [
                                {
                                    "delta": {
                                        "content": "Hello"
                                    }
                                }
                            ]
                        }),
                        json!({
                            "choices": [
                                {
                                    "finish_reason": "stop"
                                }
                            ],
                            "test_field": "test_value"
                        }),
                    ]),
                ))))
            });
        let metadata_extractor =
            OpenAICompatibleMetadataExtractor::new().with_stream_extractor(|| {
                let accumulated_value = Arc::new(Mutex::new(None::<String>));
                let accumulated_value_for_process = Arc::clone(&accumulated_value);

                OpenAICompatibleStreamMetadataExtractor::new(
                    move |chunk| {
                        let is_stop_chunk = chunk
                            .get("choices")
                            .and_then(JsonValue::as_array)
                            .and_then(|choices| choices.first())
                            .and_then(|choice| choice.get("finish_reason"))
                            .and_then(JsonValue::as_str)
                            == Some("stop");
                        if is_stop_chunk
                            && let Some(value) = chunk.get("test_field").and_then(JsonValue::as_str)
                        {
                            *accumulated_value_for_process
                                .lock()
                                .expect("metadata mutex is not poisoned") = Some(value.to_string());
                        }
                    },
                    move || {
                        accumulated_value
                            .lock()
                            .expect("metadata mutex is not poisoned")
                            .clone()
                            .map(openai_compatible_test_provider_metadata)
                    },
                )
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_metadata_extractor(metadata_extractor),
        )
        .with_transport(transport)
        .chat_model("gpt-5");

        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
            ])),
        ])));
        let finish_metadata = result.stream.iter().find_map(|part| match part {
            LanguageModelStreamPart::Finish(finish) => finish.provider_metadata.clone(),
            _ => None,
        });

        assert_eq!(
            finish_metadata,
            Some(openai_compatible_test_provider_metadata("test_value"))
        );
        assert_eq!(
            result
                .request
                .as_ref()
                .and_then(|request| request.body.clone()),
            Some(json!({
                "model": "gpt-5",
                "messages": [
                    {
                        "role": "user",
                        "content": "Hello"
                    }
                ],
                "stream": true
            }))
        );
    }

    #[test]
    fn openai_compatible_chat_transforms_request_body_in_do_generate_when_transform_request_body_is_provided()
     {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transform_inputs = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let transform_inputs_for_callback = Arc::clone(&transform_inputs);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "chatcmpl-transform",
                        "object": "chat.completion",
                        "created": 1711115037,
                        "model": "grok-3",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello!"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 30,
                            "total_tokens": 34
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_transform_request_body(move |body| {
                    transform_inputs_for_callback
                        .lock()
                        .expect("transform input mutex is not poisoned")
                        .push(body.clone());
                    let mut transformed = body;
                    if let Some(object) = transformed.as_object_mut() {
                        object.insert("custom_field".to_string(), json!("added-by-transform"));
                    }
                    transformed
                }),
        )
        .with_transport(transport)
        .chat_model("grok-3");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
            ])),
        ])));

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);
        let transform_inputs = transform_inputs
            .lock()
            .expect("transform input mutex is not poisoned");
        assert_eq!(transform_inputs.len(), 1);
        assert_eq!(
            transform_inputs[0],
            json!({
                "model": "grok-3",
                "messages": [
                    {
                        "role": "user",
                        "content": "Hello"
                    }
                ]
            })
        );

        let request_body = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .as_ref()
            .map(openai_compatible_request_body_json)
            .expect("request is captured");
        assert_eq!(request_body["custom_field"], "added-by-transform");
        assert_eq!(
            result
                .request
                .as_ref()
                .and_then(|request| request.body.clone())
                .and_then(|body| body.get("custom_field").cloned()),
            Some(json!("added-by-transform"))
        );
    }

    #[test]
    fn openai_compatible_chat_transforms_request_body_in_do_stream_when_transform_request_body_is_provided()
     {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transform_inputs = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let transform_inputs_for_callback = Arc::clone(&transform_inputs);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    sse_body([
                        json!({
                            "id": "chatcmpl-transform",
                            "object": "chat.completion.chunk",
                            "created": 1711115037,
                            "model": "grok-3",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "role": "assistant",
                                        "content": "Hello"
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-transform",
                            "object": "chat.completion.chunk",
                            "created": 1711115037,
                            "model": "grok-3",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "content": "!"
                                    },
                                    "finish_reason": "stop"
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 4,
                                "completion_tokens": 2,
                                "total_tokens": 6
                            }
                        }),
                    ]),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(
            OpenAICompatibleProviderSettings::new("test-provider", "https://api.example.com")
                .with_transform_request_body(move |body| {
                    transform_inputs_for_callback
                        .lock()
                        .expect("transform input mutex is not poisoned")
                        .push(body.clone());
                    let mut transformed = body;
                    if let Some(object) = transformed.as_object_mut() {
                        object.insert("custom_field".to_string(), json!("added-by-transform"));
                    }
                    transformed
                }),
        )
        .with_transport(transport)
        .chat_model("grok-3");

        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
            ])),
        ])));

        assert!(result.stream.iter().any(
            |part| matches!(part, LanguageModelStreamPart::TextDelta(delta) if delta.delta == "!")
        ));
        let transform_inputs = transform_inputs
            .lock()
            .expect("transform input mutex is not poisoned");
        assert_eq!(transform_inputs.len(), 1);
        assert_eq!(
            transform_inputs[0],
            json!({
                "model": "grok-3",
                "messages": [
                    {
                        "role": "user",
                        "content": "Hello"
                    }
                ],
                "stream": true
            })
        );

        let request_body = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .as_ref()
            .map(openai_compatible_request_body_json)
            .expect("request is captured");
        assert_eq!(request_body["custom_field"], "added-by-transform");
        assert_eq!(
            result
                .request
                .as_ref()
                .and_then(|request| request.body.clone())
                .and_then(|body| body.get("custom_field").cloned()),
            Some(json!("added-by-transform"))
        );
    }

    #[test]
    fn openai_compatible_chat_works_without_transform_request_body() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "chatcmpl-transform",
                        "object": "chat.completion",
                        "created": 1711115037,
                        "model": "grok-3",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello!"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 30,
                            "total_tokens": 34
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = OpenAICompatibleProvider::from_settings(OpenAICompatibleProviderSettings::new(
            "test-provider",
            "https://api.example.com",
        ))
        .with_transport(transport)
        .chat_model("grok-3");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
            ])),
        ])));

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);
        let request_body = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .as_ref()
            .map(openai_compatible_request_body_json)
            .expect("request is captured");
        assert_eq!(request_body["model"], "grok-3");
        assert!(request_body.get("custom_field").is_none());
    }

    #[test]
    fn openai_compatible_provider_passes_include_usage_true_to_all_language_model_streams() {
        let request_bodies = openai_compatible_stream_request_bodies_for_include_usage(Some(true));

        assert_openai_compatible_include_usage(
            &request_bodies,
            Some(json!({
                "include_usage": true
            })),
        );
    }

    #[test]
    fn openai_compatible_provider_omits_include_usage_false_from_all_language_model_streams() {
        let request_bodies = openai_compatible_stream_request_bodies_for_include_usage(Some(false));

        assert_openai_compatible_include_usage(&request_bodies, None);
    }

    #[test]
    fn openai_compatible_provider_omits_unspecified_include_usage_from_all_language_model_streams()
    {
        let request_bodies = openai_compatible_stream_request_bodies_for_include_usage(None);

        assert_openai_compatible_include_usage(&request_bodies, None);
    }

    fn sse_body(events: impl IntoIterator<Item = JsonValue>) -> String {
        events
            .into_iter()
            .map(|event| format!("data: {event}\n\n"))
            .chain(["data: [DONE]\n\n".to_string()])
            .collect()
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        struct NoopWake;

        impl Wake for NoopWake {
            fn wake(self: Arc<Self>) {}
        }

        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("future unexpectedly pending in test"),
        }
    }
}
