use std::collections::BTreeMap;
use std::env;
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_rust::{
    EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResult, EmbeddingModelUsage,
    FinishReason, Headers, InputTokenUsage, JsonObject, JsonValue, LanguageModel,
    LanguageModelCallOptions, LanguageModelContent, LanguageModelErrorStreamPart,
    LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelMessage,
    LanguageModelRawStreamPart, LanguageModelReasoning, LanguageModelReasoningDelta,
    LanguageModelReasoningEffort, LanguageModelReasoningEnd, LanguageModelReasoningStart,
    LanguageModelRequest, LanguageModelResponse, LanguageModelResponseFormat,
    LanguageModelStreamFinish, LanguageModelStreamPart, LanguageModelStreamResponseMetadata,
    LanguageModelStreamResult, LanguageModelStreamResultResponse, LanguageModelSupportedUrls,
    LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextStart,
    LanguageModelTool, LanguageModelToolCall, LanguageModelToolChoice, LanguageModelToolInputDelta,
    LanguageModelToolInputEnd, LanguageModelToolInputStart, LanguageModelUsage as LmUsage,
    ModelType, NoSuchModelError, OutputTokenUsage, Provider, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, RuntimeEnvironment, combine_headers, convert_to_base64,
    create_event_source_response_handler, create_json_error_response_handler,
    create_json_response_handler, generate_id, get_top_level_media_type,
    inject_json_instruction_into_messages, parse_provider_options, post_json_to_api,
    resolve_full_media_type, without_trailing_slash,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::future::Ready;
use time::OffsetDateTime;

/// Default base URL for upstream `@ai-sdk/mistral` API calls.
pub const DEFAULT_MISTRAL_BASE_URL: &str = "https://api.mistral.ai/v1";

/// Settings for the upstream Mistral provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MistralProviderSettings {
    /// Base URL for Mistral API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Mistral API key. When omitted, `MISTRAL_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl MistralProviderSettings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

type MistralTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, ai_sdk_rust::FetchErrorInfo>> + Send>>;
type MistralTransport = Arc<dyn Fn(ProviderApiRequest) -> MistralTransportFuture + Send + Sync>;

/// Upstream Mistral provider foundation.
#[derive(Clone)]
pub struct MistralProvider {
    settings: MistralProviderSettings,
    transport: MistralTransport,
}

impl MistralProvider {
    pub fn new() -> Self {
        Self::from_settings(MistralProviderSettings::new())
    }

    pub fn from_settings(settings: MistralProviderSettings) -> Self {
        Self {
            settings,
            transport: default_mistral_transport(),
        }
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    pub fn with_transport(mut self, transport: MistralTransport) -> Self {
        self.transport = transport;
        self
    }

    pub fn language_model(&self, model_id: impl Into<String>) -> MistralChatLanguageModel {
        self.chat(model_id)
    }

    pub fn chat(&self, model_id: impl Into<String>) -> MistralChatLanguageModel {
        MistralChatLanguageModel::new(
            model_id.into(),
            MistralChatModelConfig {
                provider: "mistral.chat".to_string(),
                base_url: mistral_base_url(&self.settings),
                headers: self.settings.headers.clone(),
                api_key: mistral_api_key(self.settings.api_key.as_ref()),
                transport: Arc::clone(&self.transport),
            },
        )
    }

    pub fn embedding(&self, model_id: impl Into<String>) -> MistralEmbeddingModel {
        self.embedding_model(model_id)
    }

    pub fn embedding_model(&self, model_id: impl Into<String>) -> MistralEmbeddingModel {
        MistralEmbeddingModel::new(
            model_id.into(),
            MistralEmbeddingModelConfig {
                provider: "mistral.embedding".to_string(),
                base_url: mistral_base_url(&self.settings),
                headers: self.settings.headers.clone(),
                api_key: mistral_api_key(self.settings.api_key.as_ref()),
                transport: Arc::clone(&self.transport),
            },
        )
    }

    pub fn text_embedding(&self, model_id: impl Into<String>) -> MistralEmbeddingModel {
        self.embedding_model(model_id)
    }

    pub fn text_embedding_model(&self, model_id: impl Into<String>) -> MistralEmbeddingModel {
        self.embedding_model(model_id)
    }

    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<ai_sdk_rust::OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }
}

impl Default for MistralProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for MistralProvider {
    type LanguageModel = MistralChatLanguageModel;
    type EmbeddingModel = MistralEmbeddingModel;
    type ImageModel = ai_sdk_rust::OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(MistralProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(MistralProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        self.image_model(model_id)
    }
}

/// Creates a Mistral provider with explicit settings.
pub fn create_mistral(settings: MistralProviderSettings) -> MistralProvider {
    MistralProvider::from_settings(settings)
}

/// Creates a Mistral chat language model using default provider settings.
pub fn mistral(model_id: impl Into<String>) -> MistralChatLanguageModel {
    MistralProvider::new().language_model(model_id)
}

fn mistral_base_url(settings: &MistralProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_MISTRAL_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn mistral_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("MISTRAL_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[derive(Clone)]
struct MistralChatModelConfig {
    provider: String,
    base_url: String,
    headers: Headers,
    api_key: Option<String>,
    transport: MistralTransport,
}

/// Supported Mistral chat provider options.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MistralLanguageModelChatOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_prompt: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_image_limit: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_page_limit: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_outputs: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict_json_schema: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

/// Mistral chat language model.
#[derive(Clone)]
pub struct MistralChatLanguageModel {
    model_id: String,
    config: MistralChatModelConfig,
}

impl MistralChatLanguageModel {
    fn new(model_id: String, config: MistralChatModelConfig) -> Self {
        Self { model_id, config }
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(
                mistral_provider_headers(&self.config)
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

    fn get_args(
        &self,
        options: &LanguageModelCallOptions,
    ) -> Result<(JsonValue, Vec<ai_sdk_rust::warning::Warning>), String> {
        let mut warnings = Vec::new();
        let provider_options: MistralLanguageModelChatOptions =
            parse_provider_options("mistral", options.provider_options.as_ref(), |value| {
                serde_json::from_value(value.clone()).map_err(|error| error.to_string())
            })
            .map_err(|error| error.to_string())?
            .unwrap_or_default();

        if options.top_k.is_some() {
            warnings.push(ai_sdk_rust::warning::Warning::Unsupported {
                feature: "topK".to_string(),
                details: None,
            });
        }
        if options.frequency_penalty.is_some() {
            warnings.push(ai_sdk_rust::warning::Warning::Unsupported {
                feature: "frequencyPenalty".to_string(),
                details: None,
            });
        }
        if options.presence_penalty.is_some() {
            warnings.push(ai_sdk_rust::warning::Warning::Unsupported {
                feature: "presencePenalty".to_string(),
                details: None,
            });
        }

        let supports_reasoning_effort = matches!(
            self.model_id.as_str(),
            "mistral-small-latest"
                | "mistral-small-2603"
                | "mistral-medium-3"
                | "mistral-medium-3.5"
                | "magistral-medium-latest"
                | "magistral-small-latest"
                | "magistral-medium-2509"
                | "magistral-small-2509"
        );

        let mut resolved_reasoning_effort = provider_options.reasoning_effort.clone();
        if resolved_reasoning_effort.is_none() {
            resolved_reasoning_effort = match options
                .reasoning
                .as_ref()
                .cloned()
                .unwrap_or(LanguageModelReasoningEffort::ProviderDefault)
            {
                LanguageModelReasoningEffort::ProviderDefault => None,
                LanguageModelReasoningEffort::None => Some("none".to_string()),
                LanguageModelReasoningEffort::Minimal
                | LanguageModelReasoningEffort::Low
                | LanguageModelReasoningEffort::Medium
                | LanguageModelReasoningEffort::High
                | LanguageModelReasoningEffort::Xhigh
                    if supports_reasoning_effort =>
                {
                    Some("high".to_string())
                }
                other => {
                    warnings.push(ai_sdk_rust::warning::Warning::Unsupported {
                        feature: "reasoning".to_string(),
                        details: Some(format!(
                            "This model does not support reasoning configuration ({other:?})."
                        )),
                    });
                    None
                }
            };
        }

        let structured_outputs = provider_options.structured_outputs.unwrap_or(true);
        let strict_json_schema = provider_options.strict_json_schema.unwrap_or(false);

        let mut prompt = options.prompt.clone();
        if matches!(
            options.response_format.as_ref(),
            Some(LanguageModelResponseFormat::Json { schema: None, .. })
        ) {
            prompt = inject_json_instruction_into_messages(
                ai_sdk_rust::InjectJsonInstructionIntoMessagesOptions::new(prompt),
            );
        }

        let mut body = JsonObject::new();
        body.insert(
            "model".to_string(),
            JsonValue::String(self.model_id.clone()),
        );
        body.insert(
            "safe_prompt".to_string(),
            json!(provider_options.safe_prompt.unwrap_or(false)),
        );

        if let Some(max_output_tokens) = options.max_output_tokens {
            body.insert("max_tokens".to_string(), json!(max_output_tokens));
        }
        if let Some(temperature) = options.temperature {
            body.insert("temperature".to_string(), json!(temperature));
        }
        if let Some(top_p) = options.top_p {
            body.insert("top_p".to_string(), json!(top_p));
        }
        if let Some(stop_sequences) = &options.stop_sequences {
            body.insert("stop".to_string(), json!(stop_sequences));
        }
        if let Some(seed) = options.seed {
            body.insert("random_seed".to_string(), json!(seed));
        }
        if let Some(reasoning_effort) = resolved_reasoning_effort {
            body.insert(
                "reasoning_effort".to_string(),
                JsonValue::String(reasoning_effort),
            );
        }
        if let Some(response_format) = &options.response_format {
            if matches!(response_format, LanguageModelResponseFormat::Json { .. }) {
                let response_format_value = match response_format {
                    LanguageModelResponseFormat::Json {
                        schema: Some(schema),
                        name,
                        description,
                    } if structured_outputs => {
                        json!({
                            "type": "json_schema",
                            "json_schema": {
                                "schema": schema,
                                "strict": strict_json_schema,
                                "name": name.clone().unwrap_or_else(|| "response".to_string()),
                                "description": description.clone(),
                            }
                        })
                    }
                    _ => json!({ "type": "json_object" }),
                };
                body.insert("response_format".to_string(), response_format_value);
            }
        }
        if let Some(value) = provider_options.document_image_limit {
            body.insert("document_image_limit".to_string(), json!(value));
        }
        if let Some(value) = provider_options.document_page_limit {
            body.insert("document_page_limit".to_string(), json!(value));
        }

        let messages = convert_to_mistral_chat_messages(&prompt)?;
        body.insert("messages".to_string(), JsonValue::Array(messages));

        let prepared = prepare_tools(&options.tools, &options.tool_choice, &mut warnings)?;
        let has_tools = prepared.tools.is_some();
        if let Some(tools) = prepared.tools {
            body.insert("tools".to_string(), JsonValue::Array(tools));
        }
        if let Some(tool_choice) = prepared.tool_choice {
            body.insert("tool_choice".to_string(), tool_choice);
        }
        if has_tools && provider_options.parallel_tool_calls.is_some() {
            body.insert(
                "parallel_tool_calls".to_string(),
                json!(provider_options.parallel_tool_calls.unwrap()),
            );
        }

        Ok((JsonValue::Object(body), warnings))
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let (body, warnings) = match self.get_args(&options) {
            Ok(result) => result,
            Err(error) => {
                return mistral_error_generate_result(
                    &self.model_id,
                    &error,
                    options.prompt.clone(),
                    json!({ "model": self.model_id }),
                );
            }
        };
        let request_body_for_response = body.clone();
        let request_body_for_error = body.clone();

        let post_options = ai_sdk_rust::PostJsonToApiOptions::new(
            format!("{}/chat/completions", self.config.base_url),
            body,
        )
        .with_headers(self.request_headers(options.headers.as_ref()))
        .with_environment(RuntimeEnvironment::unknown())
        .with_optional_abort_signal(options.abort_signal.clone());

        let transport = Arc::clone(&self.config.transport);
        let result = post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    |value| {
                        serde_json::from_value::<MistralResponse>(value.clone())
                            .map_err(|error| error.to_string())
                    },
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    |value| {
                        serde_json::from_value::<MistralErrorData>(value.clone())
                            .map_err(|error| error.to_string())
                    },
                    |error| error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await;

        match result {
            Ok(response) => {
                let content = mistral_response_content(&response.value.choices[0].message);
                let finish_reason =
                    mistral_finish_reason(response.value.choices[0].finish_reason.as_deref());
                let usage = convert_mistral_usage(response.value.usage.as_ref());
                let mut result = LanguageModelGenerateResult::new(content, finish_reason, usage)
                    .with_request(
                        LanguageModelRequest::new()
                            .with_messages(options.prompt.clone())
                            .with_body(request_body_for_response),
                    )
                    .with_response(mistral_response_metadata(
                        response.value.id.clone(),
                        response.value.created,
                        response.value.model.clone(),
                        response.response_headers,
                        response.raw_value,
                    ));
                for warning in warnings {
                    result = result.with_warning(warning);
                }
                result
            }
            Err(error) => mistral_error_generate_result(
                &self.model_id,
                &format!("{error:?}"),
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
        let (body, warnings) = match self.get_args(&options) {
            Ok(result) => result,
            Err(error) => {
                return mistral_error_stream_result(&error, json!({ "model": self.model_id }));
            }
        };

        let mut body = body;
        if let JsonValue::Object(map) = &mut body {
            map.insert("stream".to_string(), JsonValue::Bool(true));
        }

        let request_body_for_response = body.clone();
        let request_body_for_error = body.clone();
        let post_options =
            ai_sdk_rust::PostJsonToApiOptions::new(self.config.base_url.clone(), body)
                .with_headers(self.request_headers(options.headers.as_ref()))
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(options.abort_signal.clone());

        let transport = Arc::clone(&self.config.transport);
        let result = post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |_request, response| {
                create_event_source_response_handler(
                    response.event_source_response_handler_options(),
                    |value| {
                        serde_json::from_value::<MistralStreamChunk>(value.clone())
                            .map_err(|error| error.to_string())
                    },
                )
                .map_err(|error| ProviderApiResponseHandlerError::other(error.to_string()))
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    |value| {
                        serde_json::from_value::<MistralErrorData>(value.clone())
                            .map_err(|error| error.to_string())
                    },
                    |error| error.message.clone(),
                    |_, _| None,
                ))
            },
        )
        .await;

        match result {
            Ok(response) => mistral_stream_from_response(
                response.value,
                response.response_headers,
                response.raw_value,
                request_body_for_response,
                warnings,
                include_raw_chunks,
            ),
            Err(error) => {
                mistral_error_stream_result(&format!("{error:?}"), request_body_for_error)
            }
        }
    }
}

impl LanguageModel for MistralChatLanguageModel {
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
        ready(BTreeMap::from([(
            "application/pdf".to_string(),
            vec!["^https:\\/\\/.*$".to_string()],
        )]))
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(self.do_stream_result(options))
    }
}

#[derive(Clone)]
struct MistralEmbeddingModelConfig {
    provider: String,
    base_url: String,
    headers: Headers,
    api_key: Option<String>,
    transport: MistralTransport,
}

#[derive(Clone)]
pub struct MistralEmbeddingModel {
    model_id: String,
    config: MistralEmbeddingModelConfig,
}

impl MistralEmbeddingModel {
    fn new(model_id: String, config: MistralEmbeddingModelConfig) -> Self {
        Self { model_id, config }
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(
                mistral_provider_headers_embedding(&self.config)
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

impl EmbeddingModel for MistralEmbeddingModel {
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
        ready(Some(32))
    }

    fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
        ready(false)
    }

    fn do_embed(&self, options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
        let model_id = self.model_id.clone();
        let config = self.config.clone();
        Box::pin(async move {
            let request_body = json!({
                "model": model_id,
                "input": options.values,
                "encoding_format": "float"
            });
            let post_options = ai_sdk_rust::PostJsonToApiOptions::new(
                format!("{}/embeddings", config.base_url),
                request_body.clone(),
            )
            .with_headers(self.request_headers(options.headers.as_ref()))
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());

            let transport = Arc::clone(&config.transport);
            let result = post_json_to_api(
                post_options,
                move |request| (transport)(request),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        |value| {
                            serde_json::from_value::<MistralEmbeddingResponse>(value.clone())
                                .map_err(|error| error.to_string())
                        },
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        |value| {
                            serde_json::from_value::<MistralErrorData>(value.clone())
                                .map_err(|error| error.to_string())
                        },
                        |error| error.message.clone(),
                        |_, _| None,
                    ))
                },
            )
            .await;

            match result {
                Ok(response) => EmbeddingModelResult {
                    embeddings: response
                        .value
                        .data
                        .into_iter()
                        .map(|item| item.embedding)
                        .collect(),
                    usage: response.value.usage.map(|usage| EmbeddingModelUsage {
                        tokens: usage.prompt_tokens,
                    }),
                    provider_metadata: None,
                    response: Some(ai_sdk_rust::EmbeddingModelResponse {
                        headers: response.response_headers,
                        body: response.raw_value,
                    }),
                    warnings: Vec::new(),
                },
                Err(_error) => EmbeddingModelResult {
                    embeddings: Vec::new(),
                    usage: None,
                    provider_metadata: None,
                    response: None,
                    warnings: Vec::new(),
                },
            }
        })
    }
}

/// Creates a Mistral provider with explicit settings.
pub fn mistral_provider() -> MistralProvider {
    MistralProvider::new()
}

fn mistral_provider_headers(config: &MistralChatModelConfig) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    if let Some(api_key) = &config.api_key {
        headers.insert("authorization".to_string(), format!("Bearer {api_key}"));
    }
    for (name, value) in &config.headers {
        headers.insert(name.clone(), value.clone());
    }
    headers.insert(
        "user-agent".to_string(),
        format!("ai-sdk/mistral/{}", ai_sdk_rust::VERSION),
    );
    headers
}

