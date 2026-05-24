use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::future::{Ready, ready};
use std::rc::Rc;

use crate::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResult};
use crate::image_model::{ImageModel, ImageModelCallOptions, ImageModelResult};
use crate::language_model::{
    LanguageModel, LanguageModelCallOptions, LanguageModelGenerateResult, LanguageModelStreamPart,
    LanguageModelStreamResult, LanguageModelSupportedUrls,
};
use crate::provider::{
    ModelType, NoSuchModelError, Provider, ProviderWithRerankingModel, ProviderWithSpeechModel,
    ProviderWithTranscriptionModel, ProviderWithVideoModel,
};
use crate::reranking_model::{RerankingModel, RerankingModelCallOptions, RerankingModelResult};
use crate::speech_model::{SpeechModel, SpeechModelCallOptions, SpeechModelResult};
use crate::transcription_model::{
    TranscriptionModel, TranscriptionModelCallOptions, TranscriptionModelResult,
};
use crate::video_model::{VideoModel, VideoModelCallOptions, VideoModelResult};

const DEFAULT_PROVIDER: &str = "mock-provider";
const DEFAULT_MODEL_ID: &str = "mock-model-id";

#[derive(Clone, Debug)]
struct ScriptedCalls<C, R> {
    calls: Vec<C>,
    results: VecDeque<R>,
}

impl<C, R> Default for ScriptedCalls<C, R> {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            results: VecDeque::new(),
        }
    }
}

fn next_scripted_result<C, R: Clone>(
    script: &Rc<RefCell<ScriptedCalls<C, R>>>,
    call: C,
    label: &str,
) -> R {
    let mut script = script.borrow_mut();
    script.calls.push(call);
    script
        .results
        .pop_front()
        .unwrap_or_else(|| panic!("{label} called without a scripted result"))
}

fn recorded_calls<C: Clone, R>(script: &Rc<RefCell<ScriptedCalls<C, R>>>) -> Vec<C> {
    script.borrow().calls.clone()
}

fn clear_recorded_calls<C, R>(script: &Rc<RefCell<ScriptedCalls<C, R>>>) {
    script.borrow_mut().calls.clear();
}

/// Scriptable provider-v4 language model for deterministic tests and examples.
#[derive(Clone, Debug)]
pub struct MockLanguageModel {
    provider: String,
    model_id: String,
    supported_urls: LanguageModelSupportedUrls,
    supported_urls_calls: Rc<RefCell<usize>>,
    generate_script:
        Rc<RefCell<ScriptedCalls<LanguageModelCallOptions, LanguageModelGenerateResult>>>,
    stream_script: Rc<
        RefCell<
            ScriptedCalls<
                LanguageModelCallOptions,
                LanguageModelStreamResult<Vec<LanguageModelStreamPart>>,
            >,
        >,
    >,
}

impl MockLanguageModel {
    /// Creates a mock language model with upstream default identity values.
    pub fn new() -> Self {
        Self {
            provider: DEFAULT_PROVIDER.to_string(),
            model_id: DEFAULT_MODEL_ID.to_string(),
            supported_urls: LanguageModelSupportedUrls::new(),
            supported_urls_calls: Rc::new(RefCell::new(0)),
            generate_script: Rc::new(RefCell::new(ScriptedCalls::default())),
            stream_script: Rc::new(RefCell::new(ScriptedCalls::default())),
        }
    }

    /// Sets the provider identifier.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    /// Sets the provider-specific model id.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Sets supported URL patterns grouped by media type.
    pub fn with_supported_urls(mut self, supported_urls: LanguageModelSupportedUrls) -> Self {
        self.supported_urls = supported_urls;
        self
    }

    /// Appends one non-streaming generation result to the script.
    pub fn with_generate_result(self, result: LanguageModelGenerateResult) -> Self {
        self.push_generate_result(result);
        self
    }

    /// Replaces non-streaming generation results with the supplied script.
    pub fn with_generate_results<I>(self, results: I) -> Self
    where
        I: IntoIterator<Item = LanguageModelGenerateResult>,
    {
        self.generate_script.borrow_mut().results = results.into_iter().collect();
        self
    }

    /// Appends one non-streaming generation result to the script.
    pub fn push_generate_result(&self, result: LanguageModelGenerateResult) {
        self.generate_script.borrow_mut().results.push_back(result);
    }

    /// Appends one streaming result to the script.
    pub fn with_stream_result(
        self,
        result: LanguageModelStreamResult<Vec<LanguageModelStreamPart>>,
    ) -> Self {
        self.push_stream_result(result);
        self
    }

