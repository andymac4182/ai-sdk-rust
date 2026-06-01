//! Rust port of upstream `@ai-sdk/google-vertex`.
//!
//! The crate keeps Vertex-owned behavior here and only reuses shared
//! provider contracts/utilities. Google Gemini internals are still owned by
//! the sibling `ai-sdk-google` crate, so Vertex image/chat request planning is
//! represented locally where upstream `@ai-sdk/google-vertex` owns the wrapper.

use std::env;
use std::fmt;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use url::Url;

use ai_sdk_openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleCompletionLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel, OpenAICompatibleProvider,
    OpenAICompatibleProviderSettings, OpenAICompatibleTransport, create_openai_compatible,
};
use ai_sdk_provider::embedding_model::{
    EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResponse, EmbeddingModelResult,
    EmbeddingModelUsage,
};
use ai_sdk_provider::file_data::FileDataContent;
use ai_sdk_provider::headers::Headers;
use ai_sdk_provider::image_model::{
    ImageModel, ImageModelCallOptions, ImageModelFile, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResult, ImageModelUsage,
};
use ai_sdk_provider::json::{JsonArray, JsonObject, JsonValue};
use ai_sdk_provider::language_model::{
    FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelFinishReason,
    LanguageModelGenerateResult, LanguageModelRequest, LanguageModelStreamPart,
    LanguageModelStreamResult, LanguageModelSupportedUrls, LanguageModelUsage, OutputTokenUsage,
};
use ai_sdk_provider::provider::{
    ModelType, NoSuchModelError, Provider, ProviderMetadata, ProviderOptions,
    ProviderWithVideoModel,
};
use ai_sdk_provider::video_model::{
    VideoModel, VideoModelCallOptions, VideoModelFile, VideoModelResponse, VideoModelResult,
    VideoModelVideoData,
};
use ai_sdk_provider::warning::Warning;
use ai_sdk_provider_utils::{
    FetchErrorInfo, PostJsonToApiOptions, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, convert_to_base64, with_user_agent_suffix,
    without_trailing_slash,
};

/// Upstream package covered by this crate.
pub const UPSTREAM_PACKAGE: &str = "@ai-sdk/google-vertex";

/// Crate version used in Vertex user-agent suffixes.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Upstream package directory in `vercel/ai`.
pub const UPSTREAM_PACKAGE_DIR: &str = "packages/google-vertex";

/// Upstream commit used for the checked-in AI-01 inventory.
pub const UPSTREAM_COMMIT: &str = "ab6d66482d31afe15f4973a51c5f7cfa09c92ea6";

/// Checked-in row-level inventory document for this package.
pub const INVENTORY_DOCUMENT: &str = "docs/ai-foundational-provider-inventory.md";

/// Current upstream test files under `packages/google-vertex/src`.
pub const UPSTREAM_TEST_FILES: usize = 17;

/// Current detected upstream `it`/`test` cases under `packages/google-vertex/src`.
pub const UPSTREAM_TEST_CASES: usize = 204;

/// Current explicit TypeScript type-system exceptions.
pub const TYPE_SYSTEM_IMPOSSIBLE_CASES: usize = 0;

/// Current explicit JavaScript runtime exceptions.
pub const JS_ONLY_DOCUMENTED_CASES: usize = 3;

/// Current portable cases mapped to named Rust tests.
pub const PORTABLE_MAPPED_CASES: usize =
    UPSTREAM_TEST_CASES - TYPE_SYSTEM_IMPOSSIBLE_CASES - JS_ONLY_DOCUMENTED_CASES;

/// Current portable cases still requiring named Rust tests.
pub const PORTABLE_UNMAPPED_CASES: usize = 0;

const EXPRESS_MODE_BASE_URL: &str = "https://aiplatform.googleapis.com/v1/publishers/google";
const GOOGLE_CLOUD_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// Future returned by Vertex HTTP transports.
pub type GoogleVertexTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// Dependency-injected HTTP transport used by Vertex models.
pub type GoogleVertexTransport =
    Arc<dyn Fn(ProviderApiRequest) -> GoogleVertexTransportFuture + Send + Sync>;

/// Future returned by async header resolvers.
pub type GoogleVertexHeadersFuture =
    Pin<Box<dyn Future<Output = Result<Headers, GoogleVertexError>> + Send>>;

/// Async provider header resolver.
pub type GoogleVertexHeaders = Arc<dyn Fn() -> GoogleVertexHeadersFuture + Send + Sync>;

/// Future returned by auth token generators.
pub type GoogleVertexAuthTokenFuture =
    Pin<Box<dyn Future<Output = Result<Option<String>, GoogleVertexError>> + Send>>;

/// Async auth token generator. `None` is preserved for parity with
/// `google-auth-library`'s nullable access-token result.
pub type GoogleVertexAuthTokenGenerator =
    Arc<dyn Fn() -> GoogleVertexAuthTokenFuture + Send + Sync>;

type GoogleVertexDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type GoogleVertexIdGenerator = Arc<dyn Fn() -> String + Send + Sync>;

/// Error returned by Vertex helpers that can fail before provider response
/// mapping.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoogleVertexError {
    name: String,
    message: String,
}

impl GoogleVertexError {
    /// Creates a Google Vertex error.
    pub fn new(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message: message.into(),
        }
    }

    /// Returns the error name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the human-readable message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for GoogleVertexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GoogleVertexError {}

/// Service-account credentials used by the edge-compatible OAuth flow.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleCredentials {
    /// Google Cloud service-account client email.
    pub client_email: String,

    /// PKCS#8 PEM private key.
    pub private_key: String,

    /// Optional private key id used as the JWT `kid` header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_id: Option<String>,
}

impl GoogleCredentials {
    /// Creates service-account credentials.
    pub fn new(client_email: impl Into<String>, private_key: impl Into<String>) -> Self {
        Self {
            client_email: client_email.into(),
            private_key: private_key.into(),
            private_key_id: None,
        }
    }

    /// Sets the optional private key id.
    pub fn with_private_key_id(mut self, private_key_id: impl Into<String>) -> Self {
        self.private_key_id = Some(private_key_id.into());
        self
    }

    /// Loads credentials from `GOOGLE_CLIENT_EMAIL`, `GOOGLE_PRIVATE_KEY`, and
    /// optional `GOOGLE_PRIVATE_KEY_ID`.
    pub fn from_env() -> Result<Self, GoogleVertexError> {
        let client_email = env::var("GOOGLE_CLIENT_EMAIL").map_err(|_| {
            GoogleVertexError::new(
                "LoadSettingError",
                "Failed to load Google credentials: Google client email is missing; set GOOGLE_CLIENT_EMAIL",
            )
        })?;
        let private_key = env::var("GOOGLE_PRIVATE_KEY").map_err(|_| {
            GoogleVertexError::new(
                "LoadSettingError",
                "Failed to load Google credentials: Google private key is missing; set GOOGLE_PRIVATE_KEY",
            )
        })?;

        Ok(Self {
            client_email,
            private_key: normalize_pem_newlines(&private_key),
            private_key_id: env::var("GOOGLE_PRIVATE_KEY_ID").ok(),
        })
    }
}

/// Node-style Google auth options.
///
/// Rust does not have `google-auth-library`; the portable behavior is the
/// reusable token-generator boundary and its per-provider construction.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GoogleAuthOptions {
    /// Explicit access token for deterministic tests or callers that already
    /// integrate with a Google auth crate.
    pub access_token: Option<String>,

    /// OAuth scopes requested by the upstream package.
    pub scopes: Vec<String>,

    /// Optional quota project marker retained for parity tests.
    pub quota_project_id: Option<String>,
}

impl GoogleAuthOptions {
    /// Creates default Google auth options with the Vertex cloud-platform scope.
    pub fn new() -> Self {
        Self {
            access_token: None,
            scopes: vec![GOOGLE_CLOUD_SCOPE.to_string()],
            quota_project_id: None,
        }
    }

    /// Sets an explicit access token.
    pub fn with_access_token(mut self, access_token: impl Into<String>) -> Self {
        self.access_token = Some(access_token.into());
        self
    }

    /// Sets an optional quota project id.
    pub fn with_quota_project_id(mut self, quota_project_id: impl Into<String>) -> Self {
        self.quota_project_id = Some(quota_project_id.into());
        self
    }
}

/// Creates a node-style auth-token generator.
pub fn create_auth_token_generator(options: GoogleAuthOptions) -> GoogleVertexAuthTokenGenerator {
    Arc::new(move || {
        let token = options
            .access_token
            .clone()
            .or_else(|| env::var("GOOGLE_VERTEX_ACCESS_TOKEN").ok());
        Box::pin(ready(Ok(token)))
    })
}

/// Constructs the RS256 service-account JWT used by the edge auth flow.
pub fn build_edge_auth_jwt(
    credentials: &GoogleCredentials,
    issued_at_unix_seconds: i64,
) -> Result<String, GoogleVertexError> {
    #[derive(Serialize)]
    struct Claims<'a> {
        iss: &'a str,
        scope: &'a str,
        aud: &'a str,
        exp: i64,
        iat: i64,
    }

    if credentials.client_email.trim().is_empty() {
        return Err(GoogleVertexError::new(
            "GoogleVertexAuthError",
            "Google client email must not be empty",
        ));
    }
    if credentials.private_key.trim().is_empty() {
        return Err(GoogleVertexError::new(
            "GoogleVertexAuthError",
            "Google private key must not be empty",
        ));
    }

    let mut header = Header::new(Algorithm::RS256);
    header.typ = Some("JWT".to_string());
    header.kid = credentials.private_key_id.clone();

    let claims = Claims {
        iss: &credentials.client_email,
        scope: GOOGLE_CLOUD_SCOPE,
        aud: OAUTH_TOKEN_URL,
        exp: issued_at_unix_seconds + 3600,
        iat: issued_at_unix_seconds,
    };
    let key =
        EncodingKey::from_rsa_pem(normalize_pem_newlines(&credentials.private_key).as_bytes())
            .map_err(|error| {
                GoogleVertexError::new(
                    "GoogleVertexAuthError",
                    format!("Failed to import Google private key: {error}"),
                )
            })?;

    encode(&header, &claims, &key).map_err(|error| {
        GoogleVertexError::new(
            "GoogleVertexAuthError",
            format!("Failed to build Google auth JWT: {error}"),
        )
    })
}

/// Generates an OAuth access token via the edge-compatible service-account
/// JWT flow.
pub async fn generate_edge_auth_token(
    credentials: Option<GoogleCredentials>,
    transport: GoogleVertexTransport,
    now_unix_seconds: i64,
) -> Result<String, GoogleVertexError> {
    let credentials = match credentials {
        Some(credentials) => credentials,
        None => GoogleCredentials::from_env()?,
    };
    let jwt = build_edge_auth_jwt(&credentials, now_unix_seconds)?;
    let headers = with_user_agent_suffix(
        Some([(
            "Content-Type".to_string(),
            Some("application/x-www-form-urlencoded".to_string()),
        )]),
        [format!("ai-sdk/google-vertex/{}", crate::VERSION)],
    );
    let body =
        format!("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={jwt}");
    let request = ProviderApiRequest::post(
        OAUTH_TOKEN_URL,
        headers,
        ProviderApiRequestBody::text(body),
        json!({}),
    );
    let response = transport(request)
        .await
        .map_err(|error| GoogleVertexError::new("FetchError", error.message().to_string()))?;

    if !response.is_success_status() {
        return Err(GoogleVertexError::new(
            "GoogleVertexAuthError",
            format!("Token request failed: {}", response.status_text),
        ));
    }

    let body = response.text_body().ok_or_else(|| {
        GoogleVertexError::new("GoogleVertexAuthError", "Token response body is empty")
    })?;
    let parsed: JsonValue = serde_json::from_str(body).map_err(|error| {
        GoogleVertexError::new(
            "GoogleVertexAuthError",
            format!("Token response is not valid JSON: {error}"),
        )
    })?;
    parsed
        .get("access_token")
        .and_then(JsonValue::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| {
            GoogleVertexError::new(
                "GoogleVertexAuthError",
                "Token response did not include access_token",
            )
        })
}

/// Creates a static header resolver.
pub fn static_headers(headers: Headers) -> GoogleVertexHeaders {
    Arc::new(move || {
        let headers = headers.clone();
        Box::pin(ready(Ok(headers)))
    })
}

/// Creates an auth header resolver that mirrors upstream's spread ordering.
///
/// When `user_headers_override_authorization` is true, custom headers are
/// merged after the generated token. That is what the base Vertex and
/// Anthropic wrappers do. MaaS/xAI auth fetch wrappers set it to false because
/// their generated Authorization header is merged last upstream.
pub fn auth_headers(
    token_generator: GoogleVertexAuthTokenGenerator,
    user_headers: Option<GoogleVertexHeaders>,
    user_headers_override_authorization: bool,
) -> GoogleVertexHeaders {
    Arc::new(move || {
        let token_generator = Arc::clone(&token_generator);
        let user_headers = user_headers.clone();
        Box::pin(async move {
            let token = token_generator().await?;
            let mut headers = Headers::new();
            if user_headers_override_authorization && let Some(token) = token.clone() {
                headers.insert("Authorization".to_string(), format!("Bearer {token}"));
            }
            if let Some(user_headers) = user_headers {
                headers.extend(user_headers().await?);
            }
            if !user_headers_override_authorization && let Some(token) = token {
                headers.insert("Authorization".to_string(), format!("Bearer {token}"));
            }
            Ok(headers)
        })
    })
}

/// Settings shared by the base Google Vertex provider.
#[derive(Clone, Default)]
pub struct GoogleVertexProviderSettings {
    /// Express-mode API key.
    pub api_key: Option<String>,
    /// Google Cloud location/region.
    pub location: Option<String>,
    /// Google Cloud project id.
    pub project: Option<String>,
    /// Optional base URL override.
    pub base_url: Option<String>,
    /// Resolved provider headers.
    pub headers: Option<GoogleVertexHeaders>,
    /// Optional request id generator.
    pub generate_id: Option<GoogleVertexIdGenerator>,
    /// Injected HTTP transport.
    pub transport: Option<GoogleVertexTransport>,
    /// Injected response timestamp provider.
    pub current_date: Option<GoogleVertexDateProvider>,
}

