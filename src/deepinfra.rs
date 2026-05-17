use std::collections::BTreeMap;
use std::env;
use std::future::{Future, Ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::file_data::FileDataContent;
use crate::headers::Headers;
use crate::image_model::{
    ImageModel, ImageModelCallOptions, ImageModelFile, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResult,
};
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    InputTokenUsage, LanguageModel, LanguageModelCallOptions, LanguageModelGenerateResult,
    LanguageModelStreamPart, LanguageModelStreamResult, LanguageModelSupportedUrls,
    LanguageModelUsage, OutputTokenUsage,
};
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleCompletionLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleProvider, OpenAICompatibleProviderSettings,
    OpenAICompatibleTransport,
};
use crate::provider::{NoSuchModelError, Provider, SpecificationVersion};
use crate::provider_utils::{
    ConvertToFormDataOptions, FetchErrorInfo, FormDataInputValue, FormDataValue, HandledFetchError,
    PostFormDataToApiOptions, PostJsonToApiOptions, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, ProviderApiResponseHandlerError,
    RuntimeEnvironment, combine_headers, convert_base64_to_bytes, convert_to_form_data,
    create_json_error_response_handler, create_json_response_handler, post_form_data_to_api,
    post_json_to_api, with_user_agent_suffix, without_trailing_slash,
};

/// Default base URL for upstream `@ai-sdk/deepinfra` API calls.
pub const DEFAULT_DEEPINFRA_BASE_URL: &str = "https://api.deepinfra.com/v1";

/// Settings for the upstream DeepInfra provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepInfraProviderSettings {
    /// Base URL for DeepInfra API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// DeepInfra API key. When omitted, `DEEPINFRA_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl DeepInfraProviderSettings {
    /// Creates empty DeepInfra provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the DeepInfra API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the DeepInfra API key.
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

/// Upstream DeepInfra provider foundation.
#[derive(Clone)]
pub struct DeepInfraProvider {
    settings: DeepInfraProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

/// DeepInfra chat language model with upstream DeepInfra usage correction.
#[derive(Clone)]
pub struct DeepInfraChatLanguageModel {
    inner: OpenAICompatibleChatLanguageModel,
}

/// DeepInfra image model for `/inference/{modelId}` image generation calls.
#[derive(Clone)]
pub struct DeepInfraImageModel {
    model_id: String,
    base_url: String,
    settings: DeepInfraProviderSettings,
    transport: OpenAICompatibleTransport,
}

impl DeepInfraChatLanguageModel {
    fn new(inner: OpenAICompatibleChatLanguageModel) -> Self {
        Self { inner }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        self.inner.provider()
    }

    /// Returns whether structured outputs are enabled for this chat model.
    pub fn supports_structured_outputs(&self) -> bool {
        self.inner.supports_structured_outputs()
    }
}

impl LanguageModel for DeepInfraChatLanguageModel {
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
        self.inner.specification_version()
    }

    fn provider(&self) -> &str {
        self.inner.provider()
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        self.inner.supported_urls()
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(async move {
            let mut result = self.inner.do_generate(options).await;
            result.usage = correct_deepinfra_usage(result.usage);
            result
        })
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(async move {
            let mut result = self.inner.do_stream(options).await;
            for part in &mut result.stream {
                if let LanguageModelStreamPart::Finish(finish) = part {
                    finish.usage = correct_deepinfra_usage(finish.usage.clone());
                }
            }
            result
        })
    }
}

