use std::fmt;

use crate::image_model::ImageModel;
use crate::image_model_middleware::ImageModelMiddleware;
use crate::language_model::LanguageModel;
use crate::language_model_middleware::LanguageModelMiddleware;
use crate::provider::{
    ModelType, NoSuchModelError, Provider, ProviderWithFiles, ProviderWithRerankingModel,
    ProviderWithSkills, ProviderWithSpeechModel, ProviderWithTranscriptionModel,
    ProviderWithVideoModel,
};
use crate::provider_middleware::{
    WrappedProvider, WrappedProviderWithImageModelMiddleware, wrap_provider,
    wrap_provider_with_image_model_middleware,
};

/// Configuration for a provider registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderRegistryOptions {
    separator: String,
}

impl ProviderRegistryOptions {
    /// Creates registry options with the upstream default separator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the separator used between provider id and model id.
    pub fn with_separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Returns the separator used between provider id and model id.
    pub fn separator(&self) -> &str {
        &self.separator
    }
}

impl Default for ProviderRegistryOptions {
    fn default() -> Self {
        Self {
            separator: ":".to_string(),
        }
    }
}

/// Error returned when a provider registry cannot resolve a model lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderRegistryError {
    /// The registry id did not include a valid model id for the requested type,
    /// or the provider could not resolve that model.
    NoSuchModel(NoSuchModelError),

    /// The provider id extracted from the registry id was not registered.
    NoSuchProvider(NoSuchProviderError),
}

impl ProviderRegistryError {
    /// Returns the inner missing-model error when this error represents one.
    pub fn as_no_such_model(&self) -> Option<&NoSuchModelError> {
        match self {
            Self::NoSuchModel(error) => Some(error),
            Self::NoSuchProvider(_) => None,
        }
    }

    /// Returns the inner missing-provider error when this error represents one.
    pub fn as_no_such_provider(&self) -> Option<&NoSuchProviderError> {
        match self {
            Self::NoSuchModel(_) => None,
            Self::NoSuchProvider(error) => Some(error),
        }
    }
}

impl fmt::Display for ProviderRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchModel(error) => error.fmt(formatter),
            Self::NoSuchProvider(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ProviderRegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoSuchModel(error) => Some(error),
            Self::NoSuchProvider(error) => Some(error),
        }
    }
}

impl From<NoSuchModelError> for ProviderRegistryError {
    fn from(error: NoSuchModelError) -> Self {
        Self::NoSuchModel(error)
    }
}

impl From<NoSuchProviderError> for ProviderRegistryError {
    fn from(error: NoSuchProviderError) -> Self {
        Self::NoSuchProvider(error)
    }
}

/// A Rust-native provider registry for provider-v4 model lookups.
///
/// Upstream `createProviderRegistry` accepts a record of providers and resolves
/// combined ids like `providerId:modelId`. This type mirrors that behavior for
/// Rust provider implementations that share the same concrete provider type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderRegistry<P> {
    providers: Vec<(String, P)>,
    options: ProviderRegistryOptions,
}

/// Rust equivalent of upstream `customProvider` for direct v4 model maps.
///
/// This initial Rust surface covers the required provider-v4 model families
/// (`languageModel`, `embeddingModel`, and `imageModel`). Broader optional
/// model families, files/skills, string aliases, and fallback providers are
/// tracked separately in the upstream parity ledger.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomProvider<LM, EM, IM> {
    language_models: Vec<(String, LM)>,
    embedding_models: Vec<(String, EM)>,
    image_models: Vec<(String, IM)>,
}

impl<LM, EM, IM> CustomProvider<LM, EM, IM> {
    /// Creates an empty custom provider.
    pub fn new() -> Self {
        Self {
            language_models: Vec::new(),
            embedding_models: Vec::new(),
            image_models: Vec::new(),
        }
    }

    /// Registers a language model by model id.
    pub fn with_language_model(mut self, model_id: impl Into<String>, model: LM) -> Self {
        self.language_models.push((model_id.into(), model));
        self
    }

    /// Registers an embedding model by model id.
    pub fn with_embedding_model(mut self, model_id: impl Into<String>, model: EM) -> Self {
        self.embedding_models.push((model_id.into(), model));
        self
    }

    /// Registers an image model by model id.
    pub fn with_image_model(mut self, model_id: impl Into<String>, model: IM) -> Self {
        self.image_models.push((model_id.into(), model));
        self
    }
}

impl<LM, EM, IM> Default for CustomProvider<LM, EM, IM> {
    fn default() -> Self {
        Self::new()
    }
}

impl<LM, EM, IM> Provider for CustomProvider<LM, EM, IM>
where
    LM: LanguageModel + Clone,
    EM: crate::embedding_model::EmbeddingModel + Clone,
    IM: ImageModel + Clone,
{
    type LanguageModel = LM;
    type EmbeddingModel = EM;
    type ImageModel = IM;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        self.language_models
            .iter()
            .find_map(|(id, model)| (id == model_id).then(|| model.clone()))
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::LanguageModel))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        self.embedding_models
            .iter()
            .find_map(|(id, model)| (id == model_id).then(|| model.clone()))
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        self.image_models
            .iter()
            .find_map(|(id, model)| (id == model_id).then(|| model.clone()))
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::ImageModel))
    }
}

impl<P> ProviderRegistry<P> {
    /// Creates a registry with the upstream default separator (`:`).
    pub fn new<I, K>(providers: I) -> Self
    where
        I: IntoIterator<Item = (K, P)>,
        K: Into<String>,
    {
        Self::with_options(providers, ProviderRegistryOptions::default())
    }

