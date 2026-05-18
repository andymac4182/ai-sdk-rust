use serde::{Deserialize, Serialize};

/// Warning returned by a model provider.
///
/// Warnings describe unsupported settings, compatibility behavior, deprecated
/// options, or provider-specific messages that do not prevent a call from
/// completing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Warning {
    /// A requested feature is not supported by the model.
    Unsupported {
        /// The unsupported feature name.
        feature: String,

        /// Additional provider details about the unsupported feature.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<String>,
    },

    /// A compatibility feature is being used and may produce suboptimal results.
    Compatibility {
        /// The feature used in compatibility mode.
        feature: String,

        /// Additional provider details about the compatibility behavior.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<String>,
    },

    /// A deprecated feature or option is being used.
    Deprecated {
        /// The deprecated setting or feature name.
        setting: String,

        /// Human-readable guidance for replacing the deprecated setting.
        message: String,
    },

    /// Other provider warning.
    Other {
        /// The warning message.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::Warning;
    use serde_json::json;

    #[test]
    fn warning_serializes_optional_details_for_unsupported_features() {
        let warning = Warning::Unsupported {
            feature: "topK".to_string(),
            details: Some("The selected model ignores topK.".to_string()),
        };

        assert_eq!(
            serde_json::to_value(warning).expect("warning serializes"),
            json!({
                "type": "unsupported",
                "feature": "topK",
                "details": "The selected model ignores topK."
            })
        );
    }

    #[test]
    fn warning_omits_missing_optional_details() {
        let warning = Warning::Compatibility {
            feature: "json-mode".to_string(),
            details: None,
        };

        assert_eq!(
            serde_json::to_value(warning).expect("warning serializes"),
            json!({
                "type": "compatibility",
                "feature": "json-mode"
            })
        );
    }

    #[test]
    fn warning_deserializes_deprecated_and_other_variants() {
        let deprecated: Warning = serde_json::from_value(json!({
            "type": "deprecated",
            "setting": "functionCalling",
            "message": "Use tools instead."
        }))
        .expect("deprecated warning deserializes");

        assert_eq!(
            deprecated,
            Warning::Deprecated {
                setting: "functionCalling".to_string(),
                message: "Use tools instead.".to_string(),
            }
        );

        let other: Warning = serde_json::from_value(json!({
            "type": "other",
            "message": "Provider returned a non-fatal warning."
        }))
        .expect("other warning deserializes");

        assert_eq!(
            other,
            Warning::Other {
                message: "Provider returned a non-fatal warning.".to_string(),
            }
        );
    }
}
