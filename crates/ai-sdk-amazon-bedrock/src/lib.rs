//! Rust port of upstream `@ai-sdk/amazon-bedrock`.
//!
//! The crate mirrors the provider-owned runtime surface for Amazon Bedrock:
//! Converse chat, Anthropic-on-Bedrock request transformation, embeddings,
//! Nova image generation/editing, agent-runtime reranking, Bedrock event-stream
//! decoding, and SigV4/API-key request authentication. Network behavior is
//! exercised through an injectable transport so upstream fixture parity can be
//! tested without live AWS credentials.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_rust::{
    EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResponse, EmbeddingModelResult,
    EmbeddingModelUsage, FetchErrorInfo, FileData, FileDataContent, FinishReason, Headers,
    ImageModel, ImageModelCallOptions, ImageModelFile, ImageModelResponse, ImageModelResult,
    InputTokenUsage, JsonObject, JsonValue, LanguageModel, LanguageModelAssistantContentPart,
    LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelContent,
    LanguageModelErrorStreamPart, LanguageModelFilePart, LanguageModelFinishReason,
    LanguageModelFunctionTool, LanguageModelGenerateResult, LanguageModelMessage,
    LanguageModelRawStreamPart, LanguageModelReasoning, LanguageModelReasoningDelta,
    LanguageModelReasoningEnd, LanguageModelReasoningStart, LanguageModelRequest,
    LanguageModelResponse, LanguageModelResponseFormat, LanguageModelStreamFinish,
    LanguageModelStreamPart, LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
    LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
    LanguageModelSystemMessage, LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd,
    LanguageModelTextStart, LanguageModelTool, LanguageModelToolCall, LanguageModelToolChoice,
    LanguageModelToolContentPart, LanguageModelToolInputDelta, LanguageModelToolInputEnd,
    LanguageModelToolInputStart, LanguageModelToolResultContentPart, LanguageModelToolResultOutput,
    LanguageModelUserContentPart, LanguageModelUserMessage, ModelType, NoSuchModelError,
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    OpenAICompatibleProvider, OpenAICompatibleProviderSettings, OutputTokenUsage,
    PostJsonToApiOptions, Provider, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, ProviderApiResponseHandlerError,
    ProviderMetadata, ProviderOptions, ProviderWithRerankingModel, RerankingModel,
    RerankingModelCallOptions, RerankingModelDocuments, RerankingModelRanking,
    RerankingModelResponse, RerankingModelResult, RuntimeEnvironment,
    UnsupportedFunctionalityError, Warning, combine_headers, convert_to_base64,
    create_json_error_response_handler, create_json_response_handler, get_top_level_media_type,
    is_full_media_type, post_json_to_api, resolve_full_media_type, strip_file_extension,
    without_trailing_slash,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use url::Url;

type HmacSha256 = Hmac<Sha256>;

/// Upstream package covered by this crate.
pub const UPSTREAM_PACKAGE: &str = "@ai-sdk/amazon-bedrock";

/// Upstream package directory in `vercel/ai`.
pub const UPSTREAM_PACKAGE_DIR: &str = "packages/amazon-bedrock";

/// Upstream commit used for the checked-in AI-01 inventory.
pub const UPSTREAM_COMMIT: &str = "ab6d66482d31afe15f4973a51c5f7cfa09c92ea6";

/// Checked-in row-level inventory document for this package.
pub const INVENTORY_DOCUMENT: &str = "docs/ai-foundational-provider-inventory.md";

/// Current upstream test files under `packages/amazon-bedrock/src`.
pub const UPSTREAM_TEST_FILES: usize = 17;

/// Current detected upstream `it`/`test` cases under `packages/amazon-bedrock/src`.
pub const UPSTREAM_TEST_CASES: usize = 383;

/// Current explicit TypeScript type-system exceptions.
pub const TYPE_SYSTEM_IMPOSSIBLE_CASES: usize = 0;

/// Current explicit JavaScript runtime exceptions.
pub const JS_ONLY_DOCUMENTED_CASES: usize = 3;

/// Portable upstream cases mapped to named Rust tests in this crate.
pub const PORTABLE_MAPPED_CASES: usize =
    UPSTREAM_TEST_CASES - TYPE_SYSTEM_IMPOSSIBLE_CASES - JS_ONLY_DOCUMENTED_CASES;

/// Portable cases still requiring named Rust tests.
pub const PORTABLE_UNMAPPED_CASES: usize = 0;

/// Default AWS Bedrock Runtime service endpoint region used when no region is configured.
pub const DEFAULT_AMAZON_BEDROCK_REGION: &str = "us-east-1";

/// Default provider id for Bedrock Converse models.
pub const AMAZON_BEDROCK_PROVIDER_ID: &str = "amazon-bedrock";

/// Default user-agent suffix applied to Bedrock provider requests.
pub const AMAZON_BEDROCK_USER_AGENT: &str =
    concat!("ai-sdk/amazon-bedrock/", env!("CARGO_PKG_VERSION"));

/// Bedrock prompt-cache TTL values.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AmazonBedrockCacheTtl {
    /// Five-minute prompt-cache TTL.
    #[serde(rename = "5m")]
    FiveMinutes,
    /// One-hour prompt-cache TTL.
    #[serde(rename = "1h")]
    OneHour,
}

impl AmazonBedrockCacheTtl {
    /// Returns the upstream TTL string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FiveMinutes => "5m",
            Self::OneHour => "1h",
        }
    }
}

/// Creates an upstream-compatible Bedrock cache point provider option.
pub fn create_amazon_bedrock_cache_point(ttl: Option<AmazonBedrockCacheTtl>) -> JsonValue {
    let mut cache_point = JsonObject::new();
    cache_point.insert("type".to_string(), json!("default"));
    if let Some(ttl) = ttl {
        cache_point.insert("ttl".to_string(), json!(ttl.as_str()));
    }
    json!({ "cachePoint": cache_point })
}

/// AWS credentials used for SigV4 signing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmazonBedrockCredentials {
    /// AWS region.
    pub region: String,
    /// AWS access key id.
    pub access_key_id: String,
    /// AWS secret access key.
    pub secret_access_key: String,
    /// Optional AWS session token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
}

impl AmazonBedrockCredentials {
    /// Creates explicit SigV4 credentials.
    pub fn new(
        region: impl Into<String>,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        Self {
            region: region.into(),
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
            session_token: None,
        }
    }

    /// Sets the optional AWS session token.
    pub fn with_session_token(mut self, session_token: impl Into<String>) -> Self {
        self.session_token = Some(session_token.into());
        self
    }
}

/// Future returned by an injected Amazon Bedrock HTTP transport.
pub type AmazonBedrockTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Bedrock provider models.
pub type AmazonBedrockTransport =
    Arc<dyn Fn(ProviderApiRequest) -> AmazonBedrockTransportFuture + Send + Sync>;

/// Dynamic SigV4 credential provider.
pub type AmazonBedrockCredentialProvider =
    Arc<dyn Fn() -> Result<AmazonBedrockCredentials, String> + Send + Sync>;

type AmazonBedrockDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type AmazonBedrockIdGenerator = Arc<dyn Fn() -> String + Send + Sync>;

/// Settings for the upstream Amazon Bedrock provider.
#[derive(Clone, Default)]
pub struct AmazonBedrockProviderSettings {
    /// AWS region. Falls back to `AWS_REGION`, then `us-east-1`.
    pub region: Option<String>,
    /// Bearer token API key. Takes precedence over SigV4 credentials.
    pub api_key: Option<String>,
    /// AWS access key id for SigV4.
    pub access_key_id: Option<String>,
    /// AWS secret access key for SigV4.
    pub secret_access_key: Option<String>,
    /// Optional AWS session token for SigV4.
    pub session_token: Option<String>,
    /// Optional base URL for Runtime and Agent Runtime calls.
    pub base_url: Option<String>,
    /// Custom headers included with every request.
    pub headers: Headers,
    /// Optional dynamic credential provider.
    pub credential_provider: Option<AmazonBedrockCredentialProvider>,
}

impl AmazonBedrockProviderSettings {
    /// Creates empty provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the AWS region.
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Sets the bearer-token API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets static SigV4 credentials.
    pub fn with_credentials(
        mut self,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        self.access_key_id = Some(access_key_id.into());
        self.secret_access_key = Some(secret_access_key.into());
        self
    }

    /// Sets the optional AWS session token.
    pub fn with_session_token(mut self, session_token: impl Into<String>) -> Self {
        self.session_token = Some(session_token.into());
        self
    }

    /// Sets a custom base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Sets a dynamic SigV4 credential provider.
    pub fn with_credential_provider<F>(mut self, provider: F) -> Self
    where
        F: Fn() -> Result<AmazonBedrockCredentials, String> + Send + Sync + 'static,
    {
        self.credential_provider = Some(Arc::new(provider));
        self
    }
}

/// Upstream Amazon Bedrock provider foundation.
#[derive(Clone)]
pub struct AmazonBedrockProvider {
    settings: AmazonBedrockProviderSettings,
    transport: AmazonBedrockTransport,
    current_date: AmazonBedrockDateProvider,
    generate_id: AmazonBedrockIdGenerator,
}

impl AmazonBedrockProvider {
    /// Creates a provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(AmazonBedrockProviderSettings::new())
    }

    /// Creates a provider from explicit settings.
    pub fn from_settings(settings: AmazonBedrockProviderSettings) -> Self {
        Self {
            settings,
            transport: default_amazon_bedrock_transport(),
            current_date: Arc::new(OffsetDateTime::now_utc),
            generate_id: Arc::new(default_generate_id),
        }
    }

    /// Sets the AWS region.
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.settings.region = Some(region.into());
        self
    }

    /// Sets the bearer-token API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets a custom base URL.
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
    pub fn with_transport(mut self, transport: AmazonBedrockTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Replaces the response timestamp/signing-date provider.
    pub fn with_current_date<F>(mut self, current_date: F) -> Self
    where
        F: Fn() -> OffsetDateTime + Send + Sync + 'static,
    {
        self.current_date = Arc::new(current_date);
        self
    }

    /// Replaces the generated-id provider.
    pub fn with_generate_id<F>(mut self, generate_id: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        self.generate_id = Arc::new(generate_id);
        self
    }

    /// Creates a Bedrock Converse chat model.
    pub fn language_model(&self, model_id: impl Into<String>) -> AmazonBedrockChatLanguageModel {
        self.chat(model_id)
    }

    /// Creates a Bedrock Converse chat model.
    pub fn chat(&self, model_id: impl Into<String>) -> AmazonBedrockChatLanguageModel {
        AmazonBedrockChatLanguageModel::new(
            model_id.into(),
            AmazonBedrockModelConfig::new(
                AMAZON_BEDROCK_PROVIDER_ID,
                self.runtime_base_url(),
                self.settings.headers.clone(),
                self.auth_config("bedrock"),
                Arc::clone(&self.transport),
                Arc::clone(&self.current_date),
                Arc::clone(&self.generate_id),
            ),
        )
    }

    /// Creates a Bedrock embedding model.
    pub fn embedding(&self, model_id: impl Into<String>) -> AmazonBedrockEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates a Bedrock embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> AmazonBedrockEmbeddingModel {
        AmazonBedrockEmbeddingModel::new(
            model_id.into(),
            AmazonBedrockModelConfig::new(
                AMAZON_BEDROCK_PROVIDER_ID,
                self.runtime_base_url(),
                self.settings.headers.clone(),
                self.auth_config("bedrock"),
                Arc::clone(&self.transport),
                Arc::clone(&self.current_date),
                Arc::clone(&self.generate_id),
            ),
        )
    }

    /// Deprecated upstream alias for embedding model lookup.
    pub fn text_embedding(&self, model_id: impl Into<String>) -> AmazonBedrockEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for embedding model lookup.
    pub fn text_embedding_model(&self, model_id: impl Into<String>) -> AmazonBedrockEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates a Bedrock image model.
    pub fn image(&self, model_id: impl Into<String>) -> AmazonBedrockImageModel {
        self.image_model(model_id)
    }

    /// Creates a Bedrock image model.
    pub fn image_model(&self, model_id: impl Into<String>) -> AmazonBedrockImageModel {
        AmazonBedrockImageModel::new(
            model_id.into(),
            AmazonBedrockModelConfig::new(
                AMAZON_BEDROCK_PROVIDER_ID,
                self.runtime_base_url(),
                self.settings.headers.clone(),
                self.auth_config("bedrock"),
                Arc::clone(&self.transport),
                Arc::clone(&self.current_date),
                Arc::clone(&self.generate_id),
            ),
        )
    }

    /// Creates a Bedrock Agent Runtime reranking model.
    pub fn reranking(&self, model_id: impl Into<String>) -> AmazonBedrockRerankingModel {
        self.reranking_model(model_id)
    }

    /// Creates a Bedrock Agent Runtime reranking model.
    pub fn reranking_model(&self, model_id: impl Into<String>) -> AmazonBedrockRerankingModel {
        AmazonBedrockRerankingModel::new(
            model_id.into(),
            self.region(),
            AmazonBedrockModelConfig::new(
                AMAZON_BEDROCK_PROVIDER_ID,
                self.agent_runtime_base_url(),
                self.settings.headers.clone(),
                self.auth_config("bedrock"),
                Arc::clone(&self.transport),
                Arc::clone(&self.current_date),
                Arc::clone(&self.generate_id),
            ),
        )
    }

    fn runtime_base_url(&self) -> String {
        let base_url =
            self.settings.base_url.clone().unwrap_or_else(|| {
                format!("https://bedrock-runtime.{}.amazonaws.com", self.region())
            });
        without_trailing_slash(Some(&base_url))
            .unwrap_or(&base_url)
            .to_string()
    }

    fn agent_runtime_base_url(&self) -> String {
        let base_url = self.settings.base_url.clone().unwrap_or_else(|| {
            format!(
                "https://bedrock-agent-runtime.{}.amazonaws.com",
                self.region()
            )
        });
        without_trailing_slash(Some(&base_url))
            .unwrap_or(&base_url)
            .to_string()
    }

    fn region(&self) -> String {
        first_non_empty([
            self.settings.region.clone(),
            env::var("AWS_REGION").ok(),
            Some(DEFAULT_AMAZON_BEDROCK_REGION.to_string()),
        ])
        .expect("default Bedrock region is present")
    }

    fn auth_config(&self, service: &'static str) -> AmazonBedrockAuthConfig {
        let api_key = first_non_empty([
            self.settings.api_key.clone(),
            env::var("AWS_BEARER_TOKEN_BEDROCK").ok(),
        ]);

        if let Some(api_key) = api_key {
            return AmazonBedrockAuthConfig::ApiKey { api_key };
        }

        AmazonBedrockAuthConfig::SigV4 {
            service,
            region: self.region(),
            settings: self.settings.clone(),
        }
    }
}

impl Default for AmazonBedrockProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for AmazonBedrockProvider {
    type LanguageModel = AmazonBedrockChatLanguageModel;
    type EmbeddingModel = AmazonBedrockEmbeddingModel;
    type ImageModel = AmazonBedrockImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(AmazonBedrockProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(AmazonBedrockProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(AmazonBedrockProvider::image_model(self, model_id))
    }
}

impl ProviderWithRerankingModel for AmazonBedrockProvider {
    type RerankingModel = AmazonBedrockRerankingModel;

    fn reranking_model(&self, model_id: &str) -> Result<Self::RerankingModel, NoSuchModelError> {
        Ok(AmazonBedrockProvider::reranking_model(self, model_id))
    }
}

/// Creates a provider with explicit settings.
pub fn create_amazon_bedrock(settings: AmazonBedrockProviderSettings) -> AmazonBedrockProvider {
    AmazonBedrockProvider::from_settings(settings)
}

/// Creates the default Amazon Bedrock provider.
pub fn amazon_bedrock_provider() -> AmazonBedrockProvider {
    AmazonBedrockProvider::new()
}

/// Creates a chat language model using default provider settings.
pub fn amazon_bedrock(model_id: impl Into<String>) -> AmazonBedrockChatLanguageModel {
    AmazonBedrockProvider::new().language_model(model_id)
}

#[derive(Clone)]
enum AmazonBedrockAuthConfig {
    ApiKey {
        api_key: String,
    },
    SigV4 {
        service: &'static str,
        region: String,
        settings: AmazonBedrockProviderSettings,
    },
}

#[derive(Clone)]
struct AmazonBedrockModelConfig {
    provider: String,
    base_url: String,
    headers: Headers,
    auth: AmazonBedrockAuthConfig,
    transport: AmazonBedrockTransport,
    current_date: AmazonBedrockDateProvider,
    generate_id: AmazonBedrockIdGenerator,
}

impl AmazonBedrockModelConfig {
    fn new(
        provider: impl Into<String>,
        base_url: impl Into<String>,
        headers: Headers,
        auth: AmazonBedrockAuthConfig,
        transport: AmazonBedrockTransport,
        current_date: AmazonBedrockDateProvider,
        generate_id: AmazonBedrockIdGenerator,
    ) -> Self {
        Self {
            provider: provider.into(),
            base_url: base_url.into(),
            headers,
            auth,
            transport,
            current_date,
            generate_id,
        }
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(
                headers_with_user_agent_suffix(&self.headers)
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
        ])
    }

    fn authenticated_transport(
        &self,
    ) -> impl FnOnce(ProviderApiRequest) -> AmazonBedrockTransportFuture + Send + 'static {
        let transport = Arc::clone(&self.transport);
        let auth = self.auth.clone();
        let date = (self.current_date)();

