use std::collections::BTreeMap;
use std::fmt;

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::file_data::FileDataContent;
use crate::headers::Headers;
use crate::json::JsonValue;
use crate::language_model::{
    LanguageModelMessage, LanguageModelPrompt, LanguageModelSystemMessage, LanguageModelTextPart,
    LanguageModelUserContentPart, LanguageModelUserMessage,
};
use crate::provider::InvalidPromptError;
use crate::provider_utils::convert_to_base64;

/// Timeout configuration for high-level model and tool requests.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TimeoutConfiguration {
    /// A single total request timeout in milliseconds.
    TotalMs(u64),

    /// Granular timeout settings for individual request phases.
    Detailed(TimeoutConfigurationOptions),
}

impl TimeoutConfiguration {
    /// Creates a total timeout configuration in milliseconds.
    pub const fn total_ms(total_ms: u64) -> Self {
        Self::TotalMs(total_ms)
    }

    /// Creates a detailed timeout configuration.
    pub const fn detailed(options: TimeoutConfigurationOptions) -> Self {
        Self::Detailed(options)
    }
}

impl From<u64> for TimeoutConfiguration {
    fn from(total_ms: u64) -> Self {
        Self::TotalMs(total_ms)
    }
}

impl From<TimeoutConfigurationOptions> for TimeoutConfiguration {
    fn from(options: TimeoutConfigurationOptions) -> Self {
        Self::Detailed(options)
    }
}

/// Instructions to include alongside a high-level prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Instructions {
    /// Plain system instruction text.
    Text(String),

    /// A single system model message.
    Message(LanguageModelSystemMessage),

    /// Multiple system model messages.
    Messages(Vec<LanguageModelSystemMessage>),
}

impl Instructions {
    /// Creates text instructions.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// Creates instructions from a single system model message.
    pub fn message(message: LanguageModelSystemMessage) -> Self {
        Self::Message(message)
    }

    /// Creates instructions from multiple system model messages.
    pub fn messages(messages: Vec<LanguageModelSystemMessage>) -> Self {
        Self::Messages(messages)
    }
}

impl From<String> for Instructions {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for Instructions {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<LanguageModelSystemMessage> for Instructions {
    fn from(message: LanguageModelSystemMessage) -> Self {
        Self::Message(message)
    }
}

impl From<Vec<LanguageModelSystemMessage>> for Instructions {
    fn from(messages: Vec<LanguageModelSystemMessage>) -> Self {
        Self::Messages(messages)
    }
}

/// The mutually exclusive high-level prompt input.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PromptInput {
    /// A simple text prompt.
    Text(String),

    /// A list of model messages.
    Messages(LanguageModelPrompt),
}

impl PromptInput {
    /// Creates a text prompt input.
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// Creates a model-message prompt input.
    pub fn messages(messages: LanguageModelPrompt) -> Self {
        Self::Messages(messages)
    }
}

impl From<String> for PromptInput {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for PromptInput {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<LanguageModelPrompt> for PromptInput {
    fn from(messages: LanguageModelPrompt) -> Self {
        Self::Messages(messages)
    }
}

/// High-level prompt input for AI SDK generation calls.
///
/// This mirrors upstream `Prompt`: callers may provide either `prompt` or
/// `messages`, but not both. JavaScript-only runtime values are intentionally
/// omitted from the Rust JSON contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prompt {
    /// Instructions to include with the prompt.
    pub instructions: Option<Instructions>,

    /// Deprecated upstream alias for instructions.
    pub system: Option<Instructions>,

    /// Whether system messages are allowed in prompt/message fields.
    pub allow_system_in_messages: bool,

    /// The exclusive prompt source.
    pub source: PromptSource,
}

impl Prompt {
    /// Creates a prompt from the upstream `prompt` field.
    pub fn from_prompt(prompt: impl Into<PromptInput>) -> Self {
        Self {
            instructions: None,
            system: None,
            allow_system_in_messages: false,
            source: PromptSource::Prompt(prompt.into()),
        }
    }

    /// Creates a prompt from the upstream `messages` field.
    pub fn from_messages(messages: LanguageModelPrompt) -> Self {
        Self {
            instructions: None,
            system: None,
            allow_system_in_messages: false,
            source: PromptSource::Messages(messages),
        }
    }

