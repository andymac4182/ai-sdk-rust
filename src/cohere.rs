use std::collections::BTreeMap;
use std::convert::Infallible;
use std::env;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::embedding_model::{
    EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResponse, EmbeddingModelResult,
    EmbeddingModelUsage,
};
use crate::file_data::FileData;
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelAssistantContentPart,
    LanguageModelCallOptions, LanguageModelContent, LanguageModelDocumentSource,
    LanguageModelErrorStreamPart, LanguageModelFinishReason, LanguageModelGenerateResult,
    LanguageModelMessage, LanguageModelRawStreamPart, LanguageModelReasoning,
    LanguageModelReasoningDelta, LanguageModelReasoningEffort, LanguageModelReasoningEnd,
    LanguageModelReasoningStart, LanguageModelRequest, LanguageModelResponse,
    LanguageModelResponseFormat, LanguageModelSource, LanguageModelStreamFinish,
    LanguageModelStreamPart, LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
    LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
    LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextStart,
    LanguageModelTool, LanguageModelToolCall, LanguageModelToolChoice, LanguageModelToolInputDelta,
    LanguageModelToolInputEnd, LanguageModelToolInputStart, LanguageModelToolResultOutput,
    LanguageModelUsage, LanguageModelUserContentPart, OutputTokenUsage,
};
use crate::openai_compatible::{OpenAICompatibleImageModel, OpenAICompatibleTransport};
use crate::provider::{
    ModelType, NoSuchModelError, Provider, ProviderMetadata, ProviderOptions,
    ProviderWithRerankingModel, SpecificationVersion, TooManyEmbeddingValuesForCallError,
};
use crate::provider_utils::{
    FetchErrorInfo, HandledFetchError, ParseJsonResult, PostJsonToApiOptions, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, ReasoningLevel, RuntimeEnvironment, combine_headers,
    convert_to_base64, create_event_source_response_handler, create_json_error_response_handler,
    create_json_response_handler, generate_id, get_top_level_media_type,
    map_reasoning_to_provider_budget, post_json_to_api, resolve_full_media_type, safe_parse_json,
    with_user_agent_suffix, without_trailing_slash,
};
use crate::reranking_model::{
    RerankingModel, RerankingModelCallOptions, RerankingModelDocuments, RerankingModelRanking,
    RerankingModelResponse, RerankingModelResult,
};
use crate::warning::Warning;

/// Default base URL for upstream `@ai-sdk/cohere` API calls.
pub const DEFAULT_COHERE_BASE_URL: &str = "https://api.cohere.com/v2";

const COHERE_CHAT_PROVIDER_ID: &str = "cohere.chat";
const COHERE_EMBEDDING_PROVIDER_ID: &str = "cohere.textEmbedding";
const COHERE_RERANKING_PROVIDER_ID: &str = "cohere.reranking";
const COHERE_PROVIDER_OPTIONS_NAME: &str = "cohere";

type CohereGenerateId = Arc<dyn Fn() -> String + Send + Sync>;

/// Settings for the upstream Cohere provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CohereProviderSettings {
    /// Base URL for Cohere API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Cohere API key. When omitted, `COHERE_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl CohereProviderSettings {
    /// Creates empty Cohere provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Cohere API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Cohere API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// Upstream Cohere provider foundation.
#[derive(Clone)]
pub struct CohereProvider {
    settings: CohereProviderSettings,
    transport: OpenAICompatibleTransport,
    generate_id: CohereGenerateId,
}

/// Cohere chat model for `/chat` calls.
#[derive(Clone)]
pub struct CohereChatLanguageModel {
    model_id: String,
    base_url: String,
    settings: CohereProviderSettings,
    transport: OpenAICompatibleTransport,
    generate_id: CohereGenerateId,
}

/// Cohere embedding model for `/embed` calls.
#[derive(Clone)]
pub struct CohereEmbeddingModel {
    model_id: String,
    base_url: String,
    settings: CohereProviderSettings,
    transport: OpenAICompatibleTransport,
}

/// Cohere reranking model for `/rerank` calls.
#[derive(Clone)]
pub struct CohereRerankingModel {
    model_id: String,
    base_url: String,
    settings: CohereProviderSettings,
    transport: OpenAICompatibleTransport,
}

impl CohereProvider {
    /// Creates a Cohere provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(CohereProviderSettings::new())
    }

    /// Creates a provider from explicit Cohere settings.
    pub fn from_settings(settings: CohereProviderSettings) -> Self {
        Self {
            settings,
            transport: default_cohere_transport(),
            generate_id: Arc::new(generate_id),
        }
    }

    /// Sets the Cohere API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the Cohere API base URL for this provider.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Replaces the id generator used for Cohere citation source ids.
    pub fn with_generate_id<F>(mut self, generate_id: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        self.generate_id = Arc::new(generate_id);
        self
    }

    /// Creates a Cohere chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> CohereChatLanguageModel {
        CohereChatLanguageModel::new(
            model_id,
            cohere_base_url(&self.settings),
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.generate_id),
        )
    }

    /// Alias for [`CohereProvider::language_model`].
    pub fn chat(&self, model_id: impl Into<String>) -> CohereChatLanguageModel {
        self.language_model(model_id)
    }

    /// Creates a Cohere embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> CohereEmbeddingModel {
        CohereEmbeddingModel::new(
            model_id,
            cohere_base_url(&self.settings),
            self.settings.clone(),
            Arc::clone(&self.transport),
        )
    }

    /// Alias for [`CohereProvider::embedding_model`].
    pub fn embedding(&self, model_id: impl Into<String>) -> CohereEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`CohereProvider::embedding_model`].
    pub fn text_embedding_model(&self, model_id: impl Into<String>) -> CohereEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`CohereProvider::embedding_model`].
    pub fn text_embedding(&self, model_id: impl Into<String>) -> CohereEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Reports that Cohere does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    /// Creates a Cohere reranking model.
    pub fn reranking_model(&self, model_id: impl Into<String>) -> CohereRerankingModel {
        CohereRerankingModel::new(
            model_id,
            cohere_base_url(&self.settings),
            self.settings.clone(),
            Arc::clone(&self.transport),
        )
    }

    /// Alias for [`CohereProvider::reranking_model`].
    pub fn reranking(&self, model_id: impl Into<String>) -> CohereRerankingModel {
        self.reranking_model(model_id)
    }
}

impl Default for CohereProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for CohereProvider {
    type LanguageModel = CohereChatLanguageModel;
    type EmbeddingModel = CohereEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(CohereProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(CohereProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        CohereProvider::image_model(self, model_id)
    }
}

impl ProviderWithRerankingModel for CohereProvider {
    type RerankingModel = CohereRerankingModel;

    fn reranking_model(&self, model_id: &str) -> Result<Self::RerankingModel, NoSuchModelError> {
        Ok(CohereProvider::reranking_model(self, model_id))
    }
}

impl CohereChatLanguageModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: CohereProviderSettings,
        transport: OpenAICompatibleTransport,
        generate_id: CohereGenerateId,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            base_url: base_url.into(),
            settings,
            transport,
            generate_id,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        COHERE_CHAT_PROVIDER_ID
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let (request_body, warnings) =
            match cohere_chat_request_body(&self.model_id, &options, false) {
                Ok(result) => result,
                Err(message) => {
                    return cohere_chat_error_generate_result(
                        message,
                        json!({ "model": self.model_id }),
                    );
                }
            };
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let post_options =
            PostJsonToApiOptions::new(format!("{}/chat", self.base_url), request_body)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    cohere_chat_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    cohere_error_response,
                    cohere_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => cohere_chat_generate_result_from_response(
                response.value,
                response.raw_value,
                response.response_headers,
                request_body_for_response,
                warnings,
                &self.generate_id,
            ),
            Err(error) => {
                cohere_chat_generate_result_from_error(error, request_body_for_error, warnings)
            }
        }
    }

    async fn do_stream_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let include_raw_chunks = options.include_raw_chunks.unwrap_or(false);
        let (request_body, warnings) =
            match cohere_chat_request_body(&self.model_id, &options, true) {
                Ok(result) => result,
                Err(message) => {
                    return cohere_chat_error_stream_result(
                        message,
                        json!({ "model": self.model_id, "stream": true }),
                    );
                }
            };
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let post_options =
            PostJsonToApiOptions::new(format!("{}/chat", self.base_url), request_body)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |_request, response| {
                create_event_source_response_handler(
                    response.event_source_response_handler_options(),
                    |value| Ok::<JsonValue, Infallible>(value.clone()),
                )
                .map_err(|error| ProviderApiResponseHandlerError::other(error.to_string()))
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    cohere_error_response,
                    cohere_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => cohere_chat_stream_result_from_response(
                response.value,
                response.response_headers,
                request_body_for_response,
                warnings,
                include_raw_chunks,
            ),
            Err(error) => cohere_chat_stream_result_from_error(error, request_body_for_error),
        }
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(cohere_provider_header_entries(&self.settings)),
            optional_headers(call_headers),
        ])
    }
}

impl LanguageModel for CohereChatLanguageModel {
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
        CohereChatLanguageModel::provider(self)
    }

    fn model_id(&self) -> &str {
        CohereChatLanguageModel::model_id(self)
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(BTreeMap::from([(
            "image/*".to_string(),
            vec!["^https?://.*$".to_string()],
        )]))
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(self.do_stream_result(options))
    }
}

impl CohereEmbeddingModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: CohereProviderSettings,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            base_url: base_url.into(),
            settings,
            transport,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        COHERE_EMBEDDING_PROVIDER_ID
    }

    async fn do_embed_result(&self, options: EmbeddingModelCallOptions) -> EmbeddingModelResult {
        let request_body = cohere_embedding_request_body(&self.model_id, &options);
        let request_body_for_error = request_body.clone();

        if options.values.len() > 96 {
            let error = TooManyEmbeddingValuesForCallError::new(
                self.provider(),
                self.model_id.clone(),
                96,
                options.values,
            );
            return cohere_embedding_error_result(error.to_string(), Some(request_body_for_error));
        }

        let request_headers = self.request_headers(options.headers.as_ref());
        let post_options =
            PostJsonToApiOptions::new(format!("{}/embed", self.base_url), request_body)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(options.abort_signal);
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    cohere_embedding_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    cohere_error_response,
                    cohere_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => cohere_embedding_result_from_response(
                response.value,
                response.raw_value,
                response.response_headers,
            ),
            Err(error) => cohere_embedding_result_from_error(error, Some(request_body_for_error)),
        }
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(cohere_provider_header_entries(&self.settings)),
            optional_headers(call_headers),
        ])
    }
}

impl EmbeddingModel for CohereEmbeddingModel {
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

    fn provider(&self) -> &str {
        CohereEmbeddingModel::provider(self)
    }

    fn model_id(&self) -> &str {
        CohereEmbeddingModel::model_id(self)
    }

    fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_> {
        ready(Some(96))
    }

    fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
        ready(true)
    }

    fn do_embed(&self, options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
        Box::pin(self.do_embed_result(options))
    }
}

