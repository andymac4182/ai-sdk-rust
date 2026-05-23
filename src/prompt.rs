use std::collections::BTreeMap;
use std::fmt;

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::file_data::{FileData, FileDataContent};
use crate::headers::Headers;
use crate::json::JsonValue;
use crate::language_model::{
    LanguageModelMessage, LanguageModelPrompt, LanguageModelReasoningEffort,
    LanguageModelSystemMessage, LanguageModelTextPart, LanguageModelToolChoice,
    LanguageModelUserContentPart, LanguageModelUserMessage,
};
use crate::provider::InvalidPromptError;
use crate::provider_utils::convert_to_base64;
use crate::util::InvalidArgumentError;

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

    /// Converts this standardized prompt into a language model prompt.
    ///
    /// Upstream `convertToLanguageModelPrompt` prepends `instructions` as
    /// system messages before the standardized message history. This Rust
    /// boundary already stores messages in provider-v4 shape, so instruction
    /// insertion is the remaining conversion step here.
    pub fn into_language_model_prompt(self) -> LanguageModelPrompt {
        let mut messages = match self.instructions {
            Some(instructions) => instructions_to_system_messages(instructions)
                .into_iter()
                .map(LanguageModelMessage::System)
                .collect::<LanguageModelPrompt>(),
            None => Vec::new(),
        };

        messages.extend(self.messages);
        messages
    }
}

