use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::file_data::{FileDataContent, ProviderReference};
use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
use crate::warning::Warning;

/// A provider-v4 files interface.
///
/// The upstream TypeScript contract exposes an `uploadFile` method returning a
/// `PromiseLike<FilesV4UploadFileResult>`. This Rust trait maps that boundary
/// to an associated [`Future`] without introducing an async-trait dependency.
pub trait Files {
    /// Future returned by [`Files::upload_file`].
    type UploadFileFuture<'a>: Future<Output = FilesUploadFileResult> + Send + 'a
    where
        Self: 'a;

    /// Returns the provider/files interface version implemented by this interface.
    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    /// Returns the provider identifier.
    fn provider(&self) -> &str;

    /// Uploads a file and returns a provider reference for later calls.
    fn upload_file(&self, options: FilesUploadFileCallOptions) -> Self::UploadFileFuture<'_>;
}

/// File data accepted by the provider files upload interface.
///
/// Uploads accept either raw/base64 file data or inline UTF-8 text. URL and
/// provider-reference variants are intentionally not part of this contract
/// because the upstream upload-file API only accepts new file content.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FilesUploadFileData {
    /// Raw bytes or base64-encoded file content.
    Data { data: FileDataContent },

    /// Inline text file content.
    Text { text: String },
}

impl FilesUploadFileData {
    /// Creates upload data from raw bytes or base64-encoded file content.
    pub fn data(data: FileDataContent) -> Self {
        Self::Data { data }
    }

    /// Creates upload data from inline text content.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
}

/// Options for uploading a file via a provider files interface.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesUploadFileCallOptions {
    /// The file content to upload.
    pub data: FilesUploadFileData,

    /// The IANA media type of the file, such as `application/pdf`.
    pub media_type: String,

    /// The filename of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    /// Provider-specific options passed through to the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl FilesUploadFileCallOptions {
    /// Creates file upload options with the required data and media type.
    pub fn new(data: FilesUploadFileData, media_type: impl Into<String>) -> Self {
        Self {
            data,
            media_type: media_type.into(),
            filename: None,
            provider_options: None,
        }
    }

    /// Sets the filename for the uploaded file.
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

/// Result of uploading a file via a provider files interface.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesUploadFileResult {
    /// Provider-to-file-id mapping for the uploaded file.
    pub provider_reference: ProviderReference,

    /// The IANA media type of the uploaded file, if available from the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    /// The filename of the uploaded file, if available from the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    /// Provider-specific metadata returned by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Warnings from the provider.
    pub warnings: Vec<Warning>,
}

impl FilesUploadFileResult {
    /// Creates a file upload result with no warnings.
    pub fn new(provider_reference: ProviderReference) -> Self {
        Self {
            provider_reference,
            media_type: None,
            filename: None,
            provider_metadata: None,
            warnings: Vec::new(),
        }
    }

    /// Sets the uploaded file media type.
    pub fn with_media_type(mut self, media_type: impl Into<String>) -> Self {
        self.media_type = Some(media_type.into());
        self
    }

    /// Sets the uploaded file filename.
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    /// Adds provider-specific metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }

    /// Adds a warning returned by the provider.
    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.push(warning);
        self
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    use serde_json::json;

    use super::{Files, FilesUploadFileCallOptions, FilesUploadFileData, FilesUploadFileResult};
    use crate::file_data::{FileDataContent, ProviderReference};
    use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
    use crate::warning::Warning;

    struct StaticFiles;

    impl Files for StaticFiles {
        type UploadFileFuture<'a>
            = Ready<FilesUploadFileResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn upload_file(&self, options: FilesUploadFileCallOptions) -> Self::UploadFileFuture<'_> {
            let provider_reference = ProviderReference::try_from(BTreeMap::from([(
                "test-provider".to_string(),
                "file_123".to_string(),
            )]))
            .expect("provider reference is valid");