impl CohereRerankingModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: CohereProviderSettings,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            base_url: base_url.into(),
            settings,
            transport,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        COHERE_RERANKING_PROVIDER_ID
    }

    async fn do_rerank_result(&self, options: RerankingModelCallOptions) -> RerankingModelResult {
        let (request_body, warnings) = cohere_reranking_request_body(&self.model_id, &options);
        let request_body_for_error = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let post_options =
            PostJsonToApiOptions::new(format!("{}/rerank", self.base_url), request_body)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(options.abort_signal);
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    cohere_reranking_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    cohere_error_response,
                    cohere_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => {
                let mut result = cohere_reranking_result_from_response(
                    response.value,
                    response.raw_value,
                    response.response_headers,
                );
                for warning in warnings {
                    result = result.with_warning(warning);
                }
                result
            }
            Err(error) => {
                let mut result =
                    cohere_reranking_result_from_error(error, Some(request_body_for_error));
                for warning in warnings {
                    result = result.with_warning(warning);
                }
                result
            }
        }
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(cohere_provider_header_entries(&self.settings)),
            optional_headers(call_headers),
        ])
    }
}

impl RerankingModel for CohereRerankingModel {
    type RerankFuture<'a>
        = Pin<Box<dyn Future<Output = RerankingModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        CohereRerankingModel::provider(self)
    }

    fn model_id(&self) -> &str {
        CohereRerankingModel::model_id(self)
    }

    fn do_rerank(&self, options: RerankingModelCallOptions) -> Self::RerankFuture<'_> {
        Box::pin(self.do_rerank_result(options))
    }
}

/// Creates a Cohere provider with explicit settings.
pub fn create_cohere(settings: CohereProviderSettings) -> CohereProvider {
    CohereProvider::from_settings(settings)
}

/// Creates a Cohere chat model with default provider settings.
pub fn cohere(model_id: impl Into<String>) -> CohereChatLanguageModel {
    CohereProvider::new().language_model(model_id)
}

fn cohere_base_url(settings: &CohereProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_COHERE_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn cohere_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("COHERE_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn cohere_provider_headers(settings: &CohereProviderSettings) -> Headers {
    let mut headers = Headers::new();

    if let Some(api_key) = cohere_api_key(settings.api_key.as_ref()) {
        headers.insert("authorization".to_string(), format!("Bearer {api_key}"));
    }

    for (name, value) in &settings.headers {
        headers.insert(name.clone(), value.clone());
    }

    with_user_agent_suffix(
        Some(
            headers
                .into_iter()
                .map(|(name, value)| (name, Some(value)))
                .collect::<Vec<_>>(),
        ),
        [format!("ai-sdk/cohere/{}", crate::VERSION)],
    )
}

fn cohere_provider_header_entries(
    settings: &CohereProviderSettings,
) -> Vec<(String, Option<String>)> {
    cohere_provider_headers(settings)
        .into_iter()
        .map(|(name, value)| (name, Some(value)))
        .collect()
}

fn optional_headers(headers: Option<&Headers>) -> Option<Vec<(String, Option<String>)>> {
    headers.map(|headers| {
        headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone())))
            .collect()
    })
}

#[derive(Clone, Debug)]
struct CoherePromptParts {
    messages: Vec<JsonValue>,
    documents: Vec<JsonValue>,
    warnings: Vec<Warning>,
}

fn cohere_chat_request_body(
    model_id: &str,
    options: &LanguageModelCallOptions,
    stream: bool,
) -> Result<(JsonValue, Vec<Warning>), String> {
    let mut body = JsonObject::new();
    let mut warnings = Vec::new();
    let provider_options = cohere_language_provider_options(options.provider_options.as_ref());
    let prompt = convert_to_cohere_chat_prompt(&options.prompt)?;
    let (tools, tool_choice, tool_warnings) =
        cohere_prepare_tools(options.tools.as_ref(), options.tool_choice.as_ref());

    warnings.extend(tool_warnings);
    warnings.extend(prompt.warnings.clone());

    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));

    if let Some(value) = options.frequency_penalty {
        body.insert("frequency_penalty".to_string(), json!(value));
    }
    if let Some(value) = options.presence_penalty {
        body.insert("presence_penalty".to_string(), json!(value));
    }
    if let Some(value) = options.max_output_tokens {
        body.insert("max_tokens".to_string(), json!(value));
    }
    if let Some(value) = options.temperature {
        body.insert("temperature".to_string(), json!(value));
    }
    if let Some(value) = options.top_p {
        body.insert("p".to_string(), json!(value));
    }
    if let Some(value) = options.top_k {
        body.insert("k".to_string(), json!(value));
    }
    if let Some(value) = options.seed {
        body.insert("seed".to_string(), json!(value));
    }
    if let Some(stop_sequences) = &options.stop_sequences {
        body.insert("stop_sequences".to_string(), json!(stop_sequences));
    }

    if let Some(response_format) = &options.response_format
        && let Some(value) = cohere_response_format(response_format)
    {
        body.insert("response_format".to_string(), value);
    }

    body.insert("messages".to_string(), JsonValue::Array(prompt.messages));

    if let Some(tools) = tools {
        body.insert("tools".to_string(), tools);
    }
    if let Some(tool_choice) = tool_choice {
        body.insert("tool_choice".to_string(), tool_choice);
    }
    if !prompt.documents.is_empty() {
        body.insert("documents".to_string(), JsonValue::Array(prompt.documents));
    }
    if let Some(thinking) =
        resolve_cohere_thinking(options.reasoning.as_ref(), provider_options, &mut warnings)
    {
        body.insert("thinking".to_string(), thinking);
    }
    if stream {
        body.insert("stream".to_string(), JsonValue::Bool(true));
    }

    Ok((JsonValue::Object(body), warnings))
}

fn cohere_language_provider_options(
    provider_options: Option<&ProviderOptions>,
) -> Option<&JsonObject> {
    provider_options.and_then(|options| options.get(COHERE_PROVIDER_OPTIONS_NAME))
}

fn cohere_response_format(response_format: &LanguageModelResponseFormat) -> Option<JsonValue> {
    match response_format {
        LanguageModelResponseFormat::Json { schema, .. } => {
            let mut value = JsonObject::new();
            value.insert(
                "type".to_string(),
                JsonValue::String("json_object".to_string()),
            );
            if let Some(schema) = schema {
                value.insert("json_schema".to_string(), JsonValue::Object(schema.clone()));
            }
            Some(JsonValue::Object(value))
        }
        LanguageModelResponseFormat::Text => None,
    }
}

fn resolve_cohere_thinking(
    reasoning: Option<&LanguageModelReasoningEffort>,
    cohere_options: Option<&JsonObject>,
    warnings: &mut Vec<Warning>,
) -> Option<JsonValue> {
    if let Some(thinking) = cohere_options.and_then(|options| options.get("thinking")) {
        let mut value = JsonObject::new();
        let thinking_type = thinking
            .get("type")
            .and_then(JsonValue::as_str)
            .unwrap_or("enabled");
        value.insert(
            "type".to_string(),
            JsonValue::String(thinking_type.to_string()),
        );
        if let Some(token_budget) = thinking
            .get("tokenBudget")
            .or_else(|| thinking.get("token_budget"))
            .cloned()
        {
            value.insert("token_budget".to_string(), token_budget);
        }
        return Some(JsonValue::Object(value));
    }

    match reasoning {
        None | Some(LanguageModelReasoningEffort::ProviderDefault) => None,
        Some(LanguageModelReasoningEffort::None) => Some(json!({ "type": "disabled" })),
        Some(reasoning) => {
            let Ok(level) = ReasoningLevel::try_from(reasoning.clone()) else {
                return None;
            };
            map_reasoning_to_provider_budget(level, 32768, 32768, None, None, warnings)
                .map(|token_budget| json!({ "type": "enabled", "token_budget": token_budget }))
        }
    }
}

fn cohere_prepare_tools(
    tools: Option<&Vec<LanguageModelTool>>,
    tool_choice: Option<&LanguageModelToolChoice>,
) -> (Option<JsonValue>, Option<JsonValue>, Vec<Warning>) {
    let Some(tools) = tools.filter(|tools| !tools.is_empty()) else {
        return (None, None, Vec::new());
    };

    let mut warnings = Vec::new();
    let mut cohere_tools = Vec::new();

    for tool in tools {
        match tool {
            LanguageModelTool::Function(function_tool) => {
                let mut function = JsonObject::new();
                function.insert(
                    "name".to_string(),
                    JsonValue::String(function_tool.name.clone()),
                );
                if let Some(description) = &function_tool.description {
                    function.insert(
                        "description".to_string(),
                        JsonValue::String(description.clone()),
                    );
                }
                function.insert(
                    "parameters".to_string(),
                    JsonValue::Object(function_tool.input_schema.clone()),
                );
                cohere_tools.push(json!({
                    "type": "function",
                    "function": function
                }));
            }
            LanguageModelTool::Provider(provider_tool) => {
                warnings.push(Warning::Unsupported {
                    feature: format!("provider-defined tool {}", provider_tool.id),
                    details: None,
                });
            }
        }
    }

    let cohere_tool_choice = match tool_choice {
        None | Some(LanguageModelToolChoice::Auto) => None,
        Some(LanguageModelToolChoice::None) => Some(JsonValue::String("NONE".to_string())),
        Some(LanguageModelToolChoice::Required) => Some(JsonValue::String("REQUIRED".to_string())),
        Some(LanguageModelToolChoice::Tool { tool_name }) => {
            cohere_tools.retain(|tool| {
                tool.get("function")
                    .and_then(|function| function.get("name"))
                    .and_then(JsonValue::as_str)
                    == Some(tool_name.as_str())
            });
            Some(JsonValue::String("REQUIRED".to_string()))
        }
    };

    (
        Some(JsonValue::Array(cohere_tools)),
        cohere_tool_choice,
        warnings,
    )
}

