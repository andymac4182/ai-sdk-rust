//! Portable workflow helpers for the Rust port of upstream `@ai-sdk/workflow`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use ai_sdk_provider::json::{JsonObject, JsonSchema};
use ai_sdk_provider_utils::Tool;
use serde::{Deserialize, Serialize};

/// The workflow crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Plain-object tool definitions that can cross workflow step boundaries.
pub type SerializableToolSet = BTreeMap<String, SerializableToolDef>;

/// Serializable tool definition.
///
/// This mirrors the portable fields from upstream `SerializableToolDef`.
/// Runtime-only callbacks and executors are intentionally stripped.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializableToolDef {
    /// Function tool description, when one was configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// JSON Schema 7 object describing the tool input.
    pub input_schema: JsonSchema,

    /// Provider tool discriminator.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub tool_type: Option<SerializableToolType>,

    /// Whether a provider tool is executed by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_provider_executed: Option<bool>,

    /// Provider tool identifier, for example `anthropic.web_search_20250305`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Provider tool configuration arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<JsonObject>,
}

impl SerializableToolDef {
    /// Creates a serializable function-tool definition.
    pub fn function(input_schema: JsonSchema) -> Self {
        Self {
            description: None,
            input_schema,
            tool_type: None,
            is_provider_executed: None,
            id: None,
            args: None,
        }
    }

    /// Sets the function tool description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Creates a serializable provider-tool definition.
    pub fn provider(
        id: impl Into<String>,
        args: JsonObject,
        input_schema: JsonSchema,
        is_provider_executed: bool,
    ) -> Self {
        Self {
            description: None,
            input_schema,
            tool_type: Some(SerializableToolType::Provider),
            is_provider_executed: Some(is_provider_executed),
            id: Some(id.into()),
            args: Some(args),
        }
    }
}

/// Serializable tool discriminator.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SerializableToolType {
    /// Provider-defined tool.
    Provider,
}

/// Error returned when a serialized tool cannot be reconstructed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializableToolError {
    /// A provider tool definition omitted its required provider id.
    MissingProviderToolId { tool_name: String },
}

impl fmt::Display for SerializableToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProviderToolId { tool_name } => {
                write!(formatter, "provider tool '{tool_name}' is missing an id")
            }
        }
    }
}

impl Error for SerializableToolError {}

/// Converts runtime tools into plain serializable tool definitions.
///
/// This mirrors upstream `serializeToolSet`: descriptions, input schemas, and
/// provider tool identity survive serialization; runtime callbacks do not.
pub fn serialize_tool_set(tools: impl IntoIterator<Item = Tool>) -> SerializableToolSet {
    tools
        .into_iter()
        .map(|tool| {
            let name = tool.name.clone();
            let mut serializable_tool = SerializableToolDef::function(tool.input_schema.clone());
            serializable_tool.description = tool.description.clone();

            if let Some(provider_tool_id) = tool.provider_tool_id() {
                serializable_tool.tool_type = Some(SerializableToolType::Provider);
                serializable_tool.is_provider_executed = Some(tool.is_provider_executed());
                serializable_tool.id = Some(provider_tool_id.to_string());
                serializable_tool.args = tool.provider_tool_args().cloned();
            }

            (name, serializable_tool)
        })
        .collect()
}