impl GoogleVertexProviderSettings {
    /// Creates default provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets express-mode API key authentication.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets Google Cloud location.
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Sets Google Cloud project id.
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Sets a base URL override.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets static headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(static_headers(headers));
        self
    }

    /// Sets an async header resolver.
    pub fn with_header_resolver(mut self, headers: GoogleVertexHeaders) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Sets an HTTP transport.
    pub fn with_transport(mut self, transport: GoogleVertexTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Sets the current-date provider.
    pub fn with_current_date<F>(mut self, current_date: F) -> Self
    where
        F: Fn() -> OffsetDateTime + Send + Sync + 'static,
    {
        self.current_date = Some(Arc::new(current_date));
        self
    }

    /// Sets the request id generator.
    pub fn with_generate_id<F>(mut self, generate_id: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        self.generate_id = Some(Arc::new(generate_id));
        self
    }
}

/// Config shared by Vertex models.
#[derive(Clone)]
pub struct GoogleVertexConfig {
    provider: String,
    base_url: String,
    headers: Option<GoogleVertexHeaders>,
    transport: GoogleVertexTransport,
    current_date: GoogleVertexDateProvider,
    generate_id: GoogleVertexIdGenerator,
    api_key: Option<String>,
}

impl GoogleVertexConfig {
    /// Creates a model config.
    pub fn new(provider: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            base_url: trim_trailing_slash(base_url.into()),
            headers: None,
            transport: default_vertex_transport(),
            current_date: Arc::new(OffsetDateTime::now_utc),
            generate_id: Arc::new(default_generate_id),
            api_key: None,
        }
    }

    /// Sets static headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(static_headers(headers));
        self
    }

    /// Sets an async header resolver.
    pub fn with_header_resolver(mut self, headers: GoogleVertexHeaders) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Sets a transport.
    pub fn with_transport(mut self, transport: GoogleVertexTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Sets the current-date provider.
    pub fn with_current_date<F>(mut self, current_date: F) -> Self
    where
        F: Fn() -> OffsetDateTime + Send + Sync + 'static,
    {
        self.current_date = Arc::new(current_date);
        self
    }

    /// Sets the id generator.
    pub fn with_generate_id<F>(mut self, generate_id: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        self.generate_id = Arc::new(generate_id);
        self
    }

    /// Sets express-mode API key authentication.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Returns provider id.
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Returns base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

/// Google Vertex base provider.
#[derive(Clone)]
pub struct GoogleVertexProvider {
    settings: GoogleVertexProviderSettings,
}

impl GoogleVertexProvider {
    /// Creates the base provider without adding auth headers.
    pub fn base(settings: GoogleVertexProviderSettings) -> Self {
        Self { settings }
    }

    /// Creates the node-style provider. When no API key is present this wraps
    /// requests with a bearer token generated by `google-auth-library` parity
    /// boundary [`create_auth_token_generator`].
    pub fn node(
        settings: GoogleVertexProviderSettings,
        google_auth_options: GoogleAuthOptions,
    ) -> Self {
        if load_optional_setting(settings.api_key.as_deref(), "GOOGLE_VERTEX_API_KEY").is_some() {
            return Self::base(settings);
        }

        let token_generator = create_auth_token_generator(google_auth_options);
        Self::with_auth_token(settings, token_generator, true)
    }

    /// Creates the edge-style provider from service-account credentials.
    pub fn edge(
        settings: GoogleVertexProviderSettings,
        credentials: Option<GoogleCredentials>,
    ) -> Self {
        if load_optional_setting(settings.api_key.as_deref(), "GOOGLE_VERTEX_API_KEY").is_some() {
            return Self::base(settings);
        }

        let transport = settings
            .transport
            .clone()
            .unwrap_or_else(default_vertex_transport);
        let token_generator: GoogleVertexAuthTokenGenerator = Arc::new(move || {
            let transport = Arc::clone(&transport);
            let credentials = credentials.clone();
            Box::pin(async move {
                generate_edge_auth_token(
                    credentials,
                    transport,
                    OffsetDateTime::now_utc().unix_timestamp(),
                )
                .await
                .map(Some)
            })
        });
        Self::with_auth_token(settings, token_generator, true)
    }

    /// Creates a provider that wraps headers with a generated bearer token.
    pub fn with_auth_token(
        mut settings: GoogleVertexProviderSettings,
        token_generator: GoogleVertexAuthTokenGenerator,
        user_headers_override_authorization: bool,
    ) -> Self {
        let user_headers = settings.headers.take();
        settings.headers = Some(auth_headers(
            token_generator,
            user_headers,
            user_headers_override_authorization,
        ));
        Self::base(settings)
    }

    /// Creates a language model.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<GoogleVertexLanguageModel, GoogleVertexError> {
        self.config("chat")
            .map(|config| GoogleVertexLanguageModel::new(model_id, config))
    }

    /// Upstream callable-provider alias.
    pub fn model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<GoogleVertexLanguageModel, GoogleVertexError> {
        self.language_model(model_id)
    }

    /// Creates an embedding model.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<GoogleVertexEmbeddingModel, GoogleVertexError> {
        self.config("embedding")
            .map(|config| GoogleVertexEmbeddingModel::new(model_id, config))
    }

    /// Deprecated upstream alias for [`GoogleVertexProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<GoogleVertexEmbeddingModel, GoogleVertexError> {
        self.embedding_model(model_id)
    }

    /// Creates an image model.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<GoogleVertexImageModel, GoogleVertexError> {
        self.config("image")
            .map(|config| GoogleVertexImageModel::new(model_id, config))
    }

    /// Upstream alias for [`GoogleVertexProvider::image_model`].
    pub fn image(
        &self,
        model_id: impl Into<String>,
    ) -> Result<GoogleVertexImageModel, GoogleVertexError> {
        self.image_model(model_id)
    }

    /// Creates a video model.
    pub fn video_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<GoogleVertexVideoModel, GoogleVertexError> {
        self.config("video")
            .map(|config| GoogleVertexVideoModel::new(model_id, config))
    }

    /// Upstream alias for [`GoogleVertexProvider::video_model`].
    pub fn video(
        &self,
        model_id: impl Into<String>,
    ) -> Result<GoogleVertexVideoModel, GoogleVertexError> {
        self.video_model(model_id)
    }

    /// Returns tools exposed by upstream `googleVertex.tools`.
    pub fn tools(&self) -> GoogleVertexTools {
        GoogleVertexTools::new()
    }

    fn config(&self, name: &str) -> Result<GoogleVertexConfig, GoogleVertexError> {
        let api_key =
            load_optional_setting(self.settings.api_key.as_deref(), "GOOGLE_VERTEX_API_KEY");
        let base_url = self.base_url(api_key.as_deref())?;
        let mut config = GoogleVertexConfig::new(format!("google.vertex.{name}"), base_url)
            .with_transport(
                self.settings
                    .transport
                    .clone()
                    .unwrap_or_else(default_vertex_transport),
            );

        if let Some(headers) = self.settings.headers.clone() {
            config = config.with_header_resolver(headers);
        }
        if let Some(current_date) = self.settings.current_date.clone() {
            config.current_date = current_date;
        }
        if let Some(generate_id) = self.settings.generate_id.clone() {
            config.generate_id = generate_id;
        }
        if let Some(api_key) = api_key {
            config.api_key = Some(api_key);
        }

        Ok(config)
    }

    fn base_url(&self, api_key: Option<&str>) -> Result<String, GoogleVertexError> {
        if api_key.is_some() {
            return Ok(without_trailing_slash(self.settings.base_url.as_deref())
                .unwrap_or(EXPRESS_MODE_BASE_URL)
                .to_string());
        }

        if let Some(base_url) = without_trailing_slash(self.settings.base_url.as_deref()) {
            return Ok(base_url.to_string());
        }

        let location = load_required_setting(
            self.settings.location.as_deref(),
            "location",
            "GOOGLE_VERTEX_LOCATION",
            "Google Vertex location",
        )?;
        let project = load_required_setting(
            self.settings.project.as_deref(),
            "project",
            "GOOGLE_VERTEX_PROJECT",
            "Google Vertex project",
        )?;
        let host_prefix = if location == "global" {
            String::new()
        } else {
            format!("{location}-")
        };

        Ok(format!(
            "https://{host_prefix}aiplatform.googleapis.com/v1beta1/projects/{project}/locations/{location}/publishers/google"
        ))
    }
}

impl Provider for GoogleVertexProvider {
    type LanguageModel = GoogleVertexLanguageModel;
    type EmbeddingModel = GoogleVertexEmbeddingModel;
    type ImageModel = GoogleVertexImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        GoogleVertexProvider::language_model(self, model_id).map_err(|error| {
            NoSuchModelError::with_message(model_id, ModelType::LanguageModel, error.to_string())
        })
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        GoogleVertexProvider::embedding_model(self, model_id).map_err(|error| {
            NoSuchModelError::with_message(model_id, ModelType::EmbeddingModel, error.to_string())
        })
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        GoogleVertexProvider::image_model(self, model_id).map_err(|error| {
            NoSuchModelError::with_message(model_id, ModelType::ImageModel, error.to_string())
        })
    }
}

impl ProviderWithVideoModel for GoogleVertexProvider {
    type VideoModel = GoogleVertexVideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        GoogleVertexProvider::video_model(self, model_id).map_err(|error| {
            NoSuchModelError::with_message(model_id, ModelType::VideoModel, error.to_string())
        })
    }
}

/// Creates the base Google Vertex provider.
pub fn create_google_vertex_base(settings: GoogleVertexProviderSettings) -> GoogleVertexProvider {
    GoogleVertexProvider::base(settings)
}

/// Creates the node-style Google Vertex provider.
pub fn create_google_vertex(settings: GoogleVertexProviderSettings) -> GoogleVertexProvider {
    GoogleVertexProvider::node(settings, GoogleAuthOptions::new())
}

/// Creates the edge-style Google Vertex provider.
pub fn create_google_vertex_edge(
    settings: GoogleVertexProviderSettings,
    credentials: Option<GoogleCredentials>,
) -> GoogleVertexProvider {
    GoogleVertexProvider::edge(settings, credentials)
}

/// Upstream deprecated alias.
pub fn create_vertex(settings: GoogleVertexProviderSettings) -> GoogleVertexProvider {
    create_google_vertex(settings)
}

/// Vertex-specific Google tools exposed by upstream `googleVertex.tools`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoogleVertexTools {
    names: Vec<&'static str>,
}

impl GoogleVertexTools {
    /// Creates the Vertex tool set.
    pub fn new() -> Self {
        Self {
            names: vec![
                "googleSearch",
                "enterpriseWebSearch",
                "googleMaps",
                "urlContext",
                "fileSearch",
                "codeExecution",
                "vertexRagStore",
            ],
        }
    }

    /// Returns the upstream tool names.
    pub fn names(&self) -> &[&'static str] {
        &self.names
    }
}

impl Default for GoogleVertexTools {
    fn default() -> Self {
        Self::new()
    }
}

/// Minimal Vertex chat model wrapper.
#[derive(Clone)]
pub struct GoogleVertexLanguageModel {
    model_id: String,
    config: GoogleVertexConfig,
}

impl GoogleVertexLanguageModel {
    /// Creates a language model.
    pub fn new(model_id: impl Into<String>, config: GoogleVertexConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Returns model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns provider id.
    pub fn provider(&self) -> &str {
        self.config.provider()
    }

    /// Returns base URL.
    pub fn base_url(&self) -> &str {
        self.config.base_url()
    }

    /// URL for Vertex Gemini content generation.
    pub fn generate_content_url(&self) -> String {
        format!(
            "{}/models/{}:generateContent",
            self.base_url(),
            self.model_id
        )
    }
}

impl LanguageModel for GoogleVertexLanguageModel {
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
        self.provider()
    }

    fn model_id(&self) -> &str {
        self.model_id()
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(LanguageModelSupportedUrls::from([(
            "*".to_string(),
            vec!["^https?://.*$".to_string(), "^gs://.*$".to_string()],
        )]))
    }

    fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        let model_id = self.model_id.clone();
        let provider = self.config.provider.clone();
        Box::pin(async move {
            language_error_result(
                &provider,
                "Google Vertex chat generation is owned by the sibling Google provider port",
                json!({ "model": model_id }),
            )
        })
    }

    fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(async { LanguageModelStreamResult::new(Vec::new()) })
    }
}

/// Google Vertex embedding model options.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleVertexEmbeddingModelOptions {
    /// Optional reduced output dimensionality.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_dimensionality: Option<u64>,
    /// Optional Vertex embedding task type.
    #[serde(default, rename = "taskType", skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    /// Optional retrieval document title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Whether Vertex should auto truncate long inputs.
    #[serde(
        default,
        rename = "autoTruncate",
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_truncate: Option<bool>,
}

/// Google Vertex embedding model.
#[derive(Clone)]
pub struct GoogleVertexEmbeddingModel {
    model_id: String,
    config: GoogleVertexConfig,
}

impl GoogleVertexEmbeddingModel {
    /// Creates an embedding model.
    pub fn new(model_id: impl Into<String>, config: GoogleVertexConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Returns model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns provider id.
    pub fn provider(&self) -> &str {
        self.config.provider()
    }

    /// Returns base URL.
    pub fn base_url(&self) -> &str {
        self.config.base_url()
    }

    /// Builds the upstream Vertex embedding request body.
    pub fn request_body(&self, options: &EmbeddingModelCallOptions) -> JsonValue {
        let google_options = embedding_options(options.provider_options.as_ref());
        let instances = options
            .values
            .iter()
            .map(|value| {
                let mut instance = JsonObject::new();
                instance.insert("content".to_string(), json!(value));
                if let Some(task_type) = google_options
                    .as_ref()
                    .and_then(|options| options.task_type.as_ref())
                {
                    instance.insert("task_type".to_string(), json!(task_type));
                }
                if let Some(title) = google_options
                    .as_ref()
                    .and_then(|options| options.title.as_ref())
                {
                    instance.insert("title".to_string(), json!(title));
                }
                JsonValue::Object(instance)
            })
            .collect::<JsonArray>();
        let mut parameters = JsonObject::new();
        if let Some(output_dimensionality) = google_options
            .as_ref()
            .and_then(|options| options.output_dimensionality)
        {
            parameters.insert(
                "outputDimensionality".to_string(),
                json!(output_dimensionality),
            );
        }
        if let Some(auto_truncate) = google_options
            .as_ref()
            .and_then(|options| options.auto_truncate)
        {
            parameters.insert("autoTruncate".to_string(), json!(auto_truncate));
        }

        json!({
            "instances": instances,
            "parameters": parameters,
        })
    }

    /// Builds the predict URL.
    pub fn predict_url(&self) -> String {
        format!("{}/models/{}:predict", self.base_url(), self.model_id)
    }

    async fn embed_result(&self, options: EmbeddingModelCallOptions) -> EmbeddingModelResult {
        if options.values.len() > 2048 {
            return EmbeddingModelResult::new(Vec::new()).with_provider_metadata(
                provider_error_metadata(
                    "googleVertex",
                    format!(
                        "Too many values for a single embedding call. The {} model \"{}\" can only embed up to 2048 values per call, but {} values were provided.",
                        self.provider(),
                        self.model_id,
                        options.values.len()
                    ),
                ),
            );
        }

        let request_body = self.request_body(&options);
        match post_vertex_json(
            &self.config,
            self.predict_url(),
            request_body.clone(),
            options.headers,
            options.abort_signal,
        )
        .await
        {
            Ok(response) => embedding_result_from_response(response),
            Err(error) => EmbeddingModelResult::new(Vec::new())
                .with_response(EmbeddingModelResponse::new().with_body(request_body))
                .with_provider_metadata(provider_error_metadata("googleVertex", error.to_string())),
        }
    }
}

impl EmbeddingModel for GoogleVertexEmbeddingModel {
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
        self.provider()
    }

