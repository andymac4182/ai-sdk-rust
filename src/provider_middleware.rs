use crate::image_model_middleware::{ImageModelMiddleware, WrappedImageModel, wrap_image_model};
use crate::language_model_middleware::{
    LanguageModelMiddleware, WrappedLanguageModel, wrap_language_model,
};
use crate::provider::{
    NoSuchModelError, Provider, ProviderWithFiles, ProviderWithRerankingModel, ProviderWithSkills,
    ProviderWithSpeechModel, ProviderWithTranscriptionModel, ProviderWithVideoModel,
    SpecificationVersion,
};

/// Provider wrapper that applies language model middleware to every lookup.
///
/// Upstream `wrapProvider` applies middleware around all language models
/// resolved from a provider. Rust models that behavior with an owned provider
/// and cloneable middleware, so each model lookup receives a fresh model
/// wrapper while embedding and image lookups pass through unchanged.
#[derive(Clone, Debug)]
pub struct WrappedProvider<P, LW> {
    provider: P,
    language_model_middleware: LW,
}

impl<P, LW> WrappedProvider<P, LW> {
    /// Creates a provider wrapper that applies middleware to language models.
    pub fn new(provider: P, language_model_middleware: LW) -> Self {
        Self {
            provider,
            language_model_middleware,
        }
    }

    /// Returns the wrapped provider.
    pub fn provider(&self) -> &P {
        &self.provider
    }

    /// Returns the language model middleware applied by this wrapper.
    pub fn language_model_middleware(&self) -> &LW {
        &self.language_model_middleware
    }

    /// Consumes the wrapper into the provider and middleware.
    pub fn into_parts(self) -> (P, LW) {
        (self.provider, self.language_model_middleware)
    }
}

/// Wraps a provider with language model middleware.
pub fn wrap_provider<P, LW>(provider: P, language_model_middleware: LW) -> WrappedProvider<P, LW> {
    WrappedProvider::new(provider, language_model_middleware)
}

impl<P, LW> Provider for WrappedProvider<P, LW>
where
    P: Provider,
    P::LanguageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
{
    type LanguageModel = WrappedLanguageModel<P::LanguageModel, LW>;
    type EmbeddingModel = P::EmbeddingModel;
    type ImageModel = P::ImageModel;

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(wrap_language_model(
            self.provider.language_model(model_id)?,
            self.language_model_middleware.clone(),
        ))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        self.provider.embedding_model(model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        self.provider.image_model(model_id)
    }
}

/// Provider wrapper that applies image model middleware to every image lookup.
///
/// This mirrors the upstream `imageModelMiddleware` option for provider
/// registry use without requiring a language-model middleware value.
#[derive(Clone, Debug)]
pub struct WrappedProviderWithImageModelMiddleware<P, IW> {
    provider: P,
    image_model_middleware: IW,
}

impl<P, IW> WrappedProviderWithImageModelMiddleware<P, IW> {
    /// Creates a provider wrapper that applies middleware to image models.
    pub fn new(provider: P, image_model_middleware: IW) -> Self {
        Self {
            provider,
            image_model_middleware,
        }
    }

    /// Returns the wrapped provider.
    pub fn provider(&self) -> &P {
        &self.provider
    }

    /// Returns the image model middleware applied by this wrapper.
    pub fn image_model_middleware(&self) -> &IW {
        &self.image_model_middleware
    }

    /// Consumes the wrapper into the provider and image middleware.
    pub fn into_parts(self) -> (P, IW) {
        (self.provider, self.image_model_middleware)
    }
}

/// Wraps a provider with image model middleware.
pub fn wrap_provider_with_image_model_middleware<P, IW>(
    provider: P,
    image_model_middleware: IW,
) -> WrappedProviderWithImageModelMiddleware<P, IW> {
    WrappedProviderWithImageModelMiddleware::new(provider, image_model_middleware)
}

