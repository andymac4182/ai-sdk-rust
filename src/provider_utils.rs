use std::collections::BTreeMap;
use std::env::{self, VarError};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::file_data::{
    FileData, FileDataContent, NoSuchProviderReferenceError, ProviderReference,
};
use crate::headers::Headers;
use crate::json::{JsonObject, JsonSchema, JsonValue};
use crate::language_model::{
    LanguageModelFilePart, LanguageModelFunctionTool, LanguageModelMessage, LanguageModelPrompt,
    LanguageModelReasoningEffort, LanguageModelSystemMessage, LanguageModelTool,
    LanguageModelToolInputExample,
};
use crate::provider::{
    LoadApiKeyError, LoadSettingError, ProviderOptions, UnsupportedFunctionalityError,
};
use crate::warning::Warning;

const DEFAULT_JSON_SCHEMA_INSTRUCTION_PREFIX: &str = "JSON schema:";
const DEFAULT_JSON_SCHEMA_INSTRUCTION_SUFFIX: &str =
    "You MUST answer with a JSON object that matches the JSON schema above.";
const DEFAULT_JSON_INSTRUCTION_SUFFIX: &str = "You MUST answer with JSON.";

struct MediaTypeSignature {
    media_type: &'static str,
    bytes_prefix: &'static [Option<u8>],
}

const IMAGE_MEDIA_TYPE_SIGNATURES: &[MediaTypeSignature] = &[
    MediaTypeSignature {
        media_type: "image/gif",
        bytes_prefix: &[Some(0x47), Some(0x49), Some(0x46)],
    },
    MediaTypeSignature {
        media_type: "image/png",
        bytes_prefix: &[Some(0x89), Some(0x50), Some(0x4e), Some(0x47)],
    },
    MediaTypeSignature {
        media_type: "image/jpeg",
        bytes_prefix: &[Some(0xff), Some(0xd8)],
    },
    MediaTypeSignature {
        media_type: "image/webp",
        bytes_prefix: &[
            Some(0x52),
            Some(0x49),
            Some(0x46),
            Some(0x46),
            None,
            None,
            None,
            None,
            Some(0x57),
            Some(0x45),
            Some(0x42),
            Some(0x50),
        ],
    },
    MediaTypeSignature {
        media_type: "image/bmp",
        bytes_prefix: &[Some(0x42), Some(0x4d)],
    },
    MediaTypeSignature {
        media_type: "image/tiff",
        bytes_prefix: &[Some(0x49), Some(0x49), Some(0x2a), Some(0x00)],
    },
    MediaTypeSignature {
        media_type: "image/tiff",
        bytes_prefix: &[Some(0x4d), Some(0x4d), Some(0x00), Some(0x2a)],
    },
    MediaTypeSignature {
        media_type: "image/avif",
        bytes_prefix: &[
            Some(0x00),
            Some(0x00),
            Some(0x00),
            Some(0x20),
            Some(0x66),
            Some(0x74),
            Some(0x79),
            Some(0x70),
            Some(0x61),
            Some(0x76),
            Some(0x69),
            Some(0x66),
        ],
    },
    MediaTypeSignature {
        media_type: "image/heic",
        bytes_prefix: &[
            Some(0x00),
            Some(0x00),
            Some(0x00),
            Some(0x20),
            Some(0x66),
            Some(0x74),
            Some(0x79),
            Some(0x70),
            Some(0x68),
            Some(0x65),
            Some(0x69),
            Some(0x63),
        ],
    },
];

const DOCUMENT_MEDIA_TYPE_SIGNATURES: &[MediaTypeSignature] = &[MediaTypeSignature {
    media_type: "application/pdf",
    bytes_prefix: &[Some(0x25), Some(0x50), Some(0x44), Some(0x46)],
}];

const AUDIO_MEDIA_TYPE_SIGNATURES: &[MediaTypeSignature] = &[
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xfb)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xfa)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xf3)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xf2)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xe3)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xe2)],
    },
    MediaTypeSignature {
        media_type: "audio/wav",
        bytes_prefix: &[
            Some(0x52),
            Some(0x49),
            Some(0x46),
            Some(0x46),
            None,
            None,
            None,
            None,
            Some(0x57),
            Some(0x41),
            Some(0x56),
            Some(0x45),
        ],
    },
    MediaTypeSignature {
        media_type: "audio/ogg",
        bytes_prefix: &[Some(0x4f), Some(0x67), Some(0x67), Some(0x53)],
    },
    MediaTypeSignature {
        media_type: "audio/flac",
        bytes_prefix: &[Some(0x66), Some(0x4c), Some(0x61), Some(0x43)],
    },
    MediaTypeSignature {
        media_type: "audio/aac",
        bytes_prefix: &[Some(0x40), Some(0x15), Some(0x00), Some(0x00)],
    },
    MediaTypeSignature {
        media_type: "audio/mp4",
        bytes_prefix: &[Some(0x66), Some(0x74), Some(0x79), Some(0x70)],
    },
    MediaTypeSignature {
        media_type: "audio/webm",
        bytes_prefix: &[Some(0x1a), Some(0x45), Some(0xdf), Some(0xa3)],
    },
];

const VIDEO_MEDIA_TYPE_SIGNATURES: &[MediaTypeSignature] = &[
    MediaTypeSignature {
        media_type: "video/mp4",
        bytes_prefix: &[
            Some(0x00),
            Some(0x00),
            Some(0x00),
            None,
            Some(0x66),
            Some(0x74),
            Some(0x79),
            Some(0x70),
        ],
    },
    MediaTypeSignature {
        media_type: "video/webm",
        bytes_prefix: &[Some(0x1a), Some(0x45), Some(0xdf), Some(0xa3)],
    },
    MediaTypeSignature {
        media_type: "video/quicktime",
        bytes_prefix: &[
            Some(0x00),
            Some(0x00),
            Some(0x00),
            Some(0x14),
            Some(0x66),
            Some(0x74),
            Some(0x79),
            Some(0x70),
            Some(0x71),
            Some(0x74),
        ],
    },
    MediaTypeSignature {
        media_type: "video/x-msvideo",
        bytes_prefix: &[Some(0x52), Some(0x49), Some(0x46), Some(0x46)],
    },
];

/// Future returned by a Rust tool execution function.
pub type ToolExecuteFuture =
    Pin<Box<dyn Future<Output = Result<JsonValue, ToolExecutionError>> + Send>>;

/// Function used to execute a Rust tool call.
pub type ToolExecuteFunction =
    dyn Fn(JsonValue, ToolExecutionOptions) -> ToolExecuteFuture + Send + Sync + 'static;

/// Options passed to a tool execution function.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionOptions {
    /// Identifier of the model tool call being executed.
    pub tool_call_id: String,

    /// Prompt messages sent to the model for the step that produced the tool call.
    pub messages: LanguageModelPrompt,
}

impl ToolExecutionOptions {
    /// Creates tool execution options.
    pub fn new(tool_call_id: impl Into<String>, messages: LanguageModelPrompt) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            messages,
        }
    }
}

/// Error returned by a Rust tool execution function.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionError {
    /// Human-readable execution failure message.
    pub message: String,
}

impl ToolExecutionError {
    /// Creates a tool execution error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the execution failure message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its message.
    pub fn into_message(self) -> String {
        self.message
    }
}

impl fmt::Display for ToolExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ToolExecutionError {}

impl From<String> for ToolExecutionError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for ToolExecutionError {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

/// User-defined Rust function tool made available to a language model call.
///
/// This mirrors the function-tool branch of upstream `@ai-sdk/provider-utils`
/// `Tool`: it carries model-facing schema/description metadata and may include
/// an executor for later client-side tool handling.
#[derive(Clone)]
pub struct Tool {
    /// Name of the tool, unique within a model call.
    pub name: String,