    fn model_id(&self) -> &str {
        self.model_id()
    }

    fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_> {
        ready(Some(2048))
    }

    fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
        ready(true)
    }

    fn do_embed(&self, options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
        Box::pin(self.embed_result(options))
    }
}

/// Google Vertex image model.
#[derive(Clone)]
pub struct GoogleVertexImageModel {
    model_id: String,
    config: GoogleVertexConfig,
}

impl GoogleVertexImageModel {
    /// Creates an image model.
    pub fn new(model_id: impl Into<String>, config: GoogleVertexConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Returns model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns provider id.
    pub fn provider(&self) -> &str {
        self.config.provider()
    }

    /// Returns base URL.
    pub fn base_url(&self) -> &str {
        self.config.base_url()
    }

    /// Returns whether this is a Gemini image model.
    pub fn is_gemini_model(&self) -> bool {
        self.model_id.starts_with("gemini-")
    }

    /// Builds a Vertex Imagen request body.
    pub fn imagen_request_body(&self, options: &ImageModelCallOptions) -> JsonValue {
        let provider_options = image_options(&options.provider_options);
        let edit_options = provider_options
            .as_ref()
            .and_then(|options| options.get("edit"))
            .and_then(JsonValue::as_object);
        let mut passthrough = provider_options.clone().unwrap_or_default();
        passthrough.remove("edit");

        let mut parameters = JsonObject::new();
        parameters.insert("sampleCount".to_string(), json!(options.n));
        if let Some(aspect_ratio) = options.aspect_ratio.as_ref() {
            parameters.insert("aspectRatio".to_string(), json!(aspect_ratio));
        }
        if let Some(seed) = options.seed {
            parameters.insert("seed".to_string(), json!(seed));
        }
        for (key, value) in passthrough {
            parameters.insert(key, value);
        }

        let is_edit = options
            .files
            .as_ref()
            .is_some_and(|files| !files.is_empty());
        if is_edit {
            parameters.insert(
                "editMode".to_string(),
                edit_options
                    .and_then(|options| options.get("mode"))
                    .cloned()
                    .unwrap_or_else(|| json!("EDIT_MODE_INPAINT_INSERTION")),
            );
            if let Some(base_steps) = edit_options.and_then(|options| options.get("baseSteps")) {
                parameters.insert("editConfig".to_string(), json!({ "baseSteps": base_steps }));
            }

            let mut reference_images = Vec::new();
            if let Some(files) = options.files.as_ref() {
                for (index, file) in files.iter().enumerate() {
                    reference_images.push(json!({
                        "referenceType": "REFERENCE_TYPE_RAW",
                        "referenceId": index + 1,
                        "referenceImage": {
                            "bytesBase64Encoded": image_file_base64(file),
                        },
                    }));
                }
                if let Some(mask) = options.mask.as_ref() {
                    let mask_mode = edit_options
                        .and_then(|options| options.get("maskMode"))
                        .cloned()
                        .unwrap_or_else(|| json!("MASK_MODE_USER_PROVIDED"));
                    let mut mask_image = JsonObject::new();
                    mask_image.insert("referenceType".to_string(), json!("REFERENCE_TYPE_MASK"));
                    mask_image.insert("referenceId".to_string(), json!(files.len() + 1));
                    mask_image.insert(
                        "referenceImage".to_string(),
                        json!({ "bytesBase64Encoded": image_file_base64(mask) }),
                    );
                    let mut mask_config = JsonObject::new();
                    mask_config.insert("maskMode".to_string(), mask_mode);
                    if let Some(mask_dilation) =
                        edit_options.and_then(|options| options.get("maskDilation"))
                    {
                        mask_config.insert("dilation".to_string(), mask_dilation.clone());
                    }
                    mask_image.insert(
                        "maskImageConfig".to_string(),
                        JsonValue::Object(mask_config),
                    );
                    reference_images.push(JsonValue::Object(mask_image));
                }
            }

            json!({
                "instances": [{
                    "prompt": options.prompt,
                    "referenceImages": reference_images,
                }],
                "parameters": parameters,
            })
        } else {
            json!({
                "instances": [{ "prompt": options.prompt }],
                "parameters": parameters,
            })
        }
    }

    /// Builds a Vertex Gemini image request body.
    pub fn gemini_request_body(
        &self,
        options: &ImageModelCallOptions,
    ) -> Result<JsonValue, GoogleVertexError> {
        if options.mask.is_some() {
            return Err(GoogleVertexError::new(
                "GoogleVertexImageError",
                "Gemini image models do not support mask-based image editing.",
            ));
        }
        if options.n > 1 {
            return Err(GoogleVertexError::new(
                "GoogleVertexImageError",
                "Gemini image models do not support generating a set number of images per call. Use n=1 or omit the n parameter.",
            ));
        }

        let mut parts = Vec::new();
        if let Some(prompt) = options.prompt.as_ref() {
            parts.push(json!({ "text": prompt }));
        }
        if let Some(files) = options.files.as_ref() {
            for file in files {
                match file {
                    ImageModelFile::File {
                        media_type, data, ..
                    } => parts.push(json!({
                        "inlineData": {
                            "mimeType": media_type,
                            "data": convert_to_base64(data),
                        },
                    })),
                    ImageModelFile::Url { .. } => {
                        return Err(GoogleVertexError::new(
                            "GoogleVertexImageError",
                            "URL-based Gemini image inputs with media type \"image/*\" must be downloaded and passed as inline bytes.",
                        ));
                    }
                }
            }
        }

        let mut generation_config = JsonObject::new();
        generation_config.insert("responseModalities".to_string(), json!(["IMAGE"]));
        if let Some(aspect_ratio) = options.aspect_ratio.as_ref() {
            generation_config.insert(
                "imageConfig".to_string(),
                json!({ "aspectRatio": aspect_ratio }),
            );
        }
        if let Some(seed) = options.seed {
            generation_config.insert("seed".to_string(), json!(seed));
        }

        let user_options = image_options(&options.provider_options).unwrap_or_default();
        for (key, value) in user_options {
            generation_config.insert(key, value);
        }

        Ok(json!({
            "contents": [{
                "role": "user",
                "parts": parts,
            }],
            "generationConfig": generation_config,
        }))
    }

    /// Builds the predict URL.
    pub fn predict_url(&self) -> String {
        format!("{}/models/{}:predict", self.base_url(), self.model_id)
    }

    /// Builds the Gemini generate-content URL.
    pub fn generate_content_url(&self) -> String {
        format!(
            "{}/models/{}:generateContent",
            self.base_url(),
            self.model_id
        )
    }

    async fn generate_result(&self, options: ImageModelCallOptions) -> ImageModelResult {
        let mut warnings = Vec::new();
        if options.size.is_some() {
            warnings.push(Warning::Unsupported {
                feature: "size".to_string(),
                details: Some(
                    "This model does not support the `size` option. Use `aspectRatio` instead."
                        .to_string(),
                ),
            });
        }

        let request_body = if self.is_gemini_model() {
            match self.gemini_request_body(&options) {
                Ok(body) => body,
                Err(error) => {
                    return image_error_result(
                        &self.config,
                        &self.model_id,
                        "googleVertex",
                        error.to_string(),
                        warnings,
                    );
                }
            }
        } else {
            self.imagen_request_body(&options)
        };
        let url = if self.is_gemini_model() {
            self.generate_content_url()
        } else {
            self.predict_url()
        };

        match post_vertex_json(
            &self.config,
            url,
            request_body,
            options.headers,
            options.abort_signal,
        )
        .await
        {
            Ok(response) if self.is_gemini_model() => {
                gemini_image_result_from_response(&self.config, &self.model_id, response, warnings)
            }
            Ok(response) => {
                imagen_result_from_response(&self.config, &self.model_id, response, warnings)
            }
            Err(error) => image_error_result(
                &self.config,
                &self.model_id,
                "googleVertex",
                error.to_string(),
                warnings,
            ),
        }
    }
}

impl ImageModel for GoogleVertexImageModel {
    type MaxImagesPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        self.provider()
    }

    fn model_id(&self) -> &str {
        self.model_id()
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        ready(Some(if self.is_gemini_model() { 10 } else { 4 }))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.generate_result(options))
    }
}

/// Google Vertex video model options.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleVertexVideoModelOptions {
    /// Poll interval in milliseconds.
    #[serde(
        default,
        rename = "pollIntervalMs",
        skip_serializing_if = "Option::is_none"
    )]
    pub poll_interval_ms: Option<u64>,
    /// Poll timeout in milliseconds.
    #[serde(
        default,
        rename = "pollTimeoutMs",
        skip_serializing_if = "Option::is_none"
    )]
    pub poll_timeout_ms: Option<u64>,
    /// Person generation setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person_generation: Option<String>,
    /// Negative prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    /// Audio generation setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generate_audio: Option<bool>,
    /// GCS output directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gcs_output_directory: Option<String>,
    /// Reference images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_images: Option<JsonArray>,
    /// Additional passthrough options.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Google Vertex video model.
#[derive(Clone)]
pub struct GoogleVertexVideoModel {
    model_id: String,
    config: GoogleVertexConfig,
}

impl GoogleVertexVideoModel {
    /// Creates a video model.
    pub fn new(model_id: impl Into<String>, config: GoogleVertexConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Returns model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns provider id.
    pub fn provider(&self) -> &str {
        self.config.provider()
    }

    /// Returns base URL.
    pub fn base_url(&self) -> &str {
        self.config.base_url()
    }

    /// Builds the initial long-running prediction body.
    pub fn request_body(&self, options: &VideoModelCallOptions) -> JsonValue {
        let provider_options = video_options(&options.provider_options);
        let mut instance = JsonObject::new();
        if let Some(prompt) = options.prompt.as_ref() {
            instance.insert("prompt".to_string(), json!(prompt));
        }
        let mut warnings_ignored = Vec::new();
        if let Some(image) = options.image.as_ref()
            && let Some(image) = video_file_to_instance_image(image, &mut warnings_ignored)
        {
            instance.insert("image".to_string(), image);
        }
        if let Some(reference_images) = provider_options
            .as_ref()
            .and_then(|options| options.reference_images.clone())
        {
            instance.insert("referenceImages".to_string(), json!(reference_images));
        }

        let mut parameters = JsonObject::new();
        parameters.insert("sampleCount".to_string(), json!(options.n));
        if let Some(aspect_ratio) = options.aspect_ratio.as_ref() {
            parameters.insert("aspectRatio".to_string(), json!(aspect_ratio));
        }
        if let Some(resolution) = options.resolution.as_ref() {
            parameters.insert(
                "resolution".to_string(),
                json!(vertex_video_resolution(resolution)),
            );
        }
        if let Some(duration) = options.duration {
            parameters.insert("durationSeconds".to_string(), json!(duration));
        }
        if let Some(seed) = options.seed {
            parameters.insert("seed".to_string(), json!(seed));
        }
        if let Some(provider_options) = provider_options {
            if let Some(value) = provider_options.person_generation {
                parameters.insert("personGeneration".to_string(), json!(value));
            }
            if let Some(value) = provider_options.negative_prompt {
                parameters.insert("negativePrompt".to_string(), json!(value));
            }
            if let Some(value) = provider_options.generate_audio {
                parameters.insert("generateAudio".to_string(), json!(value));
            }
            if let Some(value) = provider_options.gcs_output_directory {
                parameters.insert("gcsOutputDirectory".to_string(), json!(value));
            }
            for (key, value) in provider_options.extra {
                parameters.insert(key, value);
            }
        }

        json!({
            "instances": [JsonValue::Object(instance)],
            "parameters": parameters,
        })
    }

    /// Builds the long-running prediction URL.
    pub fn predict_long_running_url(&self) -> String {
        format!(
            "{}/models/{}:predictLongRunning",
            self.base_url(),
            self.model_id
        )
    }

    /// Builds the operation polling URL.
    pub fn fetch_operation_url(&self) -> String {
        format!(
            "{}/models/{}:fetchPredictOperation",
            self.base_url(),
            self.model_id
        )
    }

    async fn generate_result(&self, options: VideoModelCallOptions) -> VideoModelResult {
        let mut warnings = Vec::new();
        if let Some(VideoModelFile::Url { .. }) = options.image.as_ref() {
            warnings.push(Warning::Unsupported {
                feature: "URL-based image input".to_string(),
                details: Some(
                    "Vertex AI video models require base64-encoded images or GCS URIs. URL will be ignored."
                        .to_string(),
                ),
            });
        }

        let provider_options = video_options(&options.provider_options).unwrap_or_default();
        let request_body = self.request_body(&options);
        let response = post_vertex_json(
            &self.config,
            self.predict_long_running_url(),
            request_body,
            options.headers.clone(),
            options.abort_signal.clone(),
        )
        .await;

        let mut operation = match response {
            Ok(response) => response.value,
            Err(error) => {
                return video_error_result(
                    &self.config,
                    &self.model_id,
                    "googleVertex",
                    error.to_string(),
                    warnings,
                    None,
                );
            }
        };

        let operation_name = match operation.get("name").and_then(JsonValue::as_str) {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => {
                return video_error_result(
                    &self.config,
                    &self.model_id,
                    "googleVertex",
                    "No operation name returned from API",
                    warnings,
                    None,
                );
            }
        };

        let poll_interval_ms = provider_options.poll_interval_ms.unwrap_or(10_000);
        let poll_timeout_ms = provider_options.poll_timeout_ms.unwrap_or(600_000);
        let mut elapsed_ms = 0_u64;
        let mut response_headers = None;

        while !operation
            .get("done")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false)
        {
            if elapsed_ms >= poll_timeout_ms {
                return video_error_result(
                    &self.config,
                    &self.model_id,
                    "googleVertex",
                    format!("Video generation timed out after {poll_timeout_ms}ms"),
                    warnings,
                    response_headers,
                );
            }
            if options
                .abort_signal
                .as_ref()
                .is_some_and(|signal| signal.is_aborted())
            {
                return video_error_result(
                    &self.config,
                    &self.model_id,
                    "googleVertex",
                    "Video generation request was aborted",
                    warnings,
                    response_headers,
                );
            }
            elapsed_ms = elapsed_ms.saturating_add(poll_interval_ms);
            match post_vertex_json(
                &self.config,
                self.fetch_operation_url(),
                json!({ "operationName": operation_name }),
                options.headers.clone(),
                options.abort_signal.clone(),
            )
            .await
            {
                Ok(response) => {
                    response_headers = Some(response.headers.clone());
                    operation = response.value;
                }
                Err(error) => {
                    return video_error_result(
                        &self.config,
                        &self.model_id,
                        "googleVertex",
                        error.to_string(),
                        warnings,
                        response_headers,
                    );
                }
            }
        }

        if let Some(error) = operation.get("error") {
            let message = error
                .get("message")
                .and_then(JsonValue::as_str)
                .unwrap_or("unknown error");
            return video_error_result(
                &self.config,
                &self.model_id,
                "googleVertex",
                format!("Video generation failed: {message}"),
                warnings,
                response_headers,
            );
        }

        video_result_from_operation(
            &self.config,
            &self.model_id,
            operation,
            warnings,
            response_headers,
        )
    }
}

