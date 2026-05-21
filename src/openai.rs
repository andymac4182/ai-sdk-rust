use std::env;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::headers::Headers;
use crate::open_responses::{
    OpenResponsesLanguageModel, OpenResponsesProvider, OpenResponsesProviderSettings,
};
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleCompletionLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel, OpenAICompatibleProvider,
    OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
};
use crate::provider::{NoSuchModelError, Provider};
use crate::provider_utils::without_trailing_slash;

/// Default base URL for upstream `@ai-sdk/openai` API calls.
pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

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
    pub fn image(&self, model_id: impl Into<String>) -> OpenAICompatibleImageModel {
        self.openai_compatible_provider().image_model(model_id)
    }

    /// Creates an OpenAI image model.
    pub fn image_model(&self, model_id: impl Into<String>) -> OpenAICompatibleImageModel {
        self.image(model_id)
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let provider_name = openai_provider_name(&self.settings);
        let mut settings =
            OpenAICompatibleProviderSettings::new(provider_name, openai_base_url(&self.settings))
                .with_user_agent_suffix(format!("ai-sdk/openai/{}", crate::VERSION));

        if let Some(api_key) = openai_api_key(self.settings.api_key.as_ref()) {
            settings = settings.with_api_key(api_key);
        }

        if let Some(organization) = non_empty_optional_setting(self.settings.organization.clone()) {
            settings = settings.with_header("OpenAI-Organization", organization);
        }

        if let Some(project) = non_empty_optional_setting(self.settings.project.clone()) {
            settings = settings.with_header("OpenAI-Project", project);
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

    fn open_responses_provider(&self) -> OpenResponsesProvider {
        let provider_name = openai_provider_name(&self.settings);
        let mut settings = OpenResponsesProviderSettings::new(
            provider_name,
            format!("{}/responses", openai_base_url(&self.settings)),
        )
        .with_user_agent_suffix(format!("ai-sdk/openai/{}", crate::VERSION))
        .with_file_id_prefix("file-");

        if let Some(api_key) = openai_api_key(self.settings.api_key.as_ref()) {
            settings = settings.with_api_key(api_key);
        }

        if let Some(organization) = non_empty_optional_setting(self.settings.organization.clone()) {
            settings = settings.with_header("OpenAI-Organization", organization);
        }

        if let Some(project) = non_empty_optional_setting(self.settings.project.clone()) {
            settings = settings.with_header("OpenAI-Project", project);
        }

        for (name, value) in &self.settings.headers {
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
    type ImageModel = OpenAICompatibleImageModel;

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

/// Creates an OpenAI provider with explicit settings.
pub fn create_openai(settings: OpenAIProviderSettings) -> OpenAIProvider {
    OpenAIProvider::from_settings(settings)
}

/// Creates an OpenAI Responses API language model using the default provider settings.
pub fn openai(model_id: impl Into<String>) -> OpenResponsesLanguageModel {
    OpenAIProvider::new().language_model(model_id)
}

fn openai_base_url(settings: &OpenAIProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .or_else(|| non_empty_optional_setting(env::var("OPENAI_BASE_URL").ok()))
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

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_OPENAI_BASE_URL, OpenAIProvider, OpenAIProviderSettings, create_openai};
    use crate::embed::{EmbedManyOptions, embed_many};
    use crate::file_data::{FileData, FileDataContent};
    use crate::generate_image::{GenerateImageOptions, GenerateImagePrompt, generate_image};
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::language_model::{
        LanguageModel, LanguageModelCallOptions, LanguageModelFilePart, LanguageModelMessage,
        LanguageModelUserContentPart, LanguageModelUserMessage,
    };
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{Provider, ProviderOptions};
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

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
