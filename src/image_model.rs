use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use url::Url;

use crate::file_data::FileDataContent;
use crate::headers::Headers;
use crate::json::{JsonArray, JsonObject, JsonValue};
use crate::provider::{ProviderMetadata, ProviderOptions};
use crate::warning::Warning;

/// Generated image data returned by an image model.
pub type ImageModelImage = FileDataContent;

/// An image file used for image editing, variation generation, or masking.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ImageModelFile {
    /// Raw image bytes or base64-encoded image content.
    File {
        /// The IANA media type of the image file.
        #[serde(rename = "mediaType")]
        media_type: String,

        /// Image file data.
        data: FileDataContent,

        /// Optional provider-specific metadata/options for this file.
        #[serde(
            default,
            rename = "providerOptions",
            skip_serializing_if = "Option::is_none"
        )]
        provider_options: Option<ProviderMetadata>,
    },

    /// URL image input.
    Url {
        /// URL of the image file.
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

impl ImageModelFile {
    /// Creates a raw image file input.
    pub fn file(media_type: impl Into<String>, data: FileDataContent) -> Self {
        Self::File {
            media_type: media_type.into(),
            data,
            provider_options: None,
        }
    }

    /// Creates a URL image input.
    pub fn url(url: Url) -> Self {
        Self::Url {
            url,
            provider_options: None,
        }
    }

    /// Adds provider-specific metadata/options to this image input.
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

/// Options passed to an image model provider call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageModelCallOptions {
    /// Prompt for image generation. Some operations, such as upscaling, may omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,

    /// Number of images to generate.
    pub n: u64,

    /// Image size in the `{width}x{height}` format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,

    /// Image aspect ratio in the `{width}:{height}` format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,

    /// Seed for image generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,

    /// Images for editing or variation generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<ImageModelFile>>,

    /// Mask image for inpainting operations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<ImageModelFile>,

    /// Provider-specific options passed through to the provider.
    #[serde(default)]
    pub provider_options: ProviderOptions,

    /// Additional HTTP headers for HTTP-based providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,
}

impl ImageModelCallOptions {
    /// Creates image model call options with the required image count.
    pub fn new(n: u64) -> Self {
        Self {
            prompt: None,
            n,
            size: None,
            aspect_ratio: None,
            seed: None,
            files: None,
            mask: None,
            provider_options: ProviderOptions::new(),
            headers: None,
        }
    }

    /// Sets the prompt for image generation.
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
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
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Sets input images for editing or variation generation.
    pub fn with_files(mut self, files: Vec<ImageModelFile>) -> Self {
        self.files = Some(files);
        self
    }

    /// Sets the mask image for inpainting.
    pub fn with_mask(mut self, mask: ImageModelFile) -> Self {
        self.mask = Some(mask);
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

/// Provider-specific image metadata returned by an image model.
pub type ImageModelProviderMetadata = BTreeMap<String, ImageModelProviderMetadataEntry>;

/// Image metadata for a single provider.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageModelProviderMetadataEntry {
    /// Image-specific metadata values.
    pub images: JsonArray,

    /// Additional provider-specific metadata fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

impl ImageModelProviderMetadataEntry {
    /// Creates provider image metadata.
    pub fn new(images: JsonArray) -> Self {
        Self {
            images,
            extra: JsonObject::new(),
        }
    }

    /// Adds an extra provider-specific metadata field.
    pub fn with_extra(mut self, key: impl Into<String>, value: JsonValue) -> Self {
        self.extra.insert(key.into(), value);
        self
    }
}

/// Token usage for an image model call.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageModelUsage {
    /// Input prompt tokens used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,

    /// Output tokens used, if reported by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,

    /// Total tokens reported by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}

impl ImageModelUsage {
    /// Creates empty image model usage.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets input token usage.
    pub fn with_input_tokens(mut self, input_tokens: u64) -> Self {
        self.input_tokens = Some(input_tokens);
        self
    }

    /// Sets output token usage.
    pub fn with_output_tokens(mut self, output_tokens: u64) -> Self {
        self.output_tokens = Some(output_tokens);
        self
    }

    /// Sets total token usage.
    pub fn with_total_tokens(mut self, total_tokens: u64) -> Self {
        self.total_tokens = Some(total_tokens);
        self
    }
}

/// Response information for telemetry and debugging image calls.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageModelResponse {
    /// Timestamp for the start of the generated response.
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,

    /// Provider model identifier used for the response.
    pub model_id: String,

    /// Response headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,
}

impl ImageModelResponse {
    /// Creates image response metadata.
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

/// Result of an image model provider call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageModelResult {
    /// Generated images as base64-encoded strings or raw bytes.
    pub images: Vec<ImageModelImage>,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,

    /// Provider-specific metadata returned by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ImageModelProviderMetadata>,

    /// Response information for telemetry and debugging.
    pub response: ImageModelResponse,

    /// Optional token usage for the image generation call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ImageModelUsage>,
}

impl ImageModelResult {
    /// Creates an image model result with no warnings.
    pub fn new(images: Vec<ImageModelImage>, response: ImageModelResponse) -> Self {
        Self {
            images,
            warnings: Vec::new(),
            provider_metadata: None,
            response,
            usage: None,
        }
    }

    /// Adds a warning returned by the provider.
    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.push(warning);
        self
    }

    /// Adds provider-specific metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ImageModelProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }

    /// Sets token usage for the image generation call.
    pub fn with_usage(mut self, usage: ImageModelUsage) -> Self {
        self.usage = Some(usage);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ImageModelCallOptions, ImageModelFile, ImageModelProviderMetadata,
        ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResult, ImageModelUsage,
    };
    use crate::file_data::FileDataContent;
    use crate::provider::{ProviderMetadata, ProviderOptions};
    use crate::warning::Warning;
    use serde_json::json;
    use time::OffsetDateTime;
    use url::Url;

    #[test]
    fn call_options_serializes_upstream_shape_with_files_mask_and_provider_options() {
        let file_provider_options: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "purpose": "reference"
            }
        }))
        .expect("file provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "style": "vivid"
            }
        }))
        .expect("provider options deserialize");

        let options = ImageModelCallOptions::new(2)
            .with_prompt("sunny day at the beach")
            .with_size("1024x1024")
            .with_aspect_ratio("16:9")
            .with_seed(12345)
            .with_files(vec![
                ImageModelFile::file(
                    "image/png",
                    FileDataContent::Base64("iVBORw0KGgo=".to_string()),
                )
                .with_provider_options(file_provider_options),
            ])
            .with_mask(ImageModelFile::url(
                Url::parse("https://example.com/mask.png").expect("mask URL is valid"),
            ))
            .with_provider_options(provider_options)
            .with_header("x-request-id", "req_123");

        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "prompt": "sunny day at the beach",
                "n": 2,
                "size": "1024x1024",
                "aspectRatio": "16:9",
                "seed": 12345,
                "files": [
                    {
                        "type": "file",
                        "mediaType": "image/png",
                        "data": "iVBORw0KGgo=",
                        "providerOptions": {
                            "openai": {
                                "purpose": "reference"
                            }
                        }
                    }
                ],
                "mask": {
                    "type": "url",
                    "url": "https://example.com/mask.png"
                },
                "providerOptions": {
                    "openai": {
                        "style": "vivid"
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
        let options: ImageModelCallOptions = serde_json::from_value(json!({
            "n": 1
        }))
        .expect("call options deserialize");

        assert_eq!(options, ImageModelCallOptions::new(1));
        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "n": 1,
                "providerOptions": {}
            })
        );
    }

    #[test]
    fn result_serializes_images_response_usage_metadata_and_warnings() {
        let provider_metadata: ImageModelProviderMetadata = [(
            "openai".to_string(),
            ImageModelProviderMetadataEntry::new(vec![json!({
                "revisedPrompt": "A sunny beach at noon"
            })])
            .with_extra("requestId", json!("img_req_123")),
        )]
        .into_iter()
        .collect();
        let response_timestamp = OffsetDateTime::parse(
            "2024-01-02T03:04:05Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("timestamp parses");

        let result = ImageModelResult::new(
            vec![FileDataContent::Base64("iVBORw0KGgo=".to_string())],
            ImageModelResponse::new(response_timestamp, "gpt-image-1")
                .with_header("x-ratelimit-remaining", "99"),
        )
        .with_provider_metadata(provider_metadata)
        .with_usage(
            ImageModelUsage::new()
                .with_input_tokens(11)
                .with_output_tokens(22)
                .with_total_tokens(33),
        )
        .with_warning(Warning::Unsupported {
            feature: "seed".to_string(),
            details: Some("The selected model ignores seed.".to_string()),
        });

        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "images": ["iVBORw0KGgo="],
                "warnings": [
                    {
                        "type": "unsupported",
                        "feature": "seed",
                        "details": "The selected model ignores seed."
                    }
                ],
                "providerMetadata": {
                    "openai": {
                        "images": [
                            {
                                "revisedPrompt": "A sunny beach at noon"
                            }
                        ],
                        "requestId": "img_req_123"
                    }
                },
                "response": {
                    "timestamp": "2024-01-02T03:04:05Z",
                    "modelId": "gpt-image-1",
                    "headers": {
                        "x-ratelimit-remaining": "99"
                    }
                },
                "usage": {
                    "inputTokens": 11,
                    "outputTokens": 22,
                    "totalTokens": 33
                }
            })
        );
    }

    #[test]
    fn result_deserializes_empty_warnings_and_omits_optional_fields() {
        let result: ImageModelResult = serde_json::from_value(json!({
            "images": ["iVBORw0KGgo="],
            "warnings": [],
            "response": {
                "timestamp": "2024-01-02T03:04:05Z",
                "modelId": "gpt-image-1"
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
            ImageModelResult::new(
                vec![FileDataContent::Base64("iVBORw0KGgo=".to_string())],
                ImageModelResponse::new(response_timestamp, "gpt-image-1"),
            )
        );
        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "images": ["iVBORw0KGgo="],
                "warnings": [],
                "response": {
                    "timestamp": "2024-01-02T03:04:05Z",
                    "modelId": "gpt-image-1"
                }
            })
        );
    }
}