fn instructions_to_system_messages(instructions: Instructions) -> Vec<LanguageModelSystemMessage> {
    match instructions {
        Instructions::Text(text) => vec![LanguageModelSystemMessage::new(text)],
        Instructions::Message(message) => vec![message],
        Instructions::Messages(messages) => messages,
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

/// Model-facing generation controls for high-level language model calls.
///
/// This mirrors upstream `LanguageModelCallOptions` from `packages/ai` without
/// the provider-owned prompt fields that live in `LanguageModelCallOptions`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelCallSettings {
    /// Maximum number of tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,

    /// Temperature setting. The range depends on the provider and model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Nucleus sampling setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    /// Top-k sampling setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u64>,

    /// Presence penalty setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,

    /// Frequency penalty setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,

    /// Stop sequences that stop generation when emitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Seed used for deterministic sampling when supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,

    /// Reasoning effort requested for the model call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<LanguageModelReasoningEffort>,
}

impl LanguageModelCallSettings {
    /// Creates empty high-level language model call settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum number of output tokens.
    pub const fn with_max_output_tokens(mut self, max_output_tokens: u64) -> Self {
        self.max_output_tokens = Some(max_output_tokens);
        self
    }

    /// Sets the sampling temperature.
    pub const fn with_temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Sets nucleus sampling.
    pub const fn with_top_p(mut self, top_p: f64) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Sets top-k sampling.
    pub const fn with_top_k(mut self, top_k: u64) -> Self {
        self.top_k = Some(top_k);
        self
    }

    /// Sets the presence penalty.
    pub const fn with_presence_penalty(mut self, presence_penalty: f64) -> Self {
        self.presence_penalty = Some(presence_penalty);
        self
    }

    /// Sets the frequency penalty.
    pub const fn with_frequency_penalty(mut self, frequency_penalty: f64) -> Self {
        self.frequency_penalty = Some(frequency_penalty);
        self
    }

    /// Sets stop sequences.
    pub fn with_stop_sequences(mut self, stop_sequences: Vec<String>) -> Self {
        self.stop_sequences = Some(stop_sequences);
        self
    }

    /// Sets the deterministic sampling seed.
    pub const fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Sets the reasoning effort.
    pub const fn with_reasoning(mut self, reasoning: LanguageModelReasoningEffort) -> Self {
        self.reasoning = Some(reasoning);
        self
    }
}

/// Validates high-level language model call settings and returns limited values.
pub fn prepare_language_model_call_options(
    options: LanguageModelCallSettings,
) -> Result<LanguageModelCallSettings, InvalidArgumentError> {
    if options.max_output_tokens == Some(0) {
        return Err(InvalidArgumentError::new(
            "maxOutputTokens",
            JsonValue::from(0),
            "maxOutputTokens must be >= 1",
        ));
    }

    Ok(options)
}

/// Prepares the language-model tool choice for a high-level model call.
pub fn prepare_tool_choice(
    tool_choice: Option<LanguageModelToolChoice>,
) -> LanguageModelToolChoice {
    tool_choice.unwrap_or(LanguageModelToolChoice::Auto)
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

pub(crate) fn prompt_has_url_files(prompt: &LanguageModelPrompt) -> bool {
    prompt.iter().any(|message| match message {
        LanguageModelMessage::User(message) => message.content.iter().any(|part| match part {
            LanguageModelUserContentPart::File(file) => matches!(file.data, FileData::Url { .. }),
            LanguageModelUserContentPart::Text(_) => false,
        }),
        LanguageModelMessage::System(_)
        | LanguageModelMessage::Assistant(_)
        | LanguageModelMessage::Tool(_) => false,
    })
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
        LanguageModelMessage, LanguageModelPrompt, LanguageModelReasoningEffort,
        LanguageModelSystemMessage, LanguageModelTextPart, LanguageModelToolChoice,
        LanguageModelUserContentPart, LanguageModelUserMessage,
    };

    use super::{
        Instructions, InvalidDataContentError, InvalidMessageRoleError, LanguageModelCallSettings,
        MessageConversionError, Prompt, PromptInput, PromptSource, RequestOptions,
        StandardizedPrompt, TimeoutConfiguration, TimeoutConfigurationOptions,
        convert_data_content_to_base64_string, get_chunk_timeout_ms, get_step_timeout_ms,
        get_tool_timeout_ms, get_total_timeout_ms, prepare_language_model_call_options,
        prepare_tool_choice, standardize_prompt,
    };

    fn user_text_message(text: &str) -> LanguageModelMessage {
        LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new(text)),
        ]))
    }

    fn system_message(text: &str) -> LanguageModelSystemMessage {
        LanguageModelSystemMessage::new(text)
    }

    fn assert_call_settings_type_boundary_rejects(settings: JsonValue) {
        serde_json::from_value::<LanguageModelCallSettings>(settings)
            .expect_err("invalid dynamic JavaScript-shaped input is rejected by serde");
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
    fn standardize_prompt_should_throw_invalid_prompt_error_when_messages_contain_a_system_message_by_default()
     {
        let messages = vec![
            LanguageModelMessage::System(system_message("INSTRUCTIONS")),
            user_text_message("Hello, world!"),
        ];

        let error = standardize_prompt(Prompt::from_messages(messages))
            .expect_err("system messages are rejected by default");

        assert_eq!(
            error.message(),
            "Invalid prompt: System messages are not allowed in the prompt or messages fields. Use the instructions option instead."
        );
    }

    #[test]
    fn standardize_prompt_should_throw_invalid_prompt_error_when_prompt_messages_contain_a_system_message_by_default()
     {
        let messages = vec![
            LanguageModelMessage::System(system_message("INSTRUCTIONS")),
            user_text_message("Hello, world!"),
        ];

        let error = standardize_prompt(Prompt::from_prompt(PromptInput::messages(messages)))
            .expect_err("system messages in prompt array are rejected by default");

        assert_eq!(
            error.message(),
            "Invalid prompt: System messages are not allowed in the prompt or messages fields. Use the instructions option instead."
        );
    }

    #[test]
    fn standardize_prompt_should_allow_system_messages_in_messages_when_allow_system_in_messages_is_true()
     {
        let messages = vec![
            LanguageModelMessage::System(system_message("INSTRUCTIONS")),
            user_text_message("Hello, world!"),
        ];

        let standardized = standardize_prompt(
            Prompt::from_messages(messages.clone()).with_allow_system_in_messages(true),
        )
        .expect("system messages are allowed when configured");

        assert_eq!(standardized.instructions, None);
        assert_eq!(standardized.messages, messages);
    }

    #[test]
    fn standardize_prompt_should_allow_system_messages_in_prompt_messages_when_allow_system_in_messages_is_true()
     {
        let messages = vec![
            LanguageModelMessage::System(system_message("INSTRUCTIONS")),
            user_text_message("Hello, world!"),
        ];

        let standardized = standardize_prompt(
            Prompt::from_prompt(PromptInput::messages(messages.clone()))
                .with_allow_system_in_messages(true),
        )
        .expect("system messages in prompt array are allowed when configured");

        assert_eq!(standardized.instructions, None);
        assert_eq!(standardized.messages, messages);
    }

    #[test]
    fn standardize_prompt_should_reject_allowed_system_message_parts_at_type_boundary() {
        let prompt = serde_json::from_value::<Prompt>(json!({
            "allowSystemInMessages": true,
            "messages": [
                {
                    "role": "system",
                    "content": [
                        {
                            "type": "text",
                            "text": "test"
                        }
                    ]
                }
            ]
        }));

        assert!(prompt.is_err());
    }

    #[test]
    fn standardize_prompt_should_throw_invalid_prompt_error_when_messages_array_is_empty() {
        let error = standardize_prompt(Prompt::from_messages(Vec::new()))
            .expect_err("empty messages are rejected");

        assert_eq!(
            error.message(),
            "Invalid prompt: messages must not be empty"
        );
    }

    #[test]
    fn standardize_prompt_should_support_system_model_message_instructions() {
        let standardized = standardize_prompt(
            Prompt::from_prompt("Hello, world!").with_instructions(system_message("INSTRUCTIONS")),
        )
        .expect("prompt standardizes");

        assert_eq!(
            standardized,
            StandardizedPrompt::new(
                Some(Instructions::message(system_message("INSTRUCTIONS"))),
                vec![user_text_message("Hello, world!")],
            )
        );
    }

    #[test]
    fn standardize_prompt_should_support_array_of_system_model_message_instructions() {
        let instructions = vec![
            system_message("INSTRUCTIONS"),
            system_message("INSTRUCTIONS 2"),
        ];

        let standardized = standardize_prompt(
            Prompt::from_prompt("Hello, world!").with_instructions(instructions.clone()),
        )
        .expect("prompt standardizes");

        assert_eq!(
            standardized,
            StandardizedPrompt::new(
                Some(Instructions::messages(instructions)),
                vec![user_text_message("Hello, world!")],
            )
        );
    }

    #[test]
    fn standardize_prompt_should_fall_back_to_system_when_instructions_is_not_defined() {
        let standardized = standardize_prompt(
            Prompt::from_prompt("Hello, world!").with_system(system_message("SYSTEM")),
        )
        .expect("prompt standardizes");

        assert_eq!(
            standardized,
            StandardizedPrompt::new(
                Some(Instructions::message(system_message("SYSTEM"))),
                vec![user_text_message("Hello, world!")],
            )
        );
    }

    #[test]
    fn standardize_prompt_should_prefer_instructions_over_system() {
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
    fn standardized_prompt_prepends_instructions_as_system_messages() {
        let prompt = StandardizedPrompt::new(
            Some(Instructions::messages(vec![
                system_message("First instruction."),
                system_message("Second instruction."),
            ])),
            vec![user_text_message("Hello")],
        );

        assert_eq!(
            prompt.into_language_model_prompt(),
            vec![
                LanguageModelMessage::System(system_message("First instruction.")),
                LanguageModelMessage::System(system_message("Second instruction.")),
                user_text_message("Hello"),
            ]
        );
    }

    #[test]
    fn prepare_language_model_call_options_should_not_throw_an_error_for_valid_settings() {
        let settings = LanguageModelCallSettings::new()
            .with_max_output_tokens(100)
            .with_temperature(0.7)
            .with_top_p(0.9)
            .with_top_k(50)
            .with_presence_penalty(0.5)
            .with_frequency_penalty(0.3)
            .with_seed(42);

        assert_eq!(
            prepare_language_model_call_options(settings.clone()).expect("settings are valid"),
            settings
        );
    }

    #[test]
    fn prepare_language_model_call_options_should_allow_undefined_values_for_optional_settings() {
        let settings: LanguageModelCallSettings = serde_json::from_value(json!({
            "maxOutputTokens": null,
            "temperature": null,
            "topP": null,
            "topK": null,
            "presencePenalty": null,
            "frequencyPenalty": null,
            "seed": null
        }))
        .expect("null optional settings deserialize");

        assert_eq!(settings, LanguageModelCallSettings::new());
        assert_eq!(
            prepare_language_model_call_options(settings.clone()).expect("settings are valid"),
            settings
        );
    }

    #[test]
    fn prepare_language_model_call_options_should_reject_non_integer_max_output_tokens_at_type_boundary()
     {
        assert_call_settings_type_boundary_rejects(json!({
            "maxOutputTokens": 10.5
        }));
    }

    #[test]
    fn prepare_language_model_call_options_should_throw_invalid_argument_error_if_max_output_tokens_is_less_than_1()
     {
        let error = prepare_language_model_call_options(
            LanguageModelCallSettings::new().with_max_output_tokens(0),
        )
        .expect_err("zero max output tokens is invalid");

        assert_eq!(error.parameter(), "maxOutputTokens");
        assert_eq!(error.value(), &json!(0));
        assert_eq!(
            error.message(),
            "Invalid argument for parameter maxOutputTokens: maxOutputTokens must be >= 1"
        );
    }

    #[test]
    fn prepare_language_model_call_options_should_reject_temperature_if_temperature_is_not_a_number_at_type_boundary()
     {
        assert_call_settings_type_boundary_rejects(json!({
            "temperature": "invalid"
        }));
    }

    #[test]
    fn prepare_language_model_call_options_should_reject_top_p_if_top_p_is_not_a_number_at_type_boundary()
     {
        assert_call_settings_type_boundary_rejects(json!({
            "topP": "invalid"
        }));
    }

    #[test]
    fn prepare_language_model_call_options_should_reject_top_k_if_top_k_is_not_a_number_at_type_boundary()
     {
        assert_call_settings_type_boundary_rejects(json!({
            "topK": "invalid"
        }));
    }

    #[test]
    fn prepare_language_model_call_options_should_reject_presence_penalty_if_presence_penalty_is_not_a_number_at_type_boundary()
     {
        assert_call_settings_type_boundary_rejects(json!({
            "presencePenalty": "invalid"
        }));
    }

    #[test]
    fn prepare_language_model_call_options_should_reject_frequency_penalty_if_frequency_penalty_is_not_a_number_at_type_boundary()
     {
        assert_call_settings_type_boundary_rejects(json!({
            "frequencyPenalty": "invalid"
        }));
    }

    #[test]
    fn prepare_language_model_call_options_should_reject_non_integer_seed_at_type_boundary() {
        assert_call_settings_type_boundary_rejects(json!({
            "seed": 10.5
        }));
    }

    #[test]
    fn prepare_language_model_call_options_should_pass_through_valid_reasoning_values() {
        for reasoning in [
            LanguageModelReasoningEffort::None,
            LanguageModelReasoningEffort::Minimal,
            LanguageModelReasoningEffort::Low,
            LanguageModelReasoningEffort::Medium,
            LanguageModelReasoningEffort::High,
            LanguageModelReasoningEffort::Xhigh,
        ] {
            let settings = LanguageModelCallSettings::new().with_reasoning(reasoning.clone());
            let options =
                prepare_language_model_call_options(settings).expect("reasoning is valid");

            assert_eq!(options.reasoning, Some(reasoning));
        }
    }

    #[test]
    fn prepare_language_model_call_options_should_pass_through_provider_default() {
        let options = prepare_language_model_call_options(
            LanguageModelCallSettings::new()
                .with_reasoning(LanguageModelReasoningEffort::ProviderDefault),
        )
        .expect("provider-default reasoning is valid");

        assert_eq!(
            options.reasoning,
            Some(LanguageModelReasoningEffort::ProviderDefault)
        );
    }

    #[test]
    fn prepare_language_model_call_options_should_pass_through_undefined() {
        let options = prepare_language_model_call_options(LanguageModelCallSettings::new())
            .expect("missing reasoning is valid");

        assert_eq!(options.reasoning, None);
    }

    #[test]
    fn prepare_language_model_call_options_should_return_a_new_object_with_limited_values() {
        let settings: LanguageModelCallSettings = serde_json::from_value(json!({
            "maxOutputTokens": 100,
            "temperature": 0.7,
            "random": "invalid"
        }))
        .expect("unknown fields are ignored at typed boundary");

        let options =
            prepare_language_model_call_options(settings).expect("limited settings are valid");

        assert_eq!(
            serde_json::to_value(options).expect("settings serialize"),
            json!({
                "maxOutputTokens": 100,
                "temperature": 0.7
            })
        );
    }

    #[test]
    fn prepare_tool_choice_returns_auto_when_tool_choice_is_not_provided() {
        let result = prepare_tool_choice(None);

        assert_eq!(
            serde_json::to_value(result).expect("tool choice serializes"),
            json!({ "type": "auto" })
        );
    }

    #[test]
    fn prepare_tool_choice_handles_string_tool_choice_none() {
        let result = prepare_tool_choice(Some(LanguageModelToolChoice::None));

        assert_eq!(
            serde_json::to_value(result).expect("tool choice serializes"),
            json!({ "type": "none" })
        );
    }

    #[test]
    fn prepare_tool_choice_handles_object_tool_choice() {
        let result = prepare_tool_choice(Some(LanguageModelToolChoice::Tool {
            tool_name: "tool2".to_string(),
        }));

        assert_eq!(
            serde_json::to_value(result).expect("tool choice serializes"),
            json!({
                "type": "tool",
                "toolName": "tool2",
            })
        );
    }

    #[test]
    fn prepare_tool_choice_handles_string_tool_choice_auto() {
        let result = prepare_tool_choice(Some(LanguageModelToolChoice::Auto));

        assert_eq!(
            serde_json::to_value(result).expect("tool choice serializes"),
            json!({ "type": "auto" })
        );
    }

    #[test]
    fn prepare_tool_choice_handles_string_tool_choice_required() {
        let result = prepare_tool_choice(Some(LanguageModelToolChoice::Required));

        assert_eq!(
            serde_json::to_value(result).expect("tool choice serializes"),
            json!({ "type": "required" })
        );
    }

    #[test]
    fn get_tool_timeout_ms_should_return_undefined_when_timeout_is_undefined() {
        assert_eq!(get_tool_timeout_ms(None, "testTool"), None);
    }

    #[test]
    fn get_tool_timeout_ms_should_return_undefined_when_timeout_is_a_number() {
        let total = TimeoutConfiguration::total_ms(5_000);

        assert_eq!(get_tool_timeout_ms(Some(&total), "testTool"), None);
    }

    #[test]
    fn get_tool_timeout_ms_should_return_undefined_when_tool_ms_is_not_set() {
        let timeout = TimeoutConfiguration::detailed(
            TimeoutConfigurationOptions::new().with_total_ms(10_000),
        );

        assert_eq!(get_tool_timeout_ms(Some(&timeout), "testTool"), None);
    }

    #[test]
    fn get_tool_timeout_ms_should_return_tool_ms_when_set() {
        let timeout =
            TimeoutConfiguration::detailed(TimeoutConfigurationOptions::new().with_tool_ms(3_000));

        assert_eq!(get_tool_timeout_ms(Some(&timeout), "testTool"), Some(3_000));
    }

    #[test]
    fn get_tool_timeout_ms_should_return_tool_ms_alongside_other_timeout_values() {
        let timeout = TimeoutConfiguration::detailed(
            TimeoutConfigurationOptions::new()
                .with_total_ms(30_000)
                .with_step_ms(10_000)
                .with_tool_ms(5_000),
        );

        assert_eq!(get_tool_timeout_ms(Some(&timeout), "testTool"), Some(5_000));
    }

    #[test]
    fn get_total_timeout_ms_should_return_undefined_when_timeout_is_undefined() {
        assert_eq!(get_total_timeout_ms(None), None);
    }

    #[test]
    fn get_total_timeout_ms_should_return_the_number_directly_when_timeout_is_a_number() {
        let total = TimeoutConfiguration::total_ms(5_000);

        assert_eq!(get_total_timeout_ms(Some(&total)), Some(5_000));
    }

    #[test]
    fn get_total_timeout_ms_should_return_total_ms_from_an_object() {
        let timeout = TimeoutConfiguration::detailed(
            TimeoutConfigurationOptions::new().with_total_ms(10_000),
        );

        assert_eq!(get_total_timeout_ms(Some(&timeout)), Some(10_000));
    }

    #[test]
    fn get_total_timeout_ms_should_return_undefined_when_total_ms_is_not_set() {
        let timeout =
            TimeoutConfiguration::detailed(TimeoutConfigurationOptions::new().with_step_ms(5_000));

        assert_eq!(get_total_timeout_ms(Some(&timeout)), None);
    }

    #[test]
    fn get_step_timeout_ms_should_return_undefined_when_timeout_is_undefined() {
        assert_eq!(get_step_timeout_ms(None), None);
    }

    #[test]
    fn get_step_timeout_ms_should_return_undefined_when_timeout_is_a_number() {
        let total = TimeoutConfiguration::total_ms(5_000);

        assert_eq!(get_step_timeout_ms(Some(&total)), None);
    }

    #[test]
    fn get_step_timeout_ms_should_return_step_ms_from_an_object() {
        let timeout =
            TimeoutConfiguration::detailed(TimeoutConfigurationOptions::new().with_step_ms(3_000));

        assert_eq!(get_step_timeout_ms(Some(&timeout)), Some(3_000));
    }

    #[test]
    fn get_chunk_timeout_ms_should_return_undefined_when_timeout_is_undefined() {
        assert_eq!(get_chunk_timeout_ms(None), None);
    }

    #[test]
    fn get_chunk_timeout_ms_should_return_undefined_when_timeout_is_a_number() {
        let total = TimeoutConfiguration::total_ms(5_000);

        assert_eq!(get_chunk_timeout_ms(Some(&total)), None);
    }

    #[test]
    fn get_chunk_timeout_ms_should_return_chunk_ms_from_an_object() {
        let timeout =
            TimeoutConfiguration::detailed(TimeoutConfigurationOptions::new().with_chunk_ms(2_000));

        assert_eq!(get_chunk_timeout_ms(Some(&timeout)), Some(2_000));
    }

    #[test]
    fn get_tool_timeout_ms_should_prefer_tool_specific_timeout() {
        let timeout = TimeoutConfiguration::detailed(
            TimeoutConfigurationOptions::new()
                .with_tool_ms(5_000)
                .with_tool_timeout("search", 1_000),
        );

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
