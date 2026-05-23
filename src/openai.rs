use std::collections::BTreeMap;
use std::env;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::file_data::{FileDataContent, ProviderReference};
use crate::files::{Files, FilesUploadFileCallOptions, FilesUploadFileData, FilesUploadFileResult};
use crate::headers::Headers;
use crate::image_model::{
    ImageModel, ImageModelCallOptions, ImageModelFile, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResult, ImageModelUsage,
};
use crate::json::{JsonArray, JsonObject, JsonValue};
use crate::open_responses::{
    OpenResponsesLanguageModel, OpenResponsesProvider, OpenResponsesProviderSettings,
};
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleCompletionLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleProvider, OpenAICompatibleProviderSettings,
    OpenAICompatibleTransport,
};
use crate::provider::{
    NoSuchModelError, Provider, ProviderMetadata, ProviderWithFiles, ProviderWithSkills,
    ProviderWithSpeechModel, ProviderWithTranscriptionModel,
};
use crate::provider_utils::{
    ConvertToFormDataOptions, FetchErrorInfo, FormData, FormDataInputValue, FormDataValue,
    HandledFetchError, PostFormDataToApiOptions, PostJsonToApiOptions,
    ProviderApiResponseHandlerError, ResponseHandlerResult, convert_base64_to_bytes,
    convert_to_form_data, create_binary_response_handler, create_json_response_handler,
    media_type_to_extension, post_form_data_to_api, post_json_to_api, without_trailing_slash,
};
use crate::skills::{
    Skills, SkillsFileData, SkillsUploadSkillCallOptions, SkillsUploadSkillResult,
};
use crate::speech_model::{
    SpeechModel, SpeechModelCallOptions, SpeechModelRequest, SpeechModelResponse, SpeechModelResult,
};
use crate::transcription_model::{
    TranscriptionModel, TranscriptionModelCallOptions, TranscriptionModelResponse,
    TranscriptionModelResult, TranscriptionModelSegment,
};
use crate::warning::Warning;
use time::OffsetDateTime;

/// Default base URL for upstream `@ai-sdk/openai` API calls.
pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

/// OpenAI-compatible error response payload.
///
/// Mirrors upstream `openaiErrorDataSchema`: `message` is required while
/// provider-specific `type`, `param`, and string-or-number `code` fields are
/// intentionally loose for OpenAI-compatible providers.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIErrorData {
    /// Error details returned by the OpenAI-compatible API.
    pub error: OpenAIErrorDetails,
}

/// Details from an OpenAI-compatible error response.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIErrorDetails {
    /// Human-readable error message.
    pub message: String,

    /// Provider-specific error type.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,

    /// Provider-specific parameter context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<JsonValue>,

    /// Provider-specific string or numeric error code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<JsonValue>,
}

/// Response returned by OpenAI's `/files` upload endpoint.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIFileResponse {
    /// OpenAI file identifier.
    pub id: String,

    /// OpenAI object discriminator.
    pub object: String,

    /// File size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,

    /// Unix creation timestamp.
    #[serde(
        default,
        rename = "created_at",
        skip_serializing_if = "Option::is_none"
    )]
    pub created_at: Option<u64>,

    /// Server filename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    /// OpenAI upload purpose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,

    /// OpenAI processing status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    /// Optional expiry timestamp.
    #[serde(
        default,
        rename = "expires_at",
        skip_serializing_if = "Option::is_none"
    )]
    pub expires_at: Option<u64>,
}

/// Response returned by OpenAI's `/skills` upload endpoint.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAISkillResponse {
    /// OpenAI skill identifier.
    pub id: String,

    /// Skill name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Skill description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Default version identifier.
    #[serde(
        default,
        rename = "default_version",
        skip_serializing_if = "Option::is_none"
    )]
    pub default_version: Option<String>,

    /// Latest version identifier.
    #[serde(
        default,
        rename = "latest_version",
        skip_serializing_if = "Option::is_none"
    )]
    pub latest_version: Option<String>,

    /// Unix creation timestamp.
    #[serde(
        default,
        rename = "created_at",
        skip_serializing_if = "Option::is_none"
    )]
    pub created_at: Option<u64>,

    /// Unix update timestamp.
    #[serde(
        default,
        rename = "updated_at",
        skip_serializing_if = "Option::is_none"
    )]
    pub updated_at: Option<u64>,
}

/// Response returned by OpenAI's `/audio/transcriptions` endpoint.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAITranscriptionResponse {
    /// Complete transcript text.
    pub text: String,

    /// Detected language name or code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// Audio duration in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,

    /// Segment-level timestamps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<OpenAITranscriptionSegment>>,

    /// Word-level timestamps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<OpenAITranscriptionWord>>,
}

/// Response returned by OpenAI's image endpoints.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIImageResponse {
    /// Unix creation timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<u64>,

    /// Generated image entries.
    pub data: Vec<OpenAIImageData>,

    /// Response-level background metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,

    /// Response-level output format metadata.
    #[serde(
        default,
        rename = "output_format",
        skip_serializing_if = "Option::is_none"
    )]
    pub output_format: Option<String>,

    /// Response-level size metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,

    /// Response-level quality metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,

    /// Optional image-generation token usage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<OpenAIImageUsage>,
}

/// OpenAI image response item.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIImageData {
    /// Base64-encoded image.
    #[serde(rename = "b64_json")]
    pub b64_json: String,

    /// Revised prompt, when returned by OpenAI.
    #[serde(
        default,
        rename = "revised_prompt",
        skip_serializing_if = "Option::is_none"
    )]
    pub revised_prompt: Option<String>,
}

/// OpenAI image usage response.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIImageUsage {
    /// Input token count.
    #[serde(
        default,
        rename = "input_tokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub input_tokens: Option<u64>,

    /// Output token count.
    #[serde(
        default,
        rename = "output_tokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub output_tokens: Option<u64>,

    /// Total token count.
    #[serde(
        default,
        rename = "total_tokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub total_tokens: Option<u64>,

    /// Input token detail split.
    #[serde(
        default,
        rename = "input_tokens_details",
        skip_serializing_if = "Option::is_none"
    )]
    pub input_tokens_details: Option<OpenAIImageInputTokenDetails>,
}

/// OpenAI image input token detail response.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIImageInputTokenDetails {
    /// Image input token count.
    #[serde(
        default,
        rename = "image_tokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub image_tokens: Option<u64>,

    /// Text input token count.
    #[serde(
        default,
        rename = "text_tokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub text_tokens: Option<u64>,
}

/// OpenAI transcription segment item.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAITranscriptionSegment {
    /// Segment start time in seconds.
    pub start: f64,

    /// Segment end time in seconds.
    pub end: f64,

    /// Segment text.
    pub text: String,
}

/// OpenAI transcription word item.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAITranscriptionWord {
    /// Word text.
    pub word: String,

    /// Word start time in seconds.
    pub start: f64,

    /// Word end time in seconds.
    pub end: f64,
}

/// OpenAI files upload interface.
#[derive(Clone)]
pub struct OpenAIFiles {
    provider: String,
    base_url: String,
    headers: Headers,
    transport: OpenAICompatibleTransport,
}

impl OpenAIFiles {
    fn new(
        provider: impl Into<String>,
        base_url: impl Into<String>,
        headers: Headers,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            provider: provider.into(),
            base_url: base_url.into(),
            headers,
            transport,
        }
    }
}

/// OpenAI skills upload interface.
#[derive(Clone)]
pub struct OpenAISkills {
    provider: String,
    base_url: String,
    headers: Headers,
    transport: OpenAICompatibleTransport,
}

impl OpenAISkills {
    fn new(
        provider: impl Into<String>,
        base_url: impl Into<String>,
        headers: Headers,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            provider: provider.into(),
            base_url: base_url.into(),
            headers,
            transport,
        }
    }
}

/// OpenAI image model for `/images/generations` and `/images/edits`.
#[derive(Clone)]
pub struct OpenAIImageModel {
    provider: String,
    model_id: String,
    base_url: String,
    headers: Headers,
    transport: OpenAICompatibleTransport,
    current_date: Option<Arc<dyn Fn() -> OffsetDateTime + Send + Sync>>,
}

impl OpenAIImageModel {
    fn new(
        provider: impl Into<String>,
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        headers: Headers,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            provider: provider.into(),
            model_id: model_id.into(),
            base_url: base_url.into(),
            headers,
            transport,
            current_date: None,
        }
    }

    /// Injects the response timestamp provider. This is primarily useful for deterministic tests.
    pub fn with_current_date(
        mut self,
        current_date: impl Fn() -> OffsetDateTime + Send + Sync + 'static,
    ) -> Self {
        self.current_date = Some(Arc::new(current_date));
        self
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
}

/// OpenAI speech model for `/audio/speech`.
#[derive(Clone)]
pub struct OpenAISpeechModel {
    provider: String,
    model_id: String,
    base_url: String,
    headers: Headers,
    transport: OpenAICompatibleTransport,
    current_date: Option<Arc<dyn Fn() -> OffsetDateTime + Send + Sync>>,
}

impl OpenAISpeechModel {
    fn new(
        provider: impl Into<String>,
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        headers: Headers,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            provider: provider.into(),
            model_id: model_id.into(),
            base_url: base_url.into(),
            headers,
            transport,
            current_date: None,
        }
    }

    /// Injects the response timestamp provider. This is primarily useful for deterministic tests.
    pub fn with_current_date(
        mut self,
        current_date: impl Fn() -> OffsetDateTime + Send + Sync + 'static,
    ) -> Self {
        self.current_date = Some(Arc::new(current_date));
        self
    }
}

/// OpenAI transcription model for `/audio/transcriptions`.
#[derive(Clone)]
pub struct OpenAITranscriptionModel {
    provider: String,
    model_id: String,
    base_url: String,
    headers: Headers,
    transport: OpenAICompatibleTransport,
    current_date: Option<Arc<dyn Fn() -> OffsetDateTime + Send + Sync>>,
}

impl OpenAITranscriptionModel {
    fn new(
        provider: impl Into<String>,
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        headers: Headers,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            provider: provider.into(),
            model_id: model_id.into(),
            base_url: base_url.into(),
            headers,
            transport,
            current_date: None,
        }
    }

    /// Injects the response timestamp provider. This is primarily useful for deterministic tests.
    pub fn with_current_date(
        mut self,
        current_date: impl Fn() -> OffsetDateTime + Send + Sync + 'static,
    ) -> Self {
        self.current_date = Some(Arc::new(current_date));
        self
    }
}

/// Settings for the upstream OpenAI provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAIProviderSettings {
    /// Base URL for OpenAI API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// OpenAI API key. When omitted, `OPENAI_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// OpenAI organization header value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,

    /// OpenAI project header value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,

    /// Provider name used as the provider id prefix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl OpenAIProviderSettings {
    /// Creates empty OpenAI provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the OpenAI API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the OpenAI API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets the OpenAI organization header value.
    pub fn with_organization(mut self, organization: impl Into<String>) -> Self {
        self.organization = Some(organization.into());
        self
    }

    /// Sets the OpenAI project header value.
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Sets the provider name used as the provider id prefix.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// Upstream OpenAI provider foundation.
#[derive(Clone)]
pub struct OpenAIProvider {
    settings: OpenAIProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

impl OpenAIProvider {
    /// Creates an OpenAI provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(OpenAIProviderSettings::new())
    }

    /// Creates a provider from explicit OpenAI settings.
    pub fn from_settings(settings: OpenAIProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the OpenAI API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the OpenAI API base URL for this provider.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self
    }

    /// Sets the OpenAI organization header value for this provider.
    pub fn with_organization(mut self, organization: impl Into<String>) -> Self {
        self.settings.organization = Some(organization.into());
        self
    }

    /// Sets the OpenAI project header value for this provider.
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.settings.project = Some(project.into());
        self
    }

    /// Sets the provider name used as the provider id prefix.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.settings.name = Some(name.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Creates an OpenAI Responses API language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenResponsesLanguageModel {
        self.responses(model_id)
    }

    /// Creates an OpenAI Responses API language model.
    pub fn responses(&self, model_id: impl Into<String>) -> OpenResponsesLanguageModel {
        self.open_responses_provider().language_model(model_id)
    }

    /// Creates an OpenAI chat language model.
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().chat_model(model_id)
    }

    /// Creates an OpenAI completion language model.
    pub fn completion(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleCompletionLanguageModel {
        self.openai_compatible_provider().completion_model(model_id)
    }

    /// Creates an OpenAI embedding model.
    pub fn embedding(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.openai_compatible_provider().embedding_model(model_id)
    }

    /// Creates an OpenAI embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.embedding(model_id)
    }

    /// Deprecated upstream alias for [`OpenAIProvider::embedding`].
    pub fn text_embedding(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.embedding(model_id)
    }

    /// Deprecated upstream alias for [`OpenAIProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates an OpenAI image model.
    pub fn image(&self, model_id: impl Into<String>) -> OpenAIImageModel {
        let provider_name = openai_provider_name(&self.settings);
        OpenAIImageModel::new(
            format!("{provider_name}.image"),
            model_id,
            openai_base_url(&self.settings),
            openai_headers(&self.settings),
            self.transport
                .as_ref()
                .map(Arc::clone)
                .unwrap_or_else(default_openai_files_transport),
        )
    }

    /// Creates an OpenAI image model.
    pub fn image_model(&self, model_id: impl Into<String>) -> OpenAIImageModel {
        self.image(model_id)
    }

    /// Creates an OpenAI speech model.
    pub fn speech(&self, model_id: impl Into<String>) -> OpenAISpeechModel {
        let provider_name = openai_provider_name(&self.settings);
        OpenAISpeechModel::new(
            format!("{provider_name}.speech"),
            model_id,
            openai_base_url(&self.settings),
            openai_headers(&self.settings),
            self.transport
                .as_ref()
                .map(Arc::clone)
                .unwrap_or_else(default_openai_files_transport),
        )
    }

    /// Creates an OpenAI speech model.
    pub fn speech_model(&self, model_id: impl Into<String>) -> OpenAISpeechModel {
        self.speech(model_id)
    }

    /// Creates an OpenAI transcription model.
    pub fn transcription(&self, model_id: impl Into<String>) -> OpenAITranscriptionModel {
        let provider_name = openai_provider_name(&self.settings);
        OpenAITranscriptionModel::new(
            format!("{provider_name}.transcription"),
            model_id,
            openai_base_url(&self.settings),
            openai_headers(&self.settings),
            self.transport
                .as_ref()
                .map(Arc::clone)
                .unwrap_or_else(default_openai_files_transport),
        )
    }

    /// Creates an OpenAI transcription model.
    pub fn transcription_model(&self, model_id: impl Into<String>) -> OpenAITranscriptionModel {
        self.transcription(model_id)
    }

    /// Creates the OpenAI files upload interface.
    pub fn files(&self) -> OpenAIFiles {
        let provider_name = openai_provider_name(&self.settings);
        OpenAIFiles::new(
            format!("{provider_name}.files"),
            openai_base_url(&self.settings),
            openai_headers(&self.settings),
            self.transport
                .as_ref()
                .map(Arc::clone)
                .unwrap_or_else(default_openai_files_transport),
        )
    }

    /// Creates the OpenAI skills upload interface.
    pub fn skills(&self) -> OpenAISkills {
        let provider_name = openai_provider_name(&self.settings);
        OpenAISkills::new(
            format!("{provider_name}.skills"),
            openai_base_url(&self.settings),
            openai_headers(&self.settings),
            self.transport
                .as_ref()
                .map(Arc::clone)
                .unwrap_or_else(default_openai_files_transport),
        )
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let provider_name = openai_provider_name(&self.settings);
        let mut settings =
            OpenAICompatibleProviderSettings::new(provider_name, openai_base_url(&self.settings))
                .with_user_agent_suffix(format!("ai-sdk/openai/{}", crate::VERSION));

        for (name, value) in openai_headers(&self.settings) {
            settings = settings.with_header(name.clone(), value.clone());
        }

        let provider = OpenAICompatibleProvider::from_settings(settings);

        if let Some(transport) = &self.transport {
            provider.with_transport(Arc::clone(transport))
        } else {
            provider
        }
    }

    fn open_responses_provider(&self) -> OpenResponsesProvider {
        let provider_name = openai_provider_name(&self.settings);
        let mut settings = OpenResponsesProviderSettings::new(
            provider_name,
            format!("{}/responses", openai_base_url(&self.settings)),
        )
        .with_user_agent_suffix(format!("ai-sdk/openai/{}", crate::VERSION))
        .with_file_id_prefix("file-");

        for (name, value) in openai_headers(&self.settings) {
            settings = settings.with_header(name.clone(), value.clone());
        }

        let provider = OpenResponsesProvider::from_settings(settings);

        if let Some(transport) = &self.transport {
            provider.with_transport(Arc::clone(transport))
        } else {
            provider
        }
    }
}