    /// Replaces streaming results with the supplied script.
    pub fn with_stream_results<I>(self, results: I) -> Self
    where
        I: IntoIterator<Item = LanguageModelStreamResult<Vec<LanguageModelStreamPart>>>,
    {
        self.stream_script.borrow_mut().results = results.into_iter().collect();
        self
    }

    /// Appends one streaming result to the script.
    pub fn push_stream_result(
        &self,
        result: LanguageModelStreamResult<Vec<LanguageModelStreamPart>>,
    ) {
        self.stream_script.borrow_mut().results.push_back(result);
    }

    /// Returns recorded non-streaming generation calls.
    pub fn generate_calls(&self) -> Vec<LanguageModelCallOptions> {
        recorded_calls(&self.generate_script)
    }

    /// Returns recorded streaming generation calls.
    pub fn stream_calls(&self) -> Vec<LanguageModelCallOptions> {
        recorded_calls(&self.stream_script)
    }

    /// Returns how many times supported URL patterns were requested.
    pub fn supported_urls_calls(&self) -> usize {
        *self.supported_urls_calls.borrow()
    }

    /// Clears recorded calls without changing scripted results.
    pub fn clear_calls(&self) {
        clear_recorded_calls(&self.generate_script);
        clear_recorded_calls(&self.stream_script);
        *self.supported_urls_calls.borrow_mut() = 0;
    }
}

impl Default for MockLanguageModel {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageModel for MockLanguageModel {
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
        *self.supported_urls_calls.borrow_mut() += 1;
        ready(self.supported_urls.clone())
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        ready(next_scripted_result(
            &self.generate_script,
            options,
            "MockLanguageModel::do_generate",
        ))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        ready(next_scripted_result(
            &self.stream_script,
            options,
            "MockLanguageModel::do_stream",
        ))
    }
}

/// Scriptable provider-v4 embedding model for deterministic tests and examples.
#[derive(Clone, Debug)]
pub struct MockEmbeddingModel {
    provider: String,
    model_id: String,
    max_embeddings_per_call: Option<usize>,
    supports_parallel_calls: bool,
    embed_script: Rc<RefCell<ScriptedCalls<EmbeddingModelCallOptions, EmbeddingModelResult>>>,
}

impl MockEmbeddingModel {
    /// Creates a mock embedding model with upstream default identity values.
    pub fn new() -> Self {
        Self {
            provider: DEFAULT_PROVIDER.to_string(),
            model_id: DEFAULT_MODEL_ID.to_string(),
            max_embeddings_per_call: Some(1),
            supports_parallel_calls: false,
            embed_script: Rc::new(RefCell::new(ScriptedCalls::default())),
        }
    }

    /// Sets the provider identifier.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    /// Sets the provider-specific model id.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Sets the maximum number of embeddings supported in one call.
    pub fn with_max_embeddings_per_call(mut self, max_embeddings_per_call: usize) -> Self {
        self.max_embeddings_per_call = Some(max_embeddings_per_call);
        self
    }

    /// Removes the model-specific max-embeddings limit.
    pub fn without_max_embeddings_per_call(mut self) -> Self {
        self.max_embeddings_per_call = None;
        self
    }

    /// Sets whether the model supports parallel embedding calls.
    pub fn with_supports_parallel_calls(mut self, supports_parallel_calls: bool) -> Self {
        self.supports_parallel_calls = supports_parallel_calls;
        self
    }

    /// Appends one embedding result to the script.
    pub fn with_embed_result(self, result: EmbeddingModelResult) -> Self {
        self.push_embed_result(result);
        self
    }

    /// Replaces embedding results with the supplied script.
    pub fn with_embed_results<I>(self, results: I) -> Self
    where
        I: IntoIterator<Item = EmbeddingModelResult>,
    {
        self.embed_script.borrow_mut().results = results.into_iter().collect();
        self
    }

    /// Appends one embedding result to the script.
    pub fn push_embed_result(&self, result: EmbeddingModelResult) {
        self.embed_script.borrow_mut().results.push_back(result);
    }

    /// Returns recorded embedding calls.
    pub fn embed_calls(&self) -> Vec<EmbeddingModelCallOptions> {
        recorded_calls(&self.embed_script)
    }

    /// Clears recorded calls without changing scripted results.
    pub fn clear_calls(&self) {
        clear_recorded_calls(&self.embed_script);
    }
}

impl Default for MockEmbeddingModel {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbeddingModel for MockEmbeddingModel {
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
        ready(self.max_embeddings_per_call)
    }

    fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
        ready(self.supports_parallel_calls)
    }

    fn do_embed(&self, options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
        ready(next_scripted_result(
            &self.embed_script,
            options,
            "MockEmbeddingModel::do_embed",
        ))
    }
}

/// Scriptable provider-v4 image model for deterministic tests and examples.
#[derive(Clone, Debug)]
pub struct MockImageModel {
    provider: String,
    model_id: String,
    max_images_per_call: Option<usize>,
    generate_script: Rc<RefCell<ScriptedCalls<ImageModelCallOptions, ImageModelResult>>>,
}

impl MockImageModel {
    /// Creates a mock image model with upstream default identity values.
    pub fn new() -> Self {
        Self {
            provider: DEFAULT_PROVIDER.to_string(),
            model_id: DEFAULT_MODEL_ID.to_string(),
            max_images_per_call: Some(1),
            generate_script: Rc::new(RefCell::new(ScriptedCalls::default())),
        }
    }

    /// Sets the provider identifier.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    /// Sets the provider-specific model id.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Sets the maximum number of images supported in one call.
    pub fn with_max_images_per_call(mut self, max_images_per_call: usize) -> Self {
        self.max_images_per_call = Some(max_images_per_call);
        self
    }

    /// Removes the model-specific max-images limit.
    pub fn without_max_images_per_call(mut self) -> Self {
        self.max_images_per_call = None;
        self
    }

    /// Appends one image generation result to the script.
    pub fn with_generate_result(self, result: ImageModelResult) -> Self {
        self.push_generate_result(result);
        self
    }

    /// Replaces image generation results with the supplied script.
    pub fn with_generate_results<I>(self, results: I) -> Self
    where
        I: IntoIterator<Item = ImageModelResult>,
    {
        self.generate_script.borrow_mut().results = results.into_iter().collect();
        self
    }

    /// Appends one image generation result to the script.
    pub fn push_generate_result(&self, result: ImageModelResult) {
        self.generate_script.borrow_mut().results.push_back(result);
    }

    /// Returns recorded image generation calls.
    pub fn generate_calls(&self) -> Vec<ImageModelCallOptions> {
        recorded_calls(&self.generate_script)
    }

    /// Clears recorded calls without changing scripted results.
    pub fn clear_calls(&self) {
        clear_recorded_calls(&self.generate_script);
    }
}

impl Default for MockImageModel {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageModel for MockImageModel {
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
        ready(self.max_images_per_call)
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        ready(next_scripted_result(
            &self.generate_script,
            options,
            "MockImageModel::do_generate",
        ))
    }
}

/// Scriptable provider-v4 speech model for deterministic tests and examples.
#[derive(Clone, Debug)]
pub struct MockSpeechModel {
    provider: String,
    model_id: String,
    generate_script: Rc<RefCell<ScriptedCalls<SpeechModelCallOptions, SpeechModelResult>>>,
}

impl MockSpeechModel {
    /// Creates a mock speech model with upstream default identity values.
    pub fn new() -> Self {
        Self {
            provider: DEFAULT_PROVIDER.to_string(),
            model_id: DEFAULT_MODEL_ID.to_string(),
            generate_script: Rc::new(RefCell::new(ScriptedCalls::default())),
        }
    }

    /// Sets the provider identifier.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    /// Sets the provider-specific model id.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Appends one speech generation result to the script.
    pub fn with_generate_result(self, result: SpeechModelResult) -> Self {
        self.push_generate_result(result);
        self
    }

    /// Replaces speech generation results with the supplied script.
    pub fn with_generate_results<I>(self, results: I) -> Self
    where
        I: IntoIterator<Item = SpeechModelResult>,
    {
        self.generate_script.borrow_mut().results = results.into_iter().collect();
        self
    }

    /// Appends one speech generation result to the script.
    pub fn push_generate_result(&self, result: SpeechModelResult) {
        self.generate_script.borrow_mut().results.push_back(result);
    }

    /// Returns recorded speech generation calls.
    pub fn generate_calls(&self) -> Vec<SpeechModelCallOptions> {
        recorded_calls(&self.generate_script)
    }

    /// Clears recorded calls without changing scripted results.
    pub fn clear_calls(&self) {
        clear_recorded_calls(&self.generate_script);
    }
}

impl Default for MockSpeechModel {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeechModel for MockSpeechModel {
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

    fn do_generate(&self, options: SpeechModelCallOptions) -> Self::GenerateFuture<'_> {
        ready(next_scripted_result(
            &self.generate_script,
            options,
            "MockSpeechModel::do_generate",
        ))
    }
}