        move |mut request| {
            let authenticated = authenticate_request(&auth, &mut request, date);
            Box::pin(async move {
                authenticated?;
                (transport)(request).await
            })
        }
    }
}

/// Bedrock Converse language model.
#[derive(Clone)]
pub struct AmazonBedrockChatLanguageModel {
    model_id: String,
    config: AmazonBedrockModelConfig,
}

impl AmazonBedrockChatLanguageModel {
    fn new(model_id: String, config: AmazonBedrockModelConfig) -> Self {
        Self { model_id, config }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Prepares the Bedrock Converse request body and warnings.
    pub fn request_body(
        &self,
        options: &LanguageModelCallOptions,
    ) -> Result<(JsonValue, Vec<Warning>, bool), UnsupportedFunctionalityError> {
        let mut warnings = Vec::new();
        let mut provider_options = bedrock_provider_options(options.provider_options.as_ref());

        if options.frequency_penalty.is_some() {
            warnings.push(unsupported("frequencyPenalty", None));
        }
        if options.presence_penalty.is_some() {
            warnings.push(unsupported("presencePenalty", None));
        }
        if options.seed.is_some() {
            warnings.push(unsupported("seed", None));
        }

        let mut temperature = options.temperature;
        if let Some(value) = temperature {
            if value > 1.0 {
                warnings.push(unsupported(
                    "temperature",
                    Some(format!(
                        "{value} exceeds bedrock maximum of 1.0. clamped to 1.0"
                    )),
                ));
                temperature = Some(1.0);
            } else if value < 0.0 {
                warnings.push(unsupported(
                    "temperature",
                    Some(format!(
                        "{value} is below bedrock minimum of 0. clamped to 0"
                    )),
                ));
                temperature = Some(0.0);
            }
        }

        apply_reasoning_options(
            &self.model_id,
            options.reasoning.as_ref(),
            &mut provider_options,
            &mut warnings,
        );

        let mut tools = options.tools.clone().unwrap_or_default();
        let mut uses_json_response_tool = false;
        let is_anthropic = is_anthropic_model(&self.model_id);
        if let Some(LanguageModelResponseFormat::Json {
            schema: Some(schema),
            ..
        }) = &options.response_format
        {
            let use_native_structured_output = is_anthropic;
            if !use_native_structured_output {
                uses_json_response_tool = true;
                tools.push(LanguageModelTool::Function(
                    LanguageModelFunctionTool::new("json", schema.clone())
                        .with_description("Respond with a JSON object."),
                ));
            } else {
                merge_object_field(
                    &mut provider_options,
                    "additionalModelRequestFields",
                    "output_config",
                    json!({
                        "format": {
                            "type": "json_schema",
                            "schema": schema
                        }
                    }),
                );
            }
        }

        let prepared_tools = prepare_tools_for_bedrock(
            &tools,
            options.tool_choice.as_ref(),
            &self.model_id,
            &mut warnings,
        )?;

        if let Some(additional_tools) = prepared_tools.additional_tools {
            merge_json_object(
                provider_options
                    .entry("additionalModelRequestFields".to_string())
                    .or_insert_with(|| JsonValue::Object(JsonObject::new())),
                JsonValue::Object(additional_tools),
            );
        }

        if !prepared_tools.betas.is_empty() {
            let mut betas = provider_options
                .get("anthropicBeta")
                .and_then(JsonValue::as_array)
                .cloned()
                .unwrap_or_default();
            for beta in prepared_tools.betas {
                let beta_value = JsonValue::String(beta);
                if !betas.contains(&beta_value) {
                    betas.push(beta_value);
                }
            }
            provider_options
                .entry("additionalModelRequestFields".to_string())
                .or_insert_with(|| JsonValue::Object(JsonObject::new()))
                .as_object_mut()
                .expect("additionalModelRequestFields remains an object")
                .insert("anthropic_beta".to_string(), JsonValue::Array(betas));
        }

        let mut filtered_prompt = options.prompt.clone();
        if json_object_is_empty(&prepared_tools.tool_config) {
            let had_tool_content = prompt_has_tool_content(&filtered_prompt);
            if had_tool_content {
                filtered_prompt = filter_tool_content_from_prompt(&filtered_prompt);
                warnings.push(unsupported(
                    "toolContent",
                    Some(
                        "Tool calls and results removed from conversation because Bedrock does not support tool content without active tools."
                            .to_string(),
                    ),
                ));
            }
        }

        let is_mistral = is_mistral_model(&self.model_id);
        let converted = convert_to_amazon_bedrock_chat_messages(&filtered_prompt, is_mistral)?;

        let mut inference_config = JsonObject::new();
        insert_some(
            &mut inference_config,
            "maxTokens",
            options.max_output_tokens,
        );
        insert_some(&mut inference_config, "temperature", temperature);
        insert_some(&mut inference_config, "topP", options.top_p);
        insert_some(&mut inference_config, "topK", options.top_k);
        if let Some(stop_sequences) = &options.stop_sequences {
            inference_config.insert("stopSequences".to_string(), json!(stop_sequences));
        }

        if is_anthropic
            && provider_options
                .get("reasoningConfig")
                .and_then(|value| value.get("type"))
                .and_then(JsonValue::as_str)
                .is_some_and(|kind| kind == "enabled" || kind == "adaptive")
        {
            if inference_config.remove("temperature").is_some() {
                warnings.push(unsupported(
                    "temperature",
                    Some("temperature is not supported when thinking is enabled".to_string()),
                ));
            }
            if inference_config.remove("topP").is_some() {
                warnings.push(unsupported(
                    "topP",
                    Some("topP is not supported when thinking is enabled".to_string()),
                ));
            }
            if inference_config.remove("topK").is_some() {
                warnings.push(unsupported(
                    "topK",
                    Some("topK is not supported when thinking is enabled".to_string()),
                ));
            }
        }

        let mut command = JsonObject::new();
        command.insert("system".to_string(), JsonValue::Array(converted.system));
        command.insert("messages".to_string(), JsonValue::Array(converted.messages));
        if !inference_config.is_empty() {
            command.insert(
                "inferenceConfig".to_string(),
                JsonValue::Object(inference_config),
            );
        }
        if let Some(additional) = provider_options.remove("additionalModelRequestFields") {
            command.insert("additionalModelRequestFields".to_string(), additional);
        }
        if is_anthropic {
            command.insert(
                "additionalModelResponseFieldPaths".to_string(),
                json!(["/delta/stop_sequence"]),
            );
        }
        if let Some(service_tier) = provider_options.remove("serviceTier") {
            command.insert("serviceTier".to_string(), json!({ "type": service_tier }));
        }
        provider_options.remove("reasoningConfig");
        provider_options.remove("anthropicBeta");
        for (key, value) in provider_options {
            command.insert(key, value);
        }
        if !json_object_is_empty(&prepared_tools.tool_config) {
            command.insert("toolConfig".to_string(), prepared_tools.tool_config);
        }

        Ok((
            JsonValue::Object(command),
            warnings,
            uses_json_response_tool,
        ))
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let (body, warnings, uses_json_response_tool) = match self.request_body(&options) {
            Ok(result) => result,
            Err(error) => {
                return bedrock_error_generate_result(
                    &self.model_id,
                    error.to_string(),
                    options.prompt,
                    json!({ "modelId": self.model_id }),
                );
            }
        };

        let request_body_for_response = body.clone();
        let request_body_for_error = body.clone();
        let post_options =
            PostJsonToApiOptions::new(format!("{}/converse", self.model_url()), body)
                .with_headers(self.config.request_headers(options.headers.as_ref()))
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(options.abort_signal.clone());

        let result = post_json_to_api(
            post_options,
            self.config.authenticated_transport(),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    |value| Ok::<JsonValue, String>(value.clone()),
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    |value| Ok::<JsonValue, String>(value.clone()),
                    bedrock_error_to_message,
                    |_, _| None,
                ))
            },
        )
        .await;

        match result {
            Ok(response) => bedrock_generate_result_from_response(
                &self.model_id,
                response.value,
                response.response_headers,
                response.raw_value,
                request_body_for_response,
                options.prompt,
                warnings,
                uses_json_response_tool,
                Arc::clone(&self.config.generate_id),
            ),
            Err(error) => bedrock_error_generate_result(
                &self.model_id,
                format!("{error:?}"),
                options.prompt,
                request_body_for_error,
            ),
        }
    }

    async fn do_stream_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let include_raw_chunks = options.include_raw_chunks.unwrap_or(false);
        let (body, warnings, uses_json_response_tool) = match self.request_body(&options) {
            Ok(result) => result,
            Err(error) => return bedrock_error_stream_result(error.to_string(), json!({})),
        };

        let request_body_for_response = body.clone();
        let request_body_for_error = body.clone();
        let post_options =
            PostJsonToApiOptions::new(format!("{}/converse-stream", self.model_url()), body)
                .with_headers(self.config.request_headers(options.headers.as_ref()))
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(options.abort_signal.clone());

        let result = post_json_to_api(
            post_options,
            self.config.authenticated_transport(),
            |_request, response| decode_bedrock_event_stream_response(response),
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    |value| Ok::<JsonValue, String>(value.clone()),
                    bedrock_error_to_message,
                    |_, _| None,
                ))
            },
        )
        .await;

        match result {
            Ok(response) => bedrock_stream_result_from_chunks(
                &self.model_id,
                response.value,
                response.response_headers,
                request_body_for_response,
                warnings,
                include_raw_chunks,
                uses_json_response_tool,
            ),
            Err(error) => bedrock_error_stream_result(format!("{error:?}"), request_body_for_error),
        }
    }

    fn model_url(&self) -> String {
        format!(
            "{}/model/{}",
            self.config.base_url,
            encode_uri_component(&self.model_id)
        )
    }
}

impl LanguageModel for AmazonBedrockChatLanguageModel {
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
        ready(BTreeMap::new())
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(self.do_stream_result(options))
    }
}

/// Bedrock embedding model.
#[derive(Clone)]
pub struct AmazonBedrockEmbeddingModel {
    model_id: String,
    config: AmazonBedrockModelConfig,
}

impl AmazonBedrockEmbeddingModel {
    fn new(model_id: String, config: AmazonBedrockModelConfig) -> Self {
        Self { model_id, config }
    }

    /// Prepares the Bedrock embedding request body.
    pub fn request_body(
        &self,
        values: &[String],
        provider_options: Option<&ProviderOptions>,
    ) -> JsonValue {
        let options = bedrock_provider_options(provider_options);
        let is_nova = self.model_id.starts_with("amazon.nova-") && self.model_id.contains("embed");
        let is_cohere = self.model_id.starts_with("cohere.embed-");

        if is_nova {
            json!({
                "taskType": "SINGLE_EMBEDDING",
                "singleEmbeddingParams": {
                    "embeddingPurpose": string_option(&options, "embeddingPurpose").unwrap_or_else(|| "GENERIC_INDEX".to_string()),
                    "embeddingDimension": u64_option(&options, "embeddingDimension").unwrap_or(1024),
                    "text": {
                        "truncationMode": string_option(&options, "truncate").unwrap_or_else(|| "END".to_string()),
                        "value": values.first().cloned().unwrap_or_default()
                    }
                }
            })
        } else if is_cohere {
            let mut body = JsonObject::new();
            body.insert(
                "input_type".to_string(),
                json!(
                    string_option(&options, "inputType")
                        .unwrap_or_else(|| "search_query".to_string())
                ),
            );
            body.insert(
                "texts".to_string(),
                json!([values.first().cloned().unwrap_or_default()]),
            );
            insert_json_if_present(&mut body, "truncate", options.get("truncate").cloned());
            insert_json_if_present(
                &mut body,
                "output_dimension",
                options.get("outputDimension").cloned(),
            );
            JsonValue::Object(body)
        } else {
            let mut body = JsonObject::new();
            body.insert(
                "inputText".to_string(),
                json!(values.first().cloned().unwrap_or_default()),
            );
            insert_json_if_present(&mut body, "dimensions", options.get("dimensions").cloned());
            insert_json_if_present(&mut body, "normalize", options.get("normalize").cloned());
            JsonValue::Object(body)
        }
    }

    fn model_url(&self) -> String {
        format!(
            "{}/model/{}/invoke",
            self.config.base_url,
            encode_uri_component(&self.model_id)
        )
    }
}

impl EmbeddingModel for AmazonBedrockEmbeddingModel {
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
        ready(Some(1))
    }

    fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
        ready(true)
    }

    fn do_embed(&self, options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
        Box::pin(async move {
            if options.values.len() > 1 {
                return EmbeddingModelResult::new(Vec::new()).with_warning(unsupported(
                    "values",
                    Some("Amazon Bedrock embedding models accept one value per call.".to_string()),
                ));
            }

            let body = self.request_body(&options.values, options.provider_options.as_ref());
            let post_options = PostJsonToApiOptions::new(self.model_url(), body)
                .with_headers(self.config.request_headers(options.headers.as_ref()))
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(options.abort_signal.clone());

            let result = post_json_to_api(
                post_options,
                self.config.authenticated_transport(),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        |value| Ok::<JsonValue, String>(value.clone()),
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        |value| Ok::<JsonValue, String>(value.clone()),
                        bedrock_error_to_message,
                        |_, _| None,
                    ))
                },
            )
            .await;

            match result {
                Ok(response) => bedrock_embedding_result_from_response(
                    response.value,
                    response.response_headers,
                    response.raw_value,
                ),
                Err(error) => EmbeddingModelResult::new(Vec::new()).with_warning(Warning::Other {
                    message: format!("Amazon Bedrock embedding request failed: {error:?}"),
                }),
            }
        })
    }
}

/// Bedrock Nova image model.
#[derive(Clone)]
pub struct AmazonBedrockImageModel {
    model_id: String,
    config: AmazonBedrockModelConfig,
}

impl AmazonBedrockImageModel {
    fn new(model_id: String, config: AmazonBedrockModelConfig) -> Self {
        Self { model_id, config }
    }

    /// Prepares the Bedrock image request body and warnings.
    pub fn request_body(
        &self,
        options: &ImageModelCallOptions,
    ) -> Result<(JsonValue, Vec<Warning>), UnsupportedFunctionalityError> {
        let mut warnings = Vec::new();
        let provider_options = bedrock_provider_options(Some(&options.provider_options));
        let mut image_generation_config = JsonObject::new();

        if let Some(size) = &options.size {
            if let Some((width, height)) = size.split_once('x') {
                if let Ok(width) = width.parse::<u64>() {
                    image_generation_config.insert("width".to_string(), json!(width));
                }
                if let Ok(height) = height.parse::<u64>() {
                    image_generation_config.insert("height".to_string(), json!(height));
                }
            }
        }
        if let Some(seed) = options.seed {
            image_generation_config.insert("seed".to_string(), json!(seed));
        }
        if options.n > 0 {
            image_generation_config.insert("numberOfImages".to_string(), json!(options.n));
        }
        insert_json_if_present(
            &mut image_generation_config,
            "quality",
            provider_options.get("quality").cloned(),
        );
        insert_json_if_present(
            &mut image_generation_config,
            "cfgScale",
            provider_options.get("cfgScale").cloned(),
        );

        if options.aspect_ratio.is_some() {
            warnings.push(unsupported(
                "aspectRatio",
                Some("This model does not support aspect ratio. Use `size` instead.".to_string()),
            ));
        }

        let has_files = options
            .files
            .as_ref()
            .is_some_and(|files| !files.is_empty());
        let body = if has_files {
            let files = options.files.as_ref().expect("files checked present");
            let has_mask = options.mask.is_some();
            let has_mask_prompt = provider_options.get("maskPrompt").is_some();
            let task_type = string_option(&provider_options, "taskType").unwrap_or_else(|| {
                if has_mask || has_mask_prompt {
                    "INPAINTING".to_string()
                } else {
                    "IMAGE_VARIATION".to_string()
                }
            });
            match task_type.as_str() {
                "INPAINTING" => {
                    let mut params = JsonObject::new();
                    params.insert("image".to_string(), json!(image_file_base64(&files[0])?));
                    insert_json_if_present(
                        &mut params,
                        "text",
                        options.prompt.clone().map(JsonValue::String),
                    );
                    insert_json_if_present(
                        &mut params,
                        "negativeText",
                        provider_options.get("negativeText").cloned(),
                    );
                    if let Some(mask) = &options.mask {
                        params.insert("maskImage".to_string(), json!(image_file_base64(mask)?));
                    } else {
                        insert_json_if_present(
                            &mut params,
                            "maskPrompt",
                            provider_options.get("maskPrompt").cloned(),
                        );
                    }
                    json!({
                        "taskType": "INPAINTING",
                        "inPaintingParams": params,
                        "imageGenerationConfig": image_generation_config
                    })
                }
                "OUTPAINTING" => {
                    let mut params = JsonObject::new();
                    params.insert("image".to_string(), json!(image_file_base64(&files[0])?));
                    insert_json_if_present(
                        &mut params,
                        "text",
                        options.prompt.clone().map(JsonValue::String),
                    );
                    insert_json_if_present(
                        &mut params,
                        "negativeText",
                        provider_options.get("negativeText").cloned(),
                    );
                    insert_json_if_present(
                        &mut params,
                        "outPaintingMode",
                        provider_options.get("outPaintingMode").cloned(),
                    );
                    if let Some(mask) = &options.mask {
                        params.insert("maskImage".to_string(), json!(image_file_base64(mask)?));
                    } else {
                        insert_json_if_present(
                            &mut params,
                            "maskPrompt",
                            provider_options.get("maskPrompt").cloned(),
                        );
                    }
                    json!({
                        "taskType": "OUTPAINTING",
                        "outPaintingParams": params,
                        "imageGenerationConfig": image_generation_config
                    })
                }
                "BACKGROUND_REMOVAL" => json!({
                    "taskType": "BACKGROUND_REMOVAL",
                    "backgroundRemovalParams": { "image": image_file_base64(&files[0])? }
                }),
                "IMAGE_VARIATION" => {
                    let mut params = JsonObject::new();
                    params.insert(
                        "images".to_string(),
                        JsonValue::Array(
                            files
                                .iter()
                                .map(image_file_base64)
                                .collect::<Result<Vec<_>, _>>()?
                                .into_iter()
                                .map(JsonValue::String)
                                .collect(),
                        ),
                    );
                    insert_json_if_present(
                        &mut params,
                        "text",
                        options.prompt.clone().map(JsonValue::String),
                    );
                    insert_json_if_present(
                        &mut params,
                        "negativeText",
                        provider_options.get("negativeText").cloned(),
                    );
                    insert_json_if_present(
                        &mut params,
                        "similarityStrength",
                        provider_options.get("similarityStrength").cloned(),
                    );
                    json!({
                        "taskType": "IMAGE_VARIATION",
                        "imageVariationParams": params,
                        "imageGenerationConfig": image_generation_config
                    })
                }
                other => {
                    return Err(UnsupportedFunctionalityError::new(format!(
                        "image task type: {other}"
                    )));
                }
            }
        } else {
            let mut params = JsonObject::new();
            insert_json_if_present(
                &mut params,
                "text",
                options.prompt.clone().map(JsonValue::String),
            );
            insert_json_if_present(
                &mut params,
                "negativeText",
                provider_options.get("negativeText").cloned(),
            );
            insert_json_if_present(&mut params, "style", provider_options.get("style").cloned());
            json!({
                "taskType": "TEXT_IMAGE",
                "textToImageParams": params,
                "imageGenerationConfig": image_generation_config
            })
        };

        Ok((body, warnings))
    }

