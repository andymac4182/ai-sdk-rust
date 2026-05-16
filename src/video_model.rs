use std::future::Future;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use url::Url;

use crate::file_data::FileDataContent;
use crate::headers::Headers;
use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
use crate::warning::Warning;

/// A provider-v4 video model.
///
/// The upstream TypeScript contract exposes a `maxVideosPerCall` capability
/// that may be a function returning a `PromiseLike`, plus a `doGenerate`
/// method returning a `PromiseLike<VideoModelV4Result>`. This Rust trait maps
/// those asynchronous boundaries to associated [`Future`] types without
/// introducing an async-trait dependency.
pub trait VideoModel {
    /// Future returned by [`VideoModel::max_videos_per_call`].
    type MaxVideosPerCallFuture<'a>: Future<Output = Option<usize>> + Send + 'a
    where
        Self: 'a;

    /// Future returned by [`VideoModel::do_generate`].
    type GenerateFuture<'a>: Future<Output = VideoModelResult> + Send + 'a
    where
        Self: 'a;

    /// Returns the provider/model interface version implemented by this model.
    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    /// Returns the provider identifier.
    fn provider(&self) -> &str;

    /// Returns the provider-specific model id.
    fn model_id(&self) -> &str;

    /// Returns the maximum number of videos supported in one call.
    ///
    /// `None` represents the upstream `undefined` or global-limit case.
    fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_>;

    /// Generates videos for the supplied options.
    fn do_generate(&self, options: VideoModelCallOptions) -> Self::GenerateFuture<'_>;
}

/// A video or image file used for video editing or image-to-video generation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum VideoModelFile {
    /// Raw image or video bytes, or base64-encoded media content.
    File {
        /// The IANA media type of the media file.
        #[serde(rename = "mediaType")]
        media_type: String,

        /// File data.
        data: FileDataContent,

        /// Optional provider-specific metadata/options for this file.
        #[serde(
            default,
            rename = "providerOptions",
            skip_serializing_if = "Option::is_none"
        )]
        provider_options: Option<ProviderMetadata>,
    },

    /// URL media input.
    Url {
        /// URL of the video or image file.
        url: Url,

        /// Optional provider-specific metadata/options for this file.
        #[serde(
            default,
            rename = "providerOptions",
            skip_serializing_if = "Option::is_none"
        )]
        provider_options: Option<ProviderMetadata>,
    },
}

impl VideoModelFile {
    /// Creates a raw media file input.
    pub fn file(media_type: impl Into<String>, data: FileDataContent) -> Self {
        Self::File {
            media_type: media_type.into(),
            data,
            provider_options: None,
        }
    }

    /// Creates a URL media input.
    pub fn url(url: Url) -> Self {
        Self::Url {
            url,
            provider_options: None,
        }
    }

    /// Adds provider-specific metadata/options to this media input.
    pub fn with_provider_options(mut self, provider_options: ProviderMetadata) -> Self {
        match &mut self {
            Self::File {
                provider_options: existing,
                ..
            }
            | Self::Url {
                provider_options: existing,
                ..
            } => *existing = Some(provider_options),
        }

        self
    }
}

/// Options passed to a video model provider call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoModelCallOptions {
    /// Text prompt for video generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,

    /// Number of videos to generate.
    pub n: u64,

    /// Video aspect ratio in the `{width}:{height}` format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,

    /// Video resolution in the `{width}x{height}` format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,

    /// Duration of the video in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,

    /// Frames per second for the generated video.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<f64>,

    /// Seed for video generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,

    /// Input image or video for image-to-video or editing generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<VideoModelFile>,

    /// Provider-specific options passed through to the provider.
    #[serde(default)]
    pub provider_options: ProviderOptions,

    /// Additional HTTP headers for HTTP-based providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,
}

impl VideoModelCallOptions {
    /// Creates video model call options with the required video count.
    pub fn new(n: u64) -> Self {
        Self {
            prompt: None,
            n,
            aspect_ratio: None,
            resolution: None,
            duration: None,
            fps: None,
            seed: None,
            image: None,
            provider_options: ProviderOptions::new(),
            headers: None,
        }
    }

    /// Sets the prompt for video generation.
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
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
    pub fn with_duration(mut self, duration: f64) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Sets the generated video frames per second.
    pub fn with_fps(mut self, fps: f64) -> Self {
        self.fps = Some(fps);
        self
    }

    /// Sets the video generation seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Sets the input image or video.
    pub fn with_image(mut self, image: VideoModelFile) -> Self {
        self.image = Some(image);
        self
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = provider_options;
        self
    }

    /// Adds an HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Generated video data returned by a video model.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum VideoModelVideoData {
    /// Video available at a URL.
    Url {
        /// URL of the generated video.
        url: Url,

        /// The IANA media type of the generated video.
        #[serde(rename = "mediaType")]
        media_type: String,
    },