fn convert_to_cohere_chat_prompt(
    prompt: &[LanguageModelMessage],
) -> Result<CoherePromptParts, String> {
    let mut messages = Vec::new();
    let mut documents = Vec::new();
    let warnings = Vec::new();

    for message in prompt {
        match message {
            LanguageModelMessage::System(message) => {
                messages.push(json!({ "role": "system", "content": message.content.clone() }));
            }
            LanguageModelMessage::User(message) => {
                let mut user_parts = Vec::new();
                let mut has_image = false;

                for part in &message.content {
                    match part {
                        LanguageModelUserContentPart::Text(text) if !text.text.is_empty() => {
                            user_parts.push(json!({ "type": "text", "text": text.text.clone() }));
                        }
                        LanguageModelUserContentPart::Text(_) => {}
                        LanguageModelUserContentPart::File(file) => {
                            if get_top_level_media_type(&file.media_type) == "image" {
                                has_image = true;
                                let mut image_url = JsonObject::new();
                                image_url.insert(
                                    "url".to_string(),
                                    JsonValue::String(cohere_image_part_url(file)?),
                                );
                                if let Some(detail) = file
                                    .provider_options
                                    .as_ref()
                                    .and_then(|options| options.get(COHERE_PROVIDER_OPTIONS_NAME))
                                    .and_then(|options| options.get("detail"))
                                    .and_then(JsonValue::as_str)
                                {
                                    image_url.insert(
                                        "detail".to_string(),
                                        JsonValue::String(detail.to_string()),
                                    );
                                }
                                user_parts.push(json!({
                                    "type": "image_url",
                                    "image_url": image_url
                                }));
                            } else {
                                let text_content = cohere_document_text(file)?;
                                let mut data = JsonObject::new();
                                data.insert("text".to_string(), JsonValue::String(text_content));
                                if let Some(filename) = &file.filename {
                                    data.insert(
                                        "title".to_string(),
                                        JsonValue::String(filename.clone()),
                                    );
                                }
                                documents.push(json!({ "data": data }));
                            }
                        }
                    }
                }

                if has_image {
                    messages.push(json!({ "role": "user", "content": user_parts }));
                } else {
                    let text = user_parts
                        .iter()
                        .filter_map(|part| part.get("text").and_then(JsonValue::as_str))
                        .collect::<String>();
                    messages.push(json!({ "role": "user", "content": text }));
                }
            }
            LanguageModelMessage::Assistant(message) => {
                let mut text = String::new();
                let mut tool_calls = Vec::new();

                for part in &message.content {
                    match part {
                        LanguageModelAssistantContentPart::Text(text_part) => {
                            text.push_str(&text_part.text);
                        }
                        LanguageModelAssistantContentPart::ToolCall(tool_call) => {
                            tool_calls.push(json!({
                                "id": tool_call.tool_call_id.clone(),
                                "type": "function",
                                "function": {
                                    "name": tool_call.tool_name.clone(),
                                    "arguments": tool_call.input.to_string()
                                }
                            }));
                        }
                        _ => {}
                    }
                }

                let mut cohere_message = JsonObject::new();
                cohere_message.insert(
                    "role".to_string(),
                    JsonValue::String("assistant".to_string()),
                );
                if tool_calls.is_empty() {
                    cohere_message.insert("content".to_string(), JsonValue::String(text));
                } else {
                    cohere_message.insert("tool_calls".to_string(), JsonValue::Array(tool_calls));
                }
                messages.push(JsonValue::Object(cohere_message));
            }
            LanguageModelMessage::Tool(message) => {
                for part in &message.content {
                    if let crate::language_model::LanguageModelToolContentPart::ToolResult(
                        tool_result,
                    ) = part
                    {
                        messages.push(json!({
                            "role": "tool",
                            "content": cohere_tool_result_content(&tool_result.output),
                            "tool_call_id": tool_result.tool_call_id.clone()
                        }));
                    }
                }
            }
        }
    }

    Ok(CoherePromptParts {
        messages,
        documents,
        warnings,
    })
}

fn cohere_image_part_url(
    file: &crate::language_model::LanguageModelFilePart,
) -> Result<String, String> {
    match &file.data {
        FileData::Url { url } => Ok(url.to_string()),
        FileData::Data { data } => {
            let media_type = resolve_full_media_type(file).map_err(|error| error.to_string())?;
            Ok(format!(
                "data:{media_type};base64,{}",
                convert_to_base64(data)
            ))
        }
        FileData::Reference { .. } => Err(
            "'image file parts with provider references' functionality not supported.".to_string(),
        ),
        FileData::Text { .. } => {
            Err("'image file parts with text data' functionality not supported.".to_string())
        }
    }
}

fn cohere_document_text(
    file: &crate::language_model::LanguageModelFilePart,
) -> Result<String, String> {
    match &file.data {
        FileData::Reference { .. } => {
            Err("'file parts with provider references' functionality not supported.".to_string())
        }
        FileData::Url { .. } => Err("URLs should be downloaded by the AI SDK and not reach this point. This indicates a configuration issue.".to_string()),
        FileData::Text { text } => Ok(text.clone()),
        FileData::Data { data } => match data {
            crate::file_data::FileDataContent::Bytes(bytes) => {
                Ok(String::from_utf8_lossy(bytes).to_string())
            }
            crate::file_data::FileDataContent::Base64(base64) => Ok(base64.clone()),
        },
    }
}

fn cohere_tool_result_content(output: &LanguageModelToolResultOutput) -> String {
    match output {
        LanguageModelToolResultOutput::Text { value, .. }
        | LanguageModelToolResultOutput::ErrorText { value, .. } => value.clone(),
        LanguageModelToolResultOutput::ExecutionDenied { reason, .. } => reason
            .clone()
            .unwrap_or_else(|| "Tool call execution denied.".to_string()),
        LanguageModelToolResultOutput::Json { value, .. }
        | LanguageModelToolResultOutput::ErrorJson { value, .. } => value.to_string(),
        LanguageModelToolResultOutput::Content { value } => {
            serde_json::to_string(value).unwrap_or_else(|_| "[]".to_string())
        }
    }
}

fn cohere_embedding_request_body(model_id: &str, options: &EmbeddingModelCallOptions) -> JsonValue {
    let mut body = JsonObject::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    body.insert("embedding_types".to_string(), json!(["float"]));
    body.insert("texts".to_string(), json!(options.values));
    body.insert(
        "input_type".to_string(),
        cohere_option_string(options, "inputType")
            .map(JsonValue::String)
            .unwrap_or_else(|| JsonValue::String("search_query".to_string())),
    );

    if let Some(truncate) = cohere_option_string(options, "truncate") {
        body.insert("truncate".to_string(), JsonValue::String(truncate));
    }

    if let Some(output_dimension) = cohere_option_value(options, "outputDimension") {
        body.insert("output_dimension".to_string(), output_dimension);
    }

    JsonValue::Object(body)
}

fn cohere_reranking_request_body(
    model_id: &str,
    options: &RerankingModelCallOptions,
) -> (JsonValue, Vec<Warning>) {
    let mut body = JsonObject::new();
    let mut warnings = Vec::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    body.insert(
        "query".to_string(),
        JsonValue::String(options.query.clone()),
    );
    body.insert(
        "documents".to_string(),
        cohere_reranking_documents(&options.documents, &mut warnings),
    );

    if let Some(top_n) = options.top_n {
        body.insert("top_n".to_string(), JsonValue::from(top_n));
    }

    if let Some(max_tokens_per_doc) = cohere_reranking_option_value(options, "maxTokensPerDoc") {
        body.insert("max_tokens_per_doc".to_string(), max_tokens_per_doc);
    }

    if let Some(priority) = cohere_reranking_option_value(options, "priority") {
        body.insert("priority".to_string(), priority);
    }

    (JsonValue::Object(body), warnings)
}

fn cohere_reranking_documents(
    documents: &RerankingModelDocuments,
    warnings: &mut Vec<Warning>,
) -> JsonValue {
    match documents {
        RerankingModelDocuments::Text { values } => json!(values),
        RerankingModelDocuments::Object { values } => {
            warnings.push(Warning::Compatibility {
                feature: "object documents".to_string(),
                details: Some("Object documents are converted to strings.".to_string()),
            });
            JsonValue::Array(
                values
                    .iter()
                    .map(|value| JsonValue::String(JsonValue::Object(value.clone()).to_string()))
                    .collect(),
            )
        }
    }
}

fn cohere_option_string(options: &EmbeddingModelCallOptions, name: &str) -> Option<String> {
    cohere_option_value(options, name).and_then(|value| value.as_str().map(str::to_string))
}

fn cohere_option_value(options: &EmbeddingModelCallOptions, name: &str) -> Option<JsonValue> {
    options
        .provider_options
        .as_ref()
        .and_then(|provider_options| provider_options.get("cohere"))
        .and_then(|options| options.get(name))
        .cloned()
}

fn cohere_reranking_option_value(
    options: &RerankingModelCallOptions,
    name: &str,
) -> Option<JsonValue> {
    options
        .provider_options
        .as_ref()
        .and_then(|provider_options| provider_options.get("cohere"))
        .and_then(|options| options.get(name))
        .cloned()
}

#[derive(Clone, Debug, Deserialize)]
struct CohereEmbeddingResponse {
    embeddings: CohereEmbeddingVectors,
    meta: CohereEmbeddingMeta,
}

#[derive(Clone, Debug, Deserialize)]
struct CohereEmbeddingVectors {
    float: Vec<Vec<f64>>,
}

#[derive(Clone, Debug, Deserialize)]
struct CohereEmbeddingMeta {
    billed_units: CohereEmbeddingBilledUnits,
}

#[derive(Clone, Debug, Deserialize)]
struct CohereEmbeddingBilledUnits {
    input_tokens: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct CohereRerankingResponse {
    #[serde(default)]
    id: Option<String>,
    results: Vec<CohereRerankingResult>,
}

#[derive(Clone, Debug, Deserialize)]
struct CohereRerankingResult {
    index: usize,
    relevance_score: f64,
}

fn cohere_embedding_response(
    value: &JsonValue,
) -> Result<CohereEmbeddingResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn cohere_reranking_response(
    value: &JsonValue,
) -> Result<CohereRerankingResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn cohere_error_response(value: &JsonValue) -> Result<JsonValue, serde_json::Error> {
    Ok(value.clone())
}

fn cohere_error_message(value: &JsonValue) -> String {
    value
        .get("message")
        .or_else(|| value.get("error").and_then(|error| error.get("message")))
        .and_then(JsonValue::as_str)
        .unwrap_or("Unknown error")
        .to_string()
}

fn cohere_chat_response(value: &JsonValue) -> Result<JsonValue, String> {
    let message = value
        .get("message")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "missing message object".to_string())?;
    if value
        .get("finish_reason")
        .and_then(JsonValue::as_str)
        .is_none()
    {
        return Err("missing finish_reason string".to_string());
    }
    if message
        .get("content")
        .is_some_and(|content| !content.is_null() && !content.is_array())
    {
        return Err("message.content must be an array when present".to_string());
    }
    if message
        .get("tool_calls")
        .is_some_and(|tool_calls| !tool_calls.is_null() && !tool_calls.is_array())
    {
        return Err("message.tool_calls must be an array when present".to_string());
    }
    Ok(value.clone())
}

