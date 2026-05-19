use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::VERSION;
use crate::file_data::FileDataContent;
use crate::generate_text::GeneratedFile;
use crate::headers::Headers;
use crate::json::JsonValue;
use crate::provider::{ProviderMetadata, ProviderOptions};
use crate::provider_utils::{
    DownloadError, DownloadedBlob, convert_base64_to_bytes, detect_media_type,
    with_user_agent_suffix,
};
use crate::video_model::{
    NoVideoGeneratedError, VideoModel, VideoModelCallOptions, VideoModelFile,
    VideoModelResponseMetadata, VideoModelResult, VideoModelVideoData,
};
use crate::warning::Warning;

/// Image input for high-level video generation prompts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateVideoPromptImage {
    /// Input image content. String content follows upstream semantics: HTTP(S)
    /// strings are URLs, data URLs are decoded, and other strings are treated
    /// as base64 image data.
    pub image: FileDataContent,

    /// Optional text prompt to send alongside the image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl GenerateVideoPromptImage {
    /// Creates an image-to-video prompt from image content.
    pub fn new(image: impl Into<FileDataContent>) -> Self {
        Self {
            image: image.into(),
            text: None,
        }
    }

    /// Creates an image-to-video prompt from URL text.
    pub fn url(url: Url) -> Self {
        Self::new(FileDataContent::Base64(url.to_string()))
    }

    /// Creates an image-to-video prompt from a data URL string.
    pub fn data_url(data_url: impl Into<String>) -> Self {
        Self::new(FileDataContent::Base64(data_url.into()))
    }

    /// Creates an image-to-video prompt from base64 image data.
    pub fn base64(base64: impl Into<String>) -> Self {
        Self::new(FileDataContent::Base64(base64.into()))
    }

    /// Creates an image-to-video prompt from raw bytes.
    pub fn bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self::new(FileDataContent::Bytes(bytes.into()))
    }

    /// Adds text to the image-to-video prompt.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }
}

/// Prompt accepted by high-level `generate_video`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum GenerateVideoPrompt {
    /// Plain text video-generation prompt.
    Text(String),

    /// Image-to-video prompt with optional text.
    Image(GenerateVideoPromptImage),
}

impl GenerateVideoPrompt {
    /// Creates a plain text prompt.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// Creates an image-to-video prompt.
    pub fn image(image: GenerateVideoPromptImage) -> Self {
        Self::Image(image)
    }
}

impl From<String> for GenerateVideoPrompt {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for GenerateVideoPrompt {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<GenerateVideoPromptImage> for GenerateVideoPrompt {
    fn from(image: GenerateVideoPromptImage) -> Self {
        Self::Image(image)
    }
}

/// Future returned by a video download function.
pub type GenerateVideoDownloadFuture =
    Pin<Box<dyn Future<Output = Result<DownloadedBlob, DownloadError>> + Send>>;

/// Function used to download provider-returned URL videos.
pub type GenerateVideoDownloadFunction =
    dyn Fn(Url) -> GenerateVideoDownloadFuture + Send + Sync + 'static;

/// Runtime download callback used for provider-returned URL videos.
#[derive(Clone)]
pub struct GenerateVideoDownload {
    download: Arc<GenerateVideoDownloadFunction>,
}

impl GenerateVideoDownload {
    /// Creates a URL-video download callback.
    pub fn new<F, Fut>(download: F) -> Self
    where
        F: Fn(Url) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<DownloadedBlob, DownloadError>> + Send + 'static,
    {
        Self {
            download: Arc::new(move |url| Box::pin(download(url))),
        }
    }

    /// Downloads a generated URL video.
    pub fn download(&self, url: Url) -> GenerateVideoDownloadFuture {
        (self.download)(url)
    }
}

impl fmt::Debug for GenerateVideoDownload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GenerateVideoDownload")
            .finish_non_exhaustive()
    }
}