impl Default for OpenAIProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for OpenAIProvider {
    type LanguageModel = OpenResponsesLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAIImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(OpenAIProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(OpenAIProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(OpenAIProvider::image_model(self, model_id))
    }
}

impl ProviderWithFiles for OpenAIProvider {
    type Files = OpenAIFiles;

    fn files(&self) -> Self::Files {
        OpenAIProvider::files(self)
    }
}

impl ProviderWithSkills for OpenAIProvider {
    type Skills = OpenAISkills;

    fn skills(&self) -> Self::Skills {
        OpenAIProvider::skills(self)
    }
}

impl ImageModel for OpenAIImageModel {
    type MaxImagesPerCallFuture<'a>
        = Pin<Box<dyn Future<Output = Option<usize>> + Send + 'a>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        let max_images = openai_image_max_images_per_call(&self.model_id);
        Box::pin(std::future::ready(Some(max_images)))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        let provider = self.provider.clone();
        let model_id = self.model_id.clone();
        let base_url = self.base_url.clone();
        let mut headers = self.headers.clone();
        let transport = Arc::clone(&self.transport);
        let current_date = self.current_date.as_ref().map(Arc::clone);

        Box::pin(async move {
            let timestamp = current_date
                .as_ref()
                .map(|current_date| current_date())
                .unwrap_or_else(OffsetDateTime::now_utc);
            let warnings = openai_image_warnings(&options);

            if let Some(request_headers) = &options.headers {
                for (name, value) in request_headers {
                    headers.insert(name.clone(), value.clone());
                }
            }

            let response = if options
                .files
                .as_ref()
                .is_some_and(|files| !files.is_empty())
            {
                let form_data = openai_image_edit_form_data(&model_id, &options);
                post_form_data_to_api(
                    PostFormDataToApiOptions::new(format!("{base_url}/images/edits"), form_data)
                        .with_headers(headers.into_iter().map(|(name, value)| (name, Some(value))))
                        .with_optional_abort_signal(options.abort_signal),
                    move |request| transport(request),
                    |request, response| {
                        create_json_response_handler::<OpenAIImageResponse, _, _>(
                            response.json_response_handler_options(request),
                            |value| serde_json::from_value(value.clone()),
                        )
                        .map_err(ProviderApiResponseHandlerError::from)
                    },
                    move |request, response| {
                        Ok(openai_failed_response_handler(&provider, request, response))
                    },
                )
                .await
                .expect("OpenAI image edit failed")
            } else {
                let request_body = openai_image_generation_request_body(&model_id, &options);
                post_json_to_api(
                    PostJsonToApiOptions::new(
                        format!("{base_url}/images/generations"),
                        JsonValue::Object(request_body),
                    )
                    .with_headers(headers.into_iter().map(|(name, value)| (name, Some(value))))
                    .with_optional_abort_signal(options.abort_signal),
                    move |request| transport(request),
                    |request, response| {
                        create_json_response_handler::<OpenAIImageResponse, _, _>(
                            response.json_response_handler_options(request),
                            |value| serde_json::from_value(value.clone()),
                        )
                        .map_err(ProviderApiResponseHandlerError::from)
                    },
                    move |request, response| {
                        Ok(openai_failed_response_handler(&provider, request, response))
                    },
                )
                .await
                .expect("OpenAI image generation failed")
            };

            openai_image_result(model_id, timestamp, response, warnings)
        })
    }
}

impl ProviderWithSpeechModel for OpenAIProvider {
    type SpeechModel = OpenAISpeechModel;

    fn speech_model(&self, model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError> {
        Ok(OpenAIProvider::speech_model(self, model_id))
    }
}

impl SpeechModel for OpenAISpeechModel {
    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = SpeechModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn do_generate(&self, options: SpeechModelCallOptions) -> Self::GenerateFuture<'_> {
        let provider = self.provider.clone();
        let model_id = self.model_id.clone();
        let base_url = self.base_url.clone();
        let mut headers = self.headers.clone();
        let transport = Arc::clone(&self.transport);
        let current_date = self.current_date.as_ref().map(Arc::clone);

        Box::pin(async move {
            let timestamp = current_date
                .as_ref()
                .map(|current_date| current_date())
                .unwrap_or_else(OffsetDateTime::now_utc);
            let (request_body, warnings) = openai_speech_request_body(&model_id, &options);

            if let Some(request_headers) = &options.headers {
                for (name, value) in request_headers {
                    headers.insert(name.clone(), value.clone());
                }
            }

            let response = post_json_to_api(
                PostJsonToApiOptions::new(
                    format!("{base_url}/audio/speech"),
                    JsonValue::Object(request_body.clone()),
                )
                .with_headers(headers.into_iter().map(|(name, value)| (name, Some(value))))
                .with_optional_abort_signal(options.abort_signal),
                move |request| transport(request),
                |request, response| {
                    create_binary_response_handler(
                        response.binary_response_handler_options(request),
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                move |request, response| {
                    Ok(openai_failed_response_handler(&provider, request, response))
                },
            )
            .await
            .expect("OpenAI speech generation failed");

            let mut speech_response = SpeechModelResponse::new(timestamp, model_id.clone());
            if let Some(response_headers) = response.response_headers {
                speech_response.headers = Some(response_headers);
            }

            let mut result =
                SpeechModelResult::new(FileDataContent::Bytes(response.value), speech_response)
                    .with_request(
                        SpeechModelRequest::new().with_body(JsonValue::Object(request_body)),
                    );

            for warning in warnings {
                result = result.with_warning(warning);
            }

            result
        })
    }
}

impl ProviderWithTranscriptionModel for OpenAIProvider {
    type TranscriptionModel = OpenAITranscriptionModel;

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
        Ok(OpenAIProvider::transcription_model(self, model_id))
    }
}

impl TranscriptionModel for OpenAITranscriptionModel {
    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = TranscriptionModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.provider
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn do_generate(&self, options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_> {
        let provider = self.provider.clone();
        let model_id = self.model_id.clone();
        let base_url = self.base_url.clone();
        let mut headers = self.headers.clone();
        let transport = Arc::clone(&self.transport);
        let current_date = self.current_date.as_ref().map(Arc::clone);

        Box::pin(async move {
            let timestamp = current_date
                .as_ref()
                .map(|current_date| current_date())
                .unwrap_or_else(OffsetDateTime::now_utc);
            let (form_data, warnings) = openai_transcription_form_data(&model_id, &options);

            if let Some(request_headers) = &options.headers {
                for (name, value) in request_headers {
                    headers.insert(name.clone(), value.clone());
                }
            }

            let response = post_form_data_to_api(
                PostFormDataToApiOptions::new(
                    format!("{base_url}/audio/transcriptions"),
                    form_data,
                )
                .with_headers(headers.into_iter().map(|(name, value)| (name, Some(value))))
                .with_optional_abort_signal(options.abort_signal),
                move |request| transport(request),
                |request, response| {
                    create_json_response_handler::<OpenAITranscriptionResponse, _, _>(
                        response.json_response_handler_options(request),
                        |value| serde_json::from_value(value.clone()),
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                move |request, response| {
                    Ok(openai_failed_response_handler(&provider, request, response))
                },
            )
            .await
            .expect("OpenAI transcription failed");

            openai_transcription_result(model_id, timestamp, response, warnings)
        })
    }
}

impl Files for OpenAIFiles {
    type UploadFileFuture<'a>
        = Pin<Box<dyn Future<Output = FilesUploadFileResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.provider
    }

    fn upload_file(&self, options: FilesUploadFileCallOptions) -> Self::UploadFileFuture<'_> {
        let provider = self.provider.clone();
        let base_url = self.base_url.clone();
        let headers = self.headers.clone();
        let transport = Arc::clone(&self.transport);

        Box::pin(async move {
            let filename = options.filename.clone();
            let media_type = options.media_type.clone();
            let response = upload_openai_file(provider, base_url, headers, transport, options)
                .await
                .expect("OpenAI file upload failed");

            openai_file_upload_result(response, media_type, filename)
        })
    }
}

impl Skills for OpenAISkills {
    type UploadSkillFuture<'a>
        = Pin<Box<dyn Future<Output = SkillsUploadSkillResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        &self.provider
    }

    fn upload_skill(&self, options: SkillsUploadSkillCallOptions) -> Self::UploadSkillFuture<'_> {
        let provider = self.provider.clone();
        let base_url = self.base_url.clone();
        let headers = self.headers.clone();
        let transport = Arc::clone(&self.transport);

        Box::pin(async move {
            let warnings = openai_skill_upload_warnings(&options);
            let response = upload_openai_skill(provider, base_url, headers, transport, options)
                .await
                .expect("OpenAI skill upload failed");

            openai_skill_upload_result(response, warnings)
        })
    }
}

/// Creates an OpenAI provider with explicit settings.
pub fn create_openai(settings: OpenAIProviderSettings) -> OpenAIProvider {
    OpenAIProvider::from_settings(settings)
}

/// Creates an OpenAI Responses API language model using the default provider settings.
pub fn openai(model_id: impl Into<String>) -> OpenResponsesLanguageModel {
    OpenAIProvider::new().language_model(model_id)
}

fn openai_base_url(settings: &OpenAIProviderSettings) -> String {
    openai_base_url_with_env(settings, || env::var("OPENAI_BASE_URL").ok())
}

fn openai_base_url_with_env(
    settings: &OpenAIProviderSettings,
    env_base_url: impl FnOnce() -> Option<String>,
) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .or_else(|| non_empty_optional_setting(env_base_url()))
        .unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn openai_provider_name(settings: &OpenAIProviderSettings) -> String {
    non_empty_optional_setting(settings.name.clone()).unwrap_or_else(|| "openai".to_string())
}

fn openai_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("OPENAI_API_KEY").ok()))
}

fn openai_headers(settings: &OpenAIProviderSettings) -> Headers {
    let mut headers = Headers::new();

    if let Some(api_key) = openai_api_key(settings.api_key.as_ref()) {
        headers.insert("Authorization".to_string(), format!("Bearer {api_key}"));
    }

    if let Some(organization) = non_empty_optional_setting(settings.organization.clone()) {
        headers.insert("OpenAI-Organization".to_string(), organization);
    }

    if let Some(project) = non_empty_optional_setting(settings.project.clone()) {
        headers.insert("OpenAI-Project".to_string(), project);
    }

    for (name, value) in &settings.headers {
        headers.insert(name.clone(), value.clone());
    }

    headers.insert(
        "user-agent".to_string(),
        format!("ai-sdk/openai/{}", crate::VERSION),
    );
    headers
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn default_openai_files_transport() -> OpenAICompatibleTransport {
    Arc::new(|_| {
        Box::pin(std::future::ready(Err(FetchErrorInfo::new(
            "multipart form data requires an injected OpenAI transport",
        ))))
    })
}

fn openai_image_max_images_per_call(model_id: &str) -> usize {
    match model_id {
        "dall-e-2"
        | "gpt-image-1"
        | "gpt-image-1-mini"
        | "gpt-image-1.5"
        | "gpt-image-2"
        | "chatgpt-image-latest" => 10,
        "dall-e-3" => 1,
        _ => 1,
    }
}

fn openai_image_has_default_response_format(model_id: &str) -> bool {
    [
        "chatgpt-image-",
        "gpt-image-1-mini",
        "gpt-image-1.5",
        "gpt-image-1",
        "gpt-image-2",
    ]
    .iter()
    .any(|prefix| model_id.starts_with(prefix))
}

fn openai_image_warnings(options: &ImageModelCallOptions) -> Vec<Warning> {
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

fn openai_image_generation_request_body(
    model_id: &str,
    options: &ImageModelCallOptions,
) -> JsonObject {
    let mut body = JsonObject::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));

    if let Some(prompt) = &options.prompt {
        body.insert("prompt".to_string(), JsonValue::String(prompt.clone()));
    }
    body.insert("n".to_string(), JsonValue::from(options.n));
    if let Some(size) = &options.size {
        body.insert("size".to_string(), JsonValue::String(size.clone()));
    }

    if let Some(openai_options) = options.provider_options.get("openai") {
        openai_image_insert_option(&mut body, openai_options, "quality", "quality");
        openai_image_insert_option(&mut body, openai_options, "style", "style");
        openai_image_insert_option(&mut body, openai_options, "background", "background");
        openai_image_insert_option(&mut body, openai_options, "moderation", "moderation");
        openai_image_insert_option(&mut body, openai_options, "outputFormat", "output_format");
        openai_image_insert_option(
            &mut body,
            openai_options,
            "outputCompression",
            "output_compression",
        );
        openai_image_insert_option(&mut body, openai_options, "user", "user");
    }

    if !openai_image_has_default_response_format(model_id) {
        body.insert(
            "response_format".to_string(),
            JsonValue::String("b64_json".to_string()),
        );
    }

    body
}

