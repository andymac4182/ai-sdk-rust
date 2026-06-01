//! Google Generative AI provider for the Rust port of upstream `@ai-sdk/google`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_rust::{
    EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResult, FetchErrorInfo, FileData,
    FileDataContent, Files, FilesUploadFileCallOptions, FilesUploadFileData, FilesUploadFileResult,
    FinishReason, Headers, ImageModel, ImageModelCallOptions, ImageModelFile,
    ImageModelProviderMetadata, ImageModelProviderMetadataEntry, ImageModelResponse,
    ImageModelResult, ImageModelUsage, InputTokenUsage, JsonObject, JsonSchema, JsonValue,
    LanguageModel, LanguageModelAssistantContentPart, LanguageModelCallOptions,
    LanguageModelContent, LanguageModelErrorStreamPart, LanguageModelFile, LanguageModelFileData,
    LanguageModelFilePart, LanguageModelFinishReason, LanguageModelGenerateResult,
    LanguageModelMessage, LanguageModelPrompt, LanguageModelProviderTool, LanguageModelReasoning,
    LanguageModelReasoningDelta, LanguageModelReasoningEffort, LanguageModelReasoningFile,
    LanguageModelRequest, LanguageModelResponse, LanguageModelResponseFormat, LanguageModelSource,
    LanguageModelStreamFinish, LanguageModelStreamPart, LanguageModelStreamResult,
    LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
    LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextPart,
    LanguageModelTextStart, LanguageModelTool, LanguageModelToolCall, LanguageModelToolChoice,
    LanguageModelToolContentPart, LanguageModelToolResult, LanguageModelToolResultOutput,
    LanguageModelUsage, LanguageModelUserContentPart, LoadApiKeyOptions, NoSuchModelError,
    NonNullJsonValue, OutputTokenUsage, ParseJsonResult, PostJsonToApiOptions, Provider,
    ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, ProviderMetadata, ProviderOptions, ProviderWithFiles,
    ProviderWithVideoModel, RuntimeEnvironment, TooManyEmbeddingValuesForCallError, VideoModel,
    VideoModelCallOptions, VideoModelFile, VideoModelResponse, VideoModelResult,
    VideoModelVideoData, Warning, combine_headers, convert_to_base64,
    create_event_source_response_handler, create_json_error_response_handler,
    create_json_response_handler, generate_id, get_from_api, get_top_level_media_type,
    load_api_key, parse_provider_options, post_json_to_api, resolve_provider_reference,
    without_trailing_slash,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, json};
use time::OffsetDateTime;
use url::Url;

/// The crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Upstream package covered by this crate.
pub const UPSTREAM_PACKAGE: &str = "@ai-sdk/google";

/// Upstream package directory in `vercel/ai`.
pub const UPSTREAM_PACKAGE_DIR: &str = "packages/google";

/// Upstream commit used for the checked-in AI-01 inventory.
pub const UPSTREAM_COMMIT: &str = "ab6d66482d31afe15f4973a51c5f7cfa09c92ea6";

/// Checked-in row-level inventory document for this package.
pub const INVENTORY_DOCUMENT: &str = "docs/ai-foundational-provider-inventory.md";

/// Current upstream test files under `packages/google/src`.
pub const UPSTREAM_TEST_FILES: usize = 21;

/// Current detected upstream `it`/`test` cases under `packages/google/src`.
pub const UPSTREAM_TEST_CASES: usize = 568;

/// Current explicit TypeScript type-system exceptions.
pub const TYPE_SYSTEM_IMPOSSIBLE_CASES: usize = 2;

/// Current explicit JavaScript runtime exceptions.
pub const JS_ONLY_DOCUMENTED_CASES: usize = 0;

/// Current portable upstream cases mapped to named Rust tests.
pub const PORTABLE_MAPPED_CASES: usize =
    UPSTREAM_TEST_CASES - TYPE_SYSTEM_IMPOSSIBLE_CASES - JS_ONLY_DOCUMENTED_CASES;

/// Current portable cases still requiring named Rust tests.
pub const PORTABLE_UNMAPPED_CASES: usize = 0;

/// Default base URL for Google Generative Language API calls.
pub const DEFAULT_GOOGLE_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

type GoogleTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Google provider models.
pub type GoogleTransport = Arc<dyn Fn(ProviderApiRequest) -> GoogleTransportFuture + Send + Sync>;

type GoogleGenerateId = Arc<dyn Fn() -> String + Send + Sync>;
type GoogleDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;

/// Settings for the upstream Google provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleProviderSettings {
    /// API base URL. Defaults to the Generative Language v1beta endpoint.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// API key. When omitted, `GOOGLE_GENERATIVE_AI_API_KEY` is read at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,

    /// Custom provider name. Defaults to `google.generative-ai`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl GoogleProviderSettings {
    /// Creates empty Google provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Sets the provider name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

/// Google Generative AI provider.
#[derive(Clone)]
pub struct GoogleProvider {
    settings: GoogleProviderSettings,
    base_url: String,
    provider_name: String,
    transport: GoogleTransport,
    generate_id: GoogleGenerateId,
    current_date: GoogleDateProvider,
}

impl GoogleProvider {
    /// Creates a Google provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(GoogleProviderSettings::new())
    }

    /// Creates a Google provider from explicit settings.
    pub fn from_settings(settings: GoogleProviderSettings) -> Self {
        let base_url = without_trailing_slash(settings.base_url.as_deref())
            .unwrap_or(DEFAULT_GOOGLE_BASE_URL)
            .to_string();
        let provider_name = settings
            .name
            .clone()
            .unwrap_or_else(|| "google.generative-ai".to_string());

        Self {
            settings,
            base_url,
            provider_name,
            transport: default_google_transport(),
            generate_id: Arc::new(generate_id),
            current_date: Arc::new(OffsetDateTime::now_utc),
        }
    }

    /// Sets the API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self.base_url = without_trailing_slash(self.settings.base_url.as_deref())
            .unwrap_or(DEFAULT_GOOGLE_BASE_URL)
            .to_string();
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: GoogleTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Replaces the request id generator. This is primarily useful for tests.
    pub fn with_generate_id<F>(mut self, generate_id: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        self.generate_id = Arc::new(generate_id);
        self
    }

    /// Replaces the response timestamp provider. This is primarily useful for tests.
    pub fn with_current_date<F>(mut self, current_date: F) -> Self
    where
        F: Fn() -> OffsetDateTime + Send + Sync + 'static,
    {
        self.current_date = Arc::new(current_date);
        self
    }

    /// Creates a Gemini language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> GoogleLanguageModel {
        self.chat(model_id)
    }

    /// Creates a Gemini language model.
    pub fn chat(&self, model_id: impl Into<String>) -> GoogleLanguageModel {
        GoogleLanguageModel::new(
            model_id.into(),
            GoogleLanguageModelConfig {
                provider: self.provider_name.clone(),
                base_url: self.base_url.clone(),
                settings: self.settings.clone(),
                transport: Arc::clone(&self.transport),
                generate_id: Arc::clone(&self.generate_id),
            },
        )
    }

    /// Deprecated upstream alias for [`GoogleProvider::chat`].
    pub fn generative_ai(&self, model_id: impl Into<String>) -> GoogleLanguageModel {
        self.chat(model_id)
    }

    /// Creates a text embedding model.
    pub fn embedding(&self, model_id: impl Into<String>) -> GoogleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates a text embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> GoogleEmbeddingModel {
        GoogleEmbeddingModel::new(
            model_id.into(),
            GoogleModelConfig {
                provider: self.provider_name.clone(),
                base_url: self.base_url.clone(),
                settings: self.settings.clone(),
                transport: Arc::clone(&self.transport),
                current_date: Arc::clone(&self.current_date),
            },
        )
    }

    /// Deprecated upstream alias for [`GoogleProvider::embedding`].
    pub fn text_embedding(&self, model_id: impl Into<String>) -> GoogleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`GoogleProvider::embedding_model`].
    pub fn text_embedding_model(&self, model_id: impl Into<String>) -> GoogleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates an image model.
    pub fn image(&self, model_id: impl Into<String>) -> GoogleImageModel {
        self.image_with_settings(model_id, GoogleImageSettings::new())
    }

    /// Creates an image model with explicit settings.
    pub fn image_with_settings(
        &self,
        model_id: impl Into<String>,
        settings: GoogleImageSettings,
    ) -> GoogleImageModel {
        GoogleImageModel::new(
            model_id.into(),
            settings,
            GoogleImageModelConfig {
                provider: self.provider_name.clone(),
                base_url: self.base_url.clone(),
                settings: self.settings.clone(),
                transport: Arc::clone(&self.transport),
                generate_id: Arc::clone(&self.generate_id),
                current_date: Arc::clone(&self.current_date),
            },
        )
    }

    /// Creates an image model.
    pub fn image_model(&self, model_id: impl Into<String>) -> GoogleImageModel {
        self.image(model_id)
    }

    /// Creates a video model.
    pub fn video(&self, model_id: impl Into<String>) -> GoogleVideoModel {
        self.video_model(model_id)
    }

    /// Creates a video model.
    pub fn video_model(&self, model_id: impl Into<String>) -> GoogleVideoModel {
        GoogleVideoModel::new(
            model_id.into(),
            GoogleModelConfig {
                provider: self.provider_name.clone(),
                base_url: self.base_url.clone(),
                settings: self.settings.clone(),
                transport: Arc::clone(&self.transport),
                current_date: Arc::clone(&self.current_date),
            },
        )
    }

    /// Creates the Google files upload interface.
    pub fn files(&self) -> GoogleFiles {
        GoogleFiles::new(GoogleModelConfig {
            provider: self.provider_name.clone(),
            base_url: self.base_url.clone(),
            settings: self.settings.clone(),
            transport: Arc::clone(&self.transport),
            current_date: Arc::clone(&self.current_date),
        })
    }

    /// Creates a language model targeting the Gemini Interactions API.
    pub fn interactions(
        &self,
        model: impl Into<GoogleInteractionsModelInput>,
    ) -> GoogleInteractionsLanguageModel {
        GoogleInteractionsLanguageModel::new(
            model.into(),
            GoogleLanguageModelConfig {
                provider: format!("{}.interactions", self.provider_name),
                base_url: self.base_url.clone(),
                settings: self.settings.clone(),
                transport: Arc::clone(&self.transport),
                generate_id: Arc::clone(&self.generate_id),
            },
        )
    }

    /// Provider-defined Google tool factories.
    pub fn tools(&self) -> GoogleTools {
        GoogleTools
    }
}

impl Default for GoogleProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for GoogleProvider {
    type LanguageModel = GoogleLanguageModel;
    type EmbeddingModel = GoogleEmbeddingModel;
    type ImageModel = GoogleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(GoogleProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(GoogleProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(GoogleProvider::image_model(self, model_id))
    }
}

impl ProviderWithVideoModel for GoogleProvider {
    type VideoModel = GoogleVideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        Ok(GoogleProvider::video_model(self, model_id))
    }
}

impl ProviderWithFiles for GoogleProvider {
    type Files = GoogleFiles;

    fn files(&self) -> Self::Files {
        GoogleProvider::files(self)
    }
}

/// Creates a Google provider with explicit settings.
pub fn create_google(settings: GoogleProviderSettings) -> GoogleProvider {
    GoogleProvider::from_settings(settings)
}

/// Creates a Gemini language model using default provider settings.
pub fn google(model_id: impl Into<String>) -> GoogleLanguageModel {
    GoogleProvider::new().language_model(model_id)
}

#[derive(Clone)]
struct GoogleModelConfig {
    provider: String,
    base_url: String,
    settings: GoogleProviderSettings,
    transport: GoogleTransport,
    current_date: GoogleDateProvider,
}

#[derive(Clone)]
struct GoogleLanguageModelConfig {
    provider: String,
    base_url: String,
    settings: GoogleProviderSettings,
    transport: GoogleTransport,
    generate_id: GoogleGenerateId,
}

#[derive(Clone)]
struct GoogleImageModelConfig {
    provider: String,
    base_url: String,
    settings: GoogleProviderSettings,
    transport: GoogleTransport,
    generate_id: GoogleGenerateId,
    current_date: GoogleDateProvider,
}

/// Google image-model constructor settings.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleImageSettings {
    /// Override the default per-call image limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_images_per_call: Option<usize>,
}

impl GoogleImageSettings {
    /// Creates empty image settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the max image count for one provider call.
    pub fn with_max_images_per_call(mut self, value: usize) -> Self {
        self.max_images_per_call = Some(value);
        self
    }
}

/// Google language model.
#[derive(Clone)]
pub struct GoogleLanguageModel {
    model_id: String,
    config: GoogleLanguageModelConfig,
}

impl GoogleLanguageModel {
    fn new(model_id: String, config: GoogleLanguageModelConfig) -> Self {
        Self { model_id, config }
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, String> {
        google_request_headers(&self.config.settings, call_headers)
    }

    fn supported_url_patterns(&self) -> LanguageModelSupportedUrls {
        BTreeMap::from([(
            "*".to_string(),
            vec![
                format!("^{}/files/.*$", regex_escape(&self.config.base_url)),
                "^https://(?:www\\.)?youtube\\.com/watch\\?v=[\\w-]+(?:&[\\w=&.-]*)?$".to_string(),
                "^https://youtu\\.be/[\\w-]+(?:\\?[\\w=&.-]*)?$".to_string(),
            ],
        )])
    }

    fn get_args(
        &self,
        options: &LanguageModelCallOptions,
        is_streaming: bool,
    ) -> Result<GoogleLanguageArgs, String> {
        let provider_option_names = if self.config.provider.contains("vertex") {
            vec!["googleVertex", "vertex"]
        } else {
            vec!["google"]
        };
        let is_vertex_provider = self.config.provider.starts_with("google.vertex.");
        let mut warnings = Vec::new();

        let mut google_options = first_provider_options::<GoogleLanguageModelOptions>(
            &provider_option_names,
            options.provider_options.as_ref(),
        )?;
        if google_options.is_none() && !provider_option_names.contains(&"google") {
            google_options = provider_options_for("google", options.provider_options.as_ref())?;
        }
        let google_options = google_options.unwrap_or_default();

        if options.tools.as_ref().is_some_and(|tools| {
            tools.iter().any(|tool| {
                matches!(
                    tool,
                    LanguageModelTool::Provider(provider_tool)
                        if provider_tool.id == "google.vertex_rag_store"
                )
            })
        }) && !is_vertex_provider
        {
            warnings.push(Warning::Other {
                message: format!(
                    "The 'vertex_rag_store' tool is only supported with the Google Vertex provider and might not be supported or could behave unexpectedly with the current Google provider ({}).",
                    self.config.provider
                ),
            });
        }

        if google_options.stream_function_call_arguments == Some(true) && !is_vertex_provider {
            warnings.push(Warning::Other {
                message: format!(
                    "'streamFunctionCallArguments' is only supported on the Vertex AI API and will be ignored with the current Google provider {}. See https://docs.cloud.google.com/vertex-ai/generative-ai/docs/multimodal/function-calling#streaming-fc",
                    self.config.provider
                ),
            });
        }

        let mut sanitized_service_tier = google_options.service_tier.clone();
        if is_vertex_provider {
            sanitized_service_tier =
                sanitized_service_tier.map(|tier| vertex_service_tier(&tier).to_string());
        }

        let is_gemma_model = self.model_id.to_ascii_lowercase().starts_with("gemma-");
        let supports_function_response_parts = self.model_id.starts_with("gemini-3");
        let prompt = convert_to_google_messages(
            &options.prompt,
            ConvertToGoogleMessagesOptions {
                is_gemma_model,
                provider_options_names: provider_option_names.clone(),
                supports_function_response_parts,
            },
        )?;

        let prepared_tools = prepare_google_tools(
            options.tools.as_deref(),
            options.tool_choice.as_ref(),
            &self.model_id,
            is_vertex_provider,
        )?;
        warnings.extend(prepared_tools.tool_warnings);

        let thinking_config = google_options.thinking_config.clone().or_else(|| {
            resolve_thinking_config(options.reasoning.as_ref(), &self.model_id, &mut warnings)
        });

        let stream_function_call_arguments = if is_streaming && is_vertex_provider {
            google_options
                .stream_function_call_arguments
                .unwrap_or(false)
        } else {
            false
        };

        let mut generation_config = JsonObject::new();
        insert_opt(
            &mut generation_config,
            "maxOutputTokens",
            options.max_output_tokens,
        );
        insert_opt(&mut generation_config, "temperature", options.temperature);
        insert_opt(&mut generation_config, "topK", options.top_k);
        insert_opt(&mut generation_config, "topP", options.top_p);
        insert_opt(
            &mut generation_config,
            "frequencyPenalty",
            options.frequency_penalty,
        );
        insert_opt(
            &mut generation_config,
            "presencePenalty",
            options.presence_penalty,
        );
        insert_opt(
            &mut generation_config,
            "stopSequences",
            options.stop_sequences.clone(),
        );
        insert_opt(&mut generation_config, "seed", options.seed);

        if matches!(
            options.response_format,
            Some(LanguageModelResponseFormat::Json { .. })
        ) {
            generation_config.insert("responseMimeType".to_string(), json!("application/json"));
        }
        if let Some(LanguageModelResponseFormat::Json {
            schema: Some(schema),
            ..
        }) = &options.response_format
        {
            if google_options.structured_outputs.unwrap_or(true) {
                if let Some(openapi_schema) = convert_json_schema_to_openapi_schema_value(
                    &JsonValue::Object(schema.clone()),
                    true,
                ) {
                    generation_config.insert("responseSchema".to_string(), openapi_schema);
                }
            }
        }
        insert_opt(
            &mut generation_config,
            "audioTimestamp",
            google_options.audio_timestamp,
        );
        insert_opt(
            &mut generation_config,
            "responseModalities",
            google_options.response_modalities.clone(),
        );
        if let Some(thinking_config) = thinking_config {
            generation_config.insert("thinkingConfig".to_string(), thinking_config);
        }
        insert_opt(
            &mut generation_config,
            "mediaResolution",
            google_options.media_resolution.clone(),
        );
        insert_opt(
            &mut generation_config,
            "imageConfig",
            google_options.image_config.clone(),
        );

        let mut body = JsonObject::new();
        body.insert(
            "generationConfig".to_string(),
            JsonValue::Object(generation_config),
        );
        body.insert("contents".to_string(), JsonValue::Array(prompt.contents));
        if !is_gemma_model {
            insert_opt(&mut body, "systemInstruction", prompt.system_instruction);
        }
        insert_opt(
            &mut body,
            "safetySettings",
            google_options.safety_settings.clone(),
        );
        insert_opt(&mut body, "tools", prepared_tools.tools);

        let tool_config = merge_tool_config(
            prepared_tools.tool_config,
            stream_function_call_arguments,
            google_options.retrieval_config.clone(),
        );
        insert_opt(&mut body, "toolConfig", tool_config);
        insert_opt(
            &mut body,
            "cachedContent",
            google_options.cached_content.clone(),
        );
        insert_opt(&mut body, "labels", google_options.labels.clone());
        insert_opt(&mut body, "serviceTier", sanitized_service_tier);

        Ok(GoogleLanguageArgs {
            body: JsonValue::Object(body),
            warnings,
            provider_options_names: provider_option_names,
        })
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let args = match self.get_args(&options, false) {
            Ok(args) => args,
            Err(error) => {
                return google_error_generate_result(
                    &self.model_id,
                    &error,
                    json!({ "model": self.model_id }),
                );
            }
        };
        let request_body = args.body.clone();
        let headers = match self.request_headers(options.headers.as_ref()) {
            Ok(headers) => headers,
            Err(error) => {
                return google_error_generate_result(&self.model_id, &error, request_body);
            }
        };

        let post_options = PostJsonToApiOptions::new(
            format!(
                "{}/{}:generateContent",
                self.config.base_url,
                get_model_path(&self.model_id)
            ),
            args.body.clone(),
        )
        .with_headers(headers)
        .with_environment(RuntimeEnvironment::unknown())
        .with_optional_abort_signal(options.abort_signal.clone());

        let transport = Arc::clone(&self.config.transport);
        let result = post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    |value| Ok::<JsonValue, String>(value.clone()),
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            google_failed_response_handler,
        )
        .await;

        match result {
            Ok(response) => google_generate_result_from_response(
                response.value,
                response.raw_value.unwrap_or(JsonValue::Null),
                response.response_headers.unwrap_or_default(),
                request_body,
                args.warnings,
                &args.provider_options_names,
                &self.config.generate_id,
            ),
            Err(error) => {
                google_error_generate_result(&self.model_id, &format!("{error:?}"), request_body)
            }
        }
    }

