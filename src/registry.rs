use std::{fmt, marker::PhantomData};

use crate::files::Files;
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
use crate::reranking_model::RerankingModel;
use crate::skills::Skills;
use crate::speech_model::SpeechModel;
use crate::transcription_model::TranscriptionModel;
use crate::video_model::VideoModel;

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
/// Rust resolves string aliases through the configured fallback provider
/// instead of JavaScript's ambient `globalThis.AI_SDK_DEFAULT_PROVIDER`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomProvider<LM, EM, IM, FP = NoFallbackProvider<LM, EM, IM>> {
    language_models: Vec<(String, LM)>,
    language_model_aliases: Vec<(String, String)>,
    embedding_models: Vec<(String, EM)>,
    embedding_model_aliases: Vec<(String, String)>,
    image_models: Vec<(String, IM)>,
    image_model_aliases: Vec<(String, String)>,
    transcription_model_aliases: Vec<(String, String)>,
    speech_model_aliases: Vec<(String, String)>,
    reranking_model_aliases: Vec<(String, String)>,
    video_model_aliases: Vec<(String, String)>,
    fallback_provider: FP,
}

/// Default `custom_provider` fallback that reports missing models.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoFallbackProvider<LM, EM, IM> {
    _models: PhantomData<(LM, EM, IM)>,
}

/// Custom provider wrapper that exposes a direct files interface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomProviderWithFiles<P, F> {
    provider: P,
    files: F,
}

/// Custom provider wrapper that exposes a direct skills interface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomProviderWithSkills<P, S> {
    provider: P,
    skills: S,
}

/// Custom provider wrapper that exposes direct transcription model maps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomProviderWithTranscriptionModel<P, TM> {
    provider: P,
    transcription_models: Vec<(String, TM)>,
}

/// Custom provider wrapper that exposes direct speech model maps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomProviderWithSpeechModel<P, SM> {
    provider: P,
    speech_models: Vec<(String, SM)>,
}

/// Custom provider wrapper that exposes direct reranking model maps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomProviderWithRerankingModel<P, RM> {
    provider: P,
    reranking_models: Vec<(String, RM)>,
}

/// Custom provider wrapper that exposes direct video model maps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomProviderWithVideoModel<P, VM> {
    provider: P,
    video_models: Vec<(String, VM)>,
}

impl<LM, EM, IM> Default for NoFallbackProvider<LM, EM, IM> {
    fn default() -> Self {
        Self {
            _models: PhantomData,
        }
    }
}

impl<LM, EM, IM> Provider for NoFallbackProvider<LM, EM, IM>
where
    LM: LanguageModel,
    EM: crate::embedding_model::EmbeddingModel,
    IM: ImageModel,
{
    type LanguageModel = LM;
    type EmbeddingModel = EM;
    type ImageModel = IM;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::LanguageModel))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }
}

impl<LM, EM, IM> CustomProvider<LM, EM, IM> {
    /// Creates an empty custom provider.
    pub fn new() -> Self {
        Self {
            language_models: Vec::new(),
            language_model_aliases: Vec::new(),
            embedding_models: Vec::new(),
            embedding_model_aliases: Vec::new(),
            image_models: Vec::new(),
            image_model_aliases: Vec::new(),
            transcription_model_aliases: Vec::new(),
            speech_model_aliases: Vec::new(),
            reranking_model_aliases: Vec::new(),
            video_model_aliases: Vec::new(),
            fallback_provider: NoFallbackProvider::default(),
        }
    }
}

impl<LM, EM, IM, FP> CustomProvider<LM, EM, IM, FP> {
    /// Registers a language model by model id.
    pub fn with_language_model(mut self, model_id: impl Into<String>, model: LM) -> Self {
        self.language_models.push((model_id.into(), model));
        self
    }

    /// Registers a language model alias resolved through the fallback provider.
    pub fn with_language_model_alias(
        mut self,
        model_id: impl Into<String>,
        target_model_id: impl Into<String>,
    ) -> Self {
        self.language_model_aliases
            .push((model_id.into(), target_model_id.into()));
        self
    }