    /// Creates a registry with explicit options.
    pub fn with_options<I, K>(providers: I, options: ProviderRegistryOptions) -> Self
    where
        I: IntoIterator<Item = (K, P)>,
        K: Into<String>,
    {
        Self {
            providers: providers
                .into_iter()
                .map(|(id, provider)| (id.into(), provider))
                .collect(),
            options,
        }
    }

    /// Returns the registry options.
    pub fn options(&self) -> &ProviderRegistryOptions {
        &self.options
    }

    /// Returns registered provider ids in insertion order.
    pub fn provider_ids(&self) -> impl Iterator<Item = &str> {
        self.providers.iter().map(|(id, _)| id.as_str())
    }

    fn split_id<'a>(
        &self,
        id: &'a str,
        model_type: ModelType,
    ) -> Result<(&'a str, &'a str), NoSuchModelError> {
        split_registry_model_id(id, model_type, self.options.separator())
    }

    fn get_provider(
        &self,
        provider_id: &str,
        model_type: ModelType,
    ) -> Result<&P, NoSuchProviderError> {
        self.providers
            .iter()
            .find_map(|(id, provider)| (id == provider_id).then_some(provider))
            .ok_or_else(|| {
                NoSuchProviderError::new(provider_id, model_type, provider_id, self.provider_ids())
            })
    }
}

impl<P: Provider> ProviderRegistry<P> {
    /// Returns the language model for a registry id shaped as `providerId:modelId`.
    pub fn language_model(&self, id: &str) -> Result<P::LanguageModel, ProviderRegistryError> {
        let (provider_id, model_id) = self.split_id(id, ModelType::LanguageModel)?;
        let provider = self.get_provider(provider_id, ModelType::LanguageModel)?;

        provider.language_model(model_id).map_err(Into::into)
    }

    /// Returns the embedding model for a registry id shaped as `providerId:modelId`.
    pub fn embedding_model(&self, id: &str) -> Result<P::EmbeddingModel, ProviderRegistryError> {
        let (provider_id, model_id) = self.split_id(id, ModelType::EmbeddingModel)?;
        let provider = self.get_provider(provider_id, ModelType::EmbeddingModel)?;

        provider.embedding_model(model_id).map_err(Into::into)
    }

    /// Returns the image model for a registry id shaped as `providerId:modelId`.
    pub fn image_model(&self, id: &str) -> Result<P::ImageModel, ProviderRegistryError> {
        let (provider_id, model_id) = self.split_id(id, ModelType::ImageModel)?;
        let provider = self.get_provider(provider_id, ModelType::ImageModel)?;

        provider.image_model(model_id).map_err(Into::into)
    }
}

impl<P: ProviderWithTranscriptionModel> ProviderRegistry<P> {
    /// Returns the transcription model for a registry id shaped as `providerId:modelId`.
    pub fn transcription_model(
        &self,
        id: &str,
    ) -> Result<P::TranscriptionModel, ProviderRegistryError> {
        let (provider_id, model_id) = self.split_id(id, ModelType::TranscriptionModel)?;
        let provider = self.get_provider(provider_id, ModelType::TranscriptionModel)?;

        provider.transcription_model(model_id).map_err(Into::into)
    }
}

impl<P: ProviderWithSpeechModel> ProviderRegistry<P> {
    /// Returns the speech model for a registry id shaped as `providerId:modelId`.
    pub fn speech_model(&self, id: &str) -> Result<P::SpeechModel, ProviderRegistryError> {
        let (provider_id, model_id) = self.split_id(id, ModelType::SpeechModel)?;
        let provider = self.get_provider(provider_id, ModelType::SpeechModel)?;

        provider.speech_model(model_id).map_err(Into::into)
    }
}

impl<P: ProviderWithRerankingModel> ProviderRegistry<P> {
    /// Returns the reranking model for a registry id shaped as `providerId:modelId`.
    pub fn reranking_model(&self, id: &str) -> Result<P::RerankingModel, ProviderRegistryError> {
        let (provider_id, model_id) = self.split_id(id, ModelType::RerankingModel)?;
        let provider = self.get_provider(provider_id, ModelType::RerankingModel)?;

        provider.reranking_model(model_id).map_err(Into::into)
    }
}

impl<P: ProviderWithVideoModel> ProviderRegistry<P> {
    /// Returns the video model for a registry id shaped as `providerId:modelId`.
    pub fn video_model(&self, id: &str) -> Result<P::VideoModel, ProviderRegistryError> {
        let (provider_id, model_id) = self.split_id(id, ModelType::VideoModel)?;
        let provider = self.get_provider(provider_id, ModelType::VideoModel)?;

        provider.video_model(model_id).map_err(Into::into)
    }
}

impl<P: ProviderWithFiles> ProviderRegistry<P> {
    /// Returns the files interface for a registered provider id.
    pub fn files(&self, id: &str) -> Result<P::Files, ProviderRegistryError> {
        let provider = self.get_provider(id, ModelType::LanguageModel)?;

        Ok(provider.files())
    }
}

impl<P: ProviderWithSkills> ProviderRegistry<P> {
    /// Returns the skills interface for a registered provider id.
    pub fn skills(&self, id: &str) -> Result<P::Skills, ProviderRegistryError> {
        let provider = self.get_provider(id, ModelType::LanguageModel)?;

        Ok(provider.skills())
    }
}

/// Creates a provider registry with the upstream default separator (`:`).
pub fn create_provider_registry<I, K, P>(providers: I) -> ProviderRegistry<P>
where
    I: IntoIterator<Item = (K, P)>,
    K: Into<String>,
{
    ProviderRegistry::new(providers)
}

/// Creates a provider registry with explicit options.
pub fn create_provider_registry_with_options<I, K, P>(
    providers: I,
    options: ProviderRegistryOptions,
) -> ProviderRegistry<P>
where
    I: IntoIterator<Item = (K, P)>,
    K: Into<String>,
{
    ProviderRegistry::with_options(providers, options)
}