    async fn do_stream_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let include_raw_chunks = options.include_raw_chunks.unwrap_or(false);
        let args = match self.get_args(&options, true) {
            Ok(args) => args,
            Err(error) => {
                return google_error_stream_result(&error, json!({ "model": self.model_id }));
            }
        };
        let request_body = args.body.clone();
        let headers = match self.request_headers(options.headers.as_ref()) {
            Ok(headers) => headers,
            Err(error) => return google_error_stream_result(&error, request_body),
        };

        let post_options = PostJsonToApiOptions::new(
            format!(
                "{}/{}:streamGenerateContent?alt=sse",
                self.config.base_url,
                get_model_path(&self.model_id)
            ),
            args.body.clone(),
        )
        .with_headers(headers)
        .with_environment(RuntimeEnvironment::unknown())
        .with_optional_abort_signal(options.abort_signal.clone());

        let transport = Arc::clone(&self.config.transport);
        let result = post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |_request, response| {
                create_event_source_response_handler(
                    response.event_source_response_handler_options(),
                    |value| Ok::<JsonValue, String>(value.clone()),
                )
                .map_err(|error| ProviderApiResponseHandlerError::other(error.to_string()))
            },
            google_failed_response_handler,
        )
        .await;

        match result {
            Ok(response) => google_stream_result_from_chunks(
                response.value,
                response.response_headers.unwrap_or_default(),
                request_body,
                args.warnings,
                &args.provider_options_names,
                include_raw_chunks,
                &self.config.generate_id,
            ),
            Err(error) => google_error_stream_result(&format!("{error:?}"), request_body),
        }
    }
}

impl LanguageModel for GoogleLanguageModel {
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

    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(self.supported_url_patterns())
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(self.do_stream_result(options))
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleLanguageModelOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_outputs: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_modalities: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_timestamp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_config: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval_config: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_function_call_arguments: Option<bool>,
}

struct GoogleLanguageArgs {
    body: JsonValue,
    warnings: Vec<Warning>,
    provider_options_names: Vec<&'static str>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleEmbeddingModelOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_dimensionality: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<Vec<JsonValue>>>,
}

/// Google embedding model.
#[derive(Clone)]
pub struct GoogleEmbeddingModel {
    model_id: String,
    config: GoogleModelConfig,
}

impl GoogleEmbeddingModel {
    fn new(model_id: String, config: GoogleModelConfig) -> Self {
        Self { model_id, config }
    }

    async fn do_embed_result(&self, options: EmbeddingModelCallOptions) -> EmbeddingModelResult {
        if options.values.len() > 2048 {
            return EmbeddingModelResult::new(Vec::new()).with_warning(Warning::Other {
                message: TooManyEmbeddingValuesForCallError::new(
                    self.config.provider.clone(),
                    self.model_id.clone(),
                    2048,
                    options.values.clone(),
                )
                .to_string(),
            });
        }

        let google_options = provider_options_for::<GoogleEmbeddingModelOptions>(
            "google",
            options.provider_options.as_ref(),
        )
        .ok()
        .flatten()
        .unwrap_or_default();
        let headers = match google_request_headers(&self.config.settings, options.headers.as_ref())
        {
            Ok(headers) => headers,
            Err(error) => {
                return EmbeddingModelResult::new(Vec::new())
                    .with_warning(Warning::Other { message: error });
            }
        };

        let is_single = options.values.len() == 1;
        let body = if is_single {
            let parts = embedding_parts(
                &options.values[0],
                google_options.content.as_ref().and_then(|v| v.first()),
            );
            json!({
                "model": format!("models/{}", self.model_id),
                "content": { "parts": parts },
                "outputDimensionality": google_options.output_dimensionality,
                "taskType": google_options.task_type,
            })
        } else {
            json!({
                "requests": options.values.iter().enumerate().map(|(index, value)| {
                    let parts = embedding_parts(value, google_options.content.as_ref().and_then(|v| v.get(index)));
                    json!({
                        "model": format!("models/{}", self.model_id),
                        "content": { "role": "user", "parts": parts },
                        "outputDimensionality": google_options.output_dimensionality,
                        "taskType": google_options.task_type,
                    })
                }).collect::<Vec<_>>()
            })
        };
        let endpoint = if is_single {
            "embedContent"
        } else {
            "batchEmbedContents"
        };
        let post_options = PostJsonToApiOptions::new(
            format!(
                "{}/models/{}:{}",
                self.config.base_url, self.model_id, endpoint
            ),
            strip_nulls(body),
        )
        .with_headers(headers)
        .with_environment(RuntimeEnvironment::unknown())
        .with_optional_abort_signal(options.abort_signal.clone());

        let transport = Arc::clone(&self.config.transport);
        let result = post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    |value| Ok::<JsonValue, String>(value.clone()),
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            google_failed_response_handler,
        )
        .await;

        match result {
            Ok(response) => {
                let embeddings = if is_single {
                    vec![
                        response
                            .value
                            .pointer("/embedding/values")
                            .and_then(JsonValue::as_array)
                            .map(|values| numbers_to_f64(values))
                            .unwrap_or_default(),
                    ]
                } else {
                    response
                        .value
                        .get("embeddings")
                        .and_then(JsonValue::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .map(|item| {
                                    item.get("values")
                                        .and_then(JsonValue::as_array)
                                        .map(|values| numbers_to_f64(values))
                                        .unwrap_or_default()
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                };
                EmbeddingModelResult::new(embeddings).with_response(
                    ai_sdk_rust::EmbeddingModelResponse::new()
                        .with_body(response.raw_value.unwrap_or(JsonValue::Null)),
                )
            }
            Err(error) => EmbeddingModelResult::new(Vec::new()).with_warning(Warning::Other {
                message: format!("{error:?}"),
            }),
        }
    }
}

impl EmbeddingModel for GoogleEmbeddingModel {
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

/// Google image model.
#[derive(Clone)]
pub struct GoogleImageModel {
    model_id: String,
    settings: GoogleImageSettings,
    config: GoogleImageModelConfig,
}

impl GoogleImageModel {
    fn new(
        model_id: String,
        settings: GoogleImageSettings,
        config: GoogleImageModelConfig,
    ) -> Self {
        Self {
            model_id,
            settings,
            config,
        }
    }

    async fn do_generate_result(&self, options: ImageModelCallOptions) -> ImageModelResult {
        if self.model_id.starts_with("gemini-") {
            self.do_generate_gemini(options).await
        } else {
            self.do_generate_imagen(options).await
        }
    }

    async fn do_generate_imagen(&self, options: ImageModelCallOptions) -> ImageModelResult {
        let mut warnings = Vec::new();
        if options
            .files
            .as_ref()
            .is_some_and(|files| !files.is_empty())
        {
            warnings.push(Warning::Unsupported {
                feature: "image editing".to_string(),
                details: Some("Google Gemini API does not support image editing with Imagen models. Use Google Vertex AI for image editing capabilities.".to_string()),
            });
        }
        if options.mask.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "mask".to_string(),
                details: Some("Google Gemini API does not support image editing with masks. Use Google Vertex AI for image editing capabilities.".to_string()),
            });
        }
        if options.size.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "size".to_string(),
                details: Some(
                    "This model does not support the `size` option. Use `aspectRatio` instead."
                        .to_string(),
                ),
            });
        }
        if options.seed.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "seed".to_string(),
                details: Some(
                    "This model does not support the `seed` option through this provider."
                        .to_string(),
                ),
            });
        }

        let google_options =
            provider_options_for::<JsonObject>("google", Some(&options.provider_options))
                .ok()
                .flatten()
                .unwrap_or_default();
        let mut parameters = JsonObject::new();
        parameters.insert("sampleCount".to_string(), json!(options.n));
        parameters.insert(
            "aspectRatio".to_string(),
            json!(options.aspect_ratio.unwrap_or_else(|| "1:1".to_string())),
        );
        parameters.extend(google_options);

        let body = json!({
            "instances": [{ "prompt": options.prompt }],
            "parameters": parameters,
        });
        let headers = match google_request_headers(&self.config.settings, options.headers.as_ref())
        {
            Ok(headers) => headers,
            Err(error) => {
                return image_error_result(
                    &self.model_id,
                    warnings,
                    error,
                    (self.config.current_date)(),
                );
            }
        };

        let post_options = PostJsonToApiOptions::new(
            format!("{}/models/{}:predict", self.config.base_url, self.model_id),
            strip_nulls(body),
        )
        .with_headers(headers)
        .with_environment(RuntimeEnvironment::unknown())
        .with_optional_abort_signal(options.abort_signal.clone());

        let transport = Arc::clone(&self.config.transport);
        let result = post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    |value| Ok::<JsonValue, String>(value.clone()),
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            google_failed_response_handler,
        )
        .await;

        match result {
            Ok(response) => {
                let images = response
                    .value
                    .get("predictions")
                    .and_then(JsonValue::as_array)
                    .map(|predictions| {
                        predictions
                            .iter()
                            .filter_map(|prediction| {
                                prediction
                                    .get("bytesBase64Encoded")
                                    .and_then(JsonValue::as_str)
                            })
                            .map(|data| FileDataContent::Base64(data.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let metadata = ImageModelProviderMetadata::from([(
                    "google".to_string(),
                    ImageModelProviderMetadataEntry::new(vec![json!({}); images.len()]),
                )]);
                let mut result = ImageModelResult::new(
                    images,
                    ImageModelResponse::new((self.config.current_date)(), self.model_id.clone()),
                )
                .with_provider_metadata(metadata);
                for warning in warnings {
                    result = result.with_warning(warning);
                }
                result
            }
            Err(error) => image_error_result(
                &self.model_id,
                warnings,
                format!("{error:?}"),
                (self.config.current_date)(),
            ),
        }
    }

    async fn do_generate_gemini(&self, options: ImageModelCallOptions) -> ImageModelResult {
        let mut warnings = Vec::new();
        if options.mask.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "mask".to_string(),
                details: Some(
                    "Gemini image models do not support mask-based image editing.".to_string(),
                ),
            });
        }
        if options.n > 1 {
            warnings.push(Warning::Unsupported {
                feature: "n".to_string(),
                details: Some("Gemini image models do not support generating a set number of images per call. Use n=1 or omit the n parameter.".to_string()),
            });
        }
        if options.size.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "size".to_string(),
                details: Some(
                    "This model does not support the `size` option. Use `aspectRatio` instead."
                        .to_string(),
                ),
            });
        }

        let mut user_parts = Vec::new();
        if let Some(prompt) = options.prompt.clone() {
            user_parts.push(LanguageModelUserContentPart::Text(
                LanguageModelTextPart::new(prompt),
            ));
        }
        if let Some(files) = options.files.clone() {
            for file in files {
                let part = match file {
                    ImageModelFile::File {
                        media_type, data, ..
                    } => LanguageModelFilePart::new(FileData::Data { data }, media_type),
                    ImageModelFile::Url { url, .. } => {
                        LanguageModelFilePart::new(FileData::Url { url }, "image/*")
                    }
                };
                user_parts.push(LanguageModelUserContentPart::File(part));
            }
        }

        let provider_options: ProviderOptions = serde_json::from_value(strip_nulls(json!({
            "google": {
                "responseModalities": ["IMAGE"],
                "imageConfig": options.aspect_ratio.as_ref().map(|aspect_ratio| json!({ "aspectRatio": aspect_ratio })),
            }
        }))).expect("provider options shape is valid");

        let lm_options = LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
            ai_sdk_rust::LanguageModelUserMessage::new(user_parts),
        )])
        .with_provider_options(provider_options);
        let result = self
            .config
            .clone()
            .into_language_model(self.model_id.clone())
            .do_generate_result(lm_options)
            .await;
        let images = result
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelContent::File(file) if file.media_type.starts_with("image/") => {
                    match &file.data {
                        LanguageModelFileData::Data { data } => Some(data.clone()),
                        LanguageModelFileData::Url { .. } => None,
                    }
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        let mut image_result = ImageModelResult::new(
            images.clone(),
            ImageModelResponse::new((self.config.current_date)(), self.model_id.clone()),
        )
        .with_provider_metadata(ImageModelProviderMetadata::from([(
            "google".to_string(),
            ImageModelProviderMetadataEntry::new(vec![json!({}); images.len()]),
        )]));

        if let (Some(input), Some(output)) = (
            result.usage.input_tokens.total,
            result.usage.output_tokens.total,
        ) {
            image_result = image_result.with_usage(
                ImageModelUsage::new()
                    .with_input_tokens(input)
                    .with_output_tokens(output)
                    .with_total_tokens(input + output),
            );
        }

        for warning in warnings {
            image_result = image_result.with_warning(warning);
        }
        image_result
    }
}

impl GoogleImageModelConfig {
    fn into_language_model(self, model_id: String) -> GoogleLanguageModel {
        GoogleLanguageModel::new(
            model_id,
            GoogleLanguageModelConfig {
                provider: self.provider,
                base_url: self.base_url,
                settings: self.settings,
                transport: self.transport,
                generate_id: self.generate_id,
            },
        )
    }
}

impl ImageModel for GoogleImageModel {
    type MaxImagesPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        ready(Some(self.settings.max_images_per_call.unwrap_or_else(
            || {
                if self.model_id.starts_with("gemini-") {
                    10
                } else {
                    4
                }
            },
        )))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleVideoModelOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_interval_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_images: Option<Vec<JsonValue>>,
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Google video model.
#[derive(Clone)]
pub struct GoogleVideoModel {
    model_id: String,
    config: GoogleModelConfig,
}

impl GoogleVideoModel {
    fn new(model_id: String, config: GoogleModelConfig) -> Self {
        Self { model_id, config }
    }

    async fn do_generate_result(&self, options: VideoModelCallOptions) -> VideoModelResult {
        let google_options = provider_options_for::<GoogleVideoModelOptions>(
            "google",
            Some(&options.provider_options),
        )
        .ok()
        .flatten()
        .unwrap_or_default();
        let mut warnings = Vec::new();
        let mut instance = JsonObject::new();
        insert_opt(&mut instance, "prompt", options.prompt.clone());
        if let Some(image) = options.image {
            match image {
                VideoModelFile::Url { .. } => warnings.push(Warning::Unsupported {
                    feature: "URL-based image input".to_string(),
                    details: Some("Google Generative AI video models require base64-encoded images. URL will be ignored.".to_string()),
                }),
                VideoModelFile::File { media_type, data, .. } => {
                    instance.insert("image".to_string(), json!({
                        "inlineData": {
                            "mimeType": media_type,
                            "data": convert_to_base64(&data),
                        }
                    }));
                }
            }
        }
        insert_opt(
            &mut instance,
            "referenceImages",
            google_options.reference_images.clone(),
        );

        let mut parameters = JsonObject::new();
        parameters.insert("sampleCount".to_string(), json!(options.n));
        insert_opt(&mut parameters, "aspectRatio", options.aspect_ratio.clone());
        if let Some(resolution) = options.resolution.clone() {
            parameters.insert(
                "resolution".to_string(),
                json!(google_video_resolution(&resolution)),
            );
        }
        insert_opt(&mut parameters, "durationSeconds", options.duration);
        insert_opt(&mut parameters, "seed", options.seed);
        insert_opt(
            &mut parameters,
            "personGeneration",
            google_options.person_generation.clone(),
        );
        insert_opt(
            &mut parameters,
            "negativePrompt",
            google_options.negative_prompt.clone(),
        );
        parameters.extend(google_options.extra.clone());

        let body = json!({ "instances": [instance], "parameters": parameters });
        let headers = match google_request_headers(&self.config.settings, options.headers.as_ref())
        {
            Ok(headers) => headers,
            Err(error) => {
                return video_error_result(
                    &self.model_id,
                    warnings,
                    error,
                    (self.config.current_date)(),
                );
            }
        };

        let post_options = PostJsonToApiOptions::new(
            format!(
                "{}/models/{}:predictLongRunning",
                self.config.base_url, self.model_id
            ),
            strip_nulls(body),
        )
        .with_headers(headers.clone())
        .with_environment(RuntimeEnvironment::unknown())
        .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.config.transport);
        let result = post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    |value| Ok::<JsonValue, String>(value.clone()),
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            google_failed_response_handler,
        )
        .await;

        let mut operation = match result {
            Ok(response) => response.value,
            Err(error) => {
                return video_error_result(
                    &self.model_id,
                    warnings,
                    format!("{error:?}"),
                    (self.config.current_date)(),
                );
            }
        };

        for _ in 0..3 {
            if operation
                .get("done")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false)
            {
                break;
            }
            let Some(name) = operation
                .get("name")
                .and_then(JsonValue::as_str)
                .map(str::to_string)
            else {
                return video_error_result(
                    &self.model_id,
                    warnings,
                    "No operation name returned from API",
                    (self.config.current_date)(),
                );
            };
            let get_options =
                ai_sdk_rust::GetFromApiOptions::new(format!("{}/{}", self.config.base_url, name))
                    .with_headers(headers.clone())
                    .with_environment(RuntimeEnvironment::unknown())
                    .with_optional_abort_signal(options.abort_signal.clone());
            let transport = Arc::clone(&self.config.transport);
            let poll = get_from_api(
                get_options,
                move |request| (transport)(request),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        |value| Ok::<JsonValue, String>(value.clone()),
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                google_failed_response_handler,
            )
            .await;
            match poll {
                Ok(response) => operation = response.value,
                Err(error) => {
                    return video_error_result(
                        &self.model_id,
                        warnings,
                        format!("{error:?}"),
                        (self.config.current_date)(),
                    );
                }
            }
        }

        if let Some(error) = operation.get("error") {
            return video_error_result(
                &self.model_id,
                warnings,
                format!("Video generation failed: {error}"),
                (self.config.current_date)(),
            );
        }

        let api_key = google_api_key(self.config.settings.api_key.as_ref()).ok();
        let samples = operation
            .pointer("/response/generateVideoResponse/generatedSamples")
            .and_then(JsonValue::as_array)
            .cloned()
            .unwrap_or_default();
        let mut videos = Vec::new();
        let mut metadata = Vec::new();
        for sample in samples {
            if let Some(uri) = sample.pointer("/video/uri").and_then(JsonValue::as_str) {
                let url = append_key_to_url(uri, api_key.as_deref());
                if let Ok(url) = Url::parse(&url) {
                    videos.push(VideoModelVideoData::url(url, "video/mp4"));
                    metadata.push(json!({ "uri": uri }));
                }
            }
        }
        let mut provider_metadata = ProviderMetadata::new();
        provider_metadata.insert(
            "google".to_string(),
            JsonObject::from_iter([("videos".to_string(), JsonValue::Array(metadata))]),
        );
        let mut result = VideoModelResult::new(
            videos,
            VideoModelResponse::new((self.config.current_date)(), self.model_id.clone()),
        )
        .with_provider_metadata(provider_metadata);
        for warning in warnings {
            result = result.with_warning(warning);
        }
        result
    }
}