/// Scriptable provider-v4 transcription model for deterministic tests and examples.
#[derive(Clone, Debug)]
pub struct MockTranscriptionModel {
    provider: String,
    model_id: String,
    generate_script:
        Rc<RefCell<ScriptedCalls<TranscriptionModelCallOptions, TranscriptionModelResult>>>,
}

impl MockTranscriptionModel {
    /// Creates a mock transcription model with upstream default identity values.
    pub fn new() -> Self {
        Self {
            provider: DEFAULT_PROVIDER.to_string(),
            model_id: DEFAULT_MODEL_ID.to_string(),
            generate_script: Rc::new(RefCell::new(ScriptedCalls::default())),
        }
    }

    /// Sets the provider identifier.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    /// Sets the provider-specific model id.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Appends one transcription result to the script.
    pub fn with_generate_result(self, result: TranscriptionModelResult) -> Self {
        self.push_generate_result(result);
        self
    }

    /// Replaces transcription results with the supplied script.
    pub fn with_generate_results<I>(self, results: I) -> Self
    where
        I: IntoIterator<Item = TranscriptionModelResult>,
    {
        self.generate_script.borrow_mut().results = results.into_iter().collect();
        self
    }

    /// Appends one transcription result to the script.
    pub fn push_generate_result(&self, result: TranscriptionModelResult) {
        self.generate_script.borrow_mut().results.push_back(result);
    }

    /// Returns recorded transcription calls.
    pub fn generate_calls(&self) -> Vec<TranscriptionModelCallOptions> {
        recorded_calls(&self.generate_script)
    }

    /// Clears recorded calls without changing scripted results.
    pub fn clear_calls(&self) {
        clear_recorded_calls(&self.generate_script);
    }
}

impl Default for MockTranscriptionModel {
    fn default() -> Self {
        Self::new()
    }
}

impl TranscriptionModel for MockTranscriptionModel {
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

    fn do_generate(&self, options: TranscriptionModelCallOptions) -> Self::GenerateFuture<'_> {
        ready(next_scripted_result(
            &self.generate_script,
            options,
            "MockTranscriptionModel::do_generate",
        ))
    }
}

/// Scriptable provider-v4 reranking model for deterministic tests and examples.
#[derive(Clone, Debug)]
pub struct MockRerankingModel {
    provider: String,
    model_id: String,
    rerank_script: Rc<RefCell<ScriptedCalls<RerankingModelCallOptions, RerankingModelResult>>>,
}

impl MockRerankingModel {
    /// Creates a mock reranking model with upstream default identity values.
    pub fn new() -> Self {
        Self {
            provider: DEFAULT_PROVIDER.to_string(),
            model_id: DEFAULT_MODEL_ID.to_string(),
            rerank_script: Rc::new(RefCell::new(ScriptedCalls::default())),
        }
    }

    /// Sets the provider identifier.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    /// Sets the provider-specific model id.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Appends one reranking result to the script.
    pub fn with_rerank_result(self, result: RerankingModelResult) -> Self {
        self.push_rerank_result(result);
        self
    }

    /// Replaces reranking results with the supplied script.
    pub fn with_rerank_results<I>(self, results: I) -> Self
    where
        I: IntoIterator<Item = RerankingModelResult>,
    {
        self.rerank_script.borrow_mut().results = results.into_iter().collect();
        self
    }

    /// Appends one reranking result to the script.
    pub fn push_rerank_result(&self, result: RerankingModelResult) {
        self.rerank_script.borrow_mut().results.push_back(result);
    }

    /// Returns recorded reranking calls.
    pub fn rerank_calls(&self) -> Vec<RerankingModelCallOptions> {
        recorded_calls(&self.rerank_script)
    }

    /// Clears recorded calls without changing scripted results.
    pub fn clear_calls(&self) {
        clear_recorded_calls(&self.rerank_script);
    }
}

impl Default for MockRerankingModel {
    fn default() -> Self {
        Self::new()
    }
}

impl RerankingModel for MockRerankingModel {
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

    fn do_rerank(&self, options: RerankingModelCallOptions) -> Self::RerankFuture<'_> {
        ready(next_scripted_result(
            &self.rerank_script,
            options,
            "MockRerankingModel::do_rerank",
        ))
    }
}

/// Scriptable provider-v4 video model for deterministic tests and examples.
#[derive(Clone, Debug)]
pub struct MockVideoModel {
    provider: String,
    model_id: String,
    max_videos_per_call: Option<usize>,
    generate_script: Rc<RefCell<ScriptedCalls<VideoModelCallOptions, VideoModelResult>>>,
}