/// Creates an empty custom provider with direct v4 model maps.
pub fn custom_provider<LM, EM, IM>() -> CustomProvider<LM, EM, IM> {
    CustomProvider::new()
}

/// Creates a provider registry that wraps every language model lookup with middleware.
///
/// This mirrors upstream `createProviderRegistry(..., { languageModelMiddleware })`
/// while keeping the Rust return type explicit.
pub fn create_provider_registry_with_language_model_middleware<I, K, P, LW>(
    providers: I,
    language_model_middleware: LW,
    options: ProviderRegistryOptions,
) -> ProviderRegistry<WrappedProvider<P, LW>>
where
    I: IntoIterator<Item = (K, P)>,
    K: Into<String>,
    P: Provider,
    P::LanguageModel: LanguageModel + Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
{
    ProviderRegistry::with_options(
        providers.into_iter().map(|(id, provider)| {
            (
                id,
                wrap_provider(provider, language_model_middleware.clone()),
            )
        }),
        options,
    )
}

/// Creates a provider registry that wraps every image model lookup with middleware.
///
/// This mirrors upstream `createProviderRegistry(..., { imageModelMiddleware })`
/// while keeping the Rust return type explicit.
pub fn create_provider_registry_with_image_model_middleware<I, K, P, IW>(
    providers: I,
    image_model_middleware: IW,
    options: ProviderRegistryOptions,
) -> ProviderRegistry<WrappedProviderWithImageModelMiddleware<P, IW>>
where
    I: IntoIterator<Item = (K, P)>,
    K: Into<String>,
    P: Provider,
    P::ImageModel: ImageModel + Sync,
    IW: ImageModelMiddleware<P::ImageModel> + Clone + Sync,
{
    ProviderRegistry::with_options(
        providers.into_iter().map(|(id, provider)| {
            (
                id,
                wrap_provider_with_image_model_middleware(provider, image_model_middleware.clone()),
            )
        }),
        options,
    )
}

/// Splits a registry model id into its provider id and provider-specific model id.
pub fn split_registry_model_id<'a>(
    id: &'a str,
    model_type: ModelType,
    separator: &str,
) -> Result<(&'a str, &'a str), NoSuchModelError> {
    id.find(separator)
        .map(|index| {
            let model_id_start = index + separator.len();
            (&id[..index], &id[model_id_start..])
        })
        .ok_or_else(|| {
            NoSuchModelError::with_message(
                id,
                model_type,
                format!(
                    "Invalid {model_type} id for registry: {id} (must be in the format \"providerId{separator}modelId\")"
                ),
            )
        })
}

/// Error returned when a provider registry cannot resolve a provider id.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoSuchProviderError {
    model_id: String,
    model_type: ModelType,
    provider_id: String,
    available_providers: Vec<String>,
    message: String,
}

impl NoSuchProviderError {
    /// Creates a missing-provider error with the upstream default message.
    pub fn new(
        model_id: impl Into<String>,
        model_type: ModelType,
        provider_id: impl Into<String>,
        available_providers: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let provider_id = provider_id.into();
        let available_providers = available_providers
            .into_iter()
            .map(Into::into)
            .collect::<Vec<_>>();
        let message = no_such_provider_default_message(&provider_id, &available_providers);

        Self {
            model_id: model_id.into(),
            model_type,
            provider_id,
            available_providers,
            message,
        }
    }

    /// Creates a missing-provider error with a caller-supplied message.
    pub fn with_message(
        model_id: impl Into<String>,
        model_type: ModelType,
        provider_id: impl Into<String>,
        available_providers: impl IntoIterator<Item = impl Into<String>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            model_type,
            provider_id: provider_id.into(),
            available_providers: available_providers.into_iter().map(Into::into).collect(),
            message: message.into(),
        }
    }

    /// Returns the full registry lookup id that failed.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the model category requested from the registry.
    pub fn model_type(&self) -> ModelType {
        self.model_type
    }

    /// Returns the provider id extracted from the failed lookup.
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    /// Returns the provider ids registered in the registry at lookup time.
    pub fn available_providers(&self) -> &[String] {
        &self.available_providers
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its retained lookup context and message.
    pub fn into_parts(self) -> (String, ModelType, String, Vec<String>, String) {
        (
            self.model_id,
            self.model_type,
            self.provider_id,
            self.available_providers,
            self.message,
        )
    }
}

impl fmt::Display for NoSuchProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for NoSuchProviderError {}