    /// Registers an embedding model by model id.
    pub fn with_embedding_model(mut self, model_id: impl Into<String>, model: EM) -> Self {
        self.embedding_models.push((model_id.into(), model));
        self
    }

    /// Registers an embedding model alias resolved through the fallback provider.
    pub fn with_embedding_model_alias(
        mut self,
        model_id: impl Into<String>,
        target_model_id: impl Into<String>,
    ) -> Self {
        self.embedding_model_aliases
            .push((model_id.into(), target_model_id.into()));
        self
    }

    /// Registers an image model by model id.
    pub fn with_image_model(mut self, model_id: impl Into<String>, model: IM) -> Self {
        self.image_models.push((model_id.into(), model));
        self
    }

    /// Registers an image model alias resolved through the fallback provider.
    pub fn with_image_model_alias(
        mut self,
        model_id: impl Into<String>,
        target_model_id: impl Into<String>,
    ) -> Self {
        self.image_model_aliases
            .push((model_id.into(), target_model_id.into()));
        self
    }

    /// Sets a fallback provider used when a model id is not registered locally.
    pub fn with_fallback_provider<NewFP>(
        self,
        fallback_provider: NewFP,
    ) -> CustomProvider<LM, EM, IM, NewFP> {
        CustomProvider {
            language_models: self.language_models,
            language_model_aliases: self.language_model_aliases,
            embedding_models: self.embedding_models,
            embedding_model_aliases: self.embedding_model_aliases,
            image_models: self.image_models,
            image_model_aliases: self.image_model_aliases,
            transcription_model_aliases: self.transcription_model_aliases,
            speech_model_aliases: self.speech_model_aliases,
            reranking_model_aliases: self.reranking_model_aliases,
            video_model_aliases: self.video_model_aliases,
            fallback_provider,
        }
    }

    /// Exposes a direct files interface on this custom provider.
    pub fn with_files<F>(self, files: F) -> CustomProviderWithFiles<Self, F> {
        CustomProviderWithFiles {
            provider: self,
            files,
        }
    }

    /// Exposes a direct skills interface on this custom provider.
    pub fn with_skills<S>(self, skills: S) -> CustomProviderWithSkills<Self, S> {
        CustomProviderWithSkills {
            provider: self,
            skills,
        }
    }

    /// Registers a transcription model by model id.
    pub fn with_transcription_model<TM>(
        self,
        model_id: impl Into<String>,
        model: TM,
    ) -> CustomProviderWithTranscriptionModel<Self, TM> {
        CustomProviderWithTranscriptionModel {
            provider: self,
            transcription_models: vec![(model_id.into(), model)],
        }
    }

    /// Registers a transcription model alias resolved through the fallback provider.
    pub fn with_transcription_model_alias(
        mut self,
        model_id: impl Into<String>,
        target_model_id: impl Into<String>,
    ) -> Self {
        self.transcription_model_aliases
            .push((model_id.into(), target_model_id.into()));
        self
    }

    /// Registers a speech model by model id.
    pub fn with_speech_model<SM>(
        self,
        model_id: impl Into<String>,
        model: SM,
    ) -> CustomProviderWithSpeechModel<Self, SM> {
        CustomProviderWithSpeechModel {
            provider: self,
            speech_models: vec![(model_id.into(), model)],
        }
    }

    /// Registers a speech model alias resolved through the fallback provider.
    pub fn with_speech_model_alias(
        mut self,
        model_id: impl Into<String>,
        target_model_id: impl Into<String>,
    ) -> Self {
        self.speech_model_aliases
            .push((model_id.into(), target_model_id.into()));
        self
    }

    /// Registers a reranking model by model id.
    pub fn with_reranking_model<RM>(
        self,
        model_id: impl Into<String>,
        model: RM,
    ) -> CustomProviderWithRerankingModel<Self, RM> {
        CustomProviderWithRerankingModel {
            provider: self,
            reranking_models: vec![(model_id.into(), model)],
        }
    }

    /// Registers a reranking model alias resolved through the fallback provider.
    pub fn with_reranking_model_alias(
        mut self,
        model_id: impl Into<String>,
        target_model_id: impl Into<String>,
    ) -> Self {
        self.reranking_model_aliases
            .push((model_id.into(), target_model_id.into()));
        self
    }