impl MockVideoModel {
    /// Creates a mock video model with upstream default identity values.
    pub fn new() -> Self {
        Self {
            provider: DEFAULT_PROVIDER.to_string(),
            model_id: DEFAULT_MODEL_ID.to_string(),
            max_videos_per_call: Some(1),
            generate_script: Rc::new(RefCell::new(ScriptedCalls::default())),
        }
    }

    /// Sets the provider identifier.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    /// Sets the provider-specific model id.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Sets the maximum number of videos supported in one call.
    pub fn with_max_videos_per_call(mut self, max_videos_per_call: usize) -> Self {
        self.max_videos_per_call = Some(max_videos_per_call);
        self
    }

    /// Removes the model-specific max-videos limit.
    pub fn without_max_videos_per_call(mut self) -> Self {
        self.max_videos_per_call = None;
        self
    }

    /// Appends one video generation result to the script.
    pub fn with_generate_result(self, result: VideoModelResult) -> Self {
        self.push_generate_result(result);
        self
    }

    /// Replaces video generation results with the supplied script.
    pub fn with_generate_results<I>(self, results: I) -> Self
    where
        I: IntoIterator<Item = VideoModelResult>,
    {
        self.generate_script.borrow_mut().results = results.into_iter().collect();
        self
    }

    /// Appends one video generation result to the script.
    pub fn push_generate_result(&self, result: VideoModelResult) {
        self.generate_script.borrow_mut().results.push_back(result);
    }

    /// Returns recorded video generation calls.
    pub fn generate_calls(&self) -> Vec<VideoModelCallOptions> {
        recorded_calls(&self.generate_script)
    }

    /// Clears recorded calls without changing scripted results.
    pub fn clear_calls(&self) {
        clear_recorded_calls(&self.generate_script);
    }
}

impl Default for MockVideoModel {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoModel for MockVideoModel {
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
        ready(self.max_videos_per_call)
    }

    fn do_generate(&self, options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
        ready(next_scripted_result(
            &self.generate_script,
            options,
            "MockVideoModel::do_generate",
        ))
    }
}

/// Provider-v4 implementation backed by named mock models.
#[derive(Clone, Debug, Default)]
pub struct MockProvider {
    language_models: BTreeMap<String, MockLanguageModel>,
    embedding_models: BTreeMap<String, MockEmbeddingModel>,
    image_models: BTreeMap<String, MockImageModel>,
    transcription_models: BTreeMap<String, MockTranscriptionModel>,
    speech_models: BTreeMap<String, MockSpeechModel>,
    reranking_models: BTreeMap<String, MockRerankingModel>,
    video_models: BTreeMap<String, MockVideoModel>,
}

impl MockProvider {
    /// Creates an empty mock provider.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a language model by provider model id.
    pub fn with_language_model(
        mut self,
        model_id: impl Into<String>,
        model: MockLanguageModel,
    ) -> Self {
        self.language_models.insert(model_id.into(), model);
        self
    }

    /// Registers an embedding model by provider model id.
    pub fn with_embedding_model(
        mut self,
        model_id: impl Into<String>,
        model: MockEmbeddingModel,
    ) -> Self {
        self.embedding_models.insert(model_id.into(), model);
        self
    }

    /// Registers an image model by provider model id.
    pub fn with_image_model(mut self, model_id: impl Into<String>, model: MockImageModel) -> Self {
        self.image_models.insert(model_id.into(), model);
        self
    }

    /// Registers a transcription model by provider model id.
    pub fn with_transcription_model(
        mut self,
        model_id: impl Into<String>,
        model: MockTranscriptionModel,
    ) -> Self {
        self.transcription_models.insert(model_id.into(), model);
        self
    }

    /// Registers a speech model by provider model id.
    pub fn with_speech_model(
        mut self,
        model_id: impl Into<String>,
        model: MockSpeechModel,
    ) -> Self {
        self.speech_models.insert(model_id.into(), model);
        self
    }

    /// Registers a reranking model by provider model id.
    pub fn with_reranking_model(
        mut self,
        model_id: impl Into<String>,
        model: MockRerankingModel,
    ) -> Self {
        self.reranking_models.insert(model_id.into(), model);
        self
    }

    /// Registers a video model by provider model id.
    pub fn with_video_model(mut self, model_id: impl Into<String>, model: MockVideoModel) -> Self {
        self.video_models.insert(model_id.into(), model);
        self
    }
}

impl Provider for MockProvider {
    type LanguageModel = MockLanguageModel;
    type EmbeddingModel = MockEmbeddingModel;
    type ImageModel = MockImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        self.language_models
            .get(model_id)
            .cloned()
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::LanguageModel))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        self.embedding_models
            .get(model_id)
            .cloned()
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        self.image_models
            .get(model_id)
            .cloned()
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::ImageModel))
    }
}