            ready(
                FilesUploadFileResult::new(provider_reference).with_media_type(options.media_type),
            )
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
    fn upload_file_call_options_serializes_data_filename_and_provider_options() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "purpose": "assistants"
            }
        }))
        .expect("provider options deserialize");

        let options = FilesUploadFileCallOptions::new(
            FilesUploadFileData::data(FileDataContent::Base64("JVBERi0xLjQ=".to_string())),
            "application/pdf",
        )
        .with_filename("spec.pdf")
        .with_provider_options(provider_options);

        assert_eq!(
            serde_json::to_value(options).expect("upload options serialize"),
            json!({
                "data": {
                    "type": "data",
                    "data": "JVBERi0xLjQ="
                },
                "mediaType": "application/pdf",
                "filename": "spec.pdf",
                "providerOptions": {
                    "openai": {
                        "purpose": "assistants"
                    }
                }
            })
        );
    }

    #[test]
    fn upload_file_call_options_deserializes_text_data_and_omits_optional_fields() {
        let options: FilesUploadFileCallOptions = serde_json::from_value(json!({
            "data": {
                "type": "text",
                "text": "hello from a text file"
            },
            "mediaType": "text/plain"
        }))
        .expect("upload options deserialize");

        assert_eq!(
            options,
            FilesUploadFileCallOptions::new(
                FilesUploadFileData::text("hello from a text file"),
                "text/plain",
            )
        );
        assert_eq!(
            serde_json::to_value(options).expect("upload options serialize"),
            json!({
                "data": {
                    "type": "text",
                    "text": "hello from a text file"
                },
                "mediaType": "text/plain"
            })
        );
    }

    #[test]
    fn files_trait_exposes_upstream_v4_identity_and_upload_boundary() {
        let files = StaticFiles;
        let result = poll_ready(files.upload_file(FilesUploadFileCallOptions::new(
            FilesUploadFileData::text("hello"),
            "text/plain",
        )));
        let expected_provider_reference = ProviderReference::try_from(BTreeMap::from([(
            "test-provider".to_string(),
            "file_123".to_string(),
        )]))
        .expect("provider reference is valid");

        assert_eq!(files.specification_version(), SpecificationVersion::V4);
        assert_eq!(files.provider(), "test-provider");
        assert_eq!(
            result,
            FilesUploadFileResult::new(expected_provider_reference).with_media_type("text/plain")
        );
    }

    #[test]
    fn upload_file_result_serializes_reference_metadata_and_warnings() {
        let provider_reference = ProviderReference::try_from(BTreeMap::from([(
            "openai".to_string(),
            "file-abc123".to_string(),
        )]))
        .expect("provider reference is valid");
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "createdAt": "2024-01-02T03:04:05Z"
            }
        }))
        .expect("provider metadata deserialize");

        let result = FilesUploadFileResult::new(provider_reference)
            .with_media_type("application/pdf")
            .with_filename("spec.pdf")
            .with_provider_metadata(provider_metadata)
            .with_warning(Warning::Compatibility {
                feature: "file-search".to_string(),
                details: Some("The provider converted the file during upload.".to_string()),
            });

        assert_eq!(
            serde_json::to_value(result).expect("upload result serializes"),
            json!({
                "providerReference": {
                    "openai": "file-abc123"
                },
                "mediaType": "application/pdf",
                "filename": "spec.pdf",
                "providerMetadata": {
                    "openai": {
                        "createdAt": "2024-01-02T03:04:05Z"
                    }
                },
                "warnings": [
                    {
                        "type": "compatibility",
                        "feature": "file-search",
                        "details": "The provider converted the file during upload."
                    }
                ]
            })
        );
    }

    #[test]
    fn upload_file_result_deserializes_empty_warnings_and_omits_optional_fields() {
        let result: FilesUploadFileResult = serde_json::from_value(json!({
            "providerReference": {
                "openai": "file-abc123"
            },
            "warnings": []
        }))
        .expect("upload result deserializes");
        let provider_reference = ProviderReference::try_from(BTreeMap::from([(
            "openai".to_string(),
            "file-abc123".to_string(),
        )]))
        .expect("provider reference is valid");

        assert_eq!(result, FilesUploadFileResult::new(provider_reference));
        assert_eq!(
            serde_json::to_value(result).expect("upload result serializes"),
            json!({
                "providerReference": {
                    "openai": "file-abc123"
                },
                "warnings": []
            })
        );
    }
}