impl<P, IW> Provider for WrappedProviderWithImageModelMiddleware<P, IW>
where
    P: Provider,
    P::ImageModel: Sync,
    IW: ImageModelMiddleware<P::ImageModel> + Clone + Sync,
{
    type LanguageModel = P::LanguageModel;
    type EmbeddingModel = P::EmbeddingModel;
    type ImageModel = WrappedImageModel<P::ImageModel, IW>;

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        self.provider.language_model(model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        self.provider.embedding_model(model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(wrap_image_model(
            self.provider.image_model(model_id)?,
            self.image_model_middleware.clone(),
        ))
    }
}

/// Provider wrapper that applies language and image model middleware.
///
/// This mirrors the upstream `imageModelMiddleware` option on `wrapProvider`.
/// Embedding and optional provider extension methods are forwarded unchanged.
#[derive(Clone, Debug)]
pub struct WrappedProviderWithImageMiddleware<P, LW, IW> {
    provider: P,
    language_model_middleware: LW,
    image_model_middleware: IW,
}

impl<P, LW, IW> WrappedProviderWithImageMiddleware<P, LW, IW> {
    /// Creates a provider wrapper that applies language and image middleware.
    pub fn new(provider: P, language_model_middleware: LW, image_model_middleware: IW) -> Self {
        Self {
            provider,
            language_model_middleware,
            image_model_middleware,
        }
    }

    /// Returns the wrapped provider.
    pub fn provider(&self) -> &P {
        &self.provider
    }

    /// Returns the language model middleware applied by this wrapper.
    pub fn language_model_middleware(&self) -> &LW {
        &self.language_model_middleware
    }

    /// Returns the image model middleware applied by this wrapper.
    pub fn image_model_middleware(&self) -> &IW {
        &self.image_model_middleware
    }

    /// Consumes the wrapper into the provider and middleware values.
    pub fn into_parts(self) -> (P, LW, IW) {
        (
            self.provider,
            self.language_model_middleware,
            self.image_model_middleware,
        )
    }
}

/// Wraps a provider with language and image model middleware.
pub fn wrap_provider_with_image_middleware<P, LW, IW>(
    provider: P,
    language_model_middleware: LW,
    image_model_middleware: IW,
) -> WrappedProviderWithImageMiddleware<P, LW, IW> {
    WrappedProviderWithImageMiddleware::new(
        provider,
        language_model_middleware,
        image_model_middleware,
    )
}

impl<P, LW, IW> Provider for WrappedProviderWithImageMiddleware<P, LW, IW>
where
    P: Provider,
    P::LanguageModel: Sync,
    P::ImageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
    IW: ImageModelMiddleware<P::ImageModel> + Clone + Sync,
{
    type LanguageModel = WrappedLanguageModel<P::LanguageModel, LW>;
    type EmbeddingModel = P::EmbeddingModel;
    type ImageModel = WrappedImageModel<P::ImageModel, IW>;

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(wrap_language_model(
            self.provider.language_model(model_id)?,
            self.language_model_middleware.clone(),
        ))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        self.provider.embedding_model(model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(wrap_image_model(
            self.provider.image_model(model_id)?,
            self.image_model_middleware.clone(),
        ))
    }
}

impl<P, LW> ProviderWithTranscriptionModel for WrappedProvider<P, LW>
where
    P: ProviderWithTranscriptionModel,
    P::LanguageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
{
    type TranscriptionModel = P::TranscriptionModel;

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
        self.provider.transcription_model(model_id)
    }
}

impl<P, LW> ProviderWithSpeechModel for WrappedProvider<P, LW>
where
    P: ProviderWithSpeechModel,
    P::LanguageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
{
    type SpeechModel = P::SpeechModel;

    fn speech_model(&self, model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError> {
        self.provider.speech_model(model_id)
    }
}

impl<P, LW> ProviderWithRerankingModel for WrappedProvider<P, LW>
where
    P: ProviderWithRerankingModel,
    P::LanguageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
{
    type RerankingModel = P::RerankingModel;

    fn reranking_model(&self, model_id: &str) -> Result<Self::RerankingModel, NoSuchModelError> {
        self.provider.reranking_model(model_id)
    }
}

impl<P, LW> ProviderWithVideoModel for WrappedProvider<P, LW>
where
    P: ProviderWithVideoModel,
    P::LanguageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
{
    type VideoModel = P::VideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        self.provider.video_model(model_id)
    }
}

impl<P, LW> ProviderWithFiles for WrappedProvider<P, LW>
where
    P: ProviderWithFiles,
    P::LanguageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
{
    type Files = P::Files;

    fn files(&self) -> Self::Files {
        self.provider.files()
    }
}