    /// Video returned as base64-encoded content.
    Base64 {
        /// Base64-encoded video content.
        data: String,

        /// The IANA media type of the generated video.
        #[serde(rename = "mediaType")]
        media_type: String,
    },

    /// Video returned as raw bytes.
    Binary {
        /// Raw video bytes.
        data: Vec<u8>,

        /// The IANA media type of the generated video.
        #[serde(rename = "mediaType")]
        media_type: String,
    },
}

impl VideoModelVideoData {
    /// Creates generated video data by URL.
    pub fn url(url: Url, media_type: impl Into<String>) -> Self {
        Self::Url {
            url,
            media_type: media_type.into(),
        }
    }

    /// Creates generated video data from base64-encoded content.
    pub fn base64(data: impl Into<String>, media_type: impl Into<String>) -> Self {
        Self::Base64 {
            data: data.into(),
            media_type: media_type.into(),
        }
    }

    /// Creates generated video data from raw bytes.
    pub fn binary(data: Vec<u8>, media_type: impl Into<String>) -> Self {
        Self::Binary {
            data,
            media_type: media_type.into(),
        }
    }
}

/// Response information for telemetry and debugging video calls.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoModelResponse {
    /// Timestamp for the start of the generated response.
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,

    /// Provider model identifier used for the response.
    pub model_id: String,

    /// Response headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,
}

impl VideoModelResponse {
    /// Creates video response metadata.
    pub fn new(timestamp: OffsetDateTime, model_id: impl Into<String>) -> Self {
        Self {
            timestamp,
            model_id: model_id.into(),
            headers: None,
        }
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Result of a video model provider call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoModelResult {
    /// Generated videos as URLs, base64 strings, or raw bytes.
    pub videos: Vec<VideoModelVideoData>,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,

    /// Provider-specific metadata returned by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Response information for telemetry and debugging.
    pub response: VideoModelResponse,
}

impl VideoModelResult {
    /// Creates a video model result with no warnings.
    pub fn new(videos: Vec<VideoModelVideoData>, response: VideoModelResponse) -> Self {
        Self {
            videos,
            warnings: Vec::new(),
            provider_metadata: None,
            response,
        }
    }

    /// Adds a warning returned by the provider.
    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.push(warning);
        self
    }

    /// Adds provider-specific metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{
        VideoModel, VideoModelCallOptions, VideoModelFile, VideoModelResponse, VideoModelResult,
        VideoModelVideoData,
    };
    use crate::file_data::FileDataContent;
    use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
    use crate::warning::Warning;
    use serde_json::json;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    use time::OffsetDateTime;
    use url::Url;

