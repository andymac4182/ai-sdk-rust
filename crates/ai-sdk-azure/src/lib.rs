use std::env;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_rust::file_data::FileDataContent;
use ai_sdk_rust::json::{JsonObject, JsonValue};
use ai_sdk_rust::openai::{OpenAIErrorData, OpenAITranscriptionResponse};
use ai_sdk_rust::provider::{ProviderWithSpeechModel, ProviderWithTranscriptionModel};
use ai_sdk_rust::provider_utils::{
    FetchErrorInfo, FormData, FormDataValue, PostFormDataToApiOptions, PostJsonToApiOptions,
    ProviderApiRequest, ProviderApiResponse, ProviderApiResponseHandlerError,
    ResponseHandlerResult, convert_base64_to_bytes, create_binary_response_handler,
    create_json_response_handler, media_type_to_extension, post_form_data_to_api, post_json_to_api,
};
use ai_sdk_rust::warning::Warning;
use ai_sdk_rust::{
    Headers, NoSuchModelError, OpenAICompatibleChatLanguageModel,
    OpenAICompatibleCompletionLanguageModel, OpenAICompatibleEmbeddingModel,
    OpenAICompatibleImageModel, OpenAICompatibleProvider, OpenAICompatibleProviderSettings,
    OpenAICompatibleTransport, OpenResponsesLanguageModel, OpenResponsesProvider,
    OpenResponsesProviderSettings, Provider, SpeechModel, SpeechModelCallOptions,
    SpeechModelRequest, SpeechModelResponse, SpeechModelResult, TranscriptionModel,
    TranscriptionModelCallOptions, TranscriptionModelResponse, TranscriptionModelResult,
    TranscriptionModelSegment, without_trailing_slash,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Default API version for upstream `@ai-sdk/azure`.
pub const DEFAULT_AZURE_OPENAI_API_VERSION: &str = "v1";

/// Settings for the upstream Azure OpenAI provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureOpenAIProviderSettings {
    /// Azure OpenAI resource name used to build the default API origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_name: Option<String>,

    /// Custom base URL prefix. When set, `resource_name` is ignored.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Azure OpenAI API key. When omitted, `AZURE_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,

    /// Azure OpenAI API version query parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_version: Option<String>,

    /// Use legacy deployment-based URLs for created models.
    #[serde(default, skip_serializing_if = "is_false")]
    pub use_deployment_based_urls: bool,
}

impl AzureOpenAIProviderSettings {
    /// Creates empty Azure OpenAI provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Azure OpenAI resource name.
    pub fn with_resource_name(mut self, resource_name: impl Into<String>) -> Self {
        self.resource_name = Some(resource_name.into());
        self
    }

    /// Sets a custom Azure OpenAI base URL prefix.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Azure OpenAI API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Sets the Azure OpenAI API version query parameter.
    pub fn with_api_version(mut self, api_version: impl Into<String>) -> Self {
        self.api_version = Some(api_version.into());
        self
    }

    /// Enables or disables legacy deployment-based URLs.
    pub fn with_use_deployment_based_urls(mut self, use_deployment_based_urls: bool) -> Self {
        self.use_deployment_based_urls = use_deployment_based_urls;
        self
    }
}

