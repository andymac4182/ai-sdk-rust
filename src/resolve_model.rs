use crate::provider::{
    NoSuchModelError, Provider, ProviderWithRerankingModel, ProviderWithSpeechModel,
    ProviderWithTranscriptionModel, ProviderWithVideoModel,
};

/// A model supplied either directly or by provider-specific model id.
#[derive(Clone, Copy, Debug)]
pub enum ModelSource<'a, M> {
    /// Already-resolved model object.
    Model(&'a M),

    /// Provider-specific model id that should be resolved through a provider.
    Id(&'a str),
}

impl<'a, M> ModelSource<'a, M> {
    /// Creates a direct model source.
    pub const fn model(model: &'a M) -> Self {
        Self::Model(model)
    }

    /// Creates a provider-id model source.
    pub const fn id(model_id: &'a str) -> Self {
        Self::Id(model_id)
    }
}

impl<'a, M> From<&'a M> for ModelSource<'a, M> {
    fn from(model: &'a M) -> Self {
        Self::Model(model)
    }
}

/// A resolved model that preserves direct model identity when possible.
#[derive(Clone, Debug)]
pub enum ResolvedModel<'a, M> {
    /// The caller supplied a direct model object.
    Borrowed(&'a M),

    /// The provider returned an owned model for a model id.
    Owned(M),
}

impl<'a, M> ResolvedModel<'a, M> {
    /// Returns `true` when resolution preserved a direct model reference.
    pub const fn is_borrowed(&self) -> bool {
        matches!(self, Self::Borrowed(_))
    }

    /// Returns `true` when resolution looked up a model id through a provider.
    pub const fn is_owned(&self) -> bool {
        matches!(self, Self::Owned(_))
    }

    /// Converts the resolved model into an owned value.
    pub fn into_owned(self) -> M
    where
        M: Clone,
    {
        match self {
            Self::Borrowed(model) => model.clone(),
            Self::Owned(model) => model,
        }
    }
}

impl<M> AsRef<M> for ResolvedModel<'_, M> {
    fn as_ref(&self) -> &M {
        match self {
            Self::Borrowed(model) => model,
            Self::Owned(model) => model,
        }
    }
}

/// Resolves a language model source through the supplied provider.
pub fn resolve_language_model<'a, P>(
    provider: &P,
    model: ModelSource<'a, P::LanguageModel>,
) -> Result<ResolvedModel<'a, P::LanguageModel>, NoSuchModelError>
where
    P: Provider,
{
    match model {
        ModelSource::Model(model) => Ok(ResolvedModel::Borrowed(model)),
        ModelSource::Id(model_id) => provider.language_model(model_id).map(ResolvedModel::Owned),
    }
}

/// Resolves an embedding model source through the supplied provider.
pub fn resolve_embedding_model<'a, P>(
    provider: &P,
    model: ModelSource<'a, P::EmbeddingModel>,
) -> Result<ResolvedModel<'a, P::EmbeddingModel>, NoSuchModelError>
where
    P: Provider,
{
    match model {
        ModelSource::Model(model) => Ok(ResolvedModel::Borrowed(model)),
        ModelSource::Id(model_id) => provider.embedding_model(model_id).map(ResolvedModel::Owned),
    }
}

/// Resolves an image model source through the supplied provider.
pub fn resolve_image_model<'a, P>(
    provider: &P,
    model: ModelSource<'a, P::ImageModel>,
) -> Result<ResolvedModel<'a, P::ImageModel>, NoSuchModelError>
where
    P: Provider,
{
    match model {
        ModelSource::Model(model) => Ok(ResolvedModel::Borrowed(model)),
        ModelSource::Id(model_id) => provider.image_model(model_id).map(ResolvedModel::Owned),
    }
}

/// Resolves a transcription model source through the supplied provider.
pub fn resolve_transcription_model<'a, P>(
    provider: &P,
    model: ModelSource<'a, P::TranscriptionModel>,
) -> Result<ResolvedModel<'a, P::TranscriptionModel>, NoSuchModelError>
where
    P: ProviderWithTranscriptionModel,
{
    match model {
        ModelSource::Model(model) => Ok(ResolvedModel::Borrowed(model)),
        ModelSource::Id(model_id) => provider
            .transcription_model(model_id)
            .map(ResolvedModel::Owned),
    }
}

