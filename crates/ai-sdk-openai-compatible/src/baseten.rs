//! Rust port of upstream `@ai-sdk/baseten` (`createBaseten`).
//!
//! Baseten is an OpenAI-compatible provider that wraps
//! [`OpenAICompatibleChatLanguageModel`](crate::OpenAICompatibleChatLanguageModel)
//! and [`OpenAICompatibleEmbeddingModel`](crate::OpenAICompatibleEmbeddingModel).
//! It layers Baseten-specific request configuration on top: a default Model
//! APIs base URL, `BASETEN_API_KEY` loading, an `ai-sdk/baseten/{VERSION}`
//! user-agent suffix, `/sync/v1` vs `/predict` endpoint validation for chat
//! models, and `/sync` URL rewriting for embeddings. This module ports that
//! pure configuration logic (the parts that are deterministically testable
//! without a network) so the behavior can be exercised 1:1 against the upstream
//! `baseten-provider.unit.test.ts` cases.

use ai_sdk_provider::headers::Headers;
use ai_sdk_provider::{LoadApiKeyError, ModelType, NoSuchModelError};
use ai_sdk_provider_utils::{
    LoadApiKeyOptions, load_api_key, with_user_agent_suffix, without_trailing_slash,
};

/// The Baseten crate version reported in the user-agent suffix.
pub const BASETEN_VERSION: &str = crate::VERSION;

/// Default base URL for the Baseten Model APIs.
const DEFAULT_BASE_URL: &str = "https://inference.baseten.co/v1";

/// Settings for [`BasetenProvider`], mirroring upstream `BasetenProviderSettings`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BasetenProviderSettings {
    /// Baseten API key. Defaults to the `BASETEN_API_KEY` environment variable.
    pub api_key: Option<String>,

    /// Base URL for the Model APIs. Default: `https://inference.baseten.co/v1`.
    pub base_url: Option<String>,

    /// Model URL for custom models (chat or embeddings). When unset, the
    /// default Model APIs are used.
    pub model_url: Option<String>,

    /// Custom headers included in requests.
    pub headers: Vec<(String, String)>,
}

/// The Baseten provider, mirroring upstream `createBaseten`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BasetenProvider {
    base_url: String,
    api_key: Option<String>,
    model_url: Option<String>,
    headers: Vec<(String, String)>,
}

/// The resolved configuration for a single Baseten model, mirroring the
/// upstream `CommonModelConfig` plus the constructed model id and provider name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BasetenModelConfig {
    /// The model id passed to the OpenAI-compatible model constructor.
    pub model_id: String,

    /// The provider name (`baseten.chat` or `baseten.embedding`).
    pub provider: String,

    /// The custom URL used for URL construction, if any.
    custom_url: Option<String>,

    /// Whether this config is for embeddings (controls `/sync` URL rewriting).
    is_embedding: bool,

    /// The resolved provider base URL (used when no custom URL is present).
    base_url: String,
}

impl BasetenModelConfig {
    /// Builds the request URL for the given path, mirroring upstream
    /// `getCommonModelConfig(...).url({ path })`.
    pub fn url(&self, path: &str) -> String {
        // For embeddings with /sync URLs (but not /sync/v1), append /v1.
        if self.is_embedding {
            if let Some(custom) = &self.custom_url {
                if custom.contains("/sync") && !custom.contains("/sync/v1") {
                    return format!("{custom}/v1{path}");
                }
            }
        }
        match &self.custom_url {
            Some(custom) => format!("{custom}{path}"),
            None => format!("{}{path}", self.base_url),
        }
    }
}

impl BasetenProvider {
    /// Creates a Baseten provider from settings, mirroring `createBaseten`.
    pub fn new(settings: BasetenProviderSettings) -> Self {
        let base_url = without_trailing_slash(Some(
            settings.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
        ))
        .unwrap_or(DEFAULT_BASE_URL)
        .to_string();

        Self {
            base_url,
            api_key: settings.api_key,
            model_url: settings.model_url,
            headers: settings.headers,
        }
    }

    /// Resolves request headers, mirroring upstream `getHeaders`.
    ///
    /// Loads the Baseten API key as a `Bearer` `Authorization` header, merges
    /// custom headers, and appends the `ai-sdk/baseten/{VERSION}` user-agent
    /// suffix. Returns the same `LoadApiKeyError` upstream `loadApiKey` raises.
    pub fn headers(&self) -> Result<Headers, LoadApiKeyError> {
        let api_key = self.load_api_key()?;

        let mut base: Vec<(String, Option<String>)> = Vec::new();
        base.push((
            "Authorization".to_string(),
            Some(format!("Bearer {api_key}")),
        ));
        for (name, value) in &self.headers {
            base.push((name.clone(), Some(value.clone())));
        }

        Ok(with_user_agent_suffix(
            Some(base),
            [format!("ai-sdk/baseten/{BASETEN_VERSION}")],
        ))
    }