/// Upstream Azure OpenAI provider foundation.
#[derive(Clone)]
pub struct AzureOpenAIProvider {
    settings: AzureOpenAIProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

/// Azure OpenAI speech model for `/audio/speech`.
#[derive(Clone)]
pub struct AzureOpenAISpeechModel {
    provider: String,
    model_id: String,
    base_url: String,
    api_version: String,
    headers: Headers,
    transport: OpenAICompatibleTransport,
    current_date: Option<Arc<dyn Fn() -> OffsetDateTime + Send + Sync>>,
}

impl AzureOpenAISpeechModel {
    fn new(
        provider: impl Into<String>,
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        api_version: impl Into<String>,
        headers: Headers,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            provider: provider.into(),
            model_id: model_id.into(),
            base_url: base_url.into(),
            api_version: api_version.into(),
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

/// Azure OpenAI transcription model for `/audio/transcriptions`.
#[derive(Clone)]
pub struct AzureOpenAITranscriptionModel {
    provider: String,
    model_id: String,
    base_url: String,
    api_version: String,
    headers: Headers,
    transport: OpenAICompatibleTransport,
    current_date: Option<Arc<dyn Fn() -> OffsetDateTime + Send + Sync>>,
}

impl AzureOpenAITranscriptionModel {
    fn new(
        provider: impl Into<String>,
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        api_version: impl Into<String>,
        headers: Headers,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            provider: provider.into(),
            model_id: model_id.into(),
            base_url: base_url.into(),
            api_version: api_version.into(),
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

impl AzureOpenAIProvider {
    /// Creates an Azure OpenAI provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(AzureOpenAIProviderSettings::new())
    }

    /// Creates a provider from explicit Azure OpenAI settings.
    pub fn from_settings(settings: AzureOpenAIProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the Azure OpenAI resource name for this provider.
    pub fn with_resource_name(mut self, resource_name: impl Into<String>) -> Self {
        self.settings.resource_name = Some(resource_name.into());
        self
    }

    /// Sets a custom Azure OpenAI base URL prefix for this provider.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self
    }

    /// Sets the Azure OpenAI API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Sets the Azure OpenAI API version query parameter.
    pub fn with_api_version(mut self, api_version: impl Into<String>) -> Self {
        self.settings.api_version = Some(api_version.into());
        self
    }

    /// Enables or disables legacy deployment-based URLs.
    pub fn with_use_deployment_based_urls(mut self, use_deployment_based_urls: bool) -> Self {
        self.settings.use_deployment_based_urls = use_deployment_based_urls;
        self
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Creates an Azure OpenAI Responses API language model.
    pub fn language_model(&self, deployment_id: impl Into<String>) -> OpenResponsesLanguageModel {
        self.responses(deployment_id)
    }

    /// Creates an Azure OpenAI Responses API language model.
    pub fn responses(&self, deployment_id: impl Into<String>) -> OpenResponsesLanguageModel {
        let deployment_id = deployment_id.into();
        self.open_responses_provider(&deployment_id)
            .language_model(deployment_id)
    }

    /// Creates an Azure OpenAI chat model.
    pub fn chat(&self, deployment_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        let deployment_id = deployment_id.into();
        self.openai_compatible_provider(&deployment_id)
            .chat_model(deployment_id)
    }

    /// Creates an Azure OpenAI completion model.
    pub fn completion(
        &self,
        deployment_id: impl Into<String>,
    ) -> OpenAICompatibleCompletionLanguageModel {
        let deployment_id = deployment_id.into();
        self.openai_compatible_provider(&deployment_id)
            .completion_model(deployment_id)
    }

    /// Creates an Azure OpenAI embedding model.
    pub fn embedding(&self, deployment_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        let deployment_id = deployment_id.into();
        self.openai_compatible_provider(&deployment_id)
            .embedding_model(deployment_id)
    }

    /// Creates an Azure OpenAI embedding model.
    pub fn embedding_model(
        &self,
        deployment_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding(deployment_id)
    }

    /// Deprecated upstream alias for [`AzureOpenAIProvider::embedding`].
    pub fn text_embedding(
        &self,
        deployment_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding(deployment_id)
    }

    /// Deprecated upstream alias for [`AzureOpenAIProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        deployment_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(deployment_id)
    }

    /// Creates an Azure OpenAI image model.
    pub fn image(&self, deployment_id: impl Into<String>) -> OpenAICompatibleImageModel {
        let deployment_id = deployment_id.into();
        self.openai_compatible_provider(&deployment_id)
            .image_model(deployment_id)
    }

    /// Creates an Azure OpenAI image model.
    pub fn image_model(&self, deployment_id: impl Into<String>) -> OpenAICompatibleImageModel {
        self.image(deployment_id)
    }

    /// Creates an Azure OpenAI speech model.
    pub fn speech(&self, deployment_id: impl Into<String>) -> AzureOpenAISpeechModel {
        let deployment_id = deployment_id.into();
        let base_url = self.model_base_url(&deployment_id);
        AzureOpenAISpeechModel::new(
            "azure.speech",
            deployment_id,
            base_url,
            self.api_version(),
            self.request_headers(),
            self.transport
                .as_ref()
                .map(Arc::clone)
                .unwrap_or_else(default_azure_openai_files_transport),
        )
    }

    /// Creates an Azure OpenAI speech model.
    pub fn speech_model(&self, deployment_id: impl Into<String>) -> AzureOpenAISpeechModel {
        self.speech(deployment_id)
    }

    /// Creates an Azure OpenAI transcription model.
    pub fn transcription(&self, deployment_id: impl Into<String>) -> AzureOpenAITranscriptionModel {
        let deployment_id = deployment_id.into();
        let base_url = self.model_base_url(&deployment_id);
        AzureOpenAITranscriptionModel::new(
            "azure.transcription",
            deployment_id,
            base_url,
            self.api_version(),
            self.request_headers(),
            self.transport
                .as_ref()
                .map(Arc::clone)
                .unwrap_or_else(default_azure_openai_files_transport),
        )
    }

    /// Creates an Azure OpenAI transcription model.
    pub fn transcription_model(
        &self,
        deployment_id: impl Into<String>,
    ) -> AzureOpenAITranscriptionModel {
        self.transcription(deployment_id)
    }

    fn open_responses_provider(&self, deployment_id: &str) -> OpenResponsesProvider {
        let mut settings = OpenResponsesProviderSettings::new(
            "azure",
            format!(
                "{}/responses?api-version={}",
                self.model_base_url(deployment_id),
                self.api_version()
            ),
        )
        .with_file_id_prefix("assistant-")
        .with_user_agent_suffix(format!("ai-sdk/azure/{}", ai_sdk_rust::VERSION));

        for (name, value) in self.request_headers() {
            settings = settings.with_header(name, value);
        }

        let provider = OpenResponsesProvider::from_settings(settings);

        if let Some(transport) = &self.transport {
            provider.with_transport(Arc::clone(transport))
        } else {
            provider
        }
    }

    fn openai_compatible_provider(&self, deployment_id: &str) -> OpenAICompatibleProvider {
        let mut settings =
            OpenAICompatibleProviderSettings::new("azure", self.model_base_url(deployment_id))
                .with_query_param("api-version", self.api_version())
                .with_model_provider_name("embedding", "azure.embeddings")
                .with_user_agent_suffix(format!("ai-sdk/azure/{}", ai_sdk_rust::VERSION));

        for (name, value) in self.request_headers() {
            settings = settings.with_header(name, value);
        }

        let provider = OpenAICompatibleProvider::from_settings(settings);

        if let Some(transport) = &self.transport {
            provider.with_transport(Arc::clone(transport))
        } else {
            provider
        }
    }

    fn model_base_url(&self, deployment_id: &str) -> String {
        let prefix = self.base_url_prefix();

        if self.settings.use_deployment_based_urls {
            format!("{prefix}/deployments/{deployment_id}")
        } else {
            format!("{prefix}/v1")
        }
    }

    fn base_url_prefix(&self) -> String {
        if let Some(base_url) = non_empty_optional_setting(self.settings.base_url.clone()) {
            return without_trailing_slash(Some(&base_url))
                .unwrap_or(&base_url)
                .to_string();
        }

        let resource_name = non_empty_optional_setting(self.settings.resource_name.clone())
            .or_else(|| non_empty_optional_setting(env::var("AZURE_RESOURCE_NAME").ok()))
            .unwrap_or_default();

        format!("https://{resource_name}.openai.azure.com/openai")
    }

    fn api_version(&self) -> String {
        non_empty_optional_setting(self.settings.api_version.clone())
            .unwrap_or_else(|| DEFAULT_AZURE_OPENAI_API_VERSION.to_string())
    }

    fn request_headers(&self) -> Headers {
        let mut headers = Headers::new();

        if let Some(api_key) = non_empty_optional_setting(self.settings.api_key.clone())
            .or_else(|| non_empty_optional_setting(env::var("AZURE_API_KEY").ok()))
        {
            headers.insert("api-key".to_string(), api_key);
        }

        for (name, value) in &self.settings.headers {
            headers.insert(name.clone(), value.clone());
        }

        headers
    }
}

impl Default for AzureOpenAIProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for AzureOpenAIProvider {
    type LanguageModel = OpenResponsesLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(AzureOpenAIProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(AzureOpenAIProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(AzureOpenAIProvider::image_model(self, model_id))
    }
}

impl ProviderWithSpeechModel for AzureOpenAIProvider {
    type SpeechModel = AzureOpenAISpeechModel;

    fn speech_model(&self, model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError> {
        Ok(AzureOpenAIProvider::speech_model(self, model_id))
    }
}

impl ProviderWithTranscriptionModel for AzureOpenAIProvider {
    type TranscriptionModel = AzureOpenAITranscriptionModel;

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
        Ok(AzureOpenAIProvider::transcription_model(self, model_id))
    }
}

impl SpeechModel for AzureOpenAISpeechModel {
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
            let (request_body, warnings) = azure_speech_request_body(&model_id, &options);

            if let Some(request_headers) = &options.headers {
                for (name, value) in request_headers {
                    headers.insert(name.clone(), value.clone());
                }
            }

            let response = post_json_to_api(
                PostJsonToApiOptions::new(
                    format!("{base_url}/audio/speech?api-version={}", self.api_version),
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
                    Ok(azure_failed_response_handler(&provider, request, response))
                },
            )
            .await
            .expect("Azure speech generation failed");

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

impl TranscriptionModel for AzureOpenAITranscriptionModel {
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
            let (form_data, warnings) = azure_transcription_form_data(&model_id, &options);

            if let Some(request_headers) = &options.headers {
                for (name, value) in request_headers {
                    headers.insert(name.clone(), value.clone());
                }
            }

            let response = post_form_data_to_api(
                PostFormDataToApiOptions::new(
                    format!(
                        "{base_url}/audio/transcriptions?api-version={}",
                        self.api_version
                    ),
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
                    Ok(azure_failed_response_handler(&provider, request, response))
                },
            )
            .await
            .expect("Azure transcription failed");

            azure_transcription_result(model_id, timestamp, response, warnings)
        })
    }
}

/// Creates an Azure OpenAI provider with explicit settings.
pub fn create_azure(settings: AzureOpenAIProviderSettings) -> AzureOpenAIProvider {
    AzureOpenAIProvider::from_settings(settings)
}

/// Creates an Azure OpenAI Responses API language model using default provider settings.
pub fn azure(deployment_id: impl Into<String>) -> OpenResponsesLanguageModel {
    AzureOpenAIProvider::new().language_model(deployment_id)
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn default_azure_openai_files_transport() -> OpenAICompatibleTransport {
    Arc::new(|_| {
        Box::pin(std::future::ready(Err(FetchErrorInfo::new(
            "multipart form data requires an injected Azure transport",
        ))))
    })
}

fn azure_file_upload_data_bytes(data: &FileDataContent) -> Vec<u8> {
    match data {
        FileDataContent::Bytes(bytes) => bytes.clone(),
        FileDataContent::Base64(base64) => {
            convert_base64_to_bytes(base64).unwrap_or_else(|_| base64.as_bytes().to_vec())
        }
    }
}

fn azure_speech_request_body(
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
        if azure_speech_output_format_is_supported(output_format) {
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

fn azure_speech_output_format_is_supported(output_format: &str) -> bool {
    matches!(
        output_format,
        "mp3" | "opus" | "aac" | "flac" | "wav" | "pcm"
    )
}

fn azure_transcription_form_data(
    model_id: &str,
    options: &TranscriptionModelCallOptions,
) -> (FormData, Vec<Warning>) {
    let mut form_data = FormData::new();
    form_data.append("model", FormDataValue::text(model_id));
    form_data.append(
        "file",
        FormDataValue::bytes(azure_file_upload_data_bytes(&options.audio)),
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
            FormDataValue::text(azure_transcription_response_format(model_id)),
        );

        let temperature = openai_options
            .get("temperature")
            .map(azure_form_data_value)
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

fn azure_transcription_response_format(model_id: &str) -> &'static str {
    if matches!(model_id, "gpt-4o-transcribe" | "gpt-4o-mini-transcribe") {
        "json"
    } else {
        "verbose_json"
    }
}

fn azure_form_data_value(value: &JsonValue) -> String {
    match value {
        JsonValue::String(value) => value.clone(),
        JsonValue::Number(value) => value.to_string(),
        JsonValue::Bool(value) => value.to_string(),
        _ => value.to_string(),
    }
}

fn azure_transcription_result(
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
    if let Some(language) = language.and_then(|language| azure_transcription_language(&language)) {
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

fn azure_transcription_language(language: &str) -> Option<String> {
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

fn azure_failed_response_handler(
    provider: &str,
    request: &ProviderApiRequest,
    response: &ProviderApiResponse,
) -> ResponseHandlerResult<ai_sdk_rust::provider::ApiCallError> {
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
    let error = ai_sdk_rust::provider::ApiCallError::new(
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

#[cfg(test)]
mod azure_speech_transcription_tests {
    use super::{AzureOpenAIProviderSettings, create_azure};
    use ai_sdk_rust::{
        FileDataContent, Headers, OpenAICompatibleTransport, OpenAICompatibleTransportFuture,
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
        SpeechModel, SpeechModelCallOptions, TranscriptionModel, TranscriptionModelCallOptions,
    };
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

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

    #[test]
    fn azure_provider_speech_uses_correct_url_format() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport = Arc::new(
            move |request: ProviderApiRequest| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::bytes(
                    200,
                    "OK",
                    vec![1_u8, 2, 3],
                )
                .with_headers(Headers::from([(
                    "content-type".to_string(),
                    "audio/mp3".to_string(),
                )])))))
            },
        );
        let provider = create_azure(
            AzureOpenAIProviderSettings::new()
                .with_resource_name("test-resource")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);

        let result = poll_ready(
            provider
                .speech("tts-1")
                .do_generate(SpeechModelCallOptions::new("Hello, world!")),
        );

        assert_eq!(result.audio, FileDataContent::Bytes(vec![1_u8, 2, 3]));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://test-resource.openai.azure.com/openai/v1/audio/speech?api-version=v1"
        );
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .expect("speech request body is text");
        let request_json: serde_json::Value =
            serde_json::from_str(request_body).expect("speech request body is valid JSON");
        assert_eq!(request_json["model"], "tts-1");
        assert_eq!(request_json["input"], "Hello, world!");
    }

    #[test]
    fn azure_provider_speech_uses_deployment_based_url_when_enabled() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport = Arc::new(
            move |request: ProviderApiRequest| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::bytes(
                    200,
                    "OK",
                    vec![1_u8, 2, 3],
                ))))
            },
        );
        let provider = create_azure(
            AzureOpenAIProviderSettings::new()
                .with_resource_name("test-resource")
                .with_api_key("test-api-key")
                .with_use_deployment_based_urls(true),
        )
        .with_transport(transport);