/// Resolves a speech model source through the supplied provider.
pub fn resolve_speech_model<'a, P>(
    provider: &P,
    model: ModelSource<'a, P::SpeechModel>,
) -> Result<ResolvedModel<'a, P::SpeechModel>, NoSuchModelError>
where
    P: ProviderWithSpeechModel,
{
    match model {
        ModelSource::Model(model) => Ok(ResolvedModel::Borrowed(model)),
        ModelSource::Id(model_id) => provider.speech_model(model_id).map(ResolvedModel::Owned),
    }
}

/// Resolves a reranking model source through the supplied provider.
pub fn resolve_reranking_model<'a, P>(
    provider: &P,
    model: ModelSource<'a, P::RerankingModel>,
) -> Result<ResolvedModel<'a, P::RerankingModel>, NoSuchModelError>
where
    P: ProviderWithRerankingModel,
{
    match model {
        ModelSource::Model(model) => Ok(ResolvedModel::Borrowed(model)),
        ModelSource::Id(model_id) => provider.reranking_model(model_id).map(ResolvedModel::Owned),
    }
}

/// Resolves a video model source through the supplied provider.
pub fn resolve_video_model<'a, P>(
    provider: &P,
    model: ModelSource<'a, P::VideoModel>,
) -> Result<ResolvedModel<'a, P::VideoModel>, NoSuchModelError>
where
    P: ProviderWithVideoModel,
{
    match model {
        ModelSource::Model(model) => Ok(ResolvedModel::Borrowed(model)),
        ModelSource::Id(model_id) => provider.video_model(model_id).map(ResolvedModel::Owned),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ModelSource, resolve_embedding_model, resolve_image_model, resolve_language_model,
        resolve_reranking_model, resolve_speech_model, resolve_transcription_model,
        resolve_video_model,
    };
    use crate::embedding_model::EmbeddingModel;
    use crate::gateway::GatewayProvider;
    use crate::image_model::ImageModel;
    use crate::language_model::LanguageModel;
    use crate::mock_models::{
        MockEmbeddingModel, MockImageModel, MockLanguageModel, MockProvider, MockRerankingModel,
        MockSpeechModel, MockTranscriptionModel, MockVideoModel,
    };
    use crate::provider::ModelType;
    use crate::reranking_model::RerankingModel;
    use crate::speech_model::SpeechModel;
    use crate::transcription_model::TranscriptionModel;
    use crate::video_model::VideoModel;

    #[test]
    fn resolve_language_model_should_return_it_as_is() {
        let provider = MockProvider::new();
        let model = MockLanguageModel::new()
            .with_provider("test-provider")
            .with_model_id("test-model-id");

        let resolved = resolve_language_model(&provider, ModelSource::model(&model))
            .expect("direct language model resolves");

        assert!(resolved.is_borrowed());
        assert!(std::ptr::eq(resolved.as_ref(), &model));
        assert_eq!(resolved.as_ref().provider(), "test-provider");
        assert_eq!(resolved.as_ref().model_id(), "test-model-id");
    }

    #[test]
    fn resolve_language_model_should_return_a_gateway_language_model() {
        let provider = GatewayProvider::new();

        let resolved = resolve_language_model(&provider, ModelSource::id("test-model-id"))
            .expect("gateway language model resolves");

        assert!(resolved.is_owned());
        assert_eq!(resolved.as_ref().provider(), "gateway");
        assert_eq!(resolved.as_ref().model_id(), "test-model-id");
    }

    #[test]
    fn resolve_language_model_should_return_a_language_model_from_the_default_provider() {
        let provider = MockProvider::new().with_language_model(
            "test-model-id",
            MockLanguageModel::new()
                .with_provider("global-test-provider")
                .with_model_id("actual-test-model-id"),
        );

        let resolved = resolve_language_model(&provider, ModelSource::id("test-model-id"))
            .expect("default provider language model resolves");

        assert_eq!(resolved.as_ref().provider(), "global-test-provider");
        assert_eq!(resolved.as_ref().model_id(), "actual-test-model-id");
    }

    #[test]
    fn resolve_embedding_model_should_return_it_as_is() {
        let provider = MockProvider::new();
        let model = MockEmbeddingModel::new()
            .with_provider("test-provider")
            .with_model_id("test-model-id");

        let resolved = resolve_embedding_model(&provider, ModelSource::model(&model))
            .expect("direct embedding model resolves");

        assert!(resolved.is_borrowed());
        assert!(std::ptr::eq(resolved.as_ref(), &model));
        assert_eq!(resolved.as_ref().provider(), "test-provider");
        assert_eq!(resolved.as_ref().model_id(), "test-model-id");
    }

    #[test]
    fn resolve_embedding_model_should_return_a_gateway_embedding_model() {
        let provider = GatewayProvider::new();

        let resolved = resolve_embedding_model(&provider, ModelSource::id("test-model-id"))
            .expect("gateway embedding model resolves");

        assert!(resolved.is_owned());
        assert_eq!(resolved.as_ref().provider(), "gateway");
        assert_eq!(resolved.as_ref().model_id(), "test-model-id");
    }

    #[test]
    fn resolve_embedding_model_should_return_an_embedding_model_from_the_default_provider() {
        let provider = MockProvider::new().with_embedding_model(
            "test-model-id",
            MockEmbeddingModel::new()
                .with_provider("global-test-provider")
                .with_model_id("actual-test-model-id"),
        );

        let resolved = resolve_embedding_model(&provider, ModelSource::id("test-model-id"))
            .expect("default provider embedding model resolves");

        assert_eq!(resolved.as_ref().provider(), "global-test-provider");
        assert_eq!(resolved.as_ref().model_id(), "actual-test-model-id");
    }

    #[test]
    fn resolve_image_model_should_return_it_as_is() {
        let provider = MockProvider::new();
        let model = MockImageModel::new()
            .with_provider("test-provider")
            .with_model_id("test-model-id");

        let resolved = resolve_image_model(&provider, ModelSource::model(&model))
            .expect("direct image model resolves");

        assert!(resolved.is_borrowed());
        assert!(std::ptr::eq(resolved.as_ref(), &model));
        assert_eq!(resolved.as_ref().provider(), "test-provider");
        assert_eq!(resolved.as_ref().model_id(), "test-model-id");
    }

    #[test]
    fn resolve_image_model_should_return_a_gateway_image_model() {
        let provider = GatewayProvider::new();

        let resolved = resolve_image_model(&provider, ModelSource::id("test-model-id"))
            .expect("gateway image model resolves");

        assert!(resolved.is_owned());
        assert_eq!(resolved.as_ref().provider(), "gateway");
        assert_eq!(resolved.as_ref().model_id(), "test-model-id");
    }

    #[test]
    fn resolve_image_model_should_return_an_image_model_from_the_default_provider() {
        let provider = MockProvider::new().with_image_model(
            "test-model-id",
            MockImageModel::new()
                .with_provider("global-test-provider")
                .with_model_id("actual-test-model-id"),
        );

        let resolved = resolve_image_model(&provider, ModelSource::id("test-model-id"))
            .expect("default provider image model resolves");

        assert_eq!(resolved.as_ref().provider(), "global-test-provider");
        assert_eq!(resolved.as_ref().model_id(), "actual-test-model-id");
    }

    #[test]
    fn resolve_video_model_should_return_it_as_is() {
        let provider = MockProvider::new();
        let model = MockVideoModel::new()
            .with_provider("test-provider")
            .with_model_id("test-model-id");

        let resolved = resolve_video_model(&provider, ModelSource::model(&model))
            .expect("direct video model resolves");

        assert!(resolved.is_borrowed());
        assert!(std::ptr::eq(resolved.as_ref(), &model));
        assert_eq!(resolved.as_ref().provider(), "test-provider");
        assert_eq!(resolved.as_ref().model_id(), "test-model-id");
    }

    #[test]
    fn resolve_video_model_should_return_a_gateway_video_model_converted_to_v4() {
        let provider = GatewayProvider::new();

        let resolved = resolve_video_model(&provider, ModelSource::id("test-model-id"))
            .expect("gateway video model resolves");

        assert!(resolved.is_owned());
        assert_eq!(resolved.as_ref().provider(), "gateway");
        assert_eq!(resolved.as_ref().model_id(), "test-model-id");
    }

    #[test]
    fn resolve_video_model_should_return_a_video_model_from_the_default_provider() {
        let provider = MockProvider::new().with_video_model(
            "test-model-id",
            MockVideoModel::new()
                .with_provider("global-test-provider")
                .with_model_id("actual-test-model-id"),
        );

        let resolved = resolve_video_model(&provider, ModelSource::id("test-model-id"))
            .expect("default provider video model resolves");

        assert_eq!(resolved.as_ref().provider(), "global-test-provider");
        assert_eq!(resolved.as_ref().model_id(), "actual-test-model-id");
    }

    #[test]
    fn resolve_reranking_model_should_return_it_as_is() {
        let provider = MockProvider::new();
        let model = MockRerankingModel::new()
            .with_provider("test-provider")
            .with_model_id("test-model-id");

        let resolved = resolve_reranking_model(&provider, ModelSource::model(&model))
            .expect("direct reranking model resolves");

        assert!(resolved.is_borrowed());
        assert!(std::ptr::eq(resolved.as_ref(), &model));
        assert_eq!(resolved.as_ref().provider(), "test-provider");
        assert_eq!(resolved.as_ref().model_id(), "test-model-id");
    }

    #[test]
    fn resolve_reranking_model_should_return_a_gateway_reranking_model_converted_to_v4() {
        let provider = GatewayProvider::new();

        let resolved = resolve_reranking_model(&provider, ModelSource::id("test-model-id"))
            .expect("gateway reranking model resolves");

        assert!(resolved.is_owned());
        assert_eq!(resolved.as_ref().provider(), "gateway");
        assert_eq!(resolved.as_ref().model_id(), "test-model-id");
    }

    #[test]
    fn resolve_reranking_model_should_return_a_reranking_model_from_the_default_provider() {
        let provider = MockProvider::new().with_reranking_model(
            "test-model-id",
            MockRerankingModel::new()
                .with_provider("global-test-provider")
                .with_model_id("actual-test-model-id"),
        );

        let resolved = resolve_reranking_model(&provider, ModelSource::id("test-model-id"))
            .expect("default provider reranking model resolves");

        assert_eq!(resolved.as_ref().provider(), "global-test-provider");
        assert_eq!(resolved.as_ref().model_id(), "actual-test-model-id");
    }

    #[test]
    fn resolve_reranking_model_should_report_missing_default_provider_support_as_no_such_model() {
        let provider = MockProvider::new();

        let error = resolve_reranking_model(&provider, ModelSource::id("test-model-id"))
            .expect_err("missing reranking model reports NoSuchModelError");

        assert_eq!(error.model_id(), "test-model-id");
        assert_eq!(error.model_type(), ModelType::RerankingModel);
    }

    #[test]
    fn resolve_video_model_should_report_missing_default_provider_support_as_no_such_model() {
        let provider = MockProvider::new();

        let error = resolve_video_model(&provider, ModelSource::id("test-model-id"))
            .expect_err("missing video model reports NoSuchModelError");

        assert_eq!(error.model_id(), "test-model-id");
        assert_eq!(error.model_type(), ModelType::VideoModel);
    }

    #[test]
    fn resolve_transcription_model_supports_the_upstream_resolve_function_surface() {
        let provider = MockProvider::new().with_transcription_model(
            "test-model-id",
            MockTranscriptionModel::new()
                .with_provider("global-test-provider")
                .with_model_id("actual-test-model-id"),
        );

        let resolved = resolve_transcription_model(&provider, ModelSource::id("test-model-id"))
            .expect("default provider transcription model resolves");

        assert_eq!(resolved.as_ref().provider(), "global-test-provider");
        assert_eq!(resolved.as_ref().model_id(), "actual-test-model-id");
    }

    #[test]
    fn resolve_speech_model_supports_the_upstream_resolve_function_surface() {
        let provider = MockProvider::new().with_speech_model(
            "test-model-id",
            MockSpeechModel::new()
                .with_provider("global-test-provider")
                .with_model_id("actual-test-model-id"),
        );

        let resolved = resolve_speech_model(&provider, ModelSource::id("test-model-id"))
            .expect("default provider speech model resolves");

        assert_eq!(resolved.as_ref().provider(), "global-test-provider");
        assert_eq!(resolved.as_ref().model_id(), "actual-test-model-id");
    }

    #[test]
    fn resolved_model_into_owned_clones_direct_models_and_moves_provider_models() {
        let provider = MockProvider::new().with_language_model(
            "test-model-id",
            MockLanguageModel::new().with_model_id("actual-test-model-id"),
        );
        let direct_model = MockLanguageModel::new().with_model_id("direct-model-id");

        let direct = resolve_language_model(&provider, ModelSource::model(&direct_model))
            .expect("direct model resolves")
            .into_owned();
        let owned = resolve_language_model(&provider, ModelSource::id("test-model-id"))
            .expect("provider model resolves")
            .into_owned();

        assert_eq!(direct.model_id(), "direct-model-id");
        assert_eq!(owned.model_id(), "actual-test-model-id");
    }
}