fn mistral_provider_headers_embedding(
    config: &MistralEmbeddingModelConfig,
) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    if let Some(api_key) = &config.api_key {
        headers.insert("authorization".to_string(), format!("Bearer {api_key}"));
    }
    for (name, value) in &config.headers {
        headers.insert(name.clone(), value.clone());
    }
    headers.insert(
        "user-agent".to_string(),
        format!("ai-sdk/mistral/{}", ai_sdk_rust::VERSION),
    );
    headers
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    #[serde(default)]
    num_cached_tokens: Option<u64>,
    #[serde(default)]
    prompt_tokens_details: Option<MistralPromptTokensDetails>,
    #[serde(default)]
    prompt_token_details: Option<MistralPromptTokensDetails>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralPromptTokensDetails {
    #[serde(default)]
    cached_tokens: Option<u64>,
}

fn convert_mistral_usage(usage: Option<&MistralUsage>) -> LmUsage {
    if usage.is_none() {
        return LmUsage {
            input_tokens: InputTokenUsage {
                total: None,
                no_cache: None,
                cache_read: None,
                cache_write: None,
            },
            output_tokens: OutputTokenUsage {
                total: None,
                text: None,
                reasoning: None,
            },
            raw: None,
        };
    }

    let usage = usage.expect("checked is_some");
    let cache_read = usage
        .num_cached_tokens
        .or_else(|| {
            usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|details| details.cached_tokens)
        })
        .or_else(|| {
            usage
                .prompt_token_details
                .as_ref()
                .and_then(|details| details.cached_tokens)
        })
        .unwrap_or(0);
    let raw = serde_json::to_value(usage)
        .ok()
        .and_then(|value| value.as_object().cloned());

    LmUsage {
        input_tokens: InputTokenUsage {
            total: Some(usage.prompt_tokens),
            no_cache: Some(usage.prompt_tokens.saturating_sub(cache_read)),
            cache_read: if cache_read == 0 {
                None
            } else {
                Some(cache_read)
            },
            cache_write: None,
        },
        output_tokens: OutputTokenUsage {
            total: Some(usage.completion_tokens),
            text: Some(usage.completion_tokens),
            reasoning: None,
        },
        raw,
    }
}