/// Error returned by high-level video generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GenerateVideoError {
    /// The model produced no video files.
    NoVideoGenerated(NoVideoGeneratedError),

    /// A provider-returned URL video could not be downloaded.
    Download(DownloadError),
}

impl GenerateVideoError {
    /// Returns the inner no-video error, when present.
    pub fn as_no_video_generated(&self) -> Option<&NoVideoGeneratedError> {
        match self {
            Self::NoVideoGenerated(error) => Some(error),
            Self::Download(_) => None,
        }
    }

    /// Returns the inner download error, when present.
    pub fn as_download_error(&self) -> Option<&DownloadError> {
        match self {
            Self::NoVideoGenerated(_) => None,
            Self::Download(error) => Some(error),
        }
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        match self {
            Self::NoVideoGenerated(error) => error.message(),
            Self::Download(error) => error.message(),
        }
    }
}

impl fmt::Display for GenerateVideoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoVideoGenerated(error) => error.fmt(formatter),
            Self::Download(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for GenerateVideoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoVideoGenerated(error) => Some(error),
            Self::Download(error) => Some(error),
        }
    }
}

impl From<NoVideoGeneratedError> for GenerateVideoError {
    fn from(error: NoVideoGeneratedError) -> Self {
        Self::NoVideoGenerated(error)
    }
}

impl From<DownloadError> for GenerateVideoError {
    fn from(error: DownloadError) -> Self {
        Self::Download(error)
    }
}

/// Options for a high-level `generate_video` call.
pub struct GenerateVideoOptions<'a, M: VideoModel + ?Sized> {
    /// Video model used for the call.
    pub model: &'a M,

    /// Prompt that should be used to generate the video.
    pub prompt: GenerateVideoPrompt,

    /// Number of videos to generate.
    pub n: u64,

    /// Maximum number of videos to request in one provider call.
    pub max_videos_per_call: Option<usize>,

    /// Video aspect ratio in the `{width}:{height}` format.
    pub aspect_ratio: Option<String>,

    /// Video resolution in the `{width}x{height}` format.
    pub resolution: Option<String>,

    /// Duration of the video in seconds.
    pub duration: Option<f64>,

    /// Frames per second for the generated video.
    pub fps: Option<f64>,

    /// Seed for video generation.
    pub seed: Option<u64>,

    /// Provider-specific options passed through to the model.
    pub provider_options: Option<ProviderOptions>,

    /// Additional HTTP headers for HTTP-based providers.
    pub headers: Option<Headers>,

    /// Download function used when providers return URL videos.
    pub download: Option<GenerateVideoDownload>,
}

impl<'a, M: VideoModel + ?Sized> GenerateVideoOptions<'a, M> {
    /// Creates options for a high-level `generate_video` call.
    pub fn new(model: &'a M, prompt: impl Into<GenerateVideoPrompt>) -> Self {
        Self {
            model,
            prompt: prompt.into(),
            n: 1,
            max_videos_per_call: None,
            aspect_ratio: None,
            resolution: None,
            duration: None,
            fps: None,
            seed: None,
            provider_options: None,
            headers: None,
            download: None,
        }
    }

    /// Sets the number of videos to generate.
    pub const fn with_n(mut self, n: u64) -> Self {
        self.n = n;
        self
    }

    /// Sets the maximum number of videos to request per provider call.
    pub const fn with_max_videos_per_call(mut self, max_videos_per_call: usize) -> Self {
        self.max_videos_per_call = Some(max_videos_per_call);
        self
    }

    /// Sets the generated video aspect ratio.
    pub fn with_aspect_ratio(mut self, aspect_ratio: impl Into<String>) -> Self {
        self.aspect_ratio = Some(aspect_ratio.into());
        self
    }

    /// Sets the generated video resolution.
    pub fn with_resolution(mut self, resolution: impl Into<String>) -> Self {
        self.resolution = Some(resolution.into());
        self
    }