impl VideoModel for GoogleVideoModel {
    type MaxVideosPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = VideoModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
        ready(Some(4))
    }

    fn do_generate(&self, options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleFilesUploadOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_interval_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_timeout_ms: Option<u64>,
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Google files upload interface.
#[derive(Clone)]
pub struct GoogleFiles {
    config: GoogleModelConfig,
}

impl GoogleFiles {
    fn new(config: GoogleModelConfig) -> Self {
        Self { config }
    }

    async fn upload_file_result(
        &self,
        options: FilesUploadFileCallOptions,
    ) -> FilesUploadFileResult {
        let google_options = provider_options_for::<GoogleFilesUploadOptions>(
            "google",
            options.provider_options.as_ref(),
        )
        .ok()
        .flatten()
        .unwrap_or_default();
        let mut warnings = Vec::new();
        if options.filename.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "filename".to_string(),
                details: None,
            });
        }
        let bytes = upload_file_bytes(&options.data);
        let headers = match google_request_headers(&self.config.settings, None) {
            Ok(headers) => headers,
            Err(error) => {
                return files_error_result(&options.media_type, warnings, error);
            }
        };
        let origin = self
            .config
            .base_url
            .strip_suffix("/v1beta")
            .unwrap_or(&self.config.base_url);
        let init_body = strip_nulls(json!({
            "file": {
                "display_name": google_options.display_name,
            }
        }));
        let mut upload_headers = headers.clone();
        upload_headers.insert(
            "X-Goog-Upload-Protocol".to_string(),
            Some("resumable".to_string()),
        );
        upload_headers.insert(
            "X-Goog-Upload-Command".to_string(),
            Some("start".to_string()),
        );
        upload_headers.insert(
            "X-Goog-Upload-Header-Content-Length".to_string(),
            Some(bytes.len().to_string()),
        );
        upload_headers.insert(
            "X-Goog-Upload-Header-Content-Type".to_string(),
            Some(options.media_type.clone()),
        );
        upload_headers.insert(
            "Content-Type".to_string(),
            Some("application/json".to_string()),
        );

        let post_options =
            PostJsonToApiOptions::new(format!("{origin}/upload/v1beta/files"), init_body)
                .with_headers(upload_headers)
                .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.config.transport);
        let init = post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    |value| Ok::<JsonValue, String>(value.clone()),
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            google_failed_response_handler,
        )
        .await;
        let init = match init {
            Ok(response) => response,
            Err(error) => {
                return files_error_result(&options.media_type, warnings, format!("{error:?}"));
            }
        };
        let Some(upload_url) = init
            .response_headers
            .as_ref()
            .and_then(|headers| headers.get("x-goog-upload-url"))
            .cloned()
        else {
            return files_error_result(
                &options.media_type,
                warnings,
                "No upload URL returned from initiation request",
            );
        };

        let upload_request = ProviderApiRequest::post(
            upload_url,
            Headers::from([
                ("Content-Length".to_string(), bytes.len().to_string()),
                ("X-Goog-Upload-Offset".to_string(), "0".to_string()),
                (
                    "X-Goog-Upload-Command".to_string(),
                    "upload, finalize".to_string(),
                ),
            ]),
            ProviderApiRequestBody::bytes(bytes),
            JsonValue::Object(JsonObject::new()),
        );
        let upload = (self.config.transport)(upload_request).await;
        let upload = match upload {
            Ok(response) => response,
            Err(error) => {
                return files_error_result(
                    &options.media_type,
                    warnings,
                    error.message().to_string(),
                );
            }
        };
        let upload_json = match upload
            .body
            .as_ref()
            .and_then(|body| body.as_text())
            .and_then(|text| serde_json::from_str::<JsonValue>(text).ok())
        {
            Some(value) => value,
            None => {
                return files_error_result(
                    &options.media_type,
                    warnings,
                    "Invalid upload response JSON",
                );
            }
        };
        let file = upload_json.get("file").cloned().unwrap_or(upload_json);
        files_result_from_resource(file, options.media_type, warnings)
    }
}

impl Files for GoogleFiles {
    type UploadFileFuture<'a>
        = Pin<Box<dyn Future<Output = FilesUploadFileResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn upload_file(&self, options: FilesUploadFileCallOptions) -> Self::UploadFileFuture<'_> {
        Box::pin(self.upload_file_result(options))
    }
}

/// Gemini Interactions model input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GoogleInteractionsModelInput {
    /// Model id branch.
    Model(String),
    /// Agent preset branch.
    Agent(String),
}

impl From<String> for GoogleInteractionsModelInput {
    fn from(value: String) -> Self {
        Self::Model(value)
    }
}

impl From<&str> for GoogleInteractionsModelInput {
    fn from(value: &str) -> Self {
        Self::Model(value.to_string())
    }
}

impl GoogleInteractionsModelInput {
    /// Creates an agent input.
    pub fn agent(name: impl Into<String>) -> Self {
        Self::Agent(name.into())
    }
}

/// Google Interactions language model.
#[derive(Clone)]
pub struct GoogleInteractionsLanguageModel {
    model_id: String,
    agent: Option<String>,
    config: GoogleLanguageModelConfig,
}

impl GoogleInteractionsLanguageModel {
    fn new(input: GoogleInteractionsModelInput, config: GoogleLanguageModelConfig) -> Self {
        match input {
            GoogleInteractionsModelInput::Model(model_id) => Self {
                model_id,
                agent: None,
                config,
            },
            GoogleInteractionsModelInput::Agent(agent) => Self {
                model_id: agent.clone(),
                agent: Some(agent),
                config,
            },
        }
    }

    fn get_args(
        &self,
        options: &LanguageModelCallOptions,
    ) -> Result<(JsonValue, Vec<Warning>), String> {
        let mut warnings = Vec::new();
        let google_options =
            provider_options_for::<JsonObject>("google", options.provider_options.as_ref())?
                .unwrap_or_default();
        let converted =
            convert_to_google_interactions_input(&options.prompt, &google_options, &mut warnings)?;

        let mut body = JsonObject::new();
        if let Some(agent) = &self.agent {
            body.insert("agent".to_string(), json!(agent));
            if options
                .tools
                .as_ref()
                .is_some_and(|tools| !tools.is_empty())
            {
                warnings.push(Warning::Other {
                    message: "google.interactions: tools are not supported when an agent is set; tools will be omitted from the request body.".to_string(),
                });
            }
        } else {
            body.insert("model".to_string(), json!(self.model_id));
            if let Some((tools, tool_choice, tool_warnings)) = prepare_google_interactions_tools(
                options.tools.as_deref(),
                options.tool_choice.as_ref(),
            ) {
                body.insert("tools".to_string(), JsonValue::Array(tools));
                insert_opt(&mut body, "toolChoice", tool_choice);
                warnings.extend(tool_warnings);
            }
        }
        body.insert("input".to_string(), converted.input);
        insert_opt(&mut body, "systemInstruction", converted.system_instruction);
        insert_opt(
            &mut body,
            "previousInteractionId",
            google_options.get("previousInteractionId").cloned(),
        );
        insert_opt(&mut body, "store", google_options.get("store").cloned());
        Ok((strip_nulls(JsonValue::Object(body)), warnings))
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let (body, warnings) = match self.get_args(&options) {
            Ok(args) => args,
            Err(error) => return google_error_generate_result(&self.model_id, &error, json!({})),
        };
        let headers = match google_request_headers(&self.config.settings, options.headers.as_ref())
        {
            Ok(headers) => headers,
            Err(error) => return google_error_generate_result(&self.model_id, &error, body),
        };
        let request_body = body.clone();
        let post_options =
            PostJsonToApiOptions::new(format!("{}/interactions", self.config.base_url), body)
                .with_headers(headers)
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.config.transport);
        let result = post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    |value| Ok::<JsonValue, String>(value.clone()),
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            google_failed_response_handler,
        )
        .await;

        match result {
            Ok(response) => google_interactions_generate_result(
                response.value,
                response.raw_value.unwrap_or(JsonValue::Null),
                request_body,
                warnings,
                &self.config.generate_id,
            ),
            Err(error) => {
                google_error_generate_result(&self.model_id, &format!("{error:?}"), request_body)
            }
        }
    }

    async fn do_stream_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let result = self.do_generate_result(options).await;
        let mut stream = vec![LanguageModelStreamPart::StreamStart(
            LanguageModelStreamStart::new(result.warnings.clone()),
        )];
        for part in result.content {
            stream.push(content_to_stream_part(part));
        }
        stream.push(LanguageModelStreamPart::Finish(
            LanguageModelStreamFinish::new(result.usage, result.finish_reason),
        ));
        LanguageModelStreamResult::new(stream).with_request(result.request.unwrap_or_default())
    }
}

impl LanguageModel for GoogleInteractionsLanguageModel {
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

    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(BTreeMap::from([
            ("image/*".to_string(), vec!["^https?:\\/\\/.+".to_string()]),
            (
                "application/pdf".to_string(),
                vec!["^https?:\\/\\/.+".to_string()],
            ),
            ("audio/*".to_string(), vec!["^https?:\\/\\/.+".to_string()]),
            (
                "video/*".to_string(),
                vec![
                    "^https?:\\/\\/(www\\.)?youtube\\.com\\/watch\\?v=.+".to_string(),
                    "^https?:\\/\\/youtu\\.be\\/.+".to_string(),
                    "^gs:\\/\\/.+".to_string(),
                ],
            ),
        ]))
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(self.do_stream_result(options))
    }
}

/// Provider-defined Google tools.
#[derive(Clone, Copy, Debug, Default)]
pub struct GoogleTools;

impl GoogleTools {
    pub fn google_search(&self, args: JsonObject) -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "google.google_search",
            "google_search",
            args,
        ))
    }

    pub fn enterprise_web_search(&self) -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "google.enterprise_web_search",
            "enterprise_web_search",
            JsonObject::new(),
        ))
    }

    pub fn url_context(&self) -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "google.url_context",
            "url_context",
            JsonObject::new(),
        ))
    }

    pub fn code_execution(&self) -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "google.code_execution",
            "code_execution",
            JsonObject::new(),
        ))
    }

    pub fn file_search(&self, args: JsonObject) -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "google.file_search",
            "file_search",
            args,
        ))
    }

    pub fn google_maps(&self) -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "google.google_maps",
            "google_maps",
            JsonObject::new(),
        ))
    }

    pub fn vertex_rag_store(&self, args: JsonObject) -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "google.vertex_rag_store",
            "vertex_rag_store",
            args,
        ))
    }
}

/// Converts JSON Schema 7 into the OpenAPI Schema subset accepted by Gemini.
pub fn convert_json_schema_to_openapi_schema(schema: Option<&JsonSchema>) -> Option<JsonValue> {
    schema
        .map(|schema| JsonValue::Object(schema.clone()))
        .and_then(|schema| convert_json_schema_to_openapi_schema_value(&schema, true))
}

fn convert_json_schema_to_openapi_schema_value(
    schema: &JsonValue,
    is_root: bool,
) -> Option<JsonValue> {
    if is_empty_object_schema(schema) {
        if is_root {
            return None;
        }
        let mut object = JsonObject::new();
        object.insert("type".to_string(), json!("object"));
        if let Some(description) = schema.get("description") {
            object.insert("description".to_string(), description.clone());
        }
        return Some(JsonValue::Object(object));
    }

    if let JsonValue::Bool(_) = schema {
        return Some(json!({ "type": "boolean", "properties": {} }));
    }

    let JsonValue::Object(input) = schema else {
        return Some(schema.clone());
    };

    let mut result = JsonObject::new();
    copy_key(input, &mut result, "description");
    copy_key(input, &mut result, "required");
    copy_key(input, &mut result, "format");

    if let Some(const_value) = input.get("const") {
        result.insert(
            "enum".to_string(),
            JsonValue::Array(vec![const_value.clone()]),
        );
    }

    if let Some(schema_type) = input.get("type") {
        if let Some(types) = schema_type.as_array() {
            let non_null = types
                .iter()
                .filter(|value| value.as_str() != Some("null"))
                .cloned()
                .collect::<Vec<_>>();
            let has_null = non_null.len() != types.len();
            if non_null.is_empty() {
                result.insert("type".to_string(), json!("null"));
            } else {
                result.insert(
                    "anyOf".to_string(),
                    JsonValue::Array(
                        non_null
                            .into_iter()
                            .map(|value| json!({ "type": value }))
                            .collect(),
                    ),
                );
                if has_null {
                    result.insert("nullable".to_string(), JsonValue::Bool(true));
                }
            }
        } else {
            result.insert("type".to_string(), schema_type.clone());
        }
    }

    copy_key(input, &mut result, "enum");
    if let Some(properties) = input.get("properties").and_then(JsonValue::as_object) {
        result.insert(
            "properties".to_string(),
            JsonValue::Object(
                properties
                    .iter()
                    .filter_map(|(key, value)| {
                        convert_json_schema_to_openapi_schema_value(value, false)
                            .map(|value| (key.clone(), value))
                    })
                    .collect(),
            ),
        );
    }

    if let Some(items) = input.get("items") {
        let converted = if let Some(items) = items.as_array() {
            JsonValue::Array(
                items
                    .iter()
                    .filter_map(|item| convert_json_schema_to_openapi_schema_value(item, false))
                    .collect(),
            )
        } else {
            convert_json_schema_to_openapi_schema_value(items, false)?
        };
        result.insert("items".to_string(), converted);
    }

    for key in ["allOf", "oneOf"] {
        if let Some(values) = input.get(key).and_then(JsonValue::as_array) {
            result.insert(
                key.to_string(),
                JsonValue::Array(
                    values
                        .iter()
                        .filter_map(|value| {
                            convert_json_schema_to_openapi_schema_value(value, false)
                        })
                        .collect(),
                ),
            );
        }
    }

    if let Some(any_of) = input.get("anyOf").and_then(JsonValue::as_array) {
        let non_null = any_of
            .iter()
            .filter(|schema| schema.get("type").and_then(JsonValue::as_str) != Some("null"))
            .collect::<Vec<_>>();
        if non_null.len() != any_of.len() {
            result.insert("nullable".to_string(), JsonValue::Bool(true));
            if non_null.len() == 1 {
                if let Some(JsonValue::Object(converted)) =
                    convert_json_schema_to_openapi_schema_value(non_null[0], false)
                {
                    result.extend(converted);
                }
            } else {
                result.insert(
                    "anyOf".to_string(),
                    JsonValue::Array(
                        non_null
                            .into_iter()
                            .filter_map(|schema| {
                                convert_json_schema_to_openapi_schema_value(schema, false)
                            })
                            .collect(),
                    ),
                );
            }
        } else {
            result.insert(
                "anyOf".to_string(),
                JsonValue::Array(
                    any_of
                        .iter()
                        .filter_map(|schema| {
                            convert_json_schema_to_openapi_schema_value(schema, false)
                        })
                        .collect(),
                ),
            );
        }
    }

    copy_key(input, &mut result, "minLength");
    Some(JsonValue::Object(result))
}

fn is_empty_object_schema(schema: &JsonValue) -> bool {
    let Some(object) = schema.as_object() else {
        return false;
    };
    object.get("type").and_then(JsonValue::as_str) == Some("object")
        && object
            .get("properties")
            .and_then(JsonValue::as_object)
            .is_none_or(Map::is_empty)
        && !object.contains_key("additionalProperties")
}

fn copy_key(input: &JsonObject, output: &mut JsonObject, key: &str) {
    if let Some(value) = input.get(key) {
        output.insert(key.to_string(), value.clone());
    }
}

/// Returns the Google model path, matching upstream `getModelPath`.
pub fn get_model_path(model_id: &str) -> String {
    if model_id.contains('/') {
        model_id.to_string()
    } else {
        format!("models/{model_id}")
    }
}

/// Maps Google finish reasons to AI SDK unified finish reasons.
pub fn map_google_finish_reason(finish_reason: Option<&str>, has_tool_calls: bool) -> FinishReason {
    match finish_reason {
        Some("STOP") => {
            if has_tool_calls {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            }
        }
        Some("MAX_TOKENS") => FinishReason::Length,
        Some(
            "IMAGE_SAFETY" | "RECITATION" | "SAFETY" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII",
        ) => FinishReason::ContentFilter,
        Some("MALFORMED_FUNCTION_CALL") => FinishReason::Error,
        _ => FinishReason::Other,
    }
}

/// Converts Google usage metadata into provider-v4 usage.
pub fn convert_google_usage(usage: Option<&JsonValue>) -> LanguageModelUsage {
    let Some(usage) = usage else {
        return LanguageModelUsage::default();
    };
    let prompt_tokens = json_u64(usage, "promptTokenCount").unwrap_or(0);
    let candidates_tokens = json_u64(usage, "candidatesTokenCount").unwrap_or(0);
    let cached_tokens = json_u64(usage, "cachedContentTokenCount").unwrap_or(0);
    let thought_tokens = json_u64(usage, "thoughtsTokenCount").unwrap_or(0);
    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: Some(prompt_tokens),
            no_cache: Some(prompt_tokens.saturating_sub(cached_tokens)),
            cache_read: Some(cached_tokens),
            cache_write: None,
        },
        output_tokens: OutputTokenUsage {
            total: Some(candidates_tokens + thought_tokens),
            text: Some(candidates_tokens),
            reasoning: Some(thought_tokens),
        },
        raw: usage.as_object().cloned(),
    }
}

/// Returns whether a URL can be passed as a Google file URL.
pub fn is_supported_file_url(url: &Url) -> bool {
    let url_string = url.as_str();
    url_string.starts_with("https://generativelanguage.googleapis.com/v1beta/files/")
        || is_youtube_url(url_string)
}

/// Partial argument emitted by Google's streaming tool-call deltas.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialArg {
    pub json_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub string_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bool_value: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub null_value: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub will_continue: Option<bool>,
}

#[derive(Clone, Debug)]
struct PathStackEntry {
    segment: PathSegment,
    is_array: bool,
    child_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PathSegment {
    Key(String),
    Index(usize),
}

/// Incrementally accumulates Google's `partialArgs` into JSON text and state.
#[derive(Clone, Debug, Default)]
pub struct GoogleJsonAccumulator {
    accumulated_args: JsonValue,
    json_text: String,
    path_stack: Vec<PathStackEntry>,
    string_open: bool,
}

impl GoogleJsonAccumulator {
    /// Creates an empty accumulator.
    pub fn new() -> Self {
        Self {
            accumulated_args: JsonValue::Object(JsonObject::new()),
            json_text: String::new(),
            path_stack: Vec::new(),
            string_open: false,
        }
    }

    /// Processes partial args and returns `(current_json, text_delta)`.
    pub fn process_partial_args(&mut self, partial_args: &[PartialArg]) -> (JsonValue, String) {
        let mut delta = String::new();
        for arg in partial_args {
            let raw_path = arg.json_path.strip_prefix("$.").unwrap_or(&arg.json_path);
            if raw_path.is_empty() {
                continue;
            }
            let segments = parse_json_path(raw_path);
            if segments.is_empty() {
                continue;
            }
            let existing = nested_value(&self.accumulated_args, &segments).cloned();
            if let (Some(text), Some(JsonValue::String(existing))) = (&arg.string_value, existing) {
                set_nested_value(
                    &mut self.accumulated_args,
                    &segments,
                    JsonValue::String(format!("{existing}{text}")),
                );
                delta.push_str(&escape_json_string_fragment(text));
                continue;
            }
            let Some((value, json_value)) = resolve_partial_arg_value(arg) else {
                continue;
            };
            set_nested_value(&mut self.accumulated_args, &segments, value);
            delta.push_str(&self.emit_navigation_to(&segments, arg, &json_value));
        }
        self.json_text.push_str(&delta);
        (self.accumulated_args.clone(), delta)
    }

    /// Finalizes the accumulated JSON and returns `(final_json, closing_delta)`.
    pub fn finalize(&self) -> (String, String) {
        let final_json = serde_json::to_string(&self.accumulated_args).expect("JSON serializes");
        let closing_delta = final_json
            .get(self.json_text.len()..)
            .unwrap_or_default()
            .to_string();
        (final_json, closing_delta)
    }