fn cohere_chat_generate_result_from_response(
    response: JsonValue,
    raw_response: Option<JsonValue>,
    response_headers: Option<Headers>,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    generate_id: &CohereGenerateId,
) -> LanguageModelGenerateResult {
    let mut content = Vec::new();
    let message = response.get("message").unwrap_or(&JsonValue::Null);

    if let Some(items) = message.get("content").and_then(JsonValue::as_array) {
        for item in items {
            match item.get("type").and_then(JsonValue::as_str) {
                Some("text") => {
                    if let Some(text) = item.get("text").and_then(JsonValue::as_str)
                        && !text.is_empty()
                    {
                        content.push(LanguageModelContent::Text(LanguageModelText::new(text)));
                    }
                }
                Some("thinking") => {
                    if let Some(text) = item.get("thinking").and_then(JsonValue::as_str)
                        && !text.is_empty()
                    {
                        content.push(LanguageModelContent::Reasoning(
                            LanguageModelReasoning::new(text),
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(citations) = message.get("citations").and_then(JsonValue::as_array) {
        for citation in citations {
            content.push(LanguageModelContent::Source(LanguageModelSource::Document(
                cohere_document_source_from_citation(citation, generate_id),
            )));
        }
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(JsonValue::as_array) {
        for tool_call in tool_calls {
            let id = tool_call
                .get("id")
                .and_then(JsonValue::as_str)
                .unwrap_or_default();
            let function = tool_call.get("function").unwrap_or(&JsonValue::Null);
            let name = function
                .get("name")
                .and_then(JsonValue::as_str)
                .unwrap_or_default();
            let arguments = cohere_tool_call_arguments(
                function
                    .get("arguments")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default(),
            );
            content.push(LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                id, name, arguments,
            )));
        }
    }

    let finish_reason =
        cohere_finish_reason(response.get("finish_reason").and_then(JsonValue::as_str));
    let usage = cohere_usage(response.get("usage").and_then(|usage| usage.get("tokens")));
    let mut result = LanguageModelGenerateResult::new(content, finish_reason, usage)
        .with_request(LanguageModelRequest::new().with_body(request_body));
    let mut response_metadata = LanguageModelResponse::new();

    if let Some(id) = response.get("generation_id").and_then(JsonValue::as_str) {
        response_metadata = response_metadata.with_id(id);
    }
    if let Some(body) = raw_response {
        response_metadata = response_metadata.with_body(body);
    }
    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response_metadata = response_metadata.with_header(name, value);
        }
    }
    result = result.with_response(response_metadata);

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn cohere_document_source_from_citation(
    citation: &JsonValue,
    generate_id: &CohereGenerateId,
) -> LanguageModelDocumentSource {
    let title = citation
        .get("sources")
        .and_then(JsonValue::as_array)
        .and_then(|sources| sources.first())
        .and_then(|source| source.get("document"))
        .and_then(|document| document.get("title"))
        .and_then(JsonValue::as_str)
        .unwrap_or("Document");
    let mut extra = JsonObject::new();

    for key in ["start", "end", "text", "sources"] {
        if let Some(value) = citation.get(key) {
            extra.insert(key.to_string(), value.clone());
        }
    }
    if let Some(citation_type) = citation.get("type") {
        extra.insert("citationType".to_string(), citation_type.clone());
    }

    let mut metadata = ProviderMetadata::new();
    metadata.insert(COHERE_PROVIDER_OPTIONS_NAME.to_string(), extra);

    LanguageModelDocumentSource::new((generate_id)(), "text/plain", title)
        .with_provider_metadata(metadata)
}

fn cohere_tool_call_arguments(arguments: &str) -> String {
    if arguments.trim() == "null" {
        "{}".to_string()
    } else {
        arguments.to_string()
    }
}

fn cohere_finish_reason(raw: Option<&str>) -> LanguageModelFinishReason {
    let unified = match raw {
        Some("COMPLETE" | "STOP_SEQUENCE") => FinishReason::Stop,
        Some("MAX_TOKENS") => FinishReason::Length,
        Some("ERROR") => FinishReason::Error,
        Some("TOOL_CALL") => FinishReason::ToolCalls,
        _ => FinishReason::Other,
    };

    LanguageModelFinishReason {
        unified,
        raw: raw.map(ToString::to_string),
    }
}

fn cohere_usage(tokens: Option<&JsonValue>) -> LanguageModelUsage {
    let Some(tokens) = tokens.and_then(JsonValue::as_object) else {
        return LanguageModelUsage::default();
    };
    let input_tokens = tokens.get("input_tokens").and_then(JsonValue::as_u64);
    let output_tokens = tokens.get("output_tokens").and_then(JsonValue::as_u64);

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: input_tokens,
            no_cache: input_tokens,
            ..InputTokenUsage::default()
        },
        output_tokens: OutputTokenUsage {
            total: output_tokens,
            text: output_tokens,
            ..OutputTokenUsage::default()
        },
        raw: Some(tokens.clone()),
    }
}

fn cohere_chat_generate_result_from_error(
    error: HandledFetchError,
    request_body: JsonValue,
    warnings: Vec<Warning>,
) -> LanguageModelGenerateResult {
    let (message, headers, body) = cohere_error_parts(error);
    let mut response = LanguageModelResponse::new();
    if let Some(body) = body
        .as_deref()
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| body.map(JsonValue::String))
    {
        response = response.with_body(body);
    }
    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    let mut result = LanguageModelGenerateResult::new(
        Vec::new(),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("cohere-error".to_string()),
        },
        LanguageModelUsage::default(),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_response(response)
    .with_provider_metadata(cohere_error_metadata(message));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn cohere_chat_error_generate_result(
    message: impl Into<String>,
    request_body: JsonValue,
) -> LanguageModelGenerateResult {
    LanguageModelGenerateResult::new(
        Vec::new(),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("cohere-error".to_string()),
        },
        LanguageModelUsage::default(),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_provider_metadata(cohere_error_metadata(message.into()))
}

#[derive(Clone, Debug, Default)]
struct CoherePendingToolCall {
    id: String,
    name: String,
    arguments: String,
}

fn cohere_chat_stream_result_from_response(
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
    let mut usage = LanguageModelUsage::default();
    let mut pending_tool_call = None::<CoherePendingToolCall>;
    let mut is_active_reasoning = false;

    for event in events {
        match event {
            ParseJsonResult::Success { value, raw_value } => {
                if include_raw_chunks {
                    stream.push(LanguageModelStreamPart::Raw(
                        LanguageModelRawStreamPart::new(raw_value.clone()),
                    ));
                }

                match value.get("type").and_then(JsonValue::as_str) {
                    Some("content-start") => {
                        let index = cohere_stream_index(&value);
                        let content_type =
                            cohere_stream_nested(&value, &["delta", "message", "content", "type"])
                                .and_then(JsonValue::as_str);
                        if matches!(content_type, Some("thinking")) {
                            is_active_reasoning = true;
                            stream.push(LanguageModelStreamPart::ReasoningStart(
                                LanguageModelReasoningStart::new(index),
                            ));
                        } else {
                            stream.push(LanguageModelStreamPart::TextStart(
                                LanguageModelTextStart::new(index),
                            ));
                        }
                    }
                    Some("content-delta") => {
                        let index = cohere_stream_index(&value);
                        if let Some(delta) = cohere_stream_nested(
                            &value,
                            &["delta", "message", "content", "thinking"],
                        )
                        .and_then(JsonValue::as_str)
                        {
                            stream.push(LanguageModelStreamPart::ReasoningDelta(
                                LanguageModelReasoningDelta::new(index, delta),
                            ));
                        } else if let Some(delta) =
                            cohere_stream_nested(&value, &["delta", "message", "content", "text"])
                                .and_then(JsonValue::as_str)
                        {
                            stream.push(LanguageModelStreamPart::TextDelta(
                                LanguageModelTextDelta::new(index, delta),
                            ));
                        }
                    }
                    Some("content-end") => {
                        let index = cohere_stream_index(&value);
                        if is_active_reasoning {
                            is_active_reasoning = false;
                            stream.push(LanguageModelStreamPart::ReasoningEnd(
                                LanguageModelReasoningEnd::new(index),
                            ));
                        } else {
                            stream.push(LanguageModelStreamPart::TextEnd(
                                LanguageModelTextEnd::new(index),
                            ));
                        }
                    }
                    Some("tool-call-start") => {
                        let tool_id =
                            cohere_stream_nested(&value, &["delta", "message", "tool_calls", "id"])
                                .and_then(JsonValue::as_str)
                                .unwrap_or_default();
                        let tool_name = cohere_stream_nested(
                            &value,
                            &["delta", "message", "tool_calls", "function", "name"],
                        )
                        .and_then(JsonValue::as_str)
                        .unwrap_or_default();
                        let initial_args = cohere_stream_nested(
                            &value,
                            &["delta", "message", "tool_calls", "function", "arguments"],
                        )
                        .and_then(JsonValue::as_str)
                        .unwrap_or_default();
                        pending_tool_call = Some(CoherePendingToolCall {
                            id: tool_id.to_string(),
                            name: tool_name.to_string(),
                            arguments: initial_args.to_string(),
                        });
                        stream.push(LanguageModelStreamPart::ToolInputStart(
                            LanguageModelToolInputStart::new(tool_id, tool_name),
                        ));
                        if !initial_args.is_empty() {
                            stream.push(LanguageModelStreamPart::ToolInputDelta(
                                LanguageModelToolInputDelta::new(tool_id, initial_args),
                            ));
                        }
                    }
                    Some("tool-call-delta") => {
                        if let Some(pending) = &mut pending_tool_call {
                            let delta = cohere_stream_nested(
                                &value,
                                &["delta", "message", "tool_calls", "function", "arguments"],
                            )
                            .and_then(JsonValue::as_str)
                            .unwrap_or_default();
                            pending.arguments.push_str(delta);
                            stream.push(LanguageModelStreamPart::ToolInputDelta(
                                LanguageModelToolInputDelta::new(pending.id.clone(), delta),
                            ));
                        }
                    }
                    Some("tool-call-end") => {
                        if let Some(pending) = pending_tool_call.take() {
                            stream.push(LanguageModelStreamPart::ToolInputEnd(
                                LanguageModelToolInputEnd::new(pending.id.clone()),
                            ));
                            match cohere_stream_tool_input(&pending.arguments) {
                                Ok(input) => {
                                    stream.push(LanguageModelStreamPart::ToolCall(
                                        LanguageModelToolCall::new(pending.id, pending.name, input),
                                    ));
                                }
                                Err(error) => {
                                    finish_reason = LanguageModelFinishReason {
                                        unified: FinishReason::Error,
                                        raw: Some("cohere-stream-error".to_string()),
                                    };
                                    stream.push(LanguageModelStreamPart::Error(
                                        LanguageModelErrorStreamPart::new(json!({
                                            "message": error
                                        })),
                                    ));
                                }
                            }
                        }
                    }
                    Some("message-start") => {
                        let mut metadata = LanguageModelStreamResponseMetadata::new();
                        if let Some(id) = value.get("id").and_then(JsonValue::as_str) {
                            metadata = metadata.with_id(id);
                        }
                        stream.push(LanguageModelStreamPart::ResponseMetadata(metadata));
                    }
                    Some("message-end") => {
                        let raw_finish_reason =
                            cohere_stream_nested(&value, &["delta", "finish_reason"])
                                .and_then(JsonValue::as_str);
                        finish_reason = cohere_finish_reason(raw_finish_reason);
                        usage = cohere_usage(cohere_stream_nested(
                            &value,
                            &["delta", "usage", "tokens"],
                        ));
                    }
                    _ => {}
                }
            }
            ParseJsonResult::Failure { error, .. } => {
                finish_reason = LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: Some("cohere-stream-error".to_string()),
                };
                stream.push(LanguageModelStreamPart::Error(
                    LanguageModelErrorStreamPart::new(json!({ "message": error.to_string() })),
                ));
            }
        }
    }

    stream.push(LanguageModelStreamPart::Finish(
        LanguageModelStreamFinish::new(usage, finish_reason),
    ));

    let mut result = LanguageModelStreamResult::new(stream)
        .with_request(LanguageModelRequest::new().with_body(request_body));
    if let Some(headers) = response_headers {
        let mut response = LanguageModelStreamResultResponse::new();
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
        result = result.with_response(response);
    }
    result
}

fn cohere_stream_index(value: &JsonValue) -> String {
    value
        .get("index")
        .and_then(JsonValue::as_u64)
        .map(|index| index.to_string())
        .unwrap_or_else(|| "0".to_string())
}

fn cohere_stream_nested<'a>(value: &'a JsonValue, path: &[&str]) -> Option<&'a JsonValue> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
}