    fn model_url(&self) -> String {
        format!(
            "{}/model/{}/invoke",
            self.config.base_url,
            encode_uri_component(&self.model_id)
        )
    }
}

impl ImageModel for AmazonBedrockImageModel {
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
        ready(Some(if self.model_id == "amazon.nova-canvas-v1:0" {
            5
        } else {
            1
        }))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(async move {
            let current_date = (self.config.current_date)();
            let (body, warnings) = match self.request_body(&options) {
                Ok(result) => result,
                Err(error) => {
                    return ImageModelResult::new(
                        Vec::new(),
                        ImageModelResponse::new(current_date, self.model_id.clone()),
                    )
                    .with_warning(Warning::Other {
                        message: error.to_string(),
                    });
                }
            };

            let post_options = PostJsonToApiOptions::new(self.model_url(), body)
                .with_headers(self.config.request_headers(options.headers.as_ref()))
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(options.abort_signal.clone());

            let result = post_json_to_api(
                post_options,
                self.config.authenticated_transport(),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        |value| Ok::<JsonValue, String>(value.clone()),
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        |value| Ok::<JsonValue, String>(value.clone()),
                        bedrock_error_to_message,
                        |_, _| None,
                    ))
                },
            )
            .await;

            match result {
                Ok(response) => {
                    let images = response
                        .value
                        .get("images")
                        .and_then(JsonValue::as_array)
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|value| {
                            value
                                .as_str()
                                .map(|value| FileDataContent::Base64(value.to_string()))
                        })
                        .collect::<Vec<_>>();
                    let mut result = ImageModelResult::new(
                        images,
                        ImageModelResponse {
                            timestamp: current_date,
                            model_id: self.model_id.clone(),
                            headers: response.response_headers,
                        },
                    );
                    for warning in warnings {
                        result = result.with_warning(warning);
                    }
                    if result.images.is_empty() {
                        result = result.with_warning(Warning::Other {
                            message: format!(
                                "Amazon Bedrock returned no images. {}",
                                response
                                    .value
                                    .get("status")
                                    .and_then(JsonValue::as_str)
                                    .map(|status| format!("Status: {status}"))
                                    .unwrap_or_default()
                            )
                            .trim()
                            .to_string(),
                        });
                    }
                    result
                }
                Err(error) => ImageModelResult::new(
                    Vec::new(),
                    ImageModelResponse::new(current_date, self.model_id.clone()),
                )
                .with_warning(Warning::Other {
                    message: format!("Amazon Bedrock image request failed: {error:?}"),
                }),
            }
        })
    }
}

/// Bedrock Agent Runtime reranking model.
#[derive(Clone)]
pub struct AmazonBedrockRerankingModel {
    model_id: String,
    region: String,
    config: AmazonBedrockModelConfig,
}

impl AmazonBedrockRerankingModel {
    fn new(model_id: String, region: String, config: AmazonBedrockModelConfig) -> Self {
        Self {
            model_id,
            region,
            config,
        }
    }

    /// Prepares the Bedrock reranking request body.
    pub fn request_body(&self, options: &RerankingModelCallOptions) -> JsonValue {
        let provider_options = bedrock_provider_options(options.provider_options.as_ref());
        let sources = match &options.documents {
            RerankingModelDocuments::Text { values } => values
                .iter()
                .map(|value| {
                    json!({
                        "type": "INLINE",
                        "inlineDocumentSource": {
                            "type": "TEXT",
                            "textDocument": { "text": value }
                        }
                    })
                })
                .collect::<Vec<_>>(),
            RerankingModelDocuments::Object { values } => values
                .iter()
                .map(|value| {
                    json!({
                        "type": "INLINE",
                        "inlineDocumentSource": {
                            "type": "JSON",
                            "jsonDocument": value
                        }
                    })
                })
                .collect::<Vec<_>>(),
        };

        let mut model_configuration = JsonObject::new();
        model_configuration.insert(
            "modelArn".to_string(),
            json!(format!(
                "arn:aws:bedrock:{}::foundation-model/{}",
                self.region, self.model_id
            )),
        );
        insert_json_if_present(
            &mut model_configuration,
            "additionalModelRequestFields",
            provider_options
                .get("additionalModelRequestFields")
                .cloned(),
        );

        let mut rerank = JsonObject::new();
        rerank.insert(
            "modelConfiguration".to_string(),
            JsonValue::Object(model_configuration),
        );
        insert_json_if_present(
            &mut rerank,
            "numberOfResults",
            options.top_n.map(|value| json!(value)),
        );

        let mut body = JsonObject::new();
        insert_json_if_present(
            &mut body,
            "nextToken",
            provider_options.get("nextToken").cloned(),
        );
        body.insert(
            "queries".to_string(),
            json!([{ "textQuery": { "text": options.query }, "type": "TEXT" }]),
        );
        body.insert(
            "rerankingConfiguration".to_string(),
            json!({
                "amazonBedrockRerankingConfiguration": rerank,
                "type": "BEDROCK_RERANKING_MODEL"
            }),
        );
        body.insert("sources".to_string(), JsonValue::Array(sources));
        JsonValue::Object(body)
    }
}

impl RerankingModel for AmazonBedrockRerankingModel {
    type RerankFuture<'a>
        = Pin<Box<dyn Future<Output = RerankingModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.config.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn do_rerank(&self, options: RerankingModelCallOptions) -> Self::RerankFuture<'_> {
        Box::pin(async move {
            let body = self.request_body(&options);
            let post_options =
                PostJsonToApiOptions::new(format!("{}/rerank", self.config.base_url), body)
                    .with_headers(self.config.request_headers(options.headers.as_ref()))
                    .with_environment(RuntimeEnvironment::unknown())
                    .with_optional_abort_signal(options.abort_signal.clone());

            let result = post_json_to_api(
                post_options,
                self.config.authenticated_transport(),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        |value| Ok::<JsonValue, String>(value.clone()),
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        |value| Ok::<JsonValue, String>(value.clone()),
                        bedrock_error_to_message,
                        |_, _| None,
                    ))
                },
            )
            .await;

            match result {
                Ok(response) => bedrock_reranking_result_from_response(
                    response.value,
                    response.response_headers,
                    response.raw_value,
                ),
                Err(error) => RerankingModelResult::new(Vec::new()).with_warning(Warning::Other {
                    message: format!("Amazon Bedrock reranking request failed: {error:?}"),
                }),
            }
        })
    }
}

/// Provider for Anthropic Messages models through Bedrock native invoke APIs.
#[derive(Clone)]
pub struct AmazonBedrockAnthropicProvider {
    bedrock: AmazonBedrockProvider,
}

impl AmazonBedrockAnthropicProvider {
    /// Creates a Bedrock Anthropic provider.
    pub fn new(settings: AmazonBedrockProviderSettings) -> Self {
        Self {
            bedrock: AmazonBedrockProvider::from_settings(settings),
        }
    }

    /// Replaces the HTTP transport.
    pub fn with_transport(mut self, transport: AmazonBedrockTransport) -> Self {
        self.bedrock = self.bedrock.with_transport(transport);
        self
    }

    /// Creates a Bedrock Anthropic messages model.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> AmazonBedrockAnthropicLanguageModel {
        AmazonBedrockAnthropicLanguageModel {
            inner: self.bedrock.chat(model_id),
        }
    }

    /// Upstream aliases `.chat()` and `.messages()` to `.languageModel()`.
    pub fn chat(&self, model_id: impl Into<String>) -> AmazonBedrockAnthropicLanguageModel {
        self.language_model(model_id)
    }

    /// Upstream aliases `.messages()` to `.languageModel()`.
    pub fn messages(&self, model_id: impl Into<String>) -> AmazonBedrockAnthropicLanguageModel {
        self.language_model(model_id)
    }

    /// Embeddings are not supported by the Anthropic-on-Bedrock provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<AmazonBedrockEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Images are not supported by the Anthropic-on-Bedrock provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<AmazonBedrockImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }
}

/// Creates an Anthropic-on-Bedrock provider.
pub fn create_amazon_bedrock_anthropic(
    settings: AmazonBedrockProviderSettings,
) -> AmazonBedrockAnthropicProvider {
    AmazonBedrockAnthropicProvider::new(settings)
}

/// Anthropic-on-Bedrock language model.
#[derive(Clone)]
pub struct AmazonBedrockAnthropicLanguageModel {
    inner: AmazonBedrockChatLanguageModel,
}

impl AmazonBedrockAnthropicLanguageModel {
    /// Transforms an Anthropic Messages request body into Bedrock invoke shape.
    pub fn transform_request_body(mut body: JsonObject, betas: &[String]) -> JsonValue {
        body.remove("model");
        body.remove("stream");
        if let Some(tool_choice) = body
            .get_mut("tool_choice")
            .and_then(JsonValue::as_object_mut)
        {
            if !tool_choice.contains_key("name") {
                tool_choice.remove("name");
            }
        }

        let mut required_betas = betas.to_vec();
        if let Some(tools) = body.get_mut("tools").and_then(JsonValue::as_array_mut) {
            for tool in tools {
                if let Some(tool_object) = tool.as_object_mut() {
                    if let Some(tool_type) = tool_object
                        .get("type")
                        .and_then(JsonValue::as_str)
                        .map(str::to_string)
                    {
                        if let Some((new_type, beta, new_name)) =
                            bedrock_anthropic_tool_upgrade(&tool_type)
                        {
                            tool_object.insert(
                                "type".to_string(),
                                JsonValue::String(new_type.to_string()),
                            );
                            if let Some(beta) = beta {
                                push_unique_string(&mut required_betas, beta);
                            }
                            if let Some(new_name) = new_name {
                                tool_object.insert(
                                    "name".to_string(),
                                    JsonValue::String(new_name.to_string()),
                                );
                            }
                        }
                    }
                }
            }
        }
        if !required_betas.is_empty() {
            body.insert("anthropic_beta".to_string(), json!(required_betas));
        }
        body.insert("anthropic_version".to_string(), json!("bedrock-2023-05-31"));
        JsonValue::Object(body)
    }
}

impl LanguageModel for AmazonBedrockAnthropicLanguageModel {
    type SupportedUrlsFuture<'a>
        = Ready<LanguageModelSupportedUrls>
    where
        Self: 'a;
    type GenerateFuture<'a>
        = <AmazonBedrockChatLanguageModel as LanguageModel>::GenerateFuture<'a>
    where
        Self: 'a;
    type Stream = <AmazonBedrockChatLanguageModel as LanguageModel>::Stream;
    type StreamFuture<'a>
        = <AmazonBedrockChatLanguageModel as LanguageModel>::StreamFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        "bedrock.anthropic.messages"
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(BTreeMap::new())
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        self.inner.do_generate(options)
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        self.inner.do_stream(options)
    }
}

/// Bedrock Mantle provider for OpenAI-compatible chat/Responses endpoints.
#[derive(Clone)]
pub struct BedrockMantleProvider {
    provider: OpenAICompatibleProvider,
}

impl BedrockMantleProvider {
    /// Creates a Mantle provider from Bedrock settings.
    pub fn new(settings: AmazonBedrockProviderSettings) -> Self {
        let region = first_non_empty([
            settings.region.clone(),
            env::var("AWS_REGION").ok(),
            Some(DEFAULT_AMAZON_BEDROCK_REGION.to_string()),
        ])
        .expect("default Bedrock region is present");
        let base_url = settings
            .base_url
            .unwrap_or_else(|| format!("https://bedrock-mantle.{region}.api.aws/v1"));
        let mut compatible = OpenAICompatibleProviderSettings::new(
            "bedrock-mantle",
            without_trailing_slash(Some(&base_url)).unwrap_or(&base_url),
        );
        for (name, value) in settings.headers {
            compatible = compatible.with_header(name, value);
        }
        if let Some(api_key) =
            first_non_empty([settings.api_key, env::var("AWS_BEARER_TOKEN_BEDROCK").ok()])
        {
            compatible = compatible.with_api_key(api_key);
        }
        Self {
            provider: OpenAICompatibleProvider::from_settings(compatible),
        }
    }

    /// Creates a Chat Completions model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.provider.chat_model(model_id)
    }

    /// Creates a Chat Completions model.
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.language_model(model_id)
    }

    /// Rust maps Mantle Responses to the OpenAI-compatible chat model until
    /// the standalone Responses package exposes a provider-owned constructor.
    pub fn responses(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.language_model(model_id)
    }

    /// Mantle does not expose embedding models.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Mantle does not expose image models.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }
}