    /// Registers a video model by model id.
    pub fn with_video_model<VM>(
        self,
        model_id: impl Into<String>,
        model: VM,
    ) -> CustomProviderWithVideoModel<Self, VM> {
        CustomProviderWithVideoModel {
            provider: self,
            video_models: vec![(model_id.into(), model)],
        }
    }

    /// Registers a video model alias resolved through the fallback provider.
    pub fn with_video_model_alias(
        mut self,
        model_id: impl Into<String>,
        target_model_id: impl Into<String>,
    ) -> Self {
        self.video_model_aliases
            .push((model_id.into(), target_model_id.into()));
        self
    }
}

impl<P, F> CustomProviderWithFiles<P, F> {
    /// Exposes a direct skills interface while retaining the files interface.
    pub fn with_skills<S>(self, skills: S) -> CustomProviderWithSkills<Self, S> {
        CustomProviderWithSkills {
            provider: self,
            skills,
        }
    }
}

impl<P, S> CustomProviderWithSkills<P, S> {
    /// Exposes a direct files interface while retaining the skills interface.
    pub fn with_files<F>(self, files: F) -> CustomProviderWithFiles<Self, F> {
        CustomProviderWithFiles {
            provider: self,
            files,
        }
    }
}

impl<LM, EM, IM> Default for CustomProvider<LM, EM, IM> {
    fn default() -> Self {
        Self::new()
    }
}

impl<LM, EM, IM, FP> Provider for CustomProvider<LM, EM, IM, FP>
where
    LM: LanguageModel + Clone,
    EM: crate::embedding_model::EmbeddingModel + Clone,
    IM: ImageModel + Clone,
    FP: Provider<LanguageModel = LM, EmbeddingModel = EM, ImageModel = IM>,
{
    type LanguageModel = LM;
    type EmbeddingModel = EM;
    type ImageModel = IM;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        if let Some(model) = self
            .language_models
            .iter()
            .find_map(|(id, model)| (id == model_id).then(|| model.clone()))
        {
            return Ok(model);
        }

        if let Some(target_model_id) = lookup_model_alias(&self.language_model_aliases, model_id) {
            return self.fallback_provider.language_model(target_model_id);
        }

        self.fallback_provider.language_model(model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        if let Some(model) = self
            .embedding_models
            .iter()
            .find_map(|(id, model)| (id == model_id).then(|| model.clone()))
        {
            return Ok(model);
        }

        if let Some(target_model_id) = lookup_model_alias(&self.embedding_model_aliases, model_id) {
            return self.fallback_provider.embedding_model(target_model_id);
        }

        self.fallback_provider.embedding_model(model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        if let Some(model) = self
            .image_models
            .iter()
            .find_map(|(id, model)| (id == model_id).then(|| model.clone()))
        {
            return Ok(model);
        }

        if let Some(target_model_id) = lookup_model_alias(&self.image_model_aliases, model_id) {
            return self.fallback_provider.image_model(target_model_id);
        }

        self.fallback_provider.image_model(model_id)
    }
}

impl<LM, EM, IM, FP> ProviderWithTranscriptionModel for CustomProvider<LM, EM, IM, FP>
where
    LM: LanguageModel + Clone,
    EM: crate::embedding_model::EmbeddingModel + Clone,
    IM: ImageModel + Clone,
    FP: ProviderWithTranscriptionModel<LanguageModel = LM, EmbeddingModel = EM, ImageModel = IM>,
{
    type TranscriptionModel = FP::TranscriptionModel;

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
        if let Some(target_model_id) =
            lookup_model_alias(&self.transcription_model_aliases, model_id)
        {
            return self.fallback_provider.transcription_model(target_model_id);
        }

        self.fallback_provider.transcription_model(model_id)
    }
}