fn openai_image_edit_form_data(model_id: &str, options: &ImageModelCallOptions) -> FormData {
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
                        .map(|file| FormDataValue::bytes(openai_image_file_bytes(file)))
                        .collect(),
                )
            }),
        ),
        (
            "mask".to_string(),
            options
                .mask
                .as_ref()
                .map(|mask| FormDataInputValue::bytes(openai_image_file_bytes(mask))),
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

    if let Some(openai_options) = options.provider_options.get("openai") {
        openai_image_push_form_option(&mut input, openai_options, "quality", "quality");
        openai_image_push_form_option(&mut input, openai_options, "background", "background");
        openai_image_push_form_option(&mut input, openai_options, "outputFormat", "output_format");
        openai_image_push_form_option(
            &mut input,
            openai_options,
            "outputCompression",
            "output_compression",
        );
        openai_image_push_form_option(
            &mut input,
            openai_options,
            "inputFidelity",
            "input_fidelity",
        );
        openai_image_push_form_option(&mut input, openai_options, "user", "user");
    }

    convert_to_form_data(input, ConvertToFormDataOptions::new())
}

fn openai_image_insert_option(
    body: &mut JsonObject,
    options: &JsonObject,
    source: &str,
    target: &str,
) {
    if let Some(value) = options.get(source) {
        body.insert(target.to_string(), value.clone());
    }
}

fn openai_image_push_form_option(
    input: &mut Vec<(String, Option<FormDataInputValue>)>,
    options: &JsonObject,
    source: &str,
    target: &str,
) {
    input.push((
        target.to_string(),
        options
            .get(source)
            .cloned()
            .and_then(openai_image_form_value),
    ));
}