    /// Optional description of what the tool does.
    pub description: Option<String>,

    /// JSON Schema 7 object describing the tool input.
    pub input_schema: JsonSchema,

    /// Optional examples that show the model what inputs should look like.
    pub input_examples: Option<Vec<LanguageModelToolInputExample>>,

    /// Strict mode setting for providers that support it.
    pub strict: Option<bool>,

    /// Provider-specific options sent with the tool definition.
    pub provider_options: Option<ProviderOptions>,

    execute: Option<Arc<ToolExecuteFunction>>,
}

impl Tool {
    /// Creates a function tool definition.
    pub fn new(name: impl Into<String>, input_schema: JsonSchema) -> Self {
        Self {
            name: name.into(),
            description: None,
            input_schema,
            input_examples: None,
            strict: None,
            provider_options: None,
            execute: None,
        }
    }

    /// Sets the tool description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds a tool input example.
    pub fn with_input_example(mut self, input: JsonObject) -> Self {
        self.input_examples
            .get_or_insert_with(Vec::new)
            .push(LanguageModelToolInputExample::new(input));
        self
    }

    /// Sets strict mode for providers that support it.
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = Some(strict);
        self
    }

    /// Adds provider-specific options to this tool.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Sets the Rust executor for this tool.
    pub fn with_execute<F, Fut>(mut self, execute: F) -> Self
    where
        F: Fn(JsonValue, ToolExecutionOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<JsonValue, ToolExecutionError>> + Send + 'static,
    {
        self.execute = Some(Arc::new(move |input, options| {
            Box::pin(execute(input, options))
        }));
        self
    }

    /// Returns whether this tool has an executor.
    pub fn is_executable(&self) -> bool {
        self.execute.is_some()
    }

    /// Executes this tool when an executor is present.
    pub fn execute(
        &self,
        input: JsonValue,
        options: ToolExecutionOptions,
    ) -> Option<ToolExecuteFuture> {
        self.execute.as_ref().map(|execute| execute(input, options))
    }

    /// Converts this high-level tool into the provider-facing language-model tool shape.
    pub fn to_language_model_tool(&self) -> LanguageModelTool {
        let mut tool = LanguageModelFunctionTool::new(self.name.clone(), self.input_schema.clone());

        if let Some(description) = &self.description {
            tool = tool.with_description(description.clone());
        }

        if let Some(input_examples) = &self.input_examples {
            for input_example in input_examples {
                tool = tool.with_input_example(input_example.input.clone());
            }
        }

        if let Some(strict) = self.strict {
            tool = tool.with_strict(strict);
        }

        if let Some(provider_options) = &self.provider_options {
            tool = tool.with_provider_options(provider_options.clone());
        }

        LanguageModelTool::Function(tool)
    }
}

impl fmt::Debug for Tool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Tool")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("input_schema", &self.input_schema)
            .field("input_examples", &self.input_examples)
            .field("strict", &self.strict)
            .field("provider_options", &self.provider_options)
            .field("is_executable", &self.is_executable())
            .finish()
    }
}

/// Bidirectional mapping between caller-facing and provider-facing tool names.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolNameMapping {
    custom_tool_name_to_provider_tool_name: BTreeMap<String, String>,
    provider_tool_name_to_custom_tool_name: BTreeMap<String, String>,
}

impl ToolNameMapping {
    /// Maps a caller-facing tool name to the provider-facing name.
    ///
    /// Names without a mapping are returned unchanged.
    pub fn to_provider_tool_name(&self, custom_tool_name: &str) -> String {
        self.custom_tool_name_to_provider_tool_name
            .get(custom_tool_name)
            .cloned()
            .unwrap_or_else(|| custom_tool_name.to_string())
    }

    /// Maps a provider-facing tool name to the caller-facing name.
    ///
    /// Names without a mapping are returned unchanged.
    pub fn to_custom_tool_name(&self, provider_tool_name: &str) -> String {
        self.provider_tool_name_to_custom_tool_name
            .get(provider_tool_name)
            .cloned()
            .unwrap_or_else(|| provider_tool_name.to_string())
    }
}

/// Creates provider-defined tool name mappings from model tools.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `createToolNameMapping`:
/// only provider-defined tools whose ids are present in `provider_tool_names`
/// produce mappings; function tools and unknown provider tool ids pass through
/// unchanged.
pub fn create_tool_name_mapping<'a>(
    tools: impl IntoIterator<Item = &'a LanguageModelTool>,
    provider_tool_names: &BTreeMap<String, String>,
) -> ToolNameMapping {
    let mut mapping = ToolNameMapping::default();

    for tool in tools {
        let LanguageModelTool::Provider(tool) = tool else {
            continue;
        };

        if let Some(provider_tool_name) = provider_tool_names.get(&tool.id) {
            mapping
                .custom_tool_name_to_provider_tool_name
                .insert(tool.name.clone(), provider_tool_name.clone());
            mapping
                .provider_tool_name_to_custom_tool_name
                .insert(provider_tool_name.clone(), tool.name.clone());
        }
    }

    mapping
}

/// Converts high-level Rust tools into provider-facing language-model tools.
pub fn prepare_tools<'a>(
    tools: impl IntoIterator<Item = &'a Tool>,
) -> Option<Vec<LanguageModelTool>> {
    let tools = tools
        .into_iter()
        .map(Tool::to_language_model_tool)
        .collect::<Vec<_>>();

    if tools.is_empty() { None } else { Some(tools) }
}

/// Options for injecting JSON response instructions into a standardized prompt.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InjectJsonInstructionIntoMessagesOptions {
    /// Standardized prompt messages to update.
    pub messages: LanguageModelPrompt,

    /// JSON schema to include in the system instruction.
    pub schema: Option<JsonSchema>,

    /// Custom prefix to place before the serialized JSON schema.
    pub schema_prefix: Option<String>,

    /// Custom suffix to place after the serialized JSON schema or generic JSON instruction.
    pub schema_suffix: Option<String>,
}

impl InjectJsonInstructionIntoMessagesOptions {
    /// Creates JSON instruction injection options for a standardized prompt.
    pub fn new(messages: LanguageModelPrompt) -> Self {
        Self {
            messages,
            schema: None,
            schema_prefix: None,
            schema_suffix: None,
        }
    }

    /// Sets the JSON schema included in the system instruction.
    pub fn with_schema(mut self, schema: JsonSchema) -> Self {
        self.schema = Some(schema);
        self
    }

    /// Sets the prefix placed before the serialized JSON schema.
    pub fn with_schema_prefix(mut self, schema_prefix: impl Into<String>) -> Self {
        self.schema_prefix = Some(schema_prefix.into());
        self
    }

    /// Sets the suffix placed after the schema or generic JSON instruction.
    pub fn with_schema_suffix(mut self, schema_suffix: impl Into<String>) -> Self {
        self.schema_suffix = Some(schema_suffix.into());
        self
    }
}

/// Injects JSON response instructions into the leading system prompt message.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `injectJsonInstructionIntoMessages`: the first system message is updated
/// when present, otherwise a new system message is inserted before the original
/// prompt, and all non-system messages are preserved in order.
pub fn inject_json_instruction_into_messages(
    options: InjectJsonInstructionIntoMessagesOptions,
) -> LanguageModelPrompt {
    let InjectJsonInstructionIntoMessagesOptions {
        messages,
        schema,
        schema_prefix,
        schema_suffix,
    } = options;

    let mut messages = messages.into_iter();
    let first_message = messages.next();
    let mut remaining_messages = Vec::new();

    let mut system_message = match first_message {
        Some(LanguageModelMessage::System(system_message)) => system_message,
        Some(message) => {
            remaining_messages.push(message);
            LanguageModelSystemMessage::new("")
        }
        None => LanguageModelSystemMessage::new(""),
    };

    remaining_messages.extend(messages);
    system_message.content = inject_json_instruction(
        Some(&system_message.content),
        schema.as_ref(),
        schema_prefix.as_deref(),
        schema_suffix.as_deref(),
    );

    let mut updated_messages = Vec::with_capacity(remaining_messages.len() + 1);
    updated_messages.push(LanguageModelMessage::System(system_message));
    updated_messages.extend(remaining_messages);
    updated_messages
}

