use std::env::{self, VarError};

use serde::{Deserialize, Serialize};

use crate::file_data::{NoSuchProviderReferenceError, ProviderReference};
use crate::provider::{LoadApiKeyError, LoadSettingError};

/// A value that can be supplied as either one item or an array of items.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Arrayable<T> {
    /// A single item.
    Single(T),

    /// Multiple items.
    Array(Vec<T>),
}

impl<T> Arrayable<T> {
    /// Creates an arrayable single value.
    pub fn single(value: T) -> Self {
        Self::Single(value)
    }

    /// Creates an arrayable array value.
    pub fn array(values: Vec<T>) -> Self {
        Self::Array(values)
    }

    /// Converts the value into an array.
    pub fn into_vec(self) -> Vec<T> {
        match self {
            Self::Single(value) => vec![value],
            Self::Array(values) => values,
        }
    }
}

/// Normalizes a missing, single, or array value into an array.
pub fn as_array<T>(value: Option<Arrayable<T>>) -> Vec<T> {
    value.map_or_else(Vec::new, Arrayable::into_vec)
}

/// Checks whether an optional value is present.
pub fn is_non_nullable<T>(value: &Option<T>) -> bool {
    value.is_some()
}

/// Filters missing values out of a list of optional values.
pub fn filter_nullable<T>(values: impl IntoIterator<Item = Option<T>>) -> Vec<T> {
    values.into_iter().flatten().collect()
}

/// Options for loading a provider API key from an explicit value or environment variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadApiKeyOptions {
    /// Explicit API key value. When present, it is returned without reading the environment.
    pub api_key: Option<String>,

    /// Environment variable to read when `api_key` is not provided.
    pub environment_variable_name: String,

    /// Parameter name used in missing-key error messages.
    pub api_key_parameter_name: String,

    /// Human-readable provider or API description used in error messages.
    pub description: String,
}

impl LoadApiKeyOptions {
    /// Creates API key loading options with the upstream default `apiKey` parameter name.
    pub fn new(
        environment_variable_name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            api_key: None,
            environment_variable_name: environment_variable_name.into(),
            api_key_parameter_name: "apiKey".to_string(),
            description: description.into(),
        }
    }

    /// Sets the explicit API key value.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets the parameter name used in missing-key error messages.
    pub fn with_api_key_parameter_name(
        mut self,
        api_key_parameter_name: impl Into<String>,
    ) -> Self {
        self.api_key_parameter_name = api_key_parameter_name.into();
        self
    }
}

/// Loads a provider API key from an explicit value or environment variable.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `loadApiKey` for Rust callers:
/// typed explicit values win, missing values read the named environment variable,
/// and missing or non-Unicode environment values produce `LoadApiKeyError`.
pub fn load_api_key(options: LoadApiKeyOptions) -> Result<String, LoadApiKeyError> {
    load_api_key_with_env(options, |name| env::var(name))
}

fn load_api_key_with_env(
    options: LoadApiKeyOptions,
    load_env: impl FnOnce(&str) -> Result<String, VarError>,
) -> Result<String, LoadApiKeyError> {
    if let Some(api_key) = options.api_key {
        return Ok(api_key);
    }

    match load_env(&options.environment_variable_name) {
        Ok(api_key) => Ok(api_key),
        Err(VarError::NotPresent) => Err(LoadApiKeyError::new(format!(
            "{} API key is missing. Pass it using the '{}' parameter or the {} environment variable.",
            options.description, options.api_key_parameter_name, options.environment_variable_name
        ))),
        Err(VarError::NotUnicode(_)) => Err(LoadApiKeyError::new(format!(
            "{} API key must be a string. The value of the {} environment variable is not a string.",
            options.description, options.environment_variable_name
        ))),
    }
}

/// Options for loading a provider setting from an explicit value or environment variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadSettingOptions {
    /// Explicit setting value. When present, it is returned without reading the environment.
    pub setting_value: Option<String>,

    /// Environment variable to read when `setting_value` is not provided.
    pub environment_variable_name: String,

    /// Parameter name used in missing-setting error messages.
    pub setting_name: String,

    /// Human-readable setting description used in error messages.
    pub description: String,
}