fn mistral_finish_reason(raw: Option<&str>) -> LanguageModelFinishReason {
    let raw = raw.map(str::to_string);
    let unified = match raw.as_deref() {
        Some("stop") => FinishReason::Stop,
        Some("length") | Some("model_length") => FinishReason::Length,
        Some("tool_calls") => FinishReason::ToolCalls,
        _ => FinishReason::Other,
    };

    LanguageModelFinishReason { unified, raw }
}

fn mistral_response_metadata(
    id: Option<String>,
    created: Option<u64>,
    model: Option<String>,
    headers: Option<Headers>,
    body: Option<JsonValue>,
) -> LanguageModelResponse {
    let mut response = LanguageModelResponse {
        messages: None,
        id,
        timestamp: None,
        model_id: model,
        headers,
        body,
    };
    if let Some(timestamp) =
        created.and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds as i64).ok())
    {
        response.timestamp = Some(timestamp);
    }
    response
}

fn mistral_response_content(message: &MistralResponseMessage) -> Vec<LanguageModelContent> {
    let mut content = Vec::new();

    if let Some(message_content) = &message.content {
        if let Some(text) = message_content.as_str() {
            if !text.is_empty() {
                content.push(LanguageModelContent::Text(LanguageModelText::new(text)));
            }
        } else if let Some(parts) = message_content.as_array() {
            for part in parts {
                match part.get("type").and_then(JsonValue::as_str) {
                    Some("text") => {
                        if let Some(text) = part.get("text").and_then(JsonValue::as_str)
                            && !text.is_empty()
                        {
                            content.push(LanguageModelContent::Text(LanguageModelText::new(text)));
                        }
                    }
                    Some("thinking") => {
                        let reasoning = part
                            .get("thinking")
                            .and_then(JsonValue::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(|chunk| {
                                if chunk.get("type").and_then(JsonValue::as_str) == Some("text") {
                                    chunk.get("text").and_then(JsonValue::as_str)
                                } else {
                                    None
                                }
                            })
                            .collect::<String>();
                        if !reasoning.is_empty() {
                            content.push(LanguageModelContent::Reasoning(
                                LanguageModelReasoning::new(reasoning),
                            ));
                        }
                    }
                    Some("image_url") | Some("reference") => {}
                    _ => {}
                }
            }
        }
    }

    if let Some(tool_calls) = &message.tool_calls {
        for tool_call in tool_calls {
            content.push(LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                tool_call.id.clone(),
                tool_call.function.name.clone(),
                tool_call.function.arguments.clone(),
            )));
        }
    }

    content
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralResponse {
    id: Option<String>,
    created: Option<u64>,
    model: Option<String>,
    choices: Vec<MistralResponseChoice>,
    #[serde(default)]
    usage: Option<MistralUsage>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralResponseChoice {
    message: MistralResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralResponseMessage {
    #[serde(default)]
    content: Option<JsonValue>,
    #[serde(default)]
    tool_calls: Option<Vec<MistralResponseToolCall>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralResponseToolCall {
    id: String,
    function: MistralResponseToolCallFunction,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralResponseToolCallFunction {
    name: String,
    arguments: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralEmbeddingResponse {
    data: Vec<MistralEmbeddingResponseItem>,
    #[serde(default)]
    usage: Option<MistralEmbeddingUsage>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralEmbeddingResponseItem {
    embedding: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralEmbeddingUsage {
    prompt_tokens: u64,
    total_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralErrorData {
    object: String,
    message: String,
    #[serde(rename = "type")]
    error_type: String,
    param: Option<String>,
    code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralStreamChunk {
    id: Option<String>,
    created: Option<u64>,
    model: Option<String>,
    choices: Vec<MistralStreamChoice>,
    #[serde(default)]
    usage: Option<MistralUsage>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralStreamChoice {
    delta: MistralStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MistralStreamDelta {
    #[serde(default)]
    content: Option<JsonValue>,
    #[serde(default)]
    tool_calls: Option<Vec<MistralResponseToolCall>>,
}

fn mistral_stream_from_response(
    response: Vec<ai_sdk_rust::ParseJsonResult<MistralStreamChunk>>,
    response_headers: Option<Headers>,
    _raw_response: Option<JsonValue>,
    request_body: JsonValue,
    warnings: Vec<ai_sdk_rust::warning::Warning>,
    include_raw_chunks: bool,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let mut parts = Vec::new();
    parts.push(LanguageModelStreamPart::StreamStart(
        ai_sdk_rust::LanguageModelStreamStart::new(warnings),
    ));

    let mut emitted_metadata = false;
    let mut active_text = false;
    let mut active_reasoning: Option<String> = None;
    let mut finish_reason = LanguageModelFinishReason {
        unified: FinishReason::Other,
        raw: None,
    };
    let mut usage: Option<MistralUsage> = None;

    for chunk in response {
        if include_raw_chunks {
            if let Some(raw_value) = chunk.raw_value() {
                parts.push(LanguageModelStreamPart::Raw(
                    LanguageModelRawStreamPart::new(raw_value.clone()),
                ));
            }
        }

        let value = match chunk {
            ai_sdk_rust::ParseJsonResult::Success { value, .. } => value,
            ai_sdk_rust::ParseJsonResult::Failure { error, .. } => {
                parts.push(LanguageModelStreamPart::Error(
                    LanguageModelErrorStreamPart::new(json!({ "error": error.to_string() })),
                ));
                continue;
            }
        };

        if !emitted_metadata {
            emitted_metadata = true;
            let mut metadata = LanguageModelStreamResponseMetadata::new()
                .with_id(value.id.clone().unwrap_or_default())
                .with_model_id(value.model.clone().unwrap_or_default());
            if let Some(timestamp) = value
                .created
                .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds as i64).ok())
            {
                metadata = metadata.with_timestamp(timestamp);
            }
            parts.push(LanguageModelStreamPart::ResponseMetadata(metadata));
        }

        if let Some(new_usage) = value.usage.clone() {
            usage = Some(new_usage);
        }

        let choice = &value.choices[0];
        let delta = &choice.delta;

        if let Some(content) = &delta.content {
            if let Some(text) = content.as_str() {
                if !text.is_empty() {
                    if let Some(reasoning_id) = active_reasoning.take() {
                        parts.push(LanguageModelStreamPart::ReasoningEnd(
                            LanguageModelReasoningEnd::new(reasoning_id),
                        ));
                    }
                    if !active_text {
                        parts.push(LanguageModelStreamPart::TextStart(
                            LanguageModelTextStart::new("0"),
                        ));
                        active_text = true;
                    }
                    parts.push(LanguageModelStreamPart::TextDelta(
                        LanguageModelTextDelta::new("0", text),
                    ));
                }
            } else if let Some(arr) = content.as_array() {
                for part in arr {
                    match part.get("type").and_then(JsonValue::as_str) {
                        Some("thinking") => {
                            let reasoning = part
                                .get("thinking")
                                .and_then(JsonValue::as_array)
                                .into_iter()
                                .flatten()
                                .filter_map(|chunk| {
                                    if chunk.get("type").and_then(JsonValue::as_str) == Some("text")
                                    {
                                        chunk.get("text").and_then(JsonValue::as_str)
                                    } else {
                                        None
                                    }
                                })
                                .collect::<String>();
                            if !reasoning.is_empty() {
                                if active_reasoning.is_none() {
                                    if active_text {
                                        parts.push(LanguageModelStreamPart::TextEnd(
                                            LanguageModelTextEnd::new("0"),
                                        ));
                                        active_text = false;
                                    }
                                    let reasoning_id = generate_id();
                                    parts.push(LanguageModelStreamPart::ReasoningStart(
                                        LanguageModelReasoningStart::new(reasoning_id.clone()),
                                    ));
                                    active_reasoning = Some(reasoning_id);
                                }
                                if let Some(reasoning_id) = &active_reasoning {
                                    parts.push(LanguageModelStreamPart::ReasoningDelta(
                                        LanguageModelReasoningDelta::new(
                                            reasoning_id.clone(),
                                            reasoning,
                                        ),
                                    ));
                                }
                            }
                        }
                        Some("text") => {
                            if let Some(text) = part.get("text").and_then(JsonValue::as_str)
                                && !text.is_empty()
                            {
                                if let Some(reasoning_id) = active_reasoning.take() {
                                    parts.push(LanguageModelStreamPart::ReasoningEnd(
                                        LanguageModelReasoningEnd::new(reasoning_id),
                                    ));
                                }
                                if !active_text {
                                    parts.push(LanguageModelStreamPart::TextStart(
                                        LanguageModelTextStart::new("0"),
                                    ));
                                    active_text = true;
                                }
                                parts.push(LanguageModelStreamPart::TextDelta(
                                    LanguageModelTextDelta::new("0", text),
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if let Some(tool_calls) = &delta.tool_calls {
            for tool_call in tool_calls {
                let tool_call_id = tool_call.id.clone();
                parts.push(LanguageModelStreamPart::ToolInputStart(
                    LanguageModelToolInputStart::new(
                        &tool_call_id,
                        tool_call.function.name.clone(),
                    ),
                ));
                parts.push(LanguageModelStreamPart::ToolInputDelta(
                    LanguageModelToolInputDelta::new(
                        &tool_call_id,
                        tool_call.function.arguments.clone(),
                    ),
                ));
                parts.push(LanguageModelStreamPart::ToolInputEnd(
                    LanguageModelToolInputEnd::new(&tool_call_id),
                ));
                parts.push(LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new(
                        tool_call_id,
                        tool_call.function.name.clone(),
                        tool_call.function.arguments.clone(),
                    ),
                ));
            }
        }

        if choice.finish_reason.is_some() {
            finish_reason = mistral_finish_reason(choice.finish_reason.as_deref());
        }
    }

    if let Some(reasoning_id) = active_reasoning.take() {
        parts.push(LanguageModelStreamPart::ReasoningEnd(
            LanguageModelReasoningEnd::new(reasoning_id),
        ));
    }
    if active_text {
        parts.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
            "0",
        )));
    }

    parts.push(LanguageModelStreamPart::Finish(
        LanguageModelStreamFinish::new(convert_mistral_usage(usage.as_ref()), finish_reason),
    ));

    LanguageModelStreamResult::new(parts)
        .with_request(LanguageModelRequest::new().with_body(request_body))
        .with_response(LanguageModelStreamResultResponse {
            headers: response_headers,
        })
}

fn prepare_tools(
    tools: &Option<Vec<LanguageModelTool>>,
    tool_choice: &Option<LanguageModelToolChoice>,
    warnings: &mut Vec<ai_sdk_rust::warning::Warning>,
) -> Result<PreparedTools, String> {
    let tools = tools
        .as_ref()
        .and_then(|tools| if tools.is_empty() { None } else { Some(tools) });

    let Some(tools) = tools else {
        return Ok(PreparedTools {
            tools: None,
            tool_choice: None,
        });
    };

    let mut mistral_tools = Vec::new();
    for tool in tools {
        match tool {
            LanguageModelTool::Function(function) => {
                let mut function_object = JsonObject::new();
                function_object
                    .insert("name".to_string(), JsonValue::String(function.name.clone()));
                function_object.insert(
                    "description".to_string(),
                    function
                        .description
                        .clone()
                        .map(JsonValue::String)
                        .unwrap_or(JsonValue::Null),
                );
                function_object.insert(
                    "parameters".to_string(),
                    JsonValue::Object(function.input_schema.clone()),
                );
                if let Some(strict) = function.strict {
                    function_object.insert("strict".to_string(), JsonValue::Bool(strict));
                }
                mistral_tools.push(json!({
                    "type": "function",
                    "function": function_object
                }));
            }
            LanguageModelTool::Provider(provider) => {
                warnings.push(ai_sdk_rust::warning::Warning::Unsupported {
                    feature: format!("provider-defined tool {}", provider.id),
                    details: None,
                });
            }
        }
    }

    let tool_choice = match tool_choice {
        None => None,
        Some(LanguageModelToolChoice::Auto) => Some(JsonValue::String("auto".to_string())),
        Some(LanguageModelToolChoice::None) => Some(JsonValue::String("none".to_string())),
        Some(LanguageModelToolChoice::Required) => Some(JsonValue::String("any".to_string())),
        Some(LanguageModelToolChoice::Tool { tool_name }) => {
            mistral_tools.retain(|tool| {
                tool.get("function")
                    .and_then(JsonValue::as_object)
                    .and_then(|function| function.get("name"))
                    .and_then(JsonValue::as_str)
                    == Some(tool_name.as_str())
            });
            Some(JsonValue::String("any".to_string()))
        }
    };

    Ok(PreparedTools {
        tools: Some(mistral_tools),
        tool_choice,
    })
}

struct PreparedTools {
    tools: Option<Vec<JsonValue>>,
    tool_choice: Option<JsonValue>,
}

fn convert_to_mistral_chat_messages(
    prompt: &[LanguageModelMessage],
) -> Result<Vec<JsonValue>, String> {
    let mut messages = Vec::new();

    for (index, message) in prompt.iter().enumerate() {
        let is_last_message = index == prompt.len() - 1;

        match message {
            LanguageModelMessage::System(system) => {
                messages.push(json!({
                    "role": "system",
                    "content": system.content,
                }));
            }
            LanguageModelMessage::User(user) => {
                let mut content = Vec::new();
                for part in &user.content {
                    match part {
                        ai_sdk_rust::LanguageModelUserContentPart::Text(text) => {
                            content.push(json!({ "type": "text", "text": text.text }));
                        }
                        ai_sdk_rust::LanguageModelUserContentPart::File(file) => {
                            content.push(mistral_user_file_part(file)?);
                        }
                    }
                }
                messages.push(json!({
                    "role": "user",
                    "content": content,
                }));
            }
            LanguageModelMessage::Assistant(assistant) => {
                let mut text = String::new();
                let mut tool_calls = Vec::new();
                for part in &assistant.content {
                    match part {
                        ai_sdk_rust::LanguageModelAssistantContentPart::Text(text_part) => {
                            text.push_str(&text_part.text);
                        }
                        ai_sdk_rust::LanguageModelAssistantContentPart::Reasoning(
                            reasoning_part,
                        ) => {
                            text.push_str(&reasoning_part.text);
                        }
                        ai_sdk_rust::LanguageModelAssistantContentPart::ToolCall(tool_call) => {
                            tool_calls.push(json!({
                                "id": tool_call.tool_call_id,
                                "type": "function",
                                "function": {
                                    "name": tool_call.tool_name,
                                    "arguments": tool_call.input.to_string(),
                                }
                            }));
                        }
                        _ => {}
                    }
                }

                let mut message_object = json!({
                    "role": "assistant",
                    "content": text,
                });
                if is_last_message {
                    message_object["prefix"] = JsonValue::Bool(true);
                }
                if !tool_calls.is_empty() {
                    message_object["tool_calls"] = JsonValue::Array(tool_calls);
                }
                messages.push(message_object);
            }
            LanguageModelMessage::Tool(tool) => {
                for part in &tool.content {
                    if let ai_sdk_rust::LanguageModelToolContentPart::ToolApprovalResponse(_) = part
                    {
                        continue;
                    }
                    let (tool_call_id, tool_name, content) = match part {
                        ai_sdk_rust::LanguageModelToolContentPart::ToolResult(result) => {
                            let content = match &result.output {
                                ai_sdk_rust::LanguageModelToolResultOutput::Text {
                                    value, ..
                                } => value.clone(),
                                ai_sdk_rust::LanguageModelToolResultOutput::Json {
                                    value, ..
                                }
                                | ai_sdk_rust::LanguageModelToolResultOutput::ErrorJson {
                                    value,
                                    ..
                                } => serde_json::to_string(value)
                                    .map_err(|error| error.to_string())?,
                                ai_sdk_rust::LanguageModelToolResultOutput::ExecutionDenied {
                                    reason,
                                    ..
                                } => reason
                                    .clone()
                                    .unwrap_or_else(|| "Tool call execution denied.".to_string()),
                                ai_sdk_rust::LanguageModelToolResultOutput::ErrorText {
                                    value,
                                    ..
                                } => value.clone(),
                                ai_sdk_rust::LanguageModelToolResultOutput::Content { value } => {
                                    serde_json::to_string(value)
                                        .map_err(|error| error.to_string())?
                                }
                            };
                            (
                                result.tool_call_id.clone(),
                                result.tool_name.clone(),
                                content,
                            )
                        }
                        ai_sdk_rust::LanguageModelToolContentPart::ToolApprovalResponse(_) => {
                            unreachable!()
                        }
                    };
                    messages.push(json!({
                        "role": "tool",
                        "name": tool_name,
                        "tool_call_id": tool_call_id,
                        "content": content,
                    }));
                }
            }
        }
    }

    Ok(messages)
}

fn mistral_user_file_part(part: &ai_sdk_rust::LanguageModelFilePart) -> Result<JsonValue, String> {
    match &part.data {
        ai_sdk_rust::FileData::Reference { .. } => {
            Err("file parts with provider references".to_string())
        }
        ai_sdk_rust::FileData::Text { .. } => Err("text file parts".to_string()),
        ai_sdk_rust::FileData::Url { .. } | ai_sdk_rust::FileData::Data { .. } => {
            let top_level = get_top_level_media_type(&part.media_type);
            if top_level == "image" {
                Ok(json!({
                    "type": "image_url",
                    "image_url": mistral_file_url(part),
                }))
            } else {
                let full_media_type =
                    resolve_full_media_type(part).unwrap_or_else(|_| part.media_type.clone());
                if part.media_type != "application/pdf" && full_media_type != "application/pdf" {
                    return Err("Only images and PDF file parts are supported".to_string());
                }
                Ok(json!({
                    "type": "document_url",
                    "document_url": mistral_file_url(part),
                }))
            }
        }
    }
}

fn mistral_file_url(part: &ai_sdk_rust::LanguageModelFilePart) -> String {
    match &part.data {
        ai_sdk_rust::FileData::Url { url } => url.to_string(),
        ai_sdk_rust::FileData::Data { data } => {
            format!(
                "data:{};base64,{}",
                resolve_full_media_type(part).unwrap_or_else(|_| part.media_type.clone()),
                convert_to_base64(data)
            )
        }
        ai_sdk_rust::FileData::Reference { .. } | ai_sdk_rust::FileData::Text { .. } => {
            unreachable!()
        }
    }
}

fn mistral_error_generate_result(
    model_id: &str,
    message: &str,
    prompt: Vec<LanguageModelMessage>,
    request_body: JsonValue,
) -> LanguageModelGenerateResult {
    LanguageModelGenerateResult::new(
        Vec::new(),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("error".to_string()),
        },
        convert_mistral_usage(None),
    )
    .with_request(
        LanguageModelRequest::new()
            .with_messages(prompt)
            .with_body(request_body),
    )
    .with_response(LanguageModelResponse {
        messages: None,
        id: None,
        timestamp: None,
        model_id: Some(model_id.to_string()),
        headers: None,
        body: Some(json!({ "error": message })),
    })
}

fn mistral_error_stream_result(
    message: &str,
    request_body: JsonValue,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let error = LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!({
        "error": message,
    })));
    let finish = LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
        convert_mistral_usage(None),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("error".to_string()),
        },
    ));
    LanguageModelStreamResult::new(vec![
        LanguageModelStreamPart::StreamStart(
            ai_sdk_rust::LanguageModelStreamStart::new(Vec::new()),
        ),
        error,
        finish,
    ])
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_response(LanguageModelStreamResultResponse { headers: None })
}

fn default_mistral_transport() -> MistralTransport {
    Arc::new(|request| Box::pin(async move { execute_mistral_request(request) }))
}

fn execute_mistral_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, ai_sdk_rust::FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Post => execute_mistral_post_request(request),
        ProviderApiRequestMethod::Get => Err(ai_sdk_rust::FetchErrorInfo::new(
            "GET requests are not supported by the Mistral transport",
        )),
    }
}

fn execute_mistral_post_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, ai_sdk_rust::FetchErrorInfo> {
    let mut builder = ureq::post(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let builder = builder.config().http_status_as_error(false).build();
    let response = match request.body {
        Some(ProviderApiRequestBody::Text { content }) => builder.send(content),
        Some(ProviderApiRequestBody::Bytes { content }) => builder.send(content),
        Some(ProviderApiRequestBody::FormData { .. }) => {
            return Err(ai_sdk_rust::FetchErrorInfo::new(
                "multipart form data is not supported by the Mistral transport",
            ));
        }
        None => builder.send_empty(),
    };

    provider_api_response(response)
}

fn provider_api_response(
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<ProviderApiResponse, ai_sdk_rust::FetchErrorInfo> {
    let mut response = response.map_err(|error| {
        ai_sdk_rust::FetchErrorInfo::new("fetch failed")
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
        ai_sdk_rust::FetchErrorInfo::new("failed to read response body")
            .with_name("Error")
            .with_cause_message(error.to_string())
    })?;

    Ok(ProviderApiResponse::text(status.as_u16(), status_text, body).with_headers(headers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_sdk_rust::{
        EmbeddingModelCallOptions, GenerateTextOptions, Prompt, Provider, ProviderApiRequest,
        ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse, generate_text,
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
    fn mistral_provider_creates_chat_model_with_headers_and_base_url() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: MistralTransport = Arc::new(move |request| -> MistralTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "id": "chatcmpl-mistral",
                    "created": 1711115037,
                    "model": "mistral-small-latest",
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "content": "Hello from Mistral" },
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 4,
                        "completion_tokens": 5,
                        "total_tokens": 9
                    }
                })
                .to_string(),
            ))))
        });
        let provider = create_mistral(
            MistralProviderSettings::new()
                .with_base_url("https://proxy.example.com/v1/")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.chat("mistral-small-latest");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid"),
        ));

        assert_eq!(model.provider(), "mistral.chat");
        assert_eq!(result.text, "Hello from Mistral");
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://proxy.example.com/v1/chat/completions");
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
                .is_some_and(|value| value.contains("ai-sdk/mistral/"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
                .and_then(|body| body.get("model").cloned()),
            Some(json!("mistral-small-latest"))
        );
    }

    #[test]
    fn mistral_provider_creates_embedding_model_with_usage_and_headers() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: MistralTransport = Arc::new(move |request| -> MistralTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "data": [{
                        "object": "embedding",
                        "index": 0,
                        "embedding": [0.1, 0.2, 0.3]
                    }],
                    "usage": { "prompt_tokens": 3, "total_tokens": 3 }
                })
                .to_string(),
            ))))
        });
        let provider = MistralProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport);
        let model = provider.embedding("mistral-embed");
        let result = poll_ready(model.do_embed(EmbeddingModelCallOptions::new(vec![
            "sunny day".to_string(),
        ])));

        assert_eq!(model.provider(), "mistral.embedding");
        assert_eq!(result.embeddings, vec![vec![0.1, 0.2, 0.3]]);
        assert_eq!(result.usage.expect("usage is mapped").tokens, 3);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.mistral.ai/v1/embeddings");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
    }

    #[test]
    fn mistral_provider_uses_default_base_url_and_function_alias() {
        let provider = MistralProvider::new();
        let model = mistral("mistral-large-latest");
        let trait_model =
            Provider::language_model(&provider, "mistral-small-latest").expect("model resolves");

        assert_eq!(
            mistral_base_url(&MistralProviderSettings::new()),
            DEFAULT_MISTRAL_BASE_URL
        );
        assert_eq!(model.provider(), "mistral.chat");
        assert_eq!(model.model_id(), "mistral-large-latest");
        assert_eq!(trait_model.provider(), "mistral.chat");
        assert_eq!(trait_model.model_id(), "mistral-small-latest");
    }

    #[test]
    fn mistral_provider_reports_unsupported_image_models() {
        let provider = MistralProvider::new();
        let error = provider
            .image_model("image")
            .err()
            .expect("image models are unsupported");

        assert_eq!(error.model_id(), "image");
        assert_eq!(error.model_type(), ModelType::ImageModel);
    }

    #[test]
    fn mistral_provider_settings_serde_accepts_upstream_base_url() {
        let settings: MistralProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://proxy.example.com/v1",
            "apiKey": "key",
            "headers": { "x-provider": "mistral" }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            MistralProviderSettings::new()
                .with_base_url("https://proxy.example.com/v1")
                .with_api_key("key")
                .with_header("x-provider", "mistral")
        );
    }
}