impl ProviderWithTranscriptionModel for MockProvider {
    type TranscriptionModel = MockTranscriptionModel;

    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<Self::TranscriptionModel, NoSuchModelError> {
        self.transcription_models
            .get(model_id)
            .cloned()
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::TranscriptionModel))
    }
}

impl ProviderWithSpeechModel for MockProvider {
    type SpeechModel = MockSpeechModel;

    fn speech_model(&self, model_id: &str) -> Result<Self::SpeechModel, NoSuchModelError> {
        self.speech_models
            .get(model_id)
            .cloned()
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::SpeechModel))
    }
}

impl ProviderWithRerankingModel for MockProvider {
    type RerankingModel = MockRerankingModel;

    fn reranking_model(&self, model_id: &str) -> Result<Self::RerankingModel, NoSuchModelError> {
        self.reranking_models
            .get(model_id)
            .cloned()
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::RerankingModel))
    }
}

impl ProviderWithVideoModel for MockProvider {
    type VideoModel = MockVideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        self.video_models
            .get(model_id)
            .cloned()
            .ok_or_else(|| NoSuchModelError::new(model_id, ModelType::VideoModel))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    use serde_json::json;

    use super::{
        MockEmbeddingModel, MockImageModel, MockLanguageModel, MockProvider, MockRerankingModel,
        MockSpeechModel, MockTranscriptionModel, MockVideoModel,
    };
    use crate::embedding_model::{
        EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResult, EmbeddingModelUsage,
    };
    use crate::file_data::FileDataContent;
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::image_model::{
        ImageModel, ImageModelCallOptions, ImageModelResponse, ImageModelResult,
    };
    use crate::language_model::{
        FinishReason, InputTokenUsage, LanguageModel, LanguageModelCallOptions,
        LanguageModelContent, LanguageModelFinishReason, LanguageModelGenerateResult,
        LanguageModelStreamPart, LanguageModelStreamResult, LanguageModelStreamStart,
        LanguageModelSupportedUrls, LanguageModelText, LanguageModelTextDelta,
        LanguageModelTextEnd, LanguageModelTextStart, LanguageModelUsage, OutputTokenUsage,
    };
    use crate::prompt::Prompt;
    use crate::provider::{
        ModelType, Provider, ProviderWithRerankingModel, ProviderWithSpeechModel,
        ProviderWithTranscriptionModel, ProviderWithVideoModel,
    };
    use crate::reranking_model::{
        RerankingModel, RerankingModelCallOptions, RerankingModelDocuments, RerankingModelRanking,
        RerankingModelResult,
    };
    use crate::speech_model::{
        SpeechModel, SpeechModelCallOptions, SpeechModelResponse, SpeechModelResult,
    };
    use crate::transcription_model::{
        TranscriptionModel, TranscriptionModelCallOptions, TranscriptionModelResponse,
        TranscriptionModelResult,
    };
    use crate::video_model::{
        VideoModel, VideoModelCallOptions, VideoModelResponse, VideoModelResult,
    };