fn no_such_provider_default_message(provider_id: &str, available_providers: &[String]) -> String {
    format!(
        "No such provider: {} (available providers: {})",
        provider_id,
        available_providers.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::{
        NoSuchProviderError, ProviderRegistryOptions, create_provider_registry,
        create_provider_registry_with_image_model_middleware,
        create_provider_registry_with_language_model_middleware,
        create_provider_registry_with_options, custom_provider, split_registry_model_id,
    };
    use crate::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResult};
    use crate::file_data::{FileDataContent, ProviderReference};
    use crate::files::{Files, FilesUploadFileCallOptions, FilesUploadFileResult};
    use crate::image_model::{
        ImageModel, ImageModelCallOptions, ImageModelResponse, ImageModelResult,
    };
    use crate::image_model_middleware::{ImageModelMiddleware, ImageModelMiddlewareModelOptions};
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelStreamPart,
        LanguageModelStreamResult, LanguageModelSupportedUrls, LanguageModelText,
        LanguageModelUsage,
    };
    use crate::language_model_middleware::{
        LanguageModelMiddleware, LanguageModelMiddlewareModelOptions,
    };
    use crate::provider::{
        ModelType, NoSuchModelError, Provider, ProviderWithFiles, ProviderWithRerankingModel,
        ProviderWithSkills, ProviderWithSpeechModel, ProviderWithTranscriptionModel,
        ProviderWithVideoModel, SpecificationVersion,
    };
    use crate::reranking_model::{
        RerankingModel, RerankingModelCallOptions, RerankingModelRanking, RerankingModelResult,
    };
    use crate::skills::{Skills, SkillsUploadSkillCallOptions, SkillsUploadSkillResult};
    use crate::speech_model::{
        SpeechModel, SpeechModelCallOptions, SpeechModelResponse, SpeechModelResult,
    };
    use crate::transcription_model::{
        TranscriptionModel, TranscriptionModelCallOptions, TranscriptionModelResponse,
        TranscriptionModelResult,
    };
    use crate::video_model::{
        VideoModel, VideoModelCallOptions, VideoModelResponse, VideoModelResult,
        VideoModelVideoData,
    };
    use std::collections::BTreeMap;
    use std::future::{Ready, ready};
    use time::OffsetDateTime;

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct StaticLanguageModel {
        provider: String,
        model_id: String,
    }

    impl LanguageModel for StaticLanguageModel {
        type SupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a;
        type GenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a;
        type Stream = Vec<LanguageModelStreamPart>;
        type StreamFuture<'a>
            = Ready<LanguageModelStreamResult<Self::Stream>>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            &self.provider
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(LanguageModelSupportedUrls::new())
        }

        fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(LanguageModelGenerateResult::new(
                vec![LanguageModelContent::Text(LanguageModelText::new("ok"))],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ))
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(Vec::new()))
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct StaticEmbeddingModel {
        provider: String,
        model_id: String,
    }

    impl EmbeddingModel for StaticEmbeddingModel {
        type MaxEmbeddingsPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a;
        type SupportsParallelCallsFuture<'a>
            = Ready<bool>
        where
            Self: 'a;
        type EmbedFuture<'a>
            = Ready<EmbeddingModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            &self.provider
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_> {
            ready(Some(16))
        }

        fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
            ready(true)
        }

        fn do_embed(&self, _options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
            ready(EmbeddingModelResult::new(vec![vec![1.0, 2.0]]))
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct StaticImageModel {
        provider: String,
        model_id: String,
    }

    impl ImageModel for StaticImageModel {
        type MaxImagesPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a;
        type GenerateFuture<'a>
            = Ready<ImageModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            &self.provider
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
            ready(Some(4))
        }

        fn do_generate(&self, _options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(ImageModelResult::new(
                vec![FileDataContent::Bytes(Vec::new())],
                ImageModelResponse::new(OffsetDateTime::UNIX_EPOCH, self.model_id.clone()),
            ))
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct StaticTranscriptionModel {
        provider: String,
        model_id: String,
    }

    impl TranscriptionModel for StaticTranscriptionModel {
        type GenerateFuture<'a>
            = Ready<TranscriptionModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            &self.provider
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn do_generate(&self, _options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(TranscriptionModelResult::new(
                "hello world",
                Vec::new(),
                TranscriptionModelResponse::new(OffsetDateTime::UNIX_EPOCH, self.model_id.clone()),
            ))
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct StaticSpeechModel {
        provider: String,
        model_id: String,
    }

    impl SpeechModel for StaticSpeechModel {
        type GenerateFuture<'a>
            = Ready<SpeechModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            &self.provider
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn do_generate(&self, _options: SpeechModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(SpeechModelResult::new(
                FileDataContent::Base64("audio".to_string()),
                SpeechModelResponse::new(OffsetDateTime::UNIX_EPOCH, self.model_id.clone()),
            ))
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct StaticRerankingModel {
        provider: String,
        model_id: String,
    }

    impl RerankingModel for StaticRerankingModel {
        type RerankFuture<'a>
            = Ready<RerankingModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            &self.provider
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn do_rerank(&self, _options: RerankingModelCallOptions) -> Self::RerankFuture<'_> {
            ready(RerankingModelResult::new(vec![RerankingModelRanking::new(
                0, 1.0,
            )]))
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct StaticVideoModel {
        provider: String,
        model_id: String,
    }

    impl VideoModel for StaticVideoModel {
        type MaxVideosPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a;
        type GenerateFuture<'a>
            = Ready<VideoModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            &self.provider
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
            ready(Some(1))
        }

        fn do_generate(&self, _options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(VideoModelResult::new(
                vec![VideoModelVideoData::base64("AAAAIGZ0eXBtcDQy", "video/mp4")],
                VideoModelResponse::new(OffsetDateTime::UNIX_EPOCH, self.model_id.clone()),
            ))
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct StaticFiles {
        provider: String,
    }

    impl Files for StaticFiles {
        type UploadFileFuture<'a>
            = Ready<FilesUploadFileResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            &self.provider
        }

        fn upload_file(&self, _options: FilesUploadFileCallOptions) -> Self::UploadFileFuture<'_> {
            ready(FilesUploadFileResult::new(provider_reference(
                &self.provider,
                "file-123",
            )))
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct StaticSkills {
        provider: String,
    }

    impl Skills for StaticSkills {
        type UploadSkillFuture<'a>
            = Ready<SkillsUploadSkillResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            &self.provider
        }

        fn upload_skill(
            &self,
            _options: SkillsUploadSkillCallOptions,
        ) -> Self::UploadSkillFuture<'_> {
            ready(SkillsUploadSkillResult::new(provider_reference(
                &self.provider,
                "skill-123",
            )))
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct StaticProvider {
        id: &'static str,
    }

    impl Provider for StaticProvider {
        type LanguageModel = StaticLanguageModel;
        type EmbeddingModel = StaticEmbeddingModel;
        type ImageModel = StaticImageModel;

        fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
            lookup_model(model_id, ModelType::LanguageModel).map(|model_id| StaticLanguageModel {
                provider: self.id.to_string(),
                model_id,
            })
        }

        fn embedding_model(
            &self,
            model_id: &str,
        ) -> Result<Self::EmbeddingModel, NoSuchModelError> {
            lookup_model(model_id, ModelType::EmbeddingModel).map(|model_id| StaticEmbeddingModel {
                provider: self.id.to_string(),
                model_id,
            })
        }

        fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
            lookup_model(model_id, ModelType::ImageModel).map(|model_id| StaticImageModel {
                provider: self.id.to_string(),
                model_id,
            })
        }
    }

    impl ProviderWithTranscriptionModel for StaticProvider {
        type TranscriptionModel = StaticTranscriptionModel;

        fn transcription_model(
            &self,
            model_id: &str,
        ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
            lookup_model(model_id, ModelType::TranscriptionModel).map(|model_id| {
                StaticTranscriptionModel {
                    provider: self.id.to_string(),
                    model_id,
                }
            })
        }
    }

    impl ProviderWithSpeechModel for StaticProvider {
        type SpeechModel = StaticSpeechModel;

        fn speech_model(&self, model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError> {
            lookup_model(model_id, ModelType::SpeechModel).map(|model_id| StaticSpeechModel {
                provider: self.id.to_string(),
                model_id,
            })
        }
    }

    impl ProviderWithRerankingModel for StaticProvider {
        type RerankingModel = StaticRerankingModel;

        fn reranking_model(
            &self,
            model_id: &str,
        ) -> Result<Self::RerankingModel, NoSuchModelError> {
            lookup_model(model_id, ModelType::RerankingModel).map(|model_id| StaticRerankingModel {
                provider: self.id.to_string(),
                model_id,
            })
        }
    }

    impl ProviderWithVideoModel for StaticProvider {
        type VideoModel = StaticVideoModel;

        fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
            lookup_model(model_id, ModelType::VideoModel).map(|model_id| StaticVideoModel {
                provider: self.id.to_string(),
                model_id,
            })
        }
    }

    impl ProviderWithFiles for StaticProvider {
        type Files = StaticFiles;

        fn files(&self) -> Self::Files {
            StaticFiles {
                provider: self.id.to_string(),
            }
        }
    }

    impl ProviderWithSkills for StaticProvider {
        type Skills = StaticSkills;

        fn skills(&self) -> Self::Skills {
            StaticSkills {
                provider: self.id.to_string(),
            }
        }
    }

    fn lookup_model(model_id: &str, model_type: ModelType) -> Result<String, NoSuchModelError> {
        if model_id == "missing" {
            Err(NoSuchModelError::new(model_id, model_type))
        } else {
            Ok(model_id.to_string())
        }
    }

    fn provider_reference(provider: &str, id: &str) -> ProviderReference {
        ProviderReference::from_map(BTreeMap::from([(provider.to_string(), id.to_string())]))
            .expect("provider reference is valid")
    }

    #[test]
    fn custom_provider_language_model_should_return_the_language_model_if_it_exists() {
        let model = StaticLanguageModel {
            provider: "mock-provider".to_string(),
            model_id: "actual-language-model".to_string(),
        };
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_language_model("test-model", model.clone());

        assert_eq!(provider.language_model("test-model"), Ok(model));
    }

    #[test]
    fn custom_provider_language_model_should_throw_no_such_model_error_if_model_not_found_and_no_fallback()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>();
        let error = provider
            .language_model("test-model")
            .expect_err("missing model should error");

        assert_eq!(error.model_id(), "test-model");
        assert_eq!(error.model_type(), ModelType::LanguageModel);
    }

    #[test]
    fn custom_provider_embedding_model_should_return_the_embedding_model_if_it_exists() {
        let model = StaticEmbeddingModel {
            provider: "mock-provider".to_string(),
            model_id: "actual-embedding-model".to_string(),
        };
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_embedding_model("test-model", model.clone());

        assert_eq!(provider.embedding_model("test-model"), Ok(model));
    }

    #[test]
    fn custom_provider_embedding_model_should_throw_no_such_model_error_if_model_not_found_and_no_fallback()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>();
        let error = provider
            .embedding_model("test-model")
            .expect_err("missing model should error");

        assert_eq!(error.model_id(), "test-model");
        assert_eq!(error.model_type(), ModelType::EmbeddingModel);
    }

    #[test]
    fn custom_provider_image_model_should_return_the_image_model_if_it_exists() {
        let model = StaticImageModel {
            provider: "mock-provider".to_string(),
            model_id: "actual-image-model".to_string(),
        };
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_image_model("test-model", model.clone());

        assert_eq!(provider.image_model("test-model"), Ok(model));
    }

    #[test]
    fn custom_provider_image_model_should_throw_no_such_model_error_if_model_not_found_and_no_fallback()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>();
        let error = provider
            .image_model("test-model")
            .expect_err("missing model should error");

        assert_eq!(error.model_id(), "test-model");
        assert_eq!(error.model_type(), ModelType::ImageModel);
    }

    #[derive(Clone, Debug)]
    struct PrefixLanguageModelIdMiddleware;

    impl LanguageModelMiddleware<StaticLanguageModel> for PrefixLanguageModelIdMiddleware {
        type OverrideSupportedUrlsFuture<'a>
            = Ready<LanguageModelSupportedUrls>
        where
            Self: 'a,
            StaticLanguageModel: 'a;
        type TransformParamsFuture<'a>
            = Ready<LanguageModelCallOptions>
        where
            Self: 'a,
            StaticLanguageModel: 'a;
        type WrapGenerateFuture<'a>
            = Ready<LanguageModelGenerateResult>
        where
            Self: 'a,
            StaticLanguageModel: 'a;
        type WrapStreamFuture<'a>
            = Ready<LanguageModelStreamResult<Vec<LanguageModelStreamPart>>>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        fn override_model_id(
            &self,
            options: LanguageModelMiddlewareModelOptions<'_, StaticLanguageModel>,
        ) -> Option<String> {
            Some(format!("override-{}", options.model.model_id()))
        }
    }

    #[derive(Clone, Debug)]
    struct PrefixImageModelIdMiddleware;

    impl ImageModelMiddleware<StaticImageModel> for PrefixImageModelIdMiddleware {
        type OverrideMaxImagesPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a,
            StaticImageModel: 'a;
        type TransformParamsFuture<'a>
            = Ready<ImageModelCallOptions>
        where
            Self: 'a,
            StaticImageModel: 'a;
        type WrapGenerateFuture<'a>
            = Ready<ImageModelResult>
        where
            Self: 'a,
            StaticImageModel: 'a;

        fn override_model_id(
            &self,
            options: ImageModelMiddlewareModelOptions<'_, StaticImageModel>,
        ) -> Option<String> {
            Some(format!("override-{}", options.model.model_id()))
        }
    }

    #[test]
    fn registry_options_default_to_upstream_separator() {
        assert_eq!(ProviderRegistryOptions::new().separator(), ":");
        assert_eq!(
            ProviderRegistryOptions::new()
                .with_separator("::")
                .separator(),
            "::"
        );
    }

    #[test]
    fn split_registry_model_id_matches_upstream_first_separator_behavior() {
        assert_eq!(
            split_registry_model_id("provider:model:part2", ModelType::LanguageModel, ":")
                .expect("registry id splits"),
            ("provider", "model:part2")
        );
        assert_eq!(
            split_registry_model_id("provider::model", ModelType::EmbeddingModel, "::")
                .expect("custom registry id splits"),
            ("provider", "model")
        );
    }

    #[test]
    fn split_registry_model_id_reports_upstream_missing_separator_message() {
        let error =
            split_registry_model_id("model", ModelType::ImageModel, "::").expect_err("id fails");

        assert_eq!(error.model_id(), "model");
        assert_eq!(error.model_type(), ModelType::ImageModel);
        assert_eq!(
            error.message(),
            "Invalid imageModel id for registry: model (must be in the format \"providerId::modelId\")"
        );
    }

    #[test]
    fn create_provider_registry_resolves_required_model_interfaces() {
        let registry = create_provider_registry([("provider", StaticProvider { id: "provider" })]);

        let language_model = registry
            .language_model("provider:chat:part2")
            .expect("language model resolves");
        assert_eq!(
            language_model.specification_version(),
            SpecificationVersion::V4
        );
        assert_eq!(language_model.provider(), "provider");
        assert_eq!(language_model.model_id(), "chat:part2");

        let embedding_model = registry
            .embedding_model("provider:embed")
            .expect("embedding model resolves");
        assert_eq!(embedding_model.provider(), "provider");
        assert_eq!(embedding_model.model_id(), "embed");

        let image_model = registry
            .image_model("provider:image")
            .expect("image model resolves");
        assert_eq!(image_model.provider(), "provider");
        assert_eq!(image_model.model_id(), "image");
    }

    #[test]
    fn create_provider_registry_resolves_transcription_model_interface() {
        let registry = create_provider_registry([("provider", StaticProvider { id: "provider" })]);

        let transcription_model = registry
            .transcription_model("provider:whisper-1")
            .expect("transcription model resolves");
        assert_eq!(
            transcription_model.specification_version(),
            SpecificationVersion::V4
        );
        assert_eq!(transcription_model.provider(), "provider");
        assert_eq!(transcription_model.model_id(), "whisper-1");
    }

    #[test]
    fn create_provider_registry_resolves_speech_model_interface() {
        let registry = create_provider_registry([("provider", StaticProvider { id: "provider" })]);

        let speech_model = registry
            .speech_model("provider:tts-1")
            .expect("speech model resolves");
        assert_eq!(
            speech_model.specification_version(),
            SpecificationVersion::V4
        );
        assert_eq!(speech_model.provider(), "provider");
        assert_eq!(speech_model.model_id(), "tts-1");
    }

    #[test]
    fn create_provider_registry_resolves_reranking_model_interface() {
        let registry = create_provider_registry([("provider", StaticProvider { id: "provider" })]);

        let reranking_model = registry
            .reranking_model("provider:rerank-1")
            .expect("reranking model resolves");
        assert_eq!(
            reranking_model.specification_version(),
            SpecificationVersion::V4
        );
        assert_eq!(reranking_model.provider(), "provider");
        assert_eq!(reranking_model.model_id(), "rerank-1");
    }

    #[test]
    fn create_provider_registry_resolves_video_model_interface() {
        let registry = create_provider_registry([("provider", StaticProvider { id: "provider" })]);

        let video_model = registry
            .video_model("provider:video-1")
            .expect("video model resolves");
        assert_eq!(
            video_model.specification_version(),
            SpecificationVersion::V4
        );
        assert_eq!(video_model.provider(), "provider");
        assert_eq!(video_model.model_id(), "video-1");
    }

    #[test]
    fn create_provider_registry_resolves_files_interface() {
        let registry = create_provider_registry([("provider", StaticProvider { id: "provider" })]);

        let files = registry
            .files("provider")
            .expect("files interface resolves");

        assert_eq!(files.specification_version(), SpecificationVersion::V4);
        assert_eq!(files.provider(), "provider");
    }

    #[test]
    fn create_provider_registry_resolves_skills_interface() {
        let registry = create_provider_registry([("provider", StaticProvider { id: "provider" })]);

        let skills = registry
            .skills("provider")
            .expect("skills interface resolves");

        assert_eq!(skills.specification_version(), SpecificationVersion::V4);
        assert_eq!(skills.provider(), "provider");
    }

    #[test]
    fn create_provider_registry_supports_custom_separator() {
        let registry = create_provider_registry_with_options(
            [("provider", StaticProvider { id: "provider" })],
            ProviderRegistryOptions::new().with_separator("::"),
        );

        let model = registry
            .language_model("provider::chat")
            .expect("language model resolves");

        assert_eq!(registry.options().separator(), "::");
        assert_eq!(model.model_id(), "chat");
    }

    #[test]
    fn create_provider_registry_should_wrap_all_language_models_accessed_through_the_provider_registry()
     {
        let registry = create_provider_registry_with_language_model_middleware(
            [
                ("provider1", StaticProvider { id: "provider1" }),
                ("provider2", StaticProvider { id: "provider2" }),
            ],
            PrefixLanguageModelIdMiddleware,
            ProviderRegistryOptions::new(),
        );

        assert_eq!(
            registry
                .language_model("provider1:model-1")
                .expect("first provider model resolves")
                .model_id(),
            "override-model-1"
        );
        assert_eq!(
            registry
                .language_model("provider1:model-2")
                .expect("second model resolves")
                .model_id(),
            "override-model-2"
        );
        assert_eq!(
            registry
                .language_model("provider2:model-3")
                .expect("second provider model resolves")
                .model_id(),
            "override-model-3"
        );
    }

    #[test]
    fn create_provider_registry_should_wrap_all_image_models_accessed_through_the_provider_registry()
     {
        let registry = create_provider_registry_with_image_model_middleware(
            [
                ("provider1", StaticProvider { id: "provider1" }),
                ("provider2", StaticProvider { id: "provider2" }),
            ],
            PrefixImageModelIdMiddleware,
            ProviderRegistryOptions::new(),
        );

        assert_eq!(
            registry
                .image_model("provider1:model-1")
                .expect("first provider image model resolves")
                .model_id(),
            "override-model-1"
        );
        assert_eq!(
            registry
                .image_model("provider1:model-2")
                .expect("second image model resolves")
                .model_id(),
            "override-model-2"
        );
        assert_eq!(
            registry
                .image_model("provider2:model-3")
                .expect("second provider image model resolves")
                .model_id(),
            "override-model-3"
        );
    }

    #[test]
    fn provider_registry_reports_missing_provider_context() {
        let registry =
            create_provider_registry([("anthropic", StaticProvider { id: "anthropic" })]);

        let error = registry
            .language_model("openai:gpt-4.1")
            .expect_err("provider lookup fails");
        let provider_error = error
            .as_no_such_provider()
            .expect("error is missing provider");

        assert_eq!(provider_error.model_id(), "openai");
        assert_eq!(provider_error.model_type(), ModelType::LanguageModel);
        assert_eq!(provider_error.provider_id(), "openai");
        assert_eq!(
            provider_error.available_providers(),
            &["anthropic".to_string()]
        );
        assert_eq!(
            error.to_string(),
            "No such provider: openai (available providers: anthropic)"
        );
    }

    #[test]
    fn provider_registry_reports_missing_model_context() {
        let registry = create_provider_registry([("provider", StaticProvider { id: "provider" })]);

        let error = registry
            .embedding_model("provider:missing")
            .expect_err("model lookup fails");
        let model_error = error.as_no_such_model().expect("error is missing model");

        assert_eq!(model_error.model_id(), "missing");
        assert_eq!(model_error.model_type(), ModelType::EmbeddingModel);
        assert_eq!(error.to_string(), "No such embeddingModel: missing");
    }

    #[test]
    fn provider_registry_reports_missing_transcription_provider_context() {
        let registry =
            create_provider_registry([("anthropic", StaticProvider { id: "anthropic" })]);

        let error = registry
            .transcription_model("openai:whisper-1")
            .expect_err("provider lookup fails");
        let provider_error = error
            .as_no_such_provider()
            .expect("error is missing provider");

        assert_eq!(provider_error.model_id(), "openai");
        assert_eq!(provider_error.model_type(), ModelType::TranscriptionModel);
        assert_eq!(provider_error.provider_id(), "openai");
        assert_eq!(
            error.to_string(),
            "No such provider: openai (available providers: anthropic)"
        );
    }

    #[test]
    fn provider_registry_reports_missing_transcription_model_context() {
        let registry = create_provider_registry([("provider", StaticProvider { id: "provider" })]);

        let error = registry
            .transcription_model("provider:missing")
            .expect_err("model lookup fails");
        let model_error = error.as_no_such_model().expect("error is missing model");

        assert_eq!(model_error.model_id(), "missing");
        assert_eq!(model_error.model_type(), ModelType::TranscriptionModel);
        assert_eq!(error.to_string(), "No such transcriptionModel: missing");
    }

    #[test]
    fn provider_registry_reports_missing_speech_provider_context() {
        let registry =
            create_provider_registry([("anthropic", StaticProvider { id: "anthropic" })]);

        let error = registry
            .speech_model("openai:tts-1")
            .expect_err("provider lookup fails");
        let provider_error = error
            .as_no_such_provider()
            .expect("error is missing provider");

        assert_eq!(provider_error.model_id(), "openai");
        assert_eq!(provider_error.model_type(), ModelType::SpeechModel);
        assert_eq!(provider_error.provider_id(), "openai");
        assert_eq!(
            error.to_string(),
            "No such provider: openai (available providers: anthropic)"
        );
    }

    #[test]
    fn provider_registry_reports_missing_speech_model_context() {
        let registry = create_provider_registry([("provider", StaticProvider { id: "provider" })]);

        let error = registry
            .speech_model("provider:missing")
            .expect_err("model lookup fails");
        let model_error = error.as_no_such_model().expect("error is missing model");

        assert_eq!(model_error.model_id(), "missing");
        assert_eq!(model_error.model_type(), ModelType::SpeechModel);
        assert_eq!(error.to_string(), "No such speechModel: missing");
    }

    #[test]
    fn provider_registry_reports_missing_reranking_provider_context() {
        let registry =
            create_provider_registry([("anthropic", StaticProvider { id: "anthropic" })]);

        let error = registry
            .reranking_model("openai:rerank-1")
            .expect_err("provider lookup fails");
        let provider_error = error
            .as_no_such_provider()
            .expect("error is missing provider");

        assert_eq!(provider_error.model_id(), "openai");
        assert_eq!(provider_error.model_type(), ModelType::RerankingModel);
        assert_eq!(provider_error.provider_id(), "openai");
        assert_eq!(
            error.to_string(),
            "No such provider: openai (available providers: anthropic)"
        );
    }

    #[test]
    fn provider_registry_reports_missing_reranking_model_context() {
        let registry = create_provider_registry([("provider", StaticProvider { id: "provider" })]);

        let error = registry
            .reranking_model("provider:missing")
            .expect_err("model lookup fails");
        let model_error = error.as_no_such_model().expect("error is missing model");

        assert_eq!(model_error.model_id(), "missing");
        assert_eq!(model_error.model_type(), ModelType::RerankingModel);
        assert_eq!(error.to_string(), "No such rerankingModel: missing");
    }

    #[test]
    fn provider_registry_reports_missing_video_provider_context() {
        let registry =
            create_provider_registry([("anthropic", StaticProvider { id: "anthropic" })]);

        let error = registry
            .video_model("openai:video-1")
            .expect_err("provider lookup fails");
        let provider_error = error
            .as_no_such_provider()
            .expect("error is missing provider");

        assert_eq!(provider_error.model_id(), "openai");
        assert_eq!(provider_error.model_type(), ModelType::VideoModel);
        assert_eq!(provider_error.provider_id(), "openai");
        assert_eq!(
            error.to_string(),
            "No such provider: openai (available providers: anthropic)"
        );
    }

    #[test]
    fn provider_registry_reports_missing_video_model_context() {
        let registry = create_provider_registry([("provider", StaticProvider { id: "provider" })]);

        let error = registry
            .video_model("provider:missing")
            .expect_err("model lookup fails");
        let model_error = error.as_no_such_model().expect("error is missing model");

        assert_eq!(model_error.model_id(), "missing");
        assert_eq!(model_error.model_type(), ModelType::VideoModel);
        assert_eq!(error.to_string(), "No such videoModel: missing");
    }

    #[test]
    fn provider_registry_reports_missing_files_provider_context() {
        let registry =
            create_provider_registry([("anthropic", StaticProvider { id: "anthropic" })]);

        let error = registry.files("openai").expect_err("provider lookup fails");
        let provider_error = error
            .as_no_such_provider()
            .expect("error is missing provider");

        assert_eq!(provider_error.model_id(), "openai");
        assert_eq!(provider_error.model_type(), ModelType::LanguageModel);
        assert_eq!(provider_error.provider_id(), "openai");
        assert_eq!(
            error.to_string(),
            "No such provider: openai (available providers: anthropic)"
        );
    }

    #[test]
    fn provider_registry_reports_missing_skills_provider_context() {
        let registry =
            create_provider_registry([("anthropic", StaticProvider { id: "anthropic" })]);

        let error = registry
            .skills("openai")
            .expect_err("provider lookup fails");
        let provider_error = error
            .as_no_such_provider()
            .expect("error is missing provider");

        assert_eq!(provider_error.model_id(), "openai");
        assert_eq!(provider_error.model_type(), ModelType::LanguageModel);
        assert_eq!(provider_error.provider_id(), "openai");
        assert_eq!(
            error.to_string(),
            "No such provider: openai (available providers: anthropic)"
        );
    }

    #[test]
    fn no_such_provider_error_matches_upstream_default_message() {
        let error = NoSuchProviderError::new(
            "openai:gpt-4.1",
            ModelType::LanguageModel,
            "openai",
            ["anthropic", "google"],
        );

        assert_eq!(error.model_id(), "openai:gpt-4.1");
        assert_eq!(error.model_type(), ModelType::LanguageModel);
        assert_eq!(error.provider_id(), "openai");
        assert_eq!(
            error.available_providers(),
            &["anthropic".to_string(), "google".to_string()]
        );
        assert_eq!(
            error.message(),
            "No such provider: openai (available providers: anthropic,google)"
        );
        assert_eq!(
            error.to_string(),
            "No such provider: openai (available providers: anthropic,google)"
        );
    }

    #[test]
    fn no_such_provider_error_retains_custom_message_context() {
        let error = NoSuchProviderError::with_message(
            "missing",
            ModelType::EmbeddingModel,
            "missing",
            ["openai"],
            "registry lookup failed",
        );

        assert_eq!(error.message(), "registry lookup failed");

        let (model_id, model_type, provider_id, available_providers, message) = error.into_parts();
        assert_eq!(model_id, "missing");
        assert_eq!(model_type, ModelType::EmbeddingModel);
        assert_eq!(provider_id, "missing");
        assert_eq!(available_providers, vec!["openai".to_string()]);
        assert_eq!(message, "registry lookup failed");
    }
}
