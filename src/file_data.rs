use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use url::Url;

/// Error returned when a provider reference contains a reserved discriminator key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderReferenceError;

impl fmt::Display for ProviderReferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("provider references cannot contain the reserved `type` key")
    }
}

impl std::error::Error for ProviderReferenceError {}

/// Error returned when a provider-specific id is missing from a provider reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoSuchProviderReferenceError {
    provider: String,
    reference: ProviderReference,
    message: String,
}

impl NoSuchProviderReferenceError {
    /// Creates an error for a missing provider reference.
    pub fn new(provider: impl Into<String>, reference: ProviderReference) -> Self {
        let provider = provider.into();
        let message = no_such_provider_reference_default_message(&provider, &reference);

        Self {
            provider,
            reference,
            message,
        }
    }

    /// Creates an error for a missing provider reference with a caller-supplied message.
    pub fn with_message(
        provider: impl Into<String>,
        reference: ProviderReference,
        message: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            reference,
            message: message.into(),
        }
    }

    /// Returns the provider whose reference was requested.
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Returns the full provider reference that could not satisfy the lookup.
    pub fn reference(&self) -> &ProviderReference {
        &self.reference
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the provider reference that failed lookup.
    pub fn into_reference(self) -> ProviderReference {
        self.reference
    }

    /// Converts this error into the provider id, provider reference, and message.
    pub fn into_parts(self) -> (String, ProviderReference, String) {
        (self.provider, self.reference, self.message)
    }
}

impl fmt::Display for NoSuchProviderReferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for NoSuchProviderReferenceError {}

fn no_such_provider_reference_default_message(
    provider: &str,
    reference: &ProviderReference,
) -> String {
    let available_providers = reference
        .as_map()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "No provider reference found for provider '{provider}'. Available providers: {available_providers}"
    )
}

/// A mapping of provider names to provider-specific file identifiers.
///
/// This mirrors the AI SDK's `SharedV4ProviderReference` shape while rejecting
/// the reserved `type` key that would conflict with tagged file-data variants.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProviderReference(BTreeMap<String, String>);

impl ProviderReference {
    /// Creates a provider reference from a provider-to-file-id map.
    pub fn from_map(map: BTreeMap<String, String>) -> Result<Self, ProviderReferenceError> {
        if map.contains_key("type") {
            return Err(ProviderReferenceError);
        }

        Ok(Self(map))
    }

    /// Returns the provider-to-file-id map.
    pub fn as_map(&self) -> &BTreeMap<String, String> {
        &self.0
    }

    /// Converts this provider reference into its provider-to-file-id map.
    pub fn into_map(self) -> BTreeMap<String, String> {
        self.0
    }

    /// Returns the provider-specific id for the requested provider.
    pub fn provider_id(&self, provider: &str) -> Result<&str, NoSuchProviderReferenceError> {
        self.0
            .get(provider)
            .map(String::as_str)
            .ok_or_else(|| NoSuchProviderReferenceError::new(provider.to_string(), self.clone()))
    }
}

impl TryFrom<BTreeMap<String, String>> for ProviderReference {
    type Error = ProviderReferenceError;

    fn try_from(map: BTreeMap<String, String>) -> Result<Self, Self::Error> {
        Self::from_map(map)
    }
}

impl Serialize for ProviderReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ProviderReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let map = BTreeMap::<String, String>::deserialize(deserializer)?;
        Self::from_map(map).map_err(de::Error::custom)
    }
}

/// Raw file content represented either as bytes or as a base64-encoded string.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum FileDataContent {
    /// Raw bytes, equivalent to the AI SDK's `Uint8Array` option.
    Bytes(Vec<u8>),

    /// Base64-encoded file content.
    Base64(String),
}