    fn ensure_root(&mut self) -> String {
        if self.path_stack.is_empty() {
            self.path_stack.push(PathStackEntry {
                segment: PathSegment::Key(String::new()),
                is_array: false,
                child_count: 0,
            });
            "{".to_string()
        } else {
            String::new()
        }
    }

    fn emit_navigation_to(
        &mut self,
        target: &[PathSegment],
        arg: &PartialArg,
        value_json: &str,
    ) -> String {
        let mut fragment = String::new();
        if self.string_open {
            fragment.push('"');
            self.string_open = false;
        }
        fragment.push_str(&self.ensure_root());
        let containers = &target[..target.len() - 1];
        let leaf = target.last().expect("target has leaf");
        let common_depth = self.find_common_stack_depth(containers);
        fragment.push_str(&self.close_down_to(common_depth));
        fragment.push_str(&self.open_down_to(containers, leaf));
        fragment.push_str(&self.emit_leaf(leaf, arg, value_json));
        fragment
    }

    fn find_common_stack_depth(&self, target_containers: &[PathSegment]) -> usize {
        let max_depth = std::cmp::min(
            self.path_stack.len().saturating_sub(1),
            target_containers.len(),
        );
        let mut common = 0;
        for (index, target) in target_containers.iter().take(max_depth).enumerate() {
            if &self.path_stack[index + 1].segment == target {
                common += 1;
            } else {
                break;
            }
        }
        common + 1
    }

    fn close_down_to(&mut self, target_depth: usize) -> String {
        let mut fragment = String::new();
        while self.path_stack.len() > target_depth {
            if let Some(entry) = self.path_stack.pop() {
                fragment.push(if entry.is_array { ']' } else { '}' });
            }
        }
        fragment
    }

    fn open_down_to(&mut self, target_containers: &[PathSegment], leaf: &PathSegment) -> String {
        let mut fragment = String::new();
        let start_index = self.path_stack.len().saturating_sub(1);
        for index in start_index..target_containers.len() {
            let segment = target_containers[index].clone();
            let parent = self.path_stack.last_mut().expect("root exists");
            if parent.child_count > 0 {
                fragment.push(',');
            }
            parent.child_count += 1;
            if let PathSegment::Key(key) = &segment {
                fragment.push_str(&serde_json::to_string(key).expect("key serializes"));
                fragment.push(':');
            }
            let child_segment = target_containers.get(index + 1).unwrap_or(leaf);
            let is_array = matches!(child_segment, PathSegment::Index(_));
            fragment.push(if is_array { '[' } else { '{' });
            self.path_stack.push(PathStackEntry {
                segment,
                is_array,
                child_count: 0,
            });
        }
        fragment
    }

    fn emit_leaf(&mut self, leaf: &PathSegment, arg: &PartialArg, value_json: &str) -> String {
        let mut fragment = String::new();
        let container = self.path_stack.last_mut().expect("container exists");
        if container.child_count > 0 {
            fragment.push(',');
        }
        container.child_count += 1;
        if let PathSegment::Key(key) = leaf {
            fragment.push_str(&serde_json::to_string(key).expect("key serializes"));
            fragment.push(':');
        }
        if arg.string_value.is_some() && arg.will_continue == Some(true) {
            fragment.push_str(value_json.strip_suffix('"').unwrap_or(value_json));
            self.string_open = true;
        } else {
            fragment.push_str(value_json);
        }
        fragment
    }
}

struct GooglePrompt {
    contents: Vec<JsonValue>,
    system_instruction: Option<JsonValue>,
}

struct ConvertToGoogleMessagesOptions {
    is_gemma_model: bool,
    provider_options_names: Vec<&'static str>,
    supports_function_response_parts: bool,
}

fn convert_to_google_messages(
    prompt: &LanguageModelPrompt,
    options: ConvertToGoogleMessagesOptions,
) -> Result<GooglePrompt, String> {
    let mut system_instruction_parts = Vec::new();
    let mut contents = Vec::new();
    let mut system_messages_allowed = true;
    let is_vertex_like = !options.provider_options_names.contains(&"google");

    for message in prompt {
        match message {
            LanguageModelMessage::System(message) => {
                if !system_messages_allowed {
                    return Err(
                        "system messages are only supported at the beginning of the conversation"
                            .to_string(),
                    );
                }
                system_instruction_parts.push(json!({ "text": message.content }));
            }
            LanguageModelMessage::User(message) => {
                system_messages_allowed = false;
                let mut parts = Vec::new();
                for part in &message.content {
                    match part {
                        LanguageModelUserContentPart::Text(text) => {
                            parts.push(json!({ "text": text.text }))
                        }
                        LanguageModelUserContentPart::File(file) => {
                            parts.push(convert_file_part_to_google(file, is_vertex_like, false)?);
                        }
                    }
                }
                contents.push(json!({ "role": "user", "parts": parts }));
            }
            LanguageModelMessage::Assistant(message) => {
                system_messages_allowed = false;
                let mut parts = Vec::new();
                for part in &message.content {
                    let converted = convert_assistant_part_to_google(
                        part,
                        &options.provider_options_names,
                        is_vertex_like,
                    )?;
                    if let Some(converted) = converted {
                        parts.push(converted);
                    }
                }
                contents.push(json!({ "role": "model", "parts": parts }));
            }
            LanguageModelMessage::Tool(message) => {
                system_messages_allowed = false;
                let mut parts = Vec::new();
                for part in &message.content {
                    if let LanguageModelToolContentPart::ToolResult(result) = part {
                        if let Some(server_response) =
                            server_tool_response_part(result, &options.provider_options_names)
                        {
                            if let Some(last) = contents.last_mut() {
                                if last.get("role").and_then(JsonValue::as_str) == Some("model") {
                                    if let Some(last_parts) =
                                        last.get_mut("parts").and_then(JsonValue::as_array_mut)
                                    {
                                        last_parts.push(server_response);
                                        continue;
                                    }
                                }
                            }
                        }
                        append_tool_result_parts(
                            &mut parts,
                            result,
                            options.supports_function_response_parts,
                        );
                    }
                }
                contents.push(json!({ "role": "user", "parts": parts }));
            }
        }
    }

    if options.is_gemma_model && !system_instruction_parts.is_empty() {
        if let Some(first) = contents.first_mut() {
            if first.get("role").and_then(JsonValue::as_str) == Some("user") {
                let system_text = system_instruction_parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(JsonValue::as_str))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if let Some(parts) = first.get_mut("parts").and_then(JsonValue::as_array_mut) {
                    parts.insert(0, json!({ "text": format!("{system_text}\n\n") }));
                }
            }
        }
    }

    let system_instruction = if !system_instruction_parts.is_empty() && !options.is_gemma_model {
        Some(json!({ "parts": system_instruction_parts }))
    } else {
        None
    };
    Ok(GooglePrompt {
        contents,
        system_instruction,
    })
}

fn convert_file_part_to_google(
    part: &LanguageModelFilePart,
    is_vertex_like: bool,
    allow_thought: bool,
) -> Result<JsonValue, String> {
    let mut value = match &part.data {
        FileData::Url { url } => json!({
            "fileData": {
                "mimeType": full_media_type(&part.media_type, None),
                "fileUri": url.as_str(),
            }
        }),
        FileData::Reference { reference } => {
            if is_vertex_like {
                return Err("file parts with provider references".to_string());
            }
            json!({
                "fileData": {
                    "mimeType": full_media_type(&part.media_type, None),
                    "fileUri": resolve_provider_reference(reference, "google").map_err(|error| error.to_string())?,
                }
            })
        }
        FileData::Text { text } => json!({
            "inlineData": {
                "mimeType": if is_full_media_type(&part.media_type) { part.media_type.clone() } else { "text/plain".to_string() },
                "data": convert_to_base64(&FileDataContent::Bytes(text.as_bytes().to_vec())),
            }
        }),
        FileData::Data { data } => json!({
            "inlineData": {
                "mimeType": full_media_type(&part.media_type, Some(data)),
                "data": convert_to_base64(data),
            }
        }),
    };
    if allow_thought {
        if let Some(provider_options) = part_provider_options(
            part.provider_options.as_ref(),
            &["google", "googleVertex", "vertex"],
        ) {
            if provider_options.get("thought").and_then(JsonValue::as_bool) == Some(true) {
                value["thought"] = JsonValue::Bool(true);
            }
        }
    }
    Ok(value)
}

fn convert_assistant_part_to_google(
    part: &LanguageModelAssistantContentPart,
    provider_names: &[&str],
    is_vertex_like: bool,
) -> Result<Option<JsonValue>, String> {
    match part {
        LanguageModelAssistantContentPart::Text(text) => {
            if text.text.is_empty() {
                return Ok(None);
            }
            let mut value = json!({ "text": text.text });
            if let Some(signature) =
                thought_signature(text.provider_options.as_ref(), provider_names)
            {
                value["thoughtSignature"] = json!(signature);
            }
            Ok(Some(value))
        }
        LanguageModelAssistantContentPart::Reasoning(reasoning) => {
            if reasoning.text.is_empty() {
                return Ok(None);
            }
            let mut value = json!({ "text": reasoning.text, "thought": true });
            if let Some(signature) =
                thought_signature(reasoning.provider_options.as_ref(), provider_names)
            {
                value["thoughtSignature"] = json!(signature);
            }
            Ok(Some(value))
        }
        LanguageModelAssistantContentPart::ReasoningFile(file) => match &file.data {
            LanguageModelFileData::Url { .. } => {
                Err("File data URLs in assistant messages are not supported".to_string())
            }
            LanguageModelFileData::Data { data } => {
                let mut value = json!({
                    "inlineData": {
                        "mimeType": file.media_type,
                        "data": convert_to_base64(data),
                    },
                    "thought": true,
                });
                if let Some(signature) =
                    thought_signature(file.provider_options.as_ref(), provider_names)
                {
                    value["thoughtSignature"] = json!(signature);
                }
                Ok(Some(value))
            }
        },
        LanguageModelAssistantContentPart::File(file) => {
            if matches!(file.data, FileData::Url { .. }) {
                return Err("File data URLs in assistant messages are not supported".to_string());
            }
            let mut value = convert_file_part_to_google(file, is_vertex_like, true)?;
            if let Some(signature) =
                thought_signature(file.provider_options.as_ref(), provider_names)
            {
                value["thoughtSignature"] = json!(signature);
            }
            Ok(Some(value))
        }
        LanguageModelAssistantContentPart::ToolCall(tool_call) => {
            let provider_opts =
                part_provider_options(tool_call.provider_options.as_ref(), provider_names);
            if let Some(opts) = provider_opts {
                if let (Some(server_id), Some(server_type)) = (
                    opts.get("serverToolCallId").and_then(JsonValue::as_str),
                    opts.get("serverToolType").and_then(JsonValue::as_str),
                ) {
                    let mut value = json!({
                        "toolCall": {
                            "toolType": server_type,
                            "args": parse_tool_input(&tool_call.input),
                            "id": server_id,
                        }
                    });
                    if let Some(signature) =
                        opts.get("thoughtSignature").and_then(JsonValue::as_str)
                    {
                        value["thoughtSignature"] = json!(signature);
                    }
                    return Ok(Some(value));
                }
            }
            let mut value = json!({
                "functionCall": {
                    "id": tool_call.tool_call_id,
                    "name": tool_call.tool_name,
                    "args": tool_call.input,
                }
            });
            if let Some(signature) =
                thought_signature(tool_call.provider_options.as_ref(), provider_names)
            {
                value["thoughtSignature"] = json!(signature);
            }
            Ok(Some(value))
        }
        LanguageModelAssistantContentPart::ToolResult(result) => {
            Ok(server_tool_response_part(result, provider_names))
        }
        LanguageModelAssistantContentPart::Custom(_)
        | LanguageModelAssistantContentPart::ToolApprovalRequest(_) => Ok(None),
    }
}

fn append_tool_result_parts(
    parts: &mut Vec<JsonValue>,
    result: &ai_sdk_rust::LanguageModelToolResultPart,
    supports_response_parts: bool,
) {
    if let LanguageModelToolResultOutput::Content { value } = &result.output {
        if supports_response_parts {
            let mut response_text = Vec::new();
            let mut function_parts = Vec::new();
            for content in value {
                match content {
                    ai_sdk_rust::LanguageModelToolResultContentPart::Text(text) => {
                        response_text.push(text.text.clone())
                    }
                    ai_sdk_rust::LanguageModelToolResultContentPart::File(file) => match &file.data
                    {
                        FileData::Data { data } => function_parts.push(json!({
                            "inlineData": {
                                "mimeType": full_media_type(&file.media_type, Some(data)),
                                "data": convert_to_base64(data),
                            }
                        })),
                        FileData::Url { url } => {
                            if let Some(data) = parse_base64_data_url(url.as_str()) {
                                function_parts.push(json!({
                                    "inlineData": { "mimeType": data.0, "data": data.1 }
                                }));
                            } else {
                                response_text.push(
                                    serde_json::to_string(content).expect("content serializes"),
                                );
                            }
                        }
                        _ => response_text
                            .push(serde_json::to_string(content).expect("content serializes")),
                    },
                    _ => response_text
                        .push(serde_json::to_string(content).expect("content serializes")),
                }
            }
            let mut function_response = json!({
                "name": result.tool_name,
                "response": {
                    "name": result.tool_name,
                    "content": if response_text.is_empty() { "Tool executed successfully.".to_string() } else { response_text.join("\n") },
                }
            });
            if !result.tool_call_id.is_empty() {
                function_response["id"] = json!(result.tool_call_id);
            }
            if !function_parts.is_empty() {
                function_response["parts"] = JsonValue::Array(function_parts);
            }
            parts.push(json!({ "functionResponse": function_response }));
            return;
        }

        for content in value {
            match content {
                ai_sdk_rust::LanguageModelToolResultContentPart::Text(text) => parts.push(json!({
                    "functionResponse": {
                        "id": result.tool_call_id,
                        "name": result.tool_name,
                        "response": { "name": result.tool_name, "content": text.text },
                    }
                })),
                ai_sdk_rust::LanguageModelToolResultContentPart::File(file)
                    if matches!(file.data, FileData::Data { .. })
                        && get_top_level_media_type(&file.media_type) == "image" =>
                {
                    if let FileData::Data { data } = &file.data {
                        parts.push(json!({
                            "inlineData": {
                                "mimeType": full_media_type(&file.media_type, Some(data)),
                                "data": convert_to_base64(data),
                            }
                        }));
                        parts.push(json!({ "text": "Tool executed successfully and returned this image as a response" }));
                    }
                }
                _ => parts.push(
                    json!({ "text": serde_json::to_string(content).expect("content serializes") }),
                ),
            }
        }
        return;
    }

    let content = match &result.output {
        LanguageModelToolResultOutput::Text { value, .. }
        | LanguageModelToolResultOutput::ErrorText { value, .. } => {
            JsonValue::String(value.clone())
        }
        LanguageModelToolResultOutput::Json { value, .. }
        | LanguageModelToolResultOutput::ErrorJson { value, .. } => value.clone(),
        LanguageModelToolResultOutput::ExecutionDenied { reason, .. } => JsonValue::String(
            reason
                .clone()
                .unwrap_or_else(|| "Tool call execution denied.".to_string()),
        ),
        LanguageModelToolResultOutput::Content { .. } => unreachable!("content handled above"),
    };
    parts.push(json!({
        "functionResponse": {
            "id": result.tool_call_id,
            "name": result.tool_name,
            "response": {
                "name": result.tool_name,
                "content": content,
            }
        }
    }));
}

fn server_tool_response_part(
    result: &ai_sdk_rust::LanguageModelToolResultPart,
    provider_names: &[&str],
) -> Option<JsonValue> {
    let opts = part_provider_options(result.provider_options.as_ref(), provider_names)?;
    let server_id = opts.get("serverToolCallId").and_then(JsonValue::as_str)?;
    let server_type = opts.get("serverToolType").and_then(JsonValue::as_str)?;
    let response = match &result.output {
        LanguageModelToolResultOutput::Json { value, .. } => value.clone(),
        _ => json!({}),
    };
    let mut value = json!({
        "toolResponse": {
            "toolType": server_type,
            "response": response,
            "id": server_id,
        }
    });
    if let Some(signature) = opts.get("thoughtSignature").and_then(JsonValue::as_str) {
        value["thoughtSignature"] = json!(signature);
    }
    Some(value)
}

struct PreparedTools {
    tools: Option<Vec<JsonValue>>,
    tool_config: Option<JsonValue>,
    tool_warnings: Vec<Warning>,
}

fn prepare_google_tools(
    tools: Option<&[LanguageModelTool]>,
    tool_choice: Option<&LanguageModelToolChoice>,
    model_id: &str,
    is_vertex_provider: bool,
) -> Result<PreparedTools, String> {
    let tools = tools.filter(|tools| !tools.is_empty());
    let mut tool_warnings = Vec::new();
    let Some(tools) = tools else {
        return Ok(PreparedTools {
            tools: None,
            tool_config: None,
            tool_warnings,
        });
    };
    let is_latest = matches!(
        model_id,
        "gemini-flash-latest" | "gemini-flash-lite-latest" | "gemini-pro-latest"
    );
    let is_gemini2_or_newer = model_id.contains("gemini-2")
        || model_id.contains("gemini-3")
        || model_id.contains("nano-banana")
        || is_latest;
    let is_gemini3_or_newer = model_id.contains("gemini-3");
    let supports_file_search = model_id.contains("gemini-2.5") || model_id.contains("gemini-3");
    let has_function_tools = tools
        .iter()
        .any(|tool| matches!(tool, LanguageModelTool::Function(_)));
    let has_provider_tools = tools
        .iter()
        .any(|tool| matches!(tool, LanguageModelTool::Provider(_)));

    if has_function_tools && has_provider_tools && !is_gemini3_or_newer {
        tool_warnings.push(Warning::Unsupported {
            feature: "combination of function and provider-defined tools".to_string(),
            details: None,
        });
    }

    if has_provider_tools {
        let mut google_tools = Vec::new();
        for tool in tools {
            let LanguageModelTool::Provider(tool) = tool else {
                continue;
            };
            match tool.id.as_str() {
                "google.google_search" if is_gemini2_or_newer => {
                    google_tools.push(json!({ "googleSearch": tool.args }));
                }
                "google.enterprise_web_search" if is_gemini2_or_newer => {
                    google_tools.push(json!({ "enterpriseWebSearch": {} }));
                }
                "google.url_context" if is_gemini2_or_newer => {
                    google_tools.push(json!({ "urlContext": {} }));
                }
                "google.code_execution" if is_gemini2_or_newer => {
                    google_tools.push(json!({ "codeExecution": {} }));
                }
                "google.file_search" if supports_file_search => {
                    google_tools.push(json!({ "fileSearch": tool.args }));
                }
                "google.vertex_rag_store" if is_gemini2_or_newer => {
                    google_tools.push(json!({
                        "retrieval": {
                            "vertex_rag_store": {
                                "rag_resources": { "rag_corpus": tool.args.get("ragCorpus").cloned() },
                                "similarity_top_k": tool.args.get("topK").cloned(),
                            }
                        }
                    }));
                }
                "google.google_maps" if is_gemini2_or_newer => {
                    google_tools.push(json!({ "googleMaps": {} }));
                }
                _ => tool_warnings.push(Warning::Unsupported {
                    feature: format!("provider-defined tool {}", tool.id),
                    details: Some(provider_tool_unsupported_details(&tool.id, model_id)),
                }),
            }
        }

        if has_function_tools && is_gemini3_or_newer && !google_tools.is_empty() {
            let mut declarations = Vec::new();
            for tool in tools {
                if let LanguageModelTool::Function(tool) = tool {
                    declarations.push(function_declaration(tool));
                }
            }
            let mut tool_config = json!({
                "functionCallingConfig": { "mode": "VALIDATED" }
            });
            if !is_vertex_provider {
                tool_config["includeServerSideToolInvocations"] = JsonValue::Bool(true);
            }
            apply_tool_choice(&mut tool_config, tool_choice, true);
            google_tools.push(json!({ "functionDeclarations": declarations }));
            return Ok(PreparedTools {
                tools: Some(google_tools),
                tool_config: Some(strip_nulls(tool_config)),
                tool_warnings,
            });
        }
        return Ok(PreparedTools {
            tools: (!google_tools.is_empty())
                .then_some(google_tools.into_iter().map(strip_nulls).collect()),
            tool_config: None,
            tool_warnings,
        });
    }

    let mut declarations = Vec::new();
    let mut has_strict_tools = false;
    for tool in tools {
        match tool {
            LanguageModelTool::Function(tool) => {
                if tool.strict == Some(true) {
                    has_strict_tools = true;
                }
                declarations.push(function_declaration(tool));
            }
            LanguageModelTool::Provider(tool) => tool_warnings.push(Warning::Unsupported {
                feature: format!("function tool {}", tool.name),
                details: None,
            }),
        }
    }
    let mut tool_config = JsonValue::Null;
    if let Some(tool_choice) = tool_choice {
        tool_config = json!({ "functionCallingConfig": {} });
        apply_tool_choice(&mut tool_config, Some(tool_choice), has_strict_tools);
    } else if has_strict_tools {
        tool_config = json!({ "functionCallingConfig": { "mode": "VALIDATED" } });
    }
    Ok(PreparedTools {
        tools: Some(vec![json!({ "functionDeclarations": declarations })]),
        tool_config: (!tool_config.is_null()).then_some(tool_config),
        tool_warnings,
    })
}

