use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Reserved key prefix for system-managed attributes.
pub const RESERVED_ATTRIBUTE_KEY_PREFIX: char = '$';

/// Max length of an attribute key, in Unicode scalar values.
pub const ATTRIBUTE_KEY_MAX_LENGTH: usize = 256;

/// Max length of an attribute value, in bytes (UTF-8).
pub const ATTRIBUTE_VALUE_MAX_BYTES: usize = 256;

/// Max number of attributes on a single run after applying a change batch.
pub const ATTRIBUTE_MAX_PER_RUN: usize = 64;

/// String-string metadata attached to a workflow run.
pub type Attributes = BTreeMap<String, String>;

/// A single change in an experimental set-attributes call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeChange {
    pub key: String,
    pub value: Option<String>,
}

impl AttributeChange {
    pub fn set(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: Some(value.into()),
        }
    }

    pub fn unset(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: None,
        }
    }
}

/// Result returned by worlds after applying attribute changes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentalSetAttributesResult {
    pub attributes: Attributes,
}

/// Options for validating one attribute key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttributeKeyValidationOptions {
    pub allow_reserved_attributes: bool,
}

impl AttributeKeyValidationOptions {
    pub const fn allow_reserved_attributes() -> Self {
        Self {
            allow_reserved_attributes: true,
        }
    }
}

/// Context needed to validate a batch of attribute changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttributeValidationContext {
    pub existing_keys: Option<BTreeSet<String>>,
    pub allow_reserved_attributes: bool,
}

impl AttributeValidationContext {
    pub fn with_existing_keys(keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            existing_keys: Some(keys.into_iter().map(Into::into).collect()),
            allow_reserved_attributes: false,
        }
    }

    pub const fn allow_reserved_attributes(mut self) -> Self {
        self.allow_reserved_attributes = true;
        self
    }
}

/// Error returned when an attribute key or value violates the world contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributeValidationError {
    message: String,
}

impl AttributeValidationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for AttributeValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for AttributeValidationError {}

/// Validate a single attribute key.
pub fn validate_attribute_key(
    key: &str,
    options: AttributeKeyValidationOptions,
) -> Result<(), AttributeValidationError> {
    let char_count = key.chars().count();
    if char_count == 0 {
        return Err(AttributeValidationError::new(
            "Attribute key must not be empty",
        ));
    }
    if char_count > ATTRIBUTE_KEY_MAX_LENGTH {
        return Err(AttributeValidationError::new(format!(
            "Attribute key length {char_count} exceeds limit {ATTRIBUTE_KEY_MAX_LENGTH}"
        )));
    }
    if !options.allow_reserved_attributes && key.starts_with(RESERVED_ATTRIBUTE_KEY_PREFIX) {
        return Err(AttributeValidationError::new(format!(
            "Attribute key {key:?} starts with reserved prefix \"{RESERVED_ATTRIBUTE_KEY_PREFIX}\""
        )));
    }
    Ok(())
}

/// Validate a single attribute value. `None` represents an unset and is valid.
pub fn validate_attribute_value(value: Option<&str>) -> Result<(), AttributeValidationError> {
    if let Some(value) = value {
        let bytes = value.len();
        if bytes > ATTRIBUTE_VALUE_MAX_BYTES {
            return Err(AttributeValidationError::new(format!(
                "Attribute value byte length {bytes} exceeds limit {ATTRIBUTE_VALUE_MAX_BYTES}"
            )));
        }
    }
    Ok(())
}