/// File data as a tagged discriminated union.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FileData {
    /// Raw bytes or base64-encoded content.
    Data { data: FileDataContent },

    /// A URL pointing to the file.
    Url { url: Url },

    /// A provider-specific file reference.
    Reference {
        /// Provider-to-file-id mapping.
        reference: ProviderReference,
    },

    /// Inline text file content.
    Text { text: String },
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use url::Url;

    use super::{FileData, FileDataContent, NoSuchProviderReferenceError, ProviderReference};

    #[test]
    fn file_data_serializes_base64_data_variant() {
        let file = FileData::Data {
            data: FileDataContent::Base64("aGVsbG8=".to_string()),
        };

        assert_eq!(
            serde_json::to_value(file).expect("file data serializes"),
            json!({
                "type": "data",
                "data": "aGVsbG8="
            })
        );
    }

    #[test]
    fn file_data_serializes_url_variant_as_string() {
        let file = FileData::Url {
            url: Url::parse("https://example.com/file.png").expect("valid URL"),
        };

        assert_eq!(
            serde_json::to_value(file).expect("file data serializes"),
            json!({
                "type": "url",
                "url": "https://example.com/file.png"
            })
        );
    }

    #[test]
    fn file_data_round_trips_provider_reference_variant() {
        let reference = ProviderReference::try_from(BTreeMap::from([(
            "openai".to_string(),
            "file-abc123".to_string(),
        )]))
        .expect("provider reference is valid");

        let file = FileData::Reference { reference };
        let value = serde_json::to_value(&file).expect("file data serializes");

        assert_eq!(
            value,
            json!({
                "type": "reference",
                "reference": {
                    "openai": "file-abc123"
                }
            })
        );

        assert_eq!(
            serde_json::from_value::<FileData>(value).expect("file data deserializes"),
            file
        );
    }

    #[test]
    fn provider_reference_rejects_reserved_type_key() {
        let error = serde_json::from_value::<ProviderReference>(json!({ "type": "file-abc123" }))
            .expect_err("reserved type key is rejected");

        assert!(
            error
                .to_string()
                .contains("provider references cannot contain the reserved `type` key")
        );
    }

    #[test]
    fn provider_reference_resolves_provider_specific_id() {
        let reference = ProviderReference::try_from(BTreeMap::from([
            ("anthropic".to_string(), "file-xyz789".to_string()),
            ("openai".to_string(), "file-abc123".to_string()),
        ]))
        .expect("provider reference is valid");

        assert_eq!(
            reference
                .provider_id("openai")
                .expect("openai provider id is present"),
            "file-abc123"
        );
    }

    #[test]
    fn missing_provider_reference_error_matches_upstream_context() {
        let reference = ProviderReference::try_from(BTreeMap::from([
            ("anthropic".to_string(), "file-xyz789".to_string()),
            ("openai".to_string(), "file-abc123".to_string()),
        ]))
        .expect("provider reference is valid");

        let error = reference
            .provider_id("google")
            .expect_err("google provider id is missing");

        assert_eq!(error.provider(), "google");
        assert_eq!(error.reference(), &reference);
        assert_eq!(
            error.message(),
            "No provider reference found for provider 'google'. Available providers: anthropic, openai"
        );
        assert_eq!(
            error.to_string(),
            "No provider reference found for provider 'google'. Available providers: anthropic, openai"
        );

        let explicit_error = NoSuchProviderReferenceError::new("google", reference.clone());
        assert_eq!(explicit_error.into_reference(), reference);
    }

    #[test]
    fn missing_provider_reference_error_accepts_upstream_custom_message() {
        let reference = ProviderReference::try_from(BTreeMap::from([(
            "openai".to_string(),
            "file-abc123".to_string(),
        )]))
        .expect("provider reference is valid");

        let error = NoSuchProviderReferenceError::with_message(
            "anthropic",
            reference.clone(),
            "Provider reference cannot be used by Anthropic.",
        );

        assert_eq!(error.provider(), "anthropic");
        assert_eq!(error.reference(), &reference);
        assert_eq!(
            error.message(),
            "Provider reference cannot be used by Anthropic."
        );
        assert_eq!(
            error.to_string(),
            "Provider reference cannot be used by Anthropic."
        );
        assert_eq!(
            error.into_parts(),
            (
                "anthropic".to_string(),
                reference,
                "Provider reference cannot be used by Anthropic.".to_string()
            )
        );
    }
}