fn inject_json_instruction(
    prompt: Option<&str>,
    schema: Option<&JsonSchema>,
    schema_prefix: Option<&str>,
    schema_suffix: Option<&str>,
) -> String {
    let mut lines = Vec::new();

    if let Some(prompt) = prompt.filter(|prompt| !prompt.is_empty()) {
        lines.push(prompt.to_string());
        lines.push(String::new());
    }

    let schema_prefix = schema_prefix.or(schema.map(|_| DEFAULT_JSON_SCHEMA_INSTRUCTION_PREFIX));
    if let Some(schema_prefix) = schema_prefix {
        lines.push(schema_prefix.to_string());
    }

    if let Some(schema) = schema {
        lines.push(serde_json::to_string(schema).expect("JSON schemas serialize"));
    }

    let schema_suffix = schema_suffix.or_else(|| {
        Some(if schema.is_some() {
            DEFAULT_JSON_SCHEMA_INSTRUCTION_SUFFIX
        } else {
            DEFAULT_JSON_INSTRUCTION_SUFFIX
        })
    });
    if let Some(schema_suffix) = schema_suffix {
        lines.push(schema_suffix.to_string());
    }

    lines.join("\n")
}

/// Adds `additionalProperties: false` to object JSON schemas recursively.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `addAdditionalPropertiesToJsonSchema`: object schemas, including union
/// schemas whose `type` includes `"object"`, are made closed recursively across
/// properties, items, composition lists, and definitions.
pub fn add_additional_properties_to_json_schema(mut json_schema: JsonSchema) -> JsonSchema {
    add_additional_properties_to_json_schema_object(&mut json_schema);
    json_schema
}

fn add_additional_properties_to_json_schema_object(json_schema: &mut JsonSchema) {
    if is_object_json_schema(json_schema) {
        json_schema.insert("additionalProperties".to_string(), JsonValue::Bool(false));

        if let Some(JsonValue::Object(properties)) = json_schema.get_mut("properties") {
            for property in properties.values_mut() {
                visit_json_schema_definition(property);
            }
        }
    }

    if let Some(items) = json_schema.get_mut("items") {
        visit_json_schema_definition_or_array(items);
    }

    for key in ["anyOf", "allOf", "oneOf"] {
        if let Some(JsonValue::Array(definitions)) = json_schema.get_mut(key) {
            for definition in definitions {
                visit_json_schema_definition(definition);
            }
        }
    }

    if let Some(JsonValue::Object(definitions)) = json_schema.get_mut("definitions") {
        for definition in definitions.values_mut() {
            visit_json_schema_definition(definition);
        }
    }
}

fn visit_json_schema_definition_or_array(definition: &mut JsonValue) {
    match definition {
        JsonValue::Array(definitions) => {
            for definition in definitions {
                visit_json_schema_definition(definition);
            }
        }
        _ => visit_json_schema_definition(definition),
    }
}

fn visit_json_schema_definition(definition: &mut JsonValue) {
    if let JsonValue::Object(json_schema) = definition {
        add_additional_properties_to_json_schema_object(json_schema);
    }
}

fn is_object_json_schema(json_schema: &JsonSchema) -> bool {
    match json_schema.get("type") {
        Some(JsonValue::String(schema_type)) => schema_type == "object",
        Some(JsonValue::Array(schema_types)) => schema_types
            .iter()
            .any(|schema_type| schema_type.as_str() == Some("object")),
        _ => false,
    }
}

/// Top-level reasoning effort levels that can be mapped to provider-specific settings.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningLevel {
    /// Use minimal reasoning effort.
    Minimal,
    /// Use low reasoning effort.
    Low,
    /// Use medium reasoning effort.
    Medium,
    /// Use high reasoning effort.
    High,
    /// Use extra-high reasoning effort.
    Xhigh,
}

impl ReasoningLevel {
    /// Returns the upstream provider-v4 string for this reasoning level.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
        }
    }
}

impl TryFrom<LanguageModelReasoningEffort> for ReasoningLevel {
    type Error = LanguageModelReasoningEffort;

    fn try_from(value: LanguageModelReasoningEffort) -> Result<Self, Self::Error> {
        match value {
            LanguageModelReasoningEffort::Minimal => Ok(Self::Minimal),
            LanguageModelReasoningEffort::Low => Ok(Self::Low),
            LanguageModelReasoningEffort::Medium => Ok(Self::Medium),
            LanguageModelReasoningEffort::High => Ok(Self::High),
            LanguageModelReasoningEffort::Xhigh => Ok(Self::Xhigh),
            value => Err(value),
        }
    }
}

/// Returns whether a reasoning request should override the provider default.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `isCustomReasoning`: missing
/// reasoning and `provider-default` are not custom, while `none` and all effort
/// levels are custom reasoning settings.
pub fn is_custom_reasoning(reasoning: Option<&LanguageModelReasoningEffort>) -> bool {
    !matches!(
        reasoning,
        None | Some(LanguageModelReasoningEffort::ProviderDefault)
    )
}

/// Maps a top-level reasoning effort level to a provider-specific effort value.
///
/// This mirrors upstream `mapReasoningToProviderEffort`: unsupported levels add
/// an unsupported warning, and renamed levels add a compatibility warning.
pub fn map_reasoning_to_provider_effort<T>(
    reasoning: ReasoningLevel,
    effort_map: &BTreeMap<ReasoningLevel, T>,
    warnings: &mut Vec<Warning>,
) -> Option<T>
where
    T: AsRef<str> + Clone,
{
    let Some(mapped) = effort_map.get(&reasoning) else {
        warnings.push(Warning::Unsupported {
            feature: "reasoning".to_string(),
            details: Some(format!(
                "reasoning \"{}\" is not supported by this model.",
                reasoning.as_str()
            )),
        });
        return None;
    };

    if mapped.as_ref() != reasoning.as_str() {
        warnings.push(Warning::Compatibility {
            feature: "reasoning".to_string(),
            details: Some(format!(
                "reasoning \"{}\" is not directly supported by this model. mapped to effort \"{}\".",
                reasoning.as_str(),
                mapped.as_ref()
            )),
        });
    }

    Some(mapped.clone())
}

/// Maps a top-level reasoning effort level to a provider-specific token budget.
///
/// The budget is the rounded product of max output tokens and the configured
/// percentage, clamped between the minimum and maximum reasoning budgets.
pub fn map_reasoning_to_provider_budget(
    reasoning: ReasoningLevel,
    max_output_tokens: u64,
    max_reasoning_budget: u64,
    min_reasoning_budget: Option<u64>,
    budget_percentages: Option<&BTreeMap<ReasoningLevel, f64>>,
    warnings: &mut Vec<Warning>,
) -> Option<u64> {
    let percentage = match budget_percentages {
        Some(percentages) => percentages.get(&reasoning).copied(),
        None => Some(default_reasoning_budget_percentage(reasoning)),
    };

    let Some(percentage) = percentage else {
        warnings.push(Warning::Unsupported {
            feature: "reasoning".to_string(),
            details: Some(format!(
                "reasoning \"{}\" is not supported by this model.",
                reasoning.as_str()
            )),
        });
        return None;
    };

    let requested_budget = ((max_output_tokens as f64) * percentage).round() as u64;

    Some(
        requested_budget
            .max(min_reasoning_budget.unwrap_or(1024))
            .min(max_reasoning_budget),
    )
}