impl<LM, EM, IM, FP> ProviderWithSpeechModel for CustomProvider<LM, EM, IM, FP>
where
    LM: LanguageModel + Clone,
    EM: crate::embedding_model::EmbeddingModel + Clone,
    IM: ImageModel + Clone,
    FP: ProviderWithSpeechModel<LanguageModel = LM, EmbeddingModel = EM, ImageModel = IM>,
{
    type SpeechModel = FP::SpeechModel;

    fn speech_model(&self, model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError> {
        if let Some(target_model_id) = lookup_model_alias(&self.speech_model_aliases, model_id) {
            return self.fallback_provider.speech_model(target_model_id);
        }

        self.fallback_provider.speech_model(model_id)
    }
}

impl<LM, EM, IM, FP> ProviderWithRerankingModel for CustomProvider<LM, EM, IM, FP>
where
    LM: LanguageModel + Clone,
    EM: crate::embedding_model::EmbeddingModel + Clone,
    IM: ImageModel + Clone,
    FP: ProviderWithRerankingModel<LanguageModel = LM, EmbeddingModel = EM, ImageModel = IM>,
{
    type RerankingModel = FP::RerankingModel;

    fn reranking_model(&self, model_id: &str) -> Result<Self::RerankingModel, NoSuchModelError> {
        if let Some(target_model_id) = lookup_model_alias(&self.reranking_model_aliases, model_id) {
            return self.fallback_provider.reranking_model(target_model_id);
        }

        self.fallback_provider.reranking_model(model_id)
    }
}

impl<LM, EM, IM, FP> ProviderWithVideoModel for CustomProvider<LM, EM, IM, FP>
where
    LM: LanguageModel + Clone,
    EM: crate::embedding_model::EmbeddingModel + Clone,
    IM: ImageModel + Clone,
    FP: ProviderWithVideoModel<LanguageModel = LM, EmbeddingModel = EM, ImageModel = IM>,
{
    type VideoModel = FP::VideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        if let Some(target_model_id) = lookup_model_alias(&self.video_model_aliases, model_id) {
            return self.fallback_provider.video_model(target_model_id);
        }

        self.fallback_provider.video_model(model_id)
    }
}

impl<LM, EM, IM, FP> ProviderWithFiles for CustomProvider<LM, EM, IM, FP>
where
    LM: LanguageModel + Clone,
    EM: crate::embedding_model::EmbeddingModel + Clone,
    IM: ImageModel + Clone,
    FP: ProviderWithFiles<LanguageModel = LM, EmbeddingModel = EM, ImageModel = IM>,
{
    type Files = FP::Files;

    fn files(&self) -> Self::Files {
        self.fallback_provider.files()
    }
}

impl<LM, EM, IM, FP> ProviderWithSkills for CustomProvider<LM, EM, IM, FP>
where
    LM: LanguageModel + Clone,
    EM: crate::embedding_model::EmbeddingModel + Clone,
    IM: ImageModel + Clone,
    FP: ProviderWithSkills<LanguageModel = LM, EmbeddingModel = EM, ImageModel = IM>,
{
    type Skills = FP::Skills;

    fn skills(&self) -> Self::Skills {
        self.fallback_provider.skills()
    }
}

impl<P, TM> Provider for CustomProviderWithTranscriptionModel<P, TM>
where
    P: Provider,
    TM: TranscriptionModel + Clone,
{
    type LanguageModel = P::LanguageModel;
    type EmbeddingModel = P::EmbeddingModel;
    type ImageModel = P::ImageModel;

    fn specification_version(&self) -> crate::provider::SpecificationVersion {
        self.provider.specification_version()
    }

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        self.provider.language_model(model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        self.provider.embedding_model(model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        self.provider.image_model(model_id)
    }
}

impl<P, TM> ProviderWithTranscriptionModel for CustomProviderWithTranscriptionModel<P, TM>
where
    P: Provider,
    TM: TranscriptionModel + Clone,
{
    type TranscriptionModel = TM;

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
        self.transcription_models
            .iter()
            .find_map(|(id, model)| (id == model_id).then(|| model.clone()))
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::TranscriptionModel))
    }
}

