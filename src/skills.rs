use serde::{Deserialize, Serialize};

use crate::file_data::{FileDataContent, ProviderReference};
use crate::provider::{ProviderMetadata, ProviderOptions};
use crate::warning::Warning;

/// File data accepted by the provider skills upload interface.
///
/// Skill uploads accept either raw/base64 file data or inline UTF-8 text.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SkillsFileData {
    /// Raw bytes or base64-encoded file content.
    Data { data: FileDataContent },

    /// Inline text file content.
    Text { text: String },
}

impl SkillsFileData {
    /// Creates skill file data from raw bytes or base64-encoded file content.
    pub fn data(data: FileDataContent) -> Self {
        Self::Data { data }
    }

    /// Creates skill file data from inline text content.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
}

/// A file that makes up a skill upload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsFile {
    /// The path of the file relative to the skill root.
    pub path: String,

    /// The file content to upload.
    pub data: SkillsFileData,
}

impl SkillsFile {
    /// Creates a skill file with its root-relative path and content.
    pub fn new(path: impl Into<String>, data: SkillsFileData) -> Self {
        Self {
            path: path.into(),
            data,
        }
    }
}

/// Options for uploading a skill via a provider skills interface.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsUploadSkillCallOptions {
    /// The files that make up the skill.
    pub files: Vec<SkillsFile>,

    /// Optional human-readable title for the skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,

    /// Provider-specific options passed through to the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl SkillsUploadSkillCallOptions {
    /// Creates skill upload options with the required files.
    pub fn new(files: Vec<SkillsFile>) -> Self {
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

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

/// Result of uploading a skill via a provider skills interface.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsUploadSkillResult {
    /// Provider-to-skill-id mapping for the uploaded skill.
    pub provider_reference: ProviderReference,

    /// Human-readable title for the uploaded skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,

    /// Name of the uploaded skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Description of what the uploaded skill does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Latest version identifier of the uploaded skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,

    /// Provider-specific metadata returned by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Warnings from the provider.
    pub warnings: Vec<Warning>,
}

impl SkillsUploadSkillResult {
    /// Creates a skill upload result with no warnings.
    pub fn new(provider_reference: ProviderReference) -> Self {
        Self {
            provider_reference,
            display_title: None,
            name: None,
            description: None,
            latest_version: None,
            provider_metadata: None,
            warnings: Vec::new(),
        }
    }

    /// Sets the human-readable title for the uploaded skill.
    pub fn with_display_title(mut self, display_title: impl Into<String>) -> Self {
        self.display_title = Some(display_title.into());
        self
    }

    /// Sets the uploaded skill name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the uploaded skill description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the latest uploaded skill version identifier.
    pub fn with_latest_version(mut self, latest_version: impl Into<String>) -> Self {
        self.latest_version = Some(latest_version.into());
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

    use serde_json::json;

    use super::{
        SkillsFile, SkillsFileData, SkillsUploadSkillCallOptions, SkillsUploadSkillResult,
    };
    use crate::file_data::{FileDataContent, ProviderReference};
    use crate::provider::{ProviderMetadata, ProviderOptions};
    use crate::warning::Warning;

    #[test]
    fn upload_skill_call_options_serializes_files_title_and_provider_options() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "vercel": {
                "visibility": "private"
            }
        }))
        .expect("provider options deserialize");

        let options = SkillsUploadSkillCallOptions::new(vec![
            SkillsFile::new(
                "skill.md",
                SkillsFileData::text("# Weather skill\n\nProvides weather context."),
            ),
            SkillsFile::new(
                "assets/icon.png",
                SkillsFileData::data(FileDataContent::Base64("iVBORw0KGgo=".to_string())),
            ),
        ])
        .with_display_title("Weather skill")
        .with_provider_options(provider_options);

        assert_eq!(
            serde_json::to_value(options).expect("upload skill options serialize"),
            json!({
                "files": [
                    {
                        "path": "skill.md",
                        "data": {
                            "type": "text",
                            "text": "# Weather skill\n\nProvides weather context."
                        }
                    },
                    {
                        "path": "assets/icon.png",
                        "data": {
                            "type": "data",
                            "data": "iVBORw0KGgo="
                        }
                    }
                ],
                "displayTitle": "Weather skill",
                "providerOptions": {
                    "vercel": {
                        "visibility": "private"
                    }
                }
            })
        );
    }

    #[test]
    fn upload_skill_call_options_deserializes_minimal_files_and_omits_optional_fields() {
        let options: SkillsUploadSkillCallOptions = serde_json::from_value(json!({
            "files": [
                {
                    "path": "skill.md",
                    "data": {
                        "type": "text",
                        "text": "# Skill"
                    }
                }
            ]
        }))
        .expect("upload skill options deserialize");

        assert_eq!(
            options,
            SkillsUploadSkillCallOptions::new(vec![SkillsFile::new(
                "skill.md",
                SkillsFileData::text("# Skill"),
            )])
        );
        assert_eq!(
            serde_json::to_value(options).expect("upload skill options serialize"),
            json!({
                "files": [
                    {
                        "path": "skill.md",
                        "data": {
                            "type": "text",
                            "text": "# Skill"
                        }
                    }
                ]
            })
        );
    }

    #[test]
    fn upload_skill_result_serializes_reference_metadata_and_warnings() {
        let provider_reference = ProviderReference::try_from(BTreeMap::from([(
            "vercel".to_string(),
            "skill_abc123".to_string(),
        )]))
        .expect("provider reference is valid");
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "vercel": {
                "createdAt": "2026-05-16T01:23:45Z"
            }
        }))
        .expect("provider metadata deserialize");

        let result = SkillsUploadSkillResult::new(provider_reference)
            .with_display_title("Weather skill")
            .with_name("weather")
            .with_description("Provides weather context.")
            .with_latest_version("2026-05-16.1")
            .with_provider_metadata(provider_metadata)
            .with_warning(Warning::Unsupported {
                feature: "private-visibility".to_string(),
                details: Some("The provider stored the skill as public.".to_string()),
            });

        assert_eq!(
            serde_json::to_value(result).expect("upload skill result serializes"),
            json!({
                "providerReference": {
                    "vercel": "skill_abc123"
                },
                "displayTitle": "Weather skill",
                "name": "weather",
                "description": "Provides weather context.",
                "latestVersion": "2026-05-16.1",
                "providerMetadata": {
                    "vercel": {
                        "createdAt": "2026-05-16T01:23:45Z"
                    }
                },
                "warnings": [
                    {
                        "type": "unsupported",
                        "feature": "private-visibility",
                        "details": "The provider stored the skill as public."
                    }
                ]
            })
        );
    }

    #[test]
    fn upload_skill_result_deserializes_empty_warnings_and_omits_optional_fields() {
        let result: SkillsUploadSkillResult = serde_json::from_value(json!({
            "providerReference": {
                "vercel": "skill_abc123"
            },
            "warnings": []
        }))
        .expect("upload skill result deserializes");
        let provider_reference = ProviderReference::try_from(BTreeMap::from([(
            "vercel".to_string(),
            "skill_abc123".to_string(),
        )]))
        .expect("provider reference is valid");

        assert_eq!(result, SkillsUploadSkillResult::new(provider_reference));
        assert_eq!(
            serde_json::to_value(result).expect("upload skill result serializes"),
            json!({
                "providerReference": {
                    "vercel": "skill_abc123"
                },
                "warnings": []
            })
        );
    }
}