fn default_reasoning_budget_percentage(reasoning: ReasoningLevel) -> f64 {
    match reasoning {
        ReasoningLevel::Minimal => 0.02,
        ReasoningLevel::Low => 0.1,
        ReasoningLevel::Medium => 0.3,
        ReasoningLevel::High => 0.6,
        ReasoningLevel::Xhigh => 0.9,
    }
}

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

/// Removes entries whose values are missing.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `removeUndefinedEntries`:
/// values that are nullish in JavaScript are omitted from the returned record,
/// while present falsy-equivalent values are preserved.
pub fn remove_undefined_entries<K, T, I>(record: I) -> BTreeMap<String, T>
where
    I: IntoIterator<Item = (K, Option<T>)>,
    K: Into<String>,
{
    record
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.into(), value)))
        .collect()
}

/// Checks whether a JSON value has the provider-reference record shape.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `isProviderReference` at the
/// JSON boundary: plain objects without a `type` discriminator are treated as
/// provider references, while tagged file-data objects and non-objects are not.
pub fn is_provider_reference(data: &JsonValue) -> bool {
    data.as_object()
        .is_some_and(|object| !object.contains_key("type"))
}

/// Detects the IANA media type of raw bytes or base64-encoded file content.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `detectMediaType`: when a
/// top-level media type is supplied, only that signature table is checked;
/// otherwise image, application document, audio, and video signatures are
/// considered in upstream order.
pub fn detect_media_type(
    data: &FileDataContent,
    top_level_type: Option<&str>,
) -> Option<&'static str> {
    if let Some(top_level_type) = top_level_type {
        return match top_level_type {
            "image" => detect_media_type_by_signatures(data, IMAGE_MEDIA_TYPE_SIGNATURES),
            "audio" => detect_media_type_by_signatures(data, AUDIO_MEDIA_TYPE_SIGNATURES),
            "video" => detect_media_type_by_signatures(data, VIDEO_MEDIA_TYPE_SIGNATURES),
            "application" => detect_media_type_by_signatures(data, DOCUMENT_MEDIA_TYPE_SIGNATURES),
            _ => None,
        };
    }

    for signatures in [
        IMAGE_MEDIA_TYPE_SIGNATURES,
        DOCUMENT_MEDIA_TYPE_SIGNATURES,
        AUDIO_MEDIA_TYPE_SIGNATURES,
        VIDEO_MEDIA_TYPE_SIGNATURES,
    ] {
        if let Some(media_type) = detect_media_type_by_signatures(data, signatures) {
            return Some(media_type);
        }
    }

    None
}

fn detect_media_type_by_signatures(
    data: &FileDataContent,
    signatures: &[MediaTypeSignature],
) -> Option<&'static str> {
    let bytes = bytes_for_media_type_detection(data)?;

    signatures
        .iter()
        .find(|signature| bytes_match_signature(&bytes, signature.bytes_prefix))
        .map(|signature| signature.media_type)
}

fn bytes_match_signature(bytes: &[u8], bytes_prefix: &[Option<u8>]) -> bool {
    bytes.len() >= bytes_prefix.len()
        && bytes_prefix
            .iter()
            .enumerate()
            .all(|(index, byte)| byte.is_none_or(|byte| bytes[index] == byte))
}

fn bytes_for_media_type_detection(data: &FileDataContent) -> Option<Vec<u8>> {
    match data {
        FileDataContent::Bytes(bytes) => Some(strip_id3_tags_if_present(bytes).to_vec()),
        FileDataContent::Base64(base64) if base64.starts_with("SUQz") => {
            decode_base64(base64).map(|bytes| strip_id3_tags_if_present(&bytes).to_vec())
        }
        FileDataContent::Base64(base64) => {
            let prefix_length = base64
                .char_indices()
                .nth(24)
                .map_or(base64.len(), |(index, _)| index);
            decode_base64(&base64[..prefix_length])
        }
    }
}

fn strip_id3_tags_if_present(bytes: &[u8]) -> &[u8] {
    if bytes.len() <= 10 || !bytes.starts_with(&[0x49, 0x44, 0x33]) {
        return bytes;
    }

    let id3_size = ((usize::from(bytes[6] & 0x7f)) << 21)
        | ((usize::from(bytes[7] & 0x7f)) << 14)
        | ((usize::from(bytes[8] & 0x7f)) << 7)
        | usize::from(bytes[9] & 0x7f);

    bytes.get(id3_size + 10..).unwrap_or_default()
}