impl LoadSettingOptions {
    /// Creates setting loading options.
    pub fn new(
        environment_variable_name: impl Into<String>,
        setting_name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            setting_value: None,
            environment_variable_name: environment_variable_name.into(),
            setting_name: setting_name.into(),
            description: description.into(),
        }
    }

    /// Sets the explicit setting value.
    pub fn with_setting_value(mut self, setting_value: impl Into<String>) -> Self {
        self.setting_value = Some(setting_value.into());
        self
    }
}

/// Loads a required string setting from an explicit value or environment variable.
pub fn load_setting(options: LoadSettingOptions) -> Result<String, LoadSettingError> {
    load_setting_with_env(options, |name| env::var(name))
}

fn load_setting_with_env(
    options: LoadSettingOptions,
    load_env: impl FnOnce(&str) -> Result<String, VarError>,
) -> Result<String, LoadSettingError> {
    if let Some(setting_value) = options.setting_value {
        return Ok(setting_value);
    }

    match load_env(&options.environment_variable_name) {
        Ok(setting_value) => Ok(setting_value),
        Err(VarError::NotPresent) => Err(LoadSettingError::new(format!(
            "{} setting is missing. Pass it using the '{}' parameter or the {} environment variable.",
            options.description, options.setting_name, options.environment_variable_name
        ))),
        Err(VarError::NotUnicode(_)) => Err(LoadSettingError::new(format!(
            "{} setting must be a string. The value of the {} environment variable is not a string.",
            options.description, options.environment_variable_name
        ))),
    }
}

/// Options for loading an optional provider setting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadOptionalSettingOptions {
    /// Explicit setting value. When present, it is returned without reading the environment.
    pub setting_value: Option<String>,

    /// Environment variable to read when `setting_value` is not provided.
    pub environment_variable_name: String,
}

impl LoadOptionalSettingOptions {
    /// Creates optional setting loading options.
    pub fn new(environment_variable_name: impl Into<String>) -> Self {
        Self {
            setting_value: None,
            environment_variable_name: environment_variable_name.into(),
        }
    }

    /// Sets the explicit setting value.
    pub fn with_setting_value(mut self, setting_value: impl Into<String>) -> Self {
        self.setting_value = Some(setting_value.into());
        self
    }
}

/// Loads an optional setting from an explicit value or environment variable.
pub fn load_optional_setting(options: LoadOptionalSettingOptions) -> Option<String> {
    load_optional_setting_with_env(options, |name| env::var(name))
}

fn load_optional_setting_with_env(
    options: LoadOptionalSettingOptions,
    load_env: impl FnOnce(&str) -> Result<String, VarError>,
) -> Option<String> {
    if let Some(setting_value) = options.setting_value {
        return Some(setting_value);
    }

    load_env(&options.environment_variable_name).ok()
}

/// Maps a media type to the file extension used by upstream provider uploads.
pub fn media_type_to_extension(media_type: &str) -> String {
    let subtype = media_type
        .split_once('/')
        .map_or("", |(_, subtype)| subtype)
        .to_ascii_lowercase();

    match subtype.as_str() {
        "mpeg" => "mp3".to_string(),
        "x-wav" => "wav".to_string(),
        "opus" => "ogg".to_string(),
        "mp4" | "x-m4a" => "m4a".to_string(),
        _ => subtype,
    }
}

/// Strips all file extension segments from a filename.
pub fn strip_file_extension(filename: &str) -> &str {
    filename
        .find('.')
        .map_or(filename, |first_dot_index| &filename[..first_dot_index])
}

/// Removes exactly one trailing slash from a URL-like string when present.
pub fn without_trailing_slash(url: Option<&str>) -> Option<&str> {
    url.map(|url| url.strip_suffix('/').unwrap_or(url))
}