    fn load_api_key(&self) -> Result<String, LoadApiKeyError> {
        let mut options = LoadApiKeyOptions::new("BASETEN_API_KEY", "Baseten API key");
        options.api_key = self.api_key.clone();
        load_api_key(options)
    }

    /// Builds the chat model config, mirroring upstream `createChatModel`.
    ///
    /// Returns an error string for `/predict` endpoints (chat requires
    /// `/sync/v1`). The returned config carries the model id the OpenAI-compatible
    /// chat model would be constructed with (`placeholder` for custom `/sync/v1`
    /// URLs, `chat` for the default Model APIs, or the supplied model id).
    pub fn chat_model(&self, model_id: Option<&str>) -> Result<BasetenModelConfig, String> {
        if let Some(custom_url) = &self.model_url {
            if custom_url.contains("/sync/v1") {
                return Ok(self.common_config(
                    model_id.unwrap_or("placeholder"),
                    "chat",
                    Some(custom_url.clone()),
                    false,
                ));
            } else if custom_url.contains("/predict") {
                return Err(
                    "Not supported. You must use a /sync/v1 endpoint for chat models.".to_string(),
                );
            }
        }

        Ok(self.common_config(model_id.unwrap_or("chat"), "chat", None, false))
    }

    /// Alias for [`Self::chat_model`], mirroring upstream `languageModel`.
    pub fn language_model(&self, model_id: Option<&str>) -> Result<BasetenModelConfig, String> {
        self.chat_model(model_id)
    }

    /// Convenience for calling the provider as a function, mirroring
    /// `provider(modelId)` which delegates to `createChatModel`.
    pub fn call(&self, model_id: Option<&str>) -> Result<BasetenModelConfig, String> {
        self.chat_model(model_id)
    }

    /// Builds the embedding model config, mirroring upstream
    /// `createEmbeddingModel`.
    ///
    /// Requires a `modelURL`; only `/sync` (and `/sync/v1`) endpoints are
    /// supported. Returns an error string for the missing-URL and `/predict`
    /// cases, matching the upstream thrown messages.
    pub fn embedding_model(&self, model_id: Option<&str>) -> Result<BasetenModelConfig, String> {
        let Some(custom_url) = &self.model_url else {
            return Err(
                "No model URL provided for embeddings. Please set modelURL option for embeddings."
                    .to_string(),
            );
        };

        if custom_url.contains("/sync") {
            Ok(self.common_config(
                model_id.unwrap_or("embeddings"),
                "embedding",
                Some(custom_url.clone()),
                true,
            ))
        } else {
            Err(
                "Not supported. You must use a /sync or /sync/v1 endpoint for embeddings."
                    .to_string(),
            )
        }
    }

    /// Alias for [`Self::embedding_model`], mirroring `textEmbeddingModel`.
    pub fn text_embedding_model(
        &self,
        model_id: Option<&str>,
    ) -> Result<BasetenModelConfig, String> {
        self.embedding_model(model_id)
    }

    /// The URL passed to the Performance Client for embeddings: `/sync/v1` is
    /// rewritten to `/sync` (the Performance Client appends `/v1` itself).
    /// Mirrors upstream `customURL.replace('/sync/v1', '/sync')`.
    pub fn performance_client_url(&self) -> Option<String> {
        self.model_url
            .as_ref()
            .map(|url| url.replacen("/sync/v1", "/sync", 1))
    }

    /// Image models are unsupported, mirroring upstream `provider.imageModel`
    /// which always throws `NoSuchModelError`.
    pub fn image_model(&self, model_id: &str) -> NoSuchModelError {
        NoSuchModelError::new(model_id, ModelType::ImageModel)
    }

    fn common_config(
        &self,
        model_id: &str,
        model_type: &str,
        custom_url: Option<String>,
        is_embedding: bool,
    ) -> BasetenModelConfig {
        BasetenModelConfig {
            model_id: model_id.to_string(),
            provider: format!("baseten.{model_type}"),
            custom_url,
            is_embedding,
            base_url: self.base_url.clone(),
        }
    }
}