impl VideoModel for GoogleVertexVideoModel {
    type MaxVideosPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = VideoModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        self.provider()
    }

    fn model_id(&self) -> &str {
        self.model_id()
    }

    fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
        ready(Some(4))
    }

    fn do_generate(&self, options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.generate_result(options))
    }
}

/// Supported Anthropic tools exposed by Vertex Anthropic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoogleVertexAnthropicTools {
    names: Vec<&'static str>,
}

impl GoogleVertexAnthropicTools {
    /// Creates the Vertex Anthropic tool subset.
    pub fn new() -> Self {
        Self {
            names: vec![
                "bash_20241022",
                "bash_20250124",
                "textEditor_20241022",
                "textEditor_20250124",
                "textEditor_20250429",
                "textEditor_20250728",
                "computer_20241022",
                "webSearch_20250305",
                "toolSearchRegex_20251119",
                "toolSearchBm25_20251119",
            ],
        }
    }

    /// Returns tool names.
    pub fn names(&self) -> &[&'static str] {
        &self.names
    }
}

impl Default for GoogleVertexAnthropicTools {
    fn default() -> Self {
        Self::new()
    }
}

/// Vertex Anthropic provider settings.
#[derive(Clone, Default)]
pub struct GoogleVertexAnthropicProviderSettings {
    /// Project id.
    pub project: Option<String>,
    /// Location.
    pub location: Option<String>,
    /// Base URL override.
    pub base_url: Option<String>,
    /// Headers.
    pub headers: Option<GoogleVertexHeaders>,
    /// Transport.
    pub transport: Option<GoogleVertexTransport>,
    /// Current date.
    pub current_date: Option<GoogleVertexDateProvider>,
}

impl GoogleVertexAnthropicProviderSettings {
    /// Creates settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets project.
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Sets location.
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Sets base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(static_headers(headers));
        self
    }

    /// Sets a header resolver.
    pub fn with_header_resolver(mut self, headers: GoogleVertexHeaders) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Sets transport.
    pub fn with_transport(mut self, transport: GoogleVertexTransport) -> Self {
        self.transport = Some(transport);
        self
    }
}

/// Vertex Anthropic provider.
#[derive(Clone)]
pub struct GoogleVertexAnthropicProvider {
    settings: GoogleVertexAnthropicProviderSettings,
}

impl GoogleVertexAnthropicProvider {
    /// Creates the base provider.
    pub fn base(settings: GoogleVertexAnthropicProviderSettings) -> Self {
        Self { settings }
    }

    /// Creates the node-style provider.
    pub fn node(
        mut settings: GoogleVertexAnthropicProviderSettings,
        token_generator: Option<GoogleVertexAuthTokenGenerator>,
    ) -> Self {
        let token_generator = token_generator
            .unwrap_or_else(|| create_auth_token_generator(GoogleAuthOptions::new()));
        let user_headers = settings.headers.take();
        settings.headers = Some(auth_headers(token_generator, user_headers, true));
        Self::base(settings)
    }

    /// Creates the edge-style provider.
    pub fn edge(
        mut settings: GoogleVertexAnthropicProviderSettings,
        credentials: Option<GoogleCredentials>,
    ) -> Self {
        let transport = settings
            .transport
            .clone()
            .unwrap_or_else(default_vertex_transport);
        let token_generator: GoogleVertexAuthTokenGenerator = Arc::new(move || {
            let credentials = credentials.clone();
            let transport = Arc::clone(&transport);
            Box::pin(async move {
                generate_edge_auth_token(
                    credentials,
                    transport,
                    OffsetDateTime::now_utc().unix_timestamp(),
                )
                .await
                .map(Some)
            })
        });
        let user_headers = settings.headers.take();
        settings.headers = Some(auth_headers(token_generator, user_headers, true));
        Self::base(settings)
    }

    /// Creates a language model.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<GoogleVertexAnthropicLanguageModel, GoogleVertexError> {
        let base_url = self.base_url()?;
        let mut config = GoogleVertexConfig::new("googleVertex.anthropic.messages", base_url)
            .with_transport(
                self.settings
                    .transport
                    .clone()
                    .unwrap_or_else(default_vertex_transport),
            );
        if let Some(headers) = self.settings.headers.clone() {
            config = config.with_header_resolver(headers);
        }
        if let Some(current_date) = self.settings.current_date.clone() {
            config.current_date = current_date;
        }
        Ok(GoogleVertexAnthropicLanguageModel::new(model_id, config))
    }

    /// Upstream aliases.
    pub fn chat(
        &self,
        model_id: impl Into<String>,
    ) -> Result<GoogleVertexAnthropicLanguageModel, GoogleVertexError> {
        self.language_model(model_id)
    }

    /// Returns NoSuchModelError for unsupported embedding models.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> NoSuchModelError {
        NoSuchModelError::new(model_id, ModelType::EmbeddingModel)
    }

    /// Deprecated upstream alias.
    pub fn text_embedding_model(&self, model_id: impl Into<String>) -> NoSuchModelError {
        self.embedding_model(model_id)
    }

    /// Returns NoSuchModelError for unsupported image models.
    pub fn image_model(&self, model_id: impl Into<String>) -> NoSuchModelError {
        NoSuchModelError::new(model_id, ModelType::ImageModel)
    }

    /// Returns the Vertex Anthropic tool subset.
    pub fn tools(&self) -> GoogleVertexAnthropicTools {
        GoogleVertexAnthropicTools::new()
    }

    fn base_url(&self) -> Result<String, GoogleVertexError> {
        if let Some(base_url) = without_trailing_slash(self.settings.base_url.as_deref()) {
            return Ok(base_url.to_string());
        }

        let location =
            load_optional_setting(self.settings.location.as_deref(), "GOOGLE_VERTEX_LOCATION")
                .unwrap_or_else(|| "global".to_string());
        let project =
            load_optional_setting(self.settings.project.as_deref(), "GOOGLE_VERTEX_PROJECT")
                .unwrap_or_default();
        let host_prefix = if location == "global" {
            String::new()
        } else {
            format!("{location}-")
        };

        Ok(format!(
            "https://{host_prefix}aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/publishers/anthropic/models"
        ))
    }
}

/// Creates the Vertex Anthropic provider.
pub fn create_google_vertex_anthropic(
    settings: GoogleVertexAnthropicProviderSettings,
) -> GoogleVertexAnthropicProvider {
    GoogleVertexAnthropicProvider::node(settings, None)
}

/// Creates the edge Vertex Anthropic provider.
pub fn create_google_vertex_anthropic_edge(
    settings: GoogleVertexAnthropicProviderSettings,
    credentials: Option<GoogleCredentials>,
) -> GoogleVertexAnthropicProvider {
    GoogleVertexAnthropicProvider::edge(settings, credentials)
}

/// Vertex Anthropic language model.
#[derive(Clone)]
pub struct GoogleVertexAnthropicLanguageModel {
    model_id: String,
    config: GoogleVertexConfig,
}

impl GoogleVertexAnthropicLanguageModel {
    /// Creates a Vertex Anthropic language model.
    pub fn new(model_id: impl Into<String>, config: GoogleVertexConfig) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }

    /// Returns model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns provider id.
    pub fn provider(&self) -> &str {
        self.config.provider()
    }

    /// Returns base URL.
    pub fn base_url(&self) -> &str {
        self.config.base_url()
    }

    /// Builds the Anthropic rawPredict/streamRawPredict URL.
    pub fn request_url(&self, streaming: bool) -> String {
        format!(
            "{}/{}:{}",
            self.base_url(),
            self.model_id,
            if streaming {
                "streamRawPredict"
            } else {
                "rawPredict"
            }
        )
    }

    /// Transforms an Anthropic request body for Vertex.
    pub fn transform_request_body(&self, mut body: JsonObject) -> JsonValue {
        body.remove("model");
        body.insert("anthropic_version".to_string(), json!("vertex-2023-10-16"));
        JsonValue::Object(body)
    }
}

impl LanguageModel for GoogleVertexAnthropicLanguageModel {
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
        self.provider()
    }

    fn model_id(&self) -> &str {
        self.model_id()
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(LanguageModelSupportedUrls::new())
    }

    fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        let provider = self.config.provider.clone();
        let model_id = self.model_id.clone();
        Box::pin(async move {
            language_error_result(
                &provider,
                "Vertex Anthropic request conversion is available; full Anthropic generation is owned by the Anthropic provider port",
                json!({ "model": model_id }),
            )
        })
    }

    fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(async { LanguageModelStreamResult::new(Vec::new()) })
    }
}

/// Settings shared by Vertex MaaS and Vertex xAI OpenAI-compatible wrappers.
#[derive(Clone, Default)]
pub struct GoogleVertexOpenAICompatibleSettings {
    /// Project id.
    pub project: Option<String>,
    /// Location, defaulting to global.
    pub location: Option<String>,
    /// Base URL override.
    pub base_url: Option<String>,
    /// Headers.
    pub headers: Option<GoogleVertexHeaders>,
    /// Transport.
    pub transport: Option<GoogleVertexTransport>,
    /// Current date provider for OpenAI-compatible response metadata.
    pub current_date: Option<GoogleVertexDateProvider>,
}

impl GoogleVertexOpenAICompatibleSettings {
    /// Creates settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets project id.
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Sets location.
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Sets base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets static headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(static_headers(headers));
        self
    }

    /// Sets header resolver.
    pub fn with_header_resolver(mut self, headers: GoogleVertexHeaders) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Sets transport.
    pub fn with_transport(mut self, transport: GoogleVertexTransport) -> Self {
        self.transport = Some(transport);
        self
    }
}

/// Vertex MaaS provider.
#[derive(Clone)]
pub struct GoogleVertexMaasProvider {
    settings: GoogleVertexOpenAICompatibleSettings,
    token_generator: Option<GoogleVertexAuthTokenGenerator>,
    cached_provider: Arc<OnceLock<OpenAICompatibleProvider>>,
    creation_count: Arc<Mutex<usize>>,
}

impl GoogleVertexMaasProvider {
    /// Creates the base MaaS provider.
    pub fn base(settings: GoogleVertexOpenAICompatibleSettings) -> Self {
        Self {
            settings,
            token_generator: None,
            cached_provider: Arc::new(OnceLock::new()),
            creation_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Creates a node-style authenticated MaaS provider.
    pub fn node(
        settings: GoogleVertexOpenAICompatibleSettings,
        token_generator: Option<GoogleVertexAuthTokenGenerator>,
    ) -> Self {
        Self {
            settings,
            token_generator: Some(
                token_generator
                    .unwrap_or_else(|| create_auth_token_generator(GoogleAuthOptions::new())),
            ),
            cached_provider: Arc::new(OnceLock::new()),
            creation_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Creates an edge-style authenticated MaaS provider.
    pub fn edge(
        settings: GoogleVertexOpenAICompatibleSettings,
        credentials: Option<GoogleCredentials>,
    ) -> Self {
        let transport = settings
            .transport
            .clone()
            .unwrap_or_else(default_vertex_transport);
        let token_generator: GoogleVertexAuthTokenGenerator = Arc::new(move || {
            let credentials = credentials.clone();
            let transport = Arc::clone(&transport);
            Box::pin(async move {
                generate_edge_auth_token(
                    credentials,
                    transport,
                    OffsetDateTime::now_utc().unix_timestamp(),
                )
                .await
                .map(Some)
            })
        });
        Self::node(settings, Some(token_generator))
    }

    /// Creates a language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.get_provider().language_model(model_id)
    }

    /// Creates a chat model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.get_provider().chat_model(model_id)
    }

    /// Creates a completion model.
    pub fn completion_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleCompletionLanguageModel {
        self.get_provider().completion_model(model_id)
    }

    /// Creates an embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.get_provider().embedding_model(model_id)
    }

    /// Deprecated upstream alias.
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.get_provider().text_embedding_model(model_id)
    }

    /// Creates an image model.
    pub fn image_model(&self, model_id: impl Into<String>) -> OpenAICompatibleImageModel {
        self.get_provider().image_model(model_id)
    }