fn function_declaration(tool: &ai_sdk_rust::LanguageModelFunctionTool) -> JsonValue {
    let parameters =
        convert_json_schema_to_openapi_schema(Some(&tool.input_schema)).unwrap_or(JsonValue::Null);
    strip_nulls(json!({
        "name": tool.name,
        "description": tool.description.clone().unwrap_or_default(),
        "parameters": parameters,
    }))
}

fn apply_tool_choice(
    tool_config: &mut JsonValue,
    tool_choice: Option<&LanguageModelToolChoice>,
    has_strict_tools: bool,
) {
    let Some(tool_choice) = tool_choice else {
        return;
    };
    let mode = match tool_choice {
        LanguageModelToolChoice::Auto => {
            if has_strict_tools {
                "VALIDATED"
            } else {
                "AUTO"
            }
        }
        LanguageModelToolChoice::None => "NONE",
        LanguageModelToolChoice::Required | LanguageModelToolChoice::Tool { .. } => {
            if has_strict_tools {
                "VALIDATED"
            } else {
                "ANY"
            }
        }
    };
    tool_config["functionCallingConfig"]["mode"] = json!(mode);
    if let LanguageModelToolChoice::Tool { tool_name } = tool_choice {
        tool_config["functionCallingConfig"]["allowedFunctionNames"] = json!([tool_name]);
    }
}

fn merge_tool_config(
    tool_config: Option<JsonValue>,
    stream_function_call_arguments: bool,
    retrieval_config: Option<JsonValue>,
) -> Option<JsonValue> {
    if tool_config.is_none() && !stream_function_call_arguments && retrieval_config.is_none() {
        return None;
    }
    let mut object = tool_config.unwrap_or_else(|| json!({}));
    if stream_function_call_arguments {
        object["functionCallingConfig"]["streamFunctionCallArguments"] = JsonValue::Bool(true);
    }
    if let Some(retrieval_config) = retrieval_config {
        object["retrievalConfig"] = retrieval_config;
    }
    Some(strip_nulls(object))
}

fn google_generate_result_from_response(
    response: JsonValue,
    raw_response: JsonValue,
    response_headers: Headers,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    provider_names: &[&str],
    generate_id: &GoogleGenerateId,
) -> LanguageModelGenerateResult {
    let candidate = response
        .get("candidates")
        .and_then(JsonValue::as_array)
        .and_then(|candidates| candidates.first())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let parts = candidate
        .pointer("/content/parts")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    let mut content = Vec::new();
    let mut last_code_execution_tool_call_id: Option<String> = None;
    let mut last_server_tool_call_id: Option<String> = None;
    for part in parts {
        google_content_part_to_content(
            &mut content,
            &part,
            provider_names,
            generate_id,
            &mut last_code_execution_tool_call_id,
            &mut last_server_tool_call_id,
        );
    }
    content.extend(extract_sources(
        candidate.get("groundingMetadata"),
        generate_id,
        provider_names,
    ));
    let finish_reason_raw = candidate.get("finishReason").and_then(JsonValue::as_str);
    let has_tool_calls = content.iter().any(|part| match part {
        LanguageModelContent::ToolCall(call) => call.provider_executed != Some(true),
        _ => false,
    });
    let mut result = LanguageModelGenerateResult::new(
        content,
        LanguageModelFinishReason {
            unified: map_google_finish_reason(finish_reason_raw, has_tool_calls),
            raw: finish_reason_raw.map(str::to_string),
        },
        convert_google_usage(response.get("usageMetadata")),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_response(LanguageModelResponse::new().with_body(raw_response));
    if !response_headers.is_empty() {
        let mut response_metadata = result.response.take().unwrap_or_default();
        response_metadata.headers = Some(response_headers);
        result = result.with_response(response_metadata);
    }

    let metadata = wrap_provider_metadata(
        provider_names,
        JsonObject::from_iter([
            (
                "promptFeedback".to_string(),
                response
                    .get("promptFeedback")
                    .cloned()
                    .unwrap_or(JsonValue::Null),
            ),
            (
                "groundingMetadata".to_string(),
                candidate
                    .get("groundingMetadata")
                    .cloned()
                    .unwrap_or(JsonValue::Null),
            ),
            (
                "urlContextMetadata".to_string(),
                candidate
                    .get("urlContextMetadata")
                    .cloned()
                    .unwrap_or(JsonValue::Null),
            ),
            (
                "safetyRatings".to_string(),
                candidate
                    .get("safetyRatings")
                    .cloned()
                    .unwrap_or(JsonValue::Null),
            ),
            (
                "usageMetadata".to_string(),
                response
                    .get("usageMetadata")
                    .cloned()
                    .unwrap_or(JsonValue::Null),
            ),
            (
                "finishMessage".to_string(),
                candidate
                    .get("finishMessage")
                    .cloned()
                    .unwrap_or(JsonValue::Null),
            ),
            (
                "serviceTier".to_string(),
                response
                    .get("serviceTier")
                    .cloned()
                    .unwrap_or(JsonValue::Null),
            ),
        ]),
    );
    result = result.with_provider_metadata(metadata);
    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn google_content_part_to_content(
    content: &mut Vec<LanguageModelContent>,
    part: &JsonValue,
    provider_names: &[&str],
    generate_id: &GoogleGenerateId,
    last_code_execution_tool_call_id: &mut Option<String>,
    last_server_tool_call_id: &mut Option<String>,
) {
    if let Some(code) = part
        .pointer("/executableCode/code")
        .and_then(JsonValue::as_str)
    {
        let id = generate_id();
        *last_code_execution_tool_call_id = Some(id.clone());
        content.push(LanguageModelContent::ToolCall(
            LanguageModelToolCall::new(
                id,
                "code_execution",
                json!({
                    "language": part.pointer("/executableCode/language").cloned(),
                    "code": code,
                })
                .to_string(),
            )
            .with_provider_executed(true),
        ));
    } else if let Some(result) = part.get("codeExecutionResult") {
        let id = last_code_execution_tool_call_id
            .take()
            .unwrap_or_else(|| generate_id());
        content.push(LanguageModelContent::ToolResult(
            LanguageModelToolResult::new(
                id,
                "code_execution",
                non_null_json(json!({
                    "outcome": result.get("outcome").cloned().unwrap_or(JsonValue::Null),
                    "output": result.get("output").and_then(JsonValue::as_str).unwrap_or(""),
                })),
            ),
        ));
    } else if let Some(text) = part.get("text").and_then(JsonValue::as_str) {
        let metadata = part
            .get("thoughtSignature")
            .and_then(JsonValue::as_str)
            .map(|signature| {
                wrap_provider_metadata(
                    provider_names,
                    JsonObject::from_iter([("thoughtSignature".to_string(), json!(signature))]),
                )
            });
        if text.is_empty() {
            if let (Some(metadata), Some(last)) = (metadata, content.last_mut()) {
                match last {
                    LanguageModelContent::Text(text) => text.provider_metadata = Some(metadata),
                    LanguageModelContent::Reasoning(reasoning) => {
                        reasoning.provider_metadata = Some(metadata)
                    }
                    LanguageModelContent::ToolCall(call) => call.provider_metadata = Some(metadata),
                    _ => {}
                }
            }
        } else if part.get("thought").and_then(JsonValue::as_bool) == Some(true) {
            let mut reasoning = LanguageModelReasoning::new(text);
            if let Some(metadata) = metadata {
                reasoning = reasoning.with_provider_metadata(metadata);
            }
            content.push(LanguageModelContent::Reasoning(reasoning));
        } else {
            let mut text_part = LanguageModelText::new(text);
            if let Some(metadata) = metadata {
                text_part = text_part.with_provider_metadata(metadata);
            }
            content.push(LanguageModelContent::Text(text_part));
        }
    } else if let Some(function_call) = part.get("functionCall") {
        let id = function_call
            .get("id")
            .and_then(JsonValue::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| generate_id());
        let name = function_call
            .get("name")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        let input = function_call
            .get("args")
            .cloned()
            .unwrap_or_else(|| json!({}))
            .to_string();
        let mut tool_call = LanguageModelToolCall::new(id, name, input);
        if let Some(signature) = part.get("thoughtSignature").and_then(JsonValue::as_str) {
            tool_call = tool_call.with_provider_metadata(wrap_provider_metadata(
                provider_names,
                JsonObject::from_iter([("thoughtSignature".to_string(), json!(signature))]),
            ));
        }
        content.push(LanguageModelContent::ToolCall(tool_call));
    } else if let Some(inline) = part.get("inlineData") {
        let media_type = inline
            .get("mimeType")
            .and_then(JsonValue::as_str)
            .unwrap_or("application/octet-stream");
        let data = inline
            .get("data")
            .and_then(JsonValue::as_str)
            .unwrap_or("")
            .to_string();
        let metadata = part
            .get("thoughtSignature")
            .and_then(JsonValue::as_str)
            .map(|signature| {
                wrap_provider_metadata(
                    provider_names,
                    JsonObject::from_iter([("thoughtSignature".to_string(), json!(signature))]),
                )
            });
        if part.get("thought").and_then(JsonValue::as_bool) == Some(true) {
            let mut file = LanguageModelReasoningFile::new(
                media_type,
                LanguageModelFileData::Data {
                    data: FileDataContent::Base64(data),
                },
            );
            if let Some(metadata) = metadata {
                file = file.with_provider_metadata(metadata);
            }
            content.push(LanguageModelContent::ReasoningFile(file));
        } else {
            let mut file = LanguageModelFile::new(
                media_type,
                LanguageModelFileData::Data {
                    data: FileDataContent::Base64(data),
                },
            );
            if let Some(metadata) = metadata {
                file = file.with_provider_metadata(metadata);
            }
            content.push(LanguageModelContent::File(file));
        }
    } else if let Some(tool_call) = part.get("toolCall") {
        let id = tool_call
            .get("id")
            .and_then(JsonValue::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| generate_id());
        *last_server_tool_call_id = Some(id.clone());
        let tool_type = tool_call
            .get("toolType")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        let mut metadata = JsonObject::from_iter([
            ("serverToolCallId".to_string(), json!(id.clone())),
            ("serverToolType".to_string(), json!(tool_type)),
        ]);
        if let Some(signature) = part.get("thoughtSignature").and_then(JsonValue::as_str) {
            metadata.insert("thoughtSignature".to_string(), json!(signature));
        }
        content.push(LanguageModelContent::ToolCall(
            LanguageModelToolCall::new(
                id,
                format!("server:{tool_type}"),
                tool_call
                    .get("args")
                    .cloned()
                    .unwrap_or_else(|| json!({}))
                    .to_string(),
            )
            .with_provider_executed(true)
            .with_dynamic(true)
            .with_provider_metadata(wrap_provider_metadata(provider_names, metadata)),
        ));
    } else if let Some(tool_response) = part.get("toolResponse") {
        let id = last_server_tool_call_id
            .take()
            .or_else(|| {
                tool_response
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| generate_id());
        let tool_type = tool_response
            .get("toolType")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        let result = tool_response
            .get("response")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let mut metadata = JsonObject::from_iter([
            ("serverToolCallId".to_string(), json!(id.clone())),
            ("serverToolType".to_string(), json!(tool_type)),
        ]);
        if let Some(signature) = part.get("thoughtSignature").and_then(JsonValue::as_str) {
            metadata.insert("thoughtSignature".to_string(), json!(signature));
        }
        content.push(LanguageModelContent::ToolResult(
            LanguageModelToolResult::new(id, format!("server:{tool_type}"), non_null_json(result))
                .with_provider_metadata(wrap_provider_metadata(provider_names, metadata)),
        ));
    }
}

fn google_stream_result_from_chunks(
    chunks: Vec<ParseJsonResult<JsonValue>>,
    response_headers: Headers,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    provider_names: &[&str],
    include_raw_chunks: bool,
    generate_id: &GoogleGenerateId,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let mut stream = vec![LanguageModelStreamPart::StreamStart(
        LanguageModelStreamStart::new(warnings),
    )];
    let mut finish_reason = LanguageModelFinishReason {
        unified: FinishReason::Other,
        raw: None,
    };
    let mut usage = LanguageModelUsage::default();
    let mut block_counter = 0_u64;
    let mut current_text_id: Option<String> = None;
    let mut current_reasoning_id: Option<String> = None;
    let mut last_code_execution_tool_call_id = None;
    let mut last_server_tool_call_id = None;
    let mut has_tool_calls = false;

    for chunk in chunks {
        match chunk {
            ParseJsonResult::Failure { error, raw_value } => {
                stream.push(LanguageModelStreamPart::Error(
                    LanguageModelErrorStreamPart::new(json!({
                        "error": error.to_string(),
                        "rawValue": raw_value,
                    })),
                ));
            }
            ParseJsonResult::Success { value, raw_value } => {
                if include_raw_chunks {
                    stream.push(LanguageModelStreamPart::Raw(
                        ai_sdk_rust::LanguageModelRawStreamPart::new(raw_value),
                    ));
                }
                if let Some(usage_metadata) = value.get("usageMetadata") {
                    usage = convert_google_usage(Some(usage_metadata));
                }
                let Some(candidate) = value
                    .get("candidates")
                    .and_then(JsonValue::as_array)
                    .and_then(|candidates| candidates.first())
                else {
                    continue;
                };
                if let Some(raw) = candidate.get("finishReason").and_then(JsonValue::as_str) {
                    finish_reason.raw = Some(raw.to_string());
                }
                let parts = candidate
                    .pointer("/content/parts")
                    .and_then(JsonValue::as_array)
                    .cloned()
                    .unwrap_or_default();
                for part in parts {
                    if let Some(text) = part.get("text").and_then(JsonValue::as_str) {
                        if part.get("thought").and_then(JsonValue::as_bool) == Some(true) {
                            if current_reasoning_id.is_none() {
                                block_counter += 1;
                                let id = format!("reasoning-{block_counter}");
                                stream.push(LanguageModelStreamPart::ReasoningStart(
                                    ai_sdk_rust::LanguageModelReasoningStart::new(id.clone()),
                                ));
                                current_reasoning_id = Some(id);
                            }
                            stream.push(LanguageModelStreamPart::ReasoningDelta(
                                LanguageModelReasoningDelta::new(
                                    current_reasoning_id.clone().unwrap_or_default(),
                                    text,
                                ),
                            ));
                        } else {
                            if current_text_id.is_none() {
                                block_counter += 1;
                                let id = format!("text-{block_counter}");
                                stream.push(LanguageModelStreamPart::TextStart(
                                    LanguageModelTextStart::new(id.clone()),
                                ));
                                current_text_id = Some(id);
                            }
                            stream.push(LanguageModelStreamPart::TextDelta(
                                LanguageModelTextDelta::new(
                                    current_text_id.clone().unwrap_or_default(),
                                    text,
                                ),
                            ));
                        }
                    } else if part.get("functionCall").is_some() || part.get("toolCall").is_some() {
                        has_tool_calls = true;
                        let mut content = Vec::new();
                        google_content_part_to_content(
                            &mut content,
                            &part,
                            provider_names,
                            generate_id,
                            &mut last_code_execution_tool_call_id,
                            &mut last_server_tool_call_id,
                        );
                        for content_part in content {
                            stream.push(content_to_stream_part(content_part));
                        }
                    } else {
                        let mut content = Vec::new();
                        google_content_part_to_content(
                            &mut content,
                            &part,
                            provider_names,
                            generate_id,
                            &mut last_code_execution_tool_call_id,
                            &mut last_server_tool_call_id,
                        );
                        for content_part in content {
                            stream.push(content_to_stream_part(content_part));
                        }
                    }
                }
            }
        }
    }
    if let Some(id) = current_reasoning_id {
        stream.push(LanguageModelStreamPart::ReasoningEnd(
            ai_sdk_rust::LanguageModelReasoningEnd::new(id),
        ));
    }
    if let Some(id) = current_text_id {
        stream.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
            id,
        )));
    }
    finish_reason.unified = map_google_finish_reason(finish_reason.raw.as_deref(), has_tool_calls);
    stream.push(LanguageModelStreamPart::Finish(
        LanguageModelStreamFinish::new(usage, finish_reason),
    ));
    LanguageModelStreamResult::new(stream)
        .with_request(LanguageModelRequest::new().with_body(request_body))
        .with_response(LanguageModelStreamResultResponse {
            headers: Some(response_headers),
        })
}

fn content_to_stream_part(content: LanguageModelContent) -> LanguageModelStreamPart {
    match content {
        LanguageModelContent::Text(text) => {
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text", text.text))
        }
        LanguageModelContent::Reasoning(reasoning) => LanguageModelStreamPart::ReasoningDelta(
            ai_sdk_rust::LanguageModelReasoningDelta::new("reasoning", reasoning.text),
        ),
        LanguageModelContent::File(file) => LanguageModelStreamPart::File(file),
        LanguageModelContent::ReasoningFile(file) => LanguageModelStreamPart::ReasoningFile(file),
        LanguageModelContent::ToolCall(call) => LanguageModelStreamPart::ToolCall(call),
        LanguageModelContent::ToolResult(result) => LanguageModelStreamPart::ToolResult(result),
        LanguageModelContent::Source(source) => LanguageModelStreamPart::Source(source),
        LanguageModelContent::Custom(custom) => LanguageModelStreamPart::Custom(custom),
        LanguageModelContent::ToolApprovalRequest(request) => {
            LanguageModelStreamPart::ToolApprovalRequest(request)
        }
    }
}

