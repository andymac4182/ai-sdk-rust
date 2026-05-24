use serde::{Deserialize, Serialize};

use crate::file_data::FileDataContent;
use crate::provider::{ProviderOptions, ProviderWithSkills};
use crate::skills::{
    Skills, SkillsFile, SkillsFileData, SkillsUploadSkillCallOptions, SkillsUploadSkillResult,
};

/// File data accepted by the high-level `upload_skill` helper.
///
/// This mirrors upstream `uploadSkill`: callers can provide the tagged provider
/// skill-file data shape, or a raw byte/base64 value that is treated as file data.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum UploadSkillFileData {
    /// Tagged provider skill-file data.
    Tagged(SkillsFileData),

    /// Raw skill-file data shorthand.
    Raw(FileDataContent),
}

impl UploadSkillFileData {
    /// Creates raw upload data from bytes or base64 content.
    pub fn raw(data: impl Into<FileDataContent>) -> Self {
        Self::Raw(data.into())
    }

    /// Creates tagged data upload content.
    pub fn data(data: impl Into<FileDataContent>) -> Self {
        Self::Tagged(SkillsFileData::data(data.into()))
    }

    /// Creates tagged text upload content.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Tagged(SkillsFileData::text(text))
    }

    /// Converts this high-level input into provider skills file data.
    pub fn into_skills_file_data(self) -> SkillsFileData {
        match self {
            Self::Tagged(data) => data,
            Self::Raw(data) => SkillsFileData::data(data),
        }
    }
}

impl From<SkillsFileData> for UploadSkillFileData {
    fn from(data: SkillsFileData) -> Self {
        Self::Tagged(data)
    }
}

impl From<FileDataContent> for UploadSkillFileData {
    fn from(data: FileDataContent) -> Self {
        Self::Raw(data)
    }
}

impl From<Vec<u8>> for UploadSkillFileData {
    fn from(data: Vec<u8>) -> Self {
        Self::Raw(FileDataContent::Bytes(data))
    }
}

impl From<String> for UploadSkillFileData {
    fn from(data: String) -> Self {
        Self::Raw(FileDataContent::Base64(data))
    }
}

impl From<&str> for UploadSkillFileData {
    fn from(data: &str) -> Self {
        Self::Raw(FileDataContent::Base64(data.to_string()))
    }
}

/// A file accepted by the high-level `upload_skill` helper.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadSkillFile {
    /// The path of the file relative to the skill root.
    pub path: String,

    /// The file content to upload.
    pub data: UploadSkillFileData,
}

impl UploadSkillFile {
    /// Creates a skill upload file with its root-relative path and content.
    pub fn new(path: impl Into<String>, data: impl Into<UploadSkillFileData>) -> Self {
        Self {
            path: path.into(),
            data: data.into(),
        }
    }

    /// Converts this high-level file into a provider skills file.
    pub fn into_skills_file(self) -> SkillsFile {
        SkillsFile::new(self.path, self.data.into_skills_file_data())
    }
}

impl From<SkillsFile> for UploadSkillFile {
    fn from(file: SkillsFile) -> Self {
        Self {
            path: file.path,
            data: UploadSkillFileData::Tagged(file.data),
        }
    }
}

/// Options for a high-level `upload_skill` call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadSkillOptions {
    /// The files that make up the skill.
    pub files: Vec<UploadSkillFile>,

    /// Optional human-readable title for the skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,

    /// Provider-specific options passed through to the skills API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl UploadSkillOptions {
    /// Creates high-level skill upload options.
    pub fn new(files: Vec<UploadSkillFile>) -> Self {
        Self {
            files,
            display_title: None,
            provider_options: None,
        }
    }

    /// Sets the optional human-readable skill title.
    pub fn with_display_title(mut self, display_title: impl Into<String>) -> Self {
        self.display_title = Some(display_title.into());
        self
    }

    /// Adds provider-specific options for the upload.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Converts high-level options into provider skills call options.
    pub fn into_call_options(self) -> SkillsUploadSkillCallOptions {
        SkillsUploadSkillCallOptions {
            files: self
                .files
                .into_iter()
                .map(UploadSkillFile::into_skills_file)
                .collect(),
            display_title: self.display_title,
            provider_options: self.provider_options,
        }
    }
}

/// Result returned by the high-level `upload_skill` helper.
pub type UploadSkillResult = SkillsUploadSkillResult;

/// Uploads a skill using a provider-v4 skills API interface.
pub async fn upload_skill<S>(api: &S, options: UploadSkillOptions) -> UploadSkillResult
where
    S: Skills + ?Sized,
{
    api.upload_skill(options.into_call_options()).await
}

