use crate::VERSION;
use crate::file_data::FileDataContent;
use crate::generate_text::GeneratedFile;
use crate::headers::Headers;
use crate::image_model::{
    ImageModel, ImageModelCallOptions, ImageModelFile, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponseMetadata, ImageModelResult, ImageModelUsage,
    NoImageGeneratedError,
};
use crate::language_model::ProviderAbortSignal;
use crate::logger::{LogWarningsOptions, log_warnings};
use crate::provider::ProviderOptions;
use crate::provider_utils::{convert_base64_to_bytes, detect_media_type, with_user_agent_suffix};
use crate::warning::Warning;
use serde::{Deserialize, Serialize};
use url::Url;

/// High-level image input accepted by `generate_image` for edit/variation prompts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum GenerateImagePromptImage {
    /// Raw bytes or base64-encoded image data.
    Data { data: FileDataContent },

    /// Data URL image input.
    DataUrl {
        /// Data URL string.
        #[serde(rename = "dataUrl")]
        data_url: String,
    },

    /// URL image input.
    Url { url: Url },
}

impl GenerateImagePromptImage {
    /// Creates a base64 image input.
    pub fn base64(base64: impl Into<String>) -> Self {
        Self::Data {
            data: FileDataContent::Base64(base64.into()),
        }
    }

    /// Creates a byte image input.
    pub fn bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self::Data {
            data: FileDataContent::Bytes(bytes.into()),
        }
    }

    /// Creates a data URL image input.
    pub fn data_url(data_url: impl Into<String>) -> Self {
        Self::DataUrl {
            data_url: data_url.into(),
        }
    }

    /// Creates a URL image input.
    pub fn url(url: Url) -> Self {
        Self::Url { url }
    }

    /// Creates an input from a string using upstream string handling:
    /// `http*` strings are URLs, `data:` strings are data URLs, and other
    /// strings are treated as base64 media data.
    pub fn string(value: impl Into<String>) -> Self {
        let value = value.into();

        if value.starts_with("http") {
            if let Ok(url) = Url::parse(&value) {
                return Self::Url { url };
            }
        }

        if value.starts_with("data:") {
            return Self::DataUrl { data_url: value };
        }

        Self::base64(value)
    }
}

impl From<FileDataContent> for GenerateImagePromptImage {
    fn from(data: FileDataContent) -> Self {
        Self::Data { data }
    }
}

impl From<Url> for GenerateImagePromptImage {
    fn from(url: Url) -> Self {
        Self::Url { url }
    }
}

impl From<String> for GenerateImagePromptImage {
    fn from(value: String) -> Self {
        Self::string(value)
    }
}

impl From<&str> for GenerateImagePromptImage {
    fn from(value: &str) -> Self {
        Self::string(value)
    }
}

/// Structured prompt form accepted by high-level `generate_image`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateImagePromptImages {
    /// Input images for image editing or variation generation.
    pub images: Vec<GenerateImagePromptImage>,

    /// Optional text prompt to send alongside the images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Optional image mask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<GenerateImagePromptImage>,
}

impl GenerateImagePromptImages {
    /// Creates a structured image prompt.
    pub fn new<I>(images: I) -> Self
    where
        I: IntoIterator<Item = GenerateImagePromptImage>,
    {
        Self {
            images: images.into_iter().collect(),
            text: None,
            mask: None,
        }
    }

    /// Adds text to the structured image prompt.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Adds a mask image.
    pub fn with_mask(mut self, mask: impl Into<GenerateImagePromptImage>) -> Self {
        self.mask = Some(mask.into());
        self
    }
}

/// Prompt accepted by high-level `generate_image`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum GenerateImagePrompt {
    /// Plain text image-generation prompt.
    Text(String),

    /// Image edit/variation prompt with optional text and mask.
    Images(GenerateImagePromptImages),
}

impl GenerateImagePrompt {
    /// Creates a plain text prompt.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// Creates a structured image prompt.
    pub fn images(images: GenerateImagePromptImages) -> Self {
        Self::Images(images)
    }
}

impl From<String> for GenerateImagePrompt {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for GenerateImagePrompt {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<GenerateImagePromptImages> for GenerateImagePrompt {
    fn from(images: GenerateImagePromptImages) -> Self {
        Self::Images(images)
    }
}

/// Options for a high-level `generate_image` call.
pub struct GenerateImageOptions<'a, M: ImageModel + ?Sized> {
    /// Image model used for the call.
    pub model: &'a M,

    /// Prompt that should be used to generate the image.
    pub prompt: GenerateImagePrompt,

    /// Number of images to generate.
    pub n: u64,

    /// Maximum number of images to request in one provider call.
    pub max_images_per_call: Option<usize>,

    /// Image size in the `{width}x{height}` format.
    pub size: Option<String>,

    /// Image aspect ratio in the `{width}:{height}` format.
    pub aspect_ratio: Option<String>,

    /// Seed for image generation.
    pub seed: Option<u64>,

    /// Provider-specific options passed through to the model.
    pub provider_options: Option<ProviderOptions>,

    /// Abort signal for cancelling image generation calls.
    pub abort_signal: Option<ProviderAbortSignal>,

    /// Additional HTTP headers for HTTP-based providers.
    pub headers: Option<Headers>,
}

impl<'a, M: ImageModel + ?Sized> GenerateImageOptions<'a, M> {
    /// Creates options for a high-level `generate_image` call.
    pub fn new(model: &'a M, prompt: impl Into<GenerateImagePrompt>) -> Self {
        Self {
            model,
            prompt: prompt.into(),
            n: 1,
            max_images_per_call: None,
            size: None,
            aspect_ratio: None,
            seed: None,
            provider_options: None,
            abort_signal: None,
            headers: None,
        }
    }

    /// Sets the number of images to generate.
    pub const fn with_n(mut self, n: u64) -> Self {
        self.n = n;
        self
    }

    /// Sets the maximum number of images to request per provider call.
    pub const fn with_max_images_per_call(mut self, max_images_per_call: usize) -> Self {
        self.max_images_per_call = Some(max_images_per_call);
        self
    }

    /// Sets the generated image size.
    pub fn with_size(mut self, size: impl Into<String>) -> Self {
        self.size = Some(size.into());
        self
    }

    /// Sets the generated image aspect ratio.
    pub fn with_aspect_ratio(mut self, aspect_ratio: impl Into<String>) -> Self {
        self.aspect_ratio = Some(aspect_ratio.into());
        self
    }