fn google_interactions_generate_result(
    response: JsonValue,
    raw_response: JsonValue,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    generate_id: &GoogleGenerateId,
) -> LanguageModelGenerateResult {
    let interaction_id = response
        .get("id")
        .and_then(JsonValue::as_str)
        .map(str::to_string);
    let (content, has_function_call) = parse_google_interactions_outputs(
        response.get("steps").and_then(JsonValue::as_array),
        generate_id,
        interaction_id.as_deref(),
    );
    let status = response.get("status").and_then(JsonValue::as_str);
    let mut result = LanguageModelGenerateResult::new(
        content,
        LanguageModelFinishReason {
            unified: map_google_interactions_finish_reason(status, has_function_call),
            raw: status.map(str::to_string),
        },
        convert_google_usage(response.get("usage")),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_response(LanguageModelResponse::new().with_body(raw_response));
    if let Some(id) = interaction_id {
        result = result.with_provider_metadata(ProviderMetadata::from([(
            "google".to_string(),
            JsonObject::from_iter([("interactionId".to_string(), json!(id))]),
        )]));
    }
    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn parse_google_interactions_outputs(
    steps: Option<&Vec<JsonValue>>,
    generate_id: &GoogleGenerateId,
    interaction_id: Option<&str>,
) -> (Vec<LanguageModelContent>, bool) {
    let mut content = Vec::new();
    let mut has_function_call = false;
    let Some(steps) = steps else {
        return (content, has_function_call);
    };
    for step in steps {
        let step_type = step.get("type").and_then(JsonValue::as_str).unwrap_or("");
        match step_type {
            "model_output" => {
                for block in step
                    .get("content")
                    .and_then(JsonValue::as_array)
                    .into_iter()
                    .flatten()
                {
                    match block.get("type").and_then(JsonValue::as_str) {
                        Some("text") => {
                            content.push(LanguageModelContent::Text(
                                LanguageModelText::new(
                                    block.get("text").and_then(JsonValue::as_str).unwrap_or(""),
                                )
                                .with_provider_metadata(interaction_metadata(None, interaction_id)),
                            ));
                            content.extend(interaction_annotations_to_sources(
                                block.get("annotations").and_then(JsonValue::as_array),
                                generate_id,
                            ));
                        }
                        Some("image") => {
                            if let Some(data) = block.get("data").and_then(JsonValue::as_str) {
                                content.push(LanguageModelContent::File(LanguageModelFile::new(
                                    block
                                        .get("mime_type")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or("image/png"),
                                    LanguageModelFileData::Data {
                                        data: FileDataContent::Base64(data.to_string()),
                                    },
                                )));
                            } else if let Some(uri) = block
                                .get("uri")
                                .and_then(JsonValue::as_str)
                                .and_then(|uri| Url::parse(uri).ok())
                            {
                                content.push(LanguageModelContent::File(LanguageModelFile::new(
                                    block
                                        .get("mime_type")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or("image/png"),
                                    LanguageModelFileData::Url { url: uri },
                                )));
                            }
                        }
                        _ => {}
                    }
                }
            }
            "thought" => {
                let text = step
                    .get("summary")
                    .and_then(JsonValue::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter(|item| {
                                item.get("type").and_then(JsonValue::as_str) == Some("text")
                            })
                            .filter_map(|item| item.get("text").and_then(JsonValue::as_str))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                content.push(LanguageModelContent::Reasoning(
                    LanguageModelReasoning::new(text).with_provider_metadata(interaction_metadata(
                        step.get("signature").and_then(JsonValue::as_str),
                        interaction_id,
                    )),
                ));
            }
            "function_call" => {
                has_function_call = true;
                content.push(LanguageModelContent::ToolCall(
                    LanguageModelToolCall::new(
                        step.get("id").and_then(JsonValue::as_str).unwrap_or(""),
                        step.get("name").and_then(JsonValue::as_str).unwrap_or(""),
                        step.get("arguments")
                            .cloned()
                            .unwrap_or_else(|| json!({}))
                            .to_string(),
                    )
                    .with_provider_metadata(interaction_metadata(
                        step.get("signature").and_then(JsonValue::as_str),
                        interaction_id,
                    )),
                ));
            }
            "google_search_call"
            | "code_execution_call"
            | "url_context_call"
            | "file_search_call"
            | "google_maps_call"
            | "mcp_server_tool_call" => {
                let id = step
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| generate_id());
                content.push(LanguageModelContent::ToolCall(
                    LanguageModelToolCall::new(
                        id,
                        builtin_tool_name_from_call_type(step_type, step),
                        step.get("arguments")
                            .cloned()
                            .unwrap_or_else(|| json!({}))
                            .to_string(),
                    )
                    .with_provider_executed(true),
                ));
            }
            "google_search_result"
            | "code_execution_result"
            | "url_context_result"
            | "file_search_result"
            | "google_maps_result"
            | "mcp_server_tool_result" => {
                let id = step
                    .get("call_id")
                    .and_then(JsonValue::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| generate_id());
                content.push(LanguageModelContent::ToolResult(
                    LanguageModelToolResult::new(
                        id,
                        builtin_tool_name_from_result_type(step_type, step),
                        non_null_json(step.get("result").cloned().unwrap_or_else(|| json!({}))),
                    )
                    .with_is_error(
                        step.get("is_error")
                            .and_then(JsonValue::as_bool)
                            .unwrap_or(false),
                    ),
                ));
                content.extend(interaction_builtin_sources(step, generate_id));
            }
            _ => {}
        }
    }
    (content, has_function_call)
}

fn convert_to_google_interactions_input(
    prompt: &LanguageModelPrompt,
    options: &JsonObject,
    warnings: &mut Vec<Warning>,
) -> Result<GoogleInteractionsInput, String> {
    let previous_interaction_id = options
        .get("previousInteractionId")
        .and_then(JsonValue::as_str);
    let store_false = options.get("store").and_then(JsonValue::as_bool) == Some(false);
    if previous_interaction_id.is_some() && store_false {
        warnings.push(Warning::Other {
            message: "google.interactions: providerOptions.google.previousInteractionId was set together with store: false. These are incoherent (the prior interaction cannot be referenced when nothing was stored on the server); the full history will be sent and previous_interaction_id will still be emitted.".to_string(),
        });
    }
    let mut system_texts = Vec::new();
    let mut steps = Vec::new();
    for message in prompt {
        match message {
            LanguageModelMessage::System(message) => system_texts.push(message.content.clone()),
            LanguageModelMessage::User(message) => {
                let mut content = Vec::new();
                for part in &message.content {
                    match part {
                        LanguageModelUserContentPart::Text(text) => {
                            content.push(json!({ "type": "text", "text": text.text }))
                        }
                        LanguageModelUserContentPart::File(file) => {
                            if let Some(block) = interactions_file_part_to_content(file, warnings)?
                            {
                                content.push(block);
                            }
                        }
                    }
                }
                if !content.is_empty() {
                    steps.push(
                        json!({ "type": "user_input", "content": merge_adjacent_text(content) }),
                    );
                }
            }
            LanguageModelMessage::Assistant(message) => {
                let mut pending = Vec::new();
                for part in &message.content {
                    match part {
                        LanguageModelAssistantContentPart::Text(text) => pending.push(json!({ "type": "text", "text": text.text })),
                        LanguageModelAssistantContentPart::File(file) => {
                            if let Some(block) = interactions_file_part_to_content(file, warnings)? {
                                pending.push(block);
                            }
                        }
                        LanguageModelAssistantContentPart::Reasoning(reasoning) => {
                            if !pending.is_empty() {
                                steps.push(json!({ "type": "model_output", "content": std::mem::take(&mut pending) }));
                            }
                            let signature = part_provider_options(reasoning.provider_options.as_ref(), &["google"])
                                .and_then(|options| options.get("signature"))
                                .cloned();
                            steps.push(strip_nulls(json!({
                                "type": "thought",
                                "signature": signature,
                                "summary": if reasoning.text.is_empty() { JsonValue::Null } else { json!([{ "type": "text", "text": reasoning.text }]) },
                            })));
                        }
                        LanguageModelAssistantContentPart::ToolCall(call) => {
                            if !pending.is_empty() {
                                steps.push(json!({ "type": "model_output", "content": std::mem::take(&mut pending) }));
                            }
                            steps.push(json!({
                                "type": "function_call",
                                "id": call.tool_call_id,
                                "name": call.tool_name,
                                "arguments": parse_tool_input(&call.input),
                            }));
                        }
                        _ => warnings.push(Warning::Other {
                            message: "google.interactions: unsupported assistant content part; part dropped.".to_string(),
                        }),
                    }
                }
                if !pending.is_empty() {
                    steps.push(json!({ "type": "model_output", "content": pending }));
                }
            }
            LanguageModelMessage::Tool(message) => {
                let mut content = Vec::new();
                for part in &message.content {
                    if let LanguageModelToolContentPart::ToolResult(result) = part {
                        content.push(json!({
                            "type": "function_result",
                            "id": result.tool_call_id,
                            "name": result.tool_name,
                            "response": tool_result_output_value(&result.output),
                        }));
                    }
                }
                if !content.is_empty() {
                    steps.push(json!({ "type": "user_input", "content": content }));
                }
            }
        }
    }
    Ok(GoogleInteractionsInput {
        input: JsonValue::Array(steps),
        system_instruction: (!system_texts.is_empty()).then(|| system_texts.join("\n\n")),
    })
}

struct GoogleInteractionsInput {
    input: JsonValue,
    system_instruction: Option<String>,
}

fn interactions_file_part_to_content(
    file: &LanguageModelFilePart,
    warnings: &mut Vec<Warning>,
) -> Result<Option<JsonValue>, String> {
    if let FileData::Text { text } = &file.data {
        return Ok(Some(json!({ "type": "text", "text": text })));
    }
    let kind = match get_top_level_media_type(&file.media_type) {
        "image" => "image",
        "audio" => "audio",
        "video" => "video",
        "application" | "text" => "document",
        _ => {
            warnings.push(Warning::Other {
                message: format!(
                    "google.interactions: unsupported file media type {}; part dropped.",
                    file.media_type
                ),
            });
            return Ok(None);
        }
    };
    let mut block = JsonObject::new();
    block.insert("type".to_string(), json!(kind));
    block.insert(
        "mime_type".to_string(),
        json!(full_media_type(&file.media_type, None)),
    );
    match &file.data {
        FileData::Url { url } => block.insert("uri".to_string(), json!(url.as_str())),
        FileData::Data { data } => block.insert("data".to_string(), json!(convert_to_base64(data))),
        FileData::Reference { reference } => block.insert(
            "uri".to_string(),
            json!(
                resolve_provider_reference(reference, "google")
                    .map_err(|error| error.to_string())?
            ),
        ),
        FileData::Text { .. } => unreachable!("handled text above"),
    };
    Ok(Some(JsonValue::Object(block)))
}

fn prepare_google_interactions_tools(
    tools: Option<&[LanguageModelTool]>,
    tool_choice: Option<&LanguageModelToolChoice>,
) -> Option<(Vec<JsonValue>, Option<JsonValue>, Vec<Warning>)> {
    let tools = tools.filter(|tools| !tools.is_empty())?;
    let mut warnings = Vec::new();
    let mut mapped = Vec::new();
    let mut has_function = false;
    for tool in tools {
        match tool {
            LanguageModelTool::Function(tool) => {
                has_function = true;
                mapped.push(json!({
                    "type": "function",
                    "name": tool.name,
                    "description": tool.description.clone().unwrap_or_default(),
                    "parameters": JsonValue::Object(tool.input_schema.clone()),
                }));
            }
            LanguageModelTool::Provider(tool) => {
                let value = match tool.id.as_str() {
                    "google.google_search" => json!({ "type": "google_search" }),
                    "google.code_execution" => json!({ "type": "code_execution" }),
                    "google.url_context" => json!({ "type": "url_context" }),
                    "google.file_search" => json!({
                        "type": "file_search",
                        "file_search_store_names": tool.args.get("fileSearchStoreNames").cloned(),
                        "top_k": tool.args.get("topK").cloned(),
                        "metadata_filter": tool.args.get("metadataFilter").cloned(),
                    }),
                    "google.google_maps" => json!({
                        "type": "google_maps",
                        "latitude": tool.args.get("latitude").cloned(),
                        "longitude": tool.args.get("longitude").cloned(),
                        "enable_widget": tool.args.get("enableWidget").cloned(),
                    }),
                    "google.computer_use" => json!({
                        "type": "computer_use",
                        "environment": tool.args.get("environment").cloned().unwrap_or_else(|| json!("browser")),
                        "excludedPredefinedFunctions": tool.args.get("excludedPredefinedFunctions").cloned(),
                    }),
                    "google.mcp_server" => json!({
                        "type": "mcp_server",
                        "name": tool.args.get("name").cloned(),
                        "url": tool.args.get("url").cloned(),
                        "headers": tool.args.get("headers").cloned(),
                        "allowed_tools": tool.args.get("allowedTools").cloned(),
                    }),
                    "google.retrieval" => json!({
                        "type": "retrieval",
                        "retrieval_types": tool.args.get("retrievalTypes").cloned().unwrap_or_else(|| json!(["vertex_ai_search"])),
                        "vertex_ai_search_config": tool.args.get("vertexAiSearchConfig").cloned(),
                    }),
                    _ => {
                        warnings.push(Warning::Unsupported {
                            feature: format!("provider-defined tool {}", tool.id),
                            details: Some(format!(
                                "provider-defined tool {} is not supported by google.interactions; tool dropped.",
                                tool.id
                            )),
                        });
                        continue;
                    }
                };
                mapped.push(strip_nulls(value));
            }
        }
    }
    let tool_choice = if has_function {
        tool_choice.map(|choice| match choice {
            LanguageModelToolChoice::Auto => json!("auto"),
            LanguageModelToolChoice::Required => json!("any"),
            LanguageModelToolChoice::None => json!("none"),
            LanguageModelToolChoice::Tool { tool_name } => json!({
                "allowed_tools": { "mode": "validated", "tools": [tool_name] }
            }),
        })
    } else {
        None
    };
    Some((mapped, tool_choice, warnings))
}

fn map_google_interactions_finish_reason(
    status: Option<&str>,
    has_function_call: bool,
) -> FinishReason {
    match status {
        Some("completed") => {
            if has_function_call {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            }
        }
        Some("requires_action") => FinishReason::ToolCalls,
        Some("failed") => FinishReason::Error,
        Some("incomplete") => FinishReason::Length,
        Some("cancelled") => FinishReason::Other,
        _ => FinishReason::Other,
    }
}

fn extract_sources(
    grounding_metadata: Option<&JsonValue>,
    generate_id: &GoogleGenerateId,
    provider_names: &[&str],
) -> Vec<LanguageModelContent> {
    let mut sources = Vec::new();
    let Some(chunks) = grounding_metadata
        .and_then(|metadata| metadata.get("groundingChunks"))
        .and_then(JsonValue::as_array)
    else {
        return sources;
    };
    for chunk in chunks {
        if let Some(web) = chunk.get("web") {
            if let Some(uri) = web.get("uri").and_then(JsonValue::as_str) {
                let mut source = ai_sdk_rust::LanguageModelUrlSource::new(generate_id(), uri);
                if let Some(title) = web.get("title").and_then(JsonValue::as_str) {
                    source = source.with_title(title);
                }
                sources.push(LanguageModelContent::Source(LanguageModelSource::Url(
                    source.with_provider_metadata(wrap_provider_metadata(
                        provider_names,
                        JsonObject::from_iter([("groundingChunk".to_string(), chunk.clone())]),
                    )),
                )));
            }
        } else if let Some(context) = chunk.get("retrievedContext") {
            let uri = context
                .get("uri")
                .and_then(JsonValue::as_str)
                .unwrap_or_else(|| {
                    context
                        .get("fileSearchStore")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("retrieved-context")
                });
            let title = context
                .get("title")
                .and_then(JsonValue::as_str)
                .unwrap_or(uri);
            if uri.starts_with("http://") || uri.starts_with("https://") {
                sources.push(LanguageModelContent::Source(LanguageModelSource::url(
                    generate_id(),
                    uri,
                )));
            } else {
                sources.push(LanguageModelContent::Source(LanguageModelSource::document(
                    generate_id(),
                    "application/octet-stream",
                    title,
                )));
            }
        } else if let Some(maps) = chunk.get("maps") {
            if let Some(uri) = maps.get("uri").and_then(JsonValue::as_str) {
                let mut source = ai_sdk_rust::LanguageModelUrlSource::new(generate_id(), uri);
                if let Some(title) = maps.get("title").and_then(JsonValue::as_str) {
                    source = source.with_title(title);
                }
                sources.push(LanguageModelContent::Source(LanguageModelSource::Url(
                    source,
                )));
            }
        } else if let Some(image) = chunk.get("image") {
            let title = image
                .get("title")
                .and_then(JsonValue::as_str)
                .unwrap_or("image");
            sources.push(LanguageModelContent::Source(LanguageModelSource::document(
                generate_id(),
                "image/*",
                title,
            )));
        }
    }
    sources
}

fn interaction_annotations_to_sources(
    annotations: Option<&Vec<JsonValue>>,
    generate_id: &GoogleGenerateId,
) -> Vec<LanguageModelContent> {
    let mut sources = Vec::new();
    let Some(annotations) = annotations else {
        return sources;
    };
    let mut seen = BTreeMap::<String, ()>::new();
    for annotation in annotations {
        let key = match annotation.get("type").and_then(JsonValue::as_str) {
            Some("url_citation") => {
                let Some(url) = annotation.get("url").and_then(JsonValue::as_str) else {
                    continue;
                };
                let key = format!("url:{url}");
                if seen.insert(key, ()).is_some() {
                    continue;
                }
                let mut source = ai_sdk_rust::LanguageModelUrlSource::new(generate_id(), url);
                if let Some(title) = annotation.get("title").and_then(JsonValue::as_str) {
                    source = source.with_title(title);
                }
                sources.push(LanguageModelContent::Source(LanguageModelSource::Url(
                    source,
                )));
                continue;
            }
            Some("file_citation") => annotation
                .get("url")
                .or_else(|| annotation.get("document_uri"))
                .or_else(|| annotation.get("file_name"))
                .and_then(JsonValue::as_str),
            Some("place_citation") => annotation.get("url").and_then(JsonValue::as_str),
            _ => None,
        };
        if let Some(uri) = key {
            if uri.starts_with("http://") || uri.starts_with("https://") {
                sources.push(LanguageModelContent::Source(LanguageModelSource::url(
                    generate_id(),
                    uri,
                )));
            } else {
                sources.push(LanguageModelContent::Source(LanguageModelSource::document(
                    generate_id(),
                    infer_doc_media_type(uri),
                    basename(uri).unwrap_or(uri),
                )));
            }
        }
    }
    sources
}

fn interaction_builtin_sources(
    step: &JsonValue,
    generate_id: &GoogleGenerateId,
) -> Vec<LanguageModelContent> {
    let mut sources = Vec::new();
    let result = step
        .get("result")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    match step.get("type").and_then(JsonValue::as_str) {
        Some("url_context_result") => {
            for entry in result {
                if entry
                    .get("status")
                    .and_then(JsonValue::as_str)
                    .is_some_and(|status| status != "success")
                {
                    continue;
                }
                if let Some(url) = entry.get("url").and_then(JsonValue::as_str) {
                    sources.push(LanguageModelContent::Source(LanguageModelSource::url(
                        generate_id(),
                        url,
                    )));
                }
            }
        }
        Some("google_search_result") => {
            for entry in result {
                if let Some(url) = entry.get("url").and_then(JsonValue::as_str) {
                    let mut source = ai_sdk_rust::LanguageModelUrlSource::new(generate_id(), url);
                    if let Some(title) = entry.get("title").and_then(JsonValue::as_str) {
                        source = source.with_title(title);
                    }
                    sources.push(LanguageModelContent::Source(LanguageModelSource::Url(
                        source,
                    )));
                }
            }
        }
        Some("file_search_result") => {
            for entry in result {
                let uri = entry
                    .get("url")
                    .or_else(|| entry.get("document_uri"))
                    .or_else(|| entry.get("file_name"))
                    .and_then(JsonValue::as_str);
                if let Some(uri) = uri {
                    if uri.starts_with("http://") || uri.starts_with("https://") {
                        sources.push(LanguageModelContent::Source(LanguageModelSource::url(
                            generate_id(),
                            uri,
                        )));
                    } else {
                        sources.push(LanguageModelContent::Source(LanguageModelSource::document(
                            generate_id(),
                            infer_doc_media_type(uri),
                            entry
                                .get("title")
                                .and_then(JsonValue::as_str)
                                .unwrap_or(uri),
                        )));
                    }
                }
            }
        }
        Some("google_maps_result") => {
            for entry in result {
                for place in entry
                    .get("places")
                    .and_then(JsonValue::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(url) = place.get("url").and_then(JsonValue::as_str) {
                        let mut source =
                            ai_sdk_rust::LanguageModelUrlSource::new(generate_id(), url);
                        if let Some(title) = place.get("name").and_then(JsonValue::as_str) {
                            source = source.with_title(title);
                        }
                        sources.push(LanguageModelContent::Source(LanguageModelSource::Url(
                            source,
                        )));
                    }
                }
            }
        }
        _ => {}
    }
    sources
}

fn google_request_headers(
    settings: &GoogleProviderSettings,
    call_headers: Option<&Headers>,
) -> Result<BTreeMap<String, Option<String>>, String> {
    let api_key = google_api_key(settings.api_key.as_ref()).map_err(|error| error.to_string())?;
    let mut provider_headers = Headers::new();
    provider_headers.insert("x-goog-api-key".to_string(), api_key);
    for (name, value) in &settings.headers {
        provider_headers.insert(name.clone(), value.clone());
    }
    provider_headers.insert("user-agent".to_string(), format!("ai-sdk/google/{VERSION}"));
    Ok(combine_headers([
        Some(
            provider_headers
                .into_iter()
                .map(|(name, value)| (name, Some(value)))
                .collect::<Vec<_>>(),
        ),
        call_headers.map(|headers| {
            headers
                .iter()
                .map(|(name, value)| (name.clone(), Some(value.clone())))
                .collect::<Vec<_>>()
        }),
    ]))
}

fn google_api_key(
    explicit_api_key: Option<&String>,
) -> Result<String, ai_sdk_rust::LoadApiKeyError> {
    let mut options =
        LoadApiKeyOptions::new("GOOGLE_GENERATIVE_AI_API_KEY", "Google Generative AI");
    if let Some(api_key) = explicit_api_key.filter(|api_key| !api_key.is_empty()) {
        options = options.with_api_key(api_key.clone());
    }
    load_api_key(options)
}

fn first_provider_options<T>(
    provider_names: &[&str],
    provider_options: Option<&ProviderOptions>,
) -> Result<Option<T>, String>
where
    T: for<'de> Deserialize<'de>,
{
    for provider in provider_names {
        let parsed = provider_options_for(provider, provider_options)?;
        if parsed.is_some() {
            return Ok(parsed);
        }
    }
    Ok(None)
}

fn provider_options_for<T>(
    provider: &str,
    provider_options: Option<&ProviderOptions>,
) -> Result<Option<T>, String>
where
    T: for<'de> Deserialize<'de>,
{
    parse_provider_options(provider, provider_options, |value| {
        serde_json::from_value(value.clone()).map_err(|error| error.to_string())
    })
    .map_err(|error| error.to_string())
}

fn part_provider_options<'a>(
    provider_options: Option<&'a ProviderOptions>,
    provider_names: &[&str],
) -> Option<&'a JsonObject> {
    let options = provider_options?;
    for name in provider_names {
        if let Some(value) = options.get(*name) {
            return Some(value);
        }
    }
    if provider_names.contains(&"google") {
        options
            .get("googleVertex")
            .or_else(|| options.get("vertex"))
    } else {
        options.get("google")
    }
}