/// Uploads a skill by resolving the skills interface from a provider-v4 provider.
pub async fn upload_skill_with_provider<P>(
    provider: &P,
    options: UploadSkillOptions,
) -> UploadSkillResult
where
    P: ProviderWithSkills + ?Sized,
{
    let skills = provider.skills();
    upload_skill(&skills, options).await
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
        UploadSkillFile, UploadSkillFileData, UploadSkillOptions, UploadSkillResult, upload_skill,
        upload_skill_with_provider,
    };
    use crate::file_data::{FileDataContent, ProviderReference};
    use crate::mock_models::{MockEmbeddingModel, MockImageModel, MockLanguageModel};
    use crate::provider::{
        ModelType, NoSuchModelError, Provider, ProviderMetadata, ProviderOptions,
        ProviderWithSkills, SpecificationVersion,
    };
    use crate::skills::{
        Skills, SkillsFile, SkillsFileData, SkillsUploadSkillCallOptions, SkillsUploadSkillResult,
    };
    use crate::warning::Warning;

    #[derive(Clone, Default)]
    struct RecordingSkills {
        calls: Arc<Mutex<Vec<SkillsUploadSkillCallOptions>>>,
    }

    impl RecordingSkills {
        fn calls(&self) -> Vec<SkillsUploadSkillCallOptions> {
            self.calls
                .lock()
                .expect("recorded skills calls mutex is not poisoned")
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

    impl Skills for RecordingSkills {
        type UploadSkillFuture<'a>
            = Ready<SkillsUploadSkillResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn upload_skill(
            &self,
            options: SkillsUploadSkillCallOptions,
        ) -> Self::UploadSkillFuture<'_> {
            self.calls
                .lock()
                .expect("recorded skills calls mutex is not poisoned")
                .push(options.clone());

            let provider_reference = ProviderReference::try_from(BTreeMap::from([(
                "test-provider".to_string(),
                "skill_123".to_string(),
            )]))
            .expect("provider reference is valid");

            ready(
                SkillsUploadSkillResult::new(provider_reference).with_display_title(
                    options.display_title.unwrap_or_else(|| "Skill".to_string()),
                ),
            )
        }
    }

    #[derive(Clone)]
    struct RecordingSkillsProvider {
        skills: RecordingSkills,
    }

    impl RecordingSkillsProvider {
        fn new(skills: RecordingSkills) -> Self {
            Self { skills }
        }
    }

    impl Provider for RecordingSkillsProvider {
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

    impl ProviderWithSkills for RecordingSkillsProvider {
        type Skills = RecordingSkills;

        fn skills(&self) -> Self::Skills {
            self.skills.clone()
        }
    }

    #[derive(Clone)]
    struct StaticResultSkills {
        result: SkillsUploadSkillResult,
    }

    impl StaticResultSkills {
        fn new(result: SkillsUploadSkillResult) -> Self {
            Self { result }
        }
    }

    impl Skills for StaticResultSkills {
        type UploadSkillFuture<'a>
            = Ready<SkillsUploadSkillResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn upload_skill(
            &self,
            _options: SkillsUploadSkillCallOptions,
        ) -> Self::UploadSkillFuture<'_> {
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
    fn upload_skill_options_accepts_raw_file_data_shorthand_json() {
        let options = UploadSkillOptions::new(vec![
            UploadSkillFile::new("skill.ts", "ZXhwb3J0IGRlZmF1bHQge307"),
            UploadSkillFile::new("icon.bin", vec![0x00, 0x01]),
        ])
        .with_display_title("My Skill")
        .with_provider_options(
            serde_json::from_value::<ProviderOptions>(json!({
                "openai": {
                    "custom": "value"
                }
            }))
            .expect("provider options deserialize"),
        );

        assert_eq!(
            serde_json::to_value(options).expect("upload skill options serialize"),
            json!({
                "files": [
                    {
                        "path": "skill.ts",
                        "data": "ZXhwb3J0IGRlZmF1bHQge307"
                    },
                    {
                        "path": "icon.bin",
                        "data": [0, 1]
                    }
                ],
                "displayTitle": "My Skill",
                "providerOptions": {
                    "openai": {
                        "custom": "value"
                    }
                }
            })
        );

        let deserialized: UploadSkillOptions = serde_json::from_value(json!({
            "files": [
                {
                    "path": "skill.ts",
                    "data": "ZXhwb3J0IGRlZmF1bHQge307"
                }
            ]
        }))
        .expect("upload skill options deserialize");

        assert_eq!(
            deserialized,
            UploadSkillOptions::new(vec![UploadSkillFile::new(
                "skill.ts",
                "ZXhwb3J0IGRlZmF1bHQge307"
            )])
        );
    }

    #[test]
    fn upload_skill_file_data_preserves_tagged_text_shape() {
        let data = UploadSkillFileData::text("# Skill");

        assert_eq!(
            serde_json::to_value(data.clone()).expect("upload skill file data serializes"),
            json!({
                "type": "text",
                "text": "# Skill"
            })
        );

        assert_eq!(
            data.into_skills_file_data(),
            SkillsFileData::text("# Skill")
        );
    }

    #[test]
    fn upload_skill_options_normalizes_file_shorthand_to_provider_shape() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "vercel": {
                "visibility": "private"
            }
        }))
        .expect("provider options deserialize");

        let options = UploadSkillOptions::new(vec![
            UploadSkillFile::new("skill.ts", "ZXhwb3J0IGRlZmF1bHQge307"),
            UploadSkillFile::new("README.md", UploadSkillFileData::text("# Skill")),
        ])
        .with_display_title("My Skill")
        .with_provider_options(provider_options.clone())
        .into_call_options();

        assert_eq!(
            options,
            SkillsUploadSkillCallOptions::new(vec![
                SkillsFile::new(
                    "skill.ts",
                    SkillsFileData::data(FileDataContent::Base64(
                        "ZXhwb3J0IGRlZmF1bHQge307".to_string()
                    )),
                ),
                SkillsFile::new("README.md", SkillsFileData::text("# Skill")),
            ])
            .with_display_title("My Skill")
            .with_provider_options(provider_options)
        );
    }

    #[test]
    fn upload_skill_forwards_normalized_provider_call_options() {
        let skills = RecordingSkills::default();
        let result = poll_ready(upload_skill(
            &skills,
            UploadSkillOptions::new(vec![UploadSkillFile::new(
                "skill.ts",
                "ZXhwb3J0IGRlZmF1bHQge307",
            )])
            .with_display_title("My Skill"),
        ));

        let calls = skills.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            SkillsUploadSkillCallOptions::new(vec![SkillsFile::new(
                "skill.ts",
                SkillsFileData::data(FileDataContent::Base64(
                    "ZXhwb3J0IGRlZmF1bHQge307".to_string()
                )),
            )])
            .with_display_title("My Skill")
        );

        let expected_reference = ProviderReference::try_from(BTreeMap::from([(
            "test-provider".to_string(),
            "skill_123".to_string(),
        )]))
        .expect("provider reference is valid");

        assert_eq!(
            result,
            UploadSkillResult::new(expected_reference).with_display_title("My Skill")
        );
    }

    #[test]
    fn upload_skill_returns_provider_reference_warnings_and_provider_metadata_from_skills() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "test-provider": {
                "foo": "bar"
            }
        }))
        .expect("provider metadata deserialize");
        let skills = StaticResultSkills::new(
            SkillsUploadSkillResult::new(provider_reference("skill_123"))
                .with_provider_metadata(provider_metadata.clone())
                .with_warning(Warning::Unsupported {
                    feature: "displayTitle".to_string(),
                    details: None,
                }),
        );

        let result = poll_ready(upload_skill(
            &skills,
            UploadSkillOptions::new(vec![UploadSkillFile::new(
                "test.ts",
                UploadSkillFileData::data("aGVsbG8="),
            )]),
        ));

        assert_eq!(result.provider_reference, provider_reference("skill_123"));
        assert_eq!(result.provider_metadata, Some(provider_metadata));
        assert_eq!(
            result.warnings,
            vec![Warning::Unsupported {
                feature: "displayTitle".to_string(),
                details: None,
            }]
        );
    }

    #[test]
    fn upload_skill_passes_provider_options_to_the_skills() {
        let skills = RecordingSkills::default();
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "custom": "value"
            }
        }))
        .expect("provider options deserialize");

        let _ = poll_ready(upload_skill(
            &skills,
            UploadSkillOptions::new(vec![UploadSkillFile::new(
                "test.ts",
                UploadSkillFileData::data("aGVsbG8="),
            )])
            .with_provider_options(provider_options.clone()),
        ));

        let calls = skills.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].display_title, None);
        assert_eq!(calls[0].provider_options, Some(provider_options));
    }

    #[test]
    fn upload_skill_resolves_skills_v4_from_provider_v4_with_skills_method() {
        let skills = RecordingSkills::default();
        let provider = RecordingSkillsProvider::new(skills.clone());

        let result = poll_ready(upload_skill_with_provider(
            &provider,
            UploadSkillOptions::new(vec![UploadSkillFile::new(
                "test.ts",
                UploadSkillFileData::data("aGVsbG8="),
            )])
            .with_display_title("My Skill"),
        ));

        let calls = skills.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            SkillsUploadSkillCallOptions::new(vec![SkillsFile::new(
                "test.ts",
                SkillsFileData::data(FileDataContent::Base64("aGVsbG8=".to_string())),
            )])
            .with_display_title("My Skill")
        );

        assert_eq!(result.display_title.as_deref(), Some("My Skill"));
    }
}