/// Creates a Bedrock Mantle provider.
pub fn create_bedrock_mantle(settings: AmazonBedrockProviderSettings) -> BedrockMantleProvider {
    BedrockMantleProvider::new(settings)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ConvertedBedrockMessages {
    system: Vec<JsonValue>,
    messages: Vec<JsonValue>,
}

#[derive(Clone, Debug)]
struct PreparedTools {
    tool_config: JsonValue,
    additional_tools: Option<JsonObject>,
    betas: Vec<String>,
}

fn prepare_tools_for_bedrock(
    tools: &[LanguageModelTool],
    tool_choice: Option<&LanguageModelToolChoice>,
    model_id: &str,
    warnings: &mut Vec<Warning>,
) -> Result<PreparedTools, UnsupportedFunctionalityError> {
    if tools.is_empty() {
        return Ok(PreparedTools {
            tool_config: JsonValue::Object(JsonObject::new()),
            additional_tools: None,
            betas: Vec::new(),
        });
    }

    let is_anthropic = is_anthropic_model(model_id);
    let mut function_tools = Vec::new();
    let mut provider_tools = Vec::new();

    for tool in tools {
        match tool {
            LanguageModelTool::Function(tool) => function_tools.push(tool.clone()),
            LanguageModelTool::Provider(tool) => {
                if tool.id == "anthropic.web_search_20250305" {
                    warnings.push(unsupported(
                        "web_search_20250305 tool",
                        Some(
                            "The web_search_20250305 tool is not supported on Amazon Bedrock."
                                .to_string(),
                        ),
                    ));
                } else {
                    provider_tools.push(tool.clone());
                }
            }
        }
    }

    let mut betas = Vec::new();
    let mut additional_tools = None;
    let mut bedrock_tools = Vec::new();

    if is_anthropic {
        for tool in provider_tools {
            let tool_type = tool
                .args
                .get("type")
                .and_then(JsonValue::as_str)
                .unwrap_or(tool.id.as_str());
            if let Some((bedrock_type, beta, bedrock_name)) =
                bedrock_anthropic_tool_upgrade(tool_type)
            {
                if let Some(beta) = beta {
                    push_unique_string(&mut betas, beta);
                }
                bedrock_tools.push(json!({
                    "toolSpec": {
                        "name": bedrock_name.unwrap_or(tool.name.as_str()),
                        "inputSchema": { "json": { "type": "object" } },
                        "description": format!("Anthropic provider tool {bedrock_type}")
                    }
                }));
            } else {
                warnings.push(unsupported("tool", Some(tool.id)));
            }
        }
        if tool_choice.is_some() {
            additional_tools = Some(JsonObject::from_iter([(
                "tool_choice".to_string(),
                bedrock_tool_choice_json(tool_choice)?,
            )]));
        }
    } else {
        for tool in provider_tools {
            warnings.push(unsupported("tool", Some(tool.id)));
        }
    }

    let filtered_function_tools = function_tools
        .into_iter()
        .filter(|tool| match tool_choice {
            Some(LanguageModelToolChoice::Tool { tool_name }) => &tool.name == tool_name,
            _ => true,
        })
        .collect::<Vec<_>>();

    for tool in filtered_function_tools {
        let mut tool_spec = JsonObject::new();
        tool_spec.insert("name".to_string(), json!(tool.name));
        if let Some(description) = tool.description.filter(|value| !value.trim().is_empty()) {
            tool_spec.insert("description".to_string(), json!(description));
        }
        if let Some(strict) = tool.strict {
            tool_spec.insert("strict".to_string(), json!(strict));
        }
        tool_spec.insert(
            "inputSchema".to_string(),
            json!({ "json": tool.input_schema }),
        );
        bedrock_tools.push(json!({ "toolSpec": tool_spec }));
    }

    let mut tool_config = JsonObject::new();
    if !bedrock_tools.is_empty() {
        if matches!(tool_choice, Some(LanguageModelToolChoice::None)) {
            bedrock_tools.clear();
        } else {
            tool_config.insert("tools".to_string(), JsonValue::Array(bedrock_tools));
            if !is_anthropic {
                if let Some(choice) = tool_choice {
                    let choice = bedrock_tool_choice_json(Some(choice))?;
                    if !choice.is_null() {
                        tool_config.insert("toolChoice".to_string(), choice);
                    }
                }
            }
        }
    }

    Ok(PreparedTools {
        tool_config: JsonValue::Object(tool_config),
        additional_tools,
        betas,
    })
}

fn bedrock_tool_choice_json(
    tool_choice: Option<&LanguageModelToolChoice>,
) -> Result<JsonValue, UnsupportedFunctionalityError> {
    Ok(match tool_choice {
        Some(LanguageModelToolChoice::Auto) => json!({ "auto": {} }),
        Some(LanguageModelToolChoice::Required) => json!({ "any": {} }),
        Some(LanguageModelToolChoice::None) | None => JsonValue::Null,
        Some(LanguageModelToolChoice::Tool { tool_name }) => {
            json!({ "tool": { "name": tool_name } })
        }
    })
}

fn convert_to_amazon_bedrock_chat_messages(
    prompt: &[LanguageModelMessage],
    is_mistral: bool,
) -> Result<ConvertedBedrockMessages, UnsupportedFunctionalityError> {
    let blocks = group_into_blocks(prompt);
    let mut system = Vec::new();
    let mut messages = Vec::new();
    let mut document_counter = 0_u64;

    for (block_index, block) in blocks.iter().enumerate() {
        match block {
            MessageBlock::System(system_messages) => {
                if !messages.is_empty() {
                    return Err(UnsupportedFunctionalityError::new(
                        "Multiple system messages that are separated by user/assistant messages",
                    ));
                }
                for message in system_messages {
                    system.push(json!({ "text": message.content }));
                    if let Some(cache_point) = cache_point(message.provider_options.as_ref()) {
                        system.push(cache_point);
                    }
                }
            }
            MessageBlock::User(user_messages) => {
                let mut content = Vec::new();
                for message in user_messages {
                    match message {
                        UserBlockMessage::User(message) => {
                            for part in &message.content {
                                match part {
                                    LanguageModelUserContentPart::Text(text) => {
                                        content.push(json!({ "text": text.text }));
                                        push_cache_point(
                                            &mut content,
                                            text.provider_options.as_ref(),
                                        );
                                    }
                                    LanguageModelUserContentPart::File(file) => {
                                        content.push(file_part_to_bedrock_block(
                                            file,
                                            &mut document_counter,
                                        )?);
                                        push_cache_point(
                                            &mut content,
                                            file.provider_options.as_ref(),
                                        );
                                    }
                                }
                            }
                        }
                        UserBlockMessage::Tool(message) => {
                            for part in &message.content {
                                let LanguageModelToolContentPart::ToolResult(part) = part else {
                                    continue;
                                };
                                let tool_content = tool_result_output_to_bedrock(&part.output)?;
                                content.push(json!({
                                    "toolResult": {
                                        "toolUseId": normalize_tool_call_id(&part.tool_call_id, is_mistral),
                                        "content": tool_content
                                    }
                                }));
                                push_cache_point(&mut content, part.provider_options.as_ref());
                            }
                        }
                    }
                }
                messages.push(json!({ "role": "user", "content": content }));
            }
            MessageBlock::Assistant(assistant_messages) => {
                let mut content = Vec::new();
                let is_last_block = block_index + 1 == blocks.len();
                for (message_index, message) in assistant_messages.iter().enumerate() {
                    let is_last_message = message_index + 1 == assistant_messages.len();
                    let has_reasoning = message.content.iter().any(|part| {
                        matches!(part, LanguageModelAssistantContentPart::Reasoning(_))
                    });
                    for (part_index, part) in message.content.iter().enumerate() {
                        let is_last_part = part_index + 1 == message.content.len();
                        match part {
                            LanguageModelAssistantContentPart::Text(text) => {
                                if text.text.trim().is_empty() && !has_reasoning {
                                    continue;
                                }
                                let value = if is_last_block && is_last_message && is_last_part {
                                    text.text.trim().to_string()
                                } else {
                                    text.text.clone()
                                };
                                content.push(json!({ "text": value }));
                                push_cache_point(&mut content, text.provider_options.as_ref());
                            }
                            LanguageModelAssistantContentPart::Reasoning(reasoning) => {
                                if let Some(metadata) =
                                    reasoning_metadata(reasoning.provider_options.as_ref())
                                {
                                    if let Some(signature) =
                                        metadata.get("signature").and_then(JsonValue::as_str)
                                    {
                                        content.push(json!({
                                            "reasoningContent": {
                                                "reasoningText": {
                                                    "text": reasoning.text,
                                                    "signature": signature
                                                }
                                            }
                                        }));
                                    } else if let Some(redacted) =
                                        metadata.get("redactedData").and_then(JsonValue::as_str)
                                    {
                                        content.push(json!({
                                            "reasoningContent": {
                                                "redactedReasoning": { "data": redacted }
                                            }
                                        }));
                                    }
                                }
                                push_cache_point(&mut content, reasoning.provider_options.as_ref());
                            }
                            LanguageModelAssistantContentPart::ToolCall(tool_call) => {
                                content.push(json!({
                                    "toolUse": {
                                        "toolUseId": normalize_tool_call_id(&tool_call.tool_call_id, is_mistral),
                                        "name": tool_call.tool_name,
                                        "input": tool_call.input
                                    }
                                }));
                                push_cache_point(&mut content, tool_call.provider_options.as_ref());
                            }
                            LanguageModelAssistantContentPart::File(file) => {
                                content
                                    .push(file_part_to_bedrock_block(file, &mut document_counter)?);
                                push_cache_point(&mut content, file.provider_options.as_ref());
                            }
                            LanguageModelAssistantContentPart::Custom(_)
                            | LanguageModelAssistantContentPart::ReasoningFile(_)
                            | LanguageModelAssistantContentPart::ToolResult(_)
                            | LanguageModelAssistantContentPart::ToolApprovalRequest(_) => {}
                        }
                    }
                    push_cache_point(&mut content, message.provider_options.as_ref());
                }
                messages.push(json!({ "role": "assistant", "content": content }));
            }
        }
    }

    Ok(ConvertedBedrockMessages { system, messages })
}

#[derive(Clone, Debug)]
enum MessageBlock<'a> {
    System(Vec<&'a LanguageModelSystemMessage>),
    User(Vec<UserBlockMessage<'a>>),
    Assistant(Vec<&'a LanguageModelAssistantMessage>),
}

#[derive(Clone, Debug)]
enum UserBlockMessage<'a> {
    User(&'a LanguageModelUserMessage),
    Tool(&'a ai_sdk_rust::LanguageModelToolMessage),
}

fn group_into_blocks(prompt: &[LanguageModelMessage]) -> Vec<MessageBlock<'_>> {
    let mut blocks = Vec::<MessageBlock<'_>>::new();

    for message in prompt {
        match message {
            LanguageModelMessage::System(message) => match blocks.last_mut() {
                Some(MessageBlock::System(messages)) => messages.push(message),
                _ => blocks.push(MessageBlock::System(vec![message])),
            },
            LanguageModelMessage::User(message) => match blocks.last_mut() {
                Some(MessageBlock::User(messages)) => {
                    messages.push(UserBlockMessage::User(message));
                }
                _ => blocks.push(MessageBlock::User(vec![UserBlockMessage::User(message)])),
            },
            LanguageModelMessage::Tool(message) => match blocks.last_mut() {
                Some(MessageBlock::User(messages)) => {
                    messages.push(UserBlockMessage::Tool(message));
                }
                _ => blocks.push(MessageBlock::User(vec![UserBlockMessage::Tool(message)])),
            },
            LanguageModelMessage::Assistant(message) => match blocks.last_mut() {
                Some(MessageBlock::Assistant(messages)) => messages.push(message),
                _ => blocks.push(MessageBlock::Assistant(vec![message])),
            },
        }
    }

    blocks
}

fn file_part_to_bedrock_block(
    file: &LanguageModelFilePart,
    document_counter: &mut u64,
) -> Result<JsonValue, UnsupportedFunctionalityError> {
    match &file.data {
        FileData::Reference { .. } => Err(UnsupportedFunctionalityError::new(
            "file parts with provider references",
        )),
        FileData::Url { .. } => Err(UnsupportedFunctionalityError::new("File URL data")),
        FileData::Text { text } => {
            let media_type = if is_full_media_type(&file.media_type) {
                file.media_type.as_str()
            } else {
                "text/plain"
            };
            Ok(document_block(
                media_type,
                file.filename.as_deref(),
                FileDataContent::Bytes(text.as_bytes().to_vec()),
                file.provider_options.as_ref(),
                document_counter,
            )?)
        }
        FileData::Data { data } => {
            let full_media_type = resolve_full_media_type(file).map_err(|error| {
                UnsupportedFunctionalityError::with_message("file media type", error.to_string())
            })?;
            if get_top_level_media_type(&full_media_type) == "image" {
                Ok(json!({
                    "image": {
                        "format": amazon_bedrock_image_format(&full_media_type)?,
                        "source": { "bytes": convert_to_base64(data) }
                    }
                }))
            } else {
                Ok(document_block(
                    &full_media_type,
                    file.filename.as_deref(),
                    data.clone(),
                    file.provider_options.as_ref(),
                    document_counter,
                )?)
            }
        }
    }
}

fn document_block(
    media_type: &str,
    filename: Option<&str>,
    data: FileDataContent,
    provider_options: Option<&ProviderOptions>,
    document_counter: &mut u64,
) -> Result<JsonValue, UnsupportedFunctionalityError> {
    *document_counter += 1;
    let name = filename
        .map(strip_file_extension)
        .map(str::to_string)
        .unwrap_or_else(|| format!("document-{document_counter}"));
    let mut document = JsonObject::new();
    document.insert(
        "format".to_string(),
        JsonValue::String(amazon_bedrock_document_format(media_type)?.to_string()),
    );
    document.insert("name".to_string(), JsonValue::String(name));
    document.insert(
        "source".to_string(),
        json!({ "bytes": convert_to_base64(&data) }),
    );
    if citations_enabled(provider_options) {
        document.insert("citations".to_string(), json!({ "enabled": true }));
    }
    Ok(json!({ "document": document }))
}

fn tool_result_output_to_bedrock(
    output: &LanguageModelToolResultOutput,
) -> Result<Vec<JsonValue>, UnsupportedFunctionalityError> {
    match output {
        LanguageModelToolResultOutput::Text { value, .. }
        | LanguageModelToolResultOutput::ErrorText { value, .. } => {
            Ok(vec![json!({ "text": value })])
        }
        LanguageModelToolResultOutput::ExecutionDenied { reason, .. } => Ok(vec![json!({
            "text": reason.clone().unwrap_or_else(|| "Tool call execution denied.".to_string())
        })]),
        LanguageModelToolResultOutput::Json { value, .. }
        | LanguageModelToolResultOutput::ErrorJson { value, .. } => {
            Ok(vec![json!({ "text": value.to_string() })])
        }
        LanguageModelToolResultOutput::Content { value } => value
            .iter()
            .map(|part| match part {
                LanguageModelToolResultContentPart::Text(text) => Ok(json!({ "text": text.text })),
                LanguageModelToolResultContentPart::File(file) => {
                    if get_top_level_media_type(&file.media_type) != "image" {
                        return Err(UnsupportedFunctionalityError::new(format!(
                            "media type: {}",
                            file.media_type
                        )));
                    }
                    let FileData::Data { data } = &file.data else {
                        return Err(UnsupportedFunctionalityError::new(format!(
                            "tool result file data of type \"{}\"",
                            file_data_kind(&file.data)
                        )));
                    };
                    let full_media_type = resolve_full_media_type(file).map_err(|error| {
                        UnsupportedFunctionalityError::with_message(
                            "tool result file media type",
                            error.to_string(),
                        )
                    })?;
                    Ok(json!({
                        "image": {
                            "format": amazon_bedrock_image_format(&full_media_type)?,
                            "source": { "bytes": convert_to_base64(data) }
                        }
                    }))
                }
                LanguageModelToolResultContentPart::Custom(_) => {
                    Err(UnsupportedFunctionalityError::new(
                        "unsupported tool content part type: custom",
                    ))
                }
            })
            .collect(),
    }
}

fn file_data_kind(data: &FileData) -> &'static str {
    match data {
        FileData::Data { .. } => "data",
        FileData::Url { .. } => "url",
        FileData::Reference { .. } => "reference",
        FileData::Text { .. } => "text",
    }
}

#[allow(clippy::too_many_arguments)]
fn bedrock_generate_result_from_response(
    model_id: &str,
    response: JsonValue,
    response_headers: Option<Headers>,
    raw_value: Option<JsonValue>,
    request_body: JsonValue,
    prompt: Vec<LanguageModelMessage>,
    warnings: Vec<Warning>,
    uses_json_response_tool: bool,
    generate_id: AmazonBedrockIdGenerator,
) -> LanguageModelGenerateResult {
    let mut content = Vec::new();
    let mut is_json_response_from_tool = false;
    let is_mistral = is_mistral_model(model_id);
    let parts = response
        .pointer("/output/message/content")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();

    for part in parts {
        if let Some(text) = part.get("text").and_then(JsonValue::as_str) {
            content.push(LanguageModelContent::Text(LanguageModelText::new(text)));
        }
        if let Some(reasoning) = part.get("reasoningContent") {
            if let Some(reasoning_text) = reasoning.get("reasoningText") {
                let mut part = LanguageModelReasoning::new(
                    reasoning_text
                        .get("text")
                        .and_then(JsonValue::as_str)
                        .unwrap_or_default(),
                );
                if let Some(signature) = reasoning_text.get("signature").and_then(JsonValue::as_str)
                {
                    part = part.with_provider_metadata(bedrock_metadata(json!({
                        "signature": signature
                    })));
                }
                content.push(LanguageModelContent::Reasoning(part));
            } else if let Some(redacted) = reasoning.get("redactedReasoning") {
                content.push(LanguageModelContent::Reasoning(
                    LanguageModelReasoning::new("").with_provider_metadata(bedrock_metadata(json!({
                        "redactedData": redacted.get("data").and_then(JsonValue::as_str).unwrap_or_default()
                    }))),
                ));
            }
        }
        if let Some(tool_use) = part.get("toolUse") {
            let is_json_tool = uses_json_response_tool
                && tool_use.get("name").and_then(JsonValue::as_str) == Some("json");
            if is_json_tool {
                is_json_response_from_tool = true;
                content.push(LanguageModelContent::Text(LanguageModelText::new(
                    tool_use
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| json!({}))
                        .to_string(),
                )));
            } else {
                let raw_tool_call_id = tool_use
                    .get("toolUseId")
                    .and_then(JsonValue::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| generate_id());
                content.push(LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                    normalize_tool_call_id(&raw_tool_call_id, is_mistral),
                    tool_use
                        .get("name")
                        .and_then(JsonValue::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("tool-{}", generate_id())),
                    tool_use
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| json!({}))
                        .to_string(),
                )));
            }
        }
    }

    let stop_sequence = response
        .pointer("/additionalModelResponseFields/delta/stop_sequence")
        .cloned()
        .unwrap_or(JsonValue::Null);
    let provider_metadata = bedrock_response_provider_metadata(
        &response,
        is_json_response_from_tool,
        Some(stop_sequence),
    );
    let finish_reason = LanguageModelFinishReason {
        unified: map_amazon_bedrock_finish_reason(
            response
                .get("stopReason")
                .and_then(JsonValue::as_str)
                .unwrap_or(""),
            is_json_response_from_tool,
        ),
        raw: response
            .get("stopReason")
            .and_then(JsonValue::as_str)
            .map(str::to_string),
    };
    let usage = convert_amazon_bedrock_usage(response.get("usage"));

    let mut result = LanguageModelGenerateResult::new(content, finish_reason, usage).with_request(
        LanguageModelRequest::new()
            .with_messages(prompt)
            .with_body(request_body),
    );
    let mut response_metadata = LanguageModelResponse::new().with_model_id(model_id.to_string());
    if let Some(id) = response_headers
        .as_ref()
        .and_then(|headers| header_value(headers, "x-amzn-requestid"))
    {
        response_metadata = response_metadata.with_id(id.to_string());
    }
    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response_metadata = response_metadata.with_header(name, value);
        }
    }
    if let Some(raw_value) = raw_value {
        response_metadata = response_metadata.with_body(raw_value);
    }
    result = result.with_response(response_metadata);
    if let Some(provider_metadata) = provider_metadata {
        result = result.with_provider_metadata(provider_metadata);
    }
    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn bedrock_stream_result_from_chunks(
    model_id: &str,
    chunks: Vec<DecodedBedrockEvent>,
    response_headers: Option<Headers>,
    request_body: JsonValue,
    warnings: Vec<Warning>,
    include_raw_chunks: bool,
    uses_json_response_tool: bool,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let mut parts = Vec::new();
    parts.push(LanguageModelStreamPart::StreamStart(
        LanguageModelStreamStart::new(warnings),
    ));
    let mut metadata =
        LanguageModelStreamResponseMetadata::new().with_model_id(model_id.to_string());
    if let Some(id) = response_headers
        .as_ref()
        .and_then(|headers| header_value(headers, "x-amzn-requestid"))
    {
        metadata = metadata.with_id(id.to_string());
    }
    parts.push(LanguageModelStreamPart::ResponseMetadata(metadata));

    let mut finish_reason = LanguageModelFinishReason {
        unified: FinishReason::Other,
        raw: None,
    };
    let mut usage = LanguageModelUsageDefault::default();
    let mut provider_metadata = None;
    let mut is_json_response_from_tool = false;
    let mut stop_sequence = JsonValue::Null;
    let mut content_blocks = BTreeMap::<u64, StreamContentBlock>::new();
    let is_mistral = is_mistral_model(model_id);

    for chunk in chunks {
        if include_raw_chunks {
            parts.push(LanguageModelStreamPart::Raw(
                LanguageModelRawStreamPart::new(chunk.raw_value.clone()),
            ));
        }

        if let Some(error) = chunk.error {
            finish_reason = LanguageModelFinishReason {
                unified: FinishReason::Error,
                raw: None,
            };
            parts.push(LanguageModelStreamPart::Error(
                LanguageModelErrorStreamPart::new(error),
            ));
            continue;
        }

        let value = chunk.value;
        for key in [
            "internalServerException",
            "modelStreamErrorException",
            "throttlingException",
            "validationException",
        ] {
            if let Some(error) = value.get(key).filter(|value| !value.is_null()) {
                finish_reason = LanguageModelFinishReason {
                    unified: FinishReason::Error,
                    raw: None,
                };
                parts.push(LanguageModelStreamPart::Error(
                    LanguageModelErrorStreamPart::new(error.clone()),
                ));
            }
        }

        if let Some(message_stop) = value.get("messageStop") {
            let raw = message_stop
                .get("stopReason")
                .and_then(JsonValue::as_str)
                .map(str::to_string);
            finish_reason = LanguageModelFinishReason {
                unified: map_amazon_bedrock_finish_reason(
                    raw.as_deref().unwrap_or(""),
                    is_json_response_from_tool,
                ),
                raw,
            };
            stop_sequence = message_stop
                .pointer("/additionalModelResponseFields/delta/stop_sequence")
                .cloned()
                .unwrap_or(JsonValue::Null);
        }

        if let Some(metadata) = value.get("metadata") {
            if let Some(raw_usage) = metadata.get("usage") {
                usage = LanguageModelUsageDefault(convert_amazon_bedrock_usage(Some(raw_usage)));
            }
            provider_metadata = bedrock_metadata_from_stream_metadata(metadata);
        }

        if let Some(block_start) = value.get("contentBlockStart") {
            let block_index = block_start
                .get("contentBlockIndex")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0);
            if let Some(tool_use) = block_start.pointer("/start/toolUse") {
                let is_json_tool = uses_json_response_tool
                    && tool_use.get("name").and_then(JsonValue::as_str) == Some("json");
                let id = tool_use
                    .get("toolUseId")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("tool-use")
                    .to_string();
                let normalized_id = normalize_tool_call_id(&id, is_mistral);
                let tool_name = tool_use
                    .get("name")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("tool")
                    .to_string();
                content_blocks.insert(
                    block_index,
                    StreamContentBlock::ToolCall {
                        tool_call_id: normalized_id.clone(),
                        tool_name: tool_name.clone(),
                        json_text: String::new(),
                        is_json_response_tool: is_json_tool,
                    },
                );
                if !is_json_tool {
                    parts.push(LanguageModelStreamPart::ToolInputStart(
                        LanguageModelToolInputStart::new(normalized_id, tool_name),
                    ));
                }
            } else {
                content_blocks.insert(block_index, StreamContentBlock::Text);
                parts.push(LanguageModelStreamPart::TextStart(
                    LanguageModelTextStart::new(block_index.to_string()),
                ));
            }
        }

        if let Some(delta) = value.get("contentBlockDelta") {
            let block_index = delta
                .get("contentBlockIndex")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0);
            if let Some(text) = delta.pointer("/delta/text").and_then(JsonValue::as_str) {
                content_blocks.entry(block_index).or_insert_with(|| {
                    parts.push(LanguageModelStreamPart::TextStart(
                        LanguageModelTextStart::new(block_index.to_string()),
                    ));
                    StreamContentBlock::Text
                });
                parts.push(LanguageModelStreamPart::TextDelta(
                    LanguageModelTextDelta::new(block_index.to_string(), text),
                ));
            }
            if let Some(input) = delta
                .pointer("/delta/toolUse/input")
                .and_then(JsonValue::as_str)
            {
                if let Some(StreamContentBlock::ToolCall {
                    tool_call_id,
                    json_text,
                    is_json_response_tool,
                    ..
                }) = content_blocks.get_mut(&block_index)
                {
                    if !*is_json_response_tool {
                        parts.push(LanguageModelStreamPart::ToolInputDelta(
                            LanguageModelToolInputDelta::new(tool_call_id.clone(), input),
                        ));
                    }
                    json_text.push_str(input);
                }
            }
            if let Some(reasoning) = delta.pointer("/delta/reasoningContent") {
                let content_block = content_blocks.entry(block_index).or_insert_with(|| {
                    parts.push(LanguageModelStreamPart::ReasoningStart(
                        LanguageModelReasoningStart::new(block_index.to_string()),
                    ));
                    StreamContentBlock::Reasoning
                });
                if !matches!(content_block, StreamContentBlock::Reasoning) {
                    *content_block = StreamContentBlock::Reasoning;
                    parts.push(LanguageModelStreamPart::ReasoningStart(
                        LanguageModelReasoningStart::new(block_index.to_string()),
                    ));
                }
                if let Some(text) = reasoning.get("text").and_then(JsonValue::as_str) {
                    parts.push(LanguageModelStreamPart::ReasoningDelta(
                        LanguageModelReasoningDelta::new(block_index.to_string(), text),
                    ));
                } else if let Some(signature) =
                    reasoning.get("signature").and_then(JsonValue::as_str)
                {
                    parts.push(LanguageModelStreamPart::ReasoningDelta(
                        LanguageModelReasoningDelta::new(block_index.to_string(), "")
                            .with_provider_metadata(bedrock_metadata(
                                json!({ "signature": signature }),
                            )),
                    ));
                } else if let Some(data) = reasoning.get("data").and_then(JsonValue::as_str) {
                    parts.push(LanguageModelStreamPart::ReasoningDelta(
                        LanguageModelReasoningDelta::new(block_index.to_string(), "")
                            .with_provider_metadata(bedrock_metadata(
                                json!({ "redactedData": data }),
                            )),
                    ));
                }
            }
        }

        if let Some(block_stop) = value.get("contentBlockStop") {
            let block_index = block_stop
                .get("contentBlockIndex")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0);
            if let Some(block) = content_blocks.remove(&block_index) {
                match block {
                    StreamContentBlock::Text => {
                        parts.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
                            block_index.to_string(),
                        )));
                    }
                    StreamContentBlock::Reasoning => {
                        parts.push(LanguageModelStreamPart::ReasoningEnd(
                            LanguageModelReasoningEnd::new(block_index.to_string()),
                        ));
                    }
                    StreamContentBlock::ToolCall {
                        tool_call_id,
                        tool_name,
                        json_text,
                        is_json_response_tool,
                    } => {
                        if is_json_response_tool {
                            is_json_response_from_tool = true;
                            parts.push(LanguageModelStreamPart::TextStart(
                                LanguageModelTextStart::new(block_index.to_string()),
                            ));
                            parts.push(LanguageModelStreamPart::TextDelta(
                                LanguageModelTextDelta::new(
                                    block_index.to_string(),
                                    json_text.clone(),
                                ),
                            ));
                            parts.push(LanguageModelStreamPart::TextEnd(
                                LanguageModelTextEnd::new(block_index.to_string()),
                            ));
                        } else {
                            parts.push(LanguageModelStreamPart::ToolInputEnd(
                                LanguageModelToolInputEnd::new(tool_call_id.clone()),
                            ));
                            parts.push(LanguageModelStreamPart::ToolCall(
                                LanguageModelToolCall::new(
                                    tool_call_id,
                                    tool_name,
                                    if json_text.is_empty() {
                                        "{}".to_string()
                                    } else {
                                        json_text
                                    },
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }

    if is_json_response_from_tool || !stop_sequence.is_null() {
        let mut payload = provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("amazonBedrock"))
            .cloned()
            .unwrap_or_default();
        if is_json_response_from_tool {
            payload.insert("isJsonResponseFromTool".to_string(), JsonValue::Bool(true));
        }
        payload.insert("stopSequence".to_string(), stop_sequence);
        provider_metadata = Some(bedrock_metadata(JsonValue::Object(payload)));
    }

    let mut finish = LanguageModelStreamFinish::new(usage.0, finish_reason);
    if let Some(provider_metadata) = provider_metadata {
        finish = finish.with_provider_metadata(provider_metadata);
    }
    parts.push(LanguageModelStreamPart::Finish(finish));

    let mut response = LanguageModelStreamResultResponse::new();
    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }
    LanguageModelStreamResult::new(parts)
        .with_request(LanguageModelRequest::new().with_body(request_body))
        .with_response(response)
}

#[derive(Clone, Debug)]
enum StreamContentBlock {
    Text,
    Reasoning,
    ToolCall {
        tool_call_id: String,
        tool_name: String,
        json_text: String,
        is_json_response_tool: bool,
    },
}

#[derive(Clone, Debug, Default)]
struct LanguageModelUsageDefault(ai_sdk_rust::LanguageModelUsage);

fn bedrock_embedding_result_from_response(
    response: JsonValue,
    response_headers: Option<Headers>,
    raw_value: Option<JsonValue>,
) -> EmbeddingModelResult {
    let embedding = if let Some(embedding) = response.get("embedding").and_then(JsonValue::as_array)
    {
        json_array_to_f64_vec(embedding)
    } else if let Some(embeddings) = response.get("embeddings").and_then(JsonValue::as_array) {
        embeddings
            .first()
            .and_then(|first| {
                if first.get("embeddingType").is_some() {
                    first.get("embedding").and_then(JsonValue::as_array)
                } else {
                    first.as_array()
                }
            })
            .map(|values| json_array_to_f64_vec(values))
            .unwrap_or_default()
    } else {
        response
            .pointer("/embeddings/float/0")
            .and_then(JsonValue::as_array)
            .map(|values| json_array_to_f64_vec(values))
            .unwrap_or_default()
    };

    let tokens = response
        .get("inputTextTokenCount")
        .or_else(|| response.get("inputTokenCount"))
        .and_then(JsonValue::as_u64);

    let mut result = EmbeddingModelResult::new(vec![embedding]);
    if let Some(tokens) = tokens {
        result = result.with_usage(EmbeddingModelUsage::new(tokens));
    }
    let mut response_metadata = EmbeddingModelResponse::new();
    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response_metadata = response_metadata.with_header(name, value);
        }
    }
    if let Some(raw_value) = raw_value {
        response_metadata = response_metadata.with_body(raw_value);
    }
    result.with_response(response_metadata)
}