    #[test]
    fn mock_language_model_records_calls_and_returns_scripted_results() {
        let model = MockLanguageModel::new()
            .with_provider("test-provider")
            .with_model_id("test-model")
            .with_supported_urls(BTreeMap::from([(
                "image/*".to_string(),
                vec![r#"https://example\.com/.*"#.to_string()],
            )]))
            .with_generate_result(text_result("first"))
            .with_generate_result(text_result("second"))
            .with_stream_result(LanguageModelStreamResult::new(vec![
                LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(Vec::new())),
            ]));

        assert_eq!(model.provider(), "test-provider");
        assert_eq!(model.model_id(), "test-model");
        assert_eq!(
            poll_ready(model.supported_urls()),
            LanguageModelSupportedUrls::from([(
                "image/*".to_string(),
                vec![r#"https://example\.com/.*"#.to_string()]
            )])
        );

        let first = poll_ready(model.do_generate(LanguageModelCallOptions::new(Vec::new())));
        let second = poll_ready(model.do_generate(LanguageModelCallOptions::new(Vec::new())));
        let stream = poll_ready(model.do_stream(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(extract_text(&first), "first");
        assert_eq!(extract_text(&second), "second");
        assert_eq!(stream.stream.len(), 1);
        assert_eq!(model.generate_calls().len(), 2);
        assert_eq!(model.stream_calls().len(), 1);

        model.clear_calls();
        assert!(model.generate_calls().is_empty());
        assert!(model.stream_calls().is_empty());
    }

    #[test]
    fn mock_language_model_can_drive_generate_text() {
        let model = MockLanguageModel::new().with_generate_result(text_result("hello"));

        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt standardizes"),
        ));

        assert_eq!(result.text, "hello");
        assert_eq!(model.generate_calls().len(), 1);
        assert_eq!(model.generate_calls()[0].prompt.len(), 1);
    }

    #[test]
    fn mock_language_model_v4_returns_array_backed_generate_results_from_the_first_entry() {
        let model = MockLanguageModel::new()
            .with_generate_results([text_result("first"), text_result("second")]);

        let first = poll_ready(model.do_generate(LanguageModelCallOptions::new(Vec::new())));
        let second = poll_ready(model.do_generate(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(extract_text(&first), "first");
        assert_eq!(extract_text(&second), "second");
        assert_eq!(model.generate_calls().len(), 2);
    }

    #[test]
    fn mock_language_model_v4_returns_array_backed_stream_results_from_the_first_entry() {
        let model = MockLanguageModel::new()
            .with_stream_results([stream_result("first"), stream_result("second")]);

        let first = poll_ready(model.do_stream(LanguageModelCallOptions::new(Vec::new())));
        let second = poll_ready(model.do_stream(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(collect_stream_text(&first), "first");
        assert_eq!(collect_stream_text(&second), "second");
        assert_eq!(model.stream_calls().len(), 2);
    }

    #[test]
    fn mock_embedding_model_records_calls_and_capabilities() {
        let model = MockEmbeddingModel::new()
            .with_max_embeddings_per_call(3)
            .with_supports_parallel_calls(true)
            .with_embed_result(
                EmbeddingModelResult::new(vec![vec![0.1, 0.2]])
                    .with_usage(EmbeddingModelUsage::new(2)),
            );

        let result =
            poll_ready(model.do_embed(EmbeddingModelCallOptions::new(vec!["alpha".to_string()])));

        assert_eq!(poll_ready(model.max_embeddings_per_call()), Some(3));
        assert!(poll_ready(model.supports_parallel_calls()));
        assert_eq!(result.embeddings, vec![vec![0.1, 0.2]]);
        assert_eq!(model.embed_calls()[0].values, vec!["alpha"]);
    }

    #[test]
    fn mock_embedding_model_v4_returns_array_backed_embed_results_from_the_first_entry() {
        let model = MockEmbeddingModel::new().with_embed_results([
            EmbeddingModelResult::new(vec![vec![1.0]]),
            EmbeddingModelResult::new(vec![vec![2.0]]),
        ]);

        let first =
            poll_ready(model.do_embed(EmbeddingModelCallOptions::new(vec!["first".to_string()])));
        let second =
            poll_ready(model.do_embed(EmbeddingModelCallOptions::new(vec!["second".to_string()])));

        assert_eq!(first.embeddings, vec![vec![1.0]]);
        assert_eq!(second.embeddings, vec![vec![2.0]]);
        assert_eq!(model.embed_calls().len(), 2);
    }

    #[test]
    fn mock_media_models_record_calls_and_return_scripted_results() {
        let image = MockImageModel::new().with_generate_result(ImageModelResult::new(
            vec![FileDataContent::Base64("aW1hZ2U=".to_string())],
            ImageModelResponse::new(time::OffsetDateTime::UNIX_EPOCH, "image-model"),
        ));
        let speech = MockSpeechModel::new().with_generate_result(SpeechModelResult::new(
            FileDataContent::Base64("YXVkaW8=".to_string()),
            SpeechModelResponse::new(time::OffsetDateTime::UNIX_EPOCH, "speech-model"),
        ));
        let transcription =
            MockTranscriptionModel::new().with_generate_result(TranscriptionModelResult::new(
                "transcript",
                vec![],
                TranscriptionModelResponse::new(
                    time::OffsetDateTime::UNIX_EPOCH,
                    "transcription-model",
                ),
            ));
        let video = MockVideoModel::new().with_generate_result(VideoModelResult::new(
            vec![crate::video_model::VideoModelVideoData::base64(
                "dmlkZW8=",
                "video/mp4",
            )],
            VideoModelResponse::new(time::OffsetDateTime::UNIX_EPOCH, "video-model"),
        ));

        assert_eq!(
            poll_ready(image.do_generate(ImageModelCallOptions::new(1)))
                .images
                .len(),
            1
        );
        assert_eq!(image.generate_calls()[0].n, 1);
        assert_eq!(poll_ready(image.max_images_per_call()), Some(1));

        assert_eq!(
            poll_ready(speech.do_generate(SpeechModelCallOptions::new("speak"))).audio,
            FileDataContent::Base64("YXVkaW8=".to_string())
        );
        assert_eq!(speech.generate_calls()[0].text, "speak");

        assert_eq!(
            poll_ready(
                transcription.do_generate(TranscriptionModelCallOptions::new(
                    FileDataContent::Bytes(vec![1, 2, 3]),
                    "audio/wav",
                ))
            )
            .text,
            "transcript"
        );
        assert_eq!(transcription.generate_calls()[0].media_type, "audio/wav");

        assert_eq!(poll_ready(video.max_videos_per_call()), Some(1));
        assert_eq!(
            poll_ready(video.do_generate(VideoModelCallOptions::new(1)))
                .videos
                .len(),
            1
        );
        assert_eq!(video.generate_calls()[0].n, 1);
    }

    #[test]
    fn mock_reranking_model_records_calls_and_returns_rankings() {
        let model = MockRerankingModel::new().with_rerank_result(RerankingModelResult::new(vec![
            RerankingModelRanking::new(1, 0.9),
        ]));

        let result = poll_ready(model.do_rerank(RerankingModelCallOptions::new(
            RerankingModelDocuments::text(vec!["a".to_string(), "b".to_string()]),
            "query",
        )));

        assert_eq!(result.ranking[0].index, 1);
        assert_eq!(model.rerank_calls()[0].query, "query");
    }

    #[test]
    fn mock_provider_resolves_registered_models_and_reports_missing_ids() {
        let provider = MockProvider::new()
            .with_language_model("language", MockLanguageModel::new())
            .with_embedding_model("embedding", MockEmbeddingModel::new())
            .with_image_model("image", MockImageModel::new())
            .with_transcription_model("transcription", MockTranscriptionModel::new())
            .with_speech_model("speech", MockSpeechModel::new())
            .with_reranking_model("reranking", MockRerankingModel::new())
            .with_video_model("video", MockVideoModel::new());

        assert_eq!(
            provider
                .language_model("language")
                .expect("language model exists")
                .model_id(),
            "mock-model-id"
        );
        assert!(provider.embedding_model("embedding").is_ok());
        assert!(provider.image_model("image").is_ok());
        assert!(provider.transcription_model("transcription").is_ok());
        assert!(provider.speech_model("speech").is_ok());
        assert!(provider.reranking_model("reranking").is_ok());
        assert!(provider.video_model("video").is_ok());

        let error = provider
            .language_model("missing")
            .expect_err("missing models report NoSuchModelError");
        assert_eq!(error.model_id(), "missing");
        assert_eq!(error.model_type(), ModelType::LanguageModel);
    }

    #[test]
    fn cloned_mock_models_share_call_recording() {
        let model = MockLanguageModel::new().with_generate_result(text_result("shared"));
        let clone = model.clone();

        let result = poll_ready(clone.do_generate(LanguageModelCallOptions::new(Vec::new())));

        assert_eq!(extract_text(&result), "shared");
        assert_eq!(model.generate_calls().len(), 1);
    }

    fn text_result(text: &str) -> LanguageModelGenerateResult {
        LanguageModelGenerateResult::new(
            vec![LanguageModelContent::Text(LanguageModelText::new(text))],
            LanguageModelFinishReason {
                unified: FinishReason::Stop,
                raw: Some("stop".to_string()),
            },
            LanguageModelUsage {
                input_tokens: InputTokenUsage {
                    total: Some(1),
                    ..InputTokenUsage::default()
                },
                output_tokens: OutputTokenUsage {
                    total: Some(1),
                    text: Some(1),
                    ..OutputTokenUsage::default()
                },
                raw: Some(serde_json::Map::from_iter([(
                    "fixture".to_string(),
                    json!(true),
                )])),
            },
        )
    }

    fn stream_result(text: &str) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        LanguageModelStreamResult::new(vec![
            LanguageModelStreamPart::TextStart(LanguageModelTextStart::new(text)),
            LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new(text, text)),
            LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(text)),
        ])
    }

    fn extract_text(result: &LanguageModelGenerateResult) -> &str {
        match &result.content[0] {
            LanguageModelContent::Text(text) => &text.text,
            _ => panic!("expected text result"),
        }
    }

    fn collect_stream_text(
        result: &LanguageModelStreamResult<Vec<LanguageModelStreamPart>>,
    ) -> String {
        result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::TextDelta(delta) => Some(delta.delta.as_str()),
                _ => None,
            })
            .collect()
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("mock futures should be ready"),
        }
    }
}