impl<P, SM> Provider for CustomProviderWithSpeechModel<P, SM>
where
    P: Provider,
    SM: SpeechModel + Clone,
{
    type LanguageModel = P::LanguageModel;
    type EmbeddingModel = P::EmbeddingModel;
    type ImageModel = P::ImageModel;

    fn specification_version(&self) -> crate::provider::SpecificationVersion {
        self.provider.specification_version()
    }

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        self.provider.language_model(model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        self.provider.embedding_model(model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        self.provider.image_model(model_id)
    }
}

impl<P, SM> ProviderWithSpeechModel for CustomProviderWithSpeechModel<P, SM>
where
    P: Provider,
    SM: SpeechModel + Clone,
{
    type SpeechModel = SM;

    fn speech_model(&self, model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError> {
        self.speech_models
            .iter()
            .find_map(|(id, model)| (id == model_id).then(|| model.clone()))
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::SpeechModel))
    }
}

impl<P, RM> Provider for CustomProviderWithRerankingModel<P, RM>
where
    P: Provider,
    RM: RerankingModel + Clone,
{
    type LanguageModel = P::LanguageModel;
    type EmbeddingModel = P::EmbeddingModel;
    type ImageModel = P::ImageModel;

    fn specification_version(&self) -> crate::provider::SpecificationVersion {
        self.provider.specification_version()
    }

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        self.provider.language_model(model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        self.provider.embedding_model(model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        self.provider.image_model(model_id)
    }
}

impl<P, RM> ProviderWithRerankingModel for CustomProviderWithRerankingModel<P, RM>
where
    P: Provider,
    RM: RerankingModel + Clone,
{
    type RerankingModel = RM;

    fn reranking_model(&self, model_id: &str) -> Result<Self::RerankingModel, NoSuchModelError> {
        self.reranking_models
            .iter()
            .find_map(|(id, model)| (id == model_id).then(|| model.clone()))
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::RerankingModel))
    }
}

impl<P, VM> Provider for CustomProviderWithVideoModel<P, VM>
where
    P: Provider,
    VM: VideoModel + Clone,
{
    type LanguageModel = P::LanguageModel;
    type EmbeddingModel = P::EmbeddingModel;
    type ImageModel = P::ImageModel;

    fn specification_version(&self) -> crate::provider::SpecificationVersion {
        self.provider.specification_version()
    }

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        self.provider.language_model(model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        self.provider.embedding_model(model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        self.provider.image_model(model_id)
    }
}

impl<P, VM> ProviderWithVideoModel for CustomProviderWithVideoModel<P, VM>
where
    P: Provider,
    VM: VideoModel + Clone,
{
    type VideoModel = VM;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        self.video_models
            .iter()
            .find_map(|(id, model)| (id == model_id).then(|| model.clone()))
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::VideoModel))
    }
}