    /// Sets instructions.
    pub fn with_instructions(mut self, instructions: impl Into<Instructions>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Sets the deprecated upstream `system` alias.
    pub fn with_system(mut self, system: impl Into<Instructions>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Sets whether system messages may appear in prompt/message inputs.
    pub const fn with_allow_system_in_messages(mut self, allow_system_in_messages: bool) -> Self {
        self.allow_system_in_messages = allow_system_in_messages;
        self
    }

    /// Returns the prompt field when this prompt uses the `prompt` form.
    pub fn prompt(&self) -> Option<&PromptInput> {
        match &self.source {
            PromptSource::Prompt(prompt) => Some(prompt),
            PromptSource::Messages(_) => None,
        }
    }

    /// Returns the messages field when this prompt uses the `messages` form.
    pub fn messages(&self) -> Option<&LanguageModelPrompt> {
        match &self.source {
            PromptSource::Prompt(_) => None,
            PromptSource::Messages(messages) => Some(messages),
        }
    }
}

impl Serialize for Prompt {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut field_count = 1;
        if self.instructions.is_some() {
            field_count += 1;
        }
        if self.system.is_some() {
            field_count += 1;
        }
        if self.allow_system_in_messages {
            field_count += 1;
        }

        let mut state = serializer.serialize_struct("Prompt", field_count)?;
        if let Some(instructions) = &self.instructions {
            state.serialize_field("instructions", instructions)?;
        }
        if let Some(system) = &self.system {
            state.serialize_field("system", system)?;
        }
        if self.allow_system_in_messages {
            state.serialize_field("allowSystemInMessages", &self.allow_system_in_messages)?;
        }
        match &self.source {
            PromptSource::Prompt(prompt) => state.serialize_field("prompt", prompt)?,
            PromptSource::Messages(messages) => state.serialize_field("messages", messages)?,
        }
        state.end()
    }
}

impl<'de> Deserialize<'de> for Prompt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct PromptHelper {
            #[serde(default)]
            instructions: Option<Instructions>,
            #[serde(default)]
            system: Option<Instructions>,
            #[serde(default)]
            allow_system_in_messages: bool,
            #[serde(default)]
            prompt: Option<PromptInput>,
            #[serde(default)]
            messages: Option<LanguageModelPrompt>,
        }

        let helper = PromptHelper::deserialize(deserializer)?;
        let source = match (helper.prompt, helper.messages) {
            (Some(prompt), None) => PromptSource::Prompt(prompt),
            (None, Some(messages)) => PromptSource::Messages(messages),
            (Some(_), Some(_)) => {
                return Err(serde::de::Error::custom(
                    "prompt and messages cannot both be set",
                ));
            }
            (None, None) => {
                return Err(serde::de::Error::custom(
                    "either prompt or messages must be set",
                ));
            }
        };

        Ok(Self {
            instructions: helper.instructions,
            system: helper.system,
            allow_system_in_messages: helper.allow_system_in_messages,
            source,
        })
    }
}

/// The exclusive prompt source in a high-level prompt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PromptSource {
    /// The upstream `prompt` field.
    Prompt(PromptInput),

    /// The upstream `messages` field.
    Messages(LanguageModelPrompt),
}

/// Normalized prompt input ready for model-call preparation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardizedPrompt {
    /// Instructions normalized from `instructions` or the deprecated `system`
    /// alias.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Instructions>,

    /// Model messages normalized from text prompts, prompt messages, or
    /// messages.
    pub messages: LanguageModelPrompt,
}

impl StandardizedPrompt {
    /// Creates a standardized prompt from optional instructions and messages.
    pub fn new(instructions: Option<Instructions>, messages: LanguageModelPrompt) -> Self {
        Self {
            instructions,
            messages,
        }
    }

    /// Converts this standardized prompt into its provider-facing messages.
    pub fn into_messages(self) -> LanguageModelPrompt {
        self.messages
    }
}

/// Granular request timeout settings.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeoutConfigurationOptions {
    /// Total request timeout in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_ms: Option<u64>,

    /// Timeout for each model step in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_ms: Option<u64>,

    /// Timeout between stream chunks in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_ms: Option<u64>,

    /// Default timeout for each tool execution in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_ms: Option<u64>,

    /// Per-tool timeout overrides keyed as `{toolName}Ms`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, u64>,
}

impl TimeoutConfigurationOptions {
    /// Creates an empty detailed timeout configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the total request timeout in milliseconds.
    pub const fn with_total_ms(mut self, total_ms: u64) -> Self {
        self.total_ms = Some(total_ms);
        self
    }