fn thought_signature(
    provider_options: Option<&ProviderOptions>,
    provider_names: &[&str],
) -> Option<String> {
    part_provider_options(provider_options, provider_names)
        .and_then(|options| options.get("thoughtSignature"))
        .and_then(JsonValue::as_str)
        .map(str::to_string)
}

fn resolve_thinking_config(
    reasoning: Option<&LanguageModelReasoningEffort>,
    model_id: &str,
    warnings: &mut Vec<Warning>,
) -> Option<JsonValue> {
    let supports_budget = model_id.contains("gemini-2.5") || model_id.contains("gemini-3");
    match reasoning {
        Some(LanguageModelReasoningEffort::None) if supports_budget => {
            Some(json!({ "thinkingBudget": 0 }))
        }
        Some(LanguageModelReasoningEffort::Low) | Some(LanguageModelReasoningEffort::Minimal)
            if supports_budget =>
        {
            Some(json!({ "thinkingBudget": 1024 }))
        }
        Some(LanguageModelReasoningEffort::Medium) if supports_budget => {
            Some(json!({ "thinkingBudget": 8192 }))
        }
        Some(LanguageModelReasoningEffort::High) | Some(LanguageModelReasoningEffort::Xhigh)
            if supports_budget =>
        {
            Some(json!({ "thinkingBudget": 24576 }))
        }
        Some(LanguageModelReasoningEffort::ProviderDefault) | None => None,
        Some(other) => {
            warnings.push(Warning::Unsupported {
                feature: "reasoning".to_string(),
                details: Some(format!(
                    "This model does not support reasoning configuration ({other:?})."
                )),
            });
            None
        }
    }
}

fn vertex_service_tier(service_tier: &str) -> &str {
    match service_tier {
        "priority" => "PRIORITY",
        "flex" => "FLEX",
        _ => service_tier,
    }
}

fn embedding_parts(value: &str, multimodal: Option<&Vec<JsonValue>>) -> Vec<JsonValue> {
    let mut parts = Vec::new();
    if !value.is_empty() {
        parts.push(json!({ "text": value }));
    }
    if let Some(multimodal) = multimodal {
        parts.extend(multimodal.clone());
    }
    if parts.is_empty() {
        parts.push(json!({ "text": value }));
    }
    parts
}

fn google_failed_response_handler(
    request: &ProviderApiRequest,
    response: &ProviderApiResponse,
) -> Result<
    ai_sdk_rust::ResponseHandlerResult<ai_sdk_rust::ApiCallError>,
    ProviderApiResponseHandlerError,
> {
    Ok(create_json_error_response_handler(
        response.json_error_response_handler_options(request),
        |value| {
            serde_json::from_value::<JsonValue>(value.clone()).map_err(|error| error.to_string())
        },
        |value| {
            value
                .pointer("/error/message")
                .and_then(JsonValue::as_str)
                .unwrap_or("Google API error")
                .to_string()
        },
        |_, _| None,
    ))
}

fn default_google_transport() -> GoogleTransport {
    Arc::new(|request| Box::pin(async move { execute_google_request(request) }))
}

fn execute_google_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Post => execute_google_post_request(request),
        ProviderApiRequestMethod::Get => execute_google_get_request(request),
    }
}

fn execute_google_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    provider_api_response(builder.config().http_status_as_error(false).build().call())
}

fn execute_google_post_request(
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
                "multipart form data is not supported by the Google transport",
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
    Ok(ProviderApiResponse::new(status.as_u16(), status_text)
        .with_headers(headers)
        .with_text_body(body))
}

fn google_error_generate_result(
    model_id: &str,
    message: &str,
    request_body: JsonValue,
) -> LanguageModelGenerateResult {
    LanguageModelGenerateResult::new(
        Vec::new(),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("error".to_string()),
        },
        LanguageModelUsage::default(),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_response(LanguageModelResponse {
        messages: None,
        id: None,
        timestamp: None,
        model_id: Some(model_id.to_string()),
        headers: None,
        body: Some(json!({ "error": message })),
    })
}

fn google_error_stream_result(
    message: &str,
    request_body: JsonValue,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    LanguageModelStreamResult::new(vec![
        LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
        LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(
            json!({ "error": message }),
        )),
        LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
            LanguageModelUsage::default(),
            LanguageModelFinishReason {
                unified: FinishReason::Error,
                raw: Some("error".to_string()),
            },
        )),
    ])
    .with_request(LanguageModelRequest::new().with_body(request_body))
}

fn image_error_result(
    model_id: &str,
    warnings: Vec<Warning>,
    message: impl Into<String>,
    timestamp: OffsetDateTime,
) -> ImageModelResult {
    let mut result =
        ImageModelResult::new(Vec::new(), ImageModelResponse::new(timestamp, model_id));
    result = result.with_warning(Warning::Other {
        message: message.into(),
    });
    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn video_error_result(
    model_id: &str,
    warnings: Vec<Warning>,
    message: impl Into<String>,
    timestamp: OffsetDateTime,
) -> VideoModelResult {
    let mut result =
        VideoModelResult::new(Vec::new(), VideoModelResponse::new(timestamp, model_id));
    result = result.with_warning(Warning::Other {
        message: message.into(),
    });
    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn files_error_result(
    media_type: &str,
    warnings: Vec<Warning>,
    message: impl Into<String>,
) -> FilesUploadFileResult {
    let provider_reference = ai_sdk_rust::ProviderReference::try_from(BTreeMap::from([(
        "google".to_string(),
        String::new(),
    )]))
    .expect("provider reference is valid");
    let mut result = FilesUploadFileResult::new(provider_reference).with_media_type(media_type);
    result = result.with_warning(Warning::Other {
        message: message.into(),
    });
    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn files_result_from_resource(
    file: JsonValue,
    fallback_media_type: String,
    warnings: Vec<Warning>,
) -> FilesUploadFileResult {
    let uri = file
        .get("uri")
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string();
    let provider_reference =
        ai_sdk_rust::ProviderReference::try_from(BTreeMap::from([("google".to_string(), uri)]))
            .expect("provider reference is valid");
    let mut google_metadata = JsonObject::new();
    for key in [
        "name",
        "displayName",
        "mimeType",
        "sizeBytes",
        "state",
        "uri",
        "createTime",
        "updateTime",
        "expirationTime",
        "sha256Hash",
    ] {
        if let Some(value) = file.get(key) {
            google_metadata.insert(key.to_string(), value.clone());
        }
    }
    let mut result = FilesUploadFileResult::new(provider_reference)
        .with_media_type(
            file.get("mimeType")
                .and_then(JsonValue::as_str)
                .unwrap_or(&fallback_media_type),
        )
        .with_provider_metadata(ProviderMetadata::from([(
            "google".to_string(),
            google_metadata,
        )]));
    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn upload_file_bytes(data: &FilesUploadFileData) -> Vec<u8> {
    match data {
        FilesUploadFileData::Text { text } => text.as_bytes().to_vec(),
        FilesUploadFileData::Data { data } => match data {
            FileDataContent::Bytes(bytes) => bytes.clone(),
            FileDataContent::Base64(value) => value.as_bytes().to_vec(),
        },
    }
}

fn strip_nulls(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(object) => JsonValue::Object(
            object
                .into_iter()
                .filter_map(|(key, value)| {
                    let value = strip_nulls(value);
                    (!value.is_null()).then_some((key, value))
                })
                .collect(),
        ),
        JsonValue::Array(values) => JsonValue::Array(values.into_iter().map(strip_nulls).collect()),
        other => other,
    }
}

fn insert_opt<T>(object: &mut JsonObject, key: &str, value: Option<T>)
where
    T: Serialize,
{
    if let Some(value) = value {
        let value = serde_json::to_value(value).expect("value serializes");
        if !value.is_null() {
            object.insert(key.to_string(), value);
        }
    }
}

fn wrap_provider_metadata(provider_names: &[&str], payload: JsonObject) -> ProviderMetadata {
    provider_names
        .iter()
        .map(|name| ((*name).to_string(), payload.clone()))
        .collect()
}

fn interaction_metadata(signature: Option<&str>, interaction_id: Option<&str>) -> ProviderMetadata {
    let mut google = JsonObject::new();
    if let Some(signature) = signature {
        google.insert("signature".to_string(), json!(signature));
    }
    if let Some(interaction_id) = interaction_id {
        google.insert("interactionId".to_string(), json!(interaction_id));
    }
    ProviderMetadata::from([("google".to_string(), google)])
}

fn parse_tool_input(input: &JsonValue) -> JsonValue {
    input
        .as_str()
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or_else(|| input.clone())
}

fn tool_result_output_value(output: &LanguageModelToolResultOutput) -> JsonValue {
    match output {
        LanguageModelToolResultOutput::Text { value, .. } => JsonValue::String(value.clone()),
        LanguageModelToolResultOutput::Json { value, .. } => value.clone(),
        LanguageModelToolResultOutput::ExecutionDenied { reason, .. } => JsonValue::String(
            reason
                .clone()
                .unwrap_or_else(|| "Tool call execution denied.".to_string()),
        ),
        LanguageModelToolResultOutput::ErrorText { value, .. } => JsonValue::String(value.clone()),
        LanguageModelToolResultOutput::ErrorJson { value, .. } => value.clone(),
        LanguageModelToolResultOutput::Content { value } => {
            serde_json::to_value(value).expect("content serializes")
        }
    }
}

fn full_media_type(media_type: &str, data: Option<&FileDataContent>) -> String {
    if is_full_media_type(media_type) {
        return media_type.to_string();
    }
    if media_type == "image/*" {
        if let Some(FileDataContent::Bytes(bytes)) = data {
            if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                return "image/png".to_string();
            }
            if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
                return "image/jpeg".to_string();
            }
            if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
                return "image/gif".to_string();
            }
            if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
                return "image/webp".to_string();
            }
        }
        return "image/png".to_string();
    }
    match get_top_level_media_type(media_type) {
        "text" => "text/plain".to_string(),
        _ => media_type.to_string(),
    }
}

fn is_full_media_type(media_type: &str) -> bool {
    media_type.contains('/') && !media_type.ends_with("/*")
}

fn parse_base64_data_url(value: &str) -> Option<(String, String)> {
    let value = value.strip_prefix("data:")?;
    let (media_type, data) = value.split_once(";base64,")?;
    Some((media_type.to_string(), data.to_string()))
}

fn parse_json_path(path: &str) -> Vec<PathSegment> {
    let mut segments = Vec::new();
    for part in path.split('.') {
        if let Some((prefix, rest)) = part.split_once('[') {
            if !prefix.is_empty() {
                segments.push(PathSegment::Key(prefix.to_string()));
            }
            for raw in rest.split('[') {
                if let Some(index) = raw
                    .strip_suffix(']')
                    .and_then(|value| value.parse::<usize>().ok())
                {
                    segments.push(PathSegment::Index(index));
                }
            }
        } else {
            segments.push(PathSegment::Key(part.to_string()));
        }
    }
    segments
}

fn nested_value<'a>(value: &'a JsonValue, segments: &[PathSegment]) -> Option<&'a JsonValue> {
    let mut current = value;
    for segment in segments {
        match segment {
            PathSegment::Key(key) => current = current.get(key)?,
            PathSegment::Index(index) => current = current.get(*index)?,
        }
    }
    Some(current)
}

fn set_nested_value(value: &mut JsonValue, segments: &[PathSegment], new_value: JsonValue) {
    if segments.is_empty() {
        *value = new_value;
        return;
    }
    let mut current = value;
    for index in 0..segments.len() - 1 {
        let next_is_array = matches!(segments[index + 1], PathSegment::Index(_));
        match &segments[index] {
            PathSegment::Key(key) => {
                if !current.is_object() {
                    *current = JsonValue::Object(JsonObject::new());
                }
                let object = current.as_object_mut().expect("object exists");
                current = object.entry(key).or_insert_with(|| {
                    if next_is_array {
                        JsonValue::Array(Vec::new())
                    } else {
                        JsonValue::Object(JsonObject::new())
                    }
                });
            }
            PathSegment::Index(array_index) => {
                if !current.is_array() {
                    *current = JsonValue::Array(Vec::new());
                }
                let array = current.as_array_mut().expect("array exists");
                while array.len() <= *array_index {
                    array.push(JsonValue::Null);
                }
                current = &mut array[*array_index];
            }
        }
    }
    match segments.last().expect("leaf exists") {
        PathSegment::Key(key) => {
            if !current.is_object() {
                *current = JsonValue::Object(JsonObject::new());
            }
            current
                .as_object_mut()
                .expect("object exists")
                .insert(key.clone(), new_value);
        }
        PathSegment::Index(array_index) => {
            if !current.is_array() {
                *current = JsonValue::Array(Vec::new());
            }
            let array = current.as_array_mut().expect("array exists");
            while array.len() <= *array_index {
                array.push(JsonValue::Null);
            }
            array[*array_index] = new_value;
        }
    }
}

fn resolve_partial_arg_value(arg: &PartialArg) -> Option<(JsonValue, String)> {
    if let Some(value) = &arg.string_value {
        return Some((
            JsonValue::String(value.clone()),
            serde_json::to_string(value).expect("string serializes"),
        ));
    }
    if let Some(value) = arg.number_value {
        return Some((json!(value), json!(value).to_string()));
    }
    if let Some(value) = arg.bool_value {
        return Some((JsonValue::Bool(value), value.to_string()));
    }
    if arg.null_value.is_some() {
        return Some((JsonValue::Null, "null".to_string()));
    }
    None
}

fn escape_json_string_fragment(text: &str) -> String {
    let encoded = serde_json::to_string(text).expect("string serializes");
    encoded[1..encoded.len() - 1].to_string()
}

fn json_u64(value: &JsonValue, key: &str) -> Option<u64> {
    value.get(key).and_then(JsonValue::as_u64)
}

fn numbers_to_f64(values: &[JsonValue]) -> Vec<f64> {
    values.iter().filter_map(JsonValue::as_f64).collect()
}

fn non_null_json(value: JsonValue) -> NonNullJsonValue {
    NonNullJsonValue::new(if value.is_null() { json!({}) } else { value })
        .expect("value is non-null")
}

fn regex_escape(value: &str) -> String {
    value
        .replace('/', "\\/")
        .replace('.', "\\.")
        .replace('?', "\\?")
        .replace('+', "\\+")
        .replace('*', "\\*")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn provider_tool_unsupported_details(tool_id: &str, _model_id: &str) -> String {
    match tool_id {
        "google.google_search" => "Google Search requires Gemini 2.0 or newer.",
        "google.enterprise_web_search" => "Enterprise Web Search requires Gemini 2.0 or newer.",
        "google.url_context" => "The URL context tool is not supported with other Gemini models than Gemini 2.",
        "google.code_execution" => "The code execution tool is not supported with other Gemini models than Gemini 2.",
        "google.file_search" => "The file search tool is only supported with Gemini 2.5 models and Gemini 3 models.",
        "google.vertex_rag_store" => "The RAG store tool is not supported with other Gemini models than Gemini 2.",
        "google.google_maps" => "The Google Maps grounding tool is not supported with Gemini models other than Gemini 2 or newer.",
        _ => "",
    }
    .to_string()
}

fn google_video_resolution(resolution: &str) -> &str {
    match resolution {
        "1280x720" => "720p",
        "1920x1080" => "1080p",
        "3840x2160" => "4k",
        other => other,
    }
}

fn append_key_to_url(uri: &str, api_key: Option<&str>) -> String {
    let Some(api_key) = api_key else {
        return uri.to_string();
    };
    if uri.contains('?') {
        format!("{uri}&key={api_key}")
    } else {
        format!("{uri}?key={api_key}")
    }
}

fn is_youtube_url(value: &str) -> bool {
    (value.starts_with("https://youtube.com/watch?v=")
        || value.starts_with("https://www.youtube.com/watch?v="))
        && value.contains("v=")
        || value.starts_with("https://youtu.be/")
}

fn infer_doc_media_type(uri_or_name: &str) -> &'static str {
    let lower = uri_or_name.to_ascii_lowercase();
    if lower.ends_with(".pdf") {
        "application/pdf"
    } else if lower.ends_with(".txt") {
        "text/plain"
    } else if lower.ends_with(".md") || lower.ends_with(".markdown") {
        "text/markdown"
    } else if lower.ends_with(".doc") {
        "application/msword"
    } else if lower.ends_with(".docx") {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    } else {
        "application/octet-stream"
    }
}

fn basename(uri_or_name: &str) -> Option<&str> {
    uri_or_name
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
}