    /// Sets the image generation seed.
    pub const fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Sets the abort signal for image generation calls.
    pub fn with_abort_signal(mut self, abort_signal: ProviderAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Sets all additional HTTP headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Adds an additional HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Result of a high-level `generate_image` call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateImageResult {
    /// The first image that was generated.
    pub image: GeneratedFile,

    /// All generated images.
    pub images: Vec<GeneratedFile>,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,

    /// Response metadata from provider calls.
    pub responses: Vec<ImageModelResponseMetadata>,

    /// Provider-specific metadata aggregated across provider calls.
    pub provider_metadata: ImageModelProviderMetadata,

    /// Combined token usage across provider calls.
    pub usage: ImageModelUsage,
}

impl GenerateImageResult {
    /// Creates a high-level image-generation result.
    pub fn new<R, I>(
        images: Vec<GeneratedFile>,
        warnings: Vec<Warning>,
        responses: I,
        provider_metadata: ImageModelProviderMetadata,
        usage: ImageModelUsage,
    ) -> Result<Self, NoImageGeneratedError>
    where
        R: Into<ImageModelResponseMetadata>,
        I: IntoIterator<Item = R>,
    {
        let responses = responses.into_iter().map(Into::into).collect::<Vec<_>>();
        let image = images
            .first()
            .cloned()
            .ok_or_else(|| NoImageGeneratedError::with_responses(responses.clone()))?;

        Ok(Self {
            image,
            images,
            warnings,
            responses,
            provider_metadata,
            usage,
        })
    }
}

/// Deprecated upstream-compatible alias for [`GenerateImageResult`].
pub type ExperimentalGenerateImageResult = GenerateImageResult;

/// Generates images using an image model.
pub async fn generate_image<M: ImageModel + ?Sized>(
    options: GenerateImageOptions<'_, M>,
) -> Result<GenerateImageResult, NoImageGeneratedError> {
    let GenerateImageOptions {
        model,
        prompt,
        n,
        max_images_per_call,
        size,
        aspect_ratio,
        seed,
        provider_options,
        abort_signal,
        headers,
    } = options;

    let headers = headers_with_ai_user_agent(headers);
    let max_images_per_call = match max_images_per_call {
        Some(max_images_per_call) => max_images_per_call,
        None => model.max_images_per_call().await.unwrap_or(1),
    }
    .max(1);

    let mut images = Vec::new();
    let mut warnings = Vec::new();
    let mut responses: Vec<ImageModelResponseMetadata> = Vec::new();
    let mut provider_metadata = ImageModelProviderMetadata::new();
    let mut usage = ImageModelUsage::new();

    for image_count in image_call_counts(n, max_images_per_call) {
        let normalized_prompt = normalize_prompt(&prompt);
        let ImageModelResult {
            images: call_images,
            warnings: call_warnings,
            provider_metadata: call_provider_metadata,
            response,
            usage: call_usage,
        } = model
            .do_generate(ImageModelCallOptions {
                prompt: normalized_prompt.prompt,
                n: image_count,
                size: size.clone(),
                aspect_ratio: aspect_ratio.clone(),
                seed,
                files: normalized_prompt.files,
                mask: normalized_prompt.mask,
                provider_options: provider_options.clone().unwrap_or_default(),
                abort_signal: abort_signal.clone(),
                headers: Some(headers.clone()),
            })
            .await;

        images.extend(call_images.into_iter().map(generated_file_from_image));
        warnings.extend(call_warnings);
        responses.push(response.into());

        if let Some(call_usage) = call_usage {
            usage = add_image_model_usage(usage, call_usage);
        }

        if let Some(call_provider_metadata) = call_provider_metadata {
            merge_image_provider_metadata(&mut provider_metadata, call_provider_metadata);
        }
    }

    log_warnings(
        &LogWarningsOptions::new(warnings.clone()).with_scope(model.provider(), model.model_id()),
    );

    GenerateImageResult::new(images, warnings, responses, provider_metadata, usage)
}

/// Deprecated upstream-compatible alias for [`generate_image`].
pub async fn experimental_generate_image<M: ImageModel + ?Sized>(
    options: GenerateImageOptions<'_, M>,
) -> Result<ExperimentalGenerateImageResult, NoImageGeneratedError> {
    generate_image(options).await
}

struct NormalizedPrompt {
    prompt: Option<String>,
    files: Option<Vec<ImageModelFile>>,
    mask: Option<ImageModelFile>,
}

fn normalize_prompt(prompt: &GenerateImagePrompt) -> NormalizedPrompt {
    match prompt {
        GenerateImagePrompt::Text(prompt) => NormalizedPrompt {
            prompt: Some(prompt.clone()),
            files: None,
            mask: None,
        },
        GenerateImagePrompt::Images(prompt) => NormalizedPrompt {
            prompt: prompt.text.clone(),
            files: Some(
                prompt
                    .images
                    .iter()
                    .map(to_image_model_file)
                    .collect::<Vec<_>>(),
            ),
            mask: prompt.mask.as_ref().map(to_image_model_file),
        },
    }
}

fn to_image_model_file(image: &GenerateImagePromptImage) -> ImageModelFile {
    match image {
        GenerateImagePromptImage::Url { url } => ImageModelFile::url(url.clone()),
        GenerateImagePromptImage::Data { data } => image_model_file_from_data(data.clone(), None),
        GenerateImagePromptImage::DataUrl { data_url } => image_model_file_from_data_url(data_url),
    }
}

fn image_model_file_from_data_url(data_url: &str) -> ImageModelFile {
    let Some((header, base64_content)) = data_url.split_once(',') else {
        return image_model_file_from_data(FileDataContent::Base64(data_url.to_string()), None);
    };

    let media_type = header
        .strip_prefix("data:")
        .and_then(|header| header.split(';').next())
        .filter(|media_type| !media_type.is_empty())
        .map(str::to_string);

    let data = convert_base64_to_bytes(base64_content)
        .map(FileDataContent::Bytes)
        .unwrap_or_else(|_| FileDataContent::Base64(base64_content.to_string()));

    image_model_file_from_data(data, media_type)
}

fn image_model_file_from_data(data: FileDataContent, media_type: Option<String>) -> ImageModelFile {
    let data = match data {
        FileDataContent::Base64(base64) => convert_base64_to_bytes(&base64)
            .map(FileDataContent::Bytes)
            .unwrap_or(FileDataContent::Base64(base64)),
        FileDataContent::Bytes(bytes) => FileDataContent::Bytes(bytes),
    };
    let media_type = media_type.unwrap_or_else(|| {
        detect_media_type(&data, Some("image"))
            .unwrap_or("image/png")
            .to_string()
    });

    ImageModelFile::file(media_type, data)
}

fn generated_file_from_image(image: FileDataContent) -> GeneratedFile {
    let media_type = detect_media_type(&image, Some("image")).unwrap_or("image/png");
    GeneratedFile::new(media_type, image)
}

fn image_call_counts(n: u64, max_images_per_call: usize) -> Vec<u64> {
    if n == 0 {
        return Vec::new();
    }

    let max_images_per_call =
        u64::try_from(max_images_per_call).expect("usize fits into u64 on supported platforms");
    let call_count = n.div_ceil(max_images_per_call);

    (0..call_count)
        .map(|index| {
            if index + 1 < call_count {
                max_images_per_call
            } else {
                let remainder = n % max_images_per_call;
                if remainder == 0 {
                    max_images_per_call
                } else {
                    remainder
                }
            }
        })
        .collect()
}

fn headers_with_ai_user_agent(headers: Option<Headers>) -> Headers {
    let header_entries: Vec<(String, Option<String>)> = headers
        .unwrap_or_default()
        .into_iter()
        .map(|(name, value)| (name, Some(value)))
        .collect();

    with_user_agent_suffix(Some(header_entries), [format!("ai/{VERSION}")])
}

fn add_image_model_usage(usage1: ImageModelUsage, usage2: ImageModelUsage) -> ImageModelUsage {
    ImageModelUsage {
        input_tokens: add_token_counts(usage1.input_tokens, usage2.input_tokens),
        output_tokens: add_token_counts(usage1.output_tokens, usage2.output_tokens),
        total_tokens: add_token_counts(usage1.total_tokens, usage2.total_tokens),
    }
}

fn add_token_counts(token_count1: Option<u64>, token_count2: Option<u64>) -> Option<u64> {
    match (token_count1, token_count2) {
        (None, None) => None,
        (left, right) => Some(left.unwrap_or(0) + right.unwrap_or(0)),
    }
}

fn merge_image_provider_metadata(
    provider_metadata: &mut ImageModelProviderMetadata,
    call_provider_metadata: ImageModelProviderMetadata,
) {
    for (provider_name, metadata) in call_provider_metadata {
        if provider_name == "gateway" {
            merge_gateway_provider_metadata(provider_metadata, metadata);
            continue;
        }

        provider_metadata
            .entry(provider_name)
            .or_insert_with(|| ImageModelProviderMetadataEntry::new(Vec::new()))
            .images
            .extend(metadata.images);
    }
}

fn merge_gateway_provider_metadata(
    provider_metadata: &mut ImageModelProviderMetadata,
    metadata: ImageModelProviderMetadataEntry,
) {
    let entry = provider_metadata
        .entry("gateway".to_string())
        .or_insert_with(|| ImageModelProviderMetadataEntry::new(Vec::new()));

    if !metadata.images.is_empty() {
        entry.images = metadata.images;
    }

    entry.extra.extend(metadata.extra);
}

#[cfg(test)]
mod tests {
    use super::{
        GenerateImageOptions, GenerateImagePrompt, GenerateImagePromptImage,
        GenerateImagePromptImages, GenerateImageResult, ImageModelProviderMetadataEntry,
        experimental_generate_image,
    };
    use crate::ProviderAbortController;
    use crate::file_data::FileDataContent;
    use crate::headers::Headers;
    use crate::image_model::{
        ImageModel, ImageModelCallOptions, ImageModelProviderMetadata, ImageModelResponse,
        ImageModelResponseMetadata, ImageModelResult, ImageModelUsage,
    };
    use crate::json::JsonValue;
    use crate::logger::{LogWarningsOptions, take_log_warning_calls_for_tests};
    use crate::provider::{ProviderOptions, SpecificationVersion};
    use crate::warning::Warning;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::sync::Mutex;
    use std::task::{Context, Poll, Waker};
    use time::OffsetDateTime;

    struct RecordingImageModel {
        max_images_per_call: Option<usize>,
        max_images_calls: Mutex<usize>,
        calls: Mutex<Vec<ImageModelCallOptions>>,
        results: Mutex<VecDeque<ImageModelResult>>,
    }

    impl RecordingImageModel {
        fn new(results: Vec<ImageModelResult>) -> Self {
            Self {
                max_images_per_call: None,
                max_images_calls: Mutex::new(0),
                calls: Mutex::new(Vec::new()),
                results: Mutex::new(results.into()),
            }
        }

        fn with_max_images_per_call(mut self, max_images_per_call: usize) -> Self {
            self.max_images_per_call = Some(max_images_per_call);
            self
        }

        fn calls(&self) -> Vec<ImageModelCallOptions> {
            self.calls
                .lock()
                .expect("calls lock is not poisoned")
                .clone()
        }

        fn max_images_calls(&self) -> usize {
            *self
                .max_images_calls
                .lock()
                .expect("max-images lock is not poisoned")
        }
    }

    impl ImageModel for RecordingImageModel {
        type MaxImagesPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<ImageModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "image-test"
        }

        fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
            *self
                .max_images_calls
                .lock()
                .expect("max-images lock is not poisoned") += 1;

            ready(self.max_images_per_call)
        }

        fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
            self.calls
                .lock()
                .expect("calls lock is not poisoned")
                .push(options.clone());
            let result = self
                .results
                .lock()
                .expect("results lock is not poisoned")
                .pop_front()
                .unwrap_or_else(|| ImageModelResult::new(Vec::new(), image_response("fallback")));

            ready(result)
        }
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures should be ready"),
        }
    }

    fn image_response(model_id: &str) -> ImageModelResponse {
        ImageModelResponse::new(
            OffsetDateTime::parse(
                "2024-01-02T03:04:05Z",
                &time::format_description::well_known::Rfc3339,
            )
            .expect("timestamp parses"),
            model_id,
        )
    }

    fn png_base64() -> &'static str {
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAACklEQVR4nGMAAQAABQABDQottAAAAABJRU5ErkJggg=="
    }

    fn jpeg_base64() -> &'static str {
        "/9j/4AAQSkZJRgABAQEAYABgAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/2wBDAQkJCQwLDBgNDRgyIRwhMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjL/wAARCAABAAEDASIAAhEBAxEB/8QAFQABAQAAAAAAAAAAAAAAAAAAAAb/xAAUEAEAAAAAAAAAAAAAAAAAAAAA/8QAFQEBAQAAAAAAAAAAAAAAAAAAAAX/xAAUEQEAAAAAAAAAAAAAAAAAAAAA/9oADAMBAAIRAxEAPwCdABmX/9k="
    }

    fn gif_base64() -> &'static str {
        "R0lGODlhAQABAIAAAAUEBAAAACwAAAAAAQABAAACAkQBADs="
    }

    fn metadata(provider: &str, images: Vec<serde_json::Value>) -> ImageModelProviderMetadata {
        [(
            provider.to_string(),
            ImageModelProviderMetadataEntry::new(images),
        )]
        .into_iter()
        .collect()
    }

    fn metadata_entry(
        provider: &str,
        entry: ImageModelProviderMetadataEntry,
    ) -> ImageModelProviderMetadata {
        [(provider.to_string(), entry)].into_iter().collect()
    }

    fn base64_image_result(base64: &str) -> ImageModelResult {
        ImageModelResult::new(
            vec![FileDataContent::Base64(base64.to_string())],
            image_response("test-model-id"),
        )
    }

    #[test]
    fn result_serializes_upstream_shape() {
        let result = GenerateImageResult::new(
            vec![
                crate::GeneratedFile::from_base64("image/png", png_base64()),
                crate::GeneratedFile::from_base64("image/jpeg", jpeg_base64()),
            ],
            vec![Warning::Other {
                message: "setting ignored".to_string(),
            }],
            vec![image_response("image-test").with_header("x-response-id", "res_1")],
            metadata(
                "openai",
                vec![json!({"revisedPrompt": "a cat"}), JsonValue::Null],
            ),
            ImageModelUsage::new()
                .with_input_tokens(3)
                .with_total_tokens(3),
        )
        .expect("result has an image");

        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "image": {
                    "base64": png_base64(),
                    "mediaType": "image/png"
                },
                "images": [
                    {
                        "base64": png_base64(),
                        "mediaType": "image/png"
                    },
                    {
                        "base64": jpeg_base64(),
                        "mediaType": "image/jpeg"
                    }
                ],
                "warnings": [
                    {
                        "type": "other",
                        "message": "setting ignored"
                    }
                ],
                "responses": [
                    {
                        "timestamp": "2024-01-02T03:04:05Z",
                        "modelId": "image-test",
                        "headers": {
                            "x-response-id": "res_1"
                        }
                    }
                ],
                "providerMetadata": {
                    "openai": {
                        "images": [
                            {
                                "revisedPrompt": "a cat"
                            },
                            null
                        ]
                    }
                },
                "usage": {
                    "inputTokens": 3,
                    "totalTokens": 3
                }
            })
        );
    }

    #[test]
    fn generate_image_sends_normalized_prompt_and_ai_user_agent_to_model() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "style": "vivid"
            }
        }))
        .expect("provider options deserialize");
        let model = RecordingImageModel::new(vec![ImageModelResult::new(
            vec![FileDataContent::Base64(png_base64().to_string())],
            image_response("image-test"),
        )]);
        let data_url = format!("data:image/png;base64,{}", png_base64());

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(
                &model,
                GenerateImagePromptImages::new(vec![GenerateImagePromptImage::data_url(data_url)])
                    .with_text("sunny day")
                    .with_mask(png_base64()),
            )
            .with_size("1024x1024")
            .with_aspect_ratio("16:9")
            .with_seed(123)
            .with_provider_options(provider_options.clone())
            .with_header("custom-request-header", "request-header-value"),
        ))
        .expect("image generation succeeds");

        assert_eq!(result.image.media_type(), "image/png");
        assert_eq!(result.image.base64(), png_base64());

        let calls = model.calls();
        assert_eq!(calls.len(), 1);
        let call = &calls[0];
        assert_eq!(call.prompt.as_deref(), Some("sunny day"));
        assert_eq!(call.n, 1);
        assert_eq!(call.size.as_deref(), Some("1024x1024"));
        assert_eq!(call.aspect_ratio.as_deref(), Some("16:9"));
        assert_eq!(call.seed, Some(123));
        assert_eq!(call.provider_options, provider_options);
        assert_eq!(
            call.headers
                .as_ref()
                .and_then(|headers| headers.get("user-agent")),
            Some(&format!("ai/{}", crate::VERSION))
        );
        assert_eq!(
            call.headers
                .as_ref()
                .and_then(|headers| headers.get("custom-request-header")),
            Some(&"request-header-value".to_string())
        );
        assert!(matches!(
            call.files.as_deref(),
            Some([crate::ImageModelFile::File {
                media_type,
                data: FileDataContent::Bytes(_),
                ..
            }]) if media_type == "image/png"
        ));
        assert!(matches!(
            call.mask.as_ref(),
            Some(crate::ImageModelFile::File {
                media_type,
                data: FileDataContent::Bytes(_),
                ..
            }) if media_type == "image/png"
        ));
    }

    #[test]
    fn generate_image_forwards_abort_signal_to_model_call() {
        let abort_controller = ProviderAbortController::new();
        let model = RecordingImageModel::new(vec![ImageModelResult::new(
            vec![FileDataContent::Base64(png_base64().to_string())],
            image_response("image-test"),
        )]);

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day")
                .with_abort_signal(abort_controller.signal()),
        ))
        .expect("image generation succeeds");

        assert_eq!(result.image.base64(), png_base64());
        let calls = model.calls();
        assert_eq!(calls.len(), 1);
        let call_signal = calls[0].abort_signal.clone().expect("abort signal set");
        assert!(!call_signal.is_aborted());

        abort_controller.abort_with_reason("client-disconnected");
        assert!(call_signal.is_aborted());
        assert_eq!(call_signal.reason(), Some(json!("client-disconnected")));
    }

    #[test]
    fn generate_image_chunks_calls_and_aggregates_results() {
        let first_response = image_response("image-test").with_header("x-call", "1");
        let second_response = image_response("image-test").with_header("x-call", "2");
        let first_result = ImageModelResult::new(
            vec![FileDataContent::Base64(png_base64().to_string())],
            first_response.clone(),
        )
        .with_warning(Warning::Other {
            message: "first".to_string(),
        })
        .with_usage(
            ImageModelUsage::new()
                .with_input_tokens(2)
                .with_total_tokens(2),
        )
        .with_provider_metadata(metadata("openai", vec![json!({"revisedPrompt": "first"})]));
        let second_result = ImageModelResult::new(
            vec![FileDataContent::Base64(jpeg_base64().to_string())],
            second_response.clone(),
        )
        .with_warning(Warning::Other {
            message: "second".to_string(),
        })
        .with_usage(
            ImageModelUsage::new()
                .with_input_tokens(3)
                .with_total_tokens(3),
        )
        .with_provider_metadata(metadata("openai", vec![json!({"revisedPrompt": "second"})]));
        let model =
            RecordingImageModel::new(vec![first_result, second_result]).with_max_images_per_call(1);

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day").with_n(2),
        ))
        .expect("image generation succeeds");

        assert_eq!(model.max_images_calls(), 1);
        assert_eq!(
            model
                .calls()
                .iter()
                .map(|options| options.n)
                .collect::<Vec<_>>(),
            vec![1, 1]
        );
        assert_eq!(
            result
                .images
                .iter()
                .map(crate::GeneratedFile::base64)
                .collect::<Vec<_>>(),
            vec![png_base64(), jpeg_base64()]
        );
        assert_eq!(
            result
                .images
                .iter()
                .map(|image| image.media_type().to_string())
                .collect::<Vec<_>>(),
            vec!["image/png", "image/jpeg"]
        );
        assert_eq!(
            result.warnings,
            vec![
                Warning::Other {
                    message: "first".to_string(),
                },
                Warning::Other {
                    message: "second".to_string(),
                },
            ]
        );
        assert_eq!(
            result.responses,
            vec![
                ImageModelResponseMetadata::from_response(first_response),
                ImageModelResponseMetadata::from_response(second_response)
            ]
        );
        assert_eq!(
            result
                .provider_metadata
                .get("openai")
                .expect("openai metadata exists")
                .images,
            vec![
                json!({"revisedPrompt": "first"}),
                json!({"revisedPrompt": "second"})
            ]
        );
        assert_eq!(
            result.usage,
            ImageModelUsage::new()
                .with_input_tokens(5)
                .with_total_tokens(5)
        );
    }

    #[test]
    fn generate_image_uses_explicit_max_images_per_call_without_querying_model_limit() {
        let model = RecordingImageModel::new(vec![
            ImageModelResult::new(
                vec![FileDataContent::Base64(png_base64().to_string())],
                image_response("image-test"),
            ),
            ImageModelResult::new(
                vec![FileDataContent::Base64(jpeg_base64().to_string())],
                image_response("image-test"),
            ),
        ])
        .with_max_images_per_call(1);

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day")
                .with_n(3)
                .with_max_images_per_call(2),
        ))
        .expect("image generation succeeds");

        assert_eq!(model.max_images_calls(), 0);
        assert_eq!(
            model
                .calls()
                .iter()
                .map(|options| options.n)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert_eq!(result.images.len(), 2);
    }

    #[test]
    fn generate_image_returns_no_image_error_with_responses() {
        let response = image_response("image-test").with_header("x-response-id", "res_1");
        let expected_response = ImageModelResponseMetadata::from_response(response.clone());
        let model = RecordingImageModel::new(vec![ImageModelResult::new(Vec::new(), response)]);

        let error = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect_err("empty image response fails");

        assert_eq!(error.message(), "No image generated.");
        assert_eq!(
            error.responses().expect("responses are retained"),
            &[expected_response]
        );
    }

    #[test]
    fn generate_image_with_zero_images_makes_no_model_call_and_errors() {
        let model = RecordingImageModel::new(Vec::new());

        let error = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day").with_n(0),
        ))
        .expect_err("zero requested images fail");

        assert!(model.calls().is_empty());
        assert_eq!(error.responses(), Some(&[][..]));
    }

    #[test]
    fn prompt_image_string_parses_url_data_url_and_base64_inputs() {
        let url = GenerateImagePromptImage::from("https://example.com/image.png");
        let data_url = GenerateImagePromptImage::from("data:image/png;base64,aGVsbG8=");
        let base64 = GenerateImagePromptImage::from("aGVsbG8=");

        assert!(matches!(url, GenerateImagePromptImage::Url { .. }));
        assert!(matches!(data_url, GenerateImagePromptImage::DataUrl { .. }));
        assert_eq!(
            base64,
            GenerateImagePromptImage::Data {
                data: FileDataContent::Base64("aGVsbG8=".to_string())
            }
        );
    }

    #[test]
    fn experimental_generate_image_alias_uses_generate_image_runtime() {
        let model = RecordingImageModel::new(vec![ImageModelResult::new(
            vec![FileDataContent::Base64(png_base64().to_string())],
            image_response("image-test"),
        )]);

        let result = poll_ready(experimental_generate_image(GenerateImageOptions::new(
            &model,
            GenerateImagePrompt::text("sunny day"),
        )))
        .expect("image generation succeeds");

        assert_eq!(result.image.base64(), png_base64());
        assert_eq!(model.specification_version(), SpecificationVersion::V4);
    }

    #[test]
    fn prompt_image_data_url_serde_uses_camel_case_field_name() {
        let image = GenerateImagePromptImage::data_url("data:image/png;base64,aGVsbG8=");

        assert_eq!(
            serde_json::to_value(image).expect("prompt image serializes"),
            json!({
                "type": "data-url",
                "dataUrl": "data:image/png;base64,aGVsbG8="
            })
        );
    }

    #[test]
    fn headers_builder_accepts_existing_headers() {
        let mut headers = Headers::new();
        headers.insert("x-request-id".to_string(), "req_1".to_string());

        let model = RecordingImageModel::new(Vec::new());
        let options = GenerateImageOptions::new(&model, "sunny day").with_headers(headers.clone());

        assert_eq!(options.headers, Some(headers));
    }

    // packages-ai-0207: it should send args to doGenerate.
    #[test]
    fn generate_image_sends_args_to_do_generate() {
        let abort_controller = ProviderAbortController::new();
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "mock-provider": { "style": "vivid" }
        }))
        .expect("provider options deserialize");
        let model = RecordingImageModel::new(vec![base64_image_result(png_base64())]);
        let png_bytes = crate::provider_utils::convert_base64_to_bytes(png_base64())
            .expect("png base64 decodes");

        poll_ready(super::generate_image(
            GenerateImageOptions::new(
                &model,
                GenerateImagePromptImages::new(vec![
                    GenerateImagePromptImage::base64(png_base64()),
                ])
                .with_text("sunny day at the beach")
                .with_mask(GenerateImagePromptImage::base64(png_base64())),
            )
            .with_size("1024x1024")
            .with_aspect_ratio("16:9")
            .with_seed(12345)
            .with_provider_options(provider_options.clone())
            .with_header("custom-request-header", "request-header-value")
            .with_abort_signal(abort_controller.signal()),
        ))
        .expect("image generation succeeds");

        let calls = model.calls();
        assert_eq!(calls.len(), 1);
        let call = &calls[0];
        assert_eq!(call.n, 1);
        assert_eq!(call.prompt.as_deref(), Some("sunny day at the beach"));
        assert_eq!(call.size.as_deref(), Some("1024x1024"));
        assert_eq!(call.aspect_ratio.as_deref(), Some("16:9"));
        assert_eq!(call.seed, Some(12345));
        assert_eq!(call.provider_options, provider_options);
        assert_eq!(
            call.headers
                .as_ref()
                .and_then(|headers| headers.get("custom-request-header")),
            Some(&"request-header-value".to_string())
        );
        assert_eq!(
            call.headers
                .as_ref()
                .and_then(|headers| headers.get("user-agent")),
            Some(&format!("ai/{}", crate::VERSION))
        );
        assert!(call.abort_signal.is_some());
        assert!(matches!(
            call.files.as_deref(),
            Some([crate::ImageModelFile::File { media_type, data: FileDataContent::Bytes(bytes), .. }])
                if media_type == "image/png" && bytes == &png_bytes
        ));
        assert!(matches!(
            call.mask.as_ref(),
            Some(crate::ImageModelFile::File { media_type, data: FileDataContent::Bytes(bytes), .. })
                if media_type == "image/png" && bytes == &png_bytes
        ));
    }

    // packages-ai-0208: it should return warnings.
    #[test]
    fn generate_image_returns_warnings_from_single_call() {
        let model = RecordingImageModel::new(vec![base64_image_result(png_base64()).with_warning(
            Warning::Other {
                message: "Setting is not supported".to_string(),
            },
        )]);

        let result = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        assert_eq!(
            result.warnings,
            vec![Warning::Other {
                message: "Setting is not supported".to_string(),
            }]
        );
    }

    // packages-ai-0209: it should call logWarnings with the correct warnings.
    #[test]
    fn generate_image_calls_log_warnings_with_correct_warnings() {
        let expected_warnings = vec![
            Warning::Other {
                message: "Setting is not supported".to_string(),
            },
            Warning::Unsupported {
                feature: "size".to_string(),
                details: Some("Size parameter not supported".to_string()),
            },
        ];
        let mut result = base64_image_result(png_base64());
        result.warnings = expected_warnings.clone();
        let model = RecordingImageModel::new(vec![result]);
        take_log_warning_calls_for_tests();

        poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        assert_eq!(
            take_log_warning_calls_for_tests(),
            vec![
                LogWarningsOptions::new(expected_warnings)
                    .with_scope("test-provider", "image-test")
            ]
        );
    }

    // packages-ai-0210: it should call logWarnings with aggregated warnings from multiple calls.
    #[test]
    fn generate_image_calls_log_warnings_with_aggregated_warnings() {
        let warning1 = Warning::Other {
            message: "Warning from call 1".to_string(),
        };
        let warning2 = Warning::Other {
            message: "Warning from call 2".to_string(),
        };
        let model = RecordingImageModel::new(vec![
            base64_image_result(png_base64()).with_warning(warning1.clone()),
            base64_image_result(jpeg_base64()).with_warning(warning2.clone()),
        ])
        .with_max_images_per_call(1);
        take_log_warning_calls_for_tests();

        poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day").with_n(2),
        ))
        .expect("image generation succeeds");

        assert_eq!(
            take_log_warning_calls_for_tests(),
            vec![
                LogWarningsOptions::new(vec![warning1, warning2])
                    .with_scope("test-provider", "image-test")
            ]
        );
    }

    // packages-ai-0211: it should call logWarnings with empty array when no warnings are present.
    #[test]
    fn generate_image_calls_log_warnings_with_empty_array_when_no_warnings() {
        let model = RecordingImageModel::new(vec![base64_image_result(png_base64())]);
        take_log_warning_calls_for_tests();

        poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        assert_eq!(
            take_log_warning_calls_for_tests(),
            vec![LogWarningsOptions::new(Vec::new()).with_scope("test-provider", "image-test")]
        );
    }

    // packages-ai-0212: it should return generated images with correct mime types.
    #[test]
    fn generate_image_returns_images_with_correct_mime_types() {
        let model = RecordingImageModel::new(vec![ImageModelResult::new(
            vec![
                FileDataContent::Base64(png_base64().to_string()),
                FileDataContent::Base64(jpeg_base64().to_string()),
            ],
            image_response("test-model-id"),
        )]);

        let result = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        assert_eq!(
            result
                .images
                .iter()
                .map(|image| (image.base64(), image.media_type().to_string()))
                .collect::<Vec<_>>(),
            vec![
                (png_base64().to_string(), "image/png".to_string()),
                (jpeg_base64().to_string(), "image/jpeg".to_string()),
            ]
        );
    }

    // packages-ai-0213: it should return the first image with correct mime type.
    #[test]
    fn generate_image_returns_first_image_with_correct_mime_type() {
        let model = RecordingImageModel::new(vec![ImageModelResult::new(
            vec![
                FileDataContent::Base64(png_base64().to_string()),
                FileDataContent::Base64(jpeg_base64().to_string()),
            ],
            image_response("test-model-id"),
        )]);

        let result = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        assert_eq!(result.image.base64(), png_base64());
        assert_eq!(result.image.media_type(), "image/png");
    }

    // packages-ai-0214: it should return generated images (uint8array image data).
    #[test]
    fn generate_image_returns_uint8array_images() {
        let png_bytes = crate::provider_utils::convert_base64_to_bytes(png_base64())
            .expect("png base64 decodes");
        let jpeg_bytes = crate::provider_utils::convert_base64_to_bytes(jpeg_base64())
            .expect("jpeg base64 decodes");
        let model = RecordingImageModel::new(vec![ImageModelResult::new(
            vec![
                FileDataContent::Bytes(png_bytes.clone()),
                FileDataContent::Bytes(jpeg_bytes.clone()),
            ],
            image_response("test-model-id"),
        )]);

        let result = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        assert_eq!(
            result
                .images
                .iter()
                .map(|image| (
                    image.base64(),
                    image.uint8_array().expect("image bytes decode")
                ))
                .collect::<Vec<_>>(),
            vec![
                (png_base64().to_string(), png_bytes),
                (jpeg_base64().to_string(), jpeg_bytes),
            ]
        );
    }

    // packages-ai-0215: it should generate images (when several calls are required).
    #[test]
    fn generate_image_generates_across_several_calls() {
        let model = RecordingImageModel::new(vec![
            ImageModelResult::new(
                vec![
                    FileDataContent::Base64(png_base64().to_string()),
                    FileDataContent::Base64(jpeg_base64().to_string()),
                ],
                image_response("test-model-id"),
            ),
            base64_image_result(gif_base64()),
        ])
        .with_max_images_per_call(2);

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day")
                .with_n(3)
                .with_size("1024x1024")
                .with_aspect_ratio("16:9")
                .with_seed(12345),
        ))
        .expect("image generation succeeds");

        let calls = model.calls();
        assert_eq!(
            calls.iter().map(|call| call.n).collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert!(calls.iter().all(|call| call.files.is_none()));
        assert!(calls.iter().all(|call| call.mask.is_none()));
        assert!(calls.iter().all(|call| call.seed == Some(12345)));
        assert_eq!(
            result
                .images
                .iter()
                .map(|image| image.base64())
                .collect::<Vec<_>>(),
            vec![
                png_base64().to_string(),
                jpeg_base64().to_string(),
                gif_base64().to_string()
            ]
        );
    }

    // packages-ai-0216: it should aggregate warnings (when several calls are required).
    #[test]
    fn generate_image_aggregates_warnings_across_several_calls() {
        let model = RecordingImageModel::new(vec![
            ImageModelResult::new(
                vec![
                    FileDataContent::Base64(png_base64().to_string()),
                    FileDataContent::Base64(jpeg_base64().to_string()),
                ],
                image_response("test-model-id"),
            )
            .with_warning(Warning::Other {
                message: "1".to_string(),
            }),
            base64_image_result(gif_base64()).with_warning(Warning::Other {
                message: "2".to_string(),
            }),
        ])
        .with_max_images_per_call(2);

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day").with_n(3),
        ))
        .expect("image generation succeeds");

        assert_eq!(
            result.warnings,
            vec![
                Warning::Other {
                    message: "1".to_string(),
                },
                Warning::Other {
                    message: "2".to_string(),
                },
            ]
        );
    }

    // packages-ai-0217: test.each should generate with maxImagesPerCall = %s (sync + async).
    #[test]
    fn generate_image_queries_model_max_images_per_call_once() {
        let model = RecordingImageModel::new(vec![
            ImageModelResult::new(
                vec![
                    FileDataContent::Base64(png_base64().to_string()),
                    FileDataContent::Base64(jpeg_base64().to_string()),
                ],
                image_response("test-model-id"),
            ),
            base64_image_result(gif_base64()),
        ])
        .with_max_images_per_call(2);

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day").with_n(3),
        ))
        .expect("image generation succeeds");

        // The model's max-images limit is queried exactly once even across multiple provider calls.
        assert_eq!(model.max_images_calls(), 1);
        assert_eq!(
            model.calls().iter().map(|call| call.n).collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert_eq!(
            result
                .images
                .iter()
                .map(|image| image.base64())
                .collect::<Vec<_>>(),
            vec![
                png_base64().to_string(),
                jpeg_base64().to_string(),
                gif_base64().to_string()
            ]
        );
    }

    // packages-ai-0218: it should throw NoImageGeneratedError when no images are returned.
    #[test]
    fn generate_image_throws_no_image_generated_error() {
        let response = image_response("test-model-id");
        let model = RecordingImageModel::new(vec![ImageModelResult::new(Vec::new(), response)]);

        let error = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect_err("empty image response fails");

        assert_eq!(error.message(), "No image generated.");
        let responses = error.responses().expect("responses retained");
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].model_id, "test-model-id");
    }

    // packages-ai-0219: it should include response headers in error when no images generated.
    #[test]
    fn generate_image_includes_response_headers_in_error() {
        let response = image_response("test-model-id")
            .with_header("custom-response-header", "response-header-value")
            .with_header("user-agent", "ai/0.0.0-test");
        let model = RecordingImageModel::new(vec![ImageModelResult::new(Vec::new(), response)]);

        let error = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect_err("empty image response fails");

        let responses = error.responses().expect("responses retained");
        assert_eq!(responses.len(), 1);
        let headers = responses[0].headers.as_ref().expect("headers present");
        assert_eq!(
            headers.get("custom-response-header"),
            Some(&"response-header-value".to_string())
        );
        assert_eq!(
            headers.get("user-agent"),
            Some(&"ai/0.0.0-test".to_string())
        );
    }

    // packages-ai-0220: it should return response metadata.
    #[test]
    fn generate_image_returns_response_metadata() {
        let response = image_response("test-model").with_header("x-test", "value");
        let model = RecordingImageModel::new(vec![ImageModelResult::new(
            vec![FileDataContent::Base64(png_base64().to_string())],
            response.clone(),
        )]);

        let result = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        assert_eq!(
            result.responses,
            vec![ImageModelResponseMetadata::from_response(response)]
        );
        assert_eq!(result.responses[0].model_id, "test-model");
        assert_eq!(
            result.responses[0]
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-test")),
            Some(&"value".to_string())
        );
    }

    // packages-ai-0221: it should return provider metadata.
    #[test]
    fn generate_image_returns_provider_metadata() {
        let model = RecordingImageModel::new(vec![
            ImageModelResult::new(
                vec![
                    FileDataContent::Base64(png_base64().to_string()),
                    FileDataContent::Base64(png_base64().to_string()),
                ],
                image_response("test-model"),
            )
            .with_provider_metadata(metadata(
                "testProvider",
                vec![
                    json!({ "revisedPrompt": "test-revised-prompt" }),
                    JsonValue::Null,
                ],
            )),
        ]);

        let result = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        assert_eq!(
            result.provider_metadata.get("testProvider").unwrap().images,
            vec![
                json!({ "revisedPrompt": "test-revised-prompt" }),
                JsonValue::Null
            ]
        );
        assert!(
            result
                .provider_metadata
                .get("testProvider")
                .unwrap()
                .extra
                .is_empty()
        );
    }

    // packages-ai-0222: it should expose empty usage when provider does not report usage.
    #[test]
    fn generate_image_exposes_empty_usage_when_not_reported() {
        let model = RecordingImageModel::new(vec![base64_image_result(png_base64())]);

        let result = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        assert_eq!(result.usage, ImageModelUsage::new());
        assert_eq!(result.usage.input_tokens, None);
        assert_eq!(result.usage.output_tokens, None);
        assert_eq!(result.usage.total_tokens, None);
    }

    // packages-ai-0223: it should aggregate usage across multiple provider calls.
    #[test]
    fn generate_image_aggregates_usage_across_calls() {
        let model = RecordingImageModel::new(vec![
            base64_image_result(png_base64())
                .with_provider_metadata(metadata("testProvider", vec![JsonValue::Null]))
                .with_usage(
                    ImageModelUsage::new()
                        .with_input_tokens(10)
                        .with_output_tokens(0)
                        .with_total_tokens(10),
                ),
            base64_image_result(jpeg_base64())
                .with_provider_metadata(metadata("testProvider", vec![JsonValue::Null]))
                .with_usage(
                    ImageModelUsage::new()
                        .with_input_tokens(5)
                        .with_output_tokens(0)
                        .with_total_tokens(5),
                ),
        ])
        .with_max_images_per_call(1);

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day").with_n(2),
        ))
        .expect("image generation succeeds");

        assert_eq!(
            result
                .images
                .iter()
                .map(|image| image.base64())
                .collect::<Vec<_>>(),
            vec![png_base64().to_string(), jpeg_base64().to_string()]
        );
        assert_eq!(
            result.usage,
            ImageModelUsage::new()
                .with_input_tokens(15)
                .with_output_tokens(0)
                .with_total_tokens(15)
        );
    }

    // packages-ai-0224: it should merge provider metadata from multiple calls.
    #[test]
    fn generate_image_merges_provider_metadata_from_multiple_calls() {
        let model = RecordingImageModel::new(vec![
            base64_image_result(png_base64()).with_provider_metadata(metadata(
                "testProvider",
                vec![json!({ "revisedPrompt": "prompt-1" })],
            )),
            base64_image_result(jpeg_base64()).with_provider_metadata(metadata(
                "testProvider",
                vec![json!({ "revisedPrompt": "prompt-2" })],
            )),
        ])
        .with_max_images_per_call(1);

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day").with_n(2),
        ))
        .expect("image generation succeeds");

        assert_eq!(
            result.provider_metadata.get("testProvider").unwrap().images,
            vec![
                json!({ "revisedPrompt": "prompt-1" }),
                json!({ "revisedPrompt": "prompt-2" })
            ]
        );
    }

    // packages-ai-0225: it should merge non-image provider metadata fields.
    #[test]
    fn generate_image_merges_non_image_gateway_metadata_fields() {
        let model = RecordingImageModel::new(vec![
            base64_image_result(png_base64()).with_provider_metadata(metadata_entry(
                "gateway",
                ImageModelProviderMetadataEntry::new(Vec::new())
                    .with_extra("routing", json!({ "provider": "test1" }))
                    .with_extra("cost", json!("0.01")),
            )),
            base64_image_result(jpeg_base64()).with_provider_metadata(metadata_entry(
                "gateway",
                ImageModelProviderMetadataEntry::new(Vec::new())
                    .with_extra("routing", json!({ "provider": "test2" }))
                    .with_extra("generationId", json!("gen-123")),
            )),
        ])
        .with_max_images_per_call(1);

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day").with_n(2),
        ))
        .expect("image generation succeeds");

        let gateway = result.provider_metadata.get("gateway").unwrap();
        assert!(gateway.images.is_empty());
        assert_eq!(
            gateway.extra.get("routing"),
            Some(&json!({ "provider": "test2" }))
        );
        assert_eq!(gateway.extra.get("generationId"), Some(&json!("gen-123")));
        assert_eq!(gateway.extra.get("cost"), Some(&json!("0.01")));
    }

    // packages-ai-0226: it should drop empty images array for gateway provider.
    #[test]
    fn generate_image_drops_empty_images_for_gateway() {
        let model = RecordingImageModel::new(vec![
            base64_image_result(png_base64()).with_provider_metadata(metadata_entry(
                "gateway",
                ImageModelProviderMetadataEntry::new(Vec::new())
                    .with_extra("routing", json!({ "provider": "vertex" }))
                    .with_extra("cost", json!("0.04")),
            )),
        ]);

        let result = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        let gateway = result.provider_metadata.get("gateway").unwrap();
        // Gateway empty images are not accumulated.
        assert!(gateway.images.is_empty());
        assert_eq!(
            gateway.extra.get("routing"),
            Some(&json!({ "provider": "vertex" }))
        );
        assert_eq!(gateway.extra.get("cost"), Some(&json!("0.04")));
    }

    // packages-ai-0227: it should not drop empty images array for non-gateway providers.
    #[test]
    fn generate_image_keeps_empty_images_for_non_gateway() {
        let model = RecordingImageModel::new(vec![
            base64_image_result(png_base64()).with_provider_metadata(metadata_entry(
                "openai",
                ImageModelProviderMetadataEntry::new(Vec::new())
                    .with_extra("usage", json!({ "tokens": 100 })),
            )),
        ]);

        let result = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        let openai = result.provider_metadata.get("openai").unwrap();
        // Non-gateway providers keep their (empty) images array; only declared metadata images
        // are aggregated, so the extra `usage` field is not merged into the result.
        assert!(openai.images.is_empty());
        assert!(openai.extra.is_empty());
    }

    // packages-ai-0228: it should handle provider metadata without images field.
    #[test]
    fn generate_image_handles_gateway_metadata_without_images_field() {
        let model = RecordingImageModel::new(vec![
            base64_image_result(png_base64()).with_provider_metadata(metadata_entry(
                "gateway",
                ImageModelProviderMetadataEntry::new(Vec::new())
                    .with_extra("routing", json!({ "provider": "vertex" }))
                    .with_extra("cost", json!("0.04")),
            )),
        ]);

        let result = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        let gateway = result.provider_metadata.get("gateway").unwrap();
        assert!(gateway.images.is_empty());
        assert_eq!(
            gateway.extra.get("routing"),
            Some(&json!({ "provider": "vertex" }))
        );
        assert_eq!(gateway.extra.get("cost"), Some(&json!("0.04")));
    }

    // packages-ai-0229: it should handle undefined providerMetadata.
    #[test]
    fn generate_image_handles_undefined_provider_metadata() {
        let model = RecordingImageModel::new(vec![base64_image_result(png_base64())]);

        let result = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        assert!(result.provider_metadata.is_empty());
    }

    // packages-ai-0230: it should merge multiple providers from same call.
    #[test]
    fn generate_image_merges_multiple_providers_from_same_call() {
        let mut combined = metadata(
            "vertex",
            vec![
                json!({ "revisedPrompt": "revised-1" }),
                json!({ "revisedPrompt": "revised-2" }),
            ],
        );
        combined.insert(
            "gateway".to_string(),
            ImageModelProviderMetadataEntry::new(Vec::new())
                .with_extra("routing", json!({ "provider": "vertex" }))
                .with_extra("cost", json!("0.08")),
        );
        let model = RecordingImageModel::new(vec![
            ImageModelResult::new(
                vec![
                    FileDataContent::Base64(png_base64().to_string()),
                    FileDataContent::Base64(jpeg_base64().to_string()),
                ],
                image_response("test-model-id"),
            )
            .with_provider_metadata(combined),
        ])
        .with_max_images_per_call(2);

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day").with_n(2),
        ))
        .expect("image generation succeeds");

        let vertex = result.provider_metadata.get("vertex").unwrap();
        assert_eq!(
            vertex.images,
            vec![
                json!({ "revisedPrompt": "revised-1" }),
                json!({ "revisedPrompt": "revised-2" })
            ]
        );
        assert!(vertex.extra.is_empty());
        let gateway = result.provider_metadata.get("gateway").unwrap();
        assert!(gateway.images.is_empty());
        assert_eq!(
            gateway.extra.get("routing"),
            Some(&json!({ "provider": "vertex" }))
        );
        assert_eq!(gateway.extra.get("cost"), Some(&json!("0.08")));
    }

    // packages-ai-0231: it should merge multiple providers across multiple calls.
    #[test]
    fn generate_image_merges_multiple_providers_across_calls() {
        let mut first = metadata("vertex", vec![json!({ "revisedPrompt": "revised-1" })]);
        first.insert(
            "gateway".to_string(),
            ImageModelProviderMetadataEntry::new(Vec::new())
                .with_extra("routing", json!({ "provider": "vertex" })),
        );
        let mut second = metadata("vertex", vec![json!({ "revisedPrompt": "revised-2" })]);
        second.insert(
            "gateway".to_string(),
            ImageModelProviderMetadataEntry::new(Vec::new()).with_extra("cost", json!("0.08")),
        );
        let model = RecordingImageModel::new(vec![
            base64_image_result(png_base64()).with_provider_metadata(first),
            base64_image_result(jpeg_base64()).with_provider_metadata(second),
        ])
        .with_max_images_per_call(1);

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day").with_n(2),
        ))
        .expect("image generation succeeds");

        let vertex = result.provider_metadata.get("vertex").unwrap();
        assert_eq!(
            vertex.images,
            vec![
                json!({ "revisedPrompt": "revised-1" }),
                json!({ "revisedPrompt": "revised-2" })
            ]
        );
        let gateway = result.provider_metadata.get("gateway").unwrap();
        assert!(gateway.images.is_empty());
        assert_eq!(
            gateway.extra.get("routing"),
            Some(&json!({ "provider": "vertex" }))
        );
        assert_eq!(gateway.extra.get("cost"), Some(&json!("0.08")));
    }

    // packages-ai-0232: it should preserve null values in images array.
    #[test]
    fn generate_image_preserves_null_values_in_images_array() {
        let model = RecordingImageModel::new(vec![
            ImageModelResult::new(
                vec![
                    FileDataContent::Base64(png_base64().to_string()),
                    FileDataContent::Base64(jpeg_base64().to_string()),
                ],
                image_response("test-model-id"),
            )
            .with_provider_metadata(metadata(
                "openai",
                vec![json!({ "revisedPrompt": "revised" }), JsonValue::Null],
            )),
        ])
        .with_max_images_per_call(2);

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day").with_n(2),
        ))
        .expect("image generation succeeds");

        assert_eq!(
            result.provider_metadata.get("openai").unwrap().images,
            vec![json!({ "revisedPrompt": "revised" }), JsonValue::Null]
        );
    }

    // packages-ai-0233: it should handle complex nested metadata structures.
    #[test]
    fn generate_image_handles_complex_nested_gateway_metadata() {
        let model = RecordingImageModel::new(vec![
            base64_image_result(png_base64()).with_provider_metadata(metadata_entry(
                "gateway",
                ImageModelProviderMetadataEntry::new(Vec::new())
                    .with_extra(
                        "routing",
                        json!({
                            "provider": "vertex",
                            "attempts": [
                                { "provider": "openai", "success": false },
                                { "provider": "vertex", "success": true }
                            ]
                        }),
                    )
                    .with_extra("cost", json!("0.04"))
                    .with_extra("marketCost", json!("0.06"))
                    .with_extra("generationId", json!("gen-abc-123")),
            )),
        ]);

        let result = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        let gateway = result.provider_metadata.get("gateway").unwrap();
        assert!(gateway.images.is_empty());
        assert_eq!(
            gateway.extra.get("routing"),
            Some(&json!({
                "provider": "vertex",
                "attempts": [
                    { "provider": "openai", "success": false },
                    { "provider": "vertex", "success": true }
                ]
            }))
        );
        assert_eq!(gateway.extra.get("cost"), Some(&json!("0.04")));
        assert_eq!(gateway.extra.get("marketCost"), Some(&json!("0.06")));
        assert_eq!(
            gateway.extra.get("generationId"),
            Some(&json!("gen-abc-123"))
        );
    }

    // packages-ai-0234: it should handle empty gateway images across multiple calls.
    #[test]
    fn generate_image_handles_empty_gateway_images_across_calls() {
        let model = RecordingImageModel::new(vec![
            base64_image_result(png_base64()).with_provider_metadata(metadata_entry(
                "gateway",
                ImageModelProviderMetadataEntry::new(Vec::new())
                    .with_extra("routing", json!({ "provider": "vertex" })),
            )),
            base64_image_result(jpeg_base64()).with_provider_metadata(metadata_entry(
                "gateway",
                ImageModelProviderMetadataEntry::new(Vec::new()).with_extra("cost", json!("0.04")),
            )),
        ])
        .with_max_images_per_call(1);

        let result = poll_ready(super::generate_image(
            GenerateImageOptions::new(&model, "sunny day").with_n(2),
        ))
        .expect("image generation succeeds");

        let gateway = result.provider_metadata.get("gateway").unwrap();
        assert!(gateway.images.is_empty());
        assert_eq!(
            gateway.extra.get("routing"),
            Some(&json!({ "provider": "vertex" }))
        );
        assert_eq!(gateway.extra.get("cost"), Some(&json!("0.04")));
    }

    // packages-ai-0235: it should keep images array for gateway if non-empty.
    #[test]
    fn generate_image_keeps_non_empty_gateway_images() {
        let model = RecordingImageModel::new(vec![
            base64_image_result(png_base64()).with_provider_metadata(metadata_entry(
                "gateway",
                ImageModelProviderMetadataEntry::new(vec![json!({ "metadata": "value" })])
                    .with_extra("routing", json!({ "provider": "vertex" }))
                    .with_extra("cost", json!("0.04")),
            )),
        ]);

        let result = poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        let gateway = result.provider_metadata.get("gateway").unwrap();
        assert_eq!(gateway.images, vec![json!({ "metadata": "value" })]);
        assert_eq!(
            gateway.extra.get("routing"),
            Some(&json!({ "provider": "vertex" }))
        );
        assert_eq!(gateway.extra.get("cost"), Some(&json!("0.04")));
    }

    // packages-ai-0236: it should handle data URL with media type in prompt images.
    #[test]
    fn generate_image_handles_data_url_with_media_type() {
        let png_bytes = crate::provider_utils::convert_base64_to_bytes(png_base64())
            .expect("png base64 decodes");
        let data_url = format!("data:image/png;base64,{}", png_base64());
        let model = RecordingImageModel::new(vec![base64_image_result(png_base64())]);

        poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            GenerateImagePromptImages::new(vec![GenerateImagePromptImage::data_url(data_url)])
                .with_text("sunny day"),
        )))
        .expect("image generation succeeds");

        let calls = model.calls();
        assert!(matches!(
            calls[0].files.as_deref(),
            Some([crate::ImageModelFile::File { media_type, data: FileDataContent::Bytes(bytes), .. }])
                if media_type == "image/png" && bytes == &png_bytes
        ));
    }

    // packages-ai-0237: it should handle data URL with jpeg media type.
    #[test]
    fn generate_image_handles_data_url_with_jpeg_media_type() {
        let jpeg_bytes = crate::provider_utils::convert_base64_to_bytes(jpeg_base64())
            .expect("jpeg base64 decodes");
        let data_url = format!("data:image/jpeg;base64,{}", jpeg_base64());
        let model = RecordingImageModel::new(vec![base64_image_result(png_base64())]);

        poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            GenerateImagePromptImages::new(vec![GenerateImagePromptImage::data_url(data_url)])
                .with_text("sunny day"),
        )))
        .expect("image generation succeeds");

        let calls = model.calls();
        assert!(matches!(
            calls[0].files.as_deref(),
            Some([crate::ImageModelFile::File { media_type, data: FileDataContent::Bytes(bytes), .. }])
                if media_type == "image/jpeg" && bytes == &jpeg_bytes
        ));
    }

    // packages-ai-0238: it should handle data URL as mask.
    #[test]
    fn generate_image_handles_data_url_as_mask() {
        let png_bytes = crate::provider_utils::convert_base64_to_bytes(png_base64())
            .expect("png base64 decodes");
        let data_url = format!("data:image/png;base64,{}", png_base64());
        let model = RecordingImageModel::new(vec![base64_image_result(png_base64())]);

        poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            GenerateImagePromptImages::new(vec![GenerateImagePromptImage::base64(png_base64())])
                .with_text("sunny day")
                .with_mask(GenerateImagePromptImage::data_url(data_url)),
        )))
        .expect("image generation succeeds");

        let calls = model.calls();
        assert!(matches!(
            calls[0].mask.as_ref(),
            Some(crate::ImageModelFile::File { media_type, data: FileDataContent::Bytes(bytes), .. })
                if media_type == "image/png" && bytes == &png_bytes
        ));
    }

    // packages-ai-0239: it should detect media type from data when data URL has no media type.
    #[test]
    fn generate_image_detects_media_type_when_data_url_has_none() {
        let png_bytes = crate::provider_utils::convert_base64_to_bytes(png_base64())
            .expect("png base64 decodes");
        let data_url = format!("data:;base64,{}", png_base64());
        let model = RecordingImageModel::new(vec![base64_image_result(png_base64())]);

        poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            GenerateImagePromptImages::new(vec![GenerateImagePromptImage::data_url(data_url)])
                .with_text("sunny day"),
        )))
        .expect("image generation succeeds");

        let calls = model.calls();
        assert!(matches!(
            calls[0].files.as_deref(),
            Some([crate::ImageModelFile::File { media_type, data: FileDataContent::Bytes(bytes), .. }])
                if media_type == "image/png" && bytes == &png_bytes
        ));
    }

    // packages-ai-0240: it should handle multiple data URLs in prompt images.
    #[test]
    fn generate_image_handles_multiple_data_urls() {
        let png_bytes = crate::provider_utils::convert_base64_to_bytes(png_base64())
            .expect("png base64 decodes");
        let jpeg_bytes = crate::provider_utils::convert_base64_to_bytes(jpeg_base64())
            .expect("jpeg base64 decodes");
        let png_data_url = format!("data:image/png;base64,{}", png_base64());
        let jpeg_data_url = format!("data:image/jpeg;base64,{}", jpeg_base64());
        let model = RecordingImageModel::new(vec![base64_image_result(png_base64())]);

        poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            GenerateImagePromptImages::new(vec![
                GenerateImagePromptImage::data_url(png_data_url),
                GenerateImagePromptImage::data_url(jpeg_data_url),
            ])
            .with_text("sunny day"),
        )))
        .expect("image generation succeeds");

        let calls = model.calls();
        let files = calls[0].files.as_deref().expect("files present");
        assert_eq!(files.len(), 2);
        assert!(matches!(
            &files[0],
            crate::ImageModelFile::File { media_type, data: FileDataContent::Bytes(bytes), .. }
                if media_type == "image/png" && bytes == &png_bytes
        ));
        assert!(matches!(
            &files[1],
            crate::ImageModelFile::File { media_type, data: FileDataContent::Bytes(bytes), .. }
                if media_type == "image/jpeg" && bytes == &jpeg_bytes
        ));
    }

    // packages-ai-0241: it should handle mix of data URLs and base64 strings.
    #[test]
    fn generate_image_handles_mix_of_data_url_and_base64() {
        let png_bytes = crate::provider_utils::convert_base64_to_bytes(png_base64())
            .expect("png base64 decodes");
        let jpeg_bytes = crate::provider_utils::convert_base64_to_bytes(jpeg_base64())
            .expect("jpeg base64 decodes");
        let png_data_url = format!("data:image/png;base64,{}", png_base64());
        let model = RecordingImageModel::new(vec![base64_image_result(png_base64())]);

        poll_ready(super::generate_image(GenerateImageOptions::new(
            &model,
            GenerateImagePromptImages::new(vec![
                GenerateImagePromptImage::data_url(png_data_url),
                GenerateImagePromptImage::base64(jpeg_base64()),
            ])
            .with_text("sunny day"),
        )))
        .expect("image generation succeeds");

        let calls = model.calls();
        let files = calls[0].files.as_deref().expect("files present");
        assert_eq!(files.len(), 2);
        assert!(matches!(
            &files[0],
            crate::ImageModelFile::File { media_type, data: FileDataContent::Bytes(bytes), .. }
                if media_type == "image/png" && bytes == &png_bytes
        ));
        assert!(matches!(
            &files[1],
            crate::ImageModelFile::File { media_type, data: FileDataContent::Bytes(bytes), .. }
                if media_type == "image/jpeg" && bytes == &jpeg_bytes
        ));
    }

    // packages-ai-0242: experimental_generateImage should still work.
    #[test]
    fn experimental_generate_image_still_works() {
        let model = RecordingImageModel::new(vec![base64_image_result(png_base64())]);

        let result = poll_ready(experimental_generate_image(GenerateImageOptions::new(
            &model,
            "sunny day",
        )))
        .expect("image generation succeeds");

        assert_eq!(result.images.len(), 1);
        assert_eq!(result.image.base64(), png_base64());
    }

    // packages-ai-0243: Experimental_GenerateImageResult type should be exported.
    #[test]
    fn experimental_generate_image_result_type_is_exported() {
        let model = RecordingImageModel::new(vec![base64_image_result(png_base64())]);

        // The exported `ExperimentalGenerateImageResult` alias is usable as the result type and
        // is interchangeable with `GenerateImageResult`.
        let result: crate::ExperimentalGenerateImageResult = poll_ready(
            experimental_generate_image(GenerateImageOptions::new(&model, "sunny day")),
        )
        .expect("image generation succeeds");
        let _typed: GenerateImageResult = result.clone();

        assert_eq!(result.images.len(), 1);
        assert_eq!(result.image.base64(), png_base64());
        assert!(result.warnings.is_empty());
    }
}