fn decode_base64(base64: &str) -> Option<Vec<u8>> {
    let mut sextets = Vec::new();

    for byte in base64.bytes() {
        match byte {
            b'=' => break,
            b'\t' | b'\n' | b'\r' | b' ' => continue,
            _ => sextets.push(base64_value(byte)?),
        }
    }

    if sextets.len() % 4 == 1 {
        return None;
    }

    let mut bytes = Vec::with_capacity((sextets.len() * 3) / 4);
    let mut chunks = sextets.chunks_exact(4);

    for chunk in &mut chunks {
        let buffer = (u32::from(chunk[0]) << 18)
            | (u32::from(chunk[1]) << 12)
            | (u32::from(chunk[2]) << 6)
            | u32::from(chunk[3]);
        bytes.push((buffer >> 16) as u8);
        bytes.push((buffer >> 8) as u8);
        bytes.push(buffer as u8);
    }

    match chunks.remainder() {
        [] => {}
        [first, second] => {
            bytes.push((*first << 2) | (*second >> 4));
        }
        [first, second, third] => {
            bytes.push((*first << 2) | (*second >> 4));
            bytes.push(((*second & 0x0f) << 4) | (*third >> 2));
        }
        _ => return None,
    }

    Some(bytes)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

/// Returns the top-level segment of a media type.
pub fn get_top_level_media_type(media_type: &str) -> &str {
    media_type
        .find('/')
        .map_or(media_type, |slash_index| &media_type[..slash_index])
}

/// Returns whether a media type has a non-empty, non-wildcard subtype.
pub fn is_full_media_type(media_type: &str) -> bool {
    media_type
        .split_once('/')
        .is_some_and(|(_, subtype)| !subtype.is_empty() && subtype != "*")
}

/// Resolves a prompt file part media type to a full `type/subtype` value.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `resolveFullMediaType`:
/// full media types are returned unchanged, top-level or wildcard media types
/// are detected from inline byte data when possible, and other unresolved cases
/// report an [`UnsupportedFunctionalityError`].
pub fn resolve_full_media_type(
    part: &LanguageModelFilePart,
) -> Result<String, UnsupportedFunctionalityError> {
    if is_full_media_type(&part.media_type) {
        return Ok(part.media_type.clone());
    }

    let FileData::Data { data } = &part.data else {
        return Err(UnsupportedFunctionalityError::new(format!(
            "file of media type \"{}\" must specify subtype since it is not passed as inline bytes",
            part.media_type
        )));
    };

    detect_media_type(data, Some(get_top_level_media_type(&part.media_type)))
        .map(str::to_string)
        .ok_or_else(|| {
            UnsupportedFunctionalityError::new(format!(
                "file of media type \"{}\" must specify subtype since it could not be auto-detected",
                part.media_type
            ))
        })
}

/// Combines optional HTTP header maps, with later maps overriding earlier ones.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `combineHeaders`: missing
/// maps are ignored, header names are preserved as supplied, and missing values
/// are retained so a later `None` can intentionally override an earlier value.
pub fn combine_headers<K, V, I, H>(headers: H) -> BTreeMap<String, Option<String>>
where
    H: IntoIterator<Item = Option<I>>,
    I: IntoIterator<Item = (K, Option<V>)>,
    K: Into<String>,
    V: Into<String>,
{
    let mut combined_headers = BTreeMap::new();

    for current_headers in headers.into_iter().flatten() {
        for (key, value) in current_headers {
            combined_headers.insert(key.into(), value.map(Into::into));
        }
    }

    combined_headers
}

/// Normalizes optional HTTP header entries into a lower-case header map.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `normalizeHeaders`: missing
/// input becomes an empty map, nullish values are removed, and header names are
/// normalized to lower case.
pub fn normalize_headers<K, V, I>(headers: Option<I>) -> Headers
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
{
    let Some(headers) = headers else {
        return Headers::new();
    };

    headers
        .into_iter()
        .filter_map(|(key, value)| {
            value.map(|value| (key.as_ref().to_ascii_lowercase(), value.into()))
        })
        .collect()
}

/// Appends suffix parts to the normalized `user-agent` header.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `withUserAgentSuffix`: input
/// headers are normalized first, missing header values are removed, and empty
/// user-agent parts are skipped before joining with spaces.
pub fn with_user_agent_suffix<K, V, I, S, P>(
    headers: Option<I>,
    user_agent_suffix_parts: P,
) -> Headers
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
    P: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut headers = normalize_headers(headers);
    let current_user_agent = headers.get("user-agent").map(String::as_str).unwrap_or("");

    let mut user_agent_parts = Vec::new();

    if !current_user_agent.is_empty() {
        user_agent_parts.push(current_user_agent.to_string());
    }

    for part in user_agent_suffix_parts {
        let part = part.as_ref();
        if !part.is_empty() {
            user_agent_parts.push(part.to_string());
        }
    }

    let user_agent = user_agent_parts.join(" ");

    headers.insert("user-agent".to_string(), user_agent);
    headers
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
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    use crate::language_model::{
        LanguageModelFilePart, LanguageModelFunctionTool, LanguageModelMessage,
        LanguageModelProviderTool, LanguageModelReasoningEffort, LanguageModelSystemMessage,
        LanguageModelTextPart, LanguageModelTool, LanguageModelUserContentPart,
        LanguageModelUserMessage,
    };
    use crate::{FileData, FileDataContent, JsonObject, JsonValue, ProviderReference, Warning};
    use serde_json::json;
    use url::Url;

    use super::{
        Arrayable, InjectJsonInstructionIntoMessagesOptions, LoadApiKeyOptions,
        LoadOptionalSettingOptions, LoadSettingOptions, ReasoningLevel, Tool, ToolExecutionError,
        ToolExecutionOptions, add_additional_properties_to_json_schema, as_array, combine_headers,
        create_tool_name_mapping, detect_media_type, filter_nullable, get_top_level_media_type,
        inject_json_instruction, inject_json_instruction_into_messages, is_custom_reasoning,
        is_full_media_type, is_non_nullable, is_provider_reference, load_api_key,
        load_api_key_with_env, load_optional_setting_with_env, load_setting, load_setting_with_env,
        map_reasoning_to_provider_budget, map_reasoning_to_provider_effort,
        media_type_to_extension, normalize_headers, prepare_tools, remove_undefined_entries,
        resolve_full_media_type, resolve_provider_reference, strip_file_extension,
        with_user_agent_suffix, without_trailing_slash,
    };

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures should be ready"),
        }
    }

    fn object_schema() -> crate::JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone()
    }

    fn object_schema_json() -> String {
        serde_json::to_string(&object_schema()).expect("schema serializes")
    }

    fn expected_schema_instruction(prompt: &str) -> String {
        format!(
            "{prompt}\n\nJSON schema:\n{}\nYou MUST answer with a JSON object that matches the JSON schema above.",
            object_schema_json()
        )
    }

    #[test]
    fn inject_json_instruction_adds_schema_to_prompt() {
        assert_eq!(
            inject_json_instruction(Some("Generate weather"), Some(&object_schema()), None, None),
            expected_schema_instruction("Generate weather")
        );
    }

    #[test]
    fn inject_json_instruction_uses_generic_json_suffix_without_schema() {
        assert_eq!(
            inject_json_instruction(Some("Generate data"), None, None, None),
            "Generate data\n\nYou MUST answer with JSON."
        );
    }

    #[test]
    fn inject_json_instruction_omits_empty_prompt() {
        assert_eq!(
            inject_json_instruction(Some(""), Some(&object_schema()), None, None),
            format!(
                "JSON schema:\n{}\nYou MUST answer with a JSON object that matches the JSON schema above.",
                object_schema_json()
            )
        );
    }

    #[test]
    fn inject_json_instruction_uses_custom_schema_lines() {
        assert_eq!(
            inject_json_instruction(
                Some("Generate weather"),
                Some(&object_schema()),
                Some("Custom schema:"),
                Some("Follow this exactly."),
            ),
            format!(
                "Generate weather\n\nCustom schema:\n{}\nFollow this exactly.",
                object_schema_json()
            )
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_updates_existing_system_message() {
        let messages = vec![
            LanguageModelMessage::System(LanguageModelSystemMessage::new("Generate weather")),
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Use Brisbane")),
            ])),
        ];

        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(messages.clone())
                    .with_schema(object_schema())
            ),
            vec![
                LanguageModelMessage::System(LanguageModelSystemMessage::new(
                    expected_schema_instruction("Generate weather")
                )),
                messages[1].clone(),
            ]
        );
        assert_eq!(
            messages[0],
            LanguageModelMessage::System(LanguageModelSystemMessage::new("Generate weather"))
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_inserts_system_message() {
        let user_message = LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Generate weather")),
        ]));

        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(vec![user_message.clone()])
                    .with_schema(object_schema())
            ),
            vec![
                LanguageModelMessage::System(LanguageModelSystemMessage::new(format!(
                    "JSON schema:\n{}\nYou MUST answer with a JSON object that matches the JSON schema above.",
                    object_schema_json()
                ))),
                user_message,
            ]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_preserves_system_provider_options() {
        let provider_options = BTreeMap::from([(
            "test-provider".to_string(),
            json!({ "trace": "abc" })
                .as_object()
                .expect("provider options are an object")
                .clone(),
        )]);

        assert_eq!(
            inject_json_instruction_into_messages(InjectJsonInstructionIntoMessagesOptions::new(
                vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("Generate data")
                        .with_provider_options(provider_options.clone()),
                )]
            )),
            vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new("Generate data\n\nYou MUST answer with JSON.")
                    .with_provider_options(provider_options),
            )]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_uses_custom_schema_lines() {
        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("Generate weather"),
                )])
                .with_schema(object_schema())
                .with_schema_prefix("Custom schema:")
                .with_schema_suffix("Follow this exactly.")
            ),
            vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new(format!(
                    "Generate weather\n\nCustom schema:\n{}\nFollow this exactly.",
                    object_schema_json()
                ))
            )]
        );
    }

    #[test]
    fn reasoning_level_serializes_upstream_strings() {
        assert_eq!(
            serde_json::to_value(ReasoningLevel::Xhigh).expect("reasoning level serializes"),
            json!("xhigh")
        );
        assert_eq!(
            serde_json::from_value::<ReasoningLevel>(json!("minimal"))
                .expect("reasoning level deserializes"),
            ReasoningLevel::Minimal
        );
    }

    #[test]
    fn reasoning_level_converts_from_custom_reasoning_efforts() {
        assert_eq!(
            ReasoningLevel::try_from(LanguageModelReasoningEffort::High),
            Ok(ReasoningLevel::High)
        );
        assert_eq!(
            ReasoningLevel::try_from(LanguageModelReasoningEffort::ProviderDefault),
            Err(LanguageModelReasoningEffort::ProviderDefault)
        );
        assert_eq!(
            ReasoningLevel::try_from(LanguageModelReasoningEffort::None),
            Err(LanguageModelReasoningEffort::None)
        );
    }

    #[test]
    fn is_custom_reasoning_matches_upstream_default_handling() {
        assert!(!is_custom_reasoning(None));
        assert!(!is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::ProviderDefault
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::None
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::Minimal
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::Low
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::Medium
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::High
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::Xhigh
        )));
    }

    #[test]
    fn map_reasoning_to_provider_effort_returns_direct_match_without_warning() {
        let effort_map = BTreeMap::from([
            (ReasoningLevel::Minimal, "low".to_string()),
            (ReasoningLevel::Low, "low".to_string()),
            (ReasoningLevel::Medium, "medium".to_string()),
            (ReasoningLevel::High, "high".to_string()),
            (ReasoningLevel::Xhigh, "max".to_string()),
        ]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_effort(ReasoningLevel::Medium, &effort_map, &mut warnings),
            Some("medium".to_string())
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_effort_warns_for_renamed_match() {
        let effort_map = BTreeMap::from([
            (ReasoningLevel::Minimal, "low".to_string()),
            (ReasoningLevel::Xhigh, "max".to_string()),
        ]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_effort(ReasoningLevel::Minimal, &effort_map, &mut warnings),
            Some("low".to_string())
        );
        assert_eq!(
            warnings,
            vec![Warning::Compatibility {
                feature: "reasoning".to_string(),
                details: Some(
                    "reasoning \"minimal\" is not directly supported by this model. mapped to effort \"low\"."
                        .to_string()
                ),
            }]
        );

        warnings.clear();
        assert_eq!(
            map_reasoning_to_provider_effort(ReasoningLevel::Xhigh, &effort_map, &mut warnings),
            Some("max".to_string())
        );
        assert_eq!(
            warnings,
            vec![Warning::Compatibility {
                feature: "reasoning".to_string(),
                details: Some(
                    "reasoning \"xhigh\" is not directly supported by this model. mapped to effort \"max\"."
                        .to_string()
                ),
            }]
        );
    }

    #[test]
    fn map_reasoning_to_provider_effort_warns_for_missing_level() {
        let effort_map = BTreeMap::from([(ReasoningLevel::Medium, "medium".to_string())]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_effort(ReasoningLevel::High, &effort_map, &mut warnings),
            None
        );
        assert_eq!(
            warnings,
            vec![Warning::Unsupported {
                feature: "reasoning".to_string(),
                details: Some("reasoning \"high\" is not supported by this model.".to_string()),
            }]
        );
    }

    #[test]
    fn map_reasoning_to_provider_budget_uses_default_percentages_and_clamps() {
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Medium,
                64_000,
                64_000,
                None,
                None,
                &mut warnings,
            ),
            Some(19_200)
        );
        assert_eq!(warnings, Vec::new());

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Xhigh,
                64_000,
                50_000,
                None,
                None,
                &mut warnings,
            ),
            Some(50_000)
        );
        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Minimal,
                10_000,
                10_000,
                None,
                None,
                &mut warnings,
            ),
            Some(1024)
        );
        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Minimal,
                10_000,
                10_000,
                Some(512),
                None,
                &mut warnings,
            ),
            Some(512)
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_budget_uses_custom_percentages() {
        let budget_percentages = BTreeMap::from([(ReasoningLevel::Medium, 0.5)]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Medium,
                10_000,
                10_000,
                None,
                Some(&budget_percentages),
                &mut warnings,
            ),
            Some(5000)
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_budget_warns_for_missing_custom_percentage() {
        let budget_percentages = BTreeMap::from([(ReasoningLevel::Medium, 0.5)]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::High,
                64_000,
                64_000,
                None,
                Some(&budget_percentages),
                &mut warnings,
            ),
            None
        );
        assert_eq!(
            warnings,
            vec![Warning::Unsupported {
                feature: "reasoning".to_string(),
                details: Some("reasoning \"high\" is not supported by this model.".to_string()),
            }]
        );
    }

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
    fn add_additional_properties_to_json_schema_closes_nested_objects() {
        let schema = json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    }
                },
                "age": { "type": "number" }
            }
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "user": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "name": { "type": "string" }
                        }
                    },
                    "age": { "type": "number" }
                }
            })
            .as_object()
            .expect("schema is an object")
            .clone()
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_closes_objects_in_arrays_and_unions() {
        let schema = json!({
            "type": "object",
            "properties": {
                "ingredients": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" }
                        }
                    }
                },
                "response": {
                    "type": ["object", "null"],
                    "properties": {
                        "ok": { "type": "boolean" }
                    }
                }
            }
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "ingredients": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": {
                                "name": { "type": "string" }
                            }
                        }
                    },
                    "response": {
                        "type": ["object", "null"],
                        "additionalProperties": false,
                        "properties": {
                            "ok": { "type": "boolean" }
                        }
                    }
                }
            })
            .as_object()
            .expect("schema is an object")
            .clone()
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_visits_compositions_and_definitions() {
        let schema = json!({
            "type": "object",
            "properties": {
                "response": {
                    "anyOf": [
                        { "type": "object", "properties": { "name": { "type": "string" } } },
                        { "type": "string" }
                    ],
                    "allOf": [
                        { "type": "object", "properties": { "age": { "type": "number" } } }
                    ],
                    "oneOf": [
                        { "type": "object", "properties": { "success": { "type": "boolean" } } }
                    ]
                },
                "node": { "$ref": "#/definitions/Node" }
            },
            "definitions": {
                "Node": {
                    "type": "object",
                    "additionalProperties": true,
                    "properties": {
                        "value": { "type": "string" }
                    }
                }
            }
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "response": {
                        "anyOf": [
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": { "name": { "type": "string" } }
                            },
                            { "type": "string" }
                        ],
                        "allOf": [
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": { "age": { "type": "number" } }
                            }
                        ],
                        "oneOf": [
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": { "success": { "type": "boolean" } }
                            }
                        ]
                    },
                    "node": { "$ref": "#/definitions/Node" }
                },
                "definitions": {
                    "Node": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "value": { "type": "string" }
                        }
                    }
                }
            })
            .as_object()
            .expect("schema is an object")
            .clone()
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_leaves_non_object_schema_unchanged() {
        let schema = json!({
            "type": "string"
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            json!({
                "type": "string"
            })
            .as_object()
            .expect("schema is an object")
            .clone()
        );
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
    fn remove_undefined_entries_removes_missing_values() {
        let record = remove_undefined_entries([
            ("present", Some(json!("value"))),
            ("missing", None),
            ("alsoPresent", Some(json!({ "nested": true }))),
        ]);

        assert_eq!(
            record,
            BTreeMap::from([
                ("alsoPresent".to_string(), json!({ "nested": true })),
                ("present".to_string(), json!("value")),
            ])
        );
    }

    #[test]
    fn remove_undefined_entries_preserves_falsy_equivalent_values() {
        let record = remove_undefined_entries([
            ("zero", Some(json!(0))),
            ("false", Some(json!(false))),
            ("emptyString", Some(json!(""))),
            ("nullish", None),
        ]);

        assert_eq!(
            record,
            BTreeMap::from([
                ("emptyString".to_string(), json!("")),
                ("false".to_string(), json!(false)),
                ("zero".to_string(), json!(0)),
            ])
        );
    }

    #[test]
    fn remove_undefined_entries_handles_json_null_values_as_missing() {
        let record: BTreeMap<String, Option<serde_json::Value>> = serde_json::from_value(json!({
            "keep": "value",
            "drop": null
        }))
        .expect("record deserializes");

        assert_eq!(
            remove_undefined_entries(record),
            BTreeMap::from([("keep".to_string(), json!("value"))])
        );
    }

    #[test]
    fn is_provider_reference_accepts_plain_records() {
        assert!(is_provider_reference(&json!({
            "openai": "file-abc123"
        })));
        assert!(is_provider_reference(&json!({
            "fileId": "abc"
        })));
    }

    #[test]
    fn is_provider_reference_rejects_tagged_file_data_objects() {
        assert!(!is_provider_reference(&json!({
            "type": "reference",
            "reference": {
                "fileId": "abc"
            }
        })));
        assert!(!is_provider_reference(&json!({
            "type": "data",
            "data": "x"
        })));
    }

    #[test]
    fn is_provider_reference_rejects_non_objects_and_arrays() {
        assert!(!is_provider_reference(&JsonValue::Null));
        assert!(!is_provider_reference(&json!("some-string")));
        assert!(!is_provider_reference(&json!(42)));
        assert!(!is_provider_reference(&json!([1, 2, 3])));
    }

    #[test]
    fn get_top_level_media_type_matches_upstream_edge_cases() {
        assert_eq!(get_top_level_media_type("image/png"), "image");
        assert_eq!(get_top_level_media_type("audio/*"), "audio");
        assert_eq!(get_top_level_media_type("text"), "text");
        assert_eq!(get_top_level_media_type(""), "");
        assert_eq!(get_top_level_media_type("/"), "");
        assert_eq!(get_top_level_media_type("image/"), "image");
    }

    #[test]
    fn is_full_media_type_requires_concrete_subtype() {
        assert!(is_full_media_type("image/png"));
        assert!(is_full_media_type("application/pdf"));
        assert!(!is_full_media_type("image"));
        assert!(!is_full_media_type("image/*"));
        assert!(!is_full_media_type("image/"));
        assert!(!is_full_media_type("/"));
    }

    #[test]
    fn detect_media_type_matches_top_level_signature_tables() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47, 0xff]),
                Some("image"),
            ),
            Some("image/png")
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x25, 0x50, 0x44, 0x46, 0x00]),
                Some("application"),
            ),
            Some("application/pdf")
        );
        assert_eq!(
            detect_media_type(&FileDataContent::Bytes(vec![0xff, 0xfb]), Some("audio")),
            Some("audio/mpeg")
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x1a, 0x45, 0xdf, 0xa3]),
                Some("video"),
            ),
            Some("video/webm")
        );
    }

    #[test]
    fn detect_media_type_handles_base64_and_id3_prefixed_mp3() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Base64("iVBORw0KGgo=".to_string()),
                Some("image"),
            ),
            Some("image/png")
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![
                    0x49, 0x44, 0x33, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xfb,
                ]),
                Some("audio"),
            ),
            Some("audio/mpeg")
        );
    }

    #[test]
    fn detect_media_type_returns_none_for_unsupported_or_unmatched_data() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47, 0xff]),
                Some("text"),
            ),
            None
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x00, 0x01, 0x02]),
                Some("image"),
            ),
            None
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Base64("not valid base64!".to_string()),
                None,
            ),
            None
        );
    }

    #[test]
    fn detect_media_type_without_top_level_type_uses_upstream_order() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70]),
                None,
            ),
            Some("video/mp4")
        );
        assert_eq!(
            detect_media_type(&FileDataContent::Bytes(vec![0x1a, 0x45, 0xdf, 0xa3]), None,),
            Some("audio/webm")
        );
    }

    #[test]
    fn resolve_full_media_type_returns_full_media_type_as_is() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47]),
            },
            "image/jpeg",
        );

        assert_eq!(
            resolve_full_media_type(&part).expect("full media type resolves"),
            "image/jpeg"
        );
    }

    #[test]
    fn resolve_full_media_type_detects_inline_byte_subtype() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a]),
            },
            "image",
        );

        assert_eq!(
            resolve_full_media_type(&part).expect("inline bytes resolve"),
            "image/png"
        );
    }

    #[test]
    fn resolve_full_media_type_treats_wildcard_as_top_level() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Base64("iVBORw0KGgo=".to_string()),
            },
            "image/*",
        );

        assert_eq!(
            resolve_full_media_type(&part).expect("wildcard media type resolves"),
            "image/png"
        );
    }

    #[test]
    fn resolve_full_media_type_detects_application_pdf() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Bytes(vec![0x25, 0x50, 0x44, 0x46, 0x2d]),
            },
            "application",
        );

        assert_eq!(
            resolve_full_media_type(&part).expect("application subtype resolves"),
            "application/pdf"
        );
    }

    #[test]
    fn resolve_full_media_type_rejects_non_inline_byte_data() {
        let part = LanguageModelFilePart::new(
            FileData::Url {
                url: Url::parse("https://example.com/file.png").expect("valid URL"),
            },
            "image",
        );

        let error = resolve_full_media_type(&part)
            .expect_err("top-level URL media type requires a subtype");

        assert_eq!(
            error.functionality(),
            "file of media type \"image\" must specify subtype since it is not passed as inline bytes"
        );
    }

    #[test]
    fn resolve_full_media_type_rejects_unrecognized_inline_bytes() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Bytes(vec![0x00, 0x01, 0x02]),
            },
            "image",
        );

        let error = resolve_full_media_type(&part)
            .expect_err("unrecognized inline bytes require a subtype");

        assert_eq!(
            error.functionality(),
            "file of media type \"image\" must specify subtype since it could not be auto-detected"
        );
    }

    #[test]
    fn combine_headers_returns_empty_map_for_missing_groups() {
        assert_eq!(
            combine_headers::<String, String, Vec<(String, Option<String>)>, _>([None, None,]),
            BTreeMap::new()
        );
    }

    #[test]
    fn combine_headers_preserves_keys_and_combines_groups() {
        let headers = combine_headers([
            Some(vec![
                ("Authorization", Some("Bearer token")),
                ("X-Feature", Some("alpha")),
            ]),
            None,
            Some(vec![("X-Feature", Some("beta")), ("X-Disabled", None)]),
        ]);

        assert_eq!(
            headers,
            BTreeMap::from([
                (
                    "Authorization".to_string(),
                    Some("Bearer token".to_string())
                ),
                ("X-Disabled".to_string(), None),
                ("X-Feature".to_string(), Some("beta".to_string())),
            ])
        );
    }

    #[test]
    fn combine_headers_allows_missing_values_to_override_present_values() {
        let headers = combine_headers([
            Some(vec![("x-enabled", Some("true")), ("x-empty", Some(""))]),
            Some(vec![("x-enabled", None)]),
        ]);

        assert_eq!(
            headers,
            BTreeMap::from([
                ("x-empty".to_string(), Some("".to_string())),
                ("x-enabled".to_string(), None),
            ])
        );
    }

    #[test]
    fn normalize_headers_returns_empty_map_for_missing_input() {
        assert_eq!(
            normalize_headers::<String, String, Vec<(String, Option<String>)>>(None),
            BTreeMap::new()
        );
    }

    #[test]
    fn normalize_headers_lowercases_keys_and_filters_missing_values() {
        let headers = normalize_headers(Some(vec![
            ("Authorization", Some("Bearer token")),
            ("X-Feature", Some("beta")),
            ("X-Ignore", None),
        ]));

        assert_eq!(
            headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer token".to_string()),
                ("x-feature".to_string(), "beta".to_string()),
            ])
        );
    }

    #[test]
    fn normalize_headers_preserves_empty_strings_and_allows_later_overrides() {
        let headers = normalize_headers(Some(vec![
            ("CONTENT-TYPE", Some("text/plain")),
            ("content-type", Some("application/json")),
            ("x-empty", Some("")),
        ]));

        assert_eq!(
            headers,
            BTreeMap::from([
                ("content-type".to_string(), "application/json".to_string()),
                ("x-empty".to_string(), "".to_string()),
            ])
        );
    }

    #[test]
    fn with_user_agent_suffix_creates_user_agent_header() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("Content-Type", Some("application/json")),
                ("Authorization", Some("Bearer token")),
            ]),
            ["ai-sdk/0.0.0-test", "provider/test-openai"],
        );

        assert_eq!(
            headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer token".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
                (
                    "user-agent".to_string(),
                    "ai-sdk/0.0.0-test provider/test-openai".to_string(),
                ),
            ])
        );
    }

    #[test]
    fn with_user_agent_suffix_appends_to_existing_header_and_filters_empty_parts() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("User-Agent", Some("TestApp/0.0.0-test")),
                ("Accept", Some("application/json")),
            ]),
            ["", "ai-sdk/0.0.0-test", "provider/test-anthropic"],
        );

        assert_eq!(
            headers,
            BTreeMap::from([
                ("accept".to_string(), "application/json".to_string()),
                (
                    "user-agent".to_string(),
                    "TestApp/0.0.0-test ai-sdk/0.0.0-test provider/test-anthropic".to_string(),
                ),
            ])
        );
    }

    #[test]
    fn with_user_agent_suffix_removes_missing_headers_before_appending() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("Content-Type", Some("application/json")),
                ("Authorization", None),
                ("User-Agent", Some("TestApp/0.0.0-test")),
                ("Accept", Some("application/json")),
                ("Cache-Control", None),
            ]),
            ["ai-sdk/0.0.0-test"],
        );

        assert_eq!(
            headers,
            BTreeMap::from([
                ("accept".to_string(), "application/json".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
                (
                    "user-agent".to_string(),
                    "TestApp/0.0.0-test ai-sdk/0.0.0-test".to_string(),
                ),
            ])
        );
    }

    #[test]
    fn with_user_agent_suffix_sets_empty_user_agent_when_no_parts_exist() {
        assert_eq!(
            with_user_agent_suffix::<String, String, Vec<(String, Option<String>)>, String, _>(
                None,
                Vec::new(),
            ),
            BTreeMap::from([("user-agent".to_string(), String::new())])
        );
    }

    #[test]
    fn create_tool_name_mapping_maps_provider_defined_tools() {
        let tools = vec![
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "anthropic.computer-use",
                "custom-computer-tool",
                JsonObject::new(),
            )),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "openai.code-interpreter",
                "custom-code-tool",
                JsonObject::new(),
            )),
        ];
        let provider_tool_names = BTreeMap::from([
            (
                "anthropic.computer-use".to_string(),
                "computer_use".to_string(),
            ),
            (
                "openai.code-interpreter".to_string(),
                "code_interpreter".to_string(),
            ),
        ]);

        let mapping = create_tool_name_mapping(&tools, &provider_tool_names);

        assert_eq!(
            mapping.to_provider_tool_name("custom-computer-tool"),
            "computer_use"
        );
        assert_eq!(
            mapping.to_provider_tool_name("custom-code-tool"),
            "code_interpreter"
        );
        assert_eq!(
            mapping.to_custom_tool_name("computer_use"),
            "custom-computer-tool"
        );
        assert_eq!(
            mapping.to_custom_tool_name("code_interpreter"),
            "custom-code-tool"
        );
    }

    #[test]
    fn create_tool_name_mapping_ignores_function_tools() {
        let tools = vec![LanguageModelTool::Function(LanguageModelFunctionTool::new(
            "weather",
            object_schema(),
        ))];
        let mapping = create_tool_name_mapping(&tools, &BTreeMap::new());

        assert_eq!(mapping.to_provider_tool_name("weather"), "weather");
        assert_eq!(mapping.to_custom_tool_name("weather"), "weather");
    }

    #[test]
    fn create_tool_name_mapping_passes_through_unknown_provider_tool_ids() {
        let tools = vec![LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "unknown.tool",
            "custom-tool",
            JsonObject::new(),
        ))];
        let mapping = create_tool_name_mapping(&tools, &BTreeMap::new());

        assert_eq!(mapping.to_provider_tool_name("custom-tool"), "custom-tool");
        assert_eq!(mapping.to_custom_tool_name("unknown-name"), "unknown-name");
    }

    #[test]
    fn create_tool_name_mapping_handles_mixed_and_empty_tool_sets() {
        let provider_tool_names = BTreeMap::from([(
            "anthropic.computer-use".to_string(),
            "computer_use".to_string(),
        )]);
        let mixed_tools = vec![
            LanguageModelTool::Function(LanguageModelFunctionTool::new(
                "function-tool",
                object_schema(),
            )),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "anthropic.computer-use",
                "provider-tool",
                JsonObject::new(),
            )),
        ];

        let empty_mapping =
            create_tool_name_mapping(Vec::<LanguageModelTool>::new().iter(), &BTreeMap::new());
        assert_eq!(empty_mapping.to_provider_tool_name("any-tool"), "any-tool");
        assert_eq!(empty_mapping.to_custom_tool_name("any-tool"), "any-tool");

        let mapping = create_tool_name_mapping(&mixed_tools, &provider_tool_names);
        assert_eq!(
            mapping.to_provider_tool_name("function-tool"),
            "function-tool"
        );
        assert_eq!(
            mapping.to_provider_tool_name("provider-tool"),
            "computer_use"
        );
        assert_eq!(mapping.to_custom_tool_name("computer_use"), "provider-tool");
    }

    #[test]
    fn tool_prepares_upstream_function_tool_shape() {
        let tool = Tool::new("weather", object_schema())
            .with_description("Look up weather.")
            .with_input_example(
                json!({
                    "city": "Brisbane"
                })
                .as_object()
                .expect("input example is an object")
                .clone(),
            )
            .with_strict(true);

        assert_eq!(
            tool.to_language_model_tool(),
            LanguageModelTool::Function(
                LanguageModelFunctionTool::new("weather", object_schema())
                    .with_description("Look up weather.")
                    .with_input_example(
                        json!({ "city": "Brisbane" })
                            .as_object()
                            .expect("input example is an object")
                            .clone()
                    )
                    .with_strict(true)
            )
        );
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "description": "Look up weather.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                },
                "inputExamples": [
                    {
                        "input": {
                            "city": "Brisbane"
                        }
                    }
                ],
                "strict": true
            })
        );
    }

    #[test]
    fn prepare_tools_returns_none_for_empty_tool_sets() {
        assert_eq!(prepare_tools(Vec::<Tool>::new().iter()), None);
    }

    #[test]
    fn prepare_tools_converts_high_level_tools() {
        let tools = vec![Tool::new("weather", object_schema())];

        assert_eq!(
            prepare_tools(&tools),
            Some(vec![LanguageModelTool::Function(
                LanguageModelFunctionTool::new("weather", object_schema())
            )])
        );
    }

    #[test]
    fn tool_execution_options_serialize_as_camel_case() {
        let options = ToolExecutionOptions::new(
            "call-1",
            vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
                vec![LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new("Weather?"),
                )],
            ))],
        );

        assert_eq!(
            serde_json::to_value(options).expect("execution options serialize"),
            json!({
                "toolCallId": "call-1",
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "Weather?"
                            }
                        ]
                    }
                ]
            })
        );
    }

    #[test]
    fn tool_executor_returns_json_results() {
        let tool = Tool::new("weather", object_schema()).with_execute(|input, options| {
            ready(Ok(json!({
                "input": input,
                "toolCallId": options.tool_call_id
            })))
        });

        assert!(tool.is_executable());

        let result = poll_ready(
            tool.execute(
                json!({
                    "city": "Brisbane"
                }),
                ToolExecutionOptions::new("call-1", Vec::new()),
            )
            .expect("tool has an executor"),
        )
        .expect("tool execution succeeds");

        assert_eq!(
            result,
            json!({
                "input": {
                    "city": "Brisbane"
                },
                "toolCallId": "call-1"
            })
        );
    }

    #[test]
    fn tool_execution_error_retains_message() {
        let error = ToolExecutionError::new("Tool failed.");

        assert_eq!(error.message(), "Tool failed.");
        assert_eq!(error.to_string(), "Tool failed.");
        assert_eq!(
            serde_json::to_value(error).expect("tool execution error serializes"),
            json!({
                "message": "Tool failed."
            })
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