    /// Sets the per-step timeout in milliseconds.
    pub const fn with_step_ms(mut self, step_ms: u64) -> Self {
        self.step_ms = Some(step_ms);
        self
    }

    /// Sets the stream chunk timeout in milliseconds.
    pub const fn with_chunk_ms(mut self, chunk_ms: u64) -> Self {
        self.chunk_ms = Some(chunk_ms);
        self
    }

    /// Sets the default per-tool timeout in milliseconds.
    pub const fn with_tool_ms(mut self, tool_ms: u64) -> Self {
        self.tool_ms = Some(tool_ms);
        self
    }

    /// Sets a per-tool timeout override in milliseconds.
    pub fn with_tool_timeout(mut self, tool_name: impl Into<String>, timeout_ms: u64) -> Self {
        self.tools
            .insert(format!("{}Ms", tool_name.into()), timeout_ms);
        self
    }
}

/// Request-facing controls for high-level SDK calls.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestOptions {
    /// Maximum number of retries. Set to 0 to disable retries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<usize>,

    /// Additional HTTP headers sent by HTTP-based providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Timeout configuration for the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<TimeoutConfiguration>,
}

impl RequestOptions {
    /// Creates empty request options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum number of retries.
    pub const fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = Some(max_retries);
        self
    }

    /// Adds an HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets the timeout configuration.
    pub fn with_timeout(mut self, timeout: impl Into<TimeoutConfiguration>) -> Self {
        self.timeout = Some(timeout.into());
        self
    }
}

/// Extracts the total timeout in milliseconds from a timeout configuration.
pub const fn get_total_timeout_ms(timeout: Option<&TimeoutConfiguration>) -> Option<u64> {
    match timeout {
        None => None,
        Some(TimeoutConfiguration::TotalMs(total_ms)) => Some(*total_ms),
        Some(TimeoutConfiguration::Detailed(options)) => options.total_ms,
    }
}

/// Extracts the step timeout in milliseconds from a timeout configuration.
pub const fn get_step_timeout_ms(timeout: Option<&TimeoutConfiguration>) -> Option<u64> {
    match timeout {
        Some(TimeoutConfiguration::Detailed(options)) => options.step_ms,
        Some(TimeoutConfiguration::TotalMs(_)) | None => None,
    }
}

/// Extracts the chunk timeout in milliseconds from a timeout configuration.
pub const fn get_chunk_timeout_ms(timeout: Option<&TimeoutConfiguration>) -> Option<u64> {
    match timeout {
        Some(TimeoutConfiguration::Detailed(options)) => options.chunk_ms,
        Some(TimeoutConfiguration::TotalMs(_)) | None => None,
    }
}

/// Extracts a tool-specific timeout in milliseconds from a timeout configuration.
pub fn get_tool_timeout_ms(timeout: Option<&TimeoutConfiguration>, tool_name: &str) -> Option<u64> {
    let Some(TimeoutConfiguration::Detailed(options)) = timeout else {
        return None;
    };

    options
        .tools
        .get(&format!("{tool_name}Ms"))
        .copied()
        .or(options.tool_ms)
}

/// Converts a high-level prompt into normalized model messages.
///
/// This mirrors upstream `standardizePrompt` for the Rust prompt boundary:
/// text prompts become a single user text message, `instructions` takes
/// precedence over the deprecated `system` alias, empty message arrays are
/// rejected, and system messages are only allowed in prompt/message fields when
/// explicitly enabled.
pub fn standardize_prompt(prompt: Prompt) -> Result<StandardizedPrompt, InvalidPromptError> {
    let prompt_value = serde_json::to_value(&prompt).unwrap_or(JsonValue::Null);
    let Prompt {
        instructions,
        system,
        allow_system_in_messages,
        source,
    } = prompt;

    let instructions = instructions.or(system);
    let messages = match source {
        PromptSource::Prompt(PromptInput::Text(text)) => {
            vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
                vec![LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new(text),
                )],
            ))]
        }
        PromptSource::Prompt(PromptInput::Messages(messages))
        | PromptSource::Messages(messages) => messages,
    };

    if messages.is_empty() {
        return Err(InvalidPromptError::new(
            prompt_value,
            "messages must not be empty",
        ));
    }

    if !allow_system_in_messages
        && messages
            .iter()
            .any(|message| matches!(message, LanguageModelMessage::System(_)))
    {
        return Err(InvalidPromptError::new(
            prompt_value,
            "System messages are not allowed in the prompt or messages fields. Use the instructions option instead.",
        ));
    }

    Ok(StandardizedPrompt::new(instructions, messages))
}

