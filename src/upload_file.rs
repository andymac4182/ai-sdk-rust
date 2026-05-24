use serde::{Deserialize, Serialize};

use crate::file_data::FileDataContent;
use crate::files::{Files, FilesUploadFileCallOptions, FilesUploadFileData, FilesUploadFileResult};
use crate::provider::{ProviderOptions, ProviderWithFiles};
use crate::provider_utils::{convert_base64_to_bytes, detect_media_type};

/// File data accepted by the high-level `upload_file` helper.
///
/// This mirrors upstream `uploadFile`: callers can provide the tagged provider
/// upload data shape, or a raw byte/base64 value that is treated as file data.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum UploadFileData {
    /// Tagged provider upload data.
    Tagged(FilesUploadFileData),

    /// Raw file data shorthand.
    Raw(FileDataContent),
}

impl UploadFileData {
    /// Creates raw upload data from bytes or base64 content.
    pub fn raw(data: impl Into<FileDataContent>) -> Self {
        Self::Raw(data.into())
    }

    /// Creates tagged data upload content.
    pub fn data(data: impl Into<FileDataContent>) -> Self {
        Self::Tagged(FilesUploadFileData::data(data.into()))
    }

    /// Creates tagged text upload content.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Tagged(FilesUploadFileData::text(text))
    }

    /// Converts this high-level input into provider files upload data.
    pub fn into_files_upload_data(self) -> FilesUploadFileData {
        match self {
            Self::Tagged(data) => data,
            Self::Raw(data) => FilesUploadFileData::data(data),
        }
    }
}

impl From<FilesUploadFileData> for UploadFileData {
    fn from(data: FilesUploadFileData) -> Self {
        Self::Tagged(data)
    }
}

impl From<FileDataContent> for UploadFileData {
    fn from(data: FileDataContent) -> Self {
        Self::Raw(data)
    }
}

impl From<Vec<u8>> for UploadFileData {
    fn from(data: Vec<u8>) -> Self {
        Self::Raw(FileDataContent::Bytes(data))
    }
}

impl From<String> for UploadFileData {
    fn from(data: String) -> Self {
        Self::Raw(FileDataContent::Base64(data))
    }
}

impl From<&str> for UploadFileData {
    fn from(data: &str) -> Self {
        Self::Raw(FileDataContent::Base64(data.to_string()))
    }
}

/// Options for a high-level `upload_file` call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadFileOptions {
    /// The file data to upload.
    pub data: UploadFileData,

    /// Optional IANA media type. When omitted, it is inferred from bytes when
    /// possible, then falls back to text/plain for likely text and
    /// application/octet-stream otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    /// Optional filename for the uploaded file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    /// Provider-specific options passed through to the files API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl UploadFileOptions {
    /// Creates high-level file upload options.
    pub fn new(data: impl Into<UploadFileData>) -> Self {
        Self {
            data: data.into(),
            media_type: None,
            filename: None,
            provider_options: None,
        }
    }

    /// Sets the IANA media type for the upload.
    pub fn with_media_type(mut self, media_type: impl Into<String>) -> Self {
        self.media_type = Some(media_type.into());
        self
    }

    /// Sets the filename for the upload.
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    /// Adds provider-specific options for the upload.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Converts high-level options into the provider files call options.
    pub fn into_call_options(self) -> FilesUploadFileCallOptions {
        let data = self.data.into_files_upload_data();
        let media_type = resolve_upload_file_media_type(&data, self.media_type);

        FilesUploadFileCallOptions {
            data,
            media_type,
            filename: self.filename,
            provider_options: self.provider_options,
        }
    }
}

/// Result returned by the high-level `upload_file` helper.
pub type UploadFileResult = FilesUploadFileResult;

/// Uploads a file using a provider-v4 files API interface.
pub async fn upload_file<F>(api: &F, options: UploadFileOptions) -> UploadFileResult
where
    F: Files + ?Sized,
{
    api.upload_file(options.into_call_options()).await
}