/// Creates a Baseten provider, mirroring upstream `createBaseten(options)`.
pub fn create_baseten(settings: BasetenProviderSettings) -> BasetenProvider {
    BasetenProvider::new(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(settings: BasetenProviderSettings) -> BasetenProvider {
        create_baseten(settings)
    }

    /// Default settings carrying an explicit API key.
    ///
    /// Upstream mocks `loadApiKey` to return `'mock-api-key'`; in Rust the
    /// deterministic equivalent is supplying the key explicitly, which exercises
    /// the same `headers()` Bearer/merge/user-agent behavior without touching
    /// process-global environment state (the crate forbids `unsafe`, which the
    /// `std::env::set_var` seam requires). The env-var resolution path itself is
    /// covered by `load_api_key` tests in `ai-sdk-provider-utils`.
    fn default_settings() -> BasetenProviderSettings {
        BasetenProviderSettings {
            api_key: Some("mock-api-key".to_string()),
            ..Default::default()
        }
    }

    // packages-baseten-0001
    #[test]
    fn baseten_0001_create_provider_with_default_options() {
        let p = provider(default_settings());
        let config = p.chat_model(Some("deepseek-ai/DeepSeek-V3-0324")).unwrap();
        assert_eq!(config.provider, "baseten.chat");
        let headers = p.headers().unwrap();
        assert_eq!(
            headers.get("authorization").map(String::as_str),
            Some("Bearer mock-api-key")
        );
    }

    // packages-baseten-0002
    #[test]
    fn baseten_0002_create_provider_with_custom_options() {
        let p = provider(BasetenProviderSettings {
            api_key: Some("custom-key".to_string()),
            base_url: Some("https://custom.url".to_string()),
            headers: vec![("Custom-Header".to_string(), "value".to_string())],
            ..Default::default()
        });
        let _ = p.chat_model(Some("deepseek-ai/DeepSeek-V3-0324")).unwrap();
        let headers = p.headers().unwrap();
        // Explicit api key wins; no environment read required.
        assert_eq!(
            headers.get("authorization").map(String::as_str),
            Some("Bearer custom-key")
        );
        // Custom headers are lowercased like upstream `normalizeHeaders`.
        assert_eq!(
            headers.get("custom-header").map(String::as_str),
            Some("value")
        );
    }

    // packages-baseten-0003
    #[test]
    fn baseten_0003_call_as_function_supports_optional_model_id() {
        let p = provider(default_settings());
        // Without model id -> default `chat`.
        let m1 = p.call(None).unwrap();
        assert_eq!(m1.model_id, "chat");
        assert_eq!(m1.provider, "baseten.chat");
        // With model id.
        let m2 = p.call(Some("deepseek-ai/DeepSeek-V3-0324")).unwrap();
        assert_eq!(m2.model_id, "deepseek-ai/DeepSeek-V3-0324");
    }

    // packages-baseten-0004
    #[test]
    fn baseten_0004_chat_model_default_model_apis_configuration() {
        let p = provider(default_settings());
        let model_id = "deepseek-ai/DeepSeek-V3-0324";
        let config = p.chat_model(Some(model_id)).unwrap();
        assert_eq!(config.model_id, model_id);
        assert_eq!(config.provider, "baseten.chat");
    }

    // packages-baseten-0005
    #[test]
    fn baseten_0005_chat_model_optional_model_id() {
        let p = provider(default_settings());
        // Without model id -> `chat`.
        let m1 = p.chat_model(None).unwrap();
        assert_eq!(m1.model_id, "chat");
        // With model id.
        let m2 = p.chat_model(Some("deepseek-ai/DeepSeek-V3-0324")).unwrap();
        assert_eq!(m2.model_id, "deepseek-ai/DeepSeek-V3-0324");
    }

    // packages-baseten-0006
    #[test]
    fn baseten_0006_sync_v1_endpoints_construct_and_build_url() {
        let p = provider(BasetenProviderSettings {
            model_url: Some(
                "https://model-123.api.baseten.co/environments/production/sync/v1".to_string(),
            ),
            ..default_settings()
        });
        let config = p.chat_model(None).unwrap();
        assert_eq!(config.model_id, "placeholder");
        assert_eq!(config.provider, "baseten.chat");
        assert_eq!(
            config.url("/chat/completions"),
            "https://model-123.api.baseten.co/environments/production/sync/v1/chat/completions"
        );
    }

    // packages-baseten-0007
    #[test]
    fn baseten_0007_predict_endpoints_throw_for_chat_models() {
        let p = provider(BasetenProviderSettings {
            model_url: Some(
                "https://model-123.api.baseten.co/environments/production/predict".to_string(),
            ),
            ..default_settings()
        });
        let err = p.chat_model(None).unwrap_err();
        assert_eq!(
            err,
            "Not supported. You must use a /sync/v1 endpoint for chat models."
        );
    }

    // packages-baseten-0008
    #[test]
    fn baseten_0008_language_model_is_alias_for_chat_model() {
        let p = provider(default_settings());
        let model_id = "deepseek-ai/DeepSeek-V3-0324";
        let chat = p.chat_model(Some(model_id)).unwrap();
        let language = p.language_model(Some(model_id)).unwrap();
        assert_eq!(chat, language);
        assert_eq!(language.provider, "baseten.chat");
    }

    // packages-baseten-0009
    #[test]
    fn baseten_0009_language_model_optional_model_id() {
        let p = provider(default_settings());
        let m1 = p.language_model(None).unwrap();
        assert_eq!(m1.model_id, "chat");
        let m2 = p
            .language_model(Some("deepseek-ai/DeepSeek-V3-0324"))
            .unwrap();
        assert_eq!(m2.model_id, "deepseek-ai/DeepSeek-V3-0324");
    }

    // packages-baseten-0010
    #[test]
    fn baseten_0010_embedding_throws_when_no_model_url() {
        let p = provider(default_settings());
        let err = p.embedding_model(None).unwrap_err();
        assert_eq!(
            err,
            "No model URL provided for embeddings. Please set modelURL option for embeddings."
        );
    }

    // packages-baseten-0011
    #[test]
    fn baseten_0011_embedding_sync_endpoint_builds_url_with_v1() {
        let p = provider(BasetenProviderSettings {
            model_url: Some(
                "https://model-123.api.baseten.co/environments/production/sync".to_string(),
            ),
            ..default_settings()
        });
        let config = p.embedding_model(None).unwrap();
        assert_eq!(config.model_id, "embeddings");
        assert_eq!(config.provider, "baseten.embedding");
        // Performance Client adds /v1, so /sync (not /sync/v1) gets /v1 inserted.
        assert_eq!(
            config.url("/embeddings"),
            "https://model-123.api.baseten.co/environments/production/sync/v1/embeddings"
        );
    }

    // packages-baseten-0012
    #[test]
    fn baseten_0012_embedding_predict_endpoint_throws() {
        let p = provider(BasetenProviderSettings {
            model_url: Some(
                "https://model-123.api.baseten.co/environments/production/predict".to_string(),
            ),
            ..default_settings()
        });
        let err = p.embedding_model(None).unwrap_err();
        assert_eq!(
            err,
            "Not supported. You must use a /sync or /sync/v1 endpoint for embeddings."
        );
    }

    // packages-baseten-0013
    #[test]
    fn baseten_0013_embedding_sync_v1_strips_v1_for_performance_client() {
        let p = provider(BasetenProviderSettings {
            model_url: Some(
                "https://model-123.api.baseten.co/environments/production/sync/v1".to_string(),
            ),
            ..default_settings()
        });
        let config = p.embedding_model(None).unwrap();
        assert_eq!(config.model_id, "embeddings");
        // The Performance Client URL strips the trailing /v1.
        assert_eq!(
            p.performance_client_url().as_deref(),
            Some("https://model-123.api.baseten.co/environments/production/sync")
        );
    }

    // packages-baseten-0014
    #[test]
    fn baseten_0014_embedding_custom_model_id() {
        let p = provider(BasetenProviderSettings {
            model_url: Some(
                "https://model-123.api.baseten.co/environments/production/sync".to_string(),
            ),
            ..default_settings()
        });
        // Default constructs with the `embeddings` model id.
        let config = p.embedding_model(None).unwrap();
        assert_eq!(config.model_id, "embeddings");
        assert_eq!(config.provider, "baseten.embedding");
        // A custom model id is honored.
        let custom = p.embedding_model(Some("my-embeddings")).unwrap();
        assert_eq!(custom.model_id, "my-embeddings");
    }

    // packages-baseten-0015
    #[test]
    fn baseten_0015_image_model_throws_no_such_model() {
        let p = provider(default_settings());
        let err = p.image_model("test-model");
        assert_eq!(err.model_id(), "test-model");
        assert_eq!(err.model_type(), ModelType::ImageModel);
    }

    // packages-baseten-0016
    #[test]
    fn baseten_0016_default_base_url_when_no_model_url() {
        let p = provider(default_settings());
        let config = p.chat_model(Some("test-model")).unwrap();
        assert_eq!(
            config.url("/chat/completions"),
            "https://inference.baseten.co/v1/chat/completions"
        );
    }

    // packages-baseten-0017
    #[test]
    fn baseten_0017_custom_base_url() {
        let p = provider(BasetenProviderSettings {
            base_url: Some("https://custom.baseten.co/v1".to_string()),
            ..default_settings()
        });
        let config = p.chat_model(Some("test-model")).unwrap();
        assert_eq!(
            config.url("/chat/completions"),
            "https://custom.baseten.co/v1/chat/completions"
        );
    }

    // packages-baseten-0018
    #[test]
    fn baseten_0018_model_url_for_custom_endpoints() {
        let p = provider(BasetenProviderSettings {
            model_url: Some(
                "https://model-123.api.baseten.co/environments/production/sync/v1".to_string(),
            ),
            ..default_settings()
        });
        let config = p.chat_model(None).unwrap();
        assert_eq!(
            config.url("/chat/completions"),
            "https://model-123.api.baseten.co/environments/production/sync/v1/chat/completions"
        );
    }

    // packages-baseten-0019
    #[test]
    fn baseten_0019_authorization_header_with_api_key() {
        let p = provider(default_settings());
        let _ = p.chat_model(Some("test-model")).unwrap();
        let headers = p.headers().unwrap();
        assert_eq!(
            headers.get("authorization").map(String::as_str),
            Some("Bearer mock-api-key")
        );
    }

    // packages-baseten-0020
    #[test]
    fn baseten_0020_custom_headers_included() {
        let p = provider(BasetenProviderSettings {
            headers: vec![("Custom-Header".to_string(), "custom-value".to_string())],
            ..default_settings()
        });
        let _ = p.chat_model(Some("test-model")).unwrap();
        let headers = p.headers().unwrap();
        assert_eq!(
            headers.get("authorization").map(String::as_str),
            Some("Bearer mock-api-key")
        );
        assert_eq!(
            headers.get("custom-header").map(String::as_str),
            Some("custom-value")
        );
    }

    // packages-baseten-0021
    #[test]
    fn baseten_0021_user_agent_with_version() {
        let p = provider(default_settings());
        let _ = p.chat_model(Some("test-model")).unwrap();
        let headers = p.headers().unwrap();
        let ua = headers
            .get("user-agent")
            .expect("user-agent header present");
        assert!(
            ua.contains(&format!("ai-sdk/baseten/{BASETEN_VERSION}")),
            "user-agent `{ua}` should contain the baseten suffix"
        );
    }

    // packages-baseten-0022
    #[test]
    fn baseten_0022_missing_model_url_for_embeddings() {
        let p = provider(default_settings());
        let err = p.embedding_model(None).unwrap_err();
        assert_eq!(
            err,
            "No model URL provided for embeddings. Please set modelURL option for embeddings."
        );
    }

    // packages-baseten-0023
    #[test]
    fn baseten_0023_unsupported_image_models() {
        let p = provider(default_settings());
        let err = p.image_model("unsupported-model");
        assert_eq!(err.model_id(), "unsupported-model");
        assert_eq!(err.model_type(), ModelType::ImageModel);
    }

    // packages-baseten-0024
    #[test]
    fn baseten_0024_implements_all_required_provider_methods() {
        let p = provider(default_settings());
        // call-as-function + chat + language all resolve a chat config.
        assert_eq!(p.call(None).unwrap().provider, "baseten.chat");
        assert_eq!(p.chat_model(None).unwrap().provider, "baseten.chat");
        assert_eq!(p.language_model(None).unwrap().provider, "baseten.chat");
        // embedding requires a model URL -> error without one.
        assert!(p.embedding_model(None).is_err());
        assert!(p.text_embedding_model(None).is_err());
        // image model always yields NoSuchModelError.
        assert_eq!(p.image_model("x").model_type(), ModelType::ImageModel);
    }

    // packages-baseten-0025
    #[test]
    fn baseten_0025_provider_callable_as_function() {
        let p = provider(default_settings());
        let m1 = p.call(None).unwrap();
        assert_eq!(m1.provider, "baseten.chat");
        assert_eq!(m1.model_id, "chat");
        let m2 = p.call(Some("test-model")).unwrap();
        assert_eq!(m2.model_id, "test-model");
    }
}