    /// Sets the generated video duration in seconds.
    pub const fn with_duration(mut self, duration: f64) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Sets the generated video frames per second.
    pub const fn with_fps(mut self, fps: f64) -> Self {
        self.fps = Some(fps);
        self
    }

    /// Sets the video generation seed.
    pub const fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
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

    /// Sets the download callback for provider-returned URL videos.
    pub fn with_download(mut self, download: GenerateVideoDownload) -> Self {
        self.download = Some(download);
        self
    }
}

/// Result of a high-level `generate_video` call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateVideoResult {
    /// The first video that was generated.
    pub video: GeneratedFile,

    /// All generated videos.
    pub videos: Vec<GeneratedFile>,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,

    /// Response metadata from provider calls.
    pub responses: Vec<VideoModelResponseMetadata>,

    /// Provider-specific metadata aggregated across provider calls.
    pub provider_metadata: ProviderMetadata,
}

impl GenerateVideoResult {
    /// Creates a high-level video-generation result.
    pub fn new(
        videos: Vec<GeneratedFile>,
        warnings: Vec<Warning>,
        responses: Vec<VideoModelResponseMetadata>,
        provider_metadata: ProviderMetadata,
    ) -> Result<Self, NoVideoGeneratedError> {
        let video = videos
            .first()
            .cloned()
            .ok_or_else(|| NoVideoGeneratedError::new(responses.clone()))?;

        Ok(Self {
            video,
            videos,
            warnings,
            responses,
            provider_metadata,
        })
    }
}

/// Upstream-compatible experimental result alias for [`GenerateVideoResult`].
pub type ExperimentalGenerateVideoResult = GenerateVideoResult;

/// Generates videos using a video model.
pub async fn generate_video<M: VideoModel + ?Sized>(
    options: GenerateVideoOptions<'_, M>,
) -> Result<GenerateVideoResult, GenerateVideoError> {
    let GenerateVideoOptions {
        model,
        prompt,
        n,
        max_videos_per_call,
        aspect_ratio,
        resolution,
        duration,
        fps,
        seed,
        provider_options,
        headers,
        download,
    } = options;

    let headers = headers_with_ai_user_agent(headers);
    let max_videos_per_call = match max_videos_per_call {
        Some(max_videos_per_call) => max_videos_per_call,
        None => model.max_videos_per_call().await.unwrap_or(1),
    }
    .max(1);
    let normalized_prompt = normalize_prompt(&prompt);

    let mut videos = Vec::new();
    let mut warnings = Vec::new();
    let mut responses = Vec::new();
    let mut provider_metadata = ProviderMetadata::new();

    for video_count in video_call_counts(n, max_videos_per_call) {
        let VideoModelResult {
            videos: call_videos,
            warnings: call_warnings,
            provider_metadata: call_provider_metadata,
            response,
        } = model
            .do_generate(VideoModelCallOptions {
                prompt: normalized_prompt.prompt.clone(),
                n: video_count,
                aspect_ratio: aspect_ratio.clone(),
                resolution: resolution.clone(),
                duration,
                fps,
                seed,
                image: normalized_prompt.image.clone(),
                provider_options: provider_options.clone().unwrap_or_default(),
                abort_signal: None,
                headers: Some(headers.clone()),
            })
            .await;

        let response_metadata =
            VideoModelResponseMetadata::from_response(response, call_provider_metadata.clone());

        for video in call_videos {
            videos.push(resolve_video_data(video, download.as_ref()).await?);
        }

        warnings.extend(call_warnings);
        responses.push(response_metadata);

        if let Some(call_provider_metadata) = call_provider_metadata {
            merge_provider_metadata(&mut provider_metadata, call_provider_metadata);
        }
    }

    GenerateVideoResult::new(videos, warnings, responses, provider_metadata).map_err(Into::into)
}

/// Upstream-compatible experimental alias for [`generate_video`].
pub async fn experimental_generate_video<M: VideoModel + ?Sized>(
    options: GenerateVideoOptions<'_, M>,
) -> Result<ExperimentalGenerateVideoResult, GenerateVideoError> {
    generate_video(options).await
}