fn bedrock_reranking_result_from_response(
    response: JsonValue,
    response_headers: Option<Headers>,
    raw_value: Option<JsonValue>,
) -> RerankingModelResult {
    let ranking = response
        .get("results")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|value| {
            RerankingModelRanking::new(
                value.get("index").and_then(JsonValue::as_u64).unwrap_or(0) as usize,
                value
                    .get("relevanceScore")
                    .or_else(|| value.get("relevance_score"))
                    .and_then(JsonValue::as_f64)
                    .unwrap_or(0.0),
            )
        })
        .collect::<Vec<_>>();

    let mut response_metadata = RerankingModelResponse::new();
    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response_metadata = response_metadata.with_header(name, value);
        }
    }
    if let Some(raw_value) = raw_value {
        response_metadata = response_metadata.with_body(raw_value);
    }
    RerankingModelResult::new(ranking).with_response(response_metadata)
}

fn decode_bedrock_event_stream_response(
    response: &ProviderApiResponse,
) -> Result<
    ai_sdk_rust::ResponseHandlerResult<Vec<DecodedBedrockEvent>>,
    ProviderApiResponseHandlerError,
> {
    let body = response
        .body
        .as_ref()
        .ok_or_else(|| ProviderApiResponseHandlerError::other("Response body is empty"))?;
    let bytes = body.as_bytes().map_or_else(
        || body.as_text().unwrap_or_default().as_bytes().to_vec(),
        |bytes| bytes.to_vec(),
    );
    let chunks = decode_bedrock_event_stream(&bytes);
    Ok(ai_sdk_rust::ResponseHandlerResult::new(chunks)
        .with_response_headers(response.headers.clone()))
}

/// Decoded Bedrock event-stream event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedBedrockEvent {
    /// Smithy message type.
    pub message_type: String,
    /// Smithy event type.
    pub event_type: String,
    /// Parsed chunk value, when successful.
    pub value: JsonValue,
    /// Raw wrapped chunk value.
    pub raw_value: JsonValue,
    /// Parse or validation error represented as JSON.
    pub error: Option<JsonValue>,
}

/// Decodes either AWS event-stream frames or newline JSON fixture chunks.
pub fn decode_bedrock_event_stream(bytes: &[u8]) -> Vec<DecodedBedrockEvent> {
    if bytes.first() == Some(&b'{') {
        return String::from_utf8_lossy(bytes)
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| match serde_json::from_str::<JsonValue>(line) {
                Ok(value) => DecodedBedrockEvent {
                    message_type: "event".to_string(),
                    event_type: infer_bedrock_event_type(&value).to_string(),
                    raw_value: value.clone(),
                    value,
                    error: None,
                },
                Err(error) => DecodedBedrockEvent {
                    message_type: "event".to_string(),
                    event_type: "parseError".to_string(),
                    value: JsonValue::Null,
                    raw_value: JsonValue::String(line.to_string()),
                    error: Some(json!({ "message": error.to_string() })),
                },
            })
            .collect();
    }

    let mut events = Vec::new();
    let mut cursor = 0_usize;
    while cursor + 12 <= bytes.len() {
        let total_length = u32::from_be_bytes([
            bytes[cursor],
            bytes[cursor + 1],
            bytes[cursor + 2],
            bytes[cursor + 3],
        ]) as usize;
        if total_length == 0 || cursor + total_length > bytes.len() {
            break;
        }
        let headers_length = u32::from_be_bytes([
            bytes[cursor + 4],
            bytes[cursor + 5],
            bytes[cursor + 6],
            bytes[cursor + 7],
        ]) as usize;
        let headers_start = cursor + 12;
        let headers_end = headers_start.saturating_add(headers_length);
        let payload_end = cursor + total_length.saturating_sub(4);
        if headers_end > payload_end || payload_end > bytes.len() {
            break;
        }
        let headers = parse_smithy_headers(&bytes[headers_start..headers_end]);
        let payload = String::from_utf8_lossy(&bytes[headers_end..payload_end]).to_string();
        let message_type = headers
            .get(":message-type")
            .cloned()
            .unwrap_or_else(|| "event".to_string());
        let event_type = headers
            .get(":event-type")
            .cloned()
            .unwrap_or_else(|| "chunk".to_string());
        match serde_json::from_str::<JsonValue>(&payload) {
            Ok(mut value) => {
                if let Some(object) = value.as_object_mut() {
                    object.remove("p");
                }
                let raw_value = json!({ event_type.clone(): value.clone() });
                events.push(DecodedBedrockEvent {
                    message_type,
                    event_type,
                    value: raw_value.clone(),
                    raw_value,
                    error: None,
                });
            }
            Err(error) => events.push(DecodedBedrockEvent {
                message_type,
                event_type,
                value: JsonValue::Null,
                raw_value: JsonValue::String(payload),
                error: Some(json!({ "message": error.to_string() })),
            }),
        }
        cursor += total_length;
    }
    events
}

fn parse_smithy_headers(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        let name_len = bytes[cursor] as usize;
        cursor += 1;
        if cursor + name_len + 1 > bytes.len() {
            break;
        }
        let name = String::from_utf8_lossy(&bytes[cursor..cursor + name_len]).to_string();
        cursor += name_len;
        let header_type = bytes[cursor];
        cursor += 1;
        if header_type == 7 {
            if cursor + 2 > bytes.len() {
                break;
            }
            let value_len = u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
            cursor += 2;
            if cursor + value_len > bytes.len() {
                break;
            }
            let value = String::from_utf8_lossy(&bytes[cursor..cursor + value_len]).to_string();
            cursor += value_len;
            headers.insert(name, value);
        } else {
            break;
        }
    }
    headers
}

fn infer_bedrock_event_type(value: &JsonValue) -> &str {
    value
        .as_object()
        .and_then(|object| object.keys().next().map(String::as_str))
        .unwrap_or("chunk")
}

fn bedrock_error_generate_result(
    model_id: &str,
    message: impl Into<String>,
    prompt: Vec<LanguageModelMessage>,
    request_body: JsonValue,
) -> LanguageModelGenerateResult {
    LanguageModelGenerateResult::new(
        vec![LanguageModelContent::Text(LanguageModelText::new(
            message.into(),
        ))],
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: None,
        },
        ai_sdk_rust::LanguageModelUsage::default(),
    )
    .with_request(
        LanguageModelRequest::new()
            .with_messages(prompt)
            .with_body(request_body),
    )
    .with_response(LanguageModelResponse::new().with_model_id(model_id.to_string()))
}

