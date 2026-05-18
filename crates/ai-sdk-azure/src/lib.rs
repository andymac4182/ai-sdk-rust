use std::env;
use std::sync::Arc;

use ai_sdk_rust::{
    Headers, NoSuchModelError, OpenAICompatibleChatLanguageModel,
    OpenAICompatibleCompletionLanguageModel, OpenAICompatibleEmbeddingModel,
    OpenAICompatibleImageModel, OpenAICompatibleProvider, OpenAICompatibleProviderSettings,
    OpenAICompatibleTransport, OpenResponsesLanguageModel, OpenResponsesProvider,
    OpenResponsesProviderSettings, Provider, without_trailing_slash,
};
use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::{
        AzureOpenAIProvider, AzureOpenAIProviderSettings, DEFAULT_AZURE_OPENAI_API_VERSION, azure,
        create_azure,
    };
    use ai_sdk_rust::{
        EmbeddingModel, EmbeddingModelCallOptions, GenerateTextOptions, Headers, ImageModel,
        ImageModelCallOptions, JsonValue, OpenAICompatibleTransport,
        OpenAICompatibleTransportFuture, Prompt, Provider, ProviderApiRequest,
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