/// Converts prompt data content to a base64-encoded string.
///
/// This mirrors upstream `convertDataContentToBase64String`: string content is
/// already base64 and passes through unchanged, while byte content is encoded.
pub fn convert_data_content_to_base64_string(content: &FileDataContent) -> String {
    convert_to_base64(content)
}

/// Error returned when prompt data content is not a supported media-data value.
#[derive(Clone, Debug, PartialEq)]
pub struct InvalidDataContentError {
    content: JsonValue,
    message: String,
}

impl InvalidDataContentError {
    /// Creates an invalid-data-content error with the upstream default message.
    pub fn new(content: impl Into<JsonValue>) -> Self {
        let content = content.into();
        let message = invalid_data_content_default_message(&content);

        Self { content, message }
    }

    /// Creates an invalid-data-content error with a caller-supplied message.
    pub fn with_message(content: impl Into<JsonValue>, message: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            message: message.into(),
        }
    }

    /// Returns the invalid content value.
    pub fn content(&self) -> &JsonValue {
        &self.content
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its retained content and message.
    pub fn into_parts(self) -> (JsonValue, String) {
        (self.content, self.message)
    }
}

impl fmt::Display for InvalidDataContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvalidDataContentError {}

/// Error returned when a UI message cannot be converted to a model message.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageConversionError {
    original_message: JsonValue,
    message: String,
}

impl MessageConversionError {
    /// Creates a message-conversion error with the original UI message context.
    pub fn new(original_message: impl Into<JsonValue>, message: impl Into<String>) -> Self {
        Self {
            original_message: original_message.into(),
            message: message.into(),
        }
    }

    /// Returns the original UI message that failed conversion.
    pub fn original_message(&self) -> &JsonValue {
        &self.original_message
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its retained original message and message text.
    pub fn into_parts(self) -> (JsonValue, String) {
        (self.original_message, self.message)
    }
}

impl fmt::Display for MessageConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for MessageConversionError {}

/// Error returned when a prompt message role is not supported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidMessageRoleError {
    role: String,
    message: String,
}

impl InvalidMessageRoleError {
    /// Creates an invalid-message-role error with the upstream default message.
    pub fn new(role: impl Into<String>) -> Self {
        let role = role.into();
        let message = invalid_message_role_default_message(&role);

        Self { role, message }
    }

    /// Creates an invalid-message-role error with a caller-supplied message.
    pub fn with_message(role: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            message: message.into(),
        }
    }

    /// Returns the unsupported message role.
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its retained role and message.
    pub fn into_parts(self) -> (String, String) {
        (self.role, self.message)
    }
}

impl fmt::Display for InvalidMessageRoleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvalidMessageRoleError {}

fn invalid_message_role_default_message(role: &str) -> String {
    format!(
        r#"Invalid message role: '{role}'. Must be one of: "system", "user", "assistant", "tool"."#
    )
}

fn invalid_data_content_default_message(content: &JsonValue) -> String {
    format!(
        "Invalid data content. Expected a base64 string, Uint8Array, ArrayBuffer, or Buffer, but got {}.",
        json_value_js_typeof(content)
    )
}