fn openai_image_form_value(value: JsonValue) -> Option<FormDataInputValue> {
    match value {
        JsonValue::Null => None,
        JsonValue::String(value) => Some(FormDataInputValue::text(value)),
        JsonValue::Bool(value) => Some(FormDataInputValue::text(value.to_string())),
        JsonValue::Number(value) => Some(FormDataInputValue::text(value.to_string())),
        JsonValue::Array(values) => Some(FormDataInputValue::array(
            values
                .into_iter()
                .filter_map(|value| {
                    openai_image_form_value(value).and_then(|value| match value {
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

fn openai_image_file_bytes(file: &ImageModelFile) -> Vec<u8> {
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

fn openai_image_result(
    model_id: String,
    timestamp: OffsetDateTime,
    response: ResponseHandlerResult<OpenAIImageResponse>,
    warnings: Vec<Warning>,
) -> ImageModelResult {
    let mut image_response = ImageModelResponse::new(timestamp, model_id);
    if let Some(response_headers) = response.response_headers {
        image_response.headers = Some(response_headers);
    }

    let usage = response.value.usage.as_ref().map(|usage| {
        let mut image_usage = ImageModelUsage::new();
        if let Some(input_tokens) = usage.input_tokens {
            image_usage = image_usage.with_input_tokens(input_tokens);
        }
        if let Some(output_tokens) = usage.output_tokens {
            image_usage = image_usage.with_output_tokens(output_tokens);
        }
        if let Some(total_tokens) = usage.total_tokens {
            image_usage = image_usage.with_total_tokens(total_tokens);
        }
        image_usage
    });
    let provider_metadata = openai_image_provider_metadata(&response.value);
    let mut result = ImageModelResult::new(
        response
            .value
            .data
            .iter()
            .map(|image| FileDataContent::Base64(image.b64_json.clone()))
            .collect(),
        image_response,
    )
    .with_provider_metadata(provider_metadata);

    if let Some(usage) = usage {
        result = result.with_usage(usage);
    }
    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn openai_image_provider_metadata(response: &OpenAIImageResponse) -> ImageModelProviderMetadata {
    let total = response.data.len();
    let images: JsonArray = response
        .data
        .iter()
        .enumerate()
        .map(|(index, image)| {
            let mut metadata = JsonObject::new();
            if let Some(revised_prompt) = image
                .revised_prompt
                .as_ref()
                .filter(|revised_prompt| !revised_prompt.is_empty())
            {
                metadata.insert(
                    "revisedPrompt".to_string(),
                    JsonValue::String(revised_prompt.clone()),
                );
            }
            if let Some(created) = response.created {
                metadata.insert("created".to_string(), JsonValue::from(created));
            }
            if let Some(size) = &response.size {
                metadata.insert("size".to_string(), JsonValue::String(size.clone()));
            }
            if let Some(quality) = &response.quality {
                metadata.insert("quality".to_string(), JsonValue::String(quality.clone()));
            }
            if let Some(background) = &response.background {
                metadata.insert(
                    "background".to_string(),
                    JsonValue::String(background.clone()),
                );
            }
            if let Some(output_format) = &response.output_format {
                metadata.insert(
                    "outputFormat".to_string(),
                    JsonValue::String(output_format.clone()),
                );
            }
            if let Some(details) = response
                .usage
                .as_ref()
                .and_then(|usage| usage.input_tokens_details.as_ref())
            {
                if let Some(image_tokens) =
                    openai_image_distribute_token_count(details.image_tokens, index, total)
                {
                    metadata.insert("imageTokens".to_string(), JsonValue::from(image_tokens));
                }
                if let Some(text_tokens) =
                    openai_image_distribute_token_count(details.text_tokens, index, total)
                {
                    metadata.insert("textTokens".to_string(), JsonValue::from(text_tokens));
                }
            }
            JsonValue::Object(metadata)
        })
        .collect();

    ImageModelProviderMetadata::from([(
        "openai".to_string(),
        ImageModelProviderMetadataEntry::new(images),
    )])
}

fn openai_image_distribute_token_count(
    tokens: Option<u64>,
    index: usize,
    total: usize,
) -> Option<u64> {
    let tokens = tokens?;
    let total = u64::try_from(total).ok().filter(|total| *total > 0)?;
    let base = tokens / total;
    if index + 1 == usize::try_from(total).ok()? {
        Some(tokens - base * (total - 1))
    } else {
        Some(base)
    }
}

async fn upload_openai_file(
    provider: String,
    base_url: String,
    headers: Headers,
    transport: OpenAICompatibleTransport,
    options: FilesUploadFileCallOptions,
) -> Result<OpenAIFileResponse, HandledFetchError> {
    let form_data = openai_file_upload_form_data(&options);
    let response = post_form_data_to_api(
        PostFormDataToApiOptions::new(format!("{base_url}/files"), form_data)
            .with_headers(headers.into_iter().map(|(name, value)| (name, Some(value)))),
        move |request| transport(request),
        |request, response| {
            create_json_response_handler::<OpenAIFileResponse, _, _>(
                response.json_response_handler_options(request),
                |value| serde_json::from_value(value.clone()),
            )
            .map_err(ProviderApiResponseHandlerError::from)
        },
        move |request, response| Ok(openai_failed_response_handler(&provider, request, response)),
    )
    .await?;

    Ok(response.value)
}

fn openai_file_upload_form_data(options: &FilesUploadFileCallOptions) -> FormData {
    let openai_options = options
        .provider_options
        .as_ref()
        .and_then(|options| options.get("openai"));
    let purpose = openai_options
        .and_then(|options| options.get("purpose"))
        .and_then(JsonValue::as_str)
        .unwrap_or("assistants")
        .to_string();

    let expires_after = openai_options
        .and_then(|options| options.get("expiresAfter"))
        .and_then(|value| match value {
            JsonValue::Number(number) => Some(number.to_string()),
            JsonValue::String(value) => Some(value.clone()),
            _ => None,
        });

    let file_bytes = match &options.data {
        FilesUploadFileData::Data { data } => openai_file_upload_data_bytes(data),
        FilesUploadFileData::Text { text } => text.as_bytes().to_vec(),
    };

    let form_data = convert_to_form_data(
        [
            (
                "file".to_string(),
                Some(FormDataInputValue::bytes(file_bytes)),
            ),
            (
                "purpose".to_string(),
                Some(FormDataInputValue::text(purpose)),
            ),
            (
                "expires_after".to_string(),
                expires_after.map(FormDataInputValue::text),
            ),
        ],
        Default::default(),
    );

    if let Some(filename) = &options.filename {
        let mut form_data = form_data;
        form_data.append("filename", FormDataValue::text(filename.clone()));
        form_data
    } else {
        form_data
    }
}

fn openai_file_upload_data_bytes(data: &FileDataContent) -> Vec<u8> {
    match data {
        FileDataContent::Bytes(bytes) => bytes.clone(),
        FileDataContent::Base64(base64) => {
            convert_base64_to_bytes(base64).unwrap_or_else(|_| base64.as_bytes().to_vec())
        }
    }
}

async fn upload_openai_skill(
    provider: String,
    base_url: String,
    headers: Headers,
    transport: OpenAICompatibleTransport,
    options: SkillsUploadSkillCallOptions,
) -> Result<OpenAISkillResponse, HandledFetchError> {
    let form_data = openai_skill_upload_form_data(&options);
    let response = post_form_data_to_api(
        PostFormDataToApiOptions::new(format!("{base_url}/skills"), form_data)
            .with_headers(headers.into_iter().map(|(name, value)| (name, Some(value)))),
        move |request| transport(request),
        |request, response| {
            create_json_response_handler::<OpenAISkillResponse, _, _>(
                response.json_response_handler_options(request),
                |value| serde_json::from_value(value.clone()),
            )
            .map_err(ProviderApiResponseHandlerError::from)
        },
        move |request, response| Ok(openai_failed_response_handler(&provider, request, response)),
    )
    .await?;

    Ok(response.value)
}

fn openai_skill_upload_form_data(options: &SkillsUploadSkillCallOptions) -> FormData {
    let mut form_data = FormData::new();

    for file in &options.files {
        form_data.append(
            "files[]",
            FormDataValue::bytes(openai_skill_file_bytes(&file.data)),
        );
    }

    form_data
}

fn openai_skill_file_bytes(data: &SkillsFileData) -> Vec<u8> {
    match data {
        SkillsFileData::Data { data } => openai_file_upload_data_bytes(data),
        SkillsFileData::Text { text } => text.as_bytes().to_vec(),
    }
}

fn openai_skill_upload_warnings(options: &SkillsUploadSkillCallOptions) -> Vec<Warning> {
    let mut warnings = Vec::new();

    if options.display_title.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "displayTitle".to_string(),
            details: None,
        });
    }

    warnings
}

fn openai_speech_request_body(
    model_id: &str,
    options: &SpeechModelCallOptions,
) -> (JsonObject, Vec<Warning>) {
    let mut request_body = JsonObject::new();
    let mut warnings = Vec::new();

    request_body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    request_body.insert("input".to_string(), JsonValue::String(options.text.clone()));
    request_body.insert(
        "voice".to_string(),
        JsonValue::String(options.voice.clone().unwrap_or_else(|| "alloy".to_string())),
    );

    let mut response_format = "mp3".to_string();
    if let Some(output_format) = &options.output_format {
        if openai_speech_output_format_is_supported(output_format) {
            response_format = output_format.clone();
        } else {
            warnings.push(Warning::Unsupported {
                feature: "outputFormat".to_string(),
                details: Some(format!(
                    "Unsupported output format: {output_format}. Using mp3 instead."
                )),
            });
        }
    }
    request_body.insert(
        "response_format".to_string(),
        JsonValue::String(response_format),
    );

    if let Some(speed) = options.speed {
        request_body.insert("speed".to_string(), JsonValue::from(speed));
    }
    if let Some(instructions) = &options.instructions {
        request_body.insert(
            "instructions".to_string(),
            JsonValue::String(instructions.clone()),
        );
    }
    if let Some(openai_options) = options
        .provider_options
        .as_ref()
        .and_then(|provider_options| provider_options.get("openai"))
    {
        if let Some(JsonValue::String(instructions)) = openai_options.get("instructions") {
            request_body.insert(
                "instructions".to_string(),
                JsonValue::String(instructions.clone()),
            );
        }
        if let Some(speed) = openai_options.get("speed").and_then(JsonValue::as_f64) {
            request_body.insert("speed".to_string(), JsonValue::from(speed));
        }
    }
    if let Some(language) = &options.language {
        warnings.push(Warning::Unsupported {
            feature: "language".to_string(),
            details: Some(format!(
                "OpenAI speech models do not support language selection. Language parameter \"{language}\" was ignored."
            )),
        });
    }

    (request_body, warnings)
}

fn openai_speech_output_format_is_supported(output_format: &str) -> bool {
    matches!(
        output_format,
        "mp3" | "opus" | "aac" | "flac" | "wav" | "pcm"
    )
}

fn openai_transcription_form_data(
    model_id: &str,
    options: &TranscriptionModelCallOptions,
) -> (FormData, Vec<Warning>) {
    let mut form_data = FormData::new();
    form_data.append("model", FormDataValue::text(model_id));
    form_data.append(
        "file",
        FormDataValue::bytes(openai_file_upload_data_bytes(&options.audio)),
    );

    if let Some(openai_options) = options
        .provider_options
        .as_ref()
        .and_then(|provider_options| provider_options.get("openai"))
    {
        if let Some(JsonValue::Array(include)) = openai_options.get("include") {
            for value in include.iter().filter_map(JsonValue::as_str) {
                form_data.append("include[]", FormDataValue::text(value));
            }
        }
        if let Some(language) = openai_options.get("language").and_then(JsonValue::as_str) {
            form_data.append("language", FormDataValue::text(language));
        }
        if let Some(prompt) = openai_options.get("prompt").and_then(JsonValue::as_str) {
            form_data.append("prompt", FormDataValue::text(prompt));
        }

        form_data.append(
            "response_format",
            FormDataValue::text(openai_transcription_response_format(model_id)),
        );

        let temperature = openai_options
            .get("temperature")
            .map(openai_form_data_value)
            .unwrap_or_else(|| "0".to_string());
        form_data.append("temperature", FormDataValue::text(temperature));

        if let Some(JsonValue::Array(granularities)) = openai_options.get("timestampGranularities")
        {
            for value in granularities.iter().filter_map(JsonValue::as_str) {
                form_data.append("timestamp_granularities[]", FormDataValue::text(value));
            }
        }
    }

    let _filename = format!("audio.{}", media_type_to_extension(&options.media_type));

    (form_data, Vec::new())
}

fn openai_transcription_response_format(model_id: &str) -> &'static str {
    if matches!(model_id, "gpt-4o-transcribe" | "gpt-4o-mini-transcribe") {
        "json"
    } else {
        "verbose_json"
    }
}

fn openai_form_data_value(value: &JsonValue) -> String {
    match value {
        JsonValue::String(value) => value.clone(),
        JsonValue::Number(value) => value.to_string(),
        JsonValue::Bool(value) => value.to_string(),
        _ => value.to_string(),
    }
}

fn openai_transcription_result(
    model_id: String,
    timestamp: OffsetDateTime,
    response: ResponseHandlerResult<OpenAITranscriptionResponse>,
    warnings: Vec<Warning>,
) -> TranscriptionModelResult {
    let response_headers = response.response_headers.clone();
    let raw_value = response.raw_value.clone();
    let OpenAITranscriptionResponse {
        text,
        language,
        duration,
        segments,
        words,
    } = response.value;

    let segments = segments
        .map(|segments| {
            segments
                .into_iter()
                .map(|segment| {
                    TranscriptionModelSegment::new(segment.text, segment.start, segment.end)
                })
                .collect()
        })
        .or_else(|| {
            words.map(|words| {
                words
                    .into_iter()
                    .map(|word| TranscriptionModelSegment::new(word.word, word.start, word.end))
                    .collect()
            })
        })
        .unwrap_or_default();

    let mut transcription_response = TranscriptionModelResponse::new(timestamp, model_id);
    if let Some(response_headers) = response_headers {
        transcription_response.headers = Some(response_headers);
    }
    if let Some(raw_value) = raw_value {
        transcription_response.body = Some(raw_value);
    }

    let mut result = TranscriptionModelResult::new(text, segments, transcription_response);
    if let Some(language) = language.and_then(|language| openai_transcription_language(&language)) {
        result = result.with_language(language);
    }
    if let Some(duration) = duration {
        result = result.with_duration_in_seconds(duration);
    }
    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn openai_transcription_language(language: &str) -> Option<String> {
    let code = match language {
        "afrikaans" => "af",
        "arabic" => "ar",
        "armenian" => "hy",
        "azerbaijani" => "az",
        "belarusian" => "be",
        "bosnian" => "bs",
        "bulgarian" => "bg",
        "catalan" => "ca",
        "chinese" => "zh",
        "croatian" => "hr",
        "czech" => "cs",
        "danish" => "da",
        "dutch" => "nl",
        "english" => "en",
        "estonian" => "et",
        "finnish" => "fi",
        "french" => "fr",
        "galician" => "gl",
        "german" => "de",
        "greek" => "el",
        "hebrew" => "he",
        "hindi" => "hi",
        "hungarian" => "hu",
        "icelandic" => "is",
        "indonesian" => "id",
        "italian" => "it",
        "japanese" => "ja",
        "kannada" => "kn",
        "kazakh" => "kk",
        "korean" => "ko",
        "latvian" => "lv",
        "lithuanian" => "lt",
        "macedonian" => "mk",
        "malay" => "ms",
        "marathi" => "mr",
        "maori" => "mi",
        "nepali" => "ne",
        "norwegian" => "no",
        "persian" => "fa",
        "polish" => "pl",
        "portuguese" => "pt",
        "romanian" => "ro",
        "russian" => "ru",
        "serbian" => "sr",
        "slovak" => "sk",
        "slovenian" => "sl",
        "spanish" => "es",
        "swahili" => "sw",
        "swedish" => "sv",
        "tagalog" => "tl",
        "tamil" => "ta",
        "thai" => "th",
        "turkish" => "tr",
        "ukrainian" => "uk",
        "urdu" => "ur",
        "vietnamese" => "vi",
        "welsh" => "cy",
        code if code.len() == 2 => code,
        _ => return None,
    };

    Some(code.to_string())
}

fn openai_failed_response_handler(
    provider: &str,
    request: &crate::provider_utils::ProviderApiRequest,
    response: &crate::provider_utils::ProviderApiResponse,
) -> ResponseHandlerResult<crate::provider::ApiCallError> {
    let message = response
        .text_body()
        .and_then(|body| {
            create_json_response_handler::<OpenAIErrorData, _, _>(
                response.json_response_handler_options(request),
                |value| serde_json::from_value(value.clone()),
            )
            .ok()
            .map(|parsed| parsed.value.error.message)
            .or_else(|| Some(body.to_string()))
        })
        .unwrap_or_else(|| response.status_text.clone());
    let error = crate::provider::ApiCallError::new(
        message,
        request.url.clone(),
        request.request_body_values.clone(),
    )
    .with_status_code(response.status_code)
    .with_response_headers(response.headers.clone())
    .with_data(JsonValue::Object(JsonObject::from_iter([(
        provider.to_string(),
        JsonValue::String(response.status_text.clone()),
    )])));

    ResponseHandlerResult::new(error).with_response_headers(response.headers.clone())
}

fn openai_file_upload_result(
    response: OpenAIFileResponse,
    media_type: String,
    fallback_filename: Option<String>,
) -> FilesUploadFileResult {
    let provider_reference =
        ProviderReference::try_from(BTreeMap::from([("openai".to_string(), response.id)]))
            .expect("OpenAI provider reference is valid");
    let mut metadata = JsonObject::new();

    if let Some(filename) = &response.filename {
        metadata.insert("filename".to_string(), JsonValue::String(filename.clone()));
    }
    if let Some(purpose) = &response.purpose {
        metadata.insert("purpose".to_string(), JsonValue::String(purpose.clone()));
    }
    if let Some(bytes) = response.bytes {
        metadata.insert("bytes".to_string(), JsonValue::from(bytes));
    }
    if let Some(created_at) = response.created_at {
        metadata.insert("createdAt".to_string(), JsonValue::from(created_at));
    }
    if let Some(status) = &response.status {
        metadata.insert("status".to_string(), JsonValue::String(status.clone()));
    }
    if let Some(expires_at) = response.expires_at {
        metadata.insert("expiresAt".to_string(), JsonValue::from(expires_at));
    }

    let mut result = FilesUploadFileResult::new(provider_reference)
        .with_media_type(media_type)
        .with_provider_metadata(ProviderMetadata::from([("openai".to_string(), metadata)]));

    if let Some(filename) = response.filename.or(fallback_filename) {
        result = result.with_filename(filename);
    }

    result
}

fn openai_skill_upload_result(
    response: OpenAISkillResponse,
    warnings: Vec<Warning>,
) -> SkillsUploadSkillResult {
    let provider_reference =
        ProviderReference::try_from(BTreeMap::from([("openai".to_string(), response.id)]))
            .expect("OpenAI skill provider reference is valid");
    let mut metadata = JsonObject::new();

    if let Some(default_version) = &response.default_version {
        metadata.insert(
            "defaultVersion".to_string(),
            JsonValue::String(default_version.clone()),
        );
    }
    if let Some(created_at) = response.created_at {
        metadata.insert("createdAt".to_string(), JsonValue::from(created_at));
    }
    if let Some(updated_at) = response.updated_at {
        metadata.insert("updatedAt".to_string(), JsonValue::from(updated_at));
    }

    let mut result = SkillsUploadSkillResult::new(provider_reference)
        .with_provider_metadata(ProviderMetadata::from([("openai".to_string(), metadata)]));

    if let Some(name) = response.name {
        result = result.with_name(name);
    }
    if let Some(description) = response.description {
        result = result.with_description(description);
    }
    if let Some(latest_version) = response.latest_version {
        result = result.with_latest_version(latest_version);
    }
    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_OPENAI_BASE_URL, OpenAIErrorData, OpenAIProvider, OpenAIProviderSettings,
        create_openai,
    };
    use crate::embed::{EmbedManyOptions, embed_many};
    use crate::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelUsage};
    use crate::file_data::{FileData, FileDataContent, ProviderReference};
    use crate::files::{Files, FilesUploadFileCallOptions, FilesUploadFileData};
    use crate::generate_image::{GenerateImageOptions, GenerateImagePrompt, generate_image};
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::image_model::{ImageModel, ImageModelCallOptions, ImageModelFile, ImageModelUsage};
    use crate::json::JsonValue;
    use crate::language_model::{
        LanguageModel, LanguageModelCallOptions, LanguageModelFilePart, LanguageModelMessage,
        LanguageModelUserContentPart, LanguageModelUserMessage,
    };
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{
        Provider, ProviderOptions, ProviderWithFiles, ProviderWithSkills, ProviderWithSpeechModel,
        ProviderWithTranscriptionModel, SpecificationVersion,
    };
    use crate::provider_utils::{
        FormData, FormDataValue, ParseJsonResult, ProviderApiRequest, ProviderApiRequestBody,
        ProviderApiRequestMethod, ProviderApiResponse, Schema, safe_parse_json_with_schema,
    };
    use crate::skills::{Skills, SkillsFile, SkillsFileData, SkillsUploadSkillCallOptions};
    use crate::speech_model::{SpeechModel, SpeechModelCallOptions};
    use crate::transcription_model::{
        TranscriptionModel, TranscriptionModelCallOptions, TranscriptionModelSegment,
    };
    use crate::warning::Warning;
    use serde_json::{Map, json};
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    #[test]
    fn openai_error_data_schema_should_parse_openrouter_resource_exhausted_error() {
        let error = r#"
{"error":{"message":"{\n  \"error\": {\n    \"code\": 429,\n    \"message\": \"Resource has been exhausted (e.g. check quota).\",\n    \"status\": \"RESOURCE_EXHAUSTED\"\n  }\n}\n","code":429}}
"#;

        let result = safe_parse_json_with_schema::<OpenAIErrorData>(error, Schema::new(Map::new()));
        let expected = OpenAIErrorData {
            error: super::OpenAIErrorDetails {
                message: "{\n  \"error\": {\n    \"code\": 429,\n    \"message\": \"Resource has been exhausted (e.g. check quota).\",\n    \"status\": \"RESOURCE_EXHAUSTED\"\n  }\n}\n"
                    .to_string(),
                error_type: None,
                param: None,
                code: Some(json!(429)),
            },
        };

        assert_eq!(
            result,
            ParseJsonResult::Success {
                value: expected.clone(),
                raw_value: json!({
                    "error": {
                        "message": expected.error.message,
                        "code": 429
                    }
                })
            }
        );
    }

    #[test]
    fn openai_provider_creates_chat_model_with_headers_and_base_url() {
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
                        "id": "chatcmpl-openai",
                        "created": 1711115037,
                        "model": "gpt-4o-mini",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from OpenAI"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 3,
                            "total_tokens": 7
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_openai".to_string(),
                )])))))
            });
        let provider = create_openai(
            OpenAIProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.openai.test/v1/")
                .with_organization("org_test")
                .with_project("proj_test")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.chat("gpt-4o-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(model.provider(), "openai.chat");
        assert_eq!(model.model_id(), "gpt-4o-mini");
        assert_eq!(result.text, "Hello from OpenAI");
        assert!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("openai"))
                .is_some_and(|metadata| metadata.is_empty())
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.openai.test/v1/chat/completions");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request
                .headers
                .get("openai-organization")
                .map(String::as_str),
            Some("org_test")
        );
        assert_eq!(
            request.headers.get("openai-project").map(String::as_str),
            Some("proj_test")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/openai/0.1.0")),
            "OpenAI user-agent suffix is included"
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| !value.contains("ai-sdk/openai-compatible/0.1.0")),
            "OpenAI wrapper overrides the generic OpenAI-compatible suffix"
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "gpt-4o-mini",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "max_tokens": 16,
                "temperature": 0.0
            }))
        );
    }

    #[test]
    fn openai_provider_language_model_uses_responses_endpoint() {
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
                        "id": "resp_openai",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hello from OpenAI Responses"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 4,
                            "output_tokens": 5
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = OpenAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.openai.test/v1/")
            .with_organization("org_test")
            .with_project("proj_test")
            .with_header("custom-header", "value")
            .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid"),
        ));

        assert_eq!(model.provider(), "openai.responses");
        assert_eq!(result.text, "Hello from OpenAI Responses");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.openai.test/v1/responses");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request
                .headers
                .get("openai-organization")
                .map(String::as_str),
            Some("org_test")
        );
        assert_eq!(
            request.headers.get("openai-project").map(String::as_str),
            Some("proj_test")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/openai/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "gpt-4.1-mini",
                "input": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Say hello"
                            }
                        ]
                    }
                ]
            }))
        );
    }

    #[test]
    fn openai_provider_responses_uses_default_file_id_prefix() {
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
                        "id": "resp_openai_file_id_prefix",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "File ids accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 4,
                            "output_tokens": 4
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = OpenAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.openai.test/v1/")
            .with_transport(transport);
        let model = provider.language_model("gpt-4.1-mini");

        let _result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64("file-img-abc123".to_string()),
                    },
                    "image/png",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64("file-pdf-xyz789".to_string()),
                    },
                    "application/pdf",
                )),
            ])),
        ])));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.openai.test/v1/responses");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "gpt-4.1-mini",
                "input": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "input_image",
                                "file_id": "file-img-abc123"
                            },
                            {
                                "type": "input_file",
                                "file_id": "file-pdf-xyz789"
                            }
                        ]
                    }
                ]
            }))
        );
    }

    #[test]
    fn openai_provider_creates_embedding_model_aliases() {
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
                                "object": "embedding",
                                "index": 0,
                                "embedding": [0.1, 0.2]
                            }
                        ],
                        "model": "text-embedding-3-small",
                        "usage": {
                            "prompt_tokens": 1,
                            "total_tokens": 1
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = OpenAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.openai.test/v1/")
            .with_transport(transport);
        let model = provider.embedding("text-embedding-3-small");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "dimensions": 2,
                "user": "user_123"
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(embed_many(
            EmbedManyOptions::new(&model, vec!["hello".to_string()])
                .with_provider_options(provider_options),
        ));

        assert_eq!(model.provider(), "openai.embedding");
        assert_eq!(result.embeddings, vec![vec![0.1, 0.2]]);
        assert_eq!(
            provider
                .embedding_model("text-embedding-3-small")
                .provider(),
            "openai.embedding"
        );
        assert_eq!(
            provider.text_embedding("text-embedding-3-small").provider(),
            "openai.embedding"
        );
        assert_eq!(
            provider
                .text_embedding_model("text-embedding-3-small")
                .provider(),
            "openai.embedding"
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.openai.test/v1/embeddings");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "text-embedding-3-small",
                "input": ["hello"],
                "encoding_format": "float",
                "dimensions": 2,
                "user": "user_123"
            }))
        );
    }

    #[test]
    fn openai_provider_uses_the_default_openai_base_url_when_not_provided() {
        assert_eq!(
            super::openai_base_url_with_env(&OpenAIProviderSettings::new(), || None),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn openai_provider_uses_openai_base_url_when_set() {
        assert_eq!(
            super::openai_base_url_with_env(&OpenAIProviderSettings::new(), || {
                Some("https://proxy.openai.example/v1/".to_string())
            }),
            "https://proxy.openai.example/v1"
        );
    }

    #[test]
    fn openai_provider_prefers_the_base_url_option_over_openai_base_url() {
        assert_eq!(
            super::openai_base_url_with_env(
                &OpenAIProviderSettings::new().with_base_url("https://option.openai.example/v1/"),
                || Some("https://env.openai.example/v1".to_string()),
            ),
            "https://option.openai.example/v1"
        );
    }

    #[test]
    fn openai_embedding_should_extract_embedding() {
        let provider = openai_embedding_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            openai_embedding_fixture(),
            Headers::new(),
        );

        let result = poll_ready(
            provider
                .embedding("text-embedding-3-large")
                .do_embed(openai_embedding_call_options()),
        );

        assert_eq!(
            result.embeddings,
            vec![
                vec![
                    0.0057293195,
                    -0.012727811,
                    0.020042092,
                    -0.013437585,
                    0.022833068
                ],
                vec![
                    -0.037104916,
                    -0.05178114,
                    -0.008340587,
                    0.001164541,
                    -0.0035253682
                ],
            ]
        );
    }

    #[test]
    fn openai_embedding_should_expose_the_raw_response_headers() {
        let provider = openai_embedding_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            openai_embedding_fixture(),
            Headers::from([("test-header".to_string(), "test-value".to_string())]),
        );

        let result = poll_ready(
            provider
                .embedding("text-embedding-3-large")
                .do_embed(openai_embedding_call_options()),
        );

        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("test-header")),
            Some(&"test-value".to_string())
        );
    }

    #[test]
    fn openai_embedding_should_expose_the_raw_response_body() {
        let response_body = openai_embedding_fixture();
        let provider = openai_embedding_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            response_body.clone(),
            Headers::new(),
        );

        let result = poll_ready(
            provider
                .embedding("text-embedding-3-large")
                .do_embed(openai_embedding_call_options()),
        );

        assert_eq!(
            result.response.and_then(|response| response.body),
            Some(response_body)
        );
    }

    #[test]
    fn openai_embedding_should_extract_usage() {
        let provider = openai_embedding_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            openai_embedding_fixture(),
            Headers::new(),
        );

        let result = poll_ready(
            provider
                .embedding("text-embedding-3-large")
                .do_embed(openai_embedding_call_options()),
        );

        assert_eq!(result.usage, Some(EmbeddingModelUsage::new(12)));
    }

    #[test]
    fn openai_embedding_should_pass_the_model_and_the_values() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_embedding_test_provider(
            Arc::clone(&captured_requests),
            openai_embedding_fixture(),
            Headers::new(),
        );

        let _result = poll_ready(
            provider
                .embedding("text-embedding-3-large")
                .do_embed(openai_embedding_call_options()),
        );

        assert_eq!(
            captured_json_body(&captured_requests),
            json!({
                "encoding_format": "float",
                "input": ["sunny day at the beach", "rainy day in the city"],
                "model": "text-embedding-3-large"
            })
        );
    }

    #[test]
    fn openai_embedding_should_pass_the_dimensions_setting() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_embedding_test_provider(
            Arc::clone(&captured_requests),
            openai_embedding_fixture(),
            Headers::new(),
        );
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": { "dimensions": 64 }
        }))
        .expect("provider options deserialize");

        let _result = poll_ready(
            provider
                .embedding("text-embedding-3-large")
                .do_embed(openai_embedding_call_options().with_provider_options(provider_options)),
        );

        assert_eq!(
            captured_json_body(&captured_requests),
            json!({
                "dimensions": 64,
                "encoding_format": "float",
                "input": ["sunny day at the beach", "rainy day in the city"],
                "model": "text-embedding-3-large"
            })
        );
    }

    #[test]
    fn openai_embedding_should_pass_headers() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_embedding_test_provider(
            Arc::clone(&captured_requests),
            openai_embedding_fixture(),
            Headers::new(),
        )
        .with_organization("test-organization")
        .with_project("test-project")
        .with_header("Custom-Provider-Header", "provider-header-value");

        let _result = poll_ready(
            provider.embedding("text-embedding-3-large").do_embed(
                openai_embedding_call_options()
                    .with_header("Custom-Request-Header", "request-header-value"),
            ),
        );

        let request = captured_request(&captured_requests);
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request
                .headers
                .get("openai-organization")
                .map(String::as_str),
            Some("test-organization")
        );
        assert_eq!(
            request.headers.get("openai-project").map(String::as_str),
            Some("test-project")
        );
        assert_eq!(
            request
                .headers
                .get("custom-provider-header")
                .map(String::as_str),
            Some("provider-header-value")
        );
        assert_eq!(
            request
                .headers
                .get("custom-request-header")
                .map(String::as_str),
            Some("request-header-value")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/openai/"))
        );
    }

    #[test]
    fn openai_provider_creates_completion_and_image_models() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_requests_for_transport
                    .lock()
                    .expect("captured requests mutex is not poisoned")
                    .push(request.clone());

                let response = if request.url.ends_with("/completions") {
                    json!({
                        "id": "cmpl-openai",
                        "created": 1711115037,
                        "model": "gpt-3.5-turbo-instruct",
                        "choices": [
                            {
                                "index": 0,
                                "text": " completion",
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 2,
                            "completion_tokens": 1,
                            "total_tokens": 3
                        }
                    })
                } else {
                    json!({
                        "created": 1711115037,
                        "data": [
                            {
                                "b64_json": "aW1hZ2UtYnl0ZXM="
                            }
                        ]
                    })
                };

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    response.to_string(),
                ))))
            });
        let provider = OpenAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.openai.test/v1/")
            .with_transport(transport);

        let completion_model = provider.completion("gpt-3.5-turbo-instruct");
        let completion_result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&completion_model, Prompt::from_prompt("Complete"))
                .expect("prompt is valid")
                .with_max_output_tokens(8),
        ));
        let image_model = provider.image("gpt-image-1");
        let image_result = poll_ready(generate_image(
            GenerateImageOptions::new(
                &image_model,
                GenerateImagePrompt::text("A small watercolor robot"),
            )
            .with_n(1),
        ))
        .expect("image generation succeeds");

        assert_eq!(completion_model.provider(), "openai.completion");
        assert_eq!(completion_result.text, " completion");
        assert_eq!(image_model.provider(), "openai.image");
        assert_eq!(image_result.images.len(), 1);

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].url, "https://api.openai.test/v1/completions");
        assert_eq!(
            requests[1].url,
            "https://api.openai.test/v1/images/generations"
        );
    }

    #[test]
    fn openai_image_should_pass_the_model_and_the_settings() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider =
            openai_image_test_provider(Arc::clone(&captured_requests), openai_image_fixture());
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": { "style": "vivid" }
        }))
        .expect("provider options deserialize");

        let _result = poll_ready(
            provider.image("dall-e-3").do_generate(
                openai_image_call_options()
                    .with_size("1024x1024")
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(
            captured_json_body(&captured_requests),
            json!({
                "model": "dall-e-3",
                "prompt": "A cute baby sea otter",
                "n": 1,
                "size": "1024x1024",
                "style": "vivid",
                "response_format": "b64_json"
            })
        );
    }

    #[test]
    fn openai_image_should_map_provider_options_to_snake_case_for_images_generations() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider =
            openai_image_test_provider(Arc::clone(&captured_requests), openai_image_fixture());
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "quality": "high",
                "background": "transparent",
                "moderation": "low",
                "outputFormat": "webp",
                "outputCompression": 80,
                "user": "user-123"
            }
        }))
        .expect("provider options deserialize");

        let _result = poll_ready(
            provider.image("gpt-image-1").do_generate(
                openai_image_call_options()
                    .with_size("1024x1024")
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(
            captured_json_body(&captured_requests),
            json!({
                "model": "gpt-image-1",
                "prompt": "A cute baby sea otter",
                "n": 1,
                "size": "1024x1024",
                "quality": "high",
                "background": "transparent",
                "moderation": "low",
                "output_format": "webp",
                "output_compression": 80,
                "user": "user-123"
            })
        );
    }

    #[test]
    fn openai_image_should_pass_headers() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider =
            openai_image_test_provider(Arc::clone(&captured_requests), openai_image_fixture())
                .with_organization("test-organization")
                .with_project("test-project")
                .with_header("Custom-Provider-Header", "provider-header-value");

        let _result = poll_ready(
            provider.image("dall-e-3").do_generate(
                openai_image_call_options()
                    .with_header("Custom-Request-Header", "request-header-value"),
            ),
        );

        let request = captured_request(&captured_requests);
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request
                .headers
                .get("openai-organization")
                .map(String::as_str),
            Some("test-organization")
        );
        assert_eq!(
            request.headers.get("openai-project").map(String::as_str),
            Some("test-project")
        );
        assert_eq!(
            request
                .headers
                .get("custom-provider-header")
                .map(String::as_str),
            Some("provider-header-value")
        );
        assert_eq!(
            request
                .headers
                .get("custom-request-header")
                .map(String::as_str),
            Some("request-header-value")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/openai/"))
        );
    }

    #[test]
    fn openai_image_should_extract_the_generated_images() {
        let provider =
            openai_image_test_provider(Arc::new(Mutex::new(Vec::new())), openai_image_fixture());

        let result = poll_ready(
            provider
                .image("dall-e-3")
                .do_generate(openai_image_call_options()),
        );

        assert_eq!(
            result.images,
            vec![
                FileDataContent::Base64("base64-image-1".to_string()),
                FileDataContent::Base64("base64-image-2".to_string())
            ]
        );
    }

    #[test]
    fn openai_image_should_return_warnings_for_unsupported_settings() {
        let provider =
            openai_image_test_provider(Arc::new(Mutex::new(Vec::new())), openai_image_fixture());

        let result = poll_ready(
            provider.image("dall-e-3").do_generate(
                openai_image_call_options()
                    .with_aspect_ratio("1:1")
                    .with_seed(123),
            ),
        );

        assert_eq!(
            result.warnings,
            vec![
                Warning::Unsupported {
                    feature: "aspectRatio".to_string(),
                    details: Some(
                        "This model does not support aspect ratio. Use `size` instead.".to_string()
                    )
                },
                Warning::Unsupported {
                    feature: "seed".to_string(),
                    details: None
                }
            ]
        );
    }

    #[test]
    fn openai_image_should_respect_max_images_per_call_setting() {
        let provider = OpenAIProvider::new();

        assert_eq!(
            poll_ready(provider.image("dall-e-2").max_images_per_call()),
            Some(10)
        );
        assert_eq!(
            poll_ready(provider.image("unknown-model").max_images_per_call()),
            Some(1)
        );
    }

    #[test]
    fn openai_image_should_include_response_data_with_timestamp_model_id_and_headers() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_image_test_provider_with_headers(
            Arc::clone(&captured_requests),
            openai_image_fixture(),
            Headers::from([
                ("x-request-id".to_string(), "test-request-id".to_string()),
                ("x-ratelimit-remaining".to_string(), "123".to_string()),
            ]),
        );
        let test_date =
            OffsetDateTime::parse("2024-03-15T12:00:00Z", &Rfc3339).expect("test date parses");

        let result = poll_ready(
            provider
                .image("dall-e-3")
                .with_current_date(move || test_date)
                .do_generate(openai_image_call_options()),
        );

        assert_eq!(result.response.timestamp, test_date);
        assert_eq!(result.response.model_id, "dall-e-3");
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id")),
            Some(&"test-request-id".to_string())
        );
    }

    #[test]
    fn openai_image_should_use_real_date_when_no_custom_date_provider_is_specified() {
        let provider =
            openai_image_test_provider(Arc::new(Mutex::new(Vec::new())), openai_image_fixture());
        let before_date = OffsetDateTime::now_utc();

        let result = poll_ready(
            provider
                .image("dall-e-3")
                .do_generate(openai_image_call_options()),
        );

        let after_date = OffsetDateTime::now_utc();
        assert!(result.response.timestamp >= before_date);
        assert!(result.response.timestamp <= after_date);
        assert_eq!(result.response.model_id, "dall-e-3");
    }

    #[test]
    fn openai_image_should_not_include_response_format_for_gpt_image_1() {
        assert_openai_image_model_omits_response_format("gpt-image-1");
    }

    #[test]
    fn openai_image_should_not_include_response_format_for_gpt_image_2() {
        assert_openai_image_model_omits_response_format("gpt-image-2");
    }

    #[test]
    fn openai_image_should_not_include_response_format_for_chatgpt_image_latest() {
        assert_openai_image_model_omits_response_format("chatgpt-image-latest");
    }

    #[test]
    fn openai_image_should_not_include_response_format_for_date_suffixed_gpt_image_model_ids() {
        assert_openai_image_model_omits_response_format("gpt-image-1.5-2025-12-16");
    }

    #[test]
    fn openai_image_should_handle_null_revised_prompt_responses() {
        let provider = openai_image_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            json!({
                "created": 1733837122_u64,
                "data": [
                    {
                        "revised_prompt": null,
                        "b64_json": "base64-image-1"
                    }
                ]
            }),
        );

        let result = poll_ready(
            provider
                .image("gpt-image-1")
                .do_generate(openai_image_call_options()),
        );

        assert_eq!(
            result.images,
            vec![FileDataContent::Base64("base64-image-1".to_string())]
        );
        assert!(result.warnings.is_empty());
        let metadata = result.provider_metadata.expect("provider metadata exists");
        assert_eq!(
            metadata["openai"].images,
            vec![json!({
                "created": 1733837122_u64
            })]
        );
    }

    #[test]
    fn openai_image_should_include_response_format_for_dall_e_3() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider =
            openai_image_test_provider(Arc::clone(&captured_requests), openai_image_fixture());

        let _result = poll_ready(
            provider
                .image("dall-e-3")
                .do_generate(openai_image_call_options()),
        );

        assert_eq!(
            captured_json_body(&captured_requests)
                .get("response_format")
                .and_then(JsonValue::as_str),
            Some("b64_json")
        );
    }

    #[test]
    fn openai_image_should_return_image_meta_data() {
        let provider =
            openai_image_test_provider(Arc::new(Mutex::new(Vec::new())), openai_image_fixture());

        let result = poll_ready(
            provider
                .image("dall-e-3")
                .do_generate(openai_image_call_options()),
        );

        let metadata = result.provider_metadata.expect("provider metadata exists");
        assert_eq!(
            metadata["openai"].images,
            vec![
                json!({
                    "revisedPrompt": "A small and adorable baby sea otter.",
                    "created": 1770935200_u64,
                    "size": "1024x1024",
                    "quality": "hd",
                    "background": "transparent",
                    "outputFormat": "png"
                }),
                json!({
                    "created": 1770935200_u64,
                    "size": "1024x1024",
                    "quality": "hd",
                    "background": "transparent",
                    "outputFormat": "png"
                })
            ]
        );
    }

    #[test]
    fn openai_image_should_map_openai_usage_to_usage() {
        let provider = openai_image_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            json!({
                "created": 1733837122_u64,
                "data": [{ "b64_json": "base64-image-1" }],
                "usage": {
                    "input_tokens": 12,
                    "output_tokens": 0,
                    "total_tokens": 12,
                    "input_tokens_details": {
                        "image_tokens": 7,
                        "text_tokens": 5
                    }
                }
            }),
        );

        let result = poll_ready(
            provider
                .image("gpt-image-1")
                .do_generate(openai_image_call_options()),
        );

        assert_eq!(
            result.usage,
            Some(
                ImageModelUsage::new()
                    .with_input_tokens(12)
                    .with_output_tokens(0)
                    .with_total_tokens(12)
            )
        );
        let metadata = result.provider_metadata.expect("provider metadata exists");
        assert_eq!(
            metadata["openai"].images,
            vec![json!({
                "created": 1733837122_u64,
                "imageTokens": 7,
                "textTokens": 5
            })]
        );
    }

    #[test]
    fn openai_image_should_distribute_input_token_details_evenly_across_images() {
        let provider = openai_image_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            json!({
                "created": 1733837122_u64,
                "data": [
                    { "b64_json": "base64-image-1" },
                    { "b64_json": "base64-image-2" },
                    { "b64_json": "base64-image-3" }
                ],
                "usage": {
                    "input_tokens": 30,
                    "output_tokens": 900,
                    "total_tokens": 930,
                    "input_tokens_details": {
                        "image_tokens": 194,
                        "text_tokens": 28
                    }
                }
            }),
        );

        let result = poll_ready(
            provider
                .image("gpt-image-1")
                .do_generate(openai_image_call_options_with_n(3)),
        );

        let metadata = result.provider_metadata.expect("provider metadata exists");
        assert_eq!(
            metadata["openai"].images,
            vec![
                json!({ "created": 1733837122_u64, "imageTokens": 64, "textTokens": 9 }),
                json!({ "created": 1733837122_u64, "imageTokens": 64, "textTokens": 9 }),
                json!({ "created": 1733837122_u64, "imageTokens": 66, "textTokens": 10 })
            ]
        );
    }

    #[test]
    fn openai_image_should_call_images_edits_endpoint_when_files_are_provided() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider =
            openai_image_test_provider(Arc::clone(&captured_requests), openai_image_edit_fixture());

        let _result = poll_ready(
            provider
                .image("gpt-image-1")
                .do_generate(openai_image_edit_call_options()),
        );

        assert_eq!(
            captured_request(&captured_requests).url,
            "https://api.openai.test/v1/images/edits"
        );
    }

    #[test]
    fn openai_image_should_send_image_as_form_data_with_uint8array_input() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider =
            openai_image_test_provider(Arc::clone(&captured_requests), openai_image_edit_fixture());

        let _result = poll_ready(
            provider
                .image("gpt-image-1")
                .do_generate(openai_image_edit_call_options().with_size("1024x1024")),
        );

        let form_data = captured_form_data(&captured_requests);
        assert_eq!(
            form_data.get("model"),
            Some(&FormDataValue::text("gpt-image-1"))
        );
        assert_eq!(
            form_data.get("prompt"),
            Some(&FormDataValue::text("A cute baby sea otter"))
        );
        assert_eq!(form_data.get("n"), Some(&FormDataValue::text("1")));
        assert_eq!(
            form_data.get("size"),
            Some(&FormDataValue::text("1024x1024"))
        );
        assert_eq!(
            form_data.get("image"),
            Some(&FormDataValue::bytes(vec![137, 80, 78, 71]))
        );
    }

    #[test]
    fn openai_image_should_send_image_as_form_data_with_base64_string_input() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider =
            openai_image_test_provider(Arc::clone(&captured_requests), openai_image_edit_fixture());

        let _result = poll_ready(provider.image("gpt-image-1").do_generate(
            openai_image_call_options().with_files(vec![ImageModelFile::file(
                "image/png",
                FileDataContent::Base64("iVBORw0KGgo=".to_string()),
            )]),
        ));

        let form_data = captured_form_data(&captured_requests);
        assert_eq!(
            form_data.get("model"),
            Some(&FormDataValue::text("gpt-image-1"))
        );
        assert!(form_data.get("image").is_some());
    }

    #[test]
    fn openai_image_should_send_multiple_images_as_form_data_array() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider =
            openai_image_test_provider(Arc::clone(&captured_requests), openai_image_edit_fixture());

        let _result = poll_ready(provider.image("gpt-image-1").do_generate(
            openai_image_call_options().with_files(vec![
                ImageModelFile::file("image/png", FileDataContent::Bytes(vec![137, 80, 78, 71])),
                ImageModelFile::file(
                    "image/jpeg",
                    FileDataContent::Bytes(vec![255, 216, 255, 224]),
                ),
            ]),
        ));

        let form_data = captured_form_data(&captured_requests);
        assert!(form_data.has("image[]"));
        assert_eq!(form_data.get_all("image[]").len(), 2);
    }

    #[test]
    fn openai_image_should_pass_provider_options_in_form_data() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider =
            openai_image_test_provider(Arc::clone(&captured_requests), openai_image_edit_fixture());
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "quality": "high",
                "background": "transparent"
            }
        }))
        .expect("provider options deserialize");

        let _result =
            poll_ready(provider.image("gpt-image-1").do_generate(
                openai_image_edit_call_options().with_provider_options(provider_options),
            ));

        let form_data = captured_form_data(&captured_requests);
        assert_eq!(form_data.get("quality"), Some(&FormDataValue::text("high")));
        assert_eq!(
            form_data.get("background"),
            Some(&FormDataValue::text("transparent"))
        );
    }

    #[test]
    fn openai_image_should_map_provider_options_to_snake_case_for_images_edits() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider =
            openai_image_test_provider(Arc::clone(&captured_requests), openai_image_edit_fixture());
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "inputFidelity": "high",
                "outputFormat": "webp",
                "outputCompression": 80,
                "user": "user-123"
            }
        }))
        .expect("provider options deserialize");

        let _result =
            poll_ready(provider.image("gpt-image-1").do_generate(
                openai_image_edit_call_options().with_provider_options(provider_options),
            ));

        let form_data = captured_form_data(&captured_requests);
        assert_eq!(
            form_data.get("input_fidelity"),
            Some(&FormDataValue::text("high"))
        );
        assert_eq!(
            form_data.get("output_format"),
            Some(&FormDataValue::text("webp"))
        );
        assert_eq!(
            form_data.get("output_compression"),
            Some(&FormDataValue::text("80"))
        );
        assert_eq!(
            form_data.get("user"),
            Some(&FormDataValue::text("user-123"))
        );
    }

    #[test]
    fn openai_image_should_extract_the_edited_images_from_response() {
        let provider = openai_image_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            openai_image_edit_fixture(),
        );

        let result = poll_ready(
            provider
                .image("gpt-image-1")
                .do_generate(openai_image_edit_call_options()),
        );

        assert_eq!(
            result.images,
            vec![FileDataContent::Base64("edited-base64-image-1".to_string())]
        );
    }

    #[test]
    fn openai_image_should_include_response_metadata_for_edited_images() {
        let provider = openai_image_test_provider_with_headers(
            Arc::new(Mutex::new(Vec::new())),
            openai_image_edit_fixture(),
            Headers::from([("x-request-id".to_string(), "edit-request-id".to_string())]),
        );
        let test_date =
            OffsetDateTime::parse("2024-03-15T12:00:00Z", &Rfc3339).expect("test date parses");

        let result = poll_ready(
            provider
                .image("gpt-image-1")
                .with_current_date(move || test_date)
                .do_generate(openai_image_edit_call_options()),
        );

        assert_eq!(result.response.timestamp, test_date);
        assert_eq!(result.response.model_id, "gpt-image-1");
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id")),
            Some(&"edit-request-id".to_string())
        );
    }

    #[test]
    fn openai_image_should_return_warnings_for_unsupported_settings_in_edit_mode() {
        let provider = openai_image_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            openai_image_edit_fixture(),
        );

        let result = poll_ready(
            provider.image("gpt-image-1").do_generate(
                openai_image_edit_call_options()
                    .with_aspect_ratio("16:9")
                    .with_seed(42),
            ),
        );

        assert_eq!(result.warnings.len(), 2);
        assert_eq!(
            result.warnings[0],
            Warning::Unsupported {
                feature: "aspectRatio".to_string(),
                details: Some(
                    "This model does not support aspect ratio. Use `size` instead.".to_string()
                )
            }
        );
        assert_eq!(
            result.warnings[1],
            Warning::Unsupported {
                feature: "seed".to_string(),
                details: None
            }
        );
    }

    #[test]
    fn openai_image_should_return_usage_information_for_edited_images() {
        let provider = openai_image_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            json!({
                "created": 1733837122_u64,
                "data": [{ "b64_json": "edited-base64-image-1" }],
                "usage": {
                    "input_tokens": 25,
                    "output_tokens": 0,
                    "total_tokens": 25
                }
            }),
        );

        let result = poll_ready(
            provider
                .image("gpt-image-1")
                .do_generate(openai_image_edit_call_options()),
        );

        assert_eq!(
            result.usage,
            Some(
                ImageModelUsage::new()
                    .with_input_tokens(25)
                    .with_output_tokens(0)
                    .with_total_tokens(25)
            )
        );
    }

    #[test]
    fn openai_files_should_send_correct_multipart_request_with_purpose() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_files_test_provider(Arc::clone(&captured_requests));
        let files = provider.files();
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "purpose": "fine-tune"
            }
        }))
        .expect("provider options deserialize");

        let _result = poll_ready(
            files.upload_file(
                FilesUploadFileCallOptions::new(
                    FilesUploadFileData::text("training row"),
                    "text/plain",
                )
                .with_filename("training.jsonl")
                .with_provider_options(provider_options),
            ),
        );

        let request = captured_openai_files_request(&captured_requests);
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.openai.test/v1/files");
        let form_data = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("form data body is captured");
        assert_eq!(
            form_data.get("file"),
            Some(&FormDataValue::bytes(b"training row".to_vec()))
        );
        assert_eq!(
            form_data.get("purpose"),
            Some(&FormDataValue::text("fine-tune"))
        );
        assert_eq!(
            form_data.get("filename"),
            Some(&FormDataValue::text("training.jsonl"))
        );
    }

    #[test]
    fn openai_files_should_return_provider_reference_with_openai_key() {
        let provider = openai_files_test_provider(Arc::new(Mutex::new(Vec::new())));
        let result = poll_ready(
            provider
                .files()
                .upload_file(FilesUploadFileCallOptions::new(
                    FilesUploadFileData::text("file content"),
                    "text/plain",
                )),
        );

        assert_eq!(
            result.provider_reference,
            ProviderReference::try_from(std::collections::BTreeMap::from([(
                "openai".to_string(),
                "file-openai-upload".to_string()
            )]))
            .expect("provider reference is valid")
        );
    }

    #[test]
    fn openai_files_should_return_provider_metadata_from_response() {
        let provider = openai_files_test_provider(Arc::new(Mutex::new(Vec::new())));
        let result = poll_ready(
            provider
                .files()
                .upload_file(FilesUploadFileCallOptions::new(
                    FilesUploadFileData::text("file content"),
                    "text/plain",
                )),
        );

        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("openai")),
            Some(
                &json!({
                    "filename": "uploaded.jsonl",
                    "purpose": "assistants",
                    "bytes": 12,
                    "createdAt": 1711115037,
                    "status": "processed",
                    "expiresAt": 1711125037
                })
                .as_object()
                .expect("metadata is an object")
                .clone()
            )
        );
        assert_eq!(result.filename.as_deref(), Some("uploaded.jsonl"));
        assert_eq!(result.media_type.as_deref(), Some("text/plain"));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn openai_files_should_default_purpose_to_assistants_when_not_provided() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_files_test_provider(Arc::clone(&captured_requests));

        let _result = poll_ready(
            provider
                .files()
                .upload_file(FilesUploadFileCallOptions::new(
                    FilesUploadFileData::text("file content"),
                    "text/plain",
                )),
        );

        let request = captured_openai_files_request(&captured_requests);
        let form_data = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("form data body is captured");
        assert_eq!(
            form_data.get("purpose"),
            Some(&FormDataValue::text("assistants"))
        );
    }

    #[test]
    fn openai_files_should_pass_expires_after_when_provided() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_files_test_provider(Arc::clone(&captured_requests));
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "expiresAfter": 3600
            }
        }))
        .expect("provider options deserialize");

        let _result = poll_ready(
            provider.files().upload_file(
                FilesUploadFileCallOptions::new(
                    FilesUploadFileData::text("file content"),
                    "text/plain",
                )
                .with_provider_options(provider_options),
            ),
        );

        let request = captured_openai_files_request(&captured_requests);
        let form_data = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("form data body is captured");
        assert_eq!(
            form_data.get("expires_after"),
            Some(&FormDataValue::text("3600"))
        );
    }

    #[test]
    fn openai_files_should_pass_auth_headers() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_files_test_provider(Arc::clone(&captured_requests));

        let _result = poll_ready(
            provider
                .files()
                .upload_file(FilesUploadFileCallOptions::new(
                    FilesUploadFileData::text("file content"),
                    "text/plain",
                )),
        );

        let request = captured_openai_files_request(&captured_requests);
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request
                .headers
                .get("openai-organization")
                .map(String::as_str),
            Some("org_test")
        );
        assert_eq!(
            request.headers.get("openai-project").map(String::as_str),
            Some("proj_test")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/openai/0.1.0"))
        );
    }

    #[test]
    fn openai_files_should_handle_base64_string_data() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_files_test_provider(Arc::clone(&captured_requests));

        let _result = poll_ready(
            provider
                .files()
                .upload_file(FilesUploadFileCallOptions::new(
                    FilesUploadFileData::data(FileDataContent::Base64(
                        "aGVsbG8gd29ybGQ=".to_string(),
                    )),
                    "text/plain",
                )),
        );

        let request = captured_openai_files_request(&captured_requests);
        let form_data = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("form data body is captured");
        assert_eq!(
            form_data.get("file"),
            Some(&FormDataValue::bytes(b"hello world".to_vec()))
        );
    }

    #[test]
    fn openai_files_should_set_specification_version_and_provider() {
        let provider = openai_files_test_provider(Arc::new(Mutex::new(Vec::new())));
        let files = ProviderWithFiles::files(&provider);

        assert_eq!(files.specification_version(), SpecificationVersion::V4);
        assert_eq!(files.provider(), "openai.files");
    }

    #[test]
    fn openai_skills_should_send_files_as_multipart_form_data() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_skills_test_provider(Arc::clone(&captured_requests));

        let _result = poll_ready(provider.skills().upload_skill(openai_skill_upload_options(
            SkillsFileData::data(FileDataContent::Base64(
                "Y29uc29sZS5sb2coImhlbGxvIik=".to_string(),
            )),
        )));

        let request = captured_openai_request(&captured_requests);
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.openai.test/v1/skills");
        let form_data = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("form data body is captured");
        assert_eq!(
            form_data.get("files[]"),
            Some(&FormDataValue::bytes(b"console.log(\"hello\")".to_vec()))
        );
    }

    #[test]
    fn openai_skills_should_pass_authorization_headers() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_skills_test_provider(Arc::clone(&captured_requests));

        let _result = poll_ready(provider.skills().upload_skill(openai_skill_upload_options(
            SkillsFileData::text("console.log(\"hello\")"),
        )));

        let request = captured_openai_request(&captured_requests);
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request
                .headers
                .get("openai-organization")
                .map(String::as_str),
            Some("org_test")
        );
        assert_eq!(
            request.headers.get("openai-project").map(String::as_str),
            Some("proj_test")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
    }

    #[test]
    fn openai_skills_should_map_response_to_provider_reference() {
        let provider = openai_skills_test_provider(Arc::new(Mutex::new(Vec::new())));
        let result = poll_ready(provider.skills().upload_skill(openai_skill_upload_options(
            SkillsFileData::text("console.log(\"hello\")"),
        )));

        assert_eq!(
            result.provider_reference,
            ProviderReference::try_from(std::collections::BTreeMap::from([(
                "openai".to_string(),
                "skill_699fc58f408c8191825d8d06ae75fd5c06de7b381a5db7f5".to_string()
            )]))
            .expect("provider reference is valid")
        );
        assert_eq!(result.name.as_deref(), Some("test-capture-skill"));
        assert_eq!(
            result.description.as_deref(),
            Some("A test skill for fixture capture")
        );
        assert_eq!(result.latest_version.as_deref(), Some("1"));
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("openai")),
            Some(
                &json!({
                    "defaultVersion": "1",
                    "createdAt": 1772078479_u64
                })
                .as_object()
                .expect("metadata is an object")
                .clone()
            )
        );
    }

    #[test]
    fn openai_skills_should_emit_unsupported_warning_for_display_title() {
        let provider = openai_skills_test_provider(Arc::new(Mutex::new(Vec::new())));
        let result = poll_ready(
            provider.skills().upload_skill(
                openai_skill_upload_options(SkillsFileData::text("console.log(\"hello\")"))
                    .with_display_title("My Skill"),
            ),
        );

        assert_eq!(
            result.warnings,
            vec![Warning::Unsupported {
                feature: "displayTitle".to_string(),
                details: None
            }]
        );
    }

    #[test]
    fn openai_skills_should_return_no_warnings_when_display_title_is_not_set() {
        let provider = openai_skills_test_provider(Arc::new(Mutex::new(Vec::new())));
        let result = poll_ready(provider.skills().upload_skill(openai_skill_upload_options(
            SkillsFileData::text("console.log(\"hello\")"),
        )));

        assert!(result.warnings.is_empty());
    }

    #[test]
    fn openai_skills_should_handle_uint8array_file_content() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_skills_test_provider(Arc::clone(&captured_requests));
        let result = poll_ready(provider.skills().upload_skill(openai_skill_upload_options(
            SkillsFileData::data(FileDataContent::Bytes(b"Hello".to_vec())),
        )));

        let request = captured_openai_request(&captured_requests);
        let form_data = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("form data body is captured");
        assert_eq!(
            form_data.get("files[]"),
            Some(&FormDataValue::bytes(b"Hello".to_vec()))
        );
        assert_eq!(
            result.provider_reference,
            ProviderReference::try_from(std::collections::BTreeMap::from([(
                "openai".to_string(),
                "skill_699fc58f408c8191825d8d06ae75fd5c06de7b381a5db7f5".to_string()
            )]))
            .expect("provider reference is valid")
        );
    }

    #[test]
    fn openai_skills_should_set_specification_version_and_provider() {
        let provider = openai_skills_test_provider(Arc::new(Mutex::new(Vec::new())));
        let skills = ProviderWithSkills::skills(&provider);

        assert_eq!(skills.specification_version(), SpecificationVersion::V4);
        assert_eq!(skills.provider(), "openai.skills");
    }

    #[test]
    fn openai_speech_should_pass_the_model_and_text() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_speech_test_provider(Arc::clone(&captured_requests), "mp3");

        let _result = poll_ready(
            provider
                .speech("tts-1")
                .do_generate(SpeechModelCallOptions::new("Hello from the AI SDK!")),
        );

        let request = captured_openai_request(&captured_requests);
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.openai.test/v1/audio/speech");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "tts-1",
                "input": "Hello from the AI SDK!",
                "voice": "alloy",
                "response_format": "mp3"
            }))
        );
    }

    #[test]
    fn openai_speech_should_pass_headers() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_speech_test_provider(Arc::clone(&captured_requests), "mp3");

        let _result = poll_ready(
            provider.speech("tts-1").do_generate(
                SpeechModelCallOptions::new("Hello from the AI SDK!")
                    .with_header("Custom-Request-Header", "request-header-value"),
            ),
        );

        let request = captured_openai_request(&captured_requests);
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            request
                .headers
                .get("custom-request-header")
                .map(String::as_str),
            Some("request-header-value")
        );
        assert_eq!(
            request
                .headers
                .get("openai-organization")
                .map(String::as_str),
            Some("org_test")
        );
        assert_eq!(
            request.headers.get("openai-project").map(String::as_str),
            Some("proj_test")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/openai/0.1.0"))
        );
    }

    #[test]
    fn openai_speech_should_pass_options() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_speech_test_provider(Arc::clone(&captured_requests), "opus");

        let _result = poll_ready(
            provider.speech("tts-1").do_generate(
                SpeechModelCallOptions::new("Hello from the AI SDK!")
                    .with_voice("nova")
                    .with_output_format("opus")
                    .with_speed(1.5),
            ),
        );

        let request = captured_openai_request(&captured_requests);
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "tts-1",
                "input": "Hello from the AI SDK!",
                "voice": "nova",
                "speed": 1.5,
                "response_format": "opus"
            }))
        );
    }

    #[test]
    fn openai_speech_should_return_audio_data_with_correct_content_type() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_speech_test_provider(Arc::clone(&captured_requests), "opus");
        let expected_audio = vec![7_u8; 100];

        let result = poll_ready(provider.speech("tts-1").do_generate(
            SpeechModelCallOptions::new("Hello from the AI SDK!").with_output_format("opus"),
        ));

        assert_eq!(result.audio, FileDataContent::Bytes(expected_audio));
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("content-type"))
                .map(String::as_str),
            Some("audio/opus")
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("test-request-id")
        );
    }

    #[test]
    fn openai_speech_should_include_response_data_with_timestamp_model_id_and_headers() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_speech_test_provider(Arc::clone(&captured_requests), "mp3");
        let test_date =
            OffsetDateTime::parse("1970-01-01T00:00:00Z", &Rfc3339).expect("date parses");

        let result = poll_ready(
            provider
                .speech("tts-1")
                .with_current_date(move || test_date)
                .do_generate(SpeechModelCallOptions::new("Hello from the AI SDK!")),
        );

        assert_eq!(result.response.timestamp, test_date);
        assert_eq!(result.response.model_id, "tts-1");
        assert_eq!(
            result.response.headers,
            Some(Headers::from([
                ("content-type".to_string(), "audio/mp3".to_string()),
                ("x-ratelimit-remaining".to_string(), "123".to_string()),
                ("x-request-id".to_string(), "test-request-id".to_string()),
            ]))
        );
    }

    #[test]
    fn openai_speech_should_use_real_date_when_no_custom_date_provider_is_specified() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_speech_test_provider(Arc::clone(&captured_requests), "mp3");
        let before = OffsetDateTime::now_utc();

        let result = poll_ready(
            provider
                .speech("tts-1")
                .do_generate(SpeechModelCallOptions::new("Hello from the AI SDK!")),
        );
        let after = OffsetDateTime::now_utc();

        assert!(result.response.timestamp >= before);
        assert!(result.response.timestamp <= after);
        assert_eq!(result.response.model_id, "tts-1");
    }

    #[test]
    fn openai_speech_should_handle_different_audio_formats() {
        for format in ["mp3", "opus", "aac", "flac", "wav", "pcm"] {
            let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
            let provider = openai_speech_test_provider(Arc::clone(&captured_requests), format);
            let provider_options: ProviderOptions = serde_json::from_value(json!({
                "openai": {
                    "response_format": format
                }
            }))
            .expect("provider options deserialize");

            let result = poll_ready(
                provider.speech("tts-1").do_generate(
                    SpeechModelCallOptions::new("Hello from the AI SDK!")
                        .with_provider_options(provider_options),
                ),
            );

            assert_eq!(result.audio, FileDataContent::Bytes(vec![7_u8; 100]));
            assert_eq!(
                result
                    .response
                    .headers
                    .as_ref()
                    .and_then(|headers| headers.get("content-type"))
                    .map(String::as_str),
                Some(format!("audio/{format}").as_str())
            );
        }
    }

    #[test]
    fn openai_speech_should_include_warnings_if_any_are_generated() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_speech_test_provider(Arc::clone(&captured_requests), "mp3");

        let result = poll_ready(
            provider
                .speech("tts-1")
                .do_generate(SpeechModelCallOptions::new("Hello from the AI SDK!")),
        );

        assert!(result.warnings.is_empty());
    }

    #[test]
    fn openai_speech_should_set_specification_version_and_provider() {
        let provider = openai_speech_test_provider(Arc::new(Mutex::new(Vec::new())), "mp3");
        let speech = ProviderWithSpeechModel::speech_model(&provider, "tts-1")
            .expect("speech model resolves");

        assert_eq!(speech.specification_version(), SpecificationVersion::V4);
        assert_eq!(speech.provider(), "openai.speech");
        assert_eq!(speech.model_id(), "tts-1");
    }

    #[test]
    fn openai_transcription_should_pass_the_model() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_transcription_test_provider(
            Arc::clone(&captured_requests),
            openai_transcription_fixture(),
        );

        let _result = poll_ready(
            provider
                .transcription("whisper-1")
                .do_generate(openai_transcription_call_options()),
        );

        let request = captured_openai_request(&captured_requests);
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.openai.test/v1/audio/transcriptions"
        );
        let form_data = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("form data body is captured");
        assert_eq!(
            form_data.get("model"),
            Some(&FormDataValue::text("whisper-1"))
        );
        assert_eq!(
            form_data.get("file"),
            Some(&FormDataValue::bytes(vec![1_u8, 2, 3, 4]))
        );
    }

    #[test]
    fn openai_transcription_should_pass_headers() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_transcription_test_provider(
            Arc::clone(&captured_requests),
            openai_transcription_fixture(),
        );

        let _result = poll_ready(
            provider.transcription("whisper-1").do_generate(
                openai_transcription_call_options()
                    .with_header("Custom-Request-Header", "request-header-value"),
            ),
        );

        let request = captured_openai_request(&captured_requests);
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            request
                .headers
                .get("custom-request-header")
                .map(String::as_str),
            Some("request-header-value")
        );
        assert_eq!(
            request
                .headers
                .get("openai-organization")
                .map(String::as_str),
            Some("org_test")
        );
        assert_eq!(
            request.headers.get("openai-project").map(String::as_str),
            Some("proj_test")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/openai/0.1.0"))
        );
    }

    #[test]
    fn openai_transcription_should_extract_the_transcription_text() {
        let provider = openai_transcription_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            openai_transcription_fixture(),
        );

        let result = poll_ready(
            provider
                .transcription("whisper-1")
                .do_generate(openai_transcription_call_options()),
        );

        assert_eq!(
            result.text,
            "Galileo was an American robotic space program that studied the planet Jupiter and its moons, as well as several other solar system bodies."
        );
    }

    #[test]
    fn openai_transcription_should_include_response_data_with_timestamp_model_id_and_headers() {
        let provider = openai_transcription_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            openai_transcription_fixture(),
        );
        let test_date =
            OffsetDateTime::parse("1970-01-01T00:00:00Z", &Rfc3339).expect("date parses");

        let result = poll_ready(
            provider
                .transcription("whisper-1")
                .with_current_date(move || test_date)
                .do_generate(openai_transcription_call_options()),
        );

        assert_eq!(result.response.timestamp, test_date);
        assert_eq!(result.response.model_id, "whisper-1");
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("test-request-id")
        );
        assert_eq!(result.response.body, Some(openai_transcription_fixture()));
    }

    #[test]
    fn openai_transcription_should_use_real_date_when_no_custom_date_provider_is_specified() {
        let provider = openai_transcription_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            openai_transcription_fixture(),
        );
        let before = OffsetDateTime::now_utc();

        let result = poll_ready(
            provider
                .transcription("whisper-1")
                .do_generate(openai_transcription_call_options()),
        );
        let after = OffsetDateTime::now_utc();

        assert!(result.response.timestamp >= before);
        assert!(result.response.timestamp <= after);
        assert_eq!(result.response.model_id, "whisper-1");
    }

    #[test]
    fn openai_transcription_should_pass_response_format_when_timestamp_granularities_is_set() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_transcription_test_provider(
            Arc::clone(&captured_requests),
            openai_transcription_fixture(),
        );
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "timestampGranularities": ["word"]
            }
        }))
        .expect("provider options deserialize");

        let _result = poll_ready(provider.transcription("whisper-1").do_generate(
            openai_transcription_call_options().with_provider_options(provider_options),
        ));

        let form_data = captured_openai_request(&captured_requests)
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("form data body is captured")
            .clone();
        assert_eq!(
            form_data.get("response_format"),
            Some(&FormDataValue::text("verbose_json"))
        );
        assert_eq!(
            form_data.get("temperature"),
            Some(&FormDataValue::text("0"))
        );
        assert_eq!(
            form_data.get("timestamp_granularities[]"),
            Some(&FormDataValue::text("word"))
        );
    }

    #[test]
    fn openai_transcription_should_not_set_verbose_json_for_gpt_4o_transcribe() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_transcription_test_provider(
            Arc::clone(&captured_requests),
            openai_transcription_fixture(),
        );
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "timestampGranularities": ["word"]
            }
        }))
        .expect("provider options deserialize");

        let _result = poll_ready(provider.transcription("gpt-4o-transcribe").do_generate(
            openai_transcription_call_options().with_provider_options(provider_options),
        ));

        let form_data = captured_openai_request(&captured_requests)
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("form data body is captured")
            .clone();
        assert_eq!(
            form_data.get("response_format"),
            Some(&FormDataValue::text("json"))
        );
        assert_eq!(
            form_data.get("timestamp_granularities[]"),
            Some(&FormDataValue::text("word"))
        );
    }

    #[test]
    fn openai_transcription_should_pass_timestamp_granularities_when_specified() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider = openai_transcription_test_provider(
            Arc::clone(&captured_requests),
            openai_transcription_fixture(),
        );
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "timestampGranularities": ["segment"]
            }
        }))
        .expect("provider options deserialize");

        let _result = poll_ready(provider.transcription("whisper-1").do_generate(
            openai_transcription_call_options().with_provider_options(provider_options),
        ));

        let form_data = captured_openai_request(&captured_requests)
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("form data body is captured")
            .clone();
        assert_eq!(
            form_data.get("timestamp_granularities[]"),
            Some(&FormDataValue::text("segment"))
        );
    }

    #[test]
    fn openai_transcription_should_work_when_no_words_language_or_duration_are_returned() {
        let provider = openai_transcription_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            json!({
                "task": "transcribe",
                "text": "Hello from the Vercel AI SDK!",
                "_request_id": "req_1234"
            }),
        );

        let result = poll_ready(
            provider
                .transcription("whisper-1")
                .do_generate(openai_transcription_call_options()),
        );

        assert_eq!(result.text, "Hello from the Vercel AI SDK!");
        assert_eq!(result.segments, Vec::<TranscriptionModelSegment>::new());
        assert_eq!(result.language, None);
        assert_eq!(result.duration_in_seconds, None);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn openai_transcription_should_parse_segments_when_provided_in_response() {
        let provider = openai_transcription_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            json!({
                "task": "transcribe",
                "text": "Hello world. How are you?",
                "segments": [
                    {
                        "id": 0,
                        "seek": 0,
                        "start": 0.0,
                        "end": 2.5,
                        "text": "Hello world.",
                        "tokens": [1234, 5678],
                        "temperature": 0.0,
                        "avg_logprob": -0.5,
                        "compression_ratio": 1.2,
                        "no_speech_prob": 0.1
                    },
                    {
                        "id": 1,
                        "seek": 250,
                        "start": 2.5,
                        "end": 5.0,
                        "text": " How are you?",
                        "tokens": [9012, 3456],
                        "temperature": 0.0,
                        "avg_logprob": -0.6,
                        "compression_ratio": 1.1,
                        "no_speech_prob": 0.05
                    }
                ],
                "language": "en",
                "duration": 5.0
            }),
        );

        let result = poll_ready(
            provider
                .transcription("whisper-1")
                .do_generate(openai_transcription_call_options()),
        );

        assert_eq!(
            result.segments,
            vec![
                TranscriptionModelSegment::new("Hello world.", 0.0, 2.5),
                TranscriptionModelSegment::new(" How are you?", 2.5, 5.0),
            ]
        );
        assert_eq!(result.text, "Hello world. How are you?");
        assert_eq!(result.duration_in_seconds, Some(5.0));
    }

    #[test]
    fn openai_transcription_should_fallback_to_words_when_segments_are_not_available() {
        let provider = openai_transcription_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            json!({
                "task": "transcribe",
                "text": "Hello world",
                "words": [
                    { "word": "Hello", "start": 0.0, "end": 1.0 },
                    { "word": "world", "start": 1.0, "end": 2.0 }
                ],
                "language": "en",
                "duration": 2.0
            }),
        );

        let result = poll_ready(
            provider
                .transcription("whisper-1")
                .do_generate(openai_transcription_call_options()),
        );

        assert_eq!(
            result.segments,
            vec![
                TranscriptionModelSegment::new("Hello", 0.0, 1.0),
                TranscriptionModelSegment::new("world", 1.0, 2.0),
            ]
        );
    }

    #[test]
    fn openai_transcription_should_handle_empty_segments_array() {
        let provider = openai_transcription_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            json!({
                "task": "transcribe",
                "text": "Hello world",
                "segments": [],
                "language": "en",
                "duration": 2.0
            }),
        );

        let result = poll_ready(
            provider
                .transcription("whisper-1")
                .do_generate(openai_transcription_call_options()),
        );

        assert_eq!(result.segments, Vec::<TranscriptionModelSegment>::new());
        assert_eq!(result.text, "Hello world");
    }

    #[test]
    fn openai_transcription_should_handle_segments_with_missing_optional_fields() {
        let provider = openai_transcription_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            json!({
                "task": "transcribe",
                "text": "Test",
                "segments": [
                    {
                        "id": 0,
                        "seek": 0,
                        "start": 0.0,
                        "end": 1.0,
                        "text": "Test",
                        "tokens": [1234],
                        "temperature": 0.0,
                        "avg_logprob": -0.5,
                        "compression_ratio": 1.0,
                        "no_speech_prob": 0.1
                    }
                ],
                "_request_id": "req_1234"
            }),
        );

        let result = poll_ready(
            provider
                .transcription("whisper-1")
                .do_generate(openai_transcription_call_options()),
        );

        assert_eq!(
            result.segments,
            vec![TranscriptionModelSegment::new("Test", 0.0, 1.0)]
        );
        assert_eq!(result.language, None);
        assert_eq!(result.duration_in_seconds, None);
    }

    #[test]
    fn openai_transcription_should_set_specification_version_and_provider() {
        let provider = openai_transcription_test_provider(
            Arc::new(Mutex::new(Vec::new())),
            openai_transcription_fixture(),
        );
        let transcription =
            ProviderWithTranscriptionModel::transcription_model(&provider, "whisper-1")
                .expect("transcription model resolves");

        assert_eq!(
            transcription.specification_version(),
            SpecificationVersion::V4
        );
        assert_eq!(transcription.provider(), "openai.transcription");
        assert_eq!(transcription.model_id(), "whisper-1");
    }

    #[test]
    fn openai_provider_uses_default_base_url_name_override_and_provider_trait() {
        let provider = OpenAIProvider::new().with_name("custom-openai");

        let responses = provider.language_model("gpt-4.1-mini");
        assert_eq!(responses.provider(), "custom-openai.responses");
        assert_eq!(responses.model_id(), "gpt-4.1-mini");
        assert_eq!(super::openai("gpt-4.1-mini").provider(), "openai.responses");
        assert_eq!(DEFAULT_OPENAI_BASE_URL, "https://api.openai.com/v1");

        let trait_responses =
            Provider::language_model(&provider, "gpt-4.1-mini").expect("language model exists");
        assert_eq!(trait_responses.provider(), "custom-openai.responses");
        let trait_embedding = Provider::embedding_model(&provider, "text-embedding-3-small")
            .expect("embedding model exists");
        assert_eq!(trait_embedding.provider(), "custom-openai.embedding");
        let trait_image = Provider::image_model(&provider, "dall-e-3").expect("image model exists");
        assert_eq!(trait_image.provider(), "custom-openai.image");
        let trait_speech =
            ProviderWithSpeechModel::speech_model(&provider, "tts-1").expect("speech model exists");
        assert_eq!(trait_speech.provider(), "custom-openai.speech");
        let trait_transcription =
            ProviderWithTranscriptionModel::transcription_model(&provider, "whisper-1")
                .expect("transcription model exists");
        assert_eq!(
            trait_transcription.provider(),
            "custom-openai.transcription"
        );
    }

    #[test]
    fn openai_provider_settings_serde_accepts_upstream_base_url_name() {
        let settings: OpenAIProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.openai.test/v1",
            "apiKey": "test-api-key",
            "organization": "org_test",
            "project": "proj_test",
            "name": "custom-openai",
            "headers": {
                "x-test": "value"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            OpenAIProviderSettings::new()
                .with_base_url("https://api.openai.test/v1")
                .with_api_key("test-api-key")
                .with_organization("org_test")
                .with_project("proj_test")
                .with_name("custom-openai")
                .with_header("x-test", "value")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.openai.test/v1",
                "apiKey": "test-api-key",
                "organization": "org_test",
                "project": "proj_test",
                "name": "custom-openai",
                "headers": {
                    "x-test": "value"
                }
            })
        );
    }

    fn openai_image_test_provider(
        captured_requests: Arc<Mutex<Vec<ProviderApiRequest>>>,
        response_body: JsonValue,
    ) -> OpenAIProvider {
        openai_image_test_provider_with_headers(captured_requests, response_body, Headers::new())
    }

    fn openai_image_test_provider_with_headers(
        captured_requests: Arc<Mutex<Vec<ProviderApiRequest>>>,
        response_body: JsonValue,
        response_headers: Headers,
    ) -> OpenAIProvider {
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_requests
                    .lock()
                    .expect("captured requests mutex is not poisoned")
                    .push(request);

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    response_body.to_string(),
                )
                .with_headers(response_headers.clone()))))
            });

        OpenAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.openai.test/v1/")
            .with_transport(transport)
    }

    fn openai_image_call_options() -> ImageModelCallOptions {
        openai_image_call_options_with_n(1)
    }

    fn openai_image_call_options_with_n(n: u64) -> ImageModelCallOptions {
        ImageModelCallOptions::new(n).with_prompt("A cute baby sea otter")
    }

    fn openai_image_edit_call_options() -> ImageModelCallOptions {
        openai_image_call_options().with_files(vec![ImageModelFile::file(
            "image/png",
            FileDataContent::Bytes(vec![137, 80, 78, 71]),
        )])
    }

    fn openai_image_fixture() -> JsonValue {
        json!({
            "created": 1770935200_u64,
            "size": "1024x1024",
            "quality": "hd",
            "background": "transparent",
            "output_format": "png",
            "data": [
                {
                    "b64_json": "base64-image-1",
                    "revised_prompt": "A small and adorable baby sea otter."
                },
                {
                    "b64_json": "base64-image-2"
                }
            ]
        })
    }

    fn openai_image_edit_fixture() -> JsonValue {
        json!({
            "created": 1733837122_u64,
            "data": [
                {
                    "b64_json": "edited-base64-image-1"
                }
            ]
        })
    }

    fn assert_openai_image_model_omits_response_format(model_id: &str) {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let provider =
            openai_image_test_provider(Arc::clone(&captured_requests), openai_image_fixture());

        let _result = poll_ready(
            provider
                .image(model_id)
                .do_generate(openai_image_call_options().with_size("1024x1024")),
        );

        let body = captured_json_body(&captured_requests);
        assert_eq!(
            body,
            json!({
                "model": model_id,
                "prompt": "A cute baby sea otter",
                "n": 1,
                "size": "1024x1024"
            })
        );
        assert!(body.get("response_format").is_none());
    }

    fn captured_request(
        captured_requests: &Arc<Mutex<Vec<ProviderApiRequest>>>,
    ) -> ProviderApiRequest {
        captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
            .last()
            .cloned()
            .expect("request is captured")
    }

    fn captured_json_body(captured_requests: &Arc<Mutex<Vec<ProviderApiRequest>>>) -> JsonValue {
        captured_request(captured_requests)
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("JSON body is captured")
    }

    fn captured_form_data(captured_requests: &Arc<Mutex<Vec<ProviderApiRequest>>>) -> FormData {
        captured_request(captured_requests)
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .cloned()
            .expect("form data body is captured")
    }

    fn openai_embedding_test_provider(
        captured_requests: Arc<Mutex<Vec<ProviderApiRequest>>>,
        response_body: JsonValue,
        response_headers: Headers,
    ) -> OpenAIProvider {
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_requests
                    .lock()
                    .expect("captured requests mutex is not poisoned")
                    .push(request);

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    response_body.to_string(),
                )
                .with_headers(response_headers.clone()))))
            });

        OpenAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.openai.test/v1/")
            .with_transport(transport)
    }

    fn openai_embedding_call_options() -> EmbeddingModelCallOptions {
        EmbeddingModelCallOptions::new(vec![
            "sunny day at the beach".to_string(),
            "rainy day in the city".to_string(),
        ])
    }

    fn openai_embedding_fixture() -> JsonValue {
        json!({
            "object": "list",
            "data": [
                {
                    "object": "embedding",
                    "index": 0,
                    "embedding": [
                        0.0057293195,
                        -0.012727811,
                        0.020042092,
                        -0.013437585,
                        0.022833068
                    ]
                },
                {
                    "object": "embedding",
                    "index": 1,
                    "embedding": [
                        -0.037104916,
                        -0.05178114,
                        -0.008340587,
                        0.001164541,
                        -0.0035253682
                    ]
                }
            ],
            "model": "text-embedding-3-large",
            "usage": {
                "prompt_tokens": 12,
                "total_tokens": 12
            }
        })
    }

    fn openai_files_test_provider(
        captured_requests: Arc<Mutex<Vec<ProviderApiRequest>>>,
    ) -> OpenAIProvider {
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_requests
                    .lock()
                    .expect("captured requests mutex is not poisoned")
                    .push(request);

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "file-openai-upload",
                        "object": "file",
                        "filename": "uploaded.jsonl",
                        "purpose": "assistants",
                        "bytes": 12,
                        "created_at": 1711115037,
                        "status": "processed",
                        "expires_at": 1711125037
                    })
                    .to_string(),
                ))))
            });

        OpenAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.openai.test/v1/")
            .with_organization("org_test")
            .with_project("proj_test")
            .with_header("custom-header", "value")
            .with_transport(transport)
    }

    fn openai_skills_test_provider(
        captured_requests: Arc<Mutex<Vec<ProviderApiRequest>>>,
    ) -> OpenAIProvider {
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_requests
                    .lock()
                    .expect("captured requests mutex is not poisoned")
                    .push(request);

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "skill_699fc58f408c8191825d8d06ae75fd5c06de7b381a5db7f5",
                        "object": "skill",
                        "name": "test-capture-skill",
                        "description": "A test skill for fixture capture",
                        "default_version": "1",
                        "latest_version": "1",
                        "created_at": 1772078479_u64
                    })
                    .to_string(),
                ))))
            });

        OpenAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.openai.test/v1/")
            .with_organization("org_test")
            .with_project("proj_test")
            .with_header("custom-header", "value")
            .with_transport(transport)
    }

    fn openai_speech_test_provider(
        captured_requests: Arc<Mutex<Vec<ProviderApiRequest>>>,
        format: &'static str,
    ) -> OpenAIProvider {
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_requests
                    .lock()
                    .expect("captured requests mutex is not poisoned")
                    .push(request);

                Box::pin(ready(Ok(ProviderApiResponse::bytes(
                    200,
                    "OK",
                    vec![7_u8; 100],
                )
                .with_headers(Headers::from([
                    ("content-type".to_string(), format!("audio/{format}")),
                    ("x-ratelimit-remaining".to_string(), "123".to_string()),
                    ("x-request-id".to_string(), "test-request-id".to_string()),
                ])))))
            });

        OpenAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.openai.test/v1/")
            .with_organization("org_test")
            .with_project("proj_test")
            .with_header("custom-header", "value")
            .with_transport(transport)
    }

    fn openai_transcription_test_provider(
        captured_requests: Arc<Mutex<Vec<ProviderApiRequest>>>,
        response_body: JsonValue,
    ) -> OpenAIProvider {
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_requests
                    .lock()
                    .expect("captured requests mutex is not poisoned")
                    .push(request);

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    response_body.to_string(),
                )
                .with_headers(Headers::from([
                    ("content-type".to_string(), "application/json".to_string()),
                    ("x-ratelimit-remaining".to_string(), "123".to_string()),
                    ("x-request-id".to_string(), "test-request-id".to_string()),
                ])))))
            });

        OpenAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.openai.test/v1/")
            .with_organization("org_test")
            .with_project("proj_test")
            .with_header("custom-header", "value")
            .with_transport(transport)
    }

    fn openai_transcription_call_options() -> TranscriptionModelCallOptions {
        TranscriptionModelCallOptions::new(FileDataContent::Bytes(vec![1, 2, 3, 4]), "audio/wav")
    }

    fn openai_transcription_fixture() -> JsonValue {
        json!({
            "task": "transcribe",
            "language": "english",
            "duration": 36.709999084472656_f64,
            "text": "Galileo was an American robotic space program that studied the planet Jupiter and its moons, as well as several other solar system bodies.",
            "words": [
                {
                    "word": "Galileo",
                    "start": 0,
                    "end": 0.6600000262260437_f64
                },
                {
                    "word": "was",
                    "start": 0.6600000262260437_f64,
                    "end": 0.8999999761581421_f64
                }
            ],
            "usage": {
                "type": "duration",
                "seconds": 37
            }
        })
    }

    fn openai_skill_upload_options(data: SkillsFileData) -> SkillsUploadSkillCallOptions {
        SkillsUploadSkillCallOptions::new(vec![SkillsFile::new("index.ts", data)])
    }

    fn captured_openai_files_request(
        captured_requests: &Arc<Mutex<Vec<ProviderApiRequest>>>,
    ) -> ProviderApiRequest {
        captured_openai_request(captured_requests)
    }

    fn captured_openai_request(
        captured_requests: &Arc<Mutex<Vec<ProviderApiRequest>>>,
    ) -> ProviderApiRequest {
        captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
            .first()
            .cloned()
            .expect("request is captured")
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);
        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => {
                struct NoopWake;

                impl Wake for NoopWake {
                    fn wake(self: Arc<Self>) {}
                }

                let waker = Waker::from(Arc::new(NoopWake));
                let mut context = Context::from_waker(&waker);
                loop {
                    match Pin::new(&mut future).poll(&mut context) {
                        Poll::Ready(value) => break value,
                        Poll::Pending => std::thread::yield_now(),
                    }
                }
            }
        }
    }
}