#[derive(Clone)]
struct NormalizedPrompt {
    prompt: Option<String>,
    image: Option<VideoModelFile>,
}

fn normalize_prompt(prompt: &GenerateVideoPrompt) -> NormalizedPrompt {
    match prompt {
        GenerateVideoPrompt::Text(prompt) => NormalizedPrompt {
            prompt: Some(prompt.clone()),
            image: None,
        },
        GenerateVideoPrompt::Image(prompt) => NormalizedPrompt {
            prompt: prompt.text.clone(),
            image: Some(video_model_file_from_prompt_image(&prompt.image)),
        },
    }
}

fn video_model_file_from_prompt_image(image: &FileDataContent) -> VideoModelFile {
    match image {
        FileDataContent::Base64(value) => {
            if (value.starts_with("http://") || value.starts_with("https://"))
                && let Ok(url) = Url::parse(value)
            {
                return VideoModelFile::url(url);
            }

            if value.starts_with("data:") {
                return video_model_file_from_data_url(value);
            }

            let data = convert_base64_to_bytes(value)
                .map(FileDataContent::Bytes)
                .unwrap_or_else(|_| FileDataContent::Base64(value.clone()));

            video_model_file_from_data(data, None)
        }
        FileDataContent::Bytes(bytes) => {
            video_model_file_from_data(FileDataContent::Bytes(bytes.clone()), None)
        }
    }
}

fn video_model_file_from_data_url(data_url: &str) -> VideoModelFile {
    let Some((header, base64_content)) = data_url.split_once(',') else {
        return video_model_file_from_data(FileDataContent::Base64(data_url.to_string()), None);
    };

    let media_type = header
        .strip_prefix("data:")
        .and_then(|header| header.split(';').next())
        .filter(|media_type| !media_type.is_empty())
        .map(str::to_string);

    let data = convert_base64_to_bytes(base64_content)
        .map(FileDataContent::Bytes)
        .unwrap_or_else(|_| FileDataContent::Base64(base64_content.to_string()));

    video_model_file_from_data(data, media_type)
}

fn video_model_file_from_data(data: FileDataContent, media_type: Option<String>) -> VideoModelFile {
    let media_type = media_type.unwrap_or_else(|| {
        detect_media_type(&data, Some("image"))
            .unwrap_or("image/png")
            .to_string()
    });

    VideoModelFile::file(media_type, data)
}

async fn resolve_video_data(
    video: VideoModelVideoData,
    download: Option<&GenerateVideoDownload>,
) -> Result<GeneratedFile, GenerateVideoError> {
    match video {
        VideoModelVideoData::Url { url, media_type } => {
            let Some(download) = download else {
                return Err(DownloadError::new(
                    url.to_string(),
                    "URL video requires a download function",
                )
                .into());
            };

            let blob = download.download(url).await?;
            let data = FileDataContent::Bytes(blob.data);
            let media_type = usable_media_type(&media_type)
                .or_else(|| blob.media_type.as_deref().and_then(usable_media_type))
                .map(str::to_string)
                .unwrap_or_else(|| {
                    detect_media_type(&data, Some("video"))
                        .unwrap_or("video/mp4")
                        .to_string()
                });

            Ok(GeneratedFile::new(media_type, data))
        }
        VideoModelVideoData::Base64 { data, media_type } => {
            let media_type = usable_video_media_type_or_default(&media_type);
            Ok(GeneratedFile::from_base64(media_type, data))
        }
        VideoModelVideoData::Binary { data, media_type } => {
            let data = FileDataContent::Bytes(data);
            let media_type = usable_media_type(&media_type)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    detect_media_type(&data, Some("video"))
                        .unwrap_or("video/mp4")
                        .to_string()
                });

            Ok(GeneratedFile::new(media_type, data))
        }
    }
}