fn json_value_js_typeof(content: &JsonValue) -> &'static str {
    match content {
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Null | JsonValue::Array(_) | JsonValue::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use crate::file_data::FileDataContent;
    use crate::json::JsonValue;
    use crate::language_model::{
        LanguageModelMessage, LanguageModelPrompt, LanguageModelSystemMessage,
        LanguageModelTextPart, LanguageModelUserContentPart, LanguageModelUserMessage,
    };

    use super::{
        Instructions, InvalidDataContentError, InvalidMessageRoleError, MessageConversionError,
        Prompt, PromptInput, PromptSource, RequestOptions, StandardizedPrompt,
        TimeoutConfiguration, TimeoutConfigurationOptions, convert_data_content_to_base64_string,
        get_chunk_timeout_ms, get_step_timeout_ms, get_tool_timeout_ms, get_total_timeout_ms,
        standardize_prompt,
    };

    fn user_text_message(text: &str) -> LanguageModelMessage {
        LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new(text)),
        ]))
    }

    fn system_message(text: &str) -> LanguageModelSystemMessage {
        LanguageModelSystemMessage::new(text)
    }

    #[test]
    fn timeout_configuration_serializes_number_form() {
        let timeout = TimeoutConfiguration::total_ms(5_000);

        assert_eq!(
            serde_json::to_value(timeout).expect("timeout serialize"),
            json!(5000)
        );
    }

    #[test]
    fn timeout_configuration_serializes_detailed_form() {
        let timeout = TimeoutConfiguration::detailed(
            TimeoutConfigurationOptions::new()
                .with_total_ms(30_000)
                .with_step_ms(10_000)
                .with_chunk_ms(2_000)
                .with_tool_ms(5_000)
                .with_tool_timeout("search", 1_000),
        );

        assert_eq!(
            serde_json::to_value(timeout).expect("timeout serialize"),
            json!({
                "totalMs": 30000,
                "stepMs": 10000,
                "chunkMs": 2000,
                "toolMs": 5000,
                "tools": {
                    "searchMs": 1000
                }
            })
        );
    }

    #[test]
    fn timeout_configuration_deserializes_detailed_form() {
        let timeout: TimeoutConfiguration = serde_json::from_value(json!({
            "totalMs": 10000,
            "tools": {
                "weatherMs": 2500
            }
        }))
        .expect("timeout deserialize");

        assert_eq!(
            timeout,
            TimeoutConfiguration::Detailed(TimeoutConfigurationOptions {
                total_ms: Some(10_000),
                step_ms: None,
                chunk_ms: None,
                tool_ms: None,
                tools: BTreeMap::from([("weatherMs".to_string(), 2_500)])
            })
        );
    }

    #[test]
    fn request_options_serializes_upstream_shape_without_abort_signal() {
        let options = RequestOptions::new()
            .with_max_retries(3)
            .with_header("x-api-key", "sk-test")
            .with_timeout(TimeoutConfigurationOptions::new().with_step_ms(4_000));

        assert_eq!(
            serde_json::to_value(options).expect("request options serialize"),
            json!({
                "maxRetries": 3,
                "headers": {
                    "x-api-key": "sk-test"
                },
                "timeout": {
                    "stepMs": 4000
                }
            })
        );
    }

    #[test]
    fn request_options_deserializes_minimal_shape() {
        let options: RequestOptions = serde_json::from_value(json!({})).expect("deserialize");

        assert_eq!(options, RequestOptions::new());
    }

    #[test]
    fn instructions_serialize_upstream_union_shapes() {
        assert_eq!(
            serde_json::to_value(Instructions::text("Be concise.")).expect("serialize"),
            json!("Be concise.")
        );
        assert_eq!(
            serde_json::to_value(Instructions::message(system_message("Use metric units.")))
                .expect("serialize"),
            json!({
                "role": "system",
                "content": "Use metric units."
            })
        );
        assert_eq!(
            serde_json::to_value(Instructions::messages(vec![
                system_message("Be concise."),
                system_message("Use metric units.")
            ]))
            .expect("serialize"),
            json!([
                {
                    "role": "system",
                    "content": "Be concise."
                },
                {
                    "role": "system",
                    "content": "Use metric units."
                }
            ])
        );
    }

    #[test]
    fn prompt_serializes_text_prompt_with_optional_common_fields() {
        let prompt = Prompt::from_prompt("What is the weather?")
            .with_instructions("Answer briefly.")
            .with_system(system_message("Legacy system instructions."))
            .with_allow_system_in_messages(true);

        assert_eq!(
            serde_json::to_value(prompt).expect("prompt serialize"),
            json!({
                "instructions": "Answer briefly.",
                "system": {
                    "role": "system",
                    "content": "Legacy system instructions."
                },
                "allowSystemInMessages": true,
                "prompt": "What is the weather?"
            })
        );
    }

    #[test]
    fn prompt_deserializes_messages_form() {
        let messages: LanguageModelPrompt = vec![user_text_message("Hello")];

        let prompt: Prompt = serde_json::from_value(json!({
            "instructions": [
                {
                    "role": "system",
                    "content": "Be concise."
                }
            ],
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "Hello"
                        }
                    ]
                }
            ]
        }))
        .expect("prompt deserialize");

        assert_eq!(
            prompt,
            Prompt::from_messages(messages).with_instructions(vec![system_message("Be concise.")])
        );
        assert_eq!(
            prompt.messages().expect("messages"),
            &vec![user_text_message("Hello")]
        );
        assert_eq!(prompt.prompt(), None);
    }

    #[test]
    fn prompt_input_supports_message_array_in_prompt_field() {
        let prompt = Prompt::from_prompt(PromptInput::messages(vec![user_text_message("Hello")]));

        assert_eq!(
            serde_json::to_value(prompt).expect("prompt serialize"),
            json!({
                "prompt": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "Hello"
                            }
                        ]
                    }
                ]
            })
        );
    }

    #[test]
    fn prompt_deserialization_rejects_invalid_prompt_union_shapes() {
        assert!(
            serde_json::from_value::<Prompt>(json!({
                "prompt": "Hello",
                "messages": []
            }))
            .is_err()
        );
        assert!(serde_json::from_value::<Prompt>(json!({})).is_err());

        let prompt = Prompt::from_prompt("Hello");
        assert_eq!(
            prompt.source,
            PromptSource::Prompt(PromptInput::Text("Hello".to_string()))
        );
    }

    #[test]
    fn standardize_prompt_converts_text_prompt_and_prefers_instructions() {
        let standardized = standardize_prompt(
            Prompt::from_prompt("Hello, world!")
                .with_system("SYSTEM")
                .with_instructions("INSTRUCTIONS"),
        )
        .expect("prompt standardizes");

        let expected = StandardizedPrompt::new(
            Some(Instructions::text("INSTRUCTIONS")),
            vec![user_text_message("Hello, world!")],
        );

        assert_eq!(standardized, expected);
        assert_eq!(
            serde_json::to_value(&standardized).expect("standardized prompt serializes"),
            json!({
                "instructions": "INSTRUCTIONS",
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "Hello, world!"
                            }
                        ]
                    }
                ]
            })
        );
        assert_eq!(
            serde_json::from_value::<StandardizedPrompt>(
                serde_json::to_value(&standardized).expect("standardized prompt serializes")
            )
            .expect("standardized prompt deserializes"),
            standardized
        );
    }

    #[test]
    fn standardize_prompt_falls_back_to_system_alias() {
        let standardized =
            standardize_prompt(Prompt::from_prompt("Hello").with_system(system_message("SYSTEM")))
                .expect("prompt standardizes");

        assert_eq!(
            standardized.instructions,
            Some(Instructions::message(system_message("SYSTEM")))
        );
        assert_eq!(standardized.messages, vec![user_text_message("Hello")]);
    }

    #[test]
    fn standardize_prompt_rejects_empty_message_arrays() {
        let error = standardize_prompt(Prompt::from_messages(Vec::new()))
            .expect_err("empty messages are rejected");

        assert_eq!(
            error.message(),
            "Invalid prompt: messages must not be empty"
        );
    }

    #[test]
    fn standardize_prompt_enforces_system_message_location() {
        let messages = vec![
            LanguageModelMessage::System(system_message("SYSTEM")),
            user_text_message("Hello"),
        ];

        let error = standardize_prompt(Prompt::from_messages(messages.clone()))
            .expect_err("system messages are rejected by default");
        assert_eq!(
            error.message(),
            "Invalid prompt: System messages are not allowed in the prompt or messages fields. Use the instructions option instead."
        );

        let standardized = standardize_prompt(
            Prompt::from_messages(messages.clone()).with_allow_system_in_messages(true),
        )
        .expect("system messages are allowed when configured");

        assert_eq!(standardized.messages, messages);
        assert_eq!(standardized.instructions, None);
    }

    #[test]
    fn timeout_helpers_match_upstream_number_and_missing_behavior() {
        let total = TimeoutConfiguration::total_ms(5_000);

        assert_eq!(get_total_timeout_ms(None), None);
        assert_eq!(get_total_timeout_ms(Some(&total)), Some(5_000));
        assert_eq!(get_step_timeout_ms(Some(&total)), None);
        assert_eq!(get_chunk_timeout_ms(Some(&total)), None);
        assert_eq!(get_tool_timeout_ms(Some(&total), "search"), None);
    }

    #[test]
    fn timeout_helpers_read_detailed_timeouts() {
        let timeout = TimeoutConfiguration::detailed(
            TimeoutConfigurationOptions::new()
                .with_total_ms(30_000)
                .with_step_ms(10_000)
                .with_chunk_ms(2_000)
                .with_tool_ms(5_000)
                .with_tool_timeout("search", 1_000),
        );

        assert_eq!(get_total_timeout_ms(Some(&timeout)), Some(30_000));
        assert_eq!(get_step_timeout_ms(Some(&timeout)), Some(10_000));
        assert_eq!(get_chunk_timeout_ms(Some(&timeout)), Some(2_000));
        assert_eq!(get_tool_timeout_ms(Some(&timeout), "search"), Some(1_000));
        assert_eq!(get_tool_timeout_ms(Some(&timeout), "weather"), Some(5_000));
    }

    #[test]
    fn convert_data_content_to_base64_string_passes_base64_strings_through() {
        assert_eq!(
            convert_data_content_to_base64_string(&FileDataContent::Base64(
                "already-base64".to_string()
            )),
            "already-base64"
        );
    }

    #[test]
    fn convert_data_content_to_base64_string_encodes_bytes() {
        assert_eq!(
            convert_data_content_to_base64_string(&FileDataContent::Bytes(b"Hello".to_vec())),
            "SGVsbG8="
        );
    }

    #[test]
    fn invalid_data_content_error_matches_upstream_default_message() {
        let content = json!({ "data": false });
        let error = InvalidDataContentError::new(content.clone());

        assert_eq!(error.content(), &content);
        assert_eq!(
            error.message(),
            "Invalid data content. Expected a base64 string, Uint8Array, ArrayBuffer, or Buffer, but got object."
        );
        assert_eq!(error.to_string(), error.message());
    }

    #[test]
    fn invalid_data_content_error_uses_json_typeof_for_default_message() {
        assert_eq!(
            InvalidDataContentError::new(true).message(),
            "Invalid data content. Expected a base64 string, Uint8Array, ArrayBuffer, or Buffer, but got boolean."
        );
        assert_eq!(
            InvalidDataContentError::new(42).message(),
            "Invalid data content. Expected a base64 string, Uint8Array, ArrayBuffer, or Buffer, but got number."
        );
        assert_eq!(
            InvalidDataContentError::new("not-base64").message(),
            "Invalid data content. Expected a base64 string, Uint8Array, ArrayBuffer, or Buffer, but got string."
        );
        assert_eq!(
            InvalidDataContentError::new(JsonValue::Null).message(),
            "Invalid data content. Expected a base64 string, Uint8Array, ArrayBuffer, or Buffer, but got object."
        );
    }

    #[test]
    fn invalid_data_content_error_supports_custom_message_and_parts() {
        let error = InvalidDataContentError::with_message(
            "data:text/plain,hello",
            "Invalid data URL format in content data:text/plain,hello",
        );

        assert_eq!(
            error.into_parts(),
            (
                JsonValue::String("data:text/plain,hello".to_string()),
                "Invalid data URL format in content data:text/plain,hello".to_string()
            )
        );
    }

    #[test]
    fn message_conversion_error_retains_original_message_and_message_text() {
        let original_message = json!({
            "role": "unknown",
            "parts": [{ "type": "text", "text": "unknown role message" }]
        });
        let error =
            MessageConversionError::new(original_message.clone(), "Unsupported role: unknown");

        assert_eq!(error.original_message(), &original_message);
        assert_eq!(error.message(), "Unsupported role: unknown");
        assert_eq!(error.to_string(), error.message());
    }

    #[test]
    fn message_conversion_error_supports_parts_conversion() {
        let original_message = json!({
            "role": "assistant",
            "parts": [{ "type": "custom", "kind": "example.part" }]
        });
        let error = MessageConversionError::new(
            original_message.clone(),
            "Unsupported custom UI message part",
        );

        assert_eq!(
            error.into_parts(),
            (
                original_message,
                "Unsupported custom UI message part".to_string()
            )
        );
    }

    #[test]
    fn invalid_message_role_error_matches_upstream_default_message() {
        let error = InvalidMessageRoleError::new("developer");

        assert_eq!(error.role(), "developer");
        assert_eq!(
            error.message(),
            r#"Invalid message role: 'developer'. Must be one of: "system", "user", "assistant", "tool"."#
        );
        assert_eq!(error.to_string(), error.message());
    }

    #[test]
    fn invalid_message_role_error_supports_custom_message_and_parts() {
        let error = InvalidMessageRoleError::with_message("chat", "custom role failure");

        assert_eq!(error.role(), "chat");
        assert_eq!(error.message(), "custom role failure");
        assert_eq!(
            error.into_parts(),
            ("chat".to_string(), "custom role failure".to_string())
        );
    }
}