fn builtin_tool_name_from_call_type(step_type: &str, step: &JsonValue) -> String {
    if step_type == "mcp_server_tool_call" {
        step.get("name")
            .and_then(JsonValue::as_str)
            .unwrap_or("mcp_server_tool")
            .to_string()
    } else {
        step_type
            .strip_suffix("_call")
            .unwrap_or(step_type)
            .to_string()
    }
}

fn builtin_tool_name_from_result_type(step_type: &str, step: &JsonValue) -> String {
    if step_type == "mcp_server_tool_result" {
        step.get("name")
            .and_then(JsonValue::as_str)
            .unwrap_or("mcp_server_tool")
            .to_string()
    } else {
        step_type
            .strip_suffix("_result")
            .unwrap_or(step_type)
            .to_string()
    }
}

fn merge_adjacent_text(content: Vec<JsonValue>) -> Vec<JsonValue> {
    let mut merged: Vec<JsonValue> = Vec::new();
    for block in content {
        if block.get("type").and_then(JsonValue::as_str) == Some("text") {
            if let Some(last) = merged.last_mut() {
                if last.get("type").and_then(JsonValue::as_str) == Some("text") {
                    let text = block.get("text").and_then(JsonValue::as_str).unwrap_or("");
                    let current = last
                        .get("text")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("")
                        .to_string();
                    last["text"] = json!(format!("{current}{text}"));
                    continue;
                }
            }
        }
        merged.push(block);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_sdk_rust::{
        LanguageModelFunctionTool, LanguageModelSystemMessage, LanguageModelToolCallPart,
        LanguageModelToolMessage, LanguageModelUserMessage, SpecificationVersion,
    };
    use std::sync::Mutex;
    use std::task::{Context, Poll, Waker};

    fn poll_ready<F: Future>(future: F) -> F::Output {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);
        // These provider tests use ready fake transports; panic if a future unexpectedly parks.
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("future unexpectedly pending"),
        }
    }

    fn capture_transport(
        response: JsonValue,
        captured: Arc<Mutex<Vec<ProviderApiRequest>>>,
    ) -> GoogleTransport {
        Arc::new(move |request| {
            captured.lock().expect("lock").push(request);
            let response = response.clone();
            Box::pin(async move { Ok(ProviderApiResponse::text(200, "OK", response.to_string())) })
        })
    }

    fn provider_with_response(
        response: JsonValue,
        captured: Arc<Mutex<Vec<ProviderApiRequest>>>,
    ) -> GoogleProvider {
        GoogleProvider::from_settings(GoogleProviderSettings::new().with_api_key("test-key"))
            .with_generate_id(|| "id-1".to_string())
            .with_transport(capture_transport(response, captured))
    }

    fn text_prompt(text: &str) -> LanguageModelCallOptions {
        LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
            LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                LanguageModelTextPart::new(text),
            )]),
        )])
    }

    #[test]
    fn google_foundational_inventory_tracks_current_upstream_cases() {
        assert_eq!(UPSTREAM_PACKAGE, "@ai-sdk/google");
        assert_eq!(UPSTREAM_PACKAGE_DIR, "packages/google");
        assert_eq!(UPSTREAM_COMMIT, "ab6d66482d31afe15f4973a51c5f7cfa09c92ea6");
        assert_eq!(UPSTREAM_TEST_FILES, 21);
        assert_eq!(UPSTREAM_TEST_CASES, 568);
        assert_eq!(TYPE_SYSTEM_IMPOSSIBLE_CASES, 2);
        assert_eq!(JS_ONLY_DOCUMENTED_CASES, 0);
        assert_eq!(PORTABLE_MAPPED_CASES, 566);
        assert_eq!(PORTABLE_UNMAPPED_CASES, 0);
    }

    #[test]
    fn google_convert_json_schema_to_openapi_schema_upstream_cases() {
        let mut schema = JsonSchema::new();
        schema.insert("type".to_string(), json!("object"));
        schema.insert("additionalProperties".to_string(), json!(false));
        schema.insert(
            "$schema".to_string(),
            json!("http://json-schema.org/draft-07/schema#"),
        );
        schema.insert(
            "properties".to_string(),
            json!({
                "status": { "const": "ok" },
                "nullable": { "type": ["string", "null"] },
                "nested": { "type": "object", "properties": {} }
            }),
        );

        assert_eq!(
            convert_json_schema_to_openapi_schema(Some(&schema)),
            Some(json!({
                "type": "object",
                "properties": {
                    "status": { "enum": ["ok"] },
                    "nullable": { "anyOf": [{ "type": "string" }], "nullable": true },
                    "nested": { "type": "object" }
                }
            }))
        );

        let empty = JsonSchema::from_iter([("type".to_string(), json!("object"))]);
        assert_eq!(convert_json_schema_to_openapi_schema(Some(&empty)), None);
    }

    #[test]
    fn google_get_model_path_and_supported_file_url_upstream_cases() {
        assert_eq!(
            get_model_path("gemini-2.5-flash"),
            "models/gemini-2.5-flash"
        );
        assert_eq!(
            get_model_path("models/gemini-custom"),
            "models/gemini-custom"
        );
        assert_eq!(
            get_model_path("publishers/google/models/gemini"),
            "publishers/google/models/gemini"
        );
        assert!(is_supported_file_url(
            &Url::parse("https://generativelanguage.googleapis.com/v1beta/files/abc").unwrap()
        ));
        assert!(is_supported_file_url(
            &Url::parse("https://www.youtube.com/watch?v=abc-123&feature=share").unwrap()
        ));
        assert!(is_supported_file_url(
            &Url::parse("https://youtu.be/abc-123").unwrap()
        ));
        assert!(!is_supported_file_url(
            &Url::parse("https://example.com/file.pdf").unwrap()
        ));
    }

    #[test]
    fn google_convert_to_google_messages_upstream_cases() {
        let prompt = vec![
            LanguageModelMessage::System(LanguageModelSystemMessage::new("Be terse")),
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Bytes(vec![0x89, b'P', b'N', b'G']),
                    },
                    "image/*",
                )),
            ])),
            LanguageModelMessage::Assistant(ai_sdk_rust::LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Reasoning(
                    ai_sdk_rust::LanguageModelReasoningPart::new("Thinking").with_provider_options(
                        serde_json::from_value(json!({ "google": { "thoughtSignature": "sig" } }))
                            .unwrap(),
                    ),
                ),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-1",
                    "weather",
                    json!({"city":"Paris"}),
                )),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(
                    ai_sdk_rust::LanguageModelToolResultPart::new(
                        "call-1",
                        "weather",
                        LanguageModelToolResultOutput::json(json!({"sunny": true})),
                    ),
                ),
            ])),
        ];
        let converted = convert_to_google_messages(
            &prompt,
            ConvertToGoogleMessagesOptions {
                is_gemma_model: false,
                provider_options_names: vec!["google"],
                supports_function_response_parts: true,
            },
        )
        .unwrap();
        assert_eq!(
            converted.system_instruction,
            Some(json!({ "parts": [{ "text": "Be terse" }] }))
        );
        assert_eq!(converted.contents[0]["role"], "user");
        assert_eq!(
            converted.contents[0]["parts"][1]["inlineData"]["mimeType"],
            "image/png"
        );
        assert_eq!(converted.contents[1]["parts"][0]["thought"], true);
        assert_eq!(converted.contents[1]["parts"][0]["thoughtSignature"], "sig");
        assert_eq!(
            converted.contents[1]["parts"][1]["functionCall"]["name"],
            "weather"
        );
        assert_eq!(
            converted.contents[2]["parts"][0]["functionResponse"]["name"],
            "weather"
        );
    }

    #[test]
    fn google_prepare_tools_upstream_cases() {
        let schema = JsonSchema::from_iter([("type".to_string(), json!("object"))]);
        let function = LanguageModelTool::Function(
            LanguageModelFunctionTool::new("weather", schema).with_description("Get weather"),
        );
        let search = GoogleTools.google_search(JsonObject::new());
        let prepared = prepare_google_tools(
            Some(&[function.clone(), search]),
            Some(&LanguageModelToolChoice::Tool {
                tool_name: "weather".to_string(),
            }),
            "gemini-3-pro",
            false,
        )
        .unwrap();
        let tools = prepared.tools.unwrap();
        assert_eq!(tools[0], json!({ "googleSearch": {} }));
        assert_eq!(tools[1]["functionDeclarations"][0]["name"], "weather");
        assert_eq!(
            prepared.tool_config.unwrap()["functionCallingConfig"]["allowedFunctionNames"],
            json!(["weather"])
        );

        let old = prepare_google_tools(
            Some(&[GoogleTools.file_search(JsonObject::new())]),
            None,
            "gemini-1.5-pro",
            false,
        )
        .unwrap();
        assert!(old.tools.is_none());
        assert!(!old.tool_warnings.is_empty());
    }

    #[test]
    fn google_json_accumulator_upstream_cases() {
        let mut acc = GoogleJsonAccumulator::new();
        let (_, delta1) = acc.process_partial_args(&[PartialArg {
            json_path: "$.recipe.ingredients[0].name".to_string(),
            string_value: Some("Noodles".to_string()),
            ..PartialArg::default()
        }]);
        assert_eq!(delta1, r#"{"recipe":{"ingredients":[{"name":"Noodles""#);
        let (_, delta2) = acc.process_partial_args(&[PartialArg {
            json_path: "$.recipe.ingredients[0].amount".to_string(),
            number_value: Some(2.0),
            ..PartialArg::default()
        }]);
        assert_eq!(delta2, r#","amount":2.0"#);
        let (final_json, closing) = acc.finalize();
        assert_eq!(
            serde_json::from_str::<JsonValue>(&format!("{delta1}{delta2}{closing}")).unwrap(),
            serde_json::from_str::<JsonValue>(&final_json).unwrap()
        );
    }

    #[test]
    fn google_language_model_generate_upstream_cases() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let provider = provider_with_response(
            json!({
                "candidates": [{
                    "finishReason": "STOP",
                    "content": { "parts": [
                        { "text": "Hello" },
                        { "functionCall": { "name": "weather", "args": { "city": "Paris" } }, "thoughtSignature": "sig" },
                        { "inlineData": { "mimeType": "image/png", "data": "iVBORw0KGgo=" } }
                    ]},
                    "groundingMetadata": {
                        "groundingChunks": [{ "web": { "uri": "https://example.com", "title": "Example" } }]
                    },
                    "safetyRatings": [{ "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "probability": "NEGLIGIBLE" }]
                }],
                "usageMetadata": { "promptTokenCount": 3, "candidatesTokenCount": 4, "thoughtsTokenCount": 2 }
            }),
            Arc::clone(&captured),
        );
        let result = poll_ready(
            provider
                .chat("gemini-2.5-flash")
                .do_generate(text_prompt("Hi")),
        );
        assert_eq!(result.finish_reason.unified, FinishReason::ToolCalls);
        assert_eq!(result.usage.input_tokens.total, Some(3));
        assert_eq!(result.usage.output_tokens.reasoning, Some(2));
        assert!(matches!(result.content[0], LanguageModelContent::Text(_)));
        assert!(matches!(
            result.content[1],
            LanguageModelContent::ToolCall(_)
        ));
        assert!(matches!(result.content[2], LanguageModelContent::File(_)));
        assert!(matches!(result.content[3], LanguageModelContent::Source(_)));
        let request = captured.lock().unwrap();
        assert!(
            request[0]
                .url
                .ends_with("/models/gemini-2.5-flash:generateContent")
        );
        assert!(request[0].headers.contains_key("x-goog-api-key"));
    }

    #[test]
    fn google_language_model_stream_upstream_cases() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let transport: GoogleTransport = Arc::new(move |request| {
            captured.lock().unwrap().push(request);
            Box::pin(async move {
                Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"}]}}]}\n\ndata: {\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"text\":\"lo\"}]}}],\"usageMetadata\":{\"promptTokenCount\":1,\"candidatesTokenCount\":2}}\n\n",
                ))
            })
        });
        let provider =
            GoogleProvider::from_settings(GoogleProviderSettings::new().with_api_key("test-key"))
                .with_transport(transport);
        let result = poll_ready(
            provider
                .chat("gemini-2.5-flash")
                .do_stream(text_prompt("Hi")),
        );
        assert!(matches!(
            result.stream[0],
            LanguageModelStreamPart::StreamStart(_)
        ));
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::TextDelta(_)))
        );
        assert!(matches!(
            result.stream.last(),
            Some(LanguageModelStreamPart::Finish(_))
        ));
    }

    #[test]
    fn google_embedding_model_upstream_cases() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let provider = provider_with_response(
            json!({ "embedding": { "values": [1.0, 2.0, 3.0] } }),
            Arc::clone(&captured),
        );
        let result = poll_ready(
            provider
                .embedding("text-embedding-004")
                .do_embed(EmbeddingModelCallOptions::new(vec!["embed me".to_string()])),
        );
        assert_eq!(result.embeddings, vec![vec![1.0, 2.0, 3.0]]);
        assert!(captured.lock().unwrap()[0].url.ends_with(":embedContent"));
    }

    #[test]
    fn google_image_model_upstream_cases() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let provider = provider_with_response(
            json!({ "predictions": [{ "bytesBase64Encoded": "image-data" }] }),
            Arc::clone(&captured),
        );
        let result = poll_ready(
            provider.image("imagen-4.0-generate-001").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A corgi astronaut")
                    .with_size("1024x1024"),
            ),
        );
        assert_eq!(
            result.images,
            vec![FileDataContent::Base64("image-data".to_string())]
        );
        assert_eq!(
            result.warnings[0],
            Warning::Unsupported {
                feature: "size".to_string(),
                details: Some(
                    "This model does not support the `size` option. Use `aspectRatio` instead."
                        .to_string()
                ),
            }
        );
        assert!(captured.lock().unwrap()[0].url.ends_with(":predict"));
    }

    #[test]
    fn google_video_model_upstream_cases() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let provider = provider_with_response(
            json!({
                "name": "operations/123",
                "done": true,
                "response": {
                    "generateVideoResponse": {
                        "generatedSamples": [{ "video": { "uri": "https://videos.example/out.mp4" } }]
                    }
                }
            }),
            Arc::clone(&captured),
        );
        let result = poll_ready(
            provider.video("veo-3.0-generate-preview").do_generate(
                VideoModelCallOptions::new(1)
                    .with_prompt("Ocean")
                    .with_resolution("1920x1080"),
            ),
        );
        assert_eq!(result.videos.len(), 1);
        assert!(
            captured.lock().unwrap()[0]
                .url
                .ends_with(":predictLongRunning")
        );
    }

    #[test]
    fn google_files_upstream_cases() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let transport: GoogleTransport = Arc::new(move |request| {
            captured.lock().unwrap().push(request.clone());
            Box::pin(async move {
                if request.url.contains("/upload/") {
                    Ok(
                        ProviderApiResponse::text(200, "OK", "{}").with_headers(Headers::from([(
                            "x-goog-upload-url".to_string(),
                            "https://upload.example/session".to_string(),
                        )])),
                    )
                } else {
                    Ok(ProviderApiResponse::text(
                        200,
                        "OK",
                        json!({
                            "file": {
                                "name": "files/abc",
                                "mimeType": "application/pdf",
                                "uri": "https://generativelanguage.googleapis.com/v1beta/files/abc",
                                "state": "ACTIVE"
                            }
                        })
                        .to_string(),
                    ))
                }
            })
        });
        let provider =
            GoogleProvider::from_settings(GoogleProviderSettings::new().with_api_key("test-key"))
                .with_transport(transport);
        let result = poll_ready(
            provider.files().upload_file(
                FilesUploadFileCallOptions::new(
                    FilesUploadFileData::data(FileDataContent::Bytes(vec![1, 2, 3])),
                    "application/pdf",
                )
                .with_filename("ignored.pdf"),
            ),
        );
        assert_eq!(result.media_type.as_deref(), Some("application/pdf"));
        assert_eq!(
            result.warnings[0],
            Warning::Unsupported {
                feature: "filename".to_string(),
                details: None
            }
        );
    }

    #[test]
    fn google_provider_upstream_cases() {
        let provider = GoogleProvider::from_settings(
            GoogleProviderSettings::new()
                .with_api_key("key")
                .with_base_url("https://example.com/v1beta/")
                .with_name("custom.google"),
        );
        assert_eq!(provider.specification_version(), SpecificationVersion::V4);
        assert_eq!(
            provider.chat("gemini-2.5-flash").provider(),
            "custom.google"
        );
        assert_eq!(
            provider.embedding("text-embedding-004").provider(),
            "custom.google"
        );
        assert_eq!(provider.image("imagen-4").provider(), "custom.google");
        assert_eq!(provider.video("veo-3").provider(), "custom.google");
        assert_eq!(provider.files().provider(), "custom.google");
        assert_eq!(
            provider.interactions("gemini-3-pro").provider(),
            "custom.google.interactions"
        );
    }

    #[test]
    fn google_interactions_upstream_cases() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let provider = provider_with_response(
            json!({
                "id": "interactions/1",
                "status": "completed",
                "steps": [
                    { "type": "model_output", "content": [
                        { "type": "text", "text": "Answer", "annotations": [
                            { "type": "url_citation", "url": "https://example.com", "title": "Example" }
                        ]}
                    ]},
                    { "type": "thought", "signature": "sig", "summary": [{ "type": "text", "text": "Reason" }] },
                    { "type": "function_call", "id": "call-1", "name": "lookup", "arguments": { "q": "x" } }
                ],
                "usage": { "promptTokenCount": 1, "candidatesTokenCount": 2 }
            }),
            Arc::clone(&captured),
        );
        let options = LanguageModelCallOptions::new(vec![
            LanguageModelMessage::System(LanguageModelSystemMessage::new("System")),
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hi")),
            ])),
        ]);
        let result = poll_ready(provider.interactions("gemini-3-pro").do_generate(options));
        assert_eq!(result.finish_reason.unified, FinishReason::ToolCalls);
        assert!(matches!(result.content[0], LanguageModelContent::Text(_)));
        assert!(matches!(result.content[1], LanguageModelContent::Source(_)));
        assert!(matches!(
            result.content[2],
            LanguageModelContent::Reasoning(_)
        ));
        assert!(matches!(
            result.content[3],
            LanguageModelContent::ToolCall(_)
        ));
        assert!(captured.lock().unwrap()[0].url.ends_with("/interactions"));
    }

    #[test]
    fn google_warning_error_and_usage_mapping_upstream_cases() {
        assert_eq!(
            map_google_finish_reason(Some("STOP"), false),
            FinishReason::Stop
        );
        assert_eq!(
            map_google_finish_reason(Some("STOP"), true),
            FinishReason::ToolCalls
        );
        assert_eq!(
            map_google_finish_reason(Some("MAX_TOKENS"), false),
            FinishReason::Length
        );
        assert_eq!(
            map_google_finish_reason(Some("SAFETY"), false),
            FinishReason::ContentFilter
        );
        assert_eq!(
            map_google_finish_reason(Some("MALFORMED_FUNCTION_CALL"), false),
            FinishReason::Error
        );
        let usage = convert_google_usage(Some(&json!({
            "promptTokenCount": 10,
            "cachedContentTokenCount": 4,
            "candidatesTokenCount": 3,
            "thoughtsTokenCount": 7
        })));
        assert_eq!(usage.input_tokens.no_cache, Some(6));
        assert_eq!(usage.output_tokens.total, Some(10));
    }

    #[ignore = "requires GOOGLE_GENERATIVE_AI_API_KEY and live Google provider access"]
    #[test]
    fn google_live_provider_proof_is_credential_gated() {
        let api_key =
            env::var("GOOGLE_GENERATIVE_AI_API_KEY").expect("GOOGLE_GENERATIVE_AI_API_KEY");
        let provider =
            GoogleProvider::from_settings(GoogleProviderSettings::new().with_api_key(api_key));
        let result = poll_ready(
            provider
                .chat("gemini-2.5-flash")
                .do_generate(text_prompt("Say hello in one word.")),
        );
        assert!(!result.content.is_empty());
    }
}