/// Uploads a file by resolving the files interface from a provider-v4 provider.
pub async fn upload_file_with_provider<P>(
    provider: &P,
    options: UploadFileOptions,
) -> UploadFileResult
where
    P: ProviderWithFiles + ?Sized,
{
    let files = provider.files();
    upload_file(&files, options).await
}

fn resolve_upload_file_media_type(
    data: &FilesUploadFileData,
    media_type: Option<String>,
) -> String {
    if let Some(media_type) = media_type {
        return media_type;
    }

    match data {
        FilesUploadFileData::Text { .. } => "text/plain".to_string(),
        FilesUploadFileData::Data { data } => detect_media_type(data, None)
            .map(str::to_string)
            .unwrap_or_else(|| {
                if is_likely_text(data) {
                    "text/plain".to_string()
                } else {
                    "application/octet-stream".to_string()
                }
            }),
    }
}

fn is_likely_text(data: &FileDataContent) -> bool {
    const CHECK_LENGTH: usize = 512;
    const BASE64_CHECK_LENGTH: usize = 688;

    let bytes: Vec<u8> = match data {
        FileDataContent::Bytes(bytes) => bytes.iter().copied().take(CHECK_LENGTH).collect(),
        FileDataContent::Base64(base64) => {
            let prefix_length = base64
                .char_indices()
                .nth(BASE64_CHECK_LENGTH)
                .map_or(base64.len(), |(index, _)| index);

            match convert_base64_to_bytes(&base64[..prefix_length]) {
                Ok(bytes) => bytes.into_iter().take(CHECK_LENGTH).collect(),
                Err(_) => return false,
            }
        }
    };

    if bytes.is_empty() {
        return false;
    }

    bytes
        .into_iter()
        .all(|byte| byte != 0x00 && (byte >= 0x20 || matches!(byte, 0x09 | 0x0a | 0x0d)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    use serde_json::json;

    use super::{
        UploadFileData, UploadFileOptions, UploadFileResult, upload_file, upload_file_with_provider,
    };
    use crate::file_data::{FileDataContent, ProviderReference};
    use crate::files::{
        Files, FilesUploadFileCallOptions, FilesUploadFileData, FilesUploadFileResult,
    };
    use crate::mock_models::{MockEmbeddingModel, MockImageModel, MockLanguageModel};
    use crate::provider::{
        ModelType, NoSuchModelError, Provider, ProviderMetadata, ProviderOptions,
    };
    use crate::provider::{ProviderWithFiles, SpecificationVersion};
    use crate::warning::Warning;

    #[derive(Clone, Default)]
    struct RecordingFiles {
        calls: Arc<Mutex<Vec<FilesUploadFileCallOptions>>>,
    }

    impl RecordingFiles {
        fn calls(&self) -> Vec<FilesUploadFileCallOptions> {
            self.calls
                .lock()
                .expect("recorded files calls mutex is not poisoned")
                .clone()
        }
    }

    fn provider_reference(id: &str) -> ProviderReference {
        ProviderReference::try_from(BTreeMap::from([(
            "test-provider".to_string(),
            id.to_string(),
        )]))
        .expect("provider reference is valid")
    }

    impl Files for RecordingFiles {
        type UploadFileFuture<'a>
            = Ready<FilesUploadFileResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn upload_file(&self, options: FilesUploadFileCallOptions) -> Self::UploadFileFuture<'_> {
            self.calls
                .lock()
                .expect("recorded files calls mutex is not poisoned")
                .push(options.clone());

            let provider_reference = ProviderReference::try_from(BTreeMap::from([(
                "test-provider".to_string(),
                "file_123".to_string(),
            )]))
            .expect("provider reference is valid");

            ready(
                FilesUploadFileResult::new(provider_reference)
                    .with_media_type(options.media_type)
                    .with_filename(options.filename.unwrap_or_else(|| "uploaded".to_string())),
            )
        }
    }

    #[derive(Clone)]
    struct RecordingFilesProvider {
        files: RecordingFiles,
    }

    impl RecordingFilesProvider {
        fn new(files: RecordingFiles) -> Self {
            Self { files }
        }
    }

    impl Provider for RecordingFilesProvider {
        type LanguageModel = MockLanguageModel;
        type EmbeddingModel = MockEmbeddingModel;
        type ImageModel = MockImageModel;

        fn specification_version(&self) -> SpecificationVersion {
            SpecificationVersion::V4
        }

        fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
            Err(NoSuchModelError::new(model_id, ModelType::LanguageModel))
        }

        fn embedding_model(
            &self,
            model_id: &str,
        ) -> Result<Self::EmbeddingModel, NoSuchModelError> {
            Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
        }

        fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
            Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
        }
    }

    impl ProviderWithFiles for RecordingFilesProvider {
        type Files = RecordingFiles;

        fn files(&self) -> Self::Files {
            self.files.clone()
        }
    }

    #[derive(Clone)]
    struct StaticResultFiles {
        result: FilesUploadFileResult,
    }

    impl StaticResultFiles {
        fn new(result: FilesUploadFileResult) -> Self {
            Self { result }
        }
    }

    impl Files for StaticResultFiles {
        type UploadFileFuture<'a>
            = Ready<FilesUploadFileResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn upload_file(&self, _options: FilesUploadFileCallOptions) -> Self::UploadFileFuture<'_> {
            ready(self.result.clone())
        }
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures should not be pending"),
        }
    }

    #[test]
    fn upload_file_options_accepts_raw_data_shorthand_json() {
        let options = UploadFileOptions::new("JVBERi0xLjQ=")
            .with_filename("spec.pdf")
            .with_provider_options(
                serde_json::from_value::<ProviderOptions>(json!({
                    "openai": {
                        "purpose": "assistants"
                    }
                }))
                .expect("provider options deserialize"),
            );

        assert_eq!(
            serde_json::to_value(options).expect("upload file options serialize"),
            json!({
                "data": "JVBERi0xLjQ=",
                "filename": "spec.pdf",
                "providerOptions": {
                    "openai": {
                        "purpose": "assistants"
                    }
                }
            })
        );

        let deserialized: UploadFileOptions = serde_json::from_value(json!({
            "data": [37, 80, 68, 70],
            "mediaType": "application/pdf"
        }))
        .expect("upload file options deserialize");

        assert_eq!(
            deserialized,
            UploadFileOptions::new(vec![37, 80, 68, 70]).with_media_type("application/pdf")
        );
    }

    #[test]
    fn upload_file_data_preserves_tagged_text_shape() {
        let data = UploadFileData::text("hello");

        assert_eq!(
            serde_json::to_value(data.clone()).expect("upload file data serializes"),
            json!({
                "type": "text",
                "text": "hello"
            })
        );

        assert_eq!(
            data.into_files_upload_data(),
            FilesUploadFileData::text("hello")
        );
    }

    #[test]
    fn upload_file_passes_tagged_base64_string_data_through_to_files_upload_file() {
        let files = RecordingFiles::default();

        let _ = poll_ready(upload_file(
            &files,
            UploadFileOptions::new(FilesUploadFileData::data(FileDataContent::Base64(
                "dGVzdA==".to_string(),
            ))),
        ));

        let calls = files.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].data,
            FilesUploadFileData::data(FileDataContent::Base64("dGVzdA==".to_string()))
        );
    }

    #[test]
    fn upload_file_options_resolves_media_types_like_upstream_upload_file() {
        let pdf_options = UploadFileOptions::new("JVBERi0xLjQ=").into_call_options();
        assert_eq!(pdf_options.media_type, "application/pdf");

        let text_options =
            UploadFileOptions::new(FilesUploadFileData::text("hello")).into_call_options();
        assert_eq!(text_options.media_type, "text/plain");

        let likely_text_options =
            UploadFileOptions::new(FileDataContent::Bytes(b"hello".to_vec())).into_call_options();
        assert_eq!(likely_text_options.media_type, "text/plain");

        let binary_options = UploadFileOptions::new(vec![0x00, 0x01]).into_call_options();
        assert_eq!(binary_options.media_type, "application/octet-stream");
    }

    #[test]
    fn upload_file_forwards_normalized_provider_call_options() {
        let files = RecordingFiles::default();
        let result = poll_ready(upload_file(
            &files,
            UploadFileOptions::new("JVBERi0xLjQ=").with_filename("spec.pdf"),
        ));

        let calls = files.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            FilesUploadFileCallOptions::new(
                FilesUploadFileData::data(FileDataContent::Base64("JVBERi0xLjQ=".to_string())),
                "application/pdf",
            )
            .with_filename("spec.pdf")
        );

        let expected_reference = ProviderReference::try_from(BTreeMap::from([(
            "test-provider".to_string(),
            "file_123".to_string(),
        )]))
        .expect("provider reference is valid");

        assert_eq!(
            result,
            UploadFileResult::new(expected_reference)
                .with_media_type("application/pdf")
                .with_filename("spec.pdf")
        );
    }

    #[test]
    fn upload_file_forwards_provider_options_to_files_upload_file() {
        let files = RecordingFiles::default();
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "test-provider": {
                "purpose": "assistants"
            }
        }))
        .expect("provider options deserialize");

        let _ = poll_ready(upload_file(
            &files,
            UploadFileOptions::new(vec![1_u8]).with_provider_options(provider_options.clone()),
        ));

        let calls = files.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].provider_options, Some(provider_options));
    }

    #[test]
    fn upload_file_passes_undefined_provider_options_when_not_provided() {
        let files = RecordingFiles::default();

        let _ = poll_ready(upload_file(&files, UploadFileOptions::new(vec![1_u8])));

        let calls = files.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].provider_options, None);
    }

    #[test]
    fn upload_file_passes_warnings_from_provider_result() {
        let files = StaticResultFiles::new(
            FilesUploadFileResult::new(provider_reference("file_abc")).with_warning(
                Warning::Unsupported {
                    feature: "filename".to_string(),
                    details: None,
                },
            ),
        );

        let result = poll_ready(upload_file(
            &files,
            UploadFileOptions::new(vec![1_u8]).with_filename("test.pdf"),
        ));

        assert_eq!(
            result.warnings,
            vec![Warning::Unsupported {
                feature: "filename".to_string(),
                details: None,
            }]
        );
    }

    #[test]
    fn upload_file_returns_result_without_provider_metadata_when_not_provided() {
        let files =
            StaticResultFiles::new(FilesUploadFileResult::new(provider_reference("file_xyz")));

        let result = poll_ready(upload_file(&files, UploadFileOptions::new(vec![1_u8])));

        assert_eq!(result.provider_reference, provider_reference("file_xyz"));
        assert_eq!(result.provider_metadata, None);
    }

    #[test]
    fn upload_file_returns_provider_metadata_when_provided() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "test-provider": {
                "size": 1024
            }
        }))
        .expect("provider metadata deserialize");
        let files = StaticResultFiles::new(
            FilesUploadFileResult::new(provider_reference("file_abc123"))
                .with_provider_metadata(provider_metadata.clone()),
        );

        let result = poll_ready(upload_file(&files, UploadFileOptions::new(vec![1_u8])));

        assert_eq!(result.provider_reference, provider_reference("file_abc123"));
        assert_eq!(result.provider_metadata, Some(provider_metadata));
    }

    #[test]
    fn upload_file_resolves_files_v4_from_provider_v4_with_files_method() {
        let files = RecordingFiles::default();
        let provider = RecordingFilesProvider::new(files.clone());

        let result = poll_ready(upload_file_with_provider(
            &provider,
            UploadFileOptions::new(vec![1_u8]).with_filename("test.pdf"),
        ));

        let calls = files.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            FilesUploadFileCallOptions::new(
                FilesUploadFileData::data(FileDataContent::Bytes(vec![1])),
                "application/octet-stream",
            )
            .with_filename("test.pdf")
        );

        assert_eq!(result.filename.as_deref(), Some("test.pdf"));
    }
}