impl<P, LW> ProviderWithSkills for WrappedProvider<P, LW>
where
    P: ProviderWithSkills,
    P::LanguageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
{
    type Skills = P::Skills;

    fn skills(&self) -> Self::Skills {
        self.provider.skills()
    }
}

impl<P, LW, IW> ProviderWithTranscriptionModel for WrappedProviderWithImageMiddleware<P, LW, IW>
where
    P: ProviderWithTranscriptionModel,
    P::LanguageModel: Sync,
    P::ImageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
    IW: ImageModelMiddleware<P::ImageModel> + Clone + Sync,
{
    type TranscriptionModel = P::TranscriptionModel;

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
        self.provider.transcription_model(model_id)
    }
}

impl<P, LW, IW> ProviderWithSpeechModel for WrappedProviderWithImageMiddleware<P, LW, IW>
where
    P: ProviderWithSpeechModel,
    P::LanguageModel: Sync,
    P::ImageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
    IW: ImageModelMiddleware<P::ImageModel> + Clone + Sync,
{
    type SpeechModel = P::SpeechModel;

    fn speech_model(&self, model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError> {
        self.provider.speech_model(model_id)
    }
}

impl<P, LW, IW> ProviderWithRerankingModel for WrappedProviderWithImageMiddleware<P, LW, IW>
where
    P: ProviderWithRerankingModel,
    P::LanguageModel: Sync,
    P::ImageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
    IW: ImageModelMiddleware<P::ImageModel> + Clone + Sync,
{
    type RerankingModel = P::RerankingModel;

    fn reranking_model(&self, model_id: &str) -> Result<Self::RerankingModel, NoSuchModelError> {
        self.provider.reranking_model(model_id)
    }
}

impl<P, LW, IW> ProviderWithVideoModel for WrappedProviderWithImageMiddleware<P, LW, IW>
where
    P: ProviderWithVideoModel,
    P::LanguageModel: Sync,
    P::ImageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
    IW: ImageModelMiddleware<P::ImageModel> + Clone + Sync,
{
    type VideoModel = P::VideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        self.provider.video_model(model_id)
    }
}

impl<P, LW, IW> ProviderWithFiles for WrappedProviderWithImageMiddleware<P, LW, IW>
where
    P: ProviderWithFiles,
    P::LanguageModel: Sync,
    P::ImageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
    IW: ImageModelMiddleware<P::ImageModel> + Clone + Sync,
{
    type Files = P::Files;

    fn files(&self) -> Self::Files {
        self.provider.files()
    }
}

impl<P, LW, IW> ProviderWithSkills for WrappedProviderWithImageMiddleware<P, LW, IW>
where
    P: ProviderWithSkills,
    P::LanguageModel: Sync,
    P::ImageModel: Sync,
    LW: LanguageModelMiddleware<P::LanguageModel> + Clone + Sync,
    IW: ImageModelMiddleware<P::ImageModel> + Clone + Sync,
{
    type Skills = P::Skills;

    fn skills(&self) -> Self::Skills {
        self.provider.skills()
    }
}

