use std::collections::BTreeMap;

use crate::json::JsonObject;

/// Additional provider-specific options passed through to a model provider.
///
/// The outer map is keyed by provider name and the inner object contains
/// provider-specific option keys.
pub type ProviderOptions = BTreeMap<String, JsonObject>;

/// Additional provider-specific metadata returned by a model provider.
///
/// The shape matches [`ProviderOptions`], but represents provider outputs rather
/// than provider inputs.
pub type ProviderMetadata = BTreeMap<String, JsonObject>;

#[cfg(test)]
mod tests {
    use super::ProviderOptions;
    use serde_json::json;

    #[test]
    fn provider_options_serialize_as_nested_provider_objects() {
        let options: ProviderOptions = serde_json::from_value(json!({
            "anthropic": {
                "cacheControl": { "type": "ephemeral" }
            }
        }))
        .expect("provider options deserialize");

        assert_eq!(
            serde_json::to_value(options).expect("provider options serialize"),
            json!({
                "anthropic": {
                    "cacheControl": { "type": "ephemeral" }
                }
            })
        );
    }
}