impl<P, F> Provider for CustomProviderWithFiles<P, F>
where
    P: Provider,
    F: Files + Clone,
{
    type LanguageModel = P::LanguageModel;
    type EmbeddingModel = P::EmbeddingModel;
    type ImageModel = P::ImageModel;

    fn specification_version(&self) -> crate::provider::SpecificationVersion {
        self.provider.specification_version()
    }

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        self.provider.language_model(model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        self.provider.embedding_model(model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        self.provider.image_model(model_id)
    }
}

impl<P, F> ProviderWithFiles for CustomProviderWithFiles<P, F>
where
    P: Provider,
    F: Files + Clone,
{
    type Files = F;

    fn files(&self) -> Self::Files {
        self.files.clone()
    }
}

impl<P, F, S> ProviderWithFiles for CustomProviderWithSkills<CustomProviderWithFiles<P, F>, S>
where
    P: Provider,
    F: Files + Clone,
    S: Skills + Clone,
{
    type Files = F;

    fn files(&self) -> Self::Files {
        self.provider.files()
    }
}

impl<P, S> Provider for CustomProviderWithSkills<P, S>
where
    P: Provider,
    S: Skills + Clone,
{
    type LanguageModel = P::LanguageModel;
    type EmbeddingModel = P::EmbeddingModel;
    type ImageModel = P::ImageModel;

    fn specification_version(&self) -> crate::provider::SpecificationVersion {
        self.provider.specification_version()
    }

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        self.provider.language_model(model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        self.provider.embedding_model(model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        self.provider.image_model(model_id)
    }
}

impl<P, S> ProviderWithSkills for CustomProviderWithSkills<P, S>
where
    P: Provider,
    S: Skills + Clone,
{
    type Skills = S;

    fn skills(&self) -> Self::Skills {
        self.skills.clone()
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

/// Deprecated alias for [`create_provider_registry`].
pub fn experimental_create_provider_registry<I, K, P>(providers: I) -> ProviderRegistry<P>
where
    I: IntoIterator<Item = (K, P)>,
    K: Into<String>,
{
    create_provider_registry(providers)
}

/// Deprecated alias for [`create_provider_registry_with_options`].
pub fn experimental_create_provider_registry_with_options<I, K, P>(
    providers: I,
    options: ProviderRegistryOptions,
) -> ProviderRegistry<P>
where
    I: IntoIterator<Item = (K, P)>,
    K: Into<String>,
{
    create_provider_registry_with_options(providers, options)
}

/// Creates an empty custom provider with direct v4 model maps.
pub fn custom_provider<LM, EM, IM>() -> CustomProvider<LM, EM, IM> {
    CustomProvider::new()
}

fn lookup_model_alias<'a>(aliases: &'a [(String, String)], model_id: &str) -> Option<&'a str> {
    aliases
        .iter()
        .find_map(|(id, target)| (id == model_id).then_some(target.as_str()))
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
        create_provider_registry_with_options, custom_provider,
        experimental_create_provider_registry, experimental_create_provider_registry_with_options,
        split_registry_model_id,
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
    fn custom_provider_language_model_should_use_fallback_provider_if_model_not_found_and_fallback_exists()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_fallback_provider(StaticProvider { id: "fallback" });

        assert_eq!(
            provider.language_model("test-model"),
            Ok(StaticLanguageModel {
                provider: "fallback".to_string(),
                model_id: "test-model".to_string(),
            })
        );
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
    fn custom_provider_embedding_model_should_use_fallback_provider_if_model_not_found_and_fallback_exists()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_fallback_provider(StaticProvider { id: "fallback" });

        assert_eq!(
            provider.embedding_model("test-model"),
            Ok(StaticEmbeddingModel {
                provider: "fallback".to_string(),
                model_id: "test-model".to_string(),
            })
        );
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

    #[test]
    fn custom_provider_image_model_should_use_fallback_provider_if_model_not_found_and_fallback_exists()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_fallback_provider(StaticProvider { id: "fallback" });

        assert_eq!(
            provider.image_model("test-model"),
            Ok(StaticImageModel {
                provider: "fallback".to_string(),
                model_id: "test-model".to_string(),
            })
        );
    }

    #[test]
    fn custom_provider_should_resolve_string_model_ids_through_the_explicit_default_provider() {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_language_model_alias("alias", "language")
                .with_embedding_model_alias("alias", "embedding")
                .with_image_model_alias("alias", "image")
                .with_transcription_model_alias("alias", "transcription")
                .with_speech_model_alias("alias", "speech")
                .with_reranking_model_alias("alias", "reranking")
                .with_video_model_alias("alias", "video")
                .with_fallback_provider(StaticProvider {
                    id: "default-provider",
                });

        assert_eq!(
            provider.language_model("alias"),
            Ok(StaticLanguageModel {
                provider: "default-provider".to_string(),
                model_id: "language".to_string(),
            })
        );
        assert_eq!(
            provider.embedding_model("alias"),
            Ok(StaticEmbeddingModel {
                provider: "default-provider".to_string(),
                model_id: "embedding".to_string(),
            })
        );
        assert_eq!(
            provider.image_model("alias"),
            Ok(StaticImageModel {
                provider: "default-provider".to_string(),
                model_id: "image".to_string(),
            })
        );
        assert_eq!(
            provider.transcription_model("alias"),
            Ok(StaticTranscriptionModel {
                provider: "default-provider".to_string(),
                model_id: "transcription".to_string(),
            })
        );
        assert_eq!(
            provider.speech_model("alias"),
            Ok(StaticSpeechModel {
                provider: "default-provider".to_string(),
                model_id: "speech".to_string(),
            })
        );
        assert_eq!(
            provider.reranking_model("alias"),
            Ok(StaticRerankingModel {
                provider: "default-provider".to_string(),
                model_id: "reranking".to_string(),
            })
        );
        assert_eq!(
            provider.video_model("alias"),
            Ok(StaticVideoModel {
                provider: "default-provider".to_string(),
                model_id: "video".to_string(),
            })
        );
    }

    #[test]
    fn custom_provider_files_should_return_the_files_interface_if_it_exists() {
        let files = StaticFiles {
            provider: "mock-provider".to_string(),
        };
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_files(files.clone());

        assert_eq!(ProviderWithFiles::files(&provider), files);
    }

    #[test]
    fn custom_provider_skills_should_return_the_skills_interface_if_it_exists() {
        let skills = StaticSkills {
            provider: "mock-provider".to_string(),
        };
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_skills(skills.clone());

        assert_eq!(ProviderWithSkills::skills(&provider), skills);
    }

    #[test]
    fn custom_provider_files_and_skills_should_expose_both_interfaces_when_both_exist() {
        let files = StaticFiles {
            provider: "mock-provider".to_string(),
        };
        let skills = StaticSkills {
            provider: "mock-provider".to_string(),
        };
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_files(files.clone())
                .with_skills(skills.clone());

        assert_eq!(ProviderWithFiles::files(&provider), files);
        assert_eq!(ProviderWithSkills::skills(&provider), skills);
    }

    #[test]
    fn custom_provider_files_should_use_fallback_provider_files_if_files_is_not_configured_and_fallback_exists()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_fallback_provider(StaticProvider { id: "fallback" });

        assert_eq!(
            ProviderWithFiles::files(&provider),
            StaticFiles {
                provider: "fallback".to_string(),
            }
        );
    }

    #[test]
    fn custom_provider_skills_should_use_fallback_provider_skills_if_skills_is_not_configured_and_fallback_exists()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_fallback_provider(StaticProvider { id: "fallback" });

        assert_eq!(
            ProviderWithSkills::skills(&provider),
            StaticSkills {
                provider: "fallback".to_string(),
            }
        );
    }

    #[test]
    fn custom_provider_transcription_model_should_return_the_transcription_model_if_it_exists() {
        let model = StaticTranscriptionModel {
            provider: "mock-provider".to_string(),
            model_id: "actual-transcription-model".to_string(),
        };
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_transcription_model("test-model", model.clone());

        assert_eq!(provider.transcription_model("test-model"), Ok(model));
    }

    #[test]
    fn custom_provider_transcription_model_should_throw_no_such_model_error_if_model_not_found_and_no_fallback()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_transcription_model(
                    "other-model",
                    StaticTranscriptionModel {
                        provider: "mock-provider".to_string(),
                        model_id: "actual-transcription-model".to_string(),
                    },
                );
        let error = provider
            .transcription_model("test-model")
            .expect_err("missing model should error");

        assert_eq!(error.model_id(), "test-model");
        assert_eq!(error.model_type(), ModelType::TranscriptionModel);
    }

    #[test]
    fn custom_provider_transcription_model_should_use_fallback_provider_if_model_not_found_and_fallback_exists()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_fallback_provider(StaticProvider { id: "fallback" });

        assert_eq!(
            provider.transcription_model("test-model"),
            Ok(StaticTranscriptionModel {
                provider: "fallback".to_string(),
                model_id: "test-model".to_string(),
            })
        );
    }

    #[test]
    fn custom_provider_speech_model_should_return_the_speech_model_if_it_exists() {
        let model = StaticSpeechModel {
            provider: "mock-provider".to_string(),
            model_id: "actual-speech-model".to_string(),
        };
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_speech_model("test-model", model.clone());

        assert_eq!(provider.speech_model("test-model"), Ok(model));
    }

    #[test]
    fn custom_provider_speech_model_should_throw_no_such_model_error_if_model_not_found_and_no_fallback()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_speech_model(
                    "other-model",
                    StaticSpeechModel {
                        provider: "mock-provider".to_string(),
                        model_id: "actual-speech-model".to_string(),
                    },
                );
        let error = provider
            .speech_model("test-model")
            .expect_err("missing model should error");

        assert_eq!(error.model_id(), "test-model");
        assert_eq!(error.model_type(), ModelType::SpeechModel);
    }

    #[test]
    fn custom_provider_speech_model_should_use_fallback_provider_if_model_not_found_and_fallback_exists()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_fallback_provider(StaticProvider { id: "fallback" });

        assert_eq!(
            provider.speech_model("test-model"),
            Ok(StaticSpeechModel {
                provider: "fallback".to_string(),
                model_id: "test-model".to_string(),
            })
        );
    }

    #[test]
    fn custom_provider_reranking_model_should_return_the_reranking_model_if_it_exists() {
        let model = StaticRerankingModel {
            provider: "mock-provider".to_string(),
            model_id: "actual-reranking-model".to_string(),
        };
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_reranking_model("test-model", model.clone());

        assert_eq!(provider.reranking_model("test-model"), Ok(model));
    }

    #[test]
    fn custom_provider_reranking_model_should_throw_no_such_model_error_if_model_not_found_and_no_fallback()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_reranking_model(
                    "other-model",
                    StaticRerankingModel {
                        provider: "mock-provider".to_string(),
                        model_id: "actual-reranking-model".to_string(),
                    },
                );
        let error = provider
            .reranking_model("test-model")
            .expect_err("missing model should error");

        assert_eq!(error.model_id(), "test-model");
        assert_eq!(error.model_type(), ModelType::RerankingModel);
    }

    #[test]
    fn custom_provider_reranking_model_should_use_fallback_provider_if_model_not_found_and_fallback_exists()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_fallback_provider(StaticProvider { id: "fallback" });

        assert_eq!(
            provider.reranking_model("test-model"),
            Ok(StaticRerankingModel {
                provider: "fallback".to_string(),
                model_id: "test-model".to_string(),
            })
        );
    }

    #[test]
    fn custom_provider_video_model_should_return_the_video_model_if_it_exists() {
        let model = StaticVideoModel {
            provider: "mock-provider".to_string(),
            model_id: "actual-video-model".to_string(),
        };
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_video_model("test-model", model.clone());

        assert_eq!(provider.video_model("test-model"), Ok(model));
    }

    #[test]
    fn custom_provider_video_model_should_throw_no_such_model_error_if_model_not_found_and_no_fallback()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_video_model(
                    "other-model",
                    StaticVideoModel {
                        provider: "mock-provider".to_string(),
                        model_id: "actual-video-model".to_string(),
                    },
                );
        let error = provider
            .video_model("test-model")
            .expect_err("missing model should error");

        assert_eq!(error.model_id(), "test-model");
        assert_eq!(error.model_type(), ModelType::VideoModel);
    }

    #[test]
    fn custom_provider_video_model_should_use_fallback_provider_if_model_not_found_and_fallback_exists()
     {
        let provider =
            custom_provider::<StaticLanguageModel, StaticEmbeddingModel, StaticImageModel>()
                .with_fallback_provider(StaticProvider { id: "fallback" });

        assert_eq!(
            provider.video_model("test-model"),
            Ok(StaticVideoModel {
                provider: "fallback".to_string(),
                model_id: "test-model".to_string(),
            })
        );
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
    fn experimental_create_provider_registry_is_a_deprecated_alias() {
        let registry = experimental_create_provider_registry([(
            "provider",
            StaticProvider { id: "provider" },
        )]);

        let model = registry
            .language_model("provider:chat")
            .expect("language model resolves");

        assert_eq!(registry.options().separator(), ":");
        assert_eq!(model.provider(), "provider");
        assert_eq!(model.model_id(), "chat");
    }

    #[test]
    fn experimental_create_provider_registry_with_options_is_a_deprecated_alias() {
        let registry = experimental_create_provider_registry_with_options(
            [("provider", StaticProvider { id: "provider" })],
            ProviderRegistryOptions::new().with_separator(" > "),
        );

        let model = registry
            .language_model("provider > chat")
            .expect("language model resolves");

        assert_eq!(registry.options().separator(), " > ");
        assert_eq!(model.provider(), "provider");
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