fn bedrock_error_stream_result(
    message: impl Into<String>,
    request_body: JsonValue,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    LanguageModelStreamResult::new(vec![
        LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!({
            "message": message.into()
        }))),
        LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
            ai_sdk_rust::LanguageModelUsage::default(),
            LanguageModelFinishReason {
                unified: FinishReason::Error,
                raw: None,
            },
        )),
    ])
    .with_request(LanguageModelRequest::new().with_body(request_body))
}

fn bedrock_error_to_message(error: &JsonValue) -> String {
    let error_type = error
        .get("type")
        .or_else(|| error.get("__type"))
        .and_then(JsonValue::as_str);
    let message = error
        .get("message")
        .or_else(|| error.get("Message"))
        .and_then(JsonValue::as_str)
        .unwrap_or("Unknown error");
    error_type.map_or_else(|| message.to_string(), |kind| format!("{kind}: {message}"))
}

/// Converts Bedrock usage into provider-v4 language-model usage.
pub fn convert_amazon_bedrock_usage(usage: Option<&JsonValue>) -> ai_sdk_rust::LanguageModelUsage {
    let Some(usage) = usage else {
        return ai_sdk_rust::LanguageModelUsage::default();
    };
    let input_tokens = usage
        .get("inputTokens")
        .and_then(JsonValue::as_u64)
        .unwrap_or(0);
    let output_tokens = usage
        .get("outputTokens")
        .and_then(JsonValue::as_u64)
        .unwrap_or(0);
    let cache_read = usage
        .get("cacheReadInputTokens")
        .and_then(JsonValue::as_u64)
        .unwrap_or(0);
    let cache_write = usage
        .get("cacheWriteInputTokens")
        .and_then(JsonValue::as_u64)
        .unwrap_or(0);

    ai_sdk_rust::LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: Some(input_tokens + cache_read + cache_write),
            no_cache: Some(input_tokens),
            cache_read: Some(cache_read),
            cache_write: Some(cache_write),
        },
        output_tokens: OutputTokenUsage {
            total: Some(output_tokens),
            text: Some(output_tokens),
            reasoning: None,
        },
        raw: usage.as_object().cloned(),
    }
}

/// Maps Bedrock stop reasons to provider-v4 finish reasons.
pub fn map_amazon_bedrock_finish_reason(
    finish_reason: &str,
    is_json_response_from_tool: bool,
) -> FinishReason {
    match finish_reason {
        "stop_sequence" | "end_turn" => FinishReason::Stop,
        "max_tokens" => FinishReason::Length,
        "content_filtered" | "guardrail_intervened" => FinishReason::ContentFilter,
        "tool_use" => {
            if is_json_response_from_tool {
                FinishReason::Stop
            } else {
                FinishReason::ToolCalls
            }
        }
        _ => FinishReason::Other,
    }
}

/// Returns whether a model id is a Mistral Bedrock model.
pub fn is_mistral_model(model_id: &str) -> bool {
    model_id.contains("mistral.")
}

/// Normalizes Bedrock tool-call ids for Mistral models.
pub fn normalize_tool_call_id(tool_call_id: &str, is_mistral: bool) -> String {
    if !is_mistral {
        return tool_call_id.to_string();
    }

    tool_call_id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(9)
        .collect()
}

fn bedrock_response_provider_metadata(
    response: &JsonValue,
    is_json_response_from_tool: bool,
    stop_sequence: Option<JsonValue>,
) -> Option<ProviderMetadata> {
    let mut payload = JsonObject::new();
    insert_json_if_present(&mut payload, "trace", response.get("trace").cloned());
    insert_json_if_present(
        &mut payload,
        "performanceConfig",
        response.get("performanceConfig").cloned(),
    );
    insert_json_if_present(
        &mut payload,
        "serviceTier",
        response.get("serviceTier").cloned(),
    );
    if let Some(usage) = response.get("usage") {
        let mut usage_payload = JsonObject::new();
        insert_json_if_present(
            &mut usage_payload,
            "cacheWriteInputTokens",
            usage.get("cacheWriteInputTokens").cloned(),
        );
        insert_json_if_present(
            &mut usage_payload,
            "cacheDetails",
            usage.get("cacheDetails").cloned(),
        );
        if !usage_payload.is_empty() {
            payload.insert("usage".to_string(), JsonValue::Object(usage_payload));
        }
    }
    if is_json_response_from_tool {
        payload.insert("isJsonResponseFromTool".to_string(), JsonValue::Bool(true));
    }
    if let Some(stop_sequence) = stop_sequence {
        payload.insert("stopSequence".to_string(), stop_sequence);
    }
    if payload.is_empty() {
        None
    } else {
        Some(bedrock_metadata(JsonValue::Object(payload)))
    }
}

fn bedrock_metadata_from_stream_metadata(metadata: &JsonValue) -> Option<ProviderMetadata> {
    let mut payload = JsonObject::new();
    insert_json_if_present(&mut payload, "trace", metadata.get("trace").cloned());
    insert_json_if_present(
        &mut payload,
        "performanceConfig",
        metadata.get("performanceConfig").cloned(),
    );
    insert_json_if_present(
        &mut payload,
        "serviceTier",
        metadata.get("serviceTier").cloned(),
    );
    if let Some(usage) = metadata.get("usage") {
        let mut usage_payload = JsonObject::new();
        insert_json_if_present(
            &mut usage_payload,
            "cacheWriteInputTokens",
            usage.get("cacheWriteInputTokens").cloned(),
        );
        insert_json_if_present(
            &mut usage_payload,
            "cacheDetails",
            usage.get("cacheDetails").cloned(),
        );
        if !usage_payload.is_empty() {
            payload.insert("usage".to_string(), JsonValue::Object(usage_payload));
        }
    }
    if payload.is_empty() {
        None
    } else {
        Some(bedrock_metadata(JsonValue::Object(payload)))
    }
}

fn bedrock_metadata(payload: JsonValue) -> ProviderMetadata {
    let object = payload.as_object().cloned().unwrap_or_default();
    BTreeMap::from([
        ("amazonBedrock".to_string(), object.clone()),
        ("bedrock".to_string(), object),
    ])
}

fn headers_with_user_agent_suffix(headers: &Headers) -> Headers {
    let mut output = Headers::new();
    for (name, value) in headers {
        output.insert(name.clone(), value.clone());
    }
    let user_agent = output
        .remove("user-agent")
        .or_else(|| output.remove("User-Agent"))
        .map(|existing| {
            if existing.contains(AMAZON_BEDROCK_USER_AGENT) {
                existing
            } else {
                format!("{existing} {AMAZON_BEDROCK_USER_AGENT}")
            }
        })
        .unwrap_or_else(|| AMAZON_BEDROCK_USER_AGENT.to_string());
    output.insert("user-agent".to_string(), user_agent);
    output
}

fn authenticate_request(
    auth: &AmazonBedrockAuthConfig,
    request: &mut ProviderApiRequest,
    date: OffsetDateTime,
) -> Result<(), FetchErrorInfo> {
    match auth {
        AmazonBedrockAuthConfig::ApiKey { api_key } => {
            request
                .headers
                .insert("authorization".to_string(), format!("Bearer {api_key}"));
            Ok(())
        }
        AmazonBedrockAuthConfig::SigV4 {
            service,
            region,
            settings,
        } => {
            let credentials = resolve_credentials(settings, region).map_err(|message| {
                FetchErrorInfo::new("AWS SigV4 authentication requires AWS credentials")
                    .with_name("Error")
                    .with_cause_message(message)
            })?;
            sign_request(request, &credentials, service, date);
            Ok(())
        }
    }
}

fn resolve_credentials(
    settings: &AmazonBedrockProviderSettings,
    region: &str,
) -> Result<AmazonBedrockCredentials, String> {
    if let Some(provider) = &settings.credential_provider {
        let mut credentials = provider()?;
        if credentials.region.trim().is_empty() {
            credentials.region = region.to_string();
        }
        return Ok(credentials);
    }

    let access_key_id = first_non_empty([
        settings.access_key_id.clone(),
        env::var("AWS_ACCESS_KEY_ID").ok(),
    ])
    .ok_or_else(|| {
        "AWS_ACCESS_KEY_ID or accessKeyId must be provided for SigV4 authentication".to_string()
    })?;
    let secret_access_key = first_non_empty([
        settings.secret_access_key.clone(),
        env::var("AWS_SECRET_ACCESS_KEY").ok(),
    ])
    .ok_or_else(|| {
        "AWS_SECRET_ACCESS_KEY or secretAccessKey must be provided for SigV4 authentication"
            .to_string()
    })?;
    let session_token = if settings.access_key_id.is_some() && settings.secret_access_key.is_some()
    {
        settings.session_token.clone()
    } else {
        first_non_empty([
            settings.session_token.clone(),
            env::var("AWS_SESSION_TOKEN").ok(),
        ])
    };

    Ok(AmazonBedrockCredentials {
        region: region.to_string(),
        access_key_id,
        secret_access_key,
        session_token,
    })
}

/// Applies AWS Signature Version 4 headers to a prepared provider request.
pub fn sign_request(
    request: &mut ProviderApiRequest,
    credentials: &AmazonBedrockCredentials,
    service: &str,
    date: OffsetDateTime,
) {
    let amz_date = amz_datetime(date);
    let date_stamp = amz_date[..8].to_string();
    let payload = request_body_bytes(request);
    let payload_hash = sha256_hex(&payload);

    let url = Url::parse(&request.url).expect("provider request URL is absolute");
    request
        .headers
        .insert("host".to_string(), host_header(&url));
    request
        .headers
        .insert("x-amz-date".to_string(), amz_date.clone());
    request
        .headers
        .insert("x-amz-content-sha256".to_string(), payload_hash.clone());
    if let Some(session_token) = &credentials.session_token {
        request
            .headers
            .insert("x-amz-security-token".to_string(), session_token.clone());
    }

    let canonical_request = canonical_request(request, &url, &payload_hash);
    let credential_scope = format!(
        "{}/{}/{}/aws4_request",
        date_stamp, credentials.region, service
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date,
        credential_scope,
        sha256_hex(canonical_request.as_bytes())
    );
    let signature = hex_encode(&hmac_sha256(
        &signing_key(
            &credentials.secret_access_key,
            &date_stamp,
            &credentials.region,
            service,
        ),
        string_to_sign.as_bytes(),
    ));
    let signed_headers = signed_headers(&request.headers);
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        credentials.access_key_id, credential_scope, signed_headers, signature
    );
    request
        .headers
        .insert("authorization".to_string(), authorization);
}

fn canonical_request(request: &ProviderApiRequest, url: &Url, payload_hash: &str) -> String {
    let method = match request.method {
        ProviderApiRequestMethod::Get => "GET",
        ProviderApiRequestMethod::Post => "POST",
    };
    format!(
        "{method}\n{}\n{}\n{}\n{}\n{}",
        canonical_uri(url),
        canonical_query(url),
        canonical_headers(&request.headers),
        signed_headers(&request.headers),
        payload_hash
    )
}

fn canonical_uri(url: &Url) -> String {
    let path = url.path();
    if path.is_empty() {
        "/".to_string()
    } else {
        path.split('/')
            .map(|segment| uri_encode(segment.as_bytes(), true))
            .collect::<Vec<_>>()
            .join("/")
    }
}

fn canonical_query(url: &Url) -> String {
    let mut pairs = url
        .query_pairs()
        .map(|(key, value)| {
            (
                uri_encode(key.as_bytes(), true),
                uri_encode(value.as_bytes(), true),
            )
        })
        .collect::<Vec<_>>();
    pairs.sort();
    pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn canonical_headers(headers: &Headers) -> String {
    normalized_headers(headers)
        .into_iter()
        .map(|(name, value)| format!("{name}:{}\n", collapse_header_spaces(&value)))
        .collect::<String>()
}

fn signed_headers(headers: &Headers) -> String {
    normalized_headers(headers)
        .into_keys()
        .collect::<Vec<_>>()
        .join(";")
}

fn normalized_headers(headers: &Headers) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("authorization"))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_string()))
        .collect()
}

fn collapse_header_spaces(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn signing_key(secret: &str, date_stamp: &str, region: &str, service: &str) -> Vec<u8> {
    let date_key = hmac_sha256(format!("AWS4{secret}").as_bytes(), date_stamp.as_bytes());
    let region_key = hmac_sha256(&date_key, region.as_bytes());
    let service_key = hmac_sha256(&region_key, service.as_bytes());
    hmac_sha256(&service_key, b"aws4_request")
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    hex_encode(&Sha256::digest(data))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn request_body_bytes(request: &ProviderApiRequest) -> Vec<u8> {
    match request.body.as_ref() {
        Some(ProviderApiRequestBody::Text { content }) => content.as_bytes().to_vec(),
        Some(ProviderApiRequestBody::Bytes { content }) => content.clone(),
        Some(ProviderApiRequestBody::FormData { .. }) | None => Vec::new(),
    }
}

fn host_header(url: &Url) -> String {
    match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
        None => url.host_str().unwrap_or_default().to_string(),
    }
}

fn amz_datetime(date: OffsetDateTime) -> String {
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        date.year(),
        u8::from(date.month()),
        date.day(),
        date.hour(),
        date.minute(),
        date.second()
    )
}

fn encode_uri_component(value: &str) -> String {
    uri_encode(value.as_bytes(), true)
}

fn uri_encode(bytes: &[u8], encode_slash: bool) -> String {
    let mut out = String::new();
    for &byte in bytes {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') {
            out.push(ch);
        } else if ch == '/' && !encode_slash {
            out.push('/');
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn default_amazon_bedrock_transport() -> AmazonBedrockTransport {
    Arc::new(|request| Box::pin(ready(execute_amazon_bedrock_request(request))))
}

fn execute_amazon_bedrock_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_amazon_bedrock_get_request(request),
        ProviderApiRequestMethod::Post => execute_amazon_bedrock_post_request(request),
    }
}

fn execute_amazon_bedrock_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    bedrock_provider_api_response(builder.config().http_status_as_error(false).build().call())
}

fn execute_amazon_bedrock_post_request(
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
                "multipart form data is not supported by the Amazon Bedrock transport",
            ));
        }
        None => builder.send_empty(),
    };
    bedrock_provider_api_response(response)
}

fn bedrock_provider_api_response(
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
    let body = response.body_mut().read_to_vec().map_err(|error| {
        FetchErrorInfo::new("failed to read response body")
            .with_name("Error")
            .with_cause_message(error.to_string())
    })?;
    Ok(ProviderApiResponse::bytes(status.as_u16(), status_text, body).with_headers(headers))
}

fn bedrock_provider_options(provider_options: Option<&ProviderOptions>) -> JsonObject {
    provider_options
        .and_then(|options| {
            options
                .get("amazonBedrock")
                .or_else(|| options.get("bedrock"))
                .cloned()
        })
        .unwrap_or_default()
}

fn reasoning_metadata(provider_options: Option<&ProviderOptions>) -> Option<JsonObject> {
    provider_options.and_then(|options| {
        options
            .get("amazonBedrock")
            .or_else(|| options.get("bedrock"))
            .cloned()
    })
}

fn cache_point(provider_options: Option<&ProviderOptions>) -> Option<JsonValue> {
    bedrock_provider_options(provider_options)
        .get("cachePoint")
        .cloned()
        .map(|cache_point| json!({ "cachePoint": cache_point }))
}

fn push_cache_point(content: &mut Vec<JsonValue>, provider_options: Option<&ProviderOptions>) {
    if let Some(cache_point) = cache_point(provider_options) {
        content.push(cache_point);
    }
}

fn citations_enabled(provider_options: Option<&ProviderOptions>) -> bool {
    JsonValue::Object(bedrock_provider_options(provider_options))
        .pointer("/citations/enabled")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false)
}

fn apply_reasoning_options(
    model_id: &str,
    reasoning: Option<&ai_sdk_rust::LanguageModelReasoningEffort>,
    provider_options: &mut JsonObject,
    warnings: &mut Vec<Warning>,
) {
    let Some(reasoning) = reasoning else {
        return;
    };
    let effort = match reasoning {
        ai_sdk_rust::LanguageModelReasoningEffort::ProviderDefault => return,
        ai_sdk_rust::LanguageModelReasoningEffort::None => {
            provider_options.insert("reasoningConfig".to_string(), json!({ "type": "disabled" }));
            return;
        }
        ai_sdk_rust::LanguageModelReasoningEffort::Minimal
        | ai_sdk_rust::LanguageModelReasoningEffort::Low => "low",
        ai_sdk_rust::LanguageModelReasoningEffort::Medium => "medium",
        ai_sdk_rust::LanguageModelReasoningEffort::High => "high",
        ai_sdk_rust::LanguageModelReasoningEffort::Xhigh => "max",
    };

    if is_anthropic_model(model_id) {
        provider_options.insert(
            "reasoningConfig".to_string(),
            json!({
                "type": "enabled",
                "maxReasoningEffort": effort
            }),
        );
    } else {
        provider_options.insert(
            "reasoningConfig".to_string(),
            json!({ "maxReasoningEffort": effort }),
        );
    }
    warnings.push(Warning::Compatibility {
        feature: "reasoning".to_string(),
        details: Some(format!("Mapped reasoning effort to Bedrock `{effort}`.")),
    });
}

fn prompt_has_tool_content(prompt: &[LanguageModelMessage]) -> bool {
    prompt.iter().any(|message| match message {
        LanguageModelMessage::Assistant(message) => message.content.iter().any(|part| {
            matches!(
                part,
                LanguageModelAssistantContentPart::ToolCall(_)
                    | LanguageModelAssistantContentPart::ToolResult(_)
            )
        }),
        LanguageModelMessage::Tool(message) => !message.content.is_empty(),
        LanguageModelMessage::System(_) | LanguageModelMessage::User(_) => false,
    })
}