#[cfg(test)]
mod tests {
    use super::{wrap_provider, wrap_provider_with_image_middleware};
    use crate::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResult};
    use crate::file_data::FileDataContent;
    use crate::image_model::{
        ImageModel, ImageModelCallOptions, ImageModelResponse, ImageModelResult,
    };
    use crate::image_model_middleware::{ImageModelMiddleware, ImageModelMiddlewareModelOptions};
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelFinishReason,
        LanguageModelGenerateResult, LanguageModelStreamPart, LanguageModelStreamResult,
        LanguageModelSupportedUrls, LanguageModelUsage,
    };
    use crate::language_model_middleware::{
        LanguageModelMiddleware, LanguageModelMiddlewareModelOptions,
    };
    use crate::provider::{
        ModelType, NoSuchModelError, Provider, ProviderWithTranscriptionModel, SpecificationVersion,
    };
    use crate::transcription_model::{
        TranscriptionModel, TranscriptionModelCallOptions, TranscriptionModelResponse,
        TranscriptionModelResult,
    };
    use std::future::{Pending, Ready, ready};
    use std::sync::{Arc, Mutex};
    use time::OffsetDateTime;

    #[derive(Clone, Debug)]
    struct StaticProvider;

    impl Provider for StaticProvider {
        type LanguageModel = StaticLanguageModel;
        type EmbeddingModel = StaticEmbeddingModel;
        type ImageModel = StaticImageModel;

        fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
            if model_id == "missing" {
                return Err(NoSuchModelError::new(model_id, ModelType::LanguageModel));
            }

            Ok(StaticLanguageModel::new(model_id))
        }

        fn embedding_model(
            &self,
            model_id: &str,
        ) -> Result<Self::EmbeddingModel, NoSuchModelError> {
            Ok(StaticEmbeddingModel::new(model_id))
        }

        fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
            Ok(StaticImageModel::new(model_id))
        }
    }

    impl ProviderWithTranscriptionModel for StaticProvider {
        type TranscriptionModel = StaticTranscriptionModel;

        fn transcription_model(
            &self,
            model_id: &str,
        ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
            Ok(StaticTranscriptionModel::new(model_id))
        }
    }

    #[derive(Clone, Debug)]
    struct StaticLanguageModel {
        model_id: String,
    }

    impl StaticLanguageModel {
        fn new(model_id: &str) -> Self {
            Self {
                model_id: model_id.to_string(),
            }
        }
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
            "static-provider"
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(LanguageModelSupportedUrls::new())
        }

        fn do_generate(&self, _options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(LanguageModelGenerateResult::new(
                Vec::new(),
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

    #[derive(Clone, Debug)]
    struct StaticEmbeddingModel {
        model_id: String,
    }

    impl StaticEmbeddingModel {
        fn new(model_id: &str) -> Self {
            Self {
                model_id: model_id.to_string(),
            }
        }
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
            "static-provider"
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_> {
            ready(Some(8))
        }

        fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
            ready(true)
        }

        fn do_embed(&self, _options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
            ready(EmbeddingModelResult::new(vec![vec![1.0]]))
        }
    }

    #[derive(Clone, Debug)]
    struct StaticImageModel {
        model_id: String,
    }

    impl StaticImageModel {
        fn new(model_id: &str) -> Self {
            Self {
                model_id: model_id.to_string(),
            }
        }
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
            "static-provider"
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
            ready(Some(4))
        }

        fn do_generate(&self, _options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(ImageModelResult::new(
                vec![FileDataContent::Base64("image".to_string())],
                ImageModelResponse::new(OffsetDateTime::UNIX_EPOCH, self.model_id.clone()),
            ))
        }
    }

    #[derive(Clone, Debug)]
    struct StaticTranscriptionModel {
        model_id: String,
    }

    impl StaticTranscriptionModel {
        fn new(model_id: &str) -> Self {
            Self {
                model_id: model_id.to_string(),
            }
        }
    }

    impl TranscriptionModel for StaticTranscriptionModel {
        type GenerateFuture<'a>
            = Ready<TranscriptionModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "static-provider"
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn do_generate(&self, _options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_> {
            ready(TranscriptionModelResult::new(
                "transcript",
                Vec::new(),
                TranscriptionModelResponse::new(OffsetDateTime::UNIX_EPOCH, self.model_id.clone()),
            ))
        }
    }

    #[derive(Clone, Debug)]
    struct RecordingLanguageMiddleware {
        seen_model_ids: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingLanguageMiddleware {
        fn new() -> Self {
            Self {
                seen_model_ids: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn seen_model_ids(&self) -> Vec<String> {
            self.seen_model_ids.lock().unwrap().clone()
        }
    }

    impl LanguageModelMiddleware<StaticLanguageModel> for RecordingLanguageMiddleware {
        type OverrideSupportedUrlsFuture<'a>
            = Pending<LanguageModelSupportedUrls>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        type TransformParamsFuture<'a>
            = Pending<LanguageModelCallOptions>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        type WrapGenerateFuture<'a>
            = Pending<LanguageModelGenerateResult>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        type WrapStreamFuture<'a>
            = Pending<LanguageModelStreamResult<Vec<LanguageModelStreamPart>>>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        fn override_model_id(
            &self,
            options: LanguageModelMiddlewareModelOptions<'_, StaticLanguageModel>,
        ) -> Option<String> {
            self.seen_model_ids
                .lock()
                .unwrap()
                .push(options.model.model_id().to_string());

            Some(format!("override-{}", options.model.model_id()))
        }
    }

    #[derive(Clone, Debug)]
    struct NoopLanguageMiddleware;

    impl LanguageModelMiddleware<StaticLanguageModel> for NoopLanguageMiddleware {
        type OverrideSupportedUrlsFuture<'a>
            = Pending<LanguageModelSupportedUrls>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        type TransformParamsFuture<'a>
            = Pending<LanguageModelCallOptions>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        type WrapGenerateFuture<'a>
            = Pending<LanguageModelGenerateResult>
        where
            Self: 'a,
            StaticLanguageModel: 'a;

        type WrapStreamFuture<'a>
            = Pending<LanguageModelStreamResult<Vec<LanguageModelStreamPart>>>
        where
            Self: 'a,
            StaticLanguageModel: 'a;
    }

    #[derive(Clone, Debug)]
    struct RecordingImageMiddleware {
        seen_model_ids: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingImageMiddleware {
        fn new() -> Self {
            Self {
                seen_model_ids: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn seen_model_ids(&self) -> Vec<String> {
            self.seen_model_ids.lock().unwrap().clone()
        }
    }

    impl ImageModelMiddleware<StaticImageModel> for RecordingImageMiddleware {
        type OverrideMaxImagesPerCallFuture<'a>
            = Pending<Option<usize>>
        where
            Self: 'a,
            StaticImageModel: 'a;

        type TransformParamsFuture<'a>
            = Pending<ImageModelCallOptions>
        where
            Self: 'a,
            StaticImageModel: 'a;

        type WrapGenerateFuture<'a>
            = Pending<ImageModelResult>
        where
            Self: 'a,
            StaticImageModel: 'a;

        fn override_model_id(
            &self,
            options: ImageModelMiddlewareModelOptions<'_, StaticImageModel>,
        ) -> Option<String> {
            self.seen_model_ids
                .lock()
                .unwrap()
                .push(options.model.model_id().to_string());

            Some(format!("override-{}", options.model.model_id()))
        }
    }

    #[test]
    fn wrap_provider_wraps_all_language_model_lookups() {
        let middleware = RecordingLanguageMiddleware::new();
        let provider = wrap_provider(StaticProvider, middleware.clone());

        assert_eq!(provider.specification_version(), SpecificationVersion::V4);
        assert_eq!(
            provider.language_model("model-1").unwrap().model_id(),
            "override-model-1"
        );
        assert_eq!(
            provider.language_model("model-2").unwrap().model_id(),
            "override-model-2"
        );
        assert_eq!(
            provider.language_model("model-3").unwrap().model_id(),
            "override-model-3"
        );
        assert_eq!(
            middleware.seen_model_ids(),
            vec!["model-1", "model-2", "model-3"]
        );
    }

    #[test]
    fn wrap_provider_preserves_embedding_and_required_missing_model_errors() {
        let provider = wrap_provider(StaticProvider, NoopLanguageMiddleware);

        assert_eq!(
            provider
                .embedding_model("embedding-model")
                .unwrap()
                .model_id(),
            "embedding-model"
        );
        assert_eq!(
            provider.image_model("image-model").unwrap().model_id(),
            "image-model"
        );

        let missing = provider.language_model("missing").unwrap_err();
        assert_eq!(missing.message(), "No such languageModel: missing");
    }

    #[test]
    fn wrap_provider_with_image_middleware_wraps_all_image_model_lookups() {
        let image_middleware = RecordingImageMiddleware::new();
        let provider = wrap_provider_with_image_middleware(
            StaticProvider,
            NoopLanguageMiddleware,
            image_middleware.clone(),
        );

        assert_eq!(
            provider.image_model("model-1").unwrap().model_id(),
            "override-model-1"
        );
        assert_eq!(
            provider.image_model("model-2").unwrap().model_id(),
            "override-model-2"
        );
        assert_eq!(
            provider.image_model("model-3").unwrap().model_id(),
            "override-model-3"
        );
        assert_eq!(
            image_middleware.seen_model_ids(),
            vec!["model-1", "model-2", "model-3"]
        );
    }

    #[test]
    fn wrapped_provider_forwards_optional_model_interfaces() {
        let provider = wrap_provider(StaticProvider, NoopLanguageMiddleware);

        assert_eq!(
            provider
                .transcription_model("transcription-model")
                .unwrap()
                .model_id(),
            "transcription-model"
        );
    }
}