/// Validate a batch of attribute changes against duplicate, size, and reserved-key rules.
pub fn validate_attribute_changes(
    changes: &[AttributeChange],
    context: &AttributeValidationContext,
) -> Result<(), AttributeValidationError> {
    let mut seen_keys = BTreeSet::new();
    let mut net_adds = 0usize;
    let mut net_deletes = 0usize;

    for change in changes {
        validate_attribute_key(
            &change.key,
            AttributeKeyValidationOptions {
                allow_reserved_attributes: context.allow_reserved_attributes,
            },
        )?;
        validate_attribute_value(change.value.as_deref())?;

        if !seen_keys.insert(change.key.clone()) {
            return Err(AttributeValidationError::new(format!(
                "Attribute key {:?} appears more than once in the same batch",
                change.key
            )));
        }

        match (&context.existing_keys, &change.value) {
            (Some(existing_keys), Some(_)) if existing_keys.contains(&change.key) => {}
            (_, Some(_)) => net_adds += 1,
            (Some(existing_keys), None) if existing_keys.contains(&change.key) => {
                net_deletes += 1;
            }
            (None, None) => net_deletes += 1,
            (Some(_), None) => {}
        }
    }

    let existing = context.existing_keys.as_ref().map_or(0usize, BTreeSet::len);
    let post_merge = existing + net_adds - net_deletes.min(existing + net_adds);
    if post_merge > ATTRIBUTE_MAX_PER_RUN {
        return Err(AttributeValidationError::new(format!(
            "Run attribute count would exceed limit {ATTRIBUTE_MAX_PER_RUN} (post-merge {post_merge})"
        )));
    }

    Ok(())
}