        let _ = poll_ready(
            provider
                .speech("tts-1")
                .do_generate(SpeechModelCallOptions::new("Hello, world!")),
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.url,
            "https://test-resource.openai.azure.com/openai/deployments/tts-1/audio/speech?api-version=v1"
        );
    }

    #[test]
    fn azure_provider_transcription_uses_correct_url_format() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport = Arc::new(
            move |request: ProviderApiRequest| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "text": "Hello, world!",
                        "segments": [],
                        "language": "en",
                        "duration": 5.0,
                    })
                    .to_string(),
                ))))
            },
        );
        let provider = create_azure(
            AzureOpenAIProviderSettings::new()
                .with_resource_name("test-resource")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);

        let result = poll_ready(provider.transcription("whisper-1").do_generate(
            TranscriptionModelCallOptions::new(FileDataContent::Bytes(Vec::new()), "audio/wav"),
        ));

        assert_eq!(result.text, "Hello, world!");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://test-resource.openai.azure.com/openai/v1/audio/transcriptions?api-version=v1"
        );
        assert!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_form_data)
                .is_some(),
            "transcription request body should be form data"
        );
    }

    #[test]
    fn azure_provider_transcription_uses_deployment_based_url_when_enabled() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport = Arc::new(
            move |request: ProviderApiRequest| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "text": "Hello, world!",
                        "segments": [],
                        "language": "en",
                        "duration": 5.0,
                    })
                    .to_string(),
                ))))
            },
        );
        let provider = create_azure(
            AzureOpenAIProviderSettings::new()
                .with_resource_name("test-resource")
                .with_api_key("test-api-key")
                .with_use_deployment_based_urls(true),
        )
        .with_transport(transport);

        let _ = poll_ready(provider.transcription("whisper-1").do_generate(
            TranscriptionModelCallOptions::new(FileDataContent::Bytes(Vec::new()), "audio/wav"),
        ));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.url,
            "https://test-resource.openai.azure.com/openai/deployments/whisper-1/audio/transcriptions?api-version=v1"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AzureOpenAIProvider, AzureOpenAIProviderSettings, DEFAULT_AZURE_OPENAI_API_VERSION, azure,
        create_azure,
    };
    use ai_sdk_rust::{
        ContentPart, EmbeddingModel, EmbeddingModelCallOptions, FinishReason, GenerateTextOptions,
        Headers, ImageModel, ImageModelCallOptions, JsonValue, LanguageModel,
        LanguageModelCallOptions, LanguageModelMessage, LanguageModelStreamPart,
        LanguageModelTextPart, LanguageModelToolChoice, LanguageModelUserContentPart,
        LanguageModelUserMessage, OpenAICompatibleTransport, OpenAICompatibleTransportFuture,
        Prompt, Provider, ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod,
        ProviderApiResponse, Tool, generate_text,
    };
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

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

    #[test]
    fn azure_provider_creates_responses_model_with_resource_url_headers_and_api_version() {
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
                        "id": "resp_azure",
                        "created_at": 1711115037,
                        "model": "test-deployment",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hello from Azure"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 4,
                            "output_tokens": 3
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_azure_responses".to_string(),
                )])))))
            });
        let provider = create_azure(
            AzureOpenAIProviderSettings::new()
                .with_resource_name("test-resource")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value")
                .with_api_version("2025-04-01-preview"),
        )
        .with_transport(transport);
        let model = provider.language_model("test-deployment");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid"),
        ));

        assert_eq!(model.provider(), "azure.responses");
        assert_eq!(model.model_id(), "test-deployment");
        assert_eq!(result.text, "Hello from Azure");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://test-resource.openai.azure.com/openai/v1/responses?api-version=2025-04-01-preview"
        );
        assert_eq!(
            request.headers.get("api-key").map(String::as_str),
            Some("test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/azure/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
                .and_then(|body| body.get("model").cloned()),
            Some(json!("test-deployment"))
        );
    }

    #[test]
    fn azure_provider_creates_chat_model_with_deployment_based_url() {
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
                        "id": "chatcmpl-azure",
                        "created": 1711115037,
                        "model": "gpt-35",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Azure chat"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 5
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_azure(
            AzureOpenAIProviderSettings::new()
                .with_base_url("https://test-resource.openai.azure.com/openai/")
                .with_api_key("test-api-key")
                .with_use_deployment_based_urls(true),
        )
        .with_transport(transport);
        let model = provider.chat("gpt-35");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid"),
        ));

        assert_eq!(model.provider(), "azure.chat");
        assert_eq!(result.text, "Hello from Azure chat");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.url,
            "https://test-resource.openai.azure.com/openai/deployments/gpt-35/chat/completions?api-version=v1"
        );
        assert_eq!(
            request.headers.get("api-key").map(String::as_str),
            Some("test-api-key")
        );
    }

    #[test]
    fn azure_provider_uses_default_aliases_and_provider_trait() {
        let provider = AzureOpenAIProvider::new().with_resource_name("test-resource");
        let model = azure("test-deployment");
        let trait_model =
            Provider::language_model(&provider, "trait-deployment").expect("model resolves");

        assert_eq!(model.provider(), "azure.responses");
        assert_eq!(model.model_id(), "test-deployment");
        assert_eq!(trait_model.provider(), "azure.responses");
        assert_eq!(trait_model.model_id(), "trait-deployment");
        assert_eq!(
            provider.api_version(),
            DEFAULT_AZURE_OPENAI_API_VERSION.to_string()
        );
        let embedding_model = provider.embedding("embed");
        assert_eq!(embedding_model.provider(), "azure.embeddings");
        assert_eq!(embedding_model.model_id(), "embed");
        assert_eq!(
            provider.text_embedding("embed").provider(),
            "azure.embeddings"
        );
        assert_eq!(provider.image("image").model_id(), "image");
    }

    #[test]
    fn azure_provider_creates_completion_model_with_default_v1_url() {
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
                        "id": "cmpl-azure",
                        "created": 1711363706,
                        "model": "completion-deployment",
                        "choices": [
                            {
                                "index": 0,
                                "text": "Hello from Azure completion",
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 5
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = AzureOpenAIProvider::new()
            .with_resource_name("test-resource")
            .with_api_key("test-api-key")
            .with_transport(transport);
        let model = provider.completion("completion-deployment");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid"),
        ));

        assert_eq!(model.provider(), "azure.completion");
        assert_eq!(result.text, "Hello from Azure completion");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.url,
            "https://test-resource.openai.azure.com/openai/v1/completions?api-version=v1"
        );
        assert_eq!(
            request.headers.get("api-key").map(String::as_str),
            Some("test-api-key")
        );
    }

    #[test]
    fn azure_provider_creates_embedding_and_image_models_with_upstream_urls() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_requests_for_transport
                    .lock()
                    .expect("captured requests mutex is not poisoned")
                    .push(request.clone());

                let response_body = if request.url.contains("/embeddings?") {
                    json!({
                        "data": [
                            {
                                "object": "embedding",
                                "index": 0,
                                "embedding": [0.1, 0.2, 0.3]
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 3,
                            "total_tokens": 3
                        }
                    })
                } else {
                    json!({
                        "data": [
                            {
                                "b64_json": "aW1hZ2UtMQ=="
                            }
                        ]
                    })
                };

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    response_body.to_string(),
                ))))
            });
        let provider = AzureOpenAIProvider::new()
            .with_resource_name("test-resource")
            .with_api_key("test-api-key")
            .with_api_version("2025-04-01-preview")
            .with_transport(transport);
        let embedding_result = poll_ready(provider.embedding("embedding-deployment").do_embed(
            EmbeddingModelCallOptions::new(vec!["sunny day".to_string()]),
        ));
        let image_result = poll_ready(
            provider.image("dalle-deployment").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A precise test image")
                    .with_size("1024x1024"),
            ),
        );

        assert_eq!(embedding_result.embeddings, vec![vec![0.1, 0.2, 0.3]]);
        assert_eq!(embedding_result.usage.expect("usage is mapped").tokens, 3);
        assert_eq!(image_result.images.len(), 1);

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned");
        assert_eq!(
            requests
                .iter()
                .map(|request| request.url.as_str())
                .collect::<Vec<_>>(),
            vec![
                "https://test-resource.openai.azure.com/openai/v1/embeddings?api-version=2025-04-01-preview",
                "https://test-resource.openai.azure.com/openai/v1/images/generations?api-version=2025-04-01-preview",
            ]
        );
        assert_eq!(
            requests[0]
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "embedding-deployment",
                "input": ["sunny day"],
                "encoding_format": "float"
            }))
        );
        assert_eq!(
            requests[1]
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "dalle-deployment",
                "prompt": "A precise test image",
                "n": 1,
                "size": "1024x1024",
                "response_format": "b64_json"
            }))
        );
    }

    #[test]
    fn azure_provider_uses_azure_metadata_key_for_text_result() {
        let transport: OpenAICompatibleTransport =
            Arc::new(move |_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_provider_metadata_azure",
                        "object": "response",
                        "created_at": 1234567890,
                        "status": "completed",
                        "error": null,
                        "incomplete_details": null,
                        "input": [],
                        "instructions": null,
                        "max_output_tokens": null,
                        "model": "gpt-4o",
                        "parallel_tool_calls": true,
                        "previous_response_id": null,
                        "reasoning": {
                            "effort": null,
                            "summary": null
                        },
                        "store": true,
                        "temperature": 0,
                        "text": {
                            "format": {
                                "type": "text"
                            }
                        },
                        "tool_choice": "auto",
                        "tools": [],
                        "top_p": 1,
                        "truncation": "disabled",
                        "usage": {
                            "input_tokens": 10,
                            "input_tokens_details": {
                                "cached_tokens": 0
                            },
                            "output_tokens": 5,
                            "output_tokens_details": {
                                "reasoning_tokens": 0
                            },
                            "total_tokens": 15
                        },
                        "user": null,
                        "metadata": {},
                        "output": [
                            {
                                "id": "msg_azure_text",
                                "type": "message",
                                "status": "completed",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hello from Azure!",
                                        "annotations": []
                                    }
                                ]
                            }
                        ]
                    })
                    .to_string(),
                ))))
            });
        let provider = create_azure(
            AzureOpenAIProviderSettings::new()
                .with_resource_name("test-resource")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.responses("gpt-4o");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16),
        ));

        assert_eq!(result.text, "Hello from Azure!");
        assert!(result.provider_metadata.as_ref().is_some_and(|metadata| {
            metadata.contains_key("azure") && !metadata.contains_key("openai")
        }));
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("azure"))
                .and_then(|metadata| metadata.get("responseId"))
                .and_then(JsonValue::as_str),
            Some("resp_provider_metadata_azure")
        );

        let text = result
            .content
            .iter()
            .find_map(|part| match part {
                ContentPart::Text(text) => Some(text),
                _ => None,
            })
            .expect("content includes Azure text part");
        assert_eq!(
            text.provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("azure"))
                .and_then(|metadata| metadata.get("itemId"))
                .and_then(JsonValue::as_str),
            Some("msg_azure_text")
        );
        assert!(
            text.provider_metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.contains_key("openai"))
        );
    }

    #[test]
    fn azure_provider_uses_azure_metadata_key_for_function_call_content() {
        let transport: OpenAICompatibleTransport =
            Arc::new(move |_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_azure_tool_call",
                        "created_at": 1711115037,
                        "status": "completed",
                        "model": "gpt-4o",
                        "output": [
                            {
                                "id": "fc_azure",
                                "call_id": "call_azure",
                                "type": "function_call",
                                "name": "weather",
                                "arguments": "{\"location\":\"Seattle\"}",
                                "namespace": "weather_ns",
                                "status": "completed"
                            }
                        ],
                        "usage": {
                            "input_tokens": 10,
                            "output_tokens": 5
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_azure(
            AzureOpenAIProviderSettings::new()
                .with_resource_name("test-resource")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.responses("gpt-4o");
        let input_schema = json!({
            "type": "object",
            "properties": {
                "location": { "type": "string" }
            },
            "required": ["location"],
            "additionalProperties": false
        })
        .as_object()
        .expect("schema is an object")
        .clone();
        let tool = Tool::new("weather", input_schema);
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("What is the weather in Seattle?"),
            )
            .expect("prompt is valid")
            .with_tool(tool)
            .with_tool_choice(LanguageModelToolChoice::Required),
        ));

        assert_eq!(result.finish_reason, FinishReason::ToolCalls);
        assert!(result.provider_metadata.as_ref().is_some_and(|metadata| {
            metadata.contains_key("azure") && !metadata.contains_key("openai")
        }));
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("azure"))
                .and_then(|metadata| metadata.get("responseId"))
                .and_then(JsonValue::as_str),
            Some("resp_azure_tool_call")
        );

        let tool_call = result
            .content
            .iter()
            .find_map(|part| match part {
                ContentPart::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .expect("content includes Azure tool call");
        assert_eq!(
            tool_call
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("azure"))
                .and_then(|metadata| metadata.get("itemId"))
                .and_then(JsonValue::as_str),
            Some("fc_azure")
        );
        assert_eq!(
            tool_call
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("azure"))
                .and_then(|metadata| metadata.get("namespace"))
                .and_then(JsonValue::as_str),
            Some("weather_ns")
        );
        assert!(
            tool_call
                .provider_metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.contains_key("openai"))
        );
    }

    #[test]
    fn azure_provider_streams_azure_metadata_key_for_reasoning_and_finish() {
        let transport: OpenAICompatibleTransport = Arc::new(
            move |_request| -> OpenAICompatibleTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_azure_stream","created_at":1711115037,"model":"o3-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"rs_azure","type":"reasoning","encrypted_content":null}}"#,
                    "",
                    r#"data: {"type":"response.reasoning_summary_text.delta","item_id":"rs_azure","summary_index":0,"delta":"thinking"}"#,
                    "",
                    r#"data: {"type":"response.reasoning_summary_text.done","item_id":"rs_azure","summary_index":0,"text":"thinking"}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_azure_stream","created_at":1711115037,"model":"o3-mini","usage":{"input_tokens":10,"output_tokens":20,"output_tokens_details":{"reasoning_tokens":20}}}}"#,
                    "",
                    "data: [DONE]",
                    "",
                ]
                .join("\n");

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", sse))))
            },
        );
        let provider = create_azure(
            AzureOpenAIProviderSettings::new()
                .with_resource_name("test-resource")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.responses("o3-mini");
        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Think briefly")),
            ])),
        ])));

        let reasoning_start = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::ReasoningStart(reasoning_start) => Some(reasoning_start),
                _ => None,
            })
            .expect("stream includes reasoning start");
        assert_eq!(
            reasoning_start
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("azure"))
                .and_then(|metadata| metadata.get("itemId"))
                .and_then(JsonValue::as_str),
            Some("rs_azure")
        );
        assert!(
            reasoning_start
                .provider_metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.contains_key("openai"))
        );

        let reasoning_delta = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::ReasoningDelta(reasoning_delta) => Some(reasoning_delta),
                _ => None,
            })
            .expect("stream includes reasoning delta");
        assert_eq!(
            reasoning_delta
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("azure"))
                .and_then(|metadata| metadata.get("itemId"))
                .and_then(JsonValue::as_str),
            Some("rs_azure")
        );

        let finish = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => Some(finish),
                _ => None,
            })
            .expect("stream includes finish");
        assert_eq!(
            finish
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("azure"))
                .and_then(|metadata| metadata.get("responseId"))
                .and_then(JsonValue::as_str),
            Some("resp_azure_stream")
        );
        assert!(
            finish
                .provider_metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.contains_key("openai"))
        );
    }

    #[test]
    fn azure_provider_settings_serde_accepts_upstream_shape() {
        let settings: AzureOpenAIProviderSettings = serde_json::from_value(json!({
            "resourceName": "test-resource",
            "baseURL": "https://proxy.example.com/openai",
            "apiKey": "key",
            "headers": {
                "x-provider": "azure"
            },
            "apiVersion": "2025-04-01-preview",
            "useDeploymentBasedUrls": true
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            AzureOpenAIProviderSettings::new()
                .with_resource_name("test-resource")
                .with_base_url("https://proxy.example.com/openai")
                .with_api_key("key")
                .with_header("x-provider", "azure")
                .with_api_version("2025-04-01-preview")
                .with_use_deployment_based_urls(true)
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "resourceName": "test-resource",
                "baseURL": "https://proxy.example.com/openai",
                "apiKey": "key",
                "headers": {
                    "x-provider": "azure"
                },
                "apiVersion": "2025-04-01-preview",
                "useDeploymentBasedUrls": true
            })
        );
    }
}