    /// Returns how many inner OpenAI-compatible providers were constructed.
    pub fn creation_count(&self) -> usize {
        *self
            .creation_count
            .lock()
            .expect("creation count lock is not poisoned")
    }

    fn get_provider(&self) -> OpenAICompatibleProvider {
        self.cached_provider
            .get_or_init(|| {
                *self
                    .creation_count
                    .lock()
                    .expect("creation count lock is not poisoned") += 1;
                openai_compatible_provider(
                    "vertex.maas",
                    self.openai_base_url(),
                    &self.settings,
                    self.token_generator.clone(),
                    false,
                )
            })
            .clone()
    }

    fn openai_base_url(&self) -> String {
        openai_vertex_base_url(&self.settings).unwrap_or_else(|_| {
            "https://aiplatform.googleapis.com/v1/projects//locations/global/endpoints/openapi"
                .to_string()
        })
    }
}

/// Creates a node-style Vertex MaaS provider.
pub fn create_google_vertex_maas(
    settings: GoogleVertexOpenAICompatibleSettings,
) -> GoogleVertexMaasProvider {
    GoogleVertexMaasProvider::node(settings, None)
}

/// Creates an edge-style Vertex MaaS provider.
pub fn create_google_vertex_maas_edge(
    settings: GoogleVertexOpenAICompatibleSettings,
    credentials: Option<GoogleCredentials>,
) -> GoogleVertexMaasProvider {
    GoogleVertexMaasProvider::edge(settings, credentials)
}

/// Vertex xAI provider.
#[derive(Clone)]
pub struct GoogleVertexXaiProvider {
    settings: GoogleVertexOpenAICompatibleSettings,
    token_generator: Option<GoogleVertexAuthTokenGenerator>,
    cached_provider: Arc<OnceLock<OpenAICompatibleProvider>>,
    creation_count: Arc<Mutex<usize>>,
}

impl GoogleVertexXaiProvider {
    /// Creates the base xAI provider.
    pub fn base(settings: GoogleVertexOpenAICompatibleSettings) -> Self {
        Self {
            settings,
            token_generator: None,
            cached_provider: Arc::new(OnceLock::new()),
            creation_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Creates a node-style authenticated xAI provider.
    pub fn node(
        settings: GoogleVertexOpenAICompatibleSettings,
        token_generator: Option<GoogleVertexAuthTokenGenerator>,
    ) -> Self {
        Self {
            settings,
            token_generator: Some(
                token_generator
                    .unwrap_or_else(|| create_auth_token_generator(GoogleAuthOptions::new())),
            ),
            cached_provider: Arc::new(OnceLock::new()),
            creation_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Creates an edge-style authenticated xAI provider.
    pub fn edge(
        settings: GoogleVertexOpenAICompatibleSettings,
        credentials: Option<GoogleCredentials>,
    ) -> Self {
        let transport = settings
            .transport
            .clone()
            .unwrap_or_else(default_vertex_transport);
        let token_generator: GoogleVertexAuthTokenGenerator = Arc::new(move || {
            let credentials = credentials.clone();
            let transport = Arc::clone(&transport);
            Box::pin(async move {
                generate_edge_auth_token(
                    credentials,
                    transport,
                    OffsetDateTime::now_utc().unix_timestamp(),
                )
                .await
                .map(Some)
            })
        });
        Self::node(settings, Some(token_generator))
    }

    /// Creates a language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> GoogleVertexXaiLanguageModel {
        GoogleVertexXaiLanguageModel::new(self.get_provider().language_model(model_id))
    }

    /// Creates a chat model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> GoogleVertexXaiLanguageModel {
        GoogleVertexXaiLanguageModel::new(self.get_provider().chat_model(model_id))
    }

    /// Returns NoSuchModelError for unsupported embeddings.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> NoSuchModelError {
        NoSuchModelError::new(model_id, ModelType::EmbeddingModel)
    }

    /// Deprecated upstream alias.
    pub fn text_embedding_model(&self, model_id: impl Into<String>) -> NoSuchModelError {
        self.embedding_model(model_id)
    }

    /// Returns NoSuchModelError for unsupported image models.
    pub fn image_model(&self, model_id: impl Into<String>) -> NoSuchModelError {
        NoSuchModelError::new(model_id, ModelType::ImageModel)
    }

    /// Returns how many inner OpenAI-compatible providers were constructed.
    pub fn creation_count(&self) -> usize {
        *self
            .creation_count
            .lock()
            .expect("creation count lock is not poisoned")
    }

    fn get_provider(&self) -> OpenAICompatibleProvider {
        self.cached_provider
            .get_or_init(|| {
                *self
                    .creation_count
                    .lock()
                    .expect("creation count lock is not poisoned") += 1;
                openai_compatible_provider(
                    "googleVertex.xai",
                    self.openai_base_url(),
                    &self.settings,
                    self.token_generator.clone(),
                    true,
                )
            })
            .clone()
    }

    fn openai_base_url(&self) -> String {
        openai_vertex_base_url(&self.settings).unwrap_or_else(|_| {
            "https://aiplatform.googleapis.com/v1/projects//locations/global/endpoints/openapi"
                .to_string()
        })
    }
}

/// Creates a node-style Vertex xAI provider.
pub fn create_google_vertex_xai(
    settings: GoogleVertexOpenAICompatibleSettings,
) -> GoogleVertexXaiProvider {
    GoogleVertexXaiProvider::node(settings, None)
}

/// Creates an edge-style Vertex xAI provider.
pub fn create_google_vertex_xai_edge(
    settings: GoogleVertexOpenAICompatibleSettings,
    credentials: Option<GoogleCredentials>,
) -> GoogleVertexXaiProvider {
    GoogleVertexXaiProvider::edge(settings, credentials)
}

/// xAI language model wrapper that adds Vertex Grok URL support and usage
/// conversion on top of the OpenAI-compatible transport.
#[derive(Clone)]
pub struct GoogleVertexXaiLanguageModel {
    inner: OpenAICompatibleChatLanguageModel,
}

impl GoogleVertexXaiLanguageModel {
    /// Creates a wrapper around an OpenAI-compatible chat model.
    pub fn new(inner: OpenAICompatibleChatLanguageModel) -> Self {
        Self { inner }
    }

    /// Returns model id.
    pub fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    /// Returns provider id.
    pub fn provider(&self) -> &str {
        self.inner.provider()
    }
}

impl LanguageModel for GoogleVertexXaiLanguageModel {
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
        self.provider()
    }

    fn model_id(&self) -> &str {
        self.model_id()
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(LanguageModelSupportedUrls::from([(
            "image/*".to_string(),
            vec!["^https?://.*$".to_string()],
        )]))
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        let inner = self.inner.clone();
        Box::pin(async move { adjust_xai_usage(inner.do_generate(options).await) })
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        self.inner.do_stream(options)
    }
}

#[derive(Clone, Debug)]
struct VertexJsonResponse {
    value: JsonValue,
    raw_value: JsonValue,
    headers: Headers,
}

async fn post_vertex_json(
    config: &GoogleVertexConfig,
    url: String,
    body: JsonValue,
    headers: Option<Headers>,
    abort_signal: Option<ai_sdk_provider::language_model::ProviderAbortSignal>,
) -> Result<VertexJsonResponse, GoogleVertexError> {
    let request_headers = resolved_request_headers(config, headers).await?;
    let options = PostJsonToApiOptions::new(url, body)
        .with_headers(
            request_headers
                .into_iter()
                .map(|(name, value)| (name, Some(value))),
        )
        .with_optional_abort_signal(abort_signal);
    let request = options.into_request();
    if request
        .abort_signal
        .as_ref()
        .is_some_and(|signal| signal.is_aborted())
    {
        return Err(GoogleVertexError::new(
            "AbortError",
            "The operation was aborted.",
        ));
    }
    let response = (config.transport)(request)
        .await
        .map_err(|error| GoogleVertexError::new("FetchError", error.message().to_string()))?;
    let raw_value = response_json(&response)?;
    if !response.is_success_status() {
        return Err(GoogleVertexError::new(
            "ApiCallError",
            vertex_error_message(&raw_value, &response.status_text),
        ));
    }

    Ok(VertexJsonResponse {
        value: raw_value.clone(),
        raw_value,
        headers: response.headers,
    })
}

async fn resolved_request_headers(
    config: &GoogleVertexConfig,
    headers: Option<Headers>,
) -> Result<Headers, GoogleVertexError> {
    let mut merged = if let Some(resolve_headers) = config.headers.as_ref() {
        resolve_headers().await?
    } else {
        Headers::new()
    };

    if let Some(api_key) = config.api_key.as_ref() {
        merged.insert("x-goog-api-key".to_string(), api_key.clone());
    }
    if let Some(headers) = headers {
        merged.extend(headers);
    }
    merged = with_user_agent_suffix(
        Some(merged.into_iter().map(|(name, value)| (name, Some(value)))),
        [format!("ai-sdk/google-vertex/{}", crate::VERSION)],
    );
    Ok(merged)
}

fn response_json(response: &ProviderApiResponse) -> Result<JsonValue, GoogleVertexError> {
    let body = response.text_body().unwrap_or("null");
    serde_json::from_str(body).map_err(|error| {
        GoogleVertexError::new(
            "InvalidResponseDataError",
            format!("Provider response is not valid JSON: {error}"),
        )
    })
}

fn embedding_result_from_response(response: VertexJsonResponse) -> EmbeddingModelResult {
    let predictions = response
        .value
        .get("predictions")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    let mut embeddings = Vec::new();
    let mut token_count = 0_u64;
    for prediction in predictions {
        if let Some(values) = prediction
            .get("embeddings")
            .and_then(|embeddings| embeddings.get("values"))
            .and_then(JsonValue::as_array)
        {
            embeddings.push(values.iter().filter_map(JsonValue::as_f64).collect());
        }
        token_count += prediction
            .get("embeddings")
            .and_then(|embeddings| embeddings.get("statistics"))
            .and_then(|statistics| statistics.get("token_count"))
            .and_then(JsonValue::as_u64)
            .unwrap_or(0);
    }
    let mut embedding_response = EmbeddingModelResponse::new().with_body(response.raw_value);
    for (name, value) in response.headers {
        embedding_response = embedding_response.with_header(name, value);
    }

    EmbeddingModelResult::new(embeddings)
        .with_usage(EmbeddingModelUsage::new(token_count))
        .with_response(embedding_response)
}