/// Apply a batch of validated changes to a new copy of the existing attribute map.
pub fn apply_attribute_changes(
    existing: Option<&Attributes>,
    changes: &[AttributeChange],
) -> Attributes {
    let mut next = existing.cloned().unwrap_or_default();
    for change in changes {
        if let Some(value) = &change.value {
            next.insert(change.key.clone(), value.clone());
        } else {
            next.remove(&change.key);
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(entries: &[(&str, &str)]) -> Attributes {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn validate_attribute_key_accepts_a_normal_key() {
        assert!(validate_attribute_key("phase", AttributeKeyValidationOptions::default()).is_ok());
    }

    #[test]
    fn validate_attribute_key_rejects_empty_keys() {
        assert!(validate_attribute_key("", AttributeKeyValidationOptions::default()).is_err());
    }

    #[test]
    fn validate_attribute_key_rejects_keys_over_the_length_cap() {
        let key = "k".repeat(ATTRIBUTE_KEY_MAX_LENGTH + 1);
        assert!(validate_attribute_key(&key, AttributeKeyValidationOptions::default()).is_err());
    }

    #[test]
    fn validate_attribute_key_accepts_keys_exactly_at_the_length_cap() {
        let key = "k".repeat(ATTRIBUTE_KEY_MAX_LENGTH);
        assert!(validate_attribute_key(&key, AttributeKeyValidationOptions::default()).is_ok());
    }

    #[test]
    fn validate_attribute_key_rejects_keys_starting_with_reserved_prefix_by_default() {
        assert!(
            validate_attribute_key("$internal", AttributeKeyValidationOptions::default()).is_err()
        );
    }

    #[test]
    fn validate_attribute_key_accepts_reserved_prefix_keys_when_allow_reserved_attributes_is_set() {
        assert!(
            validate_attribute_key(
                "$internal",
                AttributeKeyValidationOptions::allow_reserved_attributes()
            )
            .is_ok()
        );
    }

    #[test]
    fn validate_attribute_key_still_rejects_reserved_prefix_keys_when_allow_reserved_attributes_is_explicitly_false()
     {
        assert!(
            validate_attribute_key(
                "$internal",
                AttributeKeyValidationOptions {
                    allow_reserved_attributes: false,
                },
            )
            .is_err()
        );
    }

    #[test]
    fn validate_attribute_value_accepts_null_unset() {
        assert!(validate_attribute_value(None).is_ok());
    }

    #[test]
    fn validate_attribute_value_accepts_a_normal_string() {
        assert!(validate_attribute_value(Some("hello")).is_ok());
    }

    #[test]
    fn validate_attribute_value_rejects_values_over_the_byte_cap() {
        let value = "a".repeat(257);
        assert!(validate_attribute_value(Some(&value)).is_err());
    }

    #[test]
    fn validate_attribute_value_counts_utf8_bytes_not_characters() {
        let at_cap = "💥".repeat(64);
        assert!(validate_attribute_value(Some(&at_cap)).is_ok());

        let over_cap = "💥".repeat(65);
        assert!(validate_attribute_value(Some(&over_cap)).is_err());
    }

    #[test]
    fn validate_attribute_changes_accepts_a_small_batch_of_valid_changes() {
        let changes = [
            AttributeChange::set("phase", "init"),
            AttributeChange::unset("stale"),
        ];
        assert!(
            validate_attribute_changes(&changes, &AttributeValidationContext::default()).is_ok()
        );
    }

    #[test]
    fn validate_attribute_changes_rejects_duplicate_keys_within_a_single_batch() {
        let changes = [
            AttributeChange::set("phase", "init"),
            AttributeChange::set("phase", "done"),
        ];
        assert!(
            validate_attribute_changes(&changes, &AttributeValidationContext::default()).is_err()
        );
    }

    #[test]
    fn validate_attribute_changes_rejects_when_post_merge_count_exceeds_the_per_run_cap() {
        let changes: Vec<_> = (0..ATTRIBUTE_MAX_PER_RUN)
            .map(|index| AttributeChange::set(format!("k{index}"), "v"))
            .collect();
        let context = AttributeValidationContext::with_existing_keys(["preexisting"]);

        assert!(validate_attribute_changes(&changes, &context).is_err());
    }

    #[test]
    fn validate_attribute_changes_does_not_count_upserts_on_already_present_keys_against_the_cap() {
        let existing_keys = (0..ATTRIBUTE_MAX_PER_RUN).map(|index| format!("k{index}"));
        let context = AttributeValidationContext::with_existing_keys(existing_keys);
        let changes = [AttributeChange::set("k0", "updated")];

        assert!(validate_attribute_changes(&changes, &context).is_ok());
    }

    #[test]
    fn validate_attribute_changes_rejects_reserved_prefix_keys_in_a_batch_by_default() {
        let changes = [
            AttributeChange::set("phase", "init"),
            AttributeChange::set("$framework.kind", "agent"),
        ];

        assert!(
            validate_attribute_changes(&changes, &AttributeValidationContext::default()).is_err()
        );
    }

    #[test]
    fn validate_attribute_changes_accepts_reserved_prefix_keys_when_allow_reserved_attributes_is_set()
     {
        let changes = [
            AttributeChange::set("phase", "init"),
            AttributeChange::set("$framework.kind", "agent"),
        ];
        let context = AttributeValidationContext::default().allow_reserved_attributes();

        assert!(validate_attribute_changes(&changes, &context).is_ok());
    }

    #[test]
    fn apply_attribute_changes_upserts_new_keys() {
        let before = attrs(&[("a", "1")]);
        let after = apply_attribute_changes(Some(&before), &[AttributeChange::set("b", "2")]);

        assert_eq!(after, attrs(&[("a", "1"), ("b", "2")]));
    }

    #[test]
    fn apply_attribute_changes_overwrites_existing_keys() {
        let before = attrs(&[("a", "1")]);
        let after = apply_attribute_changes(Some(&before), &[AttributeChange::set("a", "2")]);

        assert_eq!(after, attrs(&[("a", "2")]));
    }

    #[test]
    fn apply_attribute_changes_removes_keys_when_value_is_null() {
        let before = attrs(&[("a", "1"), ("b", "2")]);
        let after = apply_attribute_changes(Some(&before), &[AttributeChange::unset("a")]);

        assert_eq!(after, attrs(&[("b", "2")]));
    }

    #[test]
    fn apply_attribute_changes_applies_set_and_unset_in_a_single_batch() {
        let before = attrs(&[("a", "1"), ("stale", "x")]);
        let changes = [
            AttributeChange::unset("stale"),
            AttributeChange::set("fresh", "yes"),
        ];
        let after = apply_attribute_changes(Some(&before), &changes);

        assert_eq!(after, attrs(&[("a", "1"), ("fresh", "yes")]));
    }

    #[test]
    fn apply_attribute_changes_returns_a_new_object_does_not_mutate_input() {
        let before = attrs(&[("a", "1")]);
        let after = apply_attribute_changes(Some(&before), &[AttributeChange::set("b", "2")]);

        assert_eq!(before, attrs(&[("a", "1")]));
        assert_eq!(after, attrs(&[("a", "1"), ("b", "2")]));
    }

    #[test]
    fn apply_attribute_changes_treats_undefined_existing_as_the_empty_record() {
        let after = apply_attribute_changes(None, &[AttributeChange::set("a", "1")]);

        assert_eq!(after, attrs(&[("a", "1")]));
    }
}