/// Resolves a provider reference to the provider-specific identifier.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `resolveProviderReference`
/// while reusing the crate's shared provider-reference contract.
pub fn resolve_provider_reference<'a>(
    reference: &'a ProviderReference,
    provider: &str,
) -> Result<&'a str, NoSuchProviderReferenceError> {
    reference.provider_id(provider)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::env::VarError;
    use std::ffi::OsString;

    use crate::ProviderReference;
    use serde_json::json;

    use super::{
        Arrayable, LoadApiKeyOptions, LoadOptionalSettingOptions, LoadSettingOptions, as_array,
        filter_nullable, is_non_nullable, load_api_key, load_api_key_with_env,
        load_optional_setting_with_env, load_setting, load_setting_with_env,
        media_type_to_extension, resolve_provider_reference, strip_file_extension,
        without_trailing_slash,
    };

    #[test]
    fn arrayable_serializes_single_or_array_values() {
        assert_eq!(
            serde_json::to_value(Arrayable::single("value")).expect("single value serializes"),
            json!("value")
        );
        assert_eq!(
            serde_json::to_value(Arrayable::array(vec!["a", "b"])).expect("array value serializes"),
            json!(["a", "b"])
        );
    }

    #[test]
    fn arrayable_deserializes_single_or_array_values() {
        assert_eq!(
            serde_json::from_value::<Arrayable<String>>(json!("value"))
                .expect("single value deserializes"),
            Arrayable::single("value".to_string())
        );
        assert_eq!(
            serde_json::from_value::<Arrayable<String>>(json!(["a", "b"]))
                .expect("array value deserializes"),
            Arrayable::array(vec!["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    fn as_array_returns_empty_array_for_missing_value() {
        assert_eq!(as_array::<String>(None), Vec::<String>::new());
    }

    #[test]
    fn as_array_wraps_single_value_in_array() {
        assert_eq!(as_array(Some(Arrayable::single("value"))), vec!["value"]);
    }

    #[test]
    fn as_array_returns_array_values_unchanged() {
        let value = vec!["a", "b"];

        assert_eq!(as_array(Some(Arrayable::array(value.clone()))), value);
    }

    #[test]
    fn is_non_nullable_reports_present_values() {
        assert!(is_non_nullable(&Some("value")));
        assert!(!is_non_nullable::<&str>(&None));
    }

    #[test]
    fn filter_nullable_removes_missing_values() {
        let values = vec![Some(1), None, Some(2), None, Some(3)];

        assert_eq!(filter_nullable(values), vec![1, 2, 3]);
    }

    #[test]
    fn filter_nullable_preserves_falsy_equivalent_values() {
        let values = vec![Some(json!(0)), Some(json!(false)), Some(json!("")), None];

        assert_eq!(
            filter_nullable(values),
            vec![json!(0), json!(false), json!("")]
        );
    }

    #[test]
    fn load_api_key_returns_explicit_value_without_reading_environment() {
        let api_key = load_api_key(
            LoadApiKeyOptions::new("AI_SDK_RUST_TEST_API_KEY", "Test Provider")
                .with_api_key("explicit-key"),
        )
        .expect("explicit API key loads");

        assert_eq!(api_key, "explicit-key");
    }

    #[test]
    fn load_api_key_reads_environment_when_value_is_missing() {
        let api_key = load_api_key_with_env(
            LoadApiKeyOptions::new("AI_SDK_RUST_TEST_API_KEY", "Test Provider"),
            |name| {
                assert_eq!(name, "AI_SDK_RUST_TEST_API_KEY");
                Ok("env-key".to_string())
            },
        )
        .expect("environment API key loads");

        assert_eq!(api_key, "env-key");
    }

    #[test]
    fn load_api_key_reports_upstream_missing_message() {
        let error = load_api_key_with_env(
            LoadApiKeyOptions::new("AI_SDK_RUST_TEST_API_KEY", "Test Provider")
                .with_api_key_parameter_name("token"),
            |_| Err(VarError::NotPresent),
        )
        .expect_err("missing API key is rejected");

        assert_eq!(
            error.to_string(),
            "Test Provider API key is missing. Pass it using the 'token' parameter or the AI_SDK_RUST_TEST_API_KEY environment variable."
        );
    }

    #[test]
    fn load_api_key_reports_non_unicode_environment_values_as_non_strings() {
        let error = load_api_key_with_env(
            LoadApiKeyOptions::new("AI_SDK_RUST_TEST_API_KEY", "Test Provider"),
            |_| Err(VarError::NotUnicode(OsString::from("not-unicode"))),
        )
        .expect_err("non-Unicode API key is rejected");

        assert_eq!(
            error.to_string(),
            "Test Provider API key must be a string. The value of the AI_SDK_RUST_TEST_API_KEY environment variable is not a string."
        );
    }

    #[test]
    fn load_setting_returns_explicit_value_without_reading_environment() {
        let setting = load_setting(
            LoadSettingOptions::new("AI_SDK_RUST_TEST_BASE_URL", "baseURL", "Test Provider")
                .with_setting_value("https://example.com"),
        )
        .expect("explicit setting loads");

        assert_eq!(setting, "https://example.com");
    }

    #[test]
    fn load_setting_reads_environment_when_value_is_missing() {
        let setting = load_setting_with_env(
            LoadSettingOptions::new("AI_SDK_RUST_TEST_BASE_URL", "baseURL", "Test Provider"),
            |name| {
                assert_eq!(name, "AI_SDK_RUST_TEST_BASE_URL");
                Ok("https://env.example.com".to_string())
            },
        )
        .expect("environment setting loads");

        assert_eq!(setting, "https://env.example.com");
    }

    #[test]
    fn load_setting_reports_upstream_missing_message() {
        let error = load_setting_with_env(
            LoadSettingOptions::new("AI_SDK_RUST_TEST_BASE_URL", "baseURL", "Test Provider"),
            |_| Err(VarError::NotPresent),
        )
        .expect_err("missing setting is rejected");

        assert_eq!(
            error.to_string(),
            "Test Provider setting is missing. Pass it using the 'baseURL' parameter or the AI_SDK_RUST_TEST_BASE_URL environment variable."
        );
    }

    #[test]
    fn load_setting_reports_non_unicode_environment_values_as_non_strings() {
        let error = load_setting_with_env(
            LoadSettingOptions::new("AI_SDK_RUST_TEST_BASE_URL", "baseURL", "Test Provider"),
            |_| Err(VarError::NotUnicode(OsString::from("not-unicode"))),
        )
        .expect_err("non-Unicode setting is rejected");

        assert_eq!(
            error.to_string(),
            "Test Provider setting must be a string. The value of the AI_SDK_RUST_TEST_BASE_URL environment variable is not a string."
        );
    }

    #[test]
    fn load_optional_setting_prefers_explicit_value() {
        let setting = load_optional_setting_with_env(
            LoadOptionalSettingOptions::new("AI_SDK_RUST_TEST_OPTIONAL")
                .with_setting_value("explicit"),
            |_| panic!("environment should not be read when explicit setting is present"),
        );

        assert_eq!(setting.as_deref(), Some("explicit"));
    }

    #[test]
    fn load_optional_setting_reads_environment_when_value_is_missing() {
        let setting = load_optional_setting_with_env(
            LoadOptionalSettingOptions::new("AI_SDK_RUST_TEST_OPTIONAL"),
            |_| Ok("env-setting".to_string()),
        );

        assert_eq!(setting.as_deref(), Some("env-setting"));
    }

    #[test]
    fn load_optional_setting_returns_none_for_missing_or_non_unicode_environment_values() {
        assert_eq!(
            load_optional_setting_with_env(
                LoadOptionalSettingOptions::new("AI_SDK_RUST_TEST_OPTIONAL"),
                |_| Err(VarError::NotPresent),
            ),
            None
        );

        assert_eq!(
            load_optional_setting_with_env(
                LoadOptionalSettingOptions::new("AI_SDK_RUST_TEST_OPTIONAL"),
                |_| Err(VarError::NotUnicode(OsString::from("not-unicode"))),
            ),
            None
        );
    }

    #[test]
    fn media_type_to_extension_maps_common_audio_media_types() {
        for (media_type, expected_extension) in [
            ("audio/mpeg", "mp3"),
            ("audio/mp3", "mp3"),
            ("audio/wav", "wav"),
            ("audio/x-wav", "wav"),
            ("audio/webm", "webm"),
            ("audio/ogg", "ogg"),
            ("audio/opus", "ogg"),
            ("audio/mp4", "m4a"),
            ("audio/x-m4a", "m4a"),
            ("audio/flac", "flac"),
            ("audio/aac", "aac"),
        ] {
            assert_eq!(
                media_type_to_extension(media_type),
                expected_extension,
                "{media_type} maps to {expected_extension}"
            );
        }
    }

    #[test]
    fn media_type_to_extension_lowercases_subtypes_and_handles_invalid_values() {
        assert_eq!(media_type_to_extension("AUDIO/MPEG"), "mp3");
        assert_eq!(media_type_to_extension("AUDIO/MP3"), "mp3");
        assert_eq!(media_type_to_extension("nope"), "");
    }

    #[test]
    fn strip_file_extension_strips_single_extension() {
        assert_eq!(strip_file_extension("report.pdf"), "report");
    }

    #[test]
    fn strip_file_extension_returns_input_when_there_is_no_dot() {
        assert_eq!(strip_file_extension("report"), "report");
    }

    #[test]
    fn strip_file_extension_strips_all_extension_segments() {
        assert_eq!(strip_file_extension("archive.tar.gz"), "archive");
    }

    #[test]
    fn strip_file_extension_strips_a_trailing_dot() {
        assert_eq!(strip_file_extension("report."), "report");
    }

    #[test]
    fn without_trailing_slash_removes_one_trailing_slash() {
        assert_eq!(
            without_trailing_slash(Some("https://api.example.com/")),
            Some("https://api.example.com")
        );
    }

    #[test]
    fn without_trailing_slash_preserves_values_without_trailing_slash() {
        assert_eq!(
            without_trailing_slash(Some("https://api.example.com/v1")),
            Some("https://api.example.com/v1")
        );
    }

    #[test]
    fn without_trailing_slash_preserves_missing_url() {
        assert_eq!(without_trailing_slash(None), None);
    }

    #[test]
    fn without_trailing_slash_only_removes_the_final_slash() {
        assert_eq!(
            without_trailing_slash(Some("https://api.example.com//")),
            Some("https://api.example.com/")
        );
    }

    #[test]
    fn resolve_provider_reference_returns_provider_specific_identifier() {
        let reference = ProviderReference::try_from(BTreeMap::from([
            ("anthropic".to_string(), "file-xyz".to_string()),
            ("openai".to_string(), "file-abc".to_string()),
        ]))
        .expect("provider reference is valid");

        assert_eq!(
            resolve_provider_reference(&reference, "openai").expect("openai reference is present"),
            "file-abc"
        );
        assert_eq!(
            resolve_provider_reference(&reference, "anthropic")
                .expect("anthropic reference is present"),
            "file-xyz"
        );
    }

    #[test]
    fn resolve_provider_reference_reports_missing_provider_context() {
        let reference = ProviderReference::try_from(BTreeMap::from([(
            "anthropic".to_string(),
            "file-xyz".to_string(),
        )]))
        .expect("provider reference is valid");

        let error = resolve_provider_reference(&reference, "openai")
            .expect_err("missing provider reference is rejected");

        assert_eq!(error.provider(), "openai");
        assert_eq!(error.reference(), &reference);
    }

    #[test]
    fn resolve_provider_reference_rejects_empty_references() {
        let reference =
            ProviderReference::try_from(BTreeMap::new()).expect("empty reference is valid");

        let error = resolve_provider_reference(&reference, "openai")
            .expect_err("empty reference cannot satisfy provider lookup");

        assert_eq!(error.provider(), "openai");
        assert_eq!(error.reference(), &reference);
    }
}