fn usable_media_type(media_type: &str) -> Option<&str> {
    if media_type.is_empty() || media_type == "application/octet-stream" {
        None
    } else {
        Some(media_type)
    }
}

fn usable_video_media_type_or_default(media_type: &str) -> String {
    usable_media_type(media_type)
        .unwrap_or("video/mp4")
        .to_string()
}

fn video_call_counts(n: u64, max_videos_per_call: usize) -> Vec<u64> {
    if n == 0 {
        return Vec::new();
    }

    let max_videos_per_call =
        u64::try_from(max_videos_per_call).expect("usize fits into u64 on supported platforms");
    let call_count = n.div_ceil(max_videos_per_call);

    (0..call_count)
        .map(|index| {
            if index + 1 < call_count {
                max_videos_per_call
            } else {
                let remainder = n % max_videos_per_call;
                if remainder == 0 {
                    max_videos_per_call
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

fn merge_provider_metadata(
    provider_metadata: &mut ProviderMetadata,
    call_provider_metadata: ProviderMetadata,
) {
    for (provider_name, metadata) in call_provider_metadata {
        let Some(existing_metadata) = provider_metadata.remove(&provider_name) else {
            provider_metadata.insert(provider_name, metadata);
            continue;
        };

        let existing_videos = existing_metadata
            .get("videos")
            .and_then(JsonValue::as_array)
            .cloned();
        let new_videos = metadata
            .get("videos")
            .and_then(JsonValue::as_array)
            .cloned();

        let mut merged_metadata = existing_metadata;
        merged_metadata.extend(metadata);

        if let (Some(mut existing_videos), Some(new_videos)) = (existing_videos, new_videos) {
            existing_videos.extend(new_videos);
            merged_metadata.insert("videos".to_string(), JsonValue::Array(existing_videos));
        }

        provider_metadata.insert(provider_name, merged_metadata);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ExperimentalGenerateVideoResult, GenerateVideoDownload, GenerateVideoOptions,
        GenerateVideoPrompt, GenerateVideoPromptImage, GenerateVideoResult,
        experimental_generate_video, generate_video,
    };
    use crate::VERSION;
    use crate::file_data::FileDataContent;
    use crate::generate_text::GeneratedFile;
    use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
    use crate::provider_utils::DownloadedBlob;
    use crate::video_model::{
        VideoModel, VideoModelCallOptions, VideoModelFile, VideoModelResponse,
        VideoModelResponseMetadata, VideoModelResult, VideoModelVideoData,
    };
    use crate::warning::Warning;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};
    use url::Url;

    struct RecordingVideoModel {
        max_videos_per_call: Option<usize>,
        max_videos_calls: Mutex<usize>,
        calls: Mutex<Vec<VideoModelCallOptions>>,
        results: Mutex<VecDeque<VideoModelResult>>,
    }

    impl RecordingVideoModel {
        fn new(results: Vec<VideoModelResult>) -> Self {
            Self {
                max_videos_per_call: None,
                max_videos_calls: Mutex::new(0),
                calls: Mutex::new(Vec::new()),
                results: Mutex::new(results.into()),
            }
        }

        fn with_max_videos_per_call(mut self, max_videos_per_call: usize) -> Self {
            self.max_videos_per_call = Some(max_videos_per_call);
            self
        }

        fn calls(&self) -> Vec<VideoModelCallOptions> {
            self.calls
                .lock()
                .expect("calls lock is not poisoned")
                .clone()
        }

        fn max_videos_calls(&self) -> usize {
            *self
                .max_videos_calls
                .lock()
                .expect("max-videos lock is not poisoned")
        }
    }

    impl VideoModel for RecordingVideoModel {
        type MaxVideosPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a;

        type GenerateFuture<'a>
            = Ready<VideoModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "video-test"
        }

        fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
            *self
                .max_videos_calls
                .lock()
                .expect("max-videos lock is not poisoned") += 1;

            ready(self.max_videos_per_call)
        }

        fn do_generate(&self, options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
            self.calls
                .lock()
                .expect("calls lock is not poisoned")
                .push(options);

            ready(
                self.results
                    .lock()
                    .expect("results lock is not poisoned")
                    .pop_front()
                    .unwrap_or_else(|| {
                        VideoModelResult::new(Vec::new(), video_response("fallback"))
                    }),
            )
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

    fn video_response(model_id: &str) -> VideoModelResponse {
        VideoModelResponse::new(
            OffsetDateTime::parse("2024-01-02T03:04:05Z", &Rfc3339).expect("timestamp parses"),
            model_id,
        )
    }

    fn mp4_base64() -> &'static str {
        "AAAAIGZ0eXBpc29tAAACAGlzb21pc28yYXZjMW1wNDE="
    }

    fn webm_base64() -> &'static str {
        "GkXfo59ChoEBQveBAULygQRC84EIQoKEd2Vib"
    }

    fn png_base64() -> &'static str {
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAACklEQVR4nGMAAQAABQABDQottAAAAABJRU5ErkJggg=="
    }

    fn provider_metadata(provider: &str, value: serde_json::Value) -> ProviderMetadata {
        serde_json::from_value(json!({ provider: value })).expect("provider metadata deserializes")
    }

    #[test]
    fn result_serializes_upstream_shape() {
        let response_metadata = VideoModelResponseMetadata::from_response(
            video_response("video-test").with_header("x-response-id", "res_1"),
            Some(provider_metadata(
                "gateway",
                json!({
                    "videos": [
                        {
                            "seed": 123
                        }
                    ]
                }),
            )),
        );
        let provider_metadata = provider_metadata(
            "gateway",
            json!({
                "videos": [
                    {
                        "seed": 123
                    }
                ],
                "routing": {
                    "provider": "fal"
                }
            }),
        );

        let result = GenerateVideoResult::new(
            vec![
                GeneratedFile::from_base64("video/mp4", mp4_base64()),
                GeneratedFile::from_base64("video/webm", webm_base64()),
            ],
            vec![Warning::Other {
                message: "setting ignored".to_string(),
            }],
            vec![response_metadata],
            provider_metadata,
        )
        .expect("result has video");

        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "video": {
                    "base64": mp4_base64(),
                    "mediaType": "video/mp4"
                },
                "videos": [
                    {
                        "base64": mp4_base64(),
                        "mediaType": "video/mp4"
                    },
                    {
                        "base64": webm_base64(),
                        "mediaType": "video/webm"
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
                        "modelId": "video-test",
                        "headers": {
                            "x-response-id": "res_1"
                        },
                        "providerMetadata": {
                            "gateway": {
                                "videos": [
                                    {
                                        "seed": 123
                                    }
                                ]
                            }
                        }
                    }
                ],
                "providerMetadata": {
                    "gateway": {
                        "videos": [
                            {
                                "seed": 123
                            }
                        ],
                        "routing": {
                            "provider": "fal"
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn generate_video_forwards_normalized_prompt_options_headers_and_provider_options() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "fal": {
                "loop": true
            }
        }))
        .expect("provider options deserialize");
        let model = RecordingVideoModel::new(vec![VideoModelResult::new(
            vec![VideoModelVideoData::base64(mp4_base64(), "video/mp4")],
            video_response("video-test"),
        )]);

        let result = poll_ready(generate_video(
            GenerateVideoOptions::new(
                &model,
                GenerateVideoPromptImage::data_url(format!(
                    "data:image/png;base64,{}",
                    png_base64()
                ))
                .with_text("image to video"),
            )
            .with_aspect_ratio("16:9")
            .with_resolution("1920x1080")
            .with_duration(5.0)
            .with_fps(30.0)
            .with_seed(12345)
            .with_provider_options(provider_options.clone())
            .with_header("custom-request-header", "request-header-value"),
        ))
        .expect("video generation succeeds");

        assert_eq!(result.video.media_type(), "video/mp4");

        let calls = model.calls();
        assert_eq!(calls.len(), 1);
        let call = &calls[0];
        assert_eq!(call.prompt.as_deref(), Some("image to video"));
        assert_eq!(call.n, 1);
        assert_eq!(call.aspect_ratio.as_deref(), Some("16:9"));
        assert_eq!(call.resolution.as_deref(), Some("1920x1080"));
        assert_eq!(call.duration, Some(5.0));
        assert_eq!(call.fps, Some(30.0));
        assert_eq!(call.seed, Some(12345));
        assert_eq!(call.provider_options, provider_options);
        assert_eq!(
            call.headers
                .as_ref()
                .and_then(|headers| headers.get("user-agent")),
            Some(&format!("ai/{VERSION}"))
        );
        assert_eq!(
            call.headers
                .as_ref()
                .and_then(|headers| headers.get("custom-request-header")),
            Some(&"request-header-value".to_string())
        );
        assert!(matches!(
            call.image.as_ref(),
            Some(VideoModelFile::File {
                media_type,
                data: FileDataContent::Bytes(_),
                ..
            }) if media_type == "image/png"
        ));
    }

    #[test]
    fn generate_video_chunks_calls_and_aggregates_metadata() {
        let first_result = VideoModelResult::new(
            vec![VideoModelVideoData::base64(mp4_base64(), "video/mp4")],
            video_response("video-test").with_header("x-call", "1"),
        )
        .with_warning(Warning::Other {
            message: "first".to_string(),
        })
        .with_provider_metadata(provider_metadata(
            "gateway",
            json!({
                "videos": [
                    {
                        "seed": 111
                    }
                ],
                "routing": {
                    "provider": "fal"
                }
            }),
        ));
        let second_result = VideoModelResult::new(
            vec![VideoModelVideoData::base64(webm_base64(), "video/webm")],
            video_response("video-test").with_header("x-call", "2"),
        )
        .with_warning(Warning::Other {
            message: "second".to_string(),
        })
        .with_provider_metadata(provider_metadata(
            "gateway",
            json!({
                "videos": [
                    {
                        "seed": 222
                    }
                ],
                "cost": "0.08"
            }),
        ));
        let model =
            RecordingVideoModel::new(vec![first_result, second_result]).with_max_videos_per_call(1);

        let result = poll_ready(generate_video(
            GenerateVideoOptions::new(&model, "moving clouds").with_n(2),
        ))
        .expect("video generation succeeds");

        assert_eq!(model.max_videos_calls(), 1);
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
                .videos
                .iter()
                .map(GeneratedFile::base64)
                .collect::<Vec<_>>(),
            vec![mp4_base64(), webm_base64()]
        );
        assert_eq!(
            result.warnings,
            vec![
                Warning::Other {
                    message: "first".to_string()
                },
                Warning::Other {
                    message: "second".to_string()
                }
            ]
        );
        assert_eq!(
            result.provider_metadata,
            provider_metadata(
                "gateway",
                json!({
                    "videos": [
                        {
                            "seed": 111
                        },
                        {
                            "seed": 222
                        }
                    ],
                    "routing": {
                        "provider": "fal"
                    },
                    "cost": "0.08"
                })
            )
        );
        assert_eq!(result.responses.len(), 2);
        assert!(result.responses[0].provider_metadata.is_some());
        assert!(result.responses[1].provider_metadata.is_some());
    }

    #[test]
    fn generate_video_downloads_url_output_and_detects_octet_stream_media_type() {
        let downloaded_urls = Arc::new(Mutex::new(Vec::new()));
        let download = GenerateVideoDownload::new({
            let downloaded_urls = Arc::clone(&downloaded_urls);
            move |url| {
                downloaded_urls
                    .lock()
                    .expect("download urls lock is not poisoned")
                    .push(url);

                ready(Ok(DownloadedBlob::new(vec![
                    0x00, 0x00, 0x00, 0x20, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm',
                ])
                .with_media_type("application/octet-stream")))
            }
        });
        let url = Url::parse("https://example.com/video.mp4").expect("url parses");
        let model = RecordingVideoModel::new(vec![VideoModelResult::new(
            vec![VideoModelVideoData::url(
                url.clone(),
                "application/octet-stream",
            )],
            video_response("video-test"),
        )]);

        let result = poll_ready(generate_video(
            GenerateVideoOptions::new(&model, "moving clouds").with_download(download),
        ))
        .expect("video generation succeeds");

        assert_eq!(
            *downloaded_urls
                .lock()
                .expect("download urls lock is not poisoned"),
            vec![url]
        );
        assert_eq!(result.video.media_type(), "video/mp4");
        assert_eq!(
            result.video.bytes().expect("downloaded video has bytes"),
            vec![
                0x00, 0x00, 0x00, 0x20, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm',
            ]
        );
    }

    #[test]
    fn generate_video_url_output_requires_download_callback() {
        let url = Url::parse("https://example.com/video.mp4").expect("url parses");
        let model = RecordingVideoModel::new(vec![VideoModelResult::new(
            vec![VideoModelVideoData::url(url.clone(), "video/mp4")],
            video_response("video-test"),
        )]);

        let error = poll_ready(generate_video(GenerateVideoOptions::new(
            &model,
            "moving clouds",
        )))
        .expect_err("missing download function errors");

        let download_error = error
            .as_download_error()
            .expect("error is a download failure");
        assert_eq!(download_error.url(), url.as_str());
        assert_eq!(
            download_error.message(),
            "URL video requires a download function"
        );
    }

    #[test]
    fn generate_video_returns_no_video_error_with_response_metadata() {
        let response = video_response("video-test").with_header("x-response-id", "res_empty");
        let model =
            RecordingVideoModel::new(vec![VideoModelResult::new(Vec::new(), response.clone())]);

        let error = poll_ready(generate_video(GenerateVideoOptions::new(
            &model,
            "moving clouds",
        )))
        .expect_err("empty video response fails");

        let no_video = error.as_no_video_generated().expect("error is no-video");
        assert_eq!(no_video.message(), "No video generated.");
        assert_eq!(
            no_video.responses(),
            &[VideoModelResponseMetadata::from_response(response, None)]
        );
    }

    #[test]
    fn generate_video_with_zero_videos_makes_no_model_call_and_errors() {
        let model = RecordingVideoModel::new(Vec::new());

        let error = poll_ready(generate_video(
            GenerateVideoOptions::new(&model, "moving clouds").with_n(0),
        ))
        .expect_err("zero requested videos fail");

        let no_video = error.as_no_video_generated().expect("error is no-video");
        assert!(model.calls().is_empty());
        assert_eq!(no_video.responses(), &[] as &[VideoModelResponseMetadata]);
    }

    #[test]
    fn prompt_image_serializes_upstream_shape() {
        let prompt = GenerateVideoPrompt::image(
            GenerateVideoPromptImage::base64("aGVsbG8=").with_text("go"),
        );

        assert_eq!(
            serde_json::to_value(prompt).expect("prompt serializes"),
            json!({
                "image": "aGVsbG8=",
                "text": "go"
            })
        );
    }

    #[test]
    fn experimental_generate_video_alias_preserves_runtime_behavior() {
        let model = RecordingVideoModel::new(vec![VideoModelResult::new(
            vec![VideoModelVideoData::base64(mp4_base64(), "video/mp4")],
            video_response("video-test"),
        )]);

        let result: ExperimentalGenerateVideoResult = poll_ready(experimental_generate_video(
            GenerateVideoOptions::new(&model, GenerateVideoPrompt::text("moving clouds")),
        ))
        .expect("video generation succeeds");

        assert_eq!(model.specification_version(), SpecificationVersion::V4);
        assert_eq!(result.video.media_type(), "video/mp4");
    }
}