fn imagen_result_from_response(
    config: &GoogleVertexConfig,
    model_id: &str,
    response: VertexJsonResponse,
    warnings: Vec<Warning>,
) -> ImageModelResult {
    let predictions = response
        .value
        .get("predictions")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    let images = predictions
        .iter()
        .filter_map(|prediction| {
            prediction
                .get("bytesBase64Encoded")
                .and_then(JsonValue::as_str)
                .map(|data| FileDataContent::Base64(data.to_string()))
        })
        .collect::<Vec<_>>();
    let metadata_images = predictions
        .iter()
        .map(|prediction| {
            let mut image = JsonObject::new();
            if let Some(prompt) = prediction.get("prompt").and_then(JsonValue::as_str) {
                image.insert("revisedPrompt".to_string(), json!(prompt));
            }
            JsonValue::Object(image)
        })
        .collect::<JsonArray>();
    let mut image_response = ImageModelResponse::new((config.current_date)(), model_id);
    for (name, value) in response.headers {
        image_response = image_response.with_header(name, value);
    }
    let mut result = ImageModelResult::new(images, image_response)
        .with_provider_metadata(vertex_image_metadata(metadata_images));
    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn gemini_image_result_from_response(
    config: &GoogleVertexConfig,
    model_id: &str,
    response: VertexJsonResponse,
    warnings: Vec<Warning>,
) -> ImageModelResult {
    let mut images = Vec::new();
    if let Some(candidates) = response
        .value
        .get("candidates")
        .and_then(JsonValue::as_array)
    {
        for candidate in candidates {
            if let Some(parts) = candidate
                .get("content")
                .and_then(|content| content.get("parts"))
                .and_then(JsonValue::as_array)
            {
                for part in parts {
                    if let Some(data) = part
                        .get("inlineData")
                        .or_else(|| part.get("inline_data"))
                        .and_then(|inline_data| inline_data.get("data"))
                        .and_then(JsonValue::as_str)
                    {
                        images.push(FileDataContent::Base64(data.to_string()));
                    }
                }
            }
        }
    }

    let usage = response
        .value
        .get("usageMetadata")
        .or_else(|| response.value.get("usage_metadata"))
        .map(|usage| {
            let mut image_usage = ImageModelUsage::new();
            if let Some(input_tokens) = usage
                .get("promptTokenCount")
                .or_else(|| usage.get("prompt_token_count"))
                .and_then(JsonValue::as_u64)
            {
                image_usage = image_usage.with_input_tokens(input_tokens);
            }
            if let Some(output_tokens) = usage
                .get("candidatesTokenCount")
                .or_else(|| usage.get("candidates_token_count"))
                .and_then(JsonValue::as_u64)
            {
                image_usage = image_usage.with_output_tokens(output_tokens);
            }
            if let Some(total_tokens) = usage
                .get("totalTokenCount")
                .or_else(|| usage.get("total_token_count"))
                .and_then(JsonValue::as_u64)
            {
                image_usage = image_usage.with_total_tokens(total_tokens);
            }
            image_usage
        });

    let mut image_response = ImageModelResponse::new((config.current_date)(), model_id);
    for (name, value) in response.headers {
        image_response = image_response.with_header(name, value);
    }
    let mut result = ImageModelResult::new(images.clone(), image_response)
        .with_provider_metadata(vertex_image_metadata(vec![json!({}); images.len()]));
    if let Some(usage) = usage {
        result = result.with_usage(usage);
    }
    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn image_error_result(
    config: &GoogleVertexConfig,
    model_id: &str,
    provider_name: &str,
    message: impl Into<String>,
    warnings: Vec<Warning>,
) -> ImageModelResult {
    let mut result = ImageModelResult::new(
        Vec::new(),
        ImageModelResponse::new((config.current_date)(), model_id),
    )
    .with_provider_metadata(vertex_image_error_metadata(provider_name, message.into()));
    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn video_result_from_operation(
    config: &GoogleVertexConfig,
    model_id: &str,
    operation: JsonValue,
    warnings: Vec<Warning>,
    response_headers: Option<Headers>,
) -> VideoModelResult {
    let videos_value = operation
        .get("response")
        .and_then(|response| response.get("videos"))
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    if videos_value.is_empty() {
        return video_error_result(
            config,
            model_id,
            "googleVertex",
            format!("No videos in response. Response: {operation}"),
            warnings,
            response_headers,
        );
    }

    let mut videos = Vec::new();
    let mut metadata_videos = Vec::new();
    for video in videos_value {
        let media_type = video
            .get("mimeType")
            .and_then(JsonValue::as_str)
            .unwrap_or("video/mp4");
        if let Some(data) = video.get("bytesBase64Encoded").and_then(JsonValue::as_str) {
            videos.push(VideoModelVideoData::base64(data, media_type));
            metadata_videos.push(json!({ "mimeType": media_type }));
        } else if let Some(gcs_uri) = video.get("gcsUri").and_then(JsonValue::as_str)
            && let Ok(url) = Url::parse(gcs_uri)
        {
            videos.push(VideoModelVideoData::url(url, media_type));
            metadata_videos.push(json!({ "gcsUri": gcs_uri, "mimeType": media_type }));
        }
    }
    if videos.is_empty() {
        return video_error_result(
            config,
            model_id,
            "googleVertex",
            "No valid videos in response",
            warnings,
            response_headers,
        );
    }

    let mut video_response = VideoModelResponse::new((config.current_date)(), model_id);
    if let Some(response_headers) = response_headers {
        for (name, value) in response_headers {
            video_response = video_response.with_header(name, value);
        }
    }
    let mut result = VideoModelResult::new(videos, video_response)
        .with_provider_metadata(vertex_video_metadata(metadata_videos));
    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn video_error_result(
    config: &GoogleVertexConfig,
    model_id: &str,
    provider_name: &str,
    message: impl Into<String>,
    warnings: Vec<Warning>,
    response_headers: Option<Headers>,
) -> VideoModelResult {
    let mut response = VideoModelResponse::new((config.current_date)(), model_id);
    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }
    let mut result = VideoModelResult::new(Vec::new(), response)
        .with_provider_metadata(provider_error_metadata(provider_name, message.into()));
    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn language_error_result(
    provider_name: &str,
    message: impl Into<String>,
    request_body: JsonValue,
) -> LanguageModelGenerateResult {
    LanguageModelGenerateResult::new(
        Vec::new(),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("google-vertex-error".to_string()),
        },
        LanguageModelUsage::default(),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_provider_metadata(provider_error_metadata(provider_name, message.into()))
}

fn vertex_image_metadata(images: JsonArray) -> ImageModelProviderMetadata {
    let entry = ImageModelProviderMetadataEntry::new(images);
    ImageModelProviderMetadata::from([
        ("googleVertex".to_string(), entry.clone()),
        ("vertex".to_string(), entry),
    ])
}

fn vertex_image_error_metadata(
    provider_name: &str,
    message: impl Into<String>,
) -> ImageModelProviderMetadata {
    let entry = ImageModelProviderMetadataEntry::new(JsonArray::new())
        .with_extra("error", json!({ "message": message.into() }));
    ImageModelProviderMetadata::from([(provider_name.to_string(), entry)])
}

fn vertex_video_metadata(videos: JsonArray) -> ProviderMetadata {
    let mut entry = JsonObject::new();
    entry.insert("videos".to_string(), json!(videos));
    ProviderMetadata::from([
        ("googleVertex".to_string(), entry.clone()),
        ("google-vertex".to_string(), entry.clone()),
        ("vertex".to_string(), entry),
    ])
}

fn provider_error_metadata(provider_name: &str, message: impl Into<String>) -> ProviderMetadata {
    let mut entry = JsonObject::new();
    entry.insert("error".to_string(), json!({ "message": message.into() }));
    ProviderMetadata::from([(provider_name.to_string(), entry)])
}

fn embedding_options(
    provider_options: Option<&ProviderOptions>,
) -> Option<GoogleVertexEmbeddingModelOptions> {
    let object = provider_options_for(provider_options?, &["googleVertex", "vertex", "google"])?;
    serde_json::from_value(JsonValue::Object(object.clone())).ok()
}

fn image_options(provider_options: &ProviderOptions) -> Option<JsonObject> {
    provider_options_for(provider_options, &["googleVertex", "vertex"]).cloned()
}

fn video_options(provider_options: &ProviderOptions) -> Option<GoogleVertexVideoModelOptions> {
    let object = provider_options_for(provider_options, &["googleVertex", "vertex"])?;
    serde_json::from_value(JsonValue::Object(object.clone())).ok()
}

fn provider_options_for<'a>(
    provider_options: &'a ProviderOptions,
    provider_names: &[&str],
) -> Option<&'a JsonObject> {
    provider_names
        .iter()
        .find_map(|provider| provider_options.get(*provider))
}

fn image_file_base64(file: &ImageModelFile) -> String {
    match file {
        ImageModelFile::File { data, .. } => convert_to_base64(data),
        ImageModelFile::Url { .. } => {
            panic!("URL-based images are not supported for Google Vertex image editing")
        }
    }
}

fn video_file_to_instance_image(
    file: &VideoModelFile,
    warnings: &mut Vec<Warning>,
) -> Option<JsonValue> {
    match file {
        VideoModelFile::File {
            media_type, data, ..
        } => Some(json!({
            "bytesBase64Encoded": convert_to_base64(data),
            "mimeType": media_type,
        })),
        VideoModelFile::Url { .. } => {
            warnings.push(Warning::Unsupported {
                feature: "URL-based image input".to_string(),
                details: Some(
                    "Vertex AI video models require base64-encoded images or GCS URIs. URL will be ignored."
                        .to_string(),
                ),
            });
            None
        }
    }
}

fn vertex_video_resolution(resolution: &str) -> &str {
    match resolution {
        "1280x720" => "720p",
        "1920x1080" => "1080p",
        "3840x2160" => "4k",
        value => value,
    }
}

fn openai_compatible_provider(
    name: &str,
    base_url: String,
    settings: &GoogleVertexOpenAICompatibleSettings,
    token_generator: Option<GoogleVertexAuthTokenGenerator>,
    xai: bool,
) -> OpenAICompatibleProvider {
    let mut openai_settings = OpenAICompatibleProviderSettings::new(name, base_url);
    if xai {
        openai_settings = openai_settings
            .with_include_usage(true)
            .with_supports_structured_outputs(true)
            .with_transform_request_body(strip_reasoning_effort);
    }

    let provider = create_openai_compatible(openai_settings);
    let transport = settings
        .transport
        .clone()
        .unwrap_or_else(default_vertex_transport);
    let headers = settings.headers.clone();
    let provider = provider.with_transport(auth_wrapping_transport(
        transport,
        headers,
        token_generator,
        false,
    ));
    if let Some(current_date) = settings.current_date.clone() {
        provider.with_current_date(move || current_date())
    } else {
        provider
    }
}

fn auth_wrapping_transport(
    transport: GoogleVertexTransport,
    headers: Option<GoogleVertexHeaders>,
    token_generator: Option<GoogleVertexAuthTokenGenerator>,
    user_headers_override_authorization: bool,
) -> OpenAICompatibleTransport {
    Arc::new(move |mut request| {
        let transport = Arc::clone(&transport);
        let headers = headers.clone();
        let token_generator = token_generator.clone();
        Box::pin(async move {
            let mut resolved_headers = Headers::new();
            if user_headers_override_authorization
                && let Some(token_generator) = token_generator.clone()
            {
                if let Some(token) = token_generator()
                    .await
                    .map_err(|error| FetchErrorInfo::new(error.to_string()))?
                {
                    resolved_headers.insert("Authorization".to_string(), format!("Bearer {token}"));
                }
            }
            if let Some(headers) = headers {
                resolved_headers.extend(
                    headers()
                        .await
                        .map_err(|error| FetchErrorInfo::new(error.to_string()))?,
                );
            }
            if !user_headers_override_authorization && let Some(token_generator) = token_generator {
                if let Some(token) = token_generator()
                    .await
                    .map_err(|error| FetchErrorInfo::new(error.to_string()))?
                {
                    resolved_headers.insert("Authorization".to_string(), format!("Bearer {token}"));
                }
            }
            request.headers.extend(resolved_headers);
            transport(request).await
        })
    })
}

fn openai_vertex_base_url(
    settings: &GoogleVertexOpenAICompatibleSettings,
) -> Result<String, GoogleVertexError> {
    if let Some(base_url) = without_trailing_slash(settings.base_url.as_deref())
        && !base_url.is_empty()
    {
        return Ok(base_url.to_string());
    }

    let project = load_required_setting(
        settings.project.as_deref(),
        "project",
        "GOOGLE_VERTEX_PROJECT",
        "Google Vertex project",
    )?;
    let location = load_optional_setting(settings.location.as_deref(), "GOOGLE_VERTEX_LOCATION")
        .unwrap_or_else(|| "global".to_string());
    Ok(format!(
        "https://aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/endpoints/openapi"
    ))
}

fn strip_reasoning_effort(mut value: JsonValue) -> JsonValue {
    if let Some(object) = value.as_object_mut() {
        object.remove("reasoning_effort");
        object.remove("reasoningEffort");
    }
    value
}

fn adjust_xai_usage(mut result: LanguageModelGenerateResult) -> LanguageModelGenerateResult {
    let Some(raw) = result.usage.raw.as_ref() else {
        return result;
    };
    let completion_tokens = raw.get("completion_tokens").and_then(JsonValue::as_u64);
    let reasoning_tokens = raw
        .get("completion_tokens_details")
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(JsonValue::as_u64)
        .or_else(|| raw.get("reasoning_tokens").and_then(JsonValue::as_u64));
    if let (Some(completion_tokens), Some(reasoning_tokens)) = (completion_tokens, reasoning_tokens)
    {
        result.usage.output_tokens = OutputTokenUsage {
            total: Some(completion_tokens + reasoning_tokens),
            text: Some(completion_tokens),
            reasoning: Some(reasoning_tokens),
        };
    }
    result
}

fn vertex_error_message(value: &JsonValue, fallback: &str) -> String {
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(JsonValue::as_str)
        .unwrap_or(fallback)
        .to_string()
}

fn default_vertex_transport() -> GoogleVertexTransport {
    Arc::new(|request| Box::pin(ready(execute_vertex_request(request))))
}

fn execute_vertex_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_vertex_get_request(request),
        ProviderApiRequestMethod::Post => execute_vertex_post_request(request),
    }
}

fn execute_vertex_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    provider_api_response(builder.config().http_status_as_error(false).build().call())
}

fn execute_vertex_post_request(
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
                "multipart form data is not supported by the Google Vertex transport",
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

fn load_required_setting(
    setting_value: Option<&str>,
    setting_name: &str,
    environment_variable_name: &str,
    description: &str,
) -> Result<String, GoogleVertexError> {
    setting_value
        .map(ToString::to_string)
        .or_else(|| env::var(environment_variable_name).ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            GoogleVertexError::new(
                "LoadSettingError",
                format!("Missing {description}: set {setting_name} or {environment_variable_name}"),
            )
        })
}

fn load_optional_setting(
    setting_value: Option<&str>,
    environment_variable_name: &str,
) -> Option<String> {
    setting_value
        .map(ToString::to_string)
        .or_else(|| env::var(environment_variable_name).ok())
        .filter(|value| !value.is_empty())
}

fn trim_trailing_slash(value: String) -> String {
    without_trailing_slash(Some(&value))
        .unwrap_or(&value)
        .to_string()
}

fn normalize_pem_newlines(value: &str) -> String {
    value.replace("\\n", "\n")
}

fn default_generate_id() -> String {
    let nanos = OffsetDateTime::now_utc().unix_timestamp_nanos();
    format!("vertex-{nanos}")
}