impl DeepInfraImageModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: DeepInfraProviderSettings,
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
        "deepinfra.image"
    }

    async fn do_generate_result(&self, options: ImageModelCallOptions) -> ImageModelResult {
        let request_headers = self.request_headers(options.headers.as_ref());
        let response = if options
            .files
            .as_ref()
            .is_some_and(|files| !files.is_empty())
        {
            self.do_generate_edit_result(options, request_headers).await
        } else {
            self.do_generate_image_result(options, request_headers)
                .await
        };

        match response {
            Ok((response, response_headers)) => {
                deepinfra_image_result_from_response(&self.model_id, response, response_headers)
            }
            Err(error) => deepinfra_image_result_from_error(&self.model_id, error),
        }
    }

    async fn do_generate_image_result(
        &self,
        options: ImageModelCallOptions,
        request_headers: BTreeMap<String, Option<String>>,
    ) -> Result<(DeepInfraImageResponse, Option<Headers>), HandledFetchError> {
        let request_body = deepinfra_image_generation_request_body(&options);
        let post_options =
            PostJsonToApiOptions::new(format!("{}/{}", self.base_url, self.model_id), request_body)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    deepinfra_image_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    deepinfra_image_error_response,
                    deepinfra_image_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => Ok((response.value, response.response_headers)),
            Err(error) => Err(error),
        }
    }

    async fn do_generate_edit_result(
        &self,
        options: ImageModelCallOptions,
        request_headers: BTreeMap<String, Option<String>>,
    ) -> Result<(DeepInfraImageResponse, Option<Headers>), HandledFetchError> {
        let form_data = deepinfra_image_edit_form_data(&self.model_id, &options);
        let post_options = PostFormDataToApiOptions::new(self.edit_url(), form_data)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        match post_form_data_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    deepinfra_image_edit_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    deepinfra_image_error_response,
                    deepinfra_image_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => Ok((response.value, response.response_headers)),
            Err(error) => Err(error),
        }
    }

    fn edit_url(&self) -> String {
        format!(
            "{}/images/edits",
            self.base_url.replacen("/inference", "/openai", 1)
        )
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(
                deepinfra_provider_headers(&self.settings)
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
}

impl ImageModel for DeepInfraImageModel {
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
        "deepinfra.image"
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        std::future::ready(Some(1))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

impl DeepInfraProvider {
    /// Creates a DeepInfra provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(DeepInfraProviderSettings::new())
    }

    /// Creates a provider from explicit DeepInfra settings.
    pub fn from_settings(settings: DeepInfraProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the DeepInfra API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the DeepInfra API base URL for this provider.
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
        self.transport = Some(transport);
        self
    }

    /// Creates a DeepInfra chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> DeepInfraChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates a DeepInfra chat language model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> DeepInfraChatLanguageModel {
        DeepInfraChatLanguageModel::new(self.openai_compatible_provider().chat_model(model_id))
    }

    /// Alias for [`DeepInfraProvider::chat_model`].
    pub fn chat(&self, model_id: impl Into<String>) -> DeepInfraChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates a DeepInfra completion language model.
    pub fn completion_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleCompletionLanguageModel {
        self.openai_compatible_provider().completion_model(model_id)
    }

    /// Creates a DeepInfra embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.openai_compatible_provider().embedding_model(model_id)
    }

    /// Alias for [`DeepInfraProvider::embedding_model`].
    pub fn embedding(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`DeepInfraProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates a DeepInfra image model.
    pub fn image_model(&self, model_id: impl Into<String>) -> DeepInfraImageModel {
        let transport = self
            .transport
            .as_ref()
            .map(Arc::clone)
            .unwrap_or_else(default_deepinfra_transport);
        DeepInfraImageModel::new(
            model_id,
            format!("{}/inference", deepinfra_base_url(&self.settings)),
            self.settings.clone(),
            transport,
        )
    }

    /// Alias for [`DeepInfraProvider::image_model`].
    pub fn image(&self, model_id: impl Into<String>) -> DeepInfraImageModel {
        self.image_model(model_id)
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings = OpenAICompatibleProviderSettings::new(
            "deepinfra",
            format!("{}/openai", deepinfra_base_url(&self.settings)),
        )
        .with_user_agent_suffix(format!("ai-sdk/deepinfra/{}", crate::VERSION));

        if let Some(api_key) = deepinfra_api_key(self.settings.api_key.as_ref()) {
            settings = settings.with_api_key(api_key);
        }

        for (name, value) in &self.settings.headers {
            settings = settings.with_header(name.clone(), value.clone());
        }

        let provider = OpenAICompatibleProvider::from_settings(settings);

        if let Some(transport) = &self.transport {
            provider.with_transport(Arc::clone(transport))
        } else {
            provider
        }
    }
}

impl Default for DeepInfraProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for DeepInfraProvider {
    type LanguageModel = DeepInfraChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = DeepInfraImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(DeepInfraProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(DeepInfraProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(DeepInfraProvider::image_model(self, model_id))
    }
}

/// Creates a DeepInfra provider with explicit settings.
pub fn create_deepinfra(settings: DeepInfraProviderSettings) -> DeepInfraProvider {
    DeepInfraProvider::from_settings(settings)
}

/// Creates a DeepInfra chat language model using the default provider settings.
pub fn deepinfra(model_id: impl Into<String>) -> DeepInfraChatLanguageModel {
    DeepInfraProvider::new().language_model(model_id)
}

fn deepinfra_base_url(settings: &DeepInfraProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_DEEPINFRA_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn deepinfra_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("DEEPINFRA_API_KEY").ok()))
}

fn correct_deepinfra_usage(usage: LanguageModelUsage) -> LanguageModelUsage {
    let Some(mut raw) = usage.raw.clone() else {
        return usage;
    };
    let Some(reasoning_tokens) = deepinfra_reasoning_tokens(&raw) else {
        return usage;
    };
    let completion_tokens = json_u64(raw.get("completion_tokens")).unwrap_or_default();

    if reasoning_tokens <= completion_tokens {
        return usage;
    }

    let corrected_completion_tokens = completion_tokens.saturating_add(reasoning_tokens);
    raw.insert(
        "completion_tokens".to_string(),
        JsonValue::from(corrected_completion_tokens),
    );

    if let Some(total_tokens) = json_u64(raw.get("total_tokens")) {
        raw.insert(
            "total_tokens".to_string(),
            JsonValue::from(total_tokens.saturating_add(reasoning_tokens)),
        );
    }

    deepinfra_usage_from_raw(raw)
}

fn deepinfra_usage_from_raw(raw: JsonObject) -> LanguageModelUsage {
    let input_total = json_u64(
        raw.get("prompt_tokens")
            .or_else(|| raw.get("promptTokens"))
            .or_else(|| raw.get("input_tokens"))
            .or_else(|| raw.get("inputTokens")),
    );
    let output_total = json_u64(
        raw.get("completion_tokens")
            .or_else(|| raw.get("completionTokens"))
            .or_else(|| raw.get("output_tokens"))
            .or_else(|| raw.get("outputTokens")),
    );
    let cache_read = json_u64(raw.get("prompt_tokens_details").and_then(|details| {
        details
            .get("cached_tokens")
            .or_else(|| details.get("cachedTokens"))
    }));
    let reasoning_tokens = deepinfra_reasoning_tokens(&raw);

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
        raw: Some(raw),
    }
}

fn deepinfra_reasoning_tokens(raw: &JsonObject) -> Option<u64> {
    json_u64(
        raw.get("completion_tokens_details")
            .and_then(|details| {
                details
                    .get("reasoning_tokens")
                    .or_else(|| details.get("reasoningTokens"))
            })
            .or_else(|| raw.get("reasoning_tokens"))
            .or_else(|| raw.get("reasoningTokens")),
    )
}

fn json_u64(value: Option<&JsonValue>) -> Option<u64> {
    value.and_then(JsonValue::as_u64)
}

fn deepinfra_provider_headers(settings: &DeepInfraProviderSettings) -> Headers {
    let mut headers = Headers::new();

    if let Some(api_key) = deepinfra_api_key(settings.api_key.as_ref()) {
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
        [format!("ai-sdk/deepinfra/{}", crate::VERSION)],
    )
}

fn deepinfra_image_generation_request_body(options: &ImageModelCallOptions) -> JsonValue {
    let mut body = JsonObject::new();

    if let Some(prompt) = &options.prompt {
        body.insert("prompt".to_string(), JsonValue::String(prompt.clone()));
    }

    body.insert("num_images".to_string(), JsonValue::from(options.n));

    if let Some(aspect_ratio) = &options.aspect_ratio {
        body.insert(
            "aspect_ratio".to_string(),
            JsonValue::String(aspect_ratio.clone()),
        );
    }

    if let Some(size) = &options.size
        && let Some((width, height)) = size.split_once('x')
    {
        body.insert("width".to_string(), JsonValue::String(width.to_string()));
        body.insert("height".to_string(), JsonValue::String(height.to_string()));
    }

    if let Some(seed) = options.seed {
        body.insert("seed".to_string(), JsonValue::from(seed));
    }

    if let Some(provider_options) = options.provider_options.get("deepinfra") {
        for (name, value) in provider_options {
            body.insert(name.clone(), value.clone());
        }
    }

    JsonValue::Object(body)
}

fn deepinfra_image_edit_form_data(
    model_id: &str,
    options: &ImageModelCallOptions,
) -> crate::provider_utils::FormData {
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
                        .map(|file| FormDataValue::bytes(deepinfra_image_file_bytes(file)))
                        .collect(),
                )
            }),
        ),
        (
            "mask".to_string(),
            options
                .mask
                .as_ref()
                .map(|mask| FormDataInputValue::bytes(deepinfra_image_file_bytes(mask))),
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

    if let Some(provider_options) = options.provider_options.get("deepinfra") {
        for (name, value) in provider_options {
            set_deepinfra_image_form_field(
                &mut input,
                name.clone(),
                deepinfra_image_form_value(value.clone()),
            );
        }
    }

    convert_to_form_data(
        input,
        ConvertToFormDataOptions::new().with_use_array_brackets(false),
    )
}

fn set_deepinfra_image_form_field(
    input: &mut Vec<(String, Option<FormDataInputValue>)>,
    name: String,
    value: Option<FormDataInputValue>,
) {
    input.retain(|(existing_name, _)| existing_name != &name);
    input.push((name, value));
}

fn deepinfra_image_file_bytes(file: &ImageModelFile) -> Vec<u8> {
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

fn deepinfra_image_form_value(value: JsonValue) -> Option<FormDataInputValue> {
    match value {
        JsonValue::Null => None,
        JsonValue::String(value) => Some(FormDataInputValue::text(value)),
        JsonValue::Bool(value) => Some(FormDataInputValue::text(value.to_string())),
        JsonValue::Number(value) => Some(FormDataInputValue::text(value.to_string())),
        JsonValue::Array(values) => Some(FormDataInputValue::array(
            values
                .into_iter()
                .filter_map(|value| {
                    deepinfra_image_form_value(value).and_then(|value| match value {
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

fn deepinfra_image_response(
    value: &JsonValue,
) -> Result<DeepInfraImageResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn deepinfra_image_edit_response(
    value: &JsonValue,
) -> Result<DeepInfraImageResponse, serde_json::Error> {
    let response: DeepInfraImageEditResponse = serde_json::from_value(value.clone())?;

    Ok(DeepInfraImageResponse {
        images: response
            .data
            .into_iter()
            .map(|image| image.b64_json)
            .collect(),
    })
}

fn deepinfra_image_error_response(value: &JsonValue) -> Result<JsonValue, serde_json::Error> {
    Ok(value.clone())
}

fn deepinfra_image_error_message(value: &JsonValue) -> String {
    value
        .get("detail")
        .and_then(|detail| detail.get("error"))
        .and_then(JsonValue::as_str)
        .or_else(|| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(JsonValue::as_str)
        })
        .unwrap_or("Unknown error")
        .to_string()
}

fn deepinfra_image_result_from_response(
    model_id: &str,
    response: DeepInfraImageResponse,
    response_headers: Option<Headers>,
) -> ImageModelResult {
    ImageModelResult::new(
        response
            .images
            .into_iter()
            .map(strip_deepinfra_image_data_prefix)
            .map(FileDataContent::Base64)
            .collect(),
        deepinfra_image_response_metadata(model_id, response_headers),
    )
}

fn deepinfra_image_result_from_error(model_id: &str, error: HandledFetchError) -> ImageModelResult {
    let (message, headers) = match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
        ),
    };
    let mut extra = JsonObject::new();
    extra.insert("errorMessage".to_string(), JsonValue::String(message));

    ImageModelResult::new(
        Vec::new(),
        deepinfra_image_response_metadata(model_id, headers),
    )
    .with_provider_metadata(ImageModelProviderMetadata::from([(
        "deepinfra".to_string(),
        ImageModelProviderMetadataEntry {
            images: Vec::new(),
            extra,
        },
    )]))
}

fn deepinfra_image_response_metadata(
    model_id: &str,
    headers: Option<Headers>,
) -> ImageModelResponse {
    let mut response = ImageModelResponse::new(OffsetDateTime::now_utc(), model_id);

    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    response
}

fn strip_deepinfra_image_data_prefix(image: String) -> String {
    if image.starts_with("data:image/")
        && let Some((_, base64)) = image.split_once(";base64,")
    {
        return base64.to_string();
    }

    image
}

#[derive(Clone, Debug, Deserialize)]
struct DeepInfraImageResponse {
    images: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct DeepInfraImageEditResponse {
    data: Vec<DeepInfraImageEditResponseData>,
}

#[derive(Clone, Debug, Deserialize)]
struct DeepInfraImageEditResponseData {
    b64_json: String,
}

fn default_deepinfra_transport() -> OpenAICompatibleTransport {
    Arc::new(|request| Box::pin(std::future::ready(execute_deepinfra_request(request))))
}

fn execute_deepinfra_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_deepinfra_get_request(request),
        ProviderApiRequestMethod::Post => execute_deepinfra_post_request(request),
    }
}

fn execute_deepinfra_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    deepinfra_provider_api_response(response)
}

fn execute_deepinfra_post_request(
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
                "multipart form data is not supported by the DeepInfra transport",
            ));
        }
        None => builder.send_empty(),
    };

    deepinfra_provider_api_response(response)
}

fn deepinfra_provider_api_response(
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

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_DEEPINFRA_BASE_URL, DeepInfraProvider, DeepInfraProviderSettings, create_deepinfra,
    };
    use crate::embed::{EmbedManyOptions, embed_many};
    use crate::file_data::FileDataContent;
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::image_model::{ImageModel, ImageModelCallOptions, ImageModelFile};
    use crate::json::JsonValue;
    use crate::language_model::{
        LanguageModel, LanguageModelCallOptions, LanguageModelMessage, LanguageModelStreamPart,
        LanguageModelSystemMessage,
    };
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{Provider, ProviderMetadata, ProviderOptions};
    use crate::provider_utils::{
        FormDataValue, ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod,
        ProviderApiResponse,
    };
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    #[test]
    fn deepinfra_provider_creates_chat_model_with_headers_and_base_url() {
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
                        "id": "chatcmpl-deepinfra",
                        "created": 1711115037,
                        "model": "meta-llama/Llama-3.3-70B-Instruct",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from DeepInfra"
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
                    "req_deepinfra".to_string(),
                )])))))
            });
        let provider = create_deepinfra(
            DeepInfraProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.deepinfra.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("meta-llama/Llama-3.3-70B-Instruct");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(model.provider(), "deepinfra.chat");
        assert_eq!(model.model_id(), "meta-llama/Llama-3.3-70B-Instruct");
        assert_eq!(result.text, "Hello from DeepInfra");
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("chatcmpl-deepinfra")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .unwrap_or(&ProviderMetadata::new())
                .get("deepinfra"),
            None
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.deepinfra.test/v1/openai/chat/completions"
        );
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
                .is_some_and(|value| value.contains("ai-sdk/deepinfra/0.1.0")),
            "DeepInfra user-agent suffix is included"
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| !value.contains("ai-sdk/openai-compatible/0.1.0")),
            "DeepInfra wrapper overrides the generic OpenAI-compatible suffix"
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "meta-llama/Llama-3.3-70B-Instruct",
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
    fn deepinfra_chat_corrects_reasoning_usage_when_reasoning_exceeds_completion_tokens() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "chatcmpl-deepinfra-reasoning",
                        "created": 1711115037,
                        "model": "google/gemma-2-9b-it",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Done"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 10,
                            "prompt_tokens_details": {
                                "cached_tokens": 3
                            },
                            "completion_tokens": 4,
                            "completion_tokens_details": {
                                "reasoning_tokens": 10
                            },
                            "total_tokens": 14
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = DeepInfraProvider::new()
            .with_transport(transport)
            .chat_model("google/gemma-2-9b-it");
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::System(LanguageModelSystemMessage::new("Think carefully")),
        ])));

        assert_eq!(result.usage.input_tokens.total, Some(10));
        assert_eq!(result.usage.input_tokens.no_cache, Some(7));
        assert_eq!(result.usage.input_tokens.cache_read, Some(3));
        assert_eq!(result.usage.output_tokens.total, Some(14));
        assert_eq!(result.usage.output_tokens.text, Some(4));
        assert_eq!(result.usage.output_tokens.reasoning, Some(10));
        assert_eq!(
            result
                .usage
                .raw
                .as_ref()
                .and_then(|raw| raw.get("completion_tokens"))
                .and_then(JsonValue::as_u64),
            Some(14)
        );
        assert_eq!(
            result
                .usage
                .raw
                .as_ref()
                .and_then(|raw| raw.get("total_tokens"))
                .and_then(JsonValue::as_u64),
            Some(24)
        );
    }

    #[test]
    fn deepinfra_chat_corrects_stream_finish_reasoning_usage() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    sse_body([
                        json!({
                            "id": "chatcmpl-deepinfra-stream",
                            "created": 1711115037,
                            "model": "google/gemma-2-9b-it",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "role": "assistant",
                                        "content": "Done"
                                    },
                                    "finish_reason": null
                                }
                            ]
                        }),
                        json!({
                            "id": "chatcmpl-deepinfra-stream",
                            "created": 1711115037,
                            "model": "google/gemma-2-9b-it",
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {},
                                    "finish_reason": "stop"
                                }
                            ],
                            "usage": {
                                "prompt_tokens": 10,
                                "completion_tokens": 4,
                                "completion_tokens_details": {
                                    "reasoning_tokens": 10
                                },
                                "total_tokens": 14
                            }
                        }),
                    ]),
                ))))
            });
        let model = DeepInfraProvider::new()
            .with_transport(transport)
            .chat_model("google/gemma-2-9b-it");
        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(Vec::new())));

        assert!(matches!(
            result.stream.last(),
            Some(LanguageModelStreamPart::Finish(finish))
                if finish.usage.output_tokens.total == Some(14)
                    && finish.usage.output_tokens.text == Some(4)
                    && finish.usage.output_tokens.reasoning == Some(10)
                    && finish
                        .usage
                        .raw
                        .as_ref()
                        .and_then(|raw| raw.get("total_tokens"))
                        .and_then(JsonValue::as_u64)
                        == Some(24)
        ));
    }

    #[test]
    fn deepinfra_provider_creates_completion_model() {
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
                        "id": "cmpl-deepinfra",
                        "created": 1711115037,
                        "model": "completion-model",
                        "choices": [
                            {
                                "index": 0,
                                "text": " completed",
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 2,
                            "completion_tokens": 1,
                            "total_tokens": 3
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = DeepInfraProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.deepinfra.test/v1/")
            .with_transport(transport)
            .completion_model("completion-model");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Complete this"))
                .expect("prompt is valid"),
        ));

        assert_eq!(model.provider(), "deepinfra.completion");
        assert_eq!(result.text, " completed");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.url,
            "https://api.deepinfra.test/v1/openai/completions"
        );
    }

    #[test]
    fn deepinfra_provider_creates_embedding_model_aliases() {
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
                        "model": "BAAI/bge-large-en-v1.5",
                        "data": [
                            {
                                "index": 0,
                                "embedding": [0.1, 0.2, 0.3]
                            },
                            {
                                "index": 1,
                                "embedding": [0.4, 0.5, 0.6]
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "total_tokens": 4
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = DeepInfraProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.deepinfra.test/v1/")
            .with_transport(transport);
        let model = provider.embedding_model("BAAI/bge-large-en-v1.5");
        let result = poll_ready(embed_many(EmbedManyOptions::new(
            &model,
            ["sunny day", "rainy city"],
        )));

        assert_eq!(model.provider(), "deepinfra.embedding");
        assert_eq!(
            provider.embedding("BAAI/bge-large-en-v1.5").provider(),
            "deepinfra.embedding"
        );
        assert_eq!(
            provider
                .text_embedding_model("BAAI/bge-large-en-v1.5")
                .provider(),
            "deepinfra.embedding"
        );
        assert_eq!(result.embeddings.len(), 2);
        assert_eq!(result.embeddings[0], vec![0.1, 0.2, 0.3]);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.url,
            "https://api.deepinfra.test/v1/openai/embeddings"
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "BAAI/bge-large-en-v1.5",
                "input": ["sunny day", "rainy city"],
                "encoding_format": "float"
            }))
        );
    }

    #[test]
    fn deepinfra_provider_uses_default_base_url_and_function_alias() {
        let model = super::deepinfra("meta-llama/Llama-3.3-70B-Instruct");

        assert_eq!(model.provider(), "deepinfra.chat");
        assert_eq!(model.model_id(), "meta-llama/Llama-3.3-70B-Instruct");
        assert_eq!(DEFAULT_DEEPINFRA_BASE_URL, "https://api.deepinfra.com/v1");
    }

    #[test]
    fn deepinfra_provider_creates_image_model_and_generates_images() {
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
                        "images": [
                            "data:image/png;base64,test-image-data",
                            "raw-image-data"
                        ]
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_deepinfra_image".to_string(),
                )])))))
            });
        let provider = create_deepinfra(
            DeepInfraProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.deepinfra.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.image_model("black-forest-labs/FLUX-1-schnell");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "deepinfra": {
                "additional_param": "value"
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(2)
                    .with_prompt("A cute baby sea otter")
                    .with_size("1024x768")
                    .with_aspect_ratio("16:9")
                    .with_seed(42)
                    .with_provider_options(provider_options)
                    .with_header("x-call", "image"),
            ),
        );

        assert_eq!(model.provider(), "deepinfra.image");
        assert_eq!(model.model_id(), "black-forest-labs/FLUX-1-schnell");
        assert_eq!(poll_ready(model.max_images_per_call()), Some(1));
        assert_eq!(
            result.images,
            vec![
                FileDataContent::Base64("test-image-data".to_string()),
                FileDataContent::Base64("raw-image-data".to_string())
            ]
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_deepinfra_image")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.deepinfra.test/v1/inference/black-forest-labs/FLUX-1-schnell"
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            request.headers.get("x-call").map(String::as_str),
            Some("image")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/deepinfra/0.1.0")),
            "DeepInfra user-agent suffix is included"
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "prompt": "A cute baby sea otter",
                "num_images": 2,
                "aspect_ratio": "16:9",
                "width": "1024",
                "height": "768",
                "seed": 42,
                "additional_param": "value"
            }))
        );

        assert_eq!(
            provider
                .image("black-forest-labs/FLUX-1-schnell")
                .provider(),
            "deepinfra.image"
        );
    }

    #[test]
    fn deepinfra_image_model_edits_with_files_mask_and_provider_options() {
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
                                "b64_json": "edited-image-base64"
                            }
                        ]
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_deepinfra_edit".to_string(),
                )])))))
            });
        let model = DeepInfraProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://edit.example.com")
            .with_header("custom-header", "value")
            .with_transport(transport)
            .image_model("black-forest-labs/FLUX.1-Kontext-dev");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "deepinfra": {
                "guidance": 7.5,
                "n": 3
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("Turn the cat into a dog")
                    .with_files(vec![
                        ImageModelFile::file(
                            "image/png",
                            FileDataContent::Bytes(vec![137, 80, 78, 71]),
                        ),
                        ImageModelFile::file(
                            "image/png",
                            FileDataContent::Base64("AQID".to_string()),
                        ),
                    ])
                    .with_mask(ImageModelFile::file(
                        "image/png",
                        FileDataContent::Base64("BAUG".to_string()),
                    ))
                    .with_size("1024x1024")
                    .with_aspect_ratio("16:9")
                    .with_seed(42)
                    .with_provider_options(provider_options)
                    .with_header("x-call", "edit"),
            ),
        );

        assert_eq!(
            result.images,
            vec![FileDataContent::Base64("edited-image-base64".to_string())]
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_deepinfra_edit")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://edit.example.com/openai/images/edits");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            request.headers.get("x-call").map(String::as_str),
            Some("edit")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/deepinfra/0.1.0")),
            "DeepInfra user-agent suffix is included"
        );

        let form_data = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("request body is form data");
        assert_eq!(
            form_data.get("model"),
            Some(&FormDataValue::text("black-forest-labs/FLUX.1-Kontext-dev"))
        );
        assert_eq!(
            form_data.get("prompt"),
            Some(&FormDataValue::text("Turn the cat into a dog"))
        );
        assert_eq!(
            form_data
                .get_all("image")
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                FormDataValue::bytes(vec![137, 80, 78, 71]),
                FormDataValue::bytes(vec![1, 2, 3])
            ]
        );
        assert_eq!(
            form_data.get("mask"),
            Some(&FormDataValue::bytes(vec![4, 5, 6]))
        );
        assert_eq!(form_data.get_all("image[]"), Vec::<&FormDataValue>::new());
        assert_eq!(form_data.get_all("n").len(), 1);
        assert_eq!(form_data.get("n"), Some(&FormDataValue::text("3")));
        assert_eq!(
            form_data.get("size"),
            Some(&FormDataValue::text("1024x1024"))
        );
        assert_eq!(form_data.get("guidance"), Some(&FormDataValue::text("7.5")));
        assert_eq!(form_data.get("aspect_ratio"), None);
        assert_eq!(form_data.get("seed"), None);
    }

    #[test]
    fn deepinfra_image_model_maps_generation_api_error_to_metadata() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({
                        "detail": {
                            "error": "bad prompt"
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_deepinfra_image_error".to_string(),
                )])))))
            });
        let model = DeepInfraProvider::new()
            .with_transport(transport)
            .image_model("black-forest-labs/FLUX-1-schnell");
        let result =
            poll_ready(model.do_generate(ImageModelCallOptions::new(1).with_prompt("bad prompt")));

        assert!(result.images.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("deepinfra"))
                .and_then(|metadata| metadata.extra.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("bad prompt")
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_deepinfra_image_error")
        );
    }

    #[test]
    fn deepinfra_image_model_maps_edit_api_error_to_metadata() {
        let transport: OpenAICompatibleTransport =
            Arc::new(|_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({
                        "error": {
                            "message": "bad edit"
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_deepinfra_edit_error".to_string(),
                )])))))
            });
        let model = DeepInfraProvider::new()
            .with_transport(transport)
            .image_model("black-forest-labs/FLUX.1-Kontext-dev");
        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("bad edit")
                    .with_files(vec![ImageModelFile::file(
                        "image/png",
                        FileDataContent::Bytes(vec![137, 80, 78, 71]),
                    )]),
            ),
        );

        assert!(result.images.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("deepinfra"))
                .and_then(|metadata| metadata.extra.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("bad edit")
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_deepinfra_edit_error")
        );
    }

    #[test]
    fn deepinfra_provider_implements_provider_trait() {
        let provider = DeepInfraProvider::new();
        let model = Provider::language_model(&provider, "meta-llama/Llama-3.3-70B-Instruct")
            .expect("language model is supported");

        assert_eq!(model.provider(), "deepinfra.chat");
        assert_eq!(model.model_id(), "meta-llama/Llama-3.3-70B-Instruct");
        let embedding = Provider::embedding_model(&provider, "BAAI/bge-large-en-v1.5")
            .expect("embedding model is supported");
        assert_eq!(embedding.provider(), "deepinfra.embedding");
        let image = Provider::image_model(&provider, "black-forest-labs/FLUX-1-schnell")
            .expect("image model is supported");
        assert_eq!(image.provider(), "deepinfra.image");
        assert_eq!(image.model_id(), "black-forest-labs/FLUX-1-schnell");
    }

    #[test]
    fn deepinfra_provider_settings_serde_accepts_upstream_base_url() {
        let settings: DeepInfraProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.deepinfra.test/v1",
            "apiKey": "test-key",
            "headers": {
                "custom-header": "value"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            DeepInfraProviderSettings::new()
                .with_base_url("https://api.deepinfra.test/v1")
                .with_api_key("test-key")
                .with_header("custom-header", "value")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.deepinfra.test/v1",
                "apiKey": "test-key",
                "headers": {
                    "custom-header": "value"
                }
            })
        );
    }

    fn sse_body(events: impl IntoIterator<Item = JsonValue>) -> String {
        events
            .into_iter()
            .map(|event| format!("data: {event}\n\n"))
            .chain(["data: [DONE]\n\n".to_string()])
            .collect()
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