fn cohere_stream_tool_input(arguments: &str) -> Result<String, String> {
    let text = arguments.trim();
    let text = if text.is_empty() { "{}" } else { text };
    match safe_parse_json(text) {
        ParseJsonResult::Success { value, .. } => Ok(value.to_string()),
        ParseJsonResult::Failure { error, .. } => Err(error.to_string()),
    }
}

fn cohere_chat_stream_result_from_error(
    error: HandledFetchError,
    request_body: JsonValue,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let (message, _headers, _body) = cohere_error_parts(error);
    cohere_chat_error_stream_result(message, request_body)
}

fn cohere_chat_error_stream_result(
    message: impl Into<String>,
    request_body: JsonValue,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    LanguageModelStreamResult::new(vec![LanguageModelStreamPart::Error(
        LanguageModelErrorStreamPart::new(json!({ "message": message.into() })),
    )])
    .with_request(LanguageModelRequest::new().with_body(request_body))
}

fn cohere_embedding_result_from_response(
    response: CohereEmbeddingResponse,
    raw_response: Option<JsonValue>,
    response_headers: Option<Headers>,
) -> EmbeddingModelResult {
    let mut result = EmbeddingModelResult::new(response.embeddings.float).with_usage(
        EmbeddingModelUsage::new(response.meta.billed_units.input_tokens),
    );
    let mut response_metadata = EmbeddingModelResponse::new();

    if let Some(body) = raw_response {
        response_metadata = response_metadata.with_body(body);
    }

    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response_metadata = response_metadata.with_header(name, value);
        }
    }

    result = result.with_response(response_metadata);
    result
}

fn cohere_embedding_result_from_error(
    error: HandledFetchError,
    request_body: Option<JsonValue>,
) -> EmbeddingModelResult {
    let (message, headers, body) = cohere_error_parts(error);
    let response_body = body
        .as_deref()
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| body.map(JsonValue::String))
        .or(request_body);
    let mut response = EmbeddingModelResponse::new();

    if let Some(body) = response_body {
        response = response.with_body(body);
    }

    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    EmbeddingModelResult::new(Vec::new())
        .with_provider_metadata(cohere_error_metadata(message))
        .with_response(response)
}

fn cohere_embedding_error_result(
    message: impl Into<String>,
    request_body: Option<JsonValue>,
) -> EmbeddingModelResult {
    let mut response = EmbeddingModelResponse::new();
    if let Some(body) = request_body {
        response = response.with_body(body);
    }

    EmbeddingModelResult::new(Vec::new())
        .with_provider_metadata(cohere_error_metadata(message.into()))
        .with_response(response)
}

fn cohere_reranking_result_from_response(
    response: CohereRerankingResponse,
    raw_response: Option<JsonValue>,
    response_headers: Option<Headers>,
) -> RerankingModelResult {
    let ranking = response
        .results
        .into_iter()
        .map(|result| RerankingModelRanking::new(result.index, result.relevance_score))
        .collect();
    let mut result = RerankingModelResult::new(ranking);
    let mut response_metadata = RerankingModelResponse::new();

    if let Some(id) = response.id {
        response_metadata = response_metadata.with_id(id);
    }

    if let Some(body) = raw_response {
        response_metadata = response_metadata.with_body(body);
    }

    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response_metadata = response_metadata.with_header(name, value);
        }
    }

    result = result.with_response(response_metadata);
    result
}

fn cohere_reranking_result_from_error(
    error: HandledFetchError,
    request_body: Option<JsonValue>,
) -> RerankingModelResult {
    let (message, headers, body) = cohere_error_parts(error);
    let response_body = body
        .as_deref()
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| body.map(JsonValue::String))
        .or(request_body);
    let mut response = RerankingModelResponse::new();

    if let Some(body) = response_body {
        response = response.with_body(body);
    }

    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    RerankingModelResult::new(Vec::new())
        .with_provider_metadata(cohere_error_metadata(message))
        .with_response(response)
}

fn cohere_error_parts(error: HandledFetchError) -> (String, Option<Headers>, Option<String>) {
    match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None, None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
            error.response_body().map(String::from),
        ),
    }
}

fn cohere_error_metadata(message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut extra = JsonObject::new();
    extra.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert("cohere".to_string(), extra);
    metadata
}

fn default_cohere_transport() -> OpenAICompatibleTransport {
    Arc::new(|request| Box::pin(ready(execute_cohere_request(request))))
}

fn execute_cohere_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_cohere_get_request(request),
        ProviderApiRequestMethod::Post => execute_cohere_post_request(request),
    }
}

fn execute_cohere_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    cohere_provider_api_response(response)
}

fn execute_cohere_post_request(
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
                "multipart form data is not supported by the Cohere transport",
            ));
        }
        None => builder.send_empty(),
    };

    cohere_provider_api_response(response)
}