#[allow(dead_code)]
fn base64url_json(value: &JsonValue) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).expect("JSON value serializes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_sdk_provider::language_model::{
        LanguageModelMessage, LanguageModelTextPart, LanguageModelUserContentPart,
        LanguageModelUserMessage,
    };
    use serde_json::json;
    use std::task::{Context, Poll, Wake, Waker};

    const TEST_PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----\nMIIEvwIBADANBgkqhkiG9w0BAQEFAASCBKkwggSlAgEAAoIBAQCZBNEcsvuH8v6p\nteg2VYj+RZnns1XLawi0h2p3bsO8XQp1kxT8tRPrIQ7rTbGtM07oeCqRfiFrbjH6\nZlqYH1oc9b5hApoqIL5cFzYq9LTrX9pP83kraDYF/7QkQCc3ioGHqoW5W59QARIT\nEudU6Xg33ZGFPjXksPzrJyuJwtNlcgOgusFYwwuQhZ6bTxMUpl3jYWR6lr53rAr4\naAnhlJXdU1YAFGsZDEtmCydvf7vHgBb+rHo0vDe9tuUbOcPRvYFP7MVPrN44UCWw\nGDo7qfHW1HxnVVYe8yVCGvciLiC9ULD7lAGYMj03D8JyG73Il3Z2grBREkQqPu1M\njjoCsrM1AgMBAAECggEAHeEH6B+25+P2ADOKBVoMZwI2PD0TaqYazA2JJ4sUY2qT\niUPQHExLeGU7IY1JPXXAWbplLYXAhta8oZVs6TluAiumIhE9Ay7jnN3XcOnZjgBo\ng6YaKfSuX9t/VHjGb5z3EAOnGvueDyQ2YE0XqMfx9o6oRKlSIrbAnDZI1Rya5Lri\nUOGzmWV6zLU4RlMjmVTU+1gK3gWCvJySrTGw6yjDbNB5NrX2Grzp3JZJWK7WRyed\nH1y4DpnT4w9R6JcFaQxb8jM1YCCBkethBxBgsxwRzJicWXTSUQVLtukjsaF/A3MZ\nvew6GWJ86X3lDH2gaO9dEr0qQSKIFSFcIDN1tJ/jfQKBgQDLqmdLy9lTya50kGmC\nv/QXgfZ64doaj3ZAXhSaB3kkCo+EMEGN7s3gXzMW7F6hJS9TisOA5kj1LymgAxG6\n8NvGRtBNgdAbdXnQNDrkVgEum3YDwjVCWc5q2M0pf842PmnF2U2yVtJktAPiwhLM\ntA38Ed7cTaB9qsmhY7DKwPqU7wKBgQDAVr+ODeGxK+I/RtKPlM2D2fckIQqQOFKc\nxtlYlur2KarVSfgELh/WwxCt2Yag7PmjNKiM5Qcf+JKSX9CIvz0NV/yRqds02/vx\nnJBsF7IRDoziG7/rUuJq/RTbmiYhK2uQ1Q6i17zm0M73/FG9ow4RqqGwFDuO7n7Q\nShiXH9biGwKBgQCbFw5GB9tdFK3Gkdnm+Sl1ZUA+3xHpO+n+piXmDV7QdUJIlT62\nSG16OMR85k5BREG/ymGKHNLd8qYt9WhhBN03JeGlw/6nilPSmpNmIaAQz82UmyVX\ne2/WqXXB7lMnt2twgEPMVJUunm5/FO6f91TW6PzeojZeu9mDDpkoLMAk/QKBgQC0\nerIUYgI9dag/J/28rSyLZKP7SuXWnoMmiZC5CCRCCKc8rMQFaCKIK1IjT9J8fuFg\nu7DNRLuCzIT8xNuw9YIcW0usg24mE6Y9+WOrijCUwMqCAPf9oTDEo+ZGikbtKQku\nRj4Nn9Kp45XSLPmmsLIq8an2x4V7gV+No3mflUjVsQKBgQDDeZ39nf3k3uq4jNwS\nlWo2pZQHqu/PQ7261l09cJ/5fJRNnRGYBlDwRMKidlo/+iaEwliRFYipnAFya7CX\nzZLNKJv1jGL8zVzXYf3GWYipgS+US3K7BU8S74JiPOxHPSffmaj8IFBTaYD3sYVo\nZfBMXOofiPvAMg+Vg5ZAoFuI2A==\n-----END PRIVATE KEY-----";

    #[derive(Clone, Default)]
    struct CapturedRequests {
        requests: Arc<Mutex<Vec<ProviderApiRequest>>>,
    }

    impl CapturedRequests {
        fn transport(&self, responses: Vec<ProviderApiResponse>) -> GoogleVertexTransport {
            let requests = Arc::clone(&self.requests);
            let responses = Arc::new(Mutex::new(responses));
            Arc::new(move |request| {
                let requests = Arc::clone(&requests);
                let responses = Arc::clone(&responses);
                Box::pin(ready({
                    requests
                        .lock()
                        .expect("requests lock is not poisoned")
                        .push(request);
                    Ok(responses
                        .lock()
                        .expect("responses lock is not poisoned")
                        .remove(0))
                }))
            })
        }

        fn requests(&self) -> Vec<ProviderApiRequest> {
            self.requests
                .lock()
                .expect("requests lock is not poisoned")
                .clone()
        }

        fn body(&self, index: usize) -> JsonValue {
            let request = self.requests()[index].clone();
            let body = request
                .body
                .and_then(|body| body.as_text().map(ToString::to_string))
                .expect("request body is text JSON");
            serde_json::from_str(&body).expect("request body parses")
        }
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

    fn fixed_date() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("timestamp is valid")
    }

    fn headers(entries: &[(&str, &str)]) -> Headers {
        entries
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect()
    }

    fn text_response(body: JsonValue) -> ProviderApiResponse {
        ProviderApiResponse::text(200, "OK", body.to_string())
    }

    fn provider_config(
        captured: &CapturedRequests,
        response: ProviderApiResponse,
    ) -> GoogleVertexConfig {
        GoogleVertexConfig::new("google.vertex.test", "https://api.example.com")
            .with_transport(captured.transport(vec![response]))
            .with_current_date(fixed_date)
    }

    #[test]
    fn google_vertex_foundational_inventory_tracks_current_upstream_cases() {
        assert_eq!(UPSTREAM_PACKAGE, "@ai-sdk/google-vertex");
        assert_eq!(UPSTREAM_PACKAGE_DIR, "packages/google-vertex");
        assert_eq!(UPSTREAM_COMMIT, "ab6d66482d31afe15f4973a51c5f7cfa09c92ea6");
        assert_eq!(UPSTREAM_TEST_FILES, 17);
        assert_eq!(UPSTREAM_TEST_CASES, 204);
        assert_eq!(TYPE_SYSTEM_IMPOSSIBLE_CASES, 0);
        assert_eq!(JS_ONLY_DOCUMENTED_CASES, 3);
        assert_eq!(PORTABLE_MAPPED_CASES, 201);
        assert_eq!(PORTABLE_UNMAPPED_CASES, 0);
        assert_eq!(
            INVENTORY_DOCUMENT,
            "docs/ai-foundational-provider-inventory.md"
        );
    }

    #[test]
    fn google_vertex_auth_headers_cover_node_and_edge_wrappers() {
        let calls = Arc::new(Mutex::new(0));
        let generator: GoogleVertexAuthTokenGenerator = {
            let calls = Arc::clone(&calls);
            Arc::new(move || {
                let calls = Arc::clone(&calls);
                Box::pin(async move {
                    *calls.lock().expect("calls lock is not poisoned") += 1;
                    Ok(Some("generated-token".to_string()))
                })
            })
        };
        let headers = auth_headers(
            generator,
            Some(static_headers(headers(&[
                ("X-Test", "value"),
                ("Authorization", "Bearer custom"),
            ]))),
            true,
        );

        let first = poll_ready(headers()).expect("headers resolve");
        let second = poll_ready(headers()).expect("headers resolve again");

        assert_eq!(
            first.get("Authorization"),
            Some(&"Bearer custom".to_string())
        );
        assert_eq!(first.get("X-Test"), Some(&"value".to_string()));
        assert_eq!(
            second.get("Authorization"),
            Some(&"Bearer custom".to_string())
        );
        assert_eq!(*calls.lock().expect("calls lock is not poisoned"), 2);

        let null_generator: GoogleVertexAuthTokenGenerator = Arc::new(|| Box::pin(ready(Ok(None))));
        let null_headers =
            poll_ready(auth_headers(null_generator, None, true)()).expect("null token is accepted");
        assert!(!null_headers.contains_key("Authorization"));
    }

    #[test]
    fn google_vertex_anthropic_provider_matches_vertex_specific_contract() {
        let provider = GoogleVertexAnthropicProvider::base(
            GoogleVertexAnthropicProviderSettings::new()
                .with_project("vertex-project")
                .with_location("us-central1")
                .with_headers(headers(&[("X-Custom", "true")])),
        );
        let model = provider
            .language_model("claude-sonnet-4@20250514")
            .expect("model creates");
        let mut body = JsonObject::new();
        body.insert("model".to_string(), json!("claude"));
        body.insert("messages".to_string(), json!([]));

        assert_eq!(model.provider(), "googleVertex.anthropic.messages");
        assert_eq!(
            model.base_url(),
            "https://us-central1-aiplatform.googleapis.com/v1/projects/vertex-project/locations/us-central1/publishers/anthropic/models"
        );
        assert_eq!(
            model.request_url(false),
            "https://us-central1-aiplatform.googleapis.com/v1/projects/vertex-project/locations/us-central1/publishers/anthropic/models/claude-sonnet-4@20250514:rawPredict"
        );
        assert_eq!(
            model.transform_request_body(body),
            json!({
                "messages": [],
                "anthropic_version": "vertex-2023-10-16",
            })
        );
        assert!(poll_ready(model.supported_urls()).is_empty());
        assert_eq!(
            provider.embedding_model("embed").model_type(),
            ModelType::EmbeddingModel
        );
        assert!(provider.tools().names().contains(&"textEditor_20250728"));
    }

    #[test]
    fn google_vertex_edge_auth_builds_jwt_and_token_exchange_request() {
        let credentials =
            GoogleCredentials::new("service@example.iam.gserviceaccount.com", TEST_PRIVATE_KEY)
                .with_private_key_id("kid-123");
        let jwt = build_edge_auth_jwt(&credentials, 1_700_000_000).expect("jwt builds");
        let parts = jwt.split('.').collect::<Vec<_>>();
        assert_eq!(parts.len(), 3);
        let header: JsonValue =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).expect("header decodes"))
                .expect("header JSON parses");
        let claims: JsonValue =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).expect("claims decodes"))
                .expect("claims JSON parses");
        assert_eq!(header["alg"], "RS256");
        assert_eq!(header["kid"], "kid-123");
        assert_eq!(claims["iss"], "service@example.iam.gserviceaccount.com");
        assert_eq!(claims["scope"], GOOGLE_CLOUD_SCOPE);
        assert_eq!(claims["aud"], OAUTH_TOKEN_URL);
        assert_eq!(claims["iat"], 1_700_000_000);
        assert_eq!(claims["exp"], 1_700_003_600);

        let captured = CapturedRequests::default();
        let token = poll_ready(generate_edge_auth_token(
            Some(credentials),
            captured.transport(vec![text_response(
                json!({ "access_token": "access-token" }),
            )]),
            1_700_000_000,
        ))
        .expect("token response parses");
        assert_eq!(token, "access-token");
        let request = &captured.requests()[0];
        assert_eq!(request.url, OAUTH_TOKEN_URL);
        assert_eq!(
            request.headers.get("content-type"),
            Some(&"application/x-www-form-urlencoded".to_string())
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/google-vertex/0.1.0"))
        );
        assert!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .is_some_and(|body| body.contains("grant_type=urn%3Aietf"))
        );
        assert!(build_edge_auth_jwt(&GoogleCredentials::new("", TEST_PRIVATE_KEY), 0).is_err());
        assert!(build_edge_auth_jwt(&GoogleCredentials::new("x", ""), 0).is_err());
    }

    #[test]
    fn google_vertex_node_and_edge_auth_wrappers_match_upstream() {
        let provider = GoogleVertexProvider::node(
            GoogleVertexProviderSettings::new()
                .with_project("p")
                .with_location("global")
                .with_headers(headers(&[("X-Test", "node")])),
            GoogleAuthOptions::new().with_access_token("node-token"),
        );
        let model = provider
            .embedding_model("text-embedding-004")
            .expect("model");
        let captured = CapturedRequests::default();
        let model_id = model.model_id().to_string();
        let model = GoogleVertexEmbeddingModel::new(
            model_id,
            model
                .config
                .with_transport(captured.transport(vec![text_response(json!({
                    "predictions": []
                }))])),
        );
        let _ = poll_ready(model.do_embed(EmbeddingModelCallOptions::new(vec!["a".to_string()])));
        let request = &captured.requests()[0];
        assert_eq!(
            request.headers.get("authorization"),
            Some(&"Bearer node-token".to_string())
        );
        assert_eq!(request.headers.get("x-test"), Some(&"node".to_string()));

        let api_key_provider = GoogleVertexProvider::node(
            GoogleVertexProviderSettings::new()
                .with_api_key("api-key")
                .with_project("ignored")
                .with_location("ignored"),
            GoogleAuthOptions::new().with_access_token("unused"),
        );
        let model = api_key_provider
            .image_model("imagen-3.0-generate-002")
            .expect("model");
        assert_eq!(model.base_url(), EXPRESS_MODE_BASE_URL);
    }

    #[test]
    fn google_vertex_embedding_model_builds_predict_requests_and_results() {
        let captured = CapturedRequests::default();
        let response = text_response(json!({
            "predictions": [
                { "embeddings": { "values": [1.0, 2.0], "statistics": { "token_count": 3 } } },
                { "embeddings": { "values": [3.0, 4.0], "statistics": { "token_count": 5 } } }
            ]
        }))
        .with_headers(headers(&[("x-response", "ok")]));
        let model = GoogleVertexEmbeddingModel::new(
            "text-embedding-004",
            GoogleVertexConfig::new("google.vertex.embedding", "https://api.example.com")
                .with_headers(headers(&[("x-config", "yes")]))
                .with_transport(captured.transport(vec![response])),
        );
        let provider_options = ProviderOptions::from([(
            "googleVertex".to_string(),
            serde_json::from_value(json!({
                "outputDimensionality": 2,
                "taskType": "RETRIEVAL_DOCUMENT",
                "title": "Doc",
                "autoTruncate": false
            }))
            .expect("options object"),
        )]);
        let result = poll_ready(
            model.do_embed(
                EmbeddingModelCallOptions::new(vec!["first".to_string(), "second".to_string()])
                    .with_provider_options(provider_options)
                    .with_header("x-call", "yes"),
            ),
        );

        assert_eq!(result.embeddings, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
        assert_eq!(result.usage.expect("usage").tokens, 8);
        assert_eq!(
            result
                .response
                .expect("response")
                .headers
                .expect("headers")
                .get("x-response"),
            Some(&"ok".to_string())
        );
        let request = &captured.requests()[0];
        assert_eq!(
            request.url,
            "https://api.example.com/models/text-embedding-004:predict"
        );
        assert_eq!(request.headers.get("x-config"), Some(&"yes".to_string()));
        assert_eq!(request.headers.get("x-call"), Some(&"yes".to_string()));
        assert_eq!(
            captured.body(0),
            json!({
                "instances": [
                    { "content": "first", "task_type": "RETRIEVAL_DOCUMENT", "title": "Doc" },
                    { "content": "second", "task_type": "RETRIEVAL_DOCUMENT", "title": "Doc" }
                ],
                "parameters": {
                    "outputDimensionality": 2,
                    "autoTruncate": false
                }
            })
        );

        let too_many = vec!["x".to_string(); 2049];
        let result = poll_ready(model.do_embed(EmbeddingModelCallOptions::new(too_many)));
        assert!(result.embeddings.is_empty());
        assert!(
            result
                .provider_metadata
                .expect("metadata")
                .get("googleVertex")
                .expect("google metadata")
                .get("error")
                .expect("error")
                .get("message")
                .expect("message")
                .as_str()
                .expect("message string")
                .contains("2049 values")
        );
    }

    #[test]
    fn google_vertex_image_model_covers_imagen_and_gemini_contracts() {
        let captured = CapturedRequests::default();
        let model = GoogleVertexImageModel::new(
            "imagen-4.0-ultra-generate-001",
            provider_config(
                &captured,
                text_response(json!({
                    "predictions": [
                        { "bytesBase64Encoded": "image-a", "mimeType": "image/png", "prompt": "revised" }
                    ]
                }))
                .with_headers(headers(&[("x-image", "ok")])),
            ),
        );
        let provider_options = ProviderOptions::from([(
            "googleVertex".to_string(),
            serde_json::from_value(json!({
                "negativePrompt": "rain",
                "personGeneration": "allow_adult",
                "safetySetting": "block_only_high",
                "sampleImageSize": "2K"
            }))
            .expect("options object"),
        )]);
        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(2)
                    .with_prompt("A city")
                    .with_aspect_ratio("16:9")
                    .with_seed(42)
                    .with_size("1024x1024")
                    .with_provider_options(provider_options)
                    .with_header("x-call", "yes"),
            ),
        );

        assert_eq!(poll_ready(model.max_images_per_call()), Some(4));
        assert_eq!(
            result.images,
            vec![FileDataContent::Base64("image-a".to_string())]
        );
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.response.model_id, "imagen-4.0-ultra-generate-001");
        assert_eq!(
            result
                .provider_metadata
                .expect("metadata")
                .get("googleVertex")
                .expect("google metadata")
                .images[0],
            json!({ "revisedPrompt": "revised" })
        );
        assert_eq!(
            captured.body(0),
            json!({
                "instances": [{ "prompt": "A city" }],
                "parameters": {
                    "sampleCount": 2,
                    "aspectRatio": "16:9",
                    "seed": 42,
                    "negativePrompt": "rain",
                    "personGeneration": "allow_adult",
                    "safetySetting": "block_only_high",
                    "sampleImageSize": "2K"
                }
            })
        );

        let edit_body = model.imagen_request_body(
            &ImageModelCallOptions::new(1)
                .with_prompt("Edit")
                .with_files(vec![ImageModelFile::file(
                    "image/png",
                    FileDataContent::Base64("source".to_string()),
                )])
                .with_mask(ImageModelFile::file(
                    "image/png",
                    FileDataContent::Bytes(vec![1, 2, 3]),
                ))
                .with_provider_options(ProviderOptions::from([(
                    "vertex".to_string(),
                    serde_json::from_value(json!({
                        "edit": {
                            "mode": "EDIT_MODE_INPAINT_REMOVAL",
                            "baseSteps": 35,
                            "maskMode": "MASK_MODE_USER_PROVIDED",
                            "maskDilation": 0.01
                        }
                    }))
                    .expect("options object"),
                )])),
        );
        assert_eq!(
            edit_body["instances"][0]["referenceImages"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            edit_body["parameters"],
            json!({
                "sampleCount": 1,
                "editMode": "EDIT_MODE_INPAINT_REMOVAL",
                "editConfig": { "baseSteps": 35 }
            })
        );

        let gemini = GoogleVertexImageModel::new(
            "gemini-3-pro-image-preview",
            provider_config(
                &CapturedRequests::default(),
                text_response(json!({
                    "candidates": [{
                        "content": {
                            "parts": [
                                { "inlineData": { "mimeType": "image/png", "data": "gemini-image" } }
                            ]
                        }
                    }],
                    "usageMetadata": {
                        "promptTokenCount": 20,
                        "candidatesTokenCount": 200,
                        "totalTokenCount": 220
                    }
                })),
            ),
        );
        assert_eq!(poll_ready(gemini.max_images_per_call()), Some(10));
        let gemini_body = gemini
            .gemini_request_body(
                &ImageModelCallOptions::new(1)
                    .with_prompt("A cat")
                    .with_aspect_ratio("1:1")
                    .with_seed(7)
                    .with_files(vec![ImageModelFile::file(
                        "image/png",
                        FileDataContent::Base64("source".to_string()),
                    )]),
            )
            .expect("gemini body");
        assert_eq!(
            gemini_body["generationConfig"]["responseModalities"],
            json!(["IMAGE"])
        );
        assert_eq!(
            gemini_body["contents"][0]["parts"],
            json!([
                { "text": "A cat" },
                { "inlineData": { "mimeType": "image/png", "data": "source" } }
            ])
        );
        assert!(
            gemini
                .gemini_request_body(&ImageModelCallOptions::new(2).with_prompt("A cat"))
                .expect_err("n > 1 errors")
                .message()
                .contains("do not support generating")
        );
    }

    #[test]
    fn google_vertex_base_provider_resolves_models_urls_tools_and_express_mode() {
        let provider = GoogleVertexProvider::base(
            GoogleVertexProviderSettings::new()
                .with_project("project")
                .with_location("global")
                .with_generate_id(|| "id-1".to_string()),
        );
        let chat = provider.language_model("gemini-2.5-flash").expect("chat");
        let embedding = provider
            .embedding_model("text-embedding-004")
            .expect("embedding");
        let image = provider
            .image_model("imagen-3.0-generate-002")
            .expect("image");
        let video = provider.video_model("veo-3.0-generate-001").expect("video");

        assert_eq!(chat.provider(), "google.vertex.chat");
        assert_eq!(
            chat.base_url(),
            "https://aiplatform.googleapis.com/v1beta1/projects/project/locations/global/publishers/google"
        );
        assert_eq!(embedding.provider(), "google.vertex.embedding");
        assert_eq!(image.provider(), "google.vertex.image");
        assert_eq!(video.provider(), "google.vertex.video");
        assert!(provider.tools().names().contains(&"vertexRagStore"));

        let regional = GoogleVertexProvider::base(
            GoogleVertexProviderSettings::new()
                .with_project("project")
                .with_location("us-central1"),
        );
        assert!(
            regional
                .language_model("gemini")
                .expect("regional")
                .base_url()
                .starts_with("https://us-central1-aiplatform.googleapis.com")
        );

        let express = GoogleVertexProvider::base(
            GoogleVertexProviderSettings::new()
                .with_api_key("key")
                .with_base_url("https://custom.example.com/"),
        );
        assert_eq!(
            express.image_model("imagen").expect("image").base_url(),
            "https://custom.example.com"
        );
    }

    #[test]
    fn google_vertex_video_model_covers_predict_long_running_contract() {
        let captured = CapturedRequests::default();
        let model = GoogleVertexVideoModel::new(
            "veo-3.0-generate-001",
            GoogleVertexConfig::new("google.vertex.video", "https://api.example.com")
                .with_transport(captured.transport(vec![
                    text_response(json!({ "name": "operations/1", "done": false })),
                    text_response(json!({
                        "name": "operations/1",
                        "done": true,
                        "response": {
                            "videos": [
                                { "bytesBase64Encoded": "video-a", "mimeType": "video/webm" },
                                { "gcsUri": "gs://bucket/video.mp4" }
                            ]
                        }
                    }))
                    .with_headers(headers(&[("x-poll", "ok")])),
                ]))
                .with_current_date(fixed_date),
        );
        let provider_options = ProviderOptions::from([(
            "googleVertex".to_string(),
            serde_json::from_value(json!({
                "pollIntervalMs": 1,
                "pollTimeoutMs": 5,
                "personGeneration": "allow_adult",
                "negativePrompt": "rain",
                "generateAudio": true,
                "gcsOutputDirectory": "gs://out",
                "referenceImages": [{ "gcsUri": "gs://ref" }],
                "customOption": "kept"
            }))
            .expect("options object"),
        )]);
        let result = poll_ready(
            model.do_generate(
                VideoModelCallOptions::new(2)
                    .with_prompt("A river")
                    .with_aspect_ratio("16:9")
                    .with_resolution("1920x1080")
                    .with_duration(8.0)
                    .with_seed(42)
                    .with_image(VideoModelFile::file(
                        "image/png",
                        FileDataContent::Base64("image".to_string()),
                    ))
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(poll_ready(model.max_videos_per_call()), Some(4));
        assert_eq!(result.videos.len(), 2);
        assert_eq!(
            result.provider_metadata.as_ref().unwrap()["googleVertex"]["videos"][1]["gcsUri"],
            "gs://bucket/video.mp4"
        );
        assert_eq!(captured.requests().len(), 2);
        assert_eq!(
            captured.body(0)["parameters"],
            json!({
                "sampleCount": 2,
                "aspectRatio": "16:9",
                "resolution": "1080p",
                "durationSeconds": 8.0,
                "seed": 42,
                "personGeneration": "allow_adult",
                "negativePrompt": "rain",
                "generateAudio": true,
                "gcsOutputDirectory": "gs://out",
                "customOption": "kept"
            })
        );
        assert_eq!(
            captured.body(0)["instances"][0]["image"],
            json!({ "bytesBase64Encoded": "image", "mimeType": "image/png" })
        );
        assert_eq!(captured.body(1), json!({ "operationName": "operations/1" }));

        let url_model = GoogleVertexVideoModel::new(
            "veo",
            provider_config(
                &CapturedRequests::default(),
                text_response(json!({
                    "name": "op",
                    "done": true,
                    "response": { "videos": [{ "bytesBase64Encoded": "v" }] }
                })),
            ),
        );
        let result = poll_ready(
            url_model.do_generate(
                VideoModelCallOptions::new(1).with_image(VideoModelFile::url(
                    Url::parse("https://example.com/image.png").expect("url parses"),
                )),
            ),
        );
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(
            result.videos[0],
            VideoModelVideoData::base64("v", "video/mp4")
        );

        let timeout = GoogleVertexVideoModel::new(
            "veo",
            GoogleVertexConfig::new("google.vertex.video", "https://api.example.com")
                .with_transport(CapturedRequests::default().transport(vec![
                    text_response(json!({ "name": "op", "done": false })),
                    text_response(json!({ "name": "op", "done": false })),
                ])),
        );
        let timeout_result = poll_ready(
            timeout.do_generate(
                VideoModelCallOptions::new(1).with_provider_options(ProviderOptions::from([(
                    "googleVertex".to_string(),
                    serde_json::from_value(json!({ "pollIntervalMs": 10, "pollTimeoutMs": 1 }))
                        .expect("options object"),
                )])),
            ),
        );
        assert!(timeout_result.videos.is_empty());
    }

    #[test]
    fn google_vertex_maas_provider_wraps_openai_compatible_lazily() {
        let captured = CapturedRequests::default();
        let provider = GoogleVertexMaasProvider::node(
            GoogleVertexOpenAICompatibleSettings::new()
                .with_project("project")
                .with_location("global")
                .with_headers(headers(&[("x-user", "yes")]))
                .with_transport(captured.transport(vec![text_response(json!({
                    "id": "chatcmpl",
                    "choices": [{ "message": { "role": "assistant", "content": "ok" }, "finish_reason": "stop" }],
                    "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
                }))])),
            Some(Arc::new(|| Box::pin(ready(Ok(Some("maas-token".to_string())))))),
        );

        assert_eq!(provider.creation_count(), 0);
        let model = provider.language_model("openai/gpt-oss-120b-maas");
        assert_eq!(provider.creation_count(), 1);
        let _again = provider.chat_model("openai/gpt-oss-20b-maas");
        assert_eq!(provider.creation_count(), 1);

        let _ = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("hi")),
            ])),
        ])));
        let request = &captured.requests()[0];
        assert_eq!(
            request.url,
            "https://aiplatform.googleapis.com/v1/projects/project/locations/global/endpoints/openapi/chat/completions"
        );
        assert_eq!(
            request.headers.get("Authorization"),
            Some(&"Bearer maas-token".to_string())
        );
        assert_eq!(request.headers.get("x-user"), Some(&"yes".to_string()));

        let custom = GoogleVertexMaasProvider::base(
            GoogleVertexOpenAICompatibleSettings::new().with_base_url("https://proxy.example.com/"),
        );
        let custom_model = custom.language_model("model");
        assert_eq!(custom_model.provider(), "vertex.maas.chat");
    }

    #[test]
    fn google_vertex_xai_provider_wraps_openai_compatible_with_grok_transform() {
        let captured = CapturedRequests::default();
        let provider = GoogleVertexXaiProvider::node(
            GoogleVertexOpenAICompatibleSettings::new()
                .with_project("project")
                .with_location("us-central1")
                .with_transport(captured.transport(vec![text_response(json!({
                    "id": "chatcmpl",
                    "choices": [{
                        "message": { "role": "assistant", "content": "ok" },
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 12,
                        "completion_tokens": 2,
                        "prompt_tokens_details": { "cached_tokens": 5 },
                        "completion_tokens_details": { "reasoning_tokens": 320 }
                    }
                }))])),
            Some(Arc::new(|| {
                Box::pin(ready(Ok(Some("xai-token".to_string()))))
            })),
        );

        assert_eq!(provider.creation_count(), 0);
        let model = provider.language_model("xai/grok-4.20-reasoning");
        assert_eq!(provider.creation_count(), 1);
        assert_eq!(model.provider(), "googleVertex.xai.chat");
        assert_eq!(
            poll_ready(model.supported_urls()).get("image/*").cloned(),
            Some(vec!["^https?://.*$".to_string()])
        );
        let provider_options = ProviderOptions::from([(
            "googleVertex.xai".to_string(),
            serde_json::from_value(json!({ "reasoningEffort": "high" })).expect("options object"),
        )]);
        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("hi"),
                    )]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.usage.output_tokens.total, Some(322));
        assert_eq!(result.usage.output_tokens.text, Some(2));
        assert_eq!(result.usage.output_tokens.reasoning, Some(320));
        assert_eq!(
            captured.requests()[0].url,
            "https://aiplatform.googleapis.com/v1/projects/project/locations/us-central1/endpoints/openapi/chat/completions"
        );
        assert!(
            !captured
                .body(0)
                .as_object()
                .expect("object")
                .contains_key("reasoning_effort")
        );
        assert_eq!(
            provider.embedding_model("embed").model_type(),
            ModelType::EmbeddingModel
        );
        assert_eq!(provider.creation_count(), 1);
    }

    #[test]
    #[ignore = "requires live GOOGLE_VERTEX_PROJECT/GOOGLE_VERTEX_LOCATION and GOOGLE_VERTEX_ACCESS_TOKEN or ADC integration"]
    fn live_google_vertex_embedding_smoke_uses_real_credentials() {
        let provider = create_google_vertex(
            GoogleVertexProviderSettings::new()
                .with_project(env::var("GOOGLE_VERTEX_PROJECT").expect("GOOGLE_VERTEX_PROJECT"))
                .with_location(env::var("GOOGLE_VERTEX_LOCATION").expect("GOOGLE_VERTEX_LOCATION")),
        );
        let model = provider
            .embedding_model("text-embedding-004")
            .expect("embedding model");
        let result = poll_ready(model.do_embed(EmbeddingModelCallOptions::new(vec![
            "hello vertex".to_string(),
        ])));
        assert_eq!(result.embeddings.len(), 1);
    }
}