    struct StaticVideoModel;

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
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "video-test"
        }

        fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
            ready(Some(1))
        }

        fn do_generate(&self, _options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
            let response_timestamp = OffsetDateTime::parse(
                "2024-01-02T03:04:05Z",
                &time::format_description::well_known::Rfc3339,
            )
            .expect("timestamp parses");

            ready(VideoModelResult::new(
                vec![VideoModelVideoData::base64("AAAAIGZ0eXBtcDQy", "video/mp4")],
                VideoModelResponse::new(response_timestamp, "video-test"),
            ))
        }
    }

    fn poll_ready<T>(mut future: Ready<T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("std::future::Ready never returns pending"),
        }
    }

    #[test]
    fn call_options_serializes_upstream_shape_with_image_and_provider_options() {
        let file_provider_options: ProviderMetadata = serde_json::from_value(json!({
            "fal": {
                "purpose": "first-frame"
            }
        }))
        .expect("file provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "fal": {
                "loop": true,
                "motionStrength": 0.8
            }
        }))
        .expect("provider options deserialize");

        let options = VideoModelCallOptions::new(1)
            .with_prompt("Animate the skyline at dusk")
            .with_aspect_ratio("16:9")
            .with_resolution("1280x720")
            .with_duration(5.0)
            .with_fps(24.0)
            .with_seed(12345)
            .with_image(
                VideoModelFile::file(
                    "image/png",
                    FileDataContent::Base64("iVBORw0KGgo=".to_string()),
                )
                .with_provider_options(file_provider_options),
            )
            .with_provider_options(provider_options)
            .with_header("x-request-id", "req_123");

        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "prompt": "Animate the skyline at dusk",
                "n": 1,
                "aspectRatio": "16:9",
                "resolution": "1280x720",
                "duration": 5.0,
                "fps": 24.0,
                "seed": 12345,
                "image": {
                    "type": "file",
                    "mediaType": "image/png",
                    "data": "iVBORw0KGgo=",
                    "providerOptions": {
                        "fal": {
                            "purpose": "first-frame"
                        }
                    }
                },
                "providerOptions": {
                    "fal": {
                        "loop": true,
                        "motionStrength": 0.8
                    }
                },
                "headers": {
                    "x-request-id": "req_123"
                }
            })
        );
    }

    #[test]
    fn call_options_deserializes_minimal_required_fields_with_empty_provider_options() {
        let options: VideoModelCallOptions = serde_json::from_value(json!({
            "n": 1
        }))
        .expect("call options deserialize");

        assert_eq!(options, VideoModelCallOptions::new(1));
        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "n": 1,
                "providerOptions": {}
            })
        );
    }

    #[test]
    fn video_model_trait_exposes_upstream_v4_identity_capability_and_generate_boundary() {
        let model = StaticVideoModel;
        let options = VideoModelCallOptions::new(1).with_prompt("A generated video");

        let max_videos_per_call = poll_ready(model.max_videos_per_call());
        let result = poll_ready(model.do_generate(options));

        assert_eq!(model.specification_version(), SpecificationVersion::V4);
        assert_eq!(model.provider(), "test-provider");
        assert_eq!(model.model_id(), "video-test");
        assert_eq!(max_videos_per_call, Some(1));
        assert_eq!(
            result.videos,
            vec![VideoModelVideoData::base64("AAAAIGZ0eXBtcDQy", "video/mp4")]
        );
    }

    #[test]
    fn result_serializes_videos_response_metadata_and_warnings() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "fal": {
                "videos": [
                    {
                        "duration": 5.0,
                        "fps": 24,
                        "width": 1280,
                        "height": 720
                    }
                ]
            }
        }))
        .expect("provider metadata deserialize");
        let response_timestamp = OffsetDateTime::parse(
            "2024-01-02T03:04:05Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("timestamp parses");

        let result = VideoModelResult::new(
            vec![
                VideoModelVideoData::url(
                    Url::parse("https://example.com/video.mp4").expect("video URL is valid"),
                    "video/mp4",
                ),
                VideoModelVideoData::base64("AAAAIGZ0eXBtcDQy", "video/mp4"),
                VideoModelVideoData::binary(vec![0, 1, 2, 3], "video/webm"),
            ],
            VideoModelResponse::new(response_timestamp, "fal-video")
                .with_header("x-ratelimit-remaining", "99"),
        )
        .with_provider_metadata(provider_metadata)
        .with_warning(Warning::Unsupported {
            feature: "fps".to_string(),
            details: Some("The selected model uses its default fps.".to_string()),
        });

        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "videos": [
                    {
                        "type": "url",
                        "url": "https://example.com/video.mp4",
                        "mediaType": "video/mp4"
                    },
                    {
                        "type": "base64",
                        "data": "AAAAIGZ0eXBtcDQy",
                        "mediaType": "video/mp4"
                    },
                    {
                        "type": "binary",
                        "data": [0, 1, 2, 3],
                        "mediaType": "video/webm"
                    }
                ],
                "warnings": [
                    {
                        "type": "unsupported",
                        "feature": "fps",
                        "details": "The selected model uses its default fps."
                    }
                ],
                "providerMetadata": {
                    "fal": {
                        "videos": [
                            {
                                "duration": 5.0,
                                "fps": 24,
                                "width": 1280,
                                "height": 720
                            }
                        ]
                    }
                },
                "response": {
                    "timestamp": "2024-01-02T03:04:05Z",
                    "modelId": "fal-video",
                    "headers": {
                        "x-ratelimit-remaining": "99"
                    }
                }
            })
        );
    }

    #[test]
    fn result_deserializes_empty_warnings_and_omits_optional_fields() {
        let result: VideoModelResult = serde_json::from_value(json!({
            "videos": [
                {
                    "type": "url",
                    "url": "https://example.com/video.mp4",
                    "mediaType": "video/mp4"
                }
            ],
            "warnings": [],
            "response": {
                "timestamp": "2024-01-02T03:04:05Z",
                "modelId": "fal-video"
            }
        }))
        .expect("result deserializes");
        let response_timestamp = OffsetDateTime::parse(
            "2024-01-02T03:04:05Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("timestamp parses");

        assert_eq!(
            result,
            VideoModelResult::new(
                vec![VideoModelVideoData::url(
                    Url::parse("https://example.com/video.mp4").expect("video URL is valid"),
                    "video/mp4",
                )],
                VideoModelResponse::new(response_timestamp, "fal-video"),
            )
        );
        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "videos": [
                    {
                        "type": "url",
                        "url": "https://example.com/video.mp4",
                        "mediaType": "video/mp4"
                    }
                ],
                "warnings": [],
                "response": {
                    "timestamp": "2024-01-02T03:04:05Z",
                    "modelId": "fal-video"
                }
            })
        );
    }
}