fn cohere_provider_api_response(
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
        CohereProvider, CohereProviderSettings, DEFAULT_COHERE_BASE_URL, cohere,
        cohere_chat_request_body, cohere_finish_reason, cohere_prepare_tools,
        convert_to_cohere_chat_prompt, create_cohere,
    };
    use crate::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions};
    use crate::file_data::{FileData, FileDataContent, ProviderReference};
    use crate::headers::Headers;
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelAssistantContentPart,
        LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelFilePart, LanguageModelFunctionTool, LanguageModelMessage,
        LanguageModelProviderTool, LanguageModelReasoningEffort, LanguageModelResponseFormat,
        LanguageModelSource, LanguageModelStreamPart, LanguageModelSystemMessage,
        LanguageModelTextPart, LanguageModelTool, LanguageModelToolCallPart,
        LanguageModelToolChoice, LanguageModelToolContentPart, LanguageModelToolMessage,
        LanguageModelToolResultOutput, LanguageModelToolResultPart, LanguageModelUserContentPart,
        LanguageModelUserMessage,
    };
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::provider::{ModelType, Provider, ProviderOptions, ProviderWithRerankingModel};
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use crate::reranking_model::{
        RerankingModel, RerankingModelCallOptions, RerankingModelDocuments,
    };
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::env;
    use std::future::{Future, ready};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use url::Url;

    struct NoopWake;

    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    fn test_waker() -> Waker {
        Waker::from(Arc::new(NoopWake))
    }

    fn poll_ready<F>(future: F) -> F::Output
    where
        F: Future,
    {
        let waker = test_waker();
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures use ready transports"),
        }
    }

    fn captured_transport(
        captured_request: Arc<Mutex<Option<ProviderApiRequest>>>,
        response: ProviderApiResponse,
    ) -> OpenAICompatibleTransport {
        Arc::new(move |request| -> OpenAICompatibleTransportFuture {
            *captured_request
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());
            Box::pin(ready(Ok(response.clone())))
        })
    }

    fn schema(properties: JsonValue) -> JsonObject {
        json!({
            "type": "object",
            "properties": properties
        })
        .as_object()
        .cloned()
        .expect("schema object")
    }

    fn cohere_success_response() -> ProviderApiResponse {
        ProviderApiResponse::text(
            200,
            "OK",
            json!({
                "generation_id": "gen-ok",
                "message": {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "ok" }
                    ]
                },
                "finish_reason": "COMPLETE",
                "usage": {
                    "tokens": {
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                }
            })
            .to_string(),
        )
    }

    fn sse(value: JsonValue) -> String {
        format!("data: {value}\n\n")
    }

    #[test]
    fn cohere_provider_creates_embedding_model_with_options_headers_and_usage() {
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
                        "embeddings": {
                            "float": [
                                [0.1, 0.2],
                                [0.3, 0.4]
                            ]
                        },
                        "meta": {
                            "billed_units": {
                                "input_tokens": 7
                            }
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_cohere_embedding".to_string(),
                )])))))
            });
        let provider = create_cohere(
            CohereProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.cohere.test/v2/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "cohere": {
                "inputType": "classification",
                "truncate": "END",
                "outputDimension": 512
            }
        }))
        .expect("provider options deserialize");
        let model = provider.embedding_model("embed-v4.0");
        let result = poll_ready(
            model.do_embed(
                EmbeddingModelCallOptions::new(vec!["sunny".to_string(), "rainy".to_string()])
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(model.provider(), "cohere.textEmbedding");
        assert_eq!(result.embeddings, vec![vec![0.1, 0.2], vec![0.3, 0.4]]);
        assert_eq!(result.usage.as_ref().map(|usage| usage.tokens), Some(7));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.cohere.test/v2/embed");
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
                .iter()
                .any(|(name, value)| name.eq_ignore_ascii_case("user-agent")
                    && value.contains("ai-sdk/cohere/0.1.0")),
            "headers: {:?}",
            request.headers
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "embed-v4.0",
                "embedding_types": ["float"],
                "texts": ["sunny", "rainy"],
                "input_type": "classification",
                "truncate": "END",
                "output_dimension": 512
            }))
        );
    }

    #[test]
    fn cohere_embedding_model_uses_default_input_type_and_exposes_raw_response() {
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
                        "embeddings": {
                            "float": [[0.1, 0.2]]
                        },
                        "meta": {
                            "billed_units": {
                                "input_tokens": 2
                            }
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_cohere_embedding_default".to_string(),
                )])))))
            });
        let model = CohereProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.cohere.test/v2/")
            .with_transport(transport)
            .embedding_model("embed-v4.0");
        let result =
            poll_ready(model.do_embed(EmbeddingModelCallOptions::new(vec!["sunny".to_string()])));

        assert_eq!(result.embeddings, vec![vec![0.1, 0.2]]);
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_cohere_embedding_default")
        );
        assert!(
            result
                .response
                .as_ref()
                .and_then(|response| response.body.as_ref())
                .and_then(|body| body.get("meta"))
                .is_some()
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "embed-v4.0",
                "embedding_types": ["float"],
                "texts": ["sunny"],
                "input_type": "search_query"
            }))
        );
    }

    #[test]
    fn cohere_provider_creates_reranking_model_with_object_warning_and_options() {
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
                        "id": "rerank-cohere",
                        "results": [
                            { "index": 1, "relevance_score": 0.91 },
                            { "index": 0, "relevance_score": 0.82 }
                        ]
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_cohere_rerank".to_string(),
                )])))))
            });
        let provider = CohereProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.cohere.test/v2/")
            .with_transport(transport);
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "cohere": {
                "maxTokensPerDoc": 256,
                "priority": 1
            }
        }))
        .expect("provider options deserialize");
        let first = json!({ "title": "A", "body": "first" })
            .as_object()
            .cloned()
            .expect("object");
        let second = json!({ "title": "B", "body": "second" })
            .as_object()
            .cloned()
            .expect("object");
        let model = provider.reranking_model("rerank-v3.5");
        let result = poll_ready(
            model.do_rerank(
                RerankingModelCallOptions::new(
                    RerankingModelDocuments::Object {
                        values: vec![first, second],
                    },
                    "query",
                )
                .with_top_n(2)
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(model.provider(), "cohere.reranking");
        assert_eq!(result.ranking[0].index, 1);
        assert!(result.warnings.iter().any(|warning| {
            matches!(warning, crate::warning::Warning::Compatibility { feature, .. } if feature == "object documents")
        }));
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("rerank-cohere")
        );
        assert!(result.provider_metadata.is_none());
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_cohere_rerank")
        );
        assert!(
            result
                .response
                .as_ref()
                .and_then(|response| response.body.as_ref())
                .and_then(|body| body.get("results"))
                .is_some()
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.cohere.test/v2/rerank");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "rerank-v3.5",
                "query": "query",
                "documents": [
                    JsonValue::Object(JsonObject::from_iter([
                        ("body".to_string(), JsonValue::String("first".to_string())),
                        ("title".to_string(), JsonValue::String("A".to_string())),
                    ])).to_string(),
                    JsonValue::Object(JsonObject::from_iter([
                        ("body".to_string(), JsonValue::String("second".to_string())),
                        ("title".to_string(), JsonValue::String("B".to_string())),
                    ])).to_string()
                ],
                "top_n": 2,
                "max_tokens_per_doc": 256,
                "priority": 1
            }))
        );
    }

    #[test]
    fn cohere_reranking_model_sends_text_documents_without_warnings() {
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
                        "id": "rerank-cohere-text",
                        "results": [
                            { "index": 1, "relevance_score": 0.91 },
                            { "index": 0, "relevance_score": 0.82 }
                        ]
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_cohere_rerank_text".to_string(),
                )])))))
            });
        let model = CohereProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.cohere.test/v2/")
            .with_transport(transport)
            .reranking_model("rerank-v3.5");
        let result = poll_ready(
            model.do_rerank(
                RerankingModelCallOptions::new(
                    RerankingModelDocuments::Text {
                        values: vec!["sunny day".to_string(), "rainy city".to_string()],
                    },
                    "rainy",
                )
                .with_top_n(2),
            ),
        );

        assert!(result.warnings.is_empty());
        assert!(result.provider_metadata.is_none());
        assert_eq!(result.ranking[0].index, 1);
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_cohere_rerank_text")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "rerank-v3.5",
                "query": "rainy",
                "documents": ["sunny day", "rainy city"],
                "top_n": 2
            }))
        );
    }

    #[test]
    fn cohere_chat_model_generates_text_reasoning_tool_calls_citations_usage_finish_and_raw_response()
     {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let transport = captured_transport(
            Arc::clone(&captured_request),
            ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "generation_id": "generation-123",
                    "message": {
                        "role": "assistant",
                        "content": [
                            { "type": "thinking", "thinking": "I should answer directly." },
                            { "type": "text", "text": "The capital of France is Paris." }
                        ],
                        "citations": [
                            {
                                "start": 0,
                                "end": 5,
                                "text": "Paris",
                                "type": "TEXT_CONTENT",
                                "sources": [
                                    {
                                        "type": "document",
                                        "id": "doc-1",
                                        "document": {
                                            "id": "doc-1",
                                            "text": "Paris is the capital.",
                                            "title": "France facts"
                                        }
                                    }
                                ]
                            }
                        ],
                        "tool_calls": [
                            {
                                "id": "weather-call",
                                "type": "function",
                                "function": {
                                    "name": "weather",
                                    "arguments": "{\"city\":\"Paris\"}"
                                }
                            },
                            {
                                "id": "time-call",
                                "type": "function",
                                "function": {
                                    "name": "currentTime",
                                    "arguments": "null"
                                }
                            }
                        ]
                    },
                    "finish_reason": "TOOL_CALL",
                    "usage": {
                        "tokens": {
                            "input_tokens": 507,
                            "output_tokens": 10
                        }
                    }
                })
                .to_string(),
            )
            .with_headers(Headers::from([(
                "x-request-id".to_string(),
                "req-chat".to_string(),
            )])),
        );
        let model = CohereProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.cohere.test/v2/")
            .with_generate_id(|| "source-id".to_string())
            .with_transport(transport)
            .language_model("command-r");
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Capital?")),
            ])),
        ])));

        assert_eq!(model.provider(), "cohere.chat");
        assert_eq!(result.finish_reason.unified, FinishReason::ToolCalls);
        assert_eq!(result.finish_reason.raw.as_deref(), Some("TOOL_CALL"));
        assert_eq!(result.usage.input_tokens.total, Some(507));
        assert_eq!(result.usage.input_tokens.no_cache, Some(507));
        assert_eq!(result.usage.output_tokens.total, Some(10));
        assert_eq!(result.usage.output_tokens.text, Some(10));
        assert!(matches!(
            &result.content[0],
            LanguageModelContent::Reasoning(reasoning)
                if reasoning.text == "I should answer directly."
        ));
        assert!(matches!(
            &result.content[1],
            LanguageModelContent::Text(text) if text.text == "The capital of France is Paris."
        ));
        assert!(matches!(
            &result.content[2],
            LanguageModelContent::Source(LanguageModelSource::Document(source))
                if source.id == "source-id"
                    && source.title == "France facts"
                    && source.media_type == "text/plain"
                    && source
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("cohere"))
                        .and_then(|cohere| cohere.get("citationType"))
                        .and_then(JsonValue::as_str)
                        == Some("TEXT_CONTENT")
        ));
        assert!(matches!(
            &result.content[3],
            LanguageModelContent::ToolCall(tool_call)
                if tool_call.tool_call_id == "weather-call"
                    && tool_call.tool_name == "weather"
                    && tool_call.input == "{\"city\":\"Paris\"}"
        ));
        assert!(matches!(
            &result.content[4],
            LanguageModelContent::ToolCall(tool_call)
                if tool_call.tool_call_id == "time-call"
                    && tool_call.tool_name == "currentTime"
                    && tool_call.input == "{}"
        ));
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("generation-123")
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req-chat")
        );
        assert!(
            result
                .request
                .as_ref()
                .and_then(|request| request.body.as_ref())
                .and_then(|body| body.get("messages"))
                .is_some()
        );
        assert_eq!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .as_ref()
                .expect("request captured")
                .url,
            "https://api.cohere.test/v2/chat"
        );
    }

    #[test]
    fn cohere_chat_model_maps_finish_reason_edges() {
        assert_eq!(
            cohere_finish_reason(Some("COMPLETE")).unified,
            FinishReason::Stop
        );
        assert_eq!(
            cohere_finish_reason(Some("STOP_SEQUENCE")).unified,
            FinishReason::Stop
        );
        assert_eq!(
            cohere_finish_reason(Some("MAX_TOKENS")).unified,
            FinishReason::Length
        );
        assert_eq!(
            cohere_finish_reason(Some("ERROR")).unified,
            FinishReason::Error
        );
        assert_eq!(
            cohere_finish_reason(Some("TOOL_CALL")).unified,
            FinishReason::ToolCalls
        );
        assert_eq!(
            cohere_finish_reason(Some("SAFETY")).unified,
            FinishReason::Other
        );
    }

    #[test]
    fn cohere_chat_request_conversion_maps_prompt_tools_documents_reasoning_and_headers() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let transport =
            captured_transport(Arc::clone(&captured_request), cohere_success_response());
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "cohere": {
                "thinking": {
                    "type": "enabled",
                    "tokenBudget": 1234
                }
            }
        }))
        .expect("provider options deserialize");
        let image_options: ProviderOptions = serde_json::from_value(json!({
            "cohere": {
                "detail": "high"
            }
        }))
        .expect("image options deserialize");
        let weather_tool = LanguageModelTool::Function(
            LanguageModelFunctionTool::new(
                "weather",
                schema(json!({
                    "city": { "type": "string" }
                })),
            )
            .with_description("Get weather"),
        );
        let provider_tool = LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "cohere.search",
            "search",
            JsonObject::new(),
        ));
        let prompt = vec![
            LanguageModelMessage::System(LanguageModelSystemMessage::new("You are concise.")),
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Look at this")),
                LanguageModelUserContentPart::File(
                    LanguageModelFilePart::new(
                        FileData::Data {
                            data: FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47]),
                        },
                        "image/png",
                    )
                    .with_provider_options(image_options),
                ),
                LanguageModelUserContentPart::File(
                    LanguageModelFilePart::new(
                        FileData::Text {
                            text: "Document body".to_string(),
                        },
                        "text/plain",
                    )
                    .with_filename("notes.txt"),
                ),
            ])),
        ];
        let model = CohereProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.cohere.test/v2")
            .with_header("x-provider", "cohere")
            .with_transport(transport)
            .language_model("command-r");
        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(prompt)
                    .with_max_output_tokens(512)
                    .with_temperature(0.4)
                    .with_top_p(0.8)
                    .with_top_k(20)
                    .with_frequency_penalty(0.1)
                    .with_presence_penalty(0.2)
                    .with_seed(42)
                    .with_stop_sequence("END")
                    .with_response_format(LanguageModelResponseFormat::json().with_schema(schema(
                        json!({
                            "answer": { "type": "string" }
                        }),
                    )))
                    .with_tool(weather_tool)
                    .with_tool(provider_tool)
                    .with_tool_choice(LanguageModelToolChoice::Tool {
                        tool_name: "weather".to_string(),
                    })
                    .with_header("x-call", "call")
                    .with_provider_options(provider_options.clone()),
            ),
        );

        assert_eq!(result.warnings.len(), 1);
        assert!(matches!(
            &result.warnings[0],
            crate::warning::Warning::Unsupported { feature, .. }
                if feature == "provider-defined tool cohere.search"
        ));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("x-provider").map(String::as_str),
            Some("cohere")
        );
        assert_eq!(
            request.headers.get("x-call").map(String::as_str),
            Some("call")
        );

        let body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body");
        assert_eq!(
            body.get("model").and_then(JsonValue::as_str),
            Some("command-r")
        );
        assert_eq!(body.get("frequency_penalty"), Some(&json!(0.1)));
        assert_eq!(body.get("presence_penalty"), Some(&json!(0.2)));
        assert_eq!(body.get("max_tokens"), Some(&json!(512)));
        assert_eq!(body.get("temperature"), Some(&json!(0.4)));
        assert_eq!(body.get("p"), Some(&json!(0.8)));
        assert_eq!(body.get("k"), Some(&json!(20)));
        assert_eq!(body.get("seed"), Some(&json!(42)));
        assert_eq!(body.get("stop_sequences"), Some(&json!(["END"])));
        assert_eq!(
            body.get("response_format"),
            Some(&json!({
                "type": "json_object",
                "json_schema": {
                    "type": "object",
                    "properties": {
                        "answer": { "type": "string" }
                    }
                }
            }))
        );
        assert_eq!(body.get("tool_choice"), Some(&json!("REQUIRED")));
        assert_eq!(
            body.get("tools")
                .and_then(JsonValue::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            body.get("thinking"),
            Some(&json!({ "type": "enabled", "token_budget": 1234 }))
        );
        assert_eq!(
            body.get("documents"),
            Some(&json!([{
                "data": {
                    "text": "Document body",
                    "title": "notes.txt"
                }
            }]))
        );
        assert_eq!(
            body.pointer("/messages/1/content/1/image_url/detail")
                .and_then(JsonValue::as_str),
            Some("high")
        );
        assert!(
            body.pointer("/messages/1/content/1/image_url/url")
                .and_then(JsonValue::as_str)
                .is_some_and(|url| url.starts_with("data:image/png;base64,"))
        );

        let (body_with_reasoning, warnings) = cohere_chat_request_body(
            "command-r",
            &LanguageModelCallOptions::new(Vec::new())
                .with_max_output_tokens(20)
                .with_reasoning(LanguageModelReasoningEffort::None),
            false,
        )
        .expect("reasoning none body");
        assert!(warnings.is_empty());
        assert_eq!(
            body_with_reasoning.get("thinking"),
            Some(&json!({ "type": "disabled" }))
        );

        let (body_with_budget, warnings) = cohere_chat_request_body(
            "command-r",
            &LanguageModelCallOptions::new(Vec::new())
                .with_max_output_tokens(20)
                .with_reasoning(LanguageModelReasoningEffort::High),
            false,
        )
        .expect("reasoning budget body");
        assert!(warnings.is_empty());
        assert_eq!(
            body_with_budget
                .pointer("/thinking/token_budget")
                .and_then(JsonValue::as_u64),
            Some(19661)
        );

        let (body_without_files, warnings) = cohere_chat_request_body(
            "command-r",
            &LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new("hello"),
                )]),
            )]),
            false,
        )
        .expect("text-only body");
        assert!(warnings.is_empty());
        assert!(body_without_files.get("documents").is_none());
        assert!(body_without_files.get("thinking").is_none());

        let (body_with_provider_override, warnings) = cohere_chat_request_body(
            "command-r",
            &LanguageModelCallOptions::new(Vec::new())
                .with_reasoning(LanguageModelReasoningEffort::None)
                .with_provider_options(provider_options),
            false,
        )
        .expect("provider thinking wins");
        assert!(warnings.is_empty());
        assert_eq!(
            body_with_provider_override.get("thinking"),
            Some(&json!({ "type": "enabled", "token_budget": 1234 }))
        );
    }

    #[test]
    fn convert_to_cohere_chat_prompt_maps_user_assistant_tool_files_and_unsupported_references() {
        let reference = ProviderReference::from_map(BTreeMap::from([(
            "cohere".to_string(),
            "file-123".to_string(),
        )]))
        .expect("provider reference");
        let prompt = vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello ")),
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("")),
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("world")),
                LanguageModelUserContentPart::File(
                    LanguageModelFilePart::new(
                        FileData::Data {
                            data: FileDataContent::Bytes(b"bytes doc".to_vec()),
                        },
                        "text/plain",
                    )
                    .with_filename("bytes.txt"),
                ),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64("base64 doc".to_string()),
                    },
                    "text/plain",
                )),
            ])),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("I will call.")),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-1",
                    "weather",
                    json!({ "city": "Paris" }),
                )),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-1",
                    "weather",
                    LanguageModelToolResultOutput::text("Sunny"),
                )),
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-2",
                    "json",
                    LanguageModelToolResultOutput::json(json!({ "ok": true })),
                )),
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-3",
                    "denied",
                    LanguageModelToolResultOutput::execution_denied(),
                )),
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-4",
                    "error",
                    LanguageModelToolResultOutput::error_text("Failed"),
                )),
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-5",
                    "error-json",
                    LanguageModelToolResultOutput::error_json(json!({ "error": true })),
                )),
            ])),
        ];
        let converted = convert_to_cohere_chat_prompt(&prompt).expect("prompt converts");

        assert_eq!(
            converted.messages[0],
            json!({ "role": "user", "content": "Hello world" })
        );
        assert_eq!(
            converted.documents,
            vec![
                json!({ "data": { "text": "bytes doc", "title": "bytes.txt" } }),
                json!({ "data": { "text": "base64 doc" } })
            ]
        );
        assert_eq!(
            converted.messages[1],
            json!({
                "role": "assistant",
                "tool_calls": [{
                    "id": "call-1",
                    "type": "function",
                    "function": {
                        "name": "weather",
                        "arguments": "{\"city\":\"Paris\"}"
                    }
                }]
            })
        );
        assert_eq!(
            converted.messages[2],
            json!({ "role": "tool", "content": "Sunny", "tool_call_id": "call-1" })
        );
        assert_eq!(
            converted.messages[3],
            json!({ "role": "tool", "content": "{\"ok\":true}", "tool_call_id": "call-2" })
        );
        assert_eq!(
            converted.messages[4],
            json!({ "role": "tool", "content": "Tool call execution denied.", "tool_call_id": "call-3" })
        );
        assert_eq!(
            converted.messages[5],
            json!({ "role": "tool", "content": "Failed", "tool_call_id": "call-4" })
        );
        assert_eq!(
            converted.messages[6],
            json!({ "role": "tool", "content": "{\"error\":true}", "tool_call_id": "call-5" })
        );

        let provider_reference_prompt = vec![LanguageModelMessage::User(
            LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::File(
                LanguageModelFilePart::new(FileData::Reference { reference }, "text/plain"),
            )]),
        )];
        let error =
            convert_to_cohere_chat_prompt(&provider_reference_prompt).expect_err("unsupported");
        assert!(error.contains("file parts with provider references"));

        let url_prompt = vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::File(
                LanguageModelFilePart::new(
                    FileData::Url {
                        url: Url::parse("https://example.com/doc.txt").expect("url"),
                    },
                    "text/plain",
                ),
            )],
        ))];
        let error = convert_to_cohere_chat_prompt(&url_prompt).expect_err("url unsupported");
        assert!(error.contains("URLs should be downloaded by the AI SDK"));

        let image_prompt = vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Url {
                        url: Url::parse("https://example.com/image.png").expect("url"),
                    },
                    "image/png",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a]),
                    },
                    "image",
                )),
            ],
        ))];
        let converted = convert_to_cohere_chat_prompt(&image_prompt).expect("images convert");
        assert_eq!(
            converted.messages[0]
                .pointer("/content/0/image_url/url")
                .and_then(JsonValue::as_str),
            Some("https://example.com/image.png")
        );
        assert!(
            converted.messages[0]
                .pointer("/content/0/image_url/detail")
                .is_none()
        );
        assert!(
            converted.messages[0]
                .pointer("/content/1/image_url/url")
                .and_then(JsonValue::as_str)
                .is_some_and(|url| url.starts_with("data:image/png;base64,"))
        );
    }

    #[test]
    fn cohere_prepare_tools_maps_all_upstream_tool_choice_cases() {
        let function_tool = LanguageModelTool::Function(LanguageModelFunctionTool::new(
            "weather",
            schema(json!({ "city": { "type": "string" } })),
        ));
        let other_tool =
            LanguageModelTool::Function(LanguageModelFunctionTool::new("time", schema(json!({}))));
        let provider_tool = LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "cohere.search",
            "search",
            JsonObject::new(),
        ));
        let tools = vec![function_tool.clone(), other_tool.clone(), provider_tool];

        let (prepared, tool_choice, warnings) = cohere_prepare_tools(None, None);
        assert!(prepared.is_none());
        assert!(tool_choice.is_none());
        assert!(warnings.is_empty());

        let empty = Vec::new();
        let (prepared, tool_choice, warnings) = cohere_prepare_tools(Some(&empty), None);
        assert!(prepared.is_none());
        assert!(tool_choice.is_none());
        assert!(warnings.is_empty());

        let (prepared, tool_choice, warnings) =
            cohere_prepare_tools(Some(&tools), Some(&LanguageModelToolChoice::Auto));
        assert_eq!(
            prepared.and_then(|value| value.as_array().map(Vec::len)),
            Some(2)
        );
        assert!(tool_choice.is_none());
        assert_eq!(warnings.len(), 1);

        let (_, tool_choice, _) =
            cohere_prepare_tools(Some(&tools), Some(&LanguageModelToolChoice::None));
        assert_eq!(tool_choice, Some(json!("NONE")));

        let (_, tool_choice, _) =
            cohere_prepare_tools(Some(&tools), Some(&LanguageModelToolChoice::Required));
        assert_eq!(tool_choice, Some(json!("REQUIRED")));

        let (prepared, tool_choice, _) = cohere_prepare_tools(
            Some(&tools),
            Some(&LanguageModelToolChoice::Tool {
                tool_name: "weather".to_string(),
            }),
        );
        assert_eq!(tool_choice, Some(json!("REQUIRED")));
        assert_eq!(
            prepared.and_then(|value| value.as_array().cloned()),
            Some(vec![json!({
                "type": "function",
                "function": {
                    "name": "weather",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "city": { "type": "string" }
                        }
                    }
                }
            })])
        );
    }

    #[test]
    fn cohere_chat_model_streams_text_reasoning_tool_calls_raw_chunks_and_metadata() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let body = [
            sse(json!({ "type": "message-start", "id": "stream-id" })),
            sse(json!({
                "type": "content-start",
                "index": 0,
                "delta": { "message": { "content": { "type": "thinking" } } }
            })),
            sse(json!({
                "type": "content-delta",
                "index": 0,
                "delta": { "message": { "content": { "thinking": "I think" } } }
            })),
            sse(json!({ "type": "content-end", "index": 0 })),
            sse(json!({
                "type": "content-start",
                "index": 1,
                "delta": { "message": { "content": { "type": "text" } } }
            })),
            sse(json!({
                "type": "content-delta",
                "index": 1,
                "delta": { "message": { "content": { "text": "Paris" } } }
            })),
            sse(json!({ "type": "content-end", "index": 1 })),
            sse(json!({
                "type": "tool-call-start",
                "delta": {
                    "message": {
                        "tool_calls": {
                            "id": "tool-1",
                            "function": {
                                "name": "weather",
                                "arguments": "{\"city\""
                            }
                        }
                    }
                }
            })),
            sse(json!({
                "type": "tool-call-delta",
                "delta": {
                    "message": {
                        "tool_calls": {
                            "function": { "arguments": ":\"Paris\"}" }
                        }
                    }
                }
            })),
            sse(json!({ "type": "tool-call-end" })),
            sse(json!({
                "type": "message-end",
                "delta": {
                    "finish_reason": "TOOL_CALL",
                    "usage": {
                        "tokens": {
                            "input_tokens": 5,
                            "output_tokens": 6
                        }
                    }
                }
            })),
        ]
        .join("");
        let transport = captured_transport(
            Arc::clone(&captured_request),
            ProviderApiResponse::text(200, "OK", body).with_headers(Headers::from([(
                "content-type".to_string(),
                "text/event-stream".to_string(),
            )])),
        );
        let model = CohereProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .language_model("command-r");
        let result = poll_ready(
            model.do_stream(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("hello"),
                    )]),
                )])
                .with_include_raw_chunks(true)
                .with_header("x-stream-call", "call"),
            ),
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request captured");
        assert_eq!(
            request.headers.get("x-stream-call").map(String::as_str),
            Some("call")
        );
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("stream request body");
        assert_eq!(request_body.get("stream"), Some(&JsonValue::Bool(true)));
        assert_eq!(
            request_body.get("model").and_then(JsonValue::as_str),
            Some("command-r")
        );
        assert_eq!(
            request_body
                .pointer("/messages/0/content")
                .and_then(JsonValue::as_str),
            Some("hello")
        );
        assert!(matches!(
            &result.stream[0],
            LanguageModelStreamPart::StreamStart(start) if start.warnings.is_empty()
        ));
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Raw(_)))
        );
        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::ResponseMetadata(metadata)
                if metadata.id.as_deref() == Some("stream-id")
        )));
        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::ReasoningDelta(delta)
                if delta.id == "0" && delta.delta == "I think"
        )));
        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::TextDelta(delta)
                if delta.id == "1" && delta.delta == "Paris"
        )));
        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::ToolInputDelta(delta)
                if delta.id == "tool-1" && delta.delta == ":\"Paris\"}"
        )));
        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::ToolCall(tool_call)
                if tool_call.tool_call_id == "tool-1"
                    && tool_call.tool_name == "weather"
                    && tool_call.input == "{\"city\":\"Paris\"}"
        )));
        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::Finish(finish)
                if finish.finish_reason.unified == FinishReason::ToolCalls
                    && finish.usage.input_tokens.total == Some(5)
                    && finish.usage.output_tokens.total == Some(6)
        )));
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("content-type"))
                .map(String::as_str),
            Some("text/event-stream")
        );
    }

    #[test]
    fn cohere_chat_streaming_maps_parse_and_tool_argument_errors() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let body = [
            "data: not-json\n\n".to_string(),
            sse(json!({
                "type": "tool-call-start",
                "delta": {
                    "message": {
                        "tool_calls": {
                            "id": "tool-1",
                            "function": {
                                "name": "unsafe",
                                "arguments": "{\"__proto__\":true}"
                            }
                        }
                    }
                }
            })),
            sse(json!({ "type": "tool-call-end" })),
        ]
        .join("");
        let transport = captured_transport(
            Arc::clone(&captured_request),
            ProviderApiResponse::text(200, "OK", body).with_headers(Headers::from([(
                "content-type".to_string(),
                "text/event-stream".to_string(),
            )])),
        );
        let model = CohereProvider::new()
            .with_transport(transport)
            .language_model("command-r");
        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(Vec::new())));

        let error_count = result
            .stream
            .iter()
            .filter(|part| matches!(part, LanguageModelStreamPart::Error(_)))
            .count();
        assert_eq!(error_count, 2);
        assert!(
            !result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Raw(_)))
        );
        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::Finish(finish)
                if finish.finish_reason.unified == FinishReason::Error
        )));
    }

    #[test]
    fn cohere_chat_streaming_handles_empty_tool_call_arguments() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let body = [
            sse(json!({
                "type": "tool-call-start",
                "delta": {
                    "message": {
                        "tool_calls": {
                            "id": "tool-empty",
                            "function": {
                                "name": "currentTime",
                                "arguments": ""
                            }
                        }
                    }
                }
            })),
            sse(json!({ "type": "tool-call-end" })),
            sse(json!({
                "type": "message-end",
                "delta": {
                    "finish_reason": "TOOL_CALL",
                    "usage": {
                        "tokens": {
                            "input_tokens": 1,
                            "output_tokens": 2
                        }
                    }
                }
            })),
        ]
        .join("");
        let transport = captured_transport(
            Arc::clone(&captured_request),
            ProviderApiResponse::text(200, "OK", body).with_headers(Headers::from([(
                "content-type".to_string(),
                "text/event-stream".to_string(),
            )])),
        );
        let model = CohereProvider::new()
            .with_transport(transport)
            .language_model("command-r");
        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(Vec::new())));

        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::ToolCall(tool_call)
                if tool_call.tool_call_id == "tool-empty"
                    && tool_call.tool_name == "currentTime"
                    && tool_call.input == "{}"
        )));
    }

    #[test]
    fn cohere_chat_model_maps_api_and_schema_errors_to_rust_metadata() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let api_transport = captured_transport(
            Arc::clone(&captured_request),
            ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({ "message": "bad chat" }).to_string(),
            ),
        );
        let model = CohereProvider::new()
            .with_transport(api_transport)
            .language_model("command-r");
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(result.finish_reason.unified, FinishReason::Error);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("cohere"))
                .and_then(|cohere| cohere.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("bad chat")
        );

        let validation_transport = captured_transport(
            Arc::new(Mutex::new(None::<ProviderApiRequest>)),
            ProviderApiResponse::text(200, "OK", json!({ "message": {} }).to_string()),
        );
        let model = CohereProvider::new()
            .with_transport(validation_transport)
            .language_model("command-r");
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(Vec::new())));
        let message = result
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("cohere"))
            .and_then(|cohere| cohere.get("errorMessage"))
            .and_then(JsonValue::as_str)
            .expect("validation error metadata");
        assert!(message.contains("Invalid JSON response"));
    }

    #[test]
    fn cohere_embedding_model_enforces_chunk_limit_and_validation_errors() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let transport =
            captured_transport(Arc::clone(&captured_request), cohere_success_response());
        let model = CohereProvider::new()
            .with_transport(transport)
            .embedding_model("embed-v4.0");
        let values = (0..97)
            .map(|index| format!("value-{index}"))
            .collect::<Vec<_>>();
        let result = poll_ready(model.do_embed(EmbeddingModelCallOptions::new(values)));
        let message = result
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("cohere"))
            .and_then(|cohere| cohere.get("errorMessage"))
            .and_then(JsonValue::as_str)
            .expect("chunk limit metadata");

        assert!(message.contains("can only embed up to 96 values per call"));
        assert!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .is_none(),
            "chunk-limit error should not make an HTTP request"
        );

        let validation_transport = captured_transport(
            Arc::new(Mutex::new(None::<ProviderApiRequest>)),
            ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "embeddings": {
                        "float": [[0.1, 0.2]]
                    }
                })
                .to_string(),
            ),
        );
        let model = CohereProvider::new()
            .with_transport(validation_transport)
            .embedding_model("embed-v4.0");
        let result =
            poll_ready(model.do_embed(EmbeddingModelCallOptions::new(vec!["sunny".to_string()])));
        let message = result
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("cohere"))
            .and_then(|cohere| cohere.get("errorMessage"))
            .and_then(JsonValue::as_str)
            .expect("validation metadata");
        assert!(message.contains("Invalid JSON response"));
    }

    #[test]
    fn cohere_reranking_model_maps_validation_and_api_errors() {
        let validation_transport = captured_transport(
            Arc::new(Mutex::new(None::<ProviderApiRequest>)),
            ProviderApiResponse::text(200, "OK", json!({ "id": "missing-results" }).to_string()),
        );
        let model = CohereProvider::new()
            .with_transport(validation_transport)
            .reranking_model("rerank-v3.5");
        let result = poll_ready(model.do_rerank(RerankingModelCallOptions::new(
            RerankingModelDocuments::Text {
                values: vec!["a".to_string(), "b".to_string()],
            },
            "query",
        )));
        let message = result
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("cohere"))
            .and_then(|cohere| cohere.get("errorMessage"))
            .and_then(JsonValue::as_str)
            .expect("validation metadata");
        assert!(message.contains("Invalid JSON response"));

        let api_transport = captured_transport(
            Arc::new(Mutex::new(None::<ProviderApiRequest>)),
            ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({ "message": "bad rerank" }).to_string(),
            )
            .with_headers(Headers::from([(
                "x-request-id".to_string(),
                "bad-rerank".to_string(),
            )])),
        );
        let model = CohereProvider::new()
            .with_transport(api_transport)
            .reranking_model("rerank-v3.5");
        let result = poll_ready(model.do_rerank(RerankingModelCallOptions::new(
            RerankingModelDocuments::Text {
                values: vec!["a".to_string(), "b".to_string()],
            },
            "query",
        )));

        assert_eq!(result.ranking.len(), 0);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("cohere"))
                .and_then(|cohere| cohere.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("bad rerank")
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("bad-rerank")
        );
    }

    #[test]
    #[ignore = "requires COHERE_API_KEY"]
    fn live_cohere_chat_acceptance_requires_cohere_api_key() {
        if env::var("COHERE_API_KEY").is_err() {
            return;
        }

        let result = poll_ready(
            cohere("command-r").do_generate(LanguageModelCallOptions::new(vec![
                LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Say hello.")),
                ])),
            ])),
        );

        assert!(
            result.provider_metadata.is_none(),
            "live Cohere chat returned metadata: {:?}",
            result.provider_metadata
        );
    }

    #[test]
    #[ignore = "requires COHERE_API_KEY"]
    fn live_cohere_embedding_acceptance_requires_cohere_api_key() {
        if env::var("COHERE_API_KEY").is_err() {
            return;
        }

        let model = CohereProvider::new().embedding_model("embed-v4.0");
        let result =
            poll_ready(model.do_embed(EmbeddingModelCallOptions::new(vec!["hello".to_string()])));

        assert!(
            result.provider_metadata.is_none(),
            "live Cohere embedding returned metadata: {:?}",
            result.provider_metadata
        );
        assert!(!result.embeddings.is_empty());
    }

    #[test]
    #[ignore = "requires COHERE_API_KEY"]
    fn live_cohere_reranking_acceptance_requires_cohere_api_key() {
        if env::var("COHERE_API_KEY").is_err() {
            return;
        }

        let model = CohereProvider::new().reranking_model("rerank-v3.5");
        let result = poll_ready(model.do_rerank(RerankingModelCallOptions::new(
            RerankingModelDocuments::Text {
                values: vec!["sunny".to_string(), "rainy".to_string()],
            },
            "weather",
        )));

        assert!(
            result.provider_metadata.is_none(),
            "live Cohere reranking returned metadata: {:?}",
            result.provider_metadata
        );
    }

    #[test]
    fn cohere_provider_creates_chat_model_and_reports_image_exception() {
        let provider = CohereProvider::new();

        let chat = cohere("command-r");
        let image_error = Provider::image_model(&provider, "image")
            .err()
            .expect("image models are unsupported");

        assert_eq!(chat.provider(), "cohere.chat");
        assert_eq!(chat.model_id(), "command-r");
        assert_eq!(image_error.model_type(), ModelType::ImageModel);
        assert_eq!(DEFAULT_COHERE_BASE_URL, "https://api.cohere.com/v2");
    }

    #[test]
    fn cohere_provider_implements_embedding_and_reranking_traits() {
        let provider = CohereProvider::new();
        let embedding = Provider::embedding_model(&provider, "embed-v4.0").expect("embedding");
        let chat = Provider::language_model(&provider, "command-r").expect("chat");
        let reranking = ProviderWithRerankingModel::reranking_model(&provider, "rerank-v3.5")
            .expect("reranking");

        assert_eq!(chat.provider(), "cohere.chat");
        assert_eq!(embedding.provider(), "cohere.textEmbedding");
        assert_eq!(reranking.provider(), "cohere.reranking");
    }

    #[test]
    fn cohere_provider_settings_serde_accepts_upstream_base_url() {
        let settings: CohereProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.cohere.test/v2/",
            "apiKey": "key",
            "headers": {
                "x-provider": "cohere"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            CohereProviderSettings::new()
                .with_base_url("https://api.cohere.test/v2/")
                .with_api_key("key")
                .with_header("x-provider", "cohere")
        );
    }
}