/// Reconstructs workflow tools from serializable definitions.
pub fn resolve_serializable_tools(
    tools: &SerializableToolSet,
) -> Result<BTreeMap<String, Tool>, SerializableToolError> {
    tools
        .iter()
        .map(|(name, tool)| {
            let resolved_tool = match tool.tool_type {
                Some(SerializableToolType::Provider) => {
                    let id = tool.id.clone().ok_or_else(|| {
                        SerializableToolError::MissingProviderToolId {
                            tool_name: name.clone(),
                        }
                    })?;
                    Tool::provider_tool(
                        name.clone(),
                        id,
                        tool.args.clone().unwrap_or_default(),
                        tool.input_schema.clone(),
                        tool.is_provider_executed.unwrap_or(false),
                    )
                }
                None => {
                    let mut function_tool = Tool::new(name.clone(), tool.input_schema.clone());
                    if let Some(description) = &tool.description {
                        function_tool = function_tool.with_description(description.clone());
                    }
                    function_tool
                }
            };

            Ok((name.clone(), resolved_tool))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn weather_schema() -> JsonSchema {
        serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "city": {
                    "type": "string"
                }
            },
            "required": ["city"]
        }))
        .expect("schema is an object")
    }

    #[test]
    fn serialize_tool_set_serializes_function_tools_with_description_and_input_schema() {
        let tools = vec![
            Tool::new("getWeather", weather_schema()).with_description("Get weather for a city"),
        ];

        assert_eq!(
            serialize_tool_set(tools),
            BTreeMap::from([(
                "getWeather".to_string(),
                SerializableToolDef::function(weather_schema())
                    .with_description("Get weather for a city")
            )])
        );
    }

    #[test]
    fn serialize_tool_set_preserves_provider_tool_identity_and_args() {
        let tools = vec![Tool::provider_tool(
            "webSearch",
            "anthropic.web_search_20250305",
            serde_json::from_value(json!({
                "maxUses": 5,
                "allowedDomains": ["vercel.com", "nextjs.org"]
            }))
            .expect("args are an object"),
            weather_schema(),
            true,
        )];

        let serialized = serialize_tool_set(tools);

        assert_eq!(
            serialized.get("webSearch"),
            Some(&SerializableToolDef::provider(
                "anthropic.web_search_20250305",
                serde_json::from_value(json!({
                    "maxUses": 5,
                    "allowedDomains": ["vercel.com", "nextjs.org"]
                }))
                .expect("args are an object"),
                weather_schema(),
                true,
            ))
        );
    }

    #[test]
    fn resolve_serializable_tools_reconstructs_function_tools() {
        let tools = BTreeMap::from([(
            "getWeather".to_string(),
            SerializableToolDef::function(weather_schema())
                .with_description("Get weather for a city"),
        )]);

        let resolved = resolve_serializable_tools(&tools).expect("tools resolve");
        let tool = resolved.get("getWeather").expect("tool exists");

        assert_eq!(tool.name, "getWeather");
        assert_eq!(tool.description.as_deref(), Some("Get weather for a city"));
        assert_eq!(tool.input_schema, weather_schema());
        assert!(!tool.is_provider_tool());
    }

    #[test]
    fn resolve_serializable_tools_reconstructs_provider_tools() {
        let args: JsonObject = serde_json::from_value(json!({
            "maxUses": 5,
            "allowedDomains": ["vercel.com"]
        }))
        .expect("args are an object");
        let tools = BTreeMap::from([(
            "webSearch".to_string(),
            SerializableToolDef::provider(
                "anthropic.web_search_20250305",
                args.clone(),
                weather_schema(),
                true,
            ),
        )]);

        let resolved = resolve_serializable_tools(&tools).expect("tools resolve");
        let tool = resolved.get("webSearch").expect("tool exists");

        assert!(tool.is_provider_tool());
        assert!(tool.is_provider_executed());
        assert_eq!(
            tool.provider_tool_id(),
            Some("anthropic.web_search_20250305")
        );
        assert_eq!(tool.provider_tool_args(), Some(&args));
    }

    #[test]
    fn resolve_serializable_tools_reports_missing_provider_tool_id() {
        let tools = BTreeMap::from([(
            "webSearch".to_string(),
            SerializableToolDef {
                description: None,
                input_schema: weather_schema(),
                tool_type: Some(SerializableToolType::Provider),
                is_provider_executed: Some(true),
                id: None,
                args: None,
            },
        )]);

        let error = resolve_serializable_tools(&tools).expect_err("provider id is required");
        assert_eq!(
            error,
            SerializableToolError::MissingProviderToolId {
                tool_name: "webSearch".to_string()
            }
        );
    }
}