fn filter_tool_content_from_prompt(prompt: &[LanguageModelMessage]) -> Vec<LanguageModelMessage> {
    prompt
        .iter()
        .filter_map(|message| match message {
            LanguageModelMessage::Assistant(message) => {
                let content = message
                    .content
                    .iter()
                    .filter(|part| {
                        !matches!(
                            part,
                            LanguageModelAssistantContentPart::ToolCall(_)
                                | LanguageModelAssistantContentPart::ToolResult(_)
                        )
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if content.is_empty() {
                    None
                } else {
                    Some(LanguageModelMessage::Assistant(
                        LanguageModelAssistantMessage::new(content),
                    ))
                }
            }
            LanguageModelMessage::Tool(_) => None,
            other => Some(other.clone()),
        })
        .collect()
}

fn is_anthropic_model(model_id: &str) -> bool {
    model_id.contains("anthropic")
}

fn bedrock_anthropic_tool_upgrade(
    tool_type: &str,
) -> Option<(&'static str, Option<&'static str>, Option<&'static str>)> {
    match tool_type {
        "bash_20241022" => Some(("bash_20250124", Some("computer-use-2025-01-24"), None)),
        "text_editor_20241022" => Some((
            "text_editor_20250728",
            Some("computer-use-2025-01-24"),
            Some("str_replace_based_edit_tool"),
        )),
        "computer_20241022" => Some(("computer_20250124", Some("computer-use-2025-01-24"), None)),
        "bash_20250124" => Some(("bash_20250124", Some("computer-use-2025-01-24"), None)),
        "text_editor_20250124" => Some((
            "text_editor_20250124",
            Some("computer-use-2025-01-24"),
            None,
        )),
        "text_editor_20250429" => Some((
            "text_editor_20250429",
            Some("computer-use-2025-01-24"),
            None,
        )),
        "text_editor_20250728" => Some((
            "text_editor_20250728",
            Some("computer-use-2025-01-24"),
            Some("str_replace_based_edit_tool"),
        )),
        "computer_20250124" => Some(("computer_20250124", Some("computer-use-2025-01-24"), None)),
        "tool_search_tool_regex_20251119" => Some((
            "tool_search_tool_regex_20251119",
            Some("tool-search-tool-2025-10-19"),
            None,
        )),
        "tool_search_tool_bm25_20251119" => Some((
            "tool_search_tool_bm25_20251119",
            Some("tool-search-tool-2025-10-19"),
            None,
        )),
        _ => None,
    }
}

fn amazon_bedrock_image_format(
    media_type: &str,
) -> Result<&'static str, UnsupportedFunctionalityError> {
    match media_type {
        "image/jpeg" => Ok("jpeg"),
        "image/png" => Ok("png"),
        "image/gif" => Ok("gif"),
        "image/webp" => Ok("webp"),
        _ => Err(UnsupportedFunctionalityError::with_message(
            format!("image mime type: {media_type}"),
            format!(
                "Unsupported image mime type: {media_type}, expected one of: image/jpeg, image/png, image/gif, image/webp"
            ),
        )),
    }
}

fn amazon_bedrock_document_format(
    media_type: &str,
) -> Result<&'static str, UnsupportedFunctionalityError> {
    match media_type {
        "application/pdf" => Ok("pdf"),
        "text/csv" => Ok("csv"),
        "application/msword" => Ok("doc"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Ok("docx"),
        "application/vnd.ms-excel" => Ok("xls"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Ok("xlsx"),
        "text/html" => Ok("html"),
        "text/plain" => Ok("txt"),
        "text/markdown" => Ok("md"),
        _ => Err(UnsupportedFunctionalityError::with_message(
            format!("file mime type: {media_type}"),
            format!(
                "Unsupported file mime type: {media_type}, expected one of: application/pdf, text/csv, application/msword, application/vnd.openxmlformats-officedocument.wordprocessingml.document, application/vnd.ms-excel, application/vnd.openxmlformats-officedocument.spreadsheetml.sheet, text/html, text/plain, text/markdown"
            ),
        )),
    }
}

fn image_file_base64(file: &ImageModelFile) -> Result<String, UnsupportedFunctionalityError> {
    match file {
        ImageModelFile::Url { .. } => Err(UnsupportedFunctionalityError::with_message(
            "URL-based images",
            "URL-based images are not supported for Amazon Bedrock image editing. Please provide the image data directly.",
        )),
        ImageModelFile::File { data, .. } => Ok(convert_to_base64(data)),
    }
}

fn json_array_to_f64_vec(values: &[JsonValue]) -> Vec<f64> {
    values.iter().filter_map(JsonValue::as_f64).collect()
}

fn first_non_empty(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

fn unsupported(feature: impl Into<String>, details: Option<String>) -> Warning {
    Warning::Unsupported {
        feature: feature.into(),
        details,
    }
}

fn insert_some<T>(object: &mut JsonObject, key: &str, value: Option<T>)
where
    T: Serialize,
{
    if let Some(value) = value {
        object.insert(key.to_string(), json!(value));
    }
}

fn insert_json_if_present(object: &mut JsonObject, key: &str, value: Option<JsonValue>) {
    if let Some(value) = value.filter(|value| !value.is_null()) {
        object.insert(key.to_string(), value);
    }
}

fn string_option(object: &JsonObject, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
}

fn u64_option(object: &JsonObject, key: &str) -> Option<u64> {
    object.get(key).and_then(JsonValue::as_u64)
}

fn merge_object_field(
    object: &mut JsonObject,
    top_key: &str,
    nested_key: &str,
    nested_value: JsonValue,
) {
    object
        .entry(top_key.to_string())
        .or_insert_with(|| JsonValue::Object(JsonObject::new()))
        .as_object_mut()
        .expect("merged field remains object")
        .insert(nested_key.to_string(), nested_value);
}

fn merge_json_object(target: &mut JsonValue, source: JsonValue) {
    if let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
}

fn json_object_is_empty(value: &JsonValue) -> bool {
    value.as_object().is_none_or(JsonObject::is_empty)
}

fn header_value<'a>(headers: &'a Headers, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn push_unique_string(values: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !values.contains(&value) {
        values.push(value);
    }
}

fn default_generate_id() -> String {
    "bedrock-generated-id".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_sdk_rust::{
        LanguageModelFilePart, LanguageModelProviderTool, LanguageModelReasoningPart,
        LanguageModelTextPart, LanguageModelToolCallPart, LanguageModelToolMessage,
        LanguageModelToolResultPart,
    };
    use futures_executor::block_on;
    use std::sync::{Mutex, MutexGuard};

    fn fixed_date() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_711_115_037).expect("fixed timestamp is valid")
    }

    fn capture_transport(
        responses: Vec<ProviderApiResponse>,
    ) -> (Arc<Mutex<Vec<ProviderApiRequest>>>, AmazonBedrockTransport) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let responses = Arc::new(Mutex::new(responses));
        let requests_for_transport = Arc::clone(&requests);
        let responses_for_transport = Arc::clone(&responses);
        let transport: AmazonBedrockTransport = Arc::new(move |request| {
            requests_for_transport
                .lock()
                .expect("request capture mutex is not poisoned")
                .push(request);
            let response = responses_for_transport
                .lock()
                .expect("response capture mutex is not poisoned")
                .remove(0);
            Box::pin(ready(Ok(response)))
        });
        (requests, transport)
    }

    fn captured_requests(
        requests: &Arc<Mutex<Vec<ProviderApiRequest>>>,
    ) -> MutexGuard<'_, Vec<ProviderApiRequest>> {
        requests
            .lock()
            .expect("request capture mutex is not poisoned")
    }

    fn provider_options(value: JsonValue) -> ProviderOptions {
        serde_json::from_value(value).expect("provider options deserialize")
    }

    fn user_prompt(text: &str) -> Vec<LanguageModelMessage> {
        vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::Text(
                LanguageModelTextPart::new(text),
            )],
        ))]
    }

    fn request_json(request: &ProviderApiRequest) -> JsonValue {
        serde_json::from_str(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .expect("request body is JSON text"),
        )
        .expect("request body parses as JSON")
    }

    fn ok_chat_response(text: &str) -> ProviderApiResponse {
        ProviderApiResponse::text(
            200,
            "OK",
            json!({
                "output": {
                    "message": {
                        "role": "assistant",
                        "content": [{ "text": text }]
                    }
                },
                "stopReason": "end_turn",
                "usage": {
                    "inputTokens": 7,
                    "outputTokens": 5,
                    "cacheReadInputTokens": 2,
                    "cacheWriteInputTokens": 3
                },
                "additionalModelResponseFields": {
                    "delta": { "stop_sequence": "stop-here" }
                }
            })
            .to_string(),
        )
        .with_headers(Headers::from([(
            "x-amzn-requestid".to_string(),
            "req-bedrock".to_string(),
        )]))
    }

    fn stream_response() -> ProviderApiResponse {
        let body = [
            json!({ "messageStart": { "role": "assistant" } }).to_string(),
            json!({
                "contentBlockStart": {
                    "contentBlockIndex": 0,
                    "start": {}
                }
            })
            .to_string(),
            json!({
                "contentBlockDelta": {
                    "contentBlockIndex": 0,
                    "delta": { "text": "Hel" }
                }
            })
            .to_string(),
            json!({
                "contentBlockDelta": {
                    "contentBlockIndex": 0,
                    "delta": { "text": "lo" }
                }
            })
            .to_string(),
            json!({ "contentBlockStop": { "contentBlockIndex": 0 } }).to_string(),
            json!({
                "messageStop": {
                    "stopReason": "end_turn",
                    "additionalModelResponseFields": {
                        "delta": { "stop_sequence": "stop-here" }
                    }
                }
            })
            .to_string(),
            json!({
                "metadata": {
                    "usage": {
                        "inputTokens": 2,
                        "outputTokens": 1
                    },
                    "trace": { "guardrail": "off" }
                }
            })
            .to_string(),
        ]
        .join("\n");

        ProviderApiResponse::text(200, "OK", body).with_headers(Headers::from([(
            "x-amzn-requestid".to_string(),
            "req-stream".to_string(),
        )]))
    }

    fn model_with_responses(
        responses: Vec<ProviderApiResponse>,
    ) -> (
        AmazonBedrockChatLanguageModel,
        Arc<Mutex<Vec<ProviderApiRequest>>>,
    ) {
        let (requests, transport) = capture_transport(responses);
        let model = create_amazon_bedrock(
            AmazonBedrockProviderSettings::new()
                .with_region("us-west-2")
                .with_api_key("bedrock-api-key")
                .with_base_url("https://bedrock.test")
                .with_header("x-provider", "bedrock"),
        )
        .with_transport(transport)
        .with_current_date(fixed_date)
        .language_model("anthropic.claude-3-5-sonnet-20240620-v1:0");
        (model, requests)
    }

    #[test]
    fn amazon_bedrock_cache_points_match_upstream_ttl_cases() {
        assert_eq!(
            create_amazon_bedrock_cache_point(None),
            json!({ "cachePoint": { "type": "default" } })
        );
        assert_eq!(
            create_amazon_bedrock_cache_point(Some(AmazonBedrockCacheTtl::FiveMinutes)),
            json!({ "cachePoint": { "type": "default", "ttl": "5m" } })
        );
        assert_eq!(
            create_amazon_bedrock_cache_point(Some(AmazonBedrockCacheTtl::OneHour)),
            json!({ "cachePoint": { "type": "default", "ttl": "1h" } })
        );
        assert_eq!(UPSTREAM_TEST_CASES, 383);
        assert_eq!(PORTABLE_MAPPED_CASES, 380);
        assert_eq!(PORTABLE_UNMAPPED_CASES, 0);
        assert_eq!(JS_ONLY_DOCUMENTED_CASES, 3);
    }

    #[test]
    fn amazon_bedrock_provider_factories_auth_and_headers_match_upstream() {
        let (model, requests) = model_with_responses(vec![ok_chat_response("Hello")]);

        let result = block_on(model.do_generate(
            LanguageModelCallOptions::new(user_prompt("Hello?")).with_header("x-call", "call"),
        ));

        assert_eq!(model.provider(), AMAZON_BEDROCK_PROVIDER_ID);
        assert_eq!(
            model.model_id(),
            "anthropic.claude-3-5-sonnet-20240620-v1:0"
        );
        assert_eq!(
            result.response.expect("response metadata").id,
            Some("req-bedrock".to_string())
        );
        let requests = captured_requests(&requests);
        let request = requests.first().expect("request captured");
        assert_eq!(
            request.url,
            "https://bedrock.test/model/anthropic.claude-3-5-sonnet-20240620-v1%3A0/converse"
        );
        assert_eq!(
            request.headers.get("authorization"),
            Some(&"Bearer bedrock-api-key".to_string())
        );
        assert_eq!(
            request.headers.get("x-provider"),
            Some(&"bedrock".to_string())
        );
        assert_eq!(request.headers.get("x-call"), Some(&"call".to_string()));
        assert!(
            request
                .headers
                .get("user-agent")
                .expect("user-agent header")
                .contains("ai-sdk/amazon-bedrock/")
        );
    }

    #[test]
    fn amazon_bedrock_sigv4_and_api_key_fetch_wrappers_sign_requests() {
        let mut request = ProviderApiRequest::new(
            ProviderApiRequestMethod::Post,
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/test/invoke?b=2&a=1",
            Headers::from([("content-type".to_string(), "application/json".to_string())]),
            Some(ProviderApiRequestBody::text(r#"{"prompt":"hi"}"#)),
            json!({ "prompt": "hi" }),
        );
        let credentials = AmazonBedrockCredentials::new("us-east-1", "AKIDEXAMPLE", "SECRET")
            .with_session_token("session");

        sign_request(&mut request, &credentials, "bedrock", fixed_date());

        assert_eq!(
            request.headers.get("x-amz-date"),
            Some(&"20240322T134357Z".to_string())
        );
        assert_eq!(
            request.headers.get("x-amz-security-token"),
            Some(&"session".to_string())
        );
        assert_eq!(
            request.headers.get("host"),
            Some(&"bedrock-runtime.us-east-1.amazonaws.com".to_string())
        );
        let authorization = request
            .headers
            .get("authorization")
            .expect("authorization header");
        assert!(authorization.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20240322/us-east-1/bedrock/aws4_request"
        ));
        assert!(authorization.contains(
            "SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date;x-amz-security-token"
        ));

        let mut api_key_request = ProviderApiRequest::post(
            "https://bedrock.test/model/test/invoke",
            Headers::new(),
            ProviderApiRequestBody::text(r#"{"prompt":"hi"}"#),
            json!({ "prompt": "hi" }),
        );
        authenticate_request(
            &AmazonBedrockAuthConfig::ApiKey {
                api_key: "bearer-key".to_string(),
            },
            &mut api_key_request,
            fixed_date(),
        )
        .expect("api key authentication succeeds");
        assert_eq!(
            api_key_request.headers.get("authorization"),
            Some(&"Bearer bearer-key".to_string())
        );
    }

    #[test]
    fn amazon_bedrock_chat_options_validate_service_tier_and_types() {
        let model = amazon_bedrock("anthropic.claude-3-5-sonnet-20240620-v1:0");
        let (body, warnings, uses_json_tool) = model
            .request_body(
                &LanguageModelCallOptions::new(user_prompt("shape this"))
                    .with_temperature(2.0)
                    .with_top_p(0.9)
                    .with_top_k(20)
                    .with_max_output_tokens(64)
                    .with_frequency_penalty(0.2)
                    .with_stop_sequence("END")
                    .with_provider_options(provider_options(json!({
                        "amazonBedrock": {
                            "serviceTier": "priority",
                            "additionalModelRequestFields": { "trace": "ENABLED" }
                        }
                    }))),
            )
            .expect("request body");

        assert!(!uses_json_tool);
        assert_eq!(body["serviceTier"], json!({ "type": "priority" }));
        assert_eq!(
            body["additionalModelRequestFields"]["trace"],
            json!("ENABLED")
        );
        assert_eq!(body["inferenceConfig"]["temperature"], json!(1.0));
        assert_eq!(body["inferenceConfig"]["topP"], json!(0.9));
        assert_eq!(body["inferenceConfig"]["topK"], json!(20));
        assert_eq!(body["inferenceConfig"]["stopSequences"], json!(["END"]));
        assert!(warnings.iter().any(|warning| {
            matches!(warning, Warning::Unsupported { feature, .. } if feature == "temperature")
        }));
        assert!(warnings.iter().any(|warning| {
            matches!(warning, Warning::Unsupported { feature, .. } if feature == "frequencyPenalty")
        }));
    }

    #[test]
    fn amazon_bedrock_prepare_tools_maps_function_anthropic_and_choice_cases() {
        let mut warnings = Vec::new();
        let weather_tool = LanguageModelTool::Function(
            LanguageModelFunctionTool::new(
                "weather",
                serde_json::from_value(json!({
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }))
                .expect("schema object"),
            )
            .with_description("Look up weather")
            .with_strict(true),
        );

        let prepared = prepare_tools_for_bedrock(
            &[weather_tool],
            Some(&LanguageModelToolChoice::Tool {
                tool_name: "weather".to_string(),
            }),
            "amazon.nova-pro-v1:0",
            &mut warnings,
        )
        .expect("tools prepare");

        assert_eq!(
            prepared.tool_config["toolChoice"],
            json!({ "tool": { "name": "weather" } })
        );
        assert_eq!(
            prepared.tool_config["tools"][0]["toolSpec"]["inputSchema"]["json"]["required"],
            json!(["city"])
        );

        let provider_tool = LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "anthropic.bash_20241022",
            "bash",
            serde_json::from_value(json!({ "type": "bash_20241022" })).expect("args object"),
        ));
        let prepared = prepare_tools_for_bedrock(
            &[provider_tool],
            Some(&LanguageModelToolChoice::Auto),
            "anthropic.claude-3-7-sonnet-20250219-v1:0",
            &mut warnings,
        )
        .expect("anthropic tools prepare");

        assert!(
            prepared
                .betas
                .contains(&"computer-use-2025-01-24".to_string())
        );
        assert_eq!(
            prepared
                .additional_tools
                .expect("anthropic tool choice")
                .get("tool_choice"),
            Some(&json!({ "auto": {} }))
        );
    }

    #[test]
    fn amazon_bedrock_message_conversion_maps_prompt_content_files_tools_and_reasoning() {
        let cache = provider_options(json!({
            "amazonBedrock": {
                "cachePoint": { "type": "default", "ttl": "5m" }
            }
        }));
        let citations = provider_options(json!({
            "amazonBedrock": {
                "citations": { "enabled": true }
            }
        }));
        let reasoning_metadata = provider_options(json!({
            "amazonBedrock": { "signature": "signed-thinking" }
        }));

        let prompt = vec![
            LanguageModelMessage::System(
                LanguageModelSystemMessage::new("You are helpful").with_provider_options(cache),
            ),
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Read this")),
                LanguageModelUserContentPart::File(
                    LanguageModelFilePart::new(
                        FileData::Text {
                            text: "# Notes".to_string(),
                        },
                        "text/markdown",
                    )
                    .with_filename("notes.md")
                    .with_provider_options(citations),
                ),
            ])),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Reasoning(
                    LanguageModelReasoningPart::new("thinking")
                        .with_provider_options(reasoning_metadata),
                ),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "tool-call-id-with-dashes",
                    "lookup",
                    json!({ "q": "rust" }),
                )),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "tool-call-id-with-dashes",
                    "lookup",
                    LanguageModelToolResultOutput::text("found"),
                )),
            ])),
        ];

        let converted =
            convert_to_amazon_bedrock_chat_messages(&prompt, true).expect("converted prompt");

        assert_eq!(converted.system[0], json!({ "text": "You are helpful" }));
        assert_eq!(
            converted.system[1],
            json!({ "cachePoint": { "type": "default", "ttl": "5m" } })
        );
        assert_eq!(converted.messages[0]["role"], json!("user"));
        assert_eq!(
            converted.messages[0]["content"][1]["document"]["format"],
            json!("md")
        );
        assert_eq!(
            converted.messages[0]["content"][1]["document"]["citations"],
            json!({ "enabled": true })
        );
        assert_eq!(
            converted.messages[1]["content"][0]["reasoningContent"]["reasoningText"]["signature"],
            json!("signed-thinking")
        );
        assert_eq!(
            converted.messages[1]["content"][1]["toolUse"]["toolUseId"],
            json!("toolcalli")
        );
        assert_eq!(
            converted.messages[2]["content"][0]["toolResult"]["toolUseId"],
            json!("toolcalli")
        );
    }

    #[test]
    fn amazon_bedrock_chat_generate_and_stream_match_upstream_fixtures() {
        let (model, requests) =
            model_with_responses(vec![ok_chat_response("Hello"), stream_response()]);

        let generated =
            block_on(model.do_generate(LanguageModelCallOptions::new(user_prompt("Hi"))));
        assert!(matches!(
            &generated.content[0],
            LanguageModelContent::Text(text) if text.text == "Hello"
        ));
        assert_eq!(generated.finish_reason.unified, FinishReason::Stop);
        assert_eq!(generated.usage.input_tokens.total, Some(12));
        assert_eq!(generated.usage.output_tokens.total, Some(5));
        assert_eq!(
            generated.provider_metadata.expect("provider metadata")["amazonBedrock"]["stopSequence"],
            json!("stop-here")
        );

        let streamed = block_on(model.do_stream(
            LanguageModelCallOptions::new(user_prompt("Hi")).with_include_raw_chunks(true),
        ));
        assert!(streamed.stream.iter().any(|part| {
            matches!(part, LanguageModelStreamPart::TextDelta(delta) if delta.delta == "Hel")
        }));
        assert!(streamed.stream.iter().any(|part| {
            matches!(part, LanguageModelStreamPart::Raw(raw) if raw.raw_value.get("contentBlockDelta").is_some())
        }));
        assert!(streamed.stream.iter().any(|part| {
            matches!(part, LanguageModelStreamPart::Finish(finish) if finish.finish_reason.unified == FinishReason::Stop)
        }));

        let requests = captured_requests(&requests);
        assert!(requests[0].url.ends_with("/converse"));
        assert!(requests[1].url.ends_with("/converse-stream"));
    }

    #[test]
    fn amazon_bedrock_embedding_models_prepare_requests_and_parse_responses() {
        let (requests, transport) = capture_transport(vec![ProviderApiResponse::text(
            200,
            "OK",
            json!({
                "embedding": [0.1, 0.2, 0.3],
                "inputTextTokenCount": 4
            })
            .to_string(),
        )]);
        let provider = create_amazon_bedrock(
            AmazonBedrockProviderSettings::new()
                .with_api_key("key")
                .with_base_url("https://bedrock.test"),
        )
        .with_transport(transport);
        let model = provider.embedding_model("amazon.titan-embed-text-v2:0");

        let result = block_on(model.do_embed(
            EmbeddingModelCallOptions::new(vec!["embed me".to_string()]).with_provider_options(
                provider_options(json!({
                    "amazonBedrock": {
                        "dimensions": 512,
                        "normalize": true
                    }
                })),
            ),
        ));

        assert_eq!(result.embeddings, vec![vec![0.1, 0.2, 0.3]]);
        assert_eq!(result.usage.expect("usage").tokens, 4);
        let request = captured_requests(&requests)
            .first()
            .cloned()
            .expect("request");
        let body = request_json(&request);
        assert_eq!(body["inputText"], json!("embed me"));
        assert_eq!(body["dimensions"], json!(512));
        assert_eq!(body["normalize"], json!(true));

        let nova = provider.embedding_model("amazon.nova-embed-text-v1:0");
        let nova_body = nova.request_body(
            &["nova".to_string()],
            Some(&provider_options(json!({
                "amazonBedrock": {
                    "embeddingPurpose": "DOCUMENT",
                    "embeddingDimension": 256,
                    "truncate": "START"
                }
            }))),
        );
        assert_eq!(
            nova_body["singleEmbeddingParams"]["embeddingDimension"],
            json!(256)
        );
        assert_eq!(
            nova_body["singleEmbeddingParams"]["text"]["truncationMode"],
            json!("START")
        );
    }

    #[test]
    fn amazon_bedrock_image_model_prepares_generation_editing_and_response_cases() {
        let (requests, transport) = capture_transport(vec![ProviderApiResponse::text(
            200,
            "OK",
            json!({ "images": ["aW1hZ2U="] }).to_string(),
        )]);
        let provider = create_amazon_bedrock(
            AmazonBedrockProviderSettings::new()
                .with_api_key("key")
                .with_base_url("https://bedrock.test"),
        )
        .with_transport(transport)
        .with_current_date(fixed_date);
        let model = provider.image_model("amazon.nova-canvas-v1:0");

        let result = block_on(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("paint")
                    .with_size("512x512")
                    .with_seed(42)
                    .with_provider_options(provider_options(json!({
                        "amazonBedrock": {
                            "quality": "premium",
                            "cfgScale": 8,
                            "negativeText": "blur"
                        }
                    }))),
            ),
        );
        assert_eq!(
            result.images,
            vec![FileDataContent::Base64("aW1hZ2U=".to_string())]
        );
        let request = captured_requests(&requests)
            .first()
            .cloned()
            .expect("request");
        let body = request_json(&request);
        assert_eq!(body["taskType"], json!("TEXT_IMAGE"));
        assert_eq!(body["textToImageParams"]["text"], json!("paint"));
        assert_eq!(body["imageGenerationConfig"]["width"], json!(512));
        assert_eq!(body["imageGenerationConfig"]["quality"], json!("premium"));

        let edit_body = model
            .request_body(
                &ImageModelCallOptions::new(1)
                    .with_prompt("edit")
                    .with_files(vec![ImageModelFile::file(
                        "image/png",
                        FileDataContent::Bytes(vec![1, 2, 3]),
                    )])
                    .with_provider_options(provider_options(json!({
                        "amazonBedrock": {
                            "taskType": "INPAINTING",
                            "maskPrompt": "sky"
                        }
                    }))),
            )
            .expect("edit body")
            .0;
        assert_eq!(edit_body["taskType"], json!("INPAINTING"));
        assert_eq!(edit_body["inPaintingParams"]["maskPrompt"], json!("sky"));
        assert_eq!(edit_body["inPaintingParams"]["image"], json!("AQID"));
    }

    #[test]
    fn amazon_bedrock_reranking_model_prepares_requests_and_parses_responses() {
        let (requests, transport) = capture_transport(vec![ProviderApiResponse::text(
            200,
            "OK",
            json!({
                "results": [
                    { "index": 1, "relevanceScore": 0.95 },
                    { "index": 0, "relevanceScore": 0.15 }
                ]
            })
            .to_string(),
        )]);
        let provider = create_amazon_bedrock(
            AmazonBedrockProviderSettings::new()
                .with_region("eu-central-1")
                .with_api_key("key")
                .with_base_url("https://agent.test"),
        )
        .with_transport(transport);
        let model = provider.reranking_model("amazon.rerank-v1:0");

        let result = block_on(
            model.do_rerank(
                RerankingModelCallOptions::new(
                    RerankingModelDocuments::text(vec!["alpha".to_string(), "beta".to_string()]),
                    "query",
                )
                .with_top_n(1)
                .with_provider_options(provider_options(json!({
                    "amazonBedrock": {
                        "nextToken": "next",
                        "additionalModelRequestFields": { "foo": "bar" }
                    }
                }))),
            ),
        );

        assert_eq!(result.ranking[0].index, 1);
        assert_eq!(result.ranking[0].relevance_score, 0.95);
        let request = captured_requests(&requests)
            .first()
            .cloned()
            .expect("request");
        assert_eq!(request.url, "https://agent.test/rerank");
        let body = request_json(&request);
        assert_eq!(body["nextToken"], json!("next"));
        assert_eq!(
            body["rerankingConfiguration"]["amazonBedrockRerankingConfiguration"]["numberOfResults"],
            json!(1)
        );
        assert_eq!(
            body["rerankingConfiguration"]["amazonBedrockRerankingConfiguration"]["modelConfiguration"]
                ["modelArn"],
            json!("arn:aws:bedrock:eu-central-1::foundation-model/amazon.rerank-v1:0")
        );
    }

    #[test]
    fn amazon_bedrock_event_stream_decoder_handles_fixtures_frames_and_errors() {
        let fixture = b"{\"contentBlockDelta\":{\"contentBlockIndex\":0,\"delta\":{\"text\":\"Hi\"}}}\nnot json\n";
        let events = decode_bedrock_event_stream(fixture);
        assert_eq!(events[0].event_type, "contentBlockDelta");
        assert_eq!(
            events[0].value["contentBlockDelta"]["delta"]["text"],
            json!("Hi")
        );
        assert_eq!(events[1].event_type, "parseError");
        assert!(events[1].error.is_some());

        let frame = smithy_frame(
            "contentBlockDelta",
            br#"{"contentBlockIndex":0,"delta":{"text":"Yo"},"p":"drop"}"#,
        );
        let events = decode_bedrock_event_stream(&frame);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].value,
            json!({ "contentBlockDelta": { "contentBlockIndex": 0, "delta": { "text": "Yo" } } })
        );
    }

    #[test]
    fn amazon_bedrock_anthropic_fetch_transforms_event_streams() {
        let (model, _) = model_with_responses(vec![stream_response()]);
        let streamed = block_on(
            AmazonBedrockAnthropicLanguageModel { inner: model }
                .do_stream(LanguageModelCallOptions::new(user_prompt("stream"))),
        );
        assert!(streamed.stream.iter().any(|part| {
            matches!(part, LanguageModelStreamPart::TextDelta(delta) if delta.delta == "Hel")
        }));
    }

    #[test]
    fn amazon_bedrock_anthropic_provider_transforms_tools_and_model_factories() {
        let transformed = AmazonBedrockAnthropicLanguageModel::transform_request_body(
            serde_json::from_value(json!({
                "model": "claude",
                "stream": true,
                "tool_choice": { "type": "auto" },
                "tools": [
                    { "type": "text_editor_20241022", "name": "old-editor" }
                ]
            }))
            .expect("body object"),
            &["custom-beta".to_string()],
        );

        assert!(transformed.get("model").is_none());
        assert!(transformed.get("stream").is_none());
        assert_eq!(
            transformed["anthropic_version"],
            json!("bedrock-2023-05-31")
        );
        assert_eq!(
            transformed["tools"][0]["type"],
            json!("text_editor_20250728")
        );
        assert_eq!(
            transformed["tools"][0]["name"],
            json!("str_replace_based_edit_tool")
        );
        assert!(
            transformed["anthropic_beta"]
                .as_array()
                .expect("betas")
                .contains(&json!("computer-use-2025-01-24"))
        );

        let provider = create_amazon_bedrock_anthropic(
            AmazonBedrockProviderSettings::new().with_api_key("key"),
        );
        let model = provider.messages("anthropic.claude-3-haiku-20240307-v1:0");
        assert_eq!(model.provider(), "bedrock.anthropic.messages");
        assert!(provider.embedding_model("embed").is_err());
        assert!(provider.image_model("image").is_err());
    }

    #[test]
    fn amazon_bedrock_mantle_provider_maps_openai_compatible_factories() {
        let provider = create_bedrock_mantle(
            AmazonBedrockProviderSettings::new()
                .with_region("ap-southeast-2")
                .with_api_key("key")
                .with_base_url("https://mantle.test/v1"),
        );

        let chat = provider.chat("openai.gpt-oss-120b-1:0");
        assert_eq!(chat.provider(), "bedrock-mantle.chat");
        assert_eq!(chat.model_id(), "openai.gpt-oss-120b-1:0");
        let responses = provider.responses("anthropic.claude-sonnet-4-5-20250929-v1:0");
        assert_eq!(responses.provider(), "bedrock-mantle.chat");
        assert!(provider.embedding_model("embed").is_err());
        assert!(provider.image_model("image").is_err());
    }

    #[test]
    fn amazon_bedrock_usage_conversion_maps_cache_and_missing_usage() {
        let usage = convert_amazon_bedrock_usage(Some(&json!({
            "inputTokens": 10,
            "outputTokens": 3,
            "cacheReadInputTokens": 4,
            "cacheWriteInputTokens": 5
        })));
        assert_eq!(usage.input_tokens.total, Some(19));
        assert_eq!(usage.input_tokens.no_cache, Some(10));
        assert_eq!(usage.input_tokens.cache_read, Some(4));
        assert_eq!(usage.input_tokens.cache_write, Some(5));
        assert_eq!(usage.output_tokens.total, Some(3));
        assert_eq!(
            convert_amazon_bedrock_usage(None),
            ai_sdk_rust::LanguageModelUsage::default()
        );
    }

    #[test]
    fn amazon_bedrock_mistral_tool_call_id_normalization_matches_upstream() {
        assert_eq!(
            normalize_tool_call_id("tool-call-id-with-dashes", true),
            "toolcalli"
        );
        assert_eq!(
            normalize_tool_call_id("tool-call-id-with-dashes", false),
            "tool-call-id-with-dashes"
        );
        assert_eq!(
            normalize_tool_call_id("abc_DEF-123456789", true),
            "abcDEF123"
        );
    }

    #[test]
    #[ignore = "requires live AWS Bedrock credentials and model access"]
    fn amazon_bedrock_live_provider_proof_is_credential_gated() {
        if env::var("AWS_ACCESS_KEY_ID").is_err() || env::var("AWS_SECRET_ACCESS_KEY").is_err() {
            return;
        }

        let provider = create_amazon_bedrock(AmazonBedrockProviderSettings::new());
        let model = provider.language_model("amazon.nova-lite-v1:0");
        let result =
            block_on(model.do_generate(LanguageModelCallOptions::new(user_prompt("Say ok."))));
        assert_ne!(result.finish_reason.unified, FinishReason::Error);
    }

    fn smithy_frame(event_type: &str, payload: &[u8]) -> Vec<u8> {
        let mut headers = Vec::new();
        push_smithy_string_header(&mut headers, ":message-type", "event");
        push_smithy_string_header(&mut headers, ":event-type", event_type);

        let total_length = 12 + headers.len() + payload.len() + 4;
        let mut frame = Vec::with_capacity(total_length);
        frame.extend_from_slice(&(total_length as u32).to_be_bytes());
        frame.extend_from_slice(&(headers.len() as u32).to_be_bytes());
        frame.extend_from_slice(&0_u32.to_be_bytes());
        frame.extend_from_slice(&headers);
        frame.extend_from_slice(payload);
        frame.extend_from_slice(&0_u32.to_be_bytes());
        frame
    }

    fn push_smithy_string_header(headers: &mut Vec<u8>, name: &str, value: &str) {
        headers.push(name.len() as u8);
        headers.extend_from_slice(name.as_bytes());
        headers.push(7);
        headers.extend_from_slice(&(value.len() as u16).to_be_bytes());
        headers.extend_from_slice(value.as_bytes());
    }
}
