//! Portable OpenTelemetry helpers for the Rust port of upstream `@ai-sdk/otel`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ai_sdk_provider::{
    FileData, FileDataContent, LanguageModelAssistantContentPart, LanguageModelMessage,
    LanguageModelPrompt, LanguageModelToolContentPart, LanguageModelToolResultOutput,
    LanguageModelUserContentPart,
};
use ai_sdk_provider_utils::convert_to_base64;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

/// The OTel crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Attribute values accepted by the Rust OTel helpers.
pub type TelemetryAttributeValue = JsonValue;

/// Deterministic attribute map used by the Rust OTel helpers.
pub type TelemetryAttributes = BTreeMap<String, TelemetryAttributeValue>;

/// Telemetry recording options mirrored from the AI SDK telemetry options used by
/// upstream `@ai-sdk/otel`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TelemetryOptions {
    /// Explicitly disables all telemetry when set to `Some(false)`.
    pub is_enabled: Option<bool>,

    /// Records input-bearing attributes unless this is `Some(false)`.
    pub record_inputs: Option<bool>,

    /// Records output-bearing attributes unless this is `Some(false)`.
    pub record_outputs: Option<bool>,

    /// Optional application function id added to operation attributes.
    pub function_id: Option<String>,
}

impl TelemetryOptions {
    /// Creates default-enabled telemetry options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets explicit telemetry enablement.
    pub fn with_enabled(mut self, is_enabled: bool) -> Self {
        self.is_enabled = Some(is_enabled);
        self
    }

    /// Sets whether input attributes are recorded.
    pub fn with_record_inputs(mut self, record_inputs: bool) -> Self {
        self.record_inputs = Some(record_inputs);
        self
    }

    /// Sets whether output attributes are recorded.
    pub fn with_record_outputs(mut self, record_outputs: bool) -> Self {
        self.record_outputs = Some(record_outputs);
        self
    }

    /// Sets the AI SDK function id.
    pub fn with_function_id(mut self, function_id: impl Into<String>) -> Self {
        self.function_id = Some(function_id.into());
        self
    }

    fn should_record(&self) -> bool {
        self.is_enabled != Some(false)
    }
}

/// Attribute selection spec mirroring upstream input/output gated attributes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttributeSpec {
    /// Plain attribute value.
    Value(TelemetryAttributeValue),

    /// Lazily/resolved input attribute value.
    Input(Option<TelemetryAttributeValue>),

    /// Lazily/resolved output attribute value.
    Output(Option<TelemetryAttributeValue>),

    /// Missing or intentionally omitted attribute.
    Omitted,
}

impl AttributeSpec {
    /// Creates a plain value attribute.
    pub fn value(value: impl Into<TelemetryAttributeValue>) -> Self {
        Self::Value(value.into())
    }

    /// Creates an input-gated attribute.
    pub fn input(value: impl Into<TelemetryAttributeValue>) -> Self {
        Self::Input(Some(value.into()))
    }

    /// Creates an output-gated attribute.
    pub fn output(value: impl Into<TelemetryAttributeValue>) -> Self {
        Self::Output(Some(value.into()))
    }
}

/// Selects attributes according to telemetry enablement and input/output flags.
pub fn select_attributes(
    telemetry: Option<&TelemetryOptions>,
    attributes: impl IntoIterator<Item = (impl Into<String>, AttributeSpec)>,
) -> TelemetryAttributes {
    if telemetry.is_some_and(|telemetry| !telemetry.should_record()) {
        return TelemetryAttributes::new();
    }

    let mut selected = TelemetryAttributes::new();
    for (key, value) in attributes {
        match value {
            AttributeSpec::Value(value) if !value.is_null() => {
                selected.insert(key.into(), value);
            }
            AttributeSpec::Input(Some(value))
                if telemetry.and_then(|telemetry| telemetry.record_inputs) != Some(false)
                    && !value.is_null() =>
            {
                selected.insert(key.into(), value);
            }
            AttributeSpec::Output(Some(value))
                if telemetry.and_then(|telemetry| telemetry.record_outputs) != Some(false)
                    && !value.is_null() =>
            {
                selected.insert(key.into(), value);
            }
            _ => {}
        }
    }
    selected
}

/// Async-free Rust equivalent of upstream `selectTelemetryAttributes`.
pub fn select_telemetry_attributes(
    telemetry: Option<&TelemetryOptions>,
    attributes: impl IntoIterator<Item = (impl Into<String>, AttributeSpec)>,
) -> TelemetryAttributes {
    select_attributes(telemetry, attributes)
}

/// Assembles standard operation/resource attributes for an AI SDK operation.
pub fn assemble_operation_name(
    operation_id: impl AsRef<str>,
    telemetry: Option<&TelemetryOptions>,
) -> TelemetryAttributes {
    let operation_id = operation_id.as_ref();
    let mut attributes = TelemetryAttributes::new();
    let function_id = telemetry.and_then(|telemetry| telemetry.function_id.as_deref());
    let operation_name = match function_id {
        Some(function_id) => format!("{operation_id} {function_id}"),
        None => operation_id.to_string(),
    };

    attributes.insert("operation.name".to_string(), json!(operation_name));
    attributes.insert("ai.operationId".to_string(), json!(operation_id));
    if let Some(function_id) = function_id {
        attributes.insert("resource.name".to_string(), json!(function_id));
        attributes.insert("ai.telemetry.functionId".to_string(), json!(function_id));
    }
    attributes
}

/// Maps an AI SDK provider id to an OTel GenAI semantic-convention provider.
pub fn map_provider_name(provider: &str) -> String {
    let lower = provider.to_ascii_lowercase();
    for (prefix, mapped) in [
        ("google.vertex", "gcp.vertex_ai"),
        ("google.generative-ai", "gcp.gemini"),
        ("google-vertex", "gcp.vertex_ai"),
        ("amazon-bedrock", "aws.bedrock"),
        ("azure-openai", "azure.ai.openai"),
        ("anthropic", "anthropic"),
        ("openai", "openai"),
        ("azure", "azure.ai.inference"),
        ("google", "gcp.gemini"),
        ("mistral", "mistral_ai"),
        ("cohere", "cohere"),
        ("bedrock", "aws.bedrock"),
        ("groq", "groq"),
        ("deepseek", "deepseek"),
        ("perplexity", "perplexity"),
        ("xai", "x_ai"),
    ] {
        if lower == prefix
            || lower
                .strip_prefix(prefix)
                .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with('-'))
        {
            return mapped.to_string();
        }
    }
    provider.to_string()
}

/// Maps an AI SDK operation id to an OTel GenAI semantic-convention operation.
pub fn map_operation_name(operation_id: &str) -> String {
    match operation_id {
        "ai.generateText" | "ai.streamText" | "ai.generateObject" | "ai.streamObject" => {
            "invoke_agent".to_string()
        }
        "ai.embed" | "ai.embedMany" => "embeddings".to_string(),
        "ai.rerank" => "rerank".to_string(),
        _ => operation_id.to_string(),
    }
}

/// System instruction in GenAI semantic-convention shape.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SemConvSystemInstruction {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Message in GenAI semantic-convention shape.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SemConvMessage {
    pub role: String,
    pub parts: Vec<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

impl SemConvMessage {
    fn input(role: impl Into<String>, parts: Vec<JsonValue>) -> Self {
        Self {
            role: role.into(),
            parts,
            finish_reason: None,
        }
    }

    fn output(parts: Vec<JsonValue>, finish_reason: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            parts,
            finish_reason: Some(finish_reason.into()),
        }
    }
}

/// Formats system instructions to GenAI semantic-convention shape.
pub fn format_system_instructions(system: impl AsRef<str>) -> Vec<SemConvSystemInstruction> {
    vec![SemConvSystemInstruction {
        kind: "text".to_string(),
        content: Some(system.as_ref().to_string()),
    }]
}

/// Extracts the first system instruction from a provider prompt.
pub fn extract_system_from_prompt(prompt: &[LanguageModelMessage]) -> Option<String> {
    prompt.iter().find_map(|message| match message {
        LanguageModelMessage::System(message) => Some(message.content.clone()),
        _ => None,
    })
}

/// Converts provider prompt messages to GenAI input-message shape.
pub fn format_input_messages(prompt: &[LanguageModelMessage]) -> Vec<SemConvMessage> {
    prompt.iter().filter_map(format_input_message).collect()
}

fn format_input_message(message: &LanguageModelMessage) -> Option<SemConvMessage> {
    match message {
        LanguageModelMessage::System(_) => None,
        LanguageModelMessage::User(message) => Some(SemConvMessage::input(
            "user",
            message
                .content
                .iter()
                .map(format_user_content_part)
                .collect(),
        )),
        LanguageModelMessage::Assistant(message) => Some(SemConvMessage::input(
            "assistant",
            message
                .content
                .iter()
                .map(format_assistant_content_part)
                .collect(),
        )),
        LanguageModelMessage::Tool(message) => Some(SemConvMessage::input(
            "tool",
            message
                .content
                .iter()
                .map(format_tool_content_part)
                .collect(),
        )),
    }
}

fn format_user_content_part(part: &LanguageModelUserContentPart) -> JsonValue {
    match part {
        LanguageModelUserContentPart::Text(part) => {
            json!({ "type": "text", "content": part.text })
        }
        LanguageModelUserContentPart::File(part) => format_file_part(&part.data, &part.media_type),
    }
}

fn format_assistant_content_part(part: &LanguageModelAssistantContentPart) -> JsonValue {
    match part {
        LanguageModelAssistantContentPart::Text(part) => {
            json!({ "type": "text", "content": part.text })
        }
        LanguageModelAssistantContentPart::File(part) => {
            format_file_part(&part.data, &part.media_type)
        }
        LanguageModelAssistantContentPart::Custom(part) => {
            json!({ "type": "custom", "kind": part.kind })
        }
        LanguageModelAssistantContentPart::Reasoning(part) => {
            json!({ "type": "reasoning", "content": part.text })
        }
        LanguageModelAssistantContentPart::ReasoningFile(_) => {
            json!({ "type": "reasoning-file" })
        }
        LanguageModelAssistantContentPart::ToolCall(part) => json!({
            "type": "tool_call",
            "id": part.tool_call_id,
            "name": part.tool_name,
            "arguments": part.input,
        }),
        LanguageModelAssistantContentPart::ToolResult(part) => json!({
            "type": "tool_call_response",
            "id": part.tool_call_id,
            "response": tool_result_response(&part.output),
        }),
        LanguageModelAssistantContentPart::ToolApprovalRequest(part) => json!({
            "type": "tool_approval_request",
            "approval_id": part.approval_id,
            "tool_call_id": part.tool_call_id,
            "is_automatic": part.is_automatic,
        }),
    }
}

fn format_tool_content_part(part: &LanguageModelToolContentPart) -> JsonValue {
    match part {
        LanguageModelToolContentPart::ToolResult(part) => json!({
            "type": "tool_call_response",
            "id": part.tool_call_id,
            "response": tool_result_response(&part.output),
        }),
        LanguageModelToolContentPart::ToolApprovalResponse(part) => json!({
            "type": "tool_approval_response",
            "approval_id": part.approval_id,
            "approved": part.approved,
            "reason": part.reason,
        }),
    }
}

fn tool_result_response(output: &LanguageModelToolResultOutput) -> JsonValue {
    match output {
        LanguageModelToolResultOutput::Text { value, .. }
        | LanguageModelToolResultOutput::ErrorText { value, .. } => json!(value),
        LanguageModelToolResultOutput::Json { value, .. }
        | LanguageModelToolResultOutput::ErrorJson { value, .. } => value.clone(),
        LanguageModelToolResultOutput::ExecutionDenied { reason, .. } => {
            json!({ "denied": true, "reason": reason })
        }
        LanguageModelToolResultOutput::Content { value } => json!(value),
    }
}

fn format_file_part(data: &FileData, media_type: &str) -> JsonValue {
    match data {
        FileData::Url { url } => json!({
            "type": "uri",
            "modality": modality(media_type),
            "mime_type": media_type,
            "uri": url.as_str(),
        }),
        FileData::Data { data } => json!({
            "type": "blob",
            "modality": modality(media_type),
            "mime_type": media_type,
            "content": convert_to_base64(data),
        }),
        FileData::Text { text } => json!({
            "type": "blob",
            "modality": modality(media_type),
            "mime_type": media_type,
            "content": text,
        }),
        FileData::Reference { reference } => json!({
            "type": "provider_reference",
            "modality": modality(media_type),
            "mime_type": media_type,
            "reference": reference,
        }),
    }
}

fn modality(media_type: &str) -> &'static str {
    if media_type.starts_with("video/") {
        "video"
    } else if media_type.starts_with("audio/") {
        "audio"
    } else {
        "image"
    }
}

/// Output reasoning item used by [`format_output_messages`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputReasoning {
    pub text: Option<String>,
}

impl OutputReasoning {
    /// Creates an output reasoning item.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
        }
    }
}

/// Output tool call item used by [`format_output_messages`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputToolCall {
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: JsonValue,
}

impl OutputToolCall {
    /// Creates an output tool call item.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: JsonValue,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
        }
    }
}

/// Output file item used by [`format_output_messages`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputFile {
    pub media_type: String,
    pub base64: String,
}

impl OutputFile {
    /// Creates an output file item.
    pub fn new(media_type: impl Into<String>, base64: impl Into<String>) -> Self {
        Self {
            media_type: media_type.into(),
            base64: base64.into(),
        }
    }
}

/// Input to [`format_output_messages`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutputMessages {
    pub text: Option<String>,
    pub reasoning: Vec<OutputReasoning>,
    pub tool_calls: Vec<OutputToolCall>,
    pub files: Vec<OutputFile>,
    pub finish_reason: String,
}

impl OutputMessages {
    /// Creates output-message formatting input.
    pub fn new(finish_reason: impl Into<String>) -> Self {
        Self {
            finish_reason: finish_reason.into(),
            ..Self::default()
        }
    }

    /// Adds text output.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Adds reasoning output.
    pub fn with_reasoning(mut self, reasoning: OutputReasoning) -> Self {
        self.reasoning.push(reasoning);
        self
    }

    /// Adds a tool call output.
    pub fn with_tool_call(mut self, tool_call: OutputToolCall) -> Self {
        self.tool_calls.push(tool_call);
        self
    }

    /// Adds a file output.
    pub fn with_file(mut self, file: OutputFile) -> Self {
        self.files.push(file);
        self
    }
}

/// Converts step output data to GenAI output-message shape.
pub fn format_output_messages(output: OutputMessages) -> Vec<SemConvMessage> {
    let mut parts = Vec::new();

    for reasoning in output.reasoning {
        if let Some(text) = reasoning.text
            && !text.is_empty()
        {
            parts.push(json!({ "type": "reasoning", "content": text }));
        }
    }

    if let Some(text) = output.text
        && !text.is_empty()
    {
        parts.push(json!({ "type": "text", "content": text }));
    }

    for tool_call in output.tool_calls {
        parts.push(json!({
            "type": "tool_call",
            "id": tool_call.tool_call_id,
            "name": tool_call.tool_name,
            "arguments": tool_call.input,
        }));
    }

    for file in output.files {
        parts.push(json!({
            "type": "blob",
            "modality": modality(&file.media_type),
            "mime_type": file.media_type,
            "content": file.base64,
        }));
    }

    vec![SemConvMessage::output(
        parts,
        map_finish_reason(&output.finish_reason),
    )]
}

/// Converts object-generation output to GenAI output-message shape.
pub fn format_object_output_messages(
    object_text: impl Into<String>,
    finish_reason: impl AsRef<str>,
) -> Vec<SemConvMessage> {
    vec![SemConvMessage::output(
        vec![json!({ "type": "text", "content": object_text.into() })],
        map_finish_reason(finish_reason.as_ref()),
    )]
}

fn map_finish_reason(reason: &str) -> String {
    match reason {
        "stop" => "stop",
        "length" => "length",
        "content-filter" => "content_filter",
        "tool-calls" => "tool_call",
        "error" => "error",
        "other" | "unknown" => "stop",
        reason => reason,
    }
    .to_string()
}

/// Base telemetry attributes for provider model calls.
pub fn get_base_telemetry_attributes(
    provider: impl AsRef<str>,
    model_id: impl AsRef<str>,
    settings: TelemetryAttributes,
    headers: Option<&BTreeMap<String, String>>,
    context: Option<&TelemetryAttributes>,
) -> TelemetryAttributes {
    let mut attributes = TelemetryAttributes::new();
    attributes.insert("ai.model.provider".to_string(), json!(provider.as_ref()));
    attributes.insert("ai.model.id".to_string(), json!(model_id.as_ref()));

    for (key, value) in settings {
        if !value.is_null() {
            attributes.insert(format!("ai.settings.{key}"), value);
        }
    }

    if let Some(context) = context {
        for (key, value) in context {
            if !value.is_null() {
                attributes.insert(format!("ai.settings.context.{key}"), value.clone());
            }
        }
    }

    if let Some(headers) = headers {
        for (key, value) in headers {
            attributes.insert(format!("ai.request.headers.{key}"), json!(value));
        }
    }

    attributes
}

/// Supplemental attribute enablement flags for the OTel package.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SupplementalAttributeOptions {
    pub usage: bool,
    pub provider_metadata: bool,
    pub embedding: bool,
    pub reranking: bool,
    pub runtime_context: bool,
    pub headers: bool,
    pub tool_choice: bool,
    pub schema: bool,
}

/// Selects enabled supplemental attributes.
pub fn select_supplemental_attributes(
    telemetry: Option<&TelemetryOptions>,
    enabled: SupplementalAttributeOptions,
    attributes: SupplementalAttributes,
) -> TelemetryAttributes {
    let mut selected = TelemetryAttributes::new();

    for (enabled, attributes) in [
        (enabled.usage, attributes.usage),
        (enabled.provider_metadata, attributes.provider_metadata),
        (enabled.embedding, attributes.embedding),
        (enabled.reranking, attributes.reranking),
        (enabled.runtime_context, attributes.runtime_context),
        (enabled.headers, attributes.headers),
        (enabled.tool_choice, attributes.tool_choice),
        (enabled.schema, attributes.schema),
    ] {
        if enabled {
            selected.extend(select_attributes(telemetry, attributes));
        }
    }

    selected
}

/// Grouped supplemental attributes keyed by upstream option family.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SupplementalAttributes {
    pub usage: Vec<(String, AttributeSpec)>,
    pub provider_metadata: Vec<(String, AttributeSpec)>,
    pub embedding: Vec<(String, AttributeSpec)>,
    pub reranking: Vec<(String, AttributeSpec)>,
    pub runtime_context: Vec<(String, AttributeSpec)>,
    pub headers: Vec<(String, AttributeSpec)>,
    pub tool_choice: Vec<(String, AttributeSpec)>,
    pub schema: Vec<(String, AttributeSpec)>,
}

/// Runtime-context attributes with upstream key prefixes.
pub fn get_runtime_context_attributes(
    context: &TelemetryAttributes,
) -> Vec<(String, AttributeSpec)> {
    context
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, value)| {
            (
                format!("ai.settings.context.{key}"),
                AttributeSpec::value(value.clone()),
            )
        })
        .collect()
}

/// Request-header attributes with upstream key prefixes.
pub fn get_header_attributes(headers: &BTreeMap<String, String>) -> Vec<(String, AttributeSpec)> {
    headers
        .iter()
        .map(|(key, value)| {
            (
                format!("ai.request.headers.{key}"),
                AttributeSpec::value(json!(value)),
            )
        })
        .collect()
}

/// Detailed usage attributes not represented by GenAI semantic conventions.
pub fn get_detailed_usage_attributes(usage: DetailedUsage) -> Vec<(String, AttributeSpec)> {
    vec![
        (
            "ai.usage.inputTokenDetails.noCacheTokens".to_string(),
            usage
                .input_no_cache_tokens
                .map_or(AttributeSpec::Omitted, |value| {
                    AttributeSpec::value(json!(value))
                }),
        ),
        (
            "ai.usage.outputTokenDetails.textTokens".to_string(),
            usage
                .output_text_tokens
                .map_or(AttributeSpec::Omitted, |value| {
                    AttributeSpec::value(json!(value))
                }),
        ),
        (
            "ai.usage.outputTokenDetails.reasoningTokens".to_string(),
            usage
                .output_reasoning_tokens
                .map_or(AttributeSpec::Omitted, |value| {
                    AttributeSpec::value(json!(value))
                }),
        ),
    ]
}

/// Detailed usage values used by [`get_detailed_usage_attributes`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DetailedUsage {
    pub input_no_cache_tokens: Option<u64>,
    pub output_text_tokens: Option<u64>,
    pub output_reasoning_tokens: Option<u64>,
}

/// Serializes a provider prompt for telemetry while converting inline bytes to
/// base64 strings, matching upstream `stringifyForTelemetry`.
pub fn stringify_for_telemetry(prompt: &LanguageModelPrompt) -> String {
    serde_json::to_string(
        &prompt
            .iter()
            .map(message_to_telemetry_json)
            .collect::<Vec<_>>(),
    )
    .expect("telemetry prompt serialization should not fail")
}

fn message_to_telemetry_json(message: &LanguageModelMessage) -> JsonValue {
    match message {
        LanguageModelMessage::System(message) => {
            json!({ "role": "system", "content": message.content })
        }
        LanguageModelMessage::User(message) => json!({
            "role": "user",
            "content": message.content.iter().map(user_part_to_telemetry_json).collect::<Vec<_>>(),
        }),
        LanguageModelMessage::Assistant(message) => json!({
            "role": "assistant",
            "content": message.content.iter().map(assistant_part_to_telemetry_json).collect::<Vec<_>>(),
        }),
        LanguageModelMessage::Tool(message) => json!({
            "role": "tool",
            "content": message.content.iter().map(tool_part_to_telemetry_json).collect::<Vec<_>>(),
        }),
    }
}

fn user_part_to_telemetry_json(part: &LanguageModelUserContentPart) -> JsonValue {
    match part {
        LanguageModelUserContentPart::Text(part) => json!({
            "type": "text",
            "text": part.text,
        }),
        LanguageModelUserContentPart::File(part) => file_part_to_telemetry_json(
            part.filename.as_deref(),
            &part.data,
            &part.media_type,
            part.provider_options.as_ref().map(|options| json!(options)),
        ),
    }
}

fn assistant_part_to_telemetry_json(part: &LanguageModelAssistantContentPart) -> JsonValue {
    match part {
        LanguageModelAssistantContentPart::Text(part) => json!({
            "type": "text",
            "text": part.text,
        }),
        LanguageModelAssistantContentPart::File(part) => file_part_to_telemetry_json(
            part.filename.as_deref(),
            &part.data,
            &part.media_type,
            part.provider_options.as_ref().map(|options| json!(options)),
        ),
        _ => serde_json::to_value(part).expect("telemetry part serialization should not fail"),
    }
}

fn tool_part_to_telemetry_json(part: &LanguageModelToolContentPart) -> JsonValue {
    serde_json::to_value(part).expect("telemetry part serialization should not fail")
}

fn file_part_to_telemetry_json(
    filename: Option<&str>,
    data: &FileData,
    media_type: &str,
    provider_options: Option<JsonValue>,
) -> JsonValue {
    let mut value = serde_json::Map::new();
    value.insert("type".to_string(), json!("file"));
    if let Some(filename) = filename {
        value.insert("filename".to_string(), json!(filename));
    }
    value.insert("data".to_string(), telemetry_file_data(data));
    value.insert("mediaType".to_string(), json!(media_type));
    if let Some(provider_options) = provider_options {
        value.insert("providerOptions".to_string(), provider_options);
    }
    JsonValue::Object(value)
}

fn telemetry_file_data(data: &FileData) -> JsonValue {
    match data {
        FileData::Data { data } => match data {
            FileDataContent::Bytes(_) | FileDataContent::Base64(_) => {
                json!(convert_to_base64(data))
            }
        },
        FileData::Url { url } => json!(url.as_str()),
        FileData::Reference { reference } => json!(reference),
        FileData::Text { text } => json!(text),
    }
}

/// Span status code used by the dependency-free test tracer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SpanStatusCode {
    /// The span has no explicit status.
    Unset,

    /// The span completed successfully.
    Ok,

    /// The span recorded an error.
    Error,
}

/// Span status used by the dependency-free test tracer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SpanStatus {
    pub code: SpanStatusCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl SpanStatus {
    /// Creates an error status.
    pub fn error(message: Option<String>) -> Self {
        Self {
            code: SpanStatusCode::Error,
            message,
        }
    }
}

/// Span context used by the dependency-free test tracer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SpanContext {
    pub trace_id: String,
    pub span_id: String,
    pub trace_flags: u8,
}

impl Default for SpanContext {
    fn default() -> Self {
        Self {
            trace_id: "test-trace-id".to_string(),
            span_id: "test-span-id".to_string(),
            trace_flags: 0,
        }
    }
}

/// Exception event recorded on a span.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SpanException {
    pub name: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
}

impl SpanException {
    /// Creates an exception event.
    pub fn new(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message: message.into(),
            stack: None,
        }
    }

    /// Adds a stack string.
    pub fn with_stack(mut self, stack: impl Into<String>) -> Self {
        self.stack = Some(stack.into());
        self
    }
}

/// Error input for [`record_error_on_span`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RecordSpanError {
    /// Error-like value with exception details.
    Exception(SpanException),

    /// Non-error thrown value. Upstream only sets error status for this case.
    StatusOnly,
}

impl RecordSpanError {
    /// Creates an exception-style span error.
    pub fn exception(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Exception(SpanException::new(name, message))
    }

    /// Returns the status message, if any.
    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Exception(exception) => Some(&exception.message),
            Self::StatusOnly => None,
        }
    }
}

/// Event recorded by the dependency-free test span.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SpanEvent {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<TelemetryAttributes>,
}

/// Span used by [`MockTracer`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MockSpan {
    pub name: String,
    pub attributes: TelemetryAttributes,
    pub events: Vec<SpanEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<SpanStatus>,
    pub ended: bool,
    pub context: SpanContext,
}

impl MockSpan {
    /// Creates a mock span.
    pub fn new(name: impl Into<String>, attributes: TelemetryAttributes) -> Self {
        Self {
            name: name.into(),
            attributes,
            events: Vec::new(),
            status: None,
            ended: false,
            context: SpanContext::default(),
        }
    }

    /// Returns the span context.
    pub fn span_context(&self) -> &SpanContext {
        &self.context
    }

    /// Sets a single span attribute.
    pub fn set_attribute(
        &mut self,
        key: impl Into<String>,
        value: impl Into<TelemetryAttributeValue>,
    ) -> &mut Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Sets multiple span attributes.
    pub fn set_attributes(&mut self, attributes: TelemetryAttributes) -> &mut Self {
        self.attributes.extend(attributes);
        self
    }

    /// Adds a span event.
    pub fn add_event(
        &mut self,
        name: impl Into<String>,
        attributes: Option<TelemetryAttributes>,
    ) -> &mut Self {
        self.events.push(SpanEvent {
            name: name.into(),
            attributes,
        });
        self
    }

    /// Sets the span status.
    pub fn set_status(&mut self, status: SpanStatus) -> &mut Self {
        self.status = Some(status);
        self
    }

    /// Renames the span.
    pub fn update_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.name = name.into();
        self
    }

    /// Ends the span.
    pub fn end(&mut self) -> &mut Self {
        self.ended = true;
        self
    }

    /// Returns whether this span is recording.
    pub fn is_recording(&self) -> bool {
        true
    }

    /// Records an exception event.
    pub fn record_exception(&mut self, exception: SpanException) -> &mut Self {
        let mut attributes = TelemetryAttributes::new();
        attributes.insert("exception.type".to_string(), json!(exception.name));
        attributes.insert("exception.name".to_string(), json!(exception.name));
        attributes.insert("exception.message".to_string(), json!(exception.message));
        if let Some(stack) = exception.stack {
            attributes.insert("exception.stack".to_string(), json!(stack));
        }
        self.add_event("exception", Some(attributes))
    }
}

/// Dependency-free tracer for deterministic OTel tests.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MockTracer {
    pub spans: Vec<MockSpan>,
}

impl MockTracer {
    /// Creates an empty mock tracer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a span and returns its index.
    pub fn start_span(
        &mut self,
        name: impl Into<String>,
        attributes: TelemetryAttributes,
    ) -> usize {
        self.spans.push(MockSpan::new(name, attributes));
        self.spans.len() - 1
    }

    /// Starts an active span and executes a closure with it.
    pub fn start_active_span<T>(
        &mut self,
        name: impl Into<String>,
        attributes: TelemetryAttributes,
        execute: impl FnOnce(&mut MockSpan) -> T,
    ) -> T {
        let index = self.start_span(name, attributes);
        execute(&mut self.spans[index])
    }

    /// Returns JSON-serializable span summaries.
    pub fn json_spans(&self) -> Vec<JsonValue> {
        self.spans
            .iter()
            .map(|span| {
                let mut value = serde_json::Map::new();
                value.insert("name".to_string(), json!(span.name));
                value.insert("attributes".to_string(), json!(span.attributes));
                value.insert("events".to_string(), json!(span.events));
                if let Some(status) = &span.status {
                    value.insert("status".to_string(), json!(status));
                }
                JsonValue::Object(value)
            })
            .collect()
    }
}

/// Tracer implementation that records nothing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoopTracer;

impl NoopTracer {
    /// Executes a closure with a throwaway span.
    pub fn start_active_span<T>(
        self,
        name: impl Into<String>,
        attributes: TelemetryAttributes,
        execute: impl FnOnce(&mut MockSpan) -> T,
    ) -> T {
        let mut span = MockSpan::new(name, attributes);
        execute(&mut span)
    }
}

/// Records an error on a span, matching upstream `recordErrorOnSpan`.
pub fn record_error_on_span(span: &mut MockSpan, error: RecordSpanError) {
    match error {
        RecordSpanError::Exception(exception) => {
            let message = exception.message.clone();
            span.record_exception(exception);
            span.set_status(SpanStatus::error(Some(message)));
        }
        RecordSpanError::StatusOnly => {
            span.set_status(SpanStatus::error(None));
        }
    }
}

/// Starts an active span, runs a closure, records errors, and optionally ends
/// the span. This is the dependency-free Rust analogue of upstream `recordSpan`.
pub fn record_span<T>(
    tracer: &mut MockTracer,
    name: impl Into<String>,
    attributes: TelemetryAttributes,
    end_when_done: bool,
    execute: impl FnOnce(&mut MockSpan) -> Result<T, RecordSpanError>,
) -> Result<T, RecordSpanError> {
    let index = tracer.start_span(name, attributes);
    let result = execute(&mut tracer.spans[index]);

    match result {
        Ok(value) => {
            if end_when_done {
                tracer.spans[index].end();
            }
            Ok(value)
        }
        Err(error) => {
            record_error_on_span(&mut tracer.spans[index], error.clone());
            tracer.spans[index].end();
            Err(error)
        }
    }
}

/// OTLP/HTTP JSON export options for locally validating recorded spans.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OtlpHttpTraceExportOptions {
    pub endpoint: String,
    pub service_name: String,
    pub scope_name: String,
    pub scope_version: String,
    pub resource_attributes: TelemetryAttributes,
}

impl OtlpHttpTraceExportOptions {
    /// Creates export options for an OTLP/HTTP `/v1/traces` endpoint.
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            service_name: "ai-sdk-rust".to_string(),
            scope_name: "ai-sdk-otel".to_string(),
            scope_version: VERSION.to_string(),
            resource_attributes: TelemetryAttributes::new(),
        }
    }

    /// Sets the OTLP resource `service.name`.
    pub fn with_service_name(mut self, service_name: impl Into<String>) -> Self {
        self.service_name = service_name.into();
        self
    }

    /// Sets the OTLP instrumentation scope name.
    pub fn with_scope_name(mut self, scope_name: impl Into<String>) -> Self {
        self.scope_name = scope_name.into();
        self
    }

    /// Sets the OTLP instrumentation scope version.
    pub fn with_scope_version(mut self, scope_version: impl Into<String>) -> Self {
        self.scope_version = scope_version.into();
        self
    }

    /// Adds a resource attribute to the OTLP export payload.
    pub fn with_resource_attribute(
        mut self,
        key: impl Into<String>,
        value: impl Into<TelemetryAttributeValue>,
    ) -> Self {
        self.resource_attributes.insert(key.into(), value.into());
        self
    }
}

/// Error produced while exporting spans over OTLP/HTTP JSON.
#[derive(Debug)]
pub enum OtlpHttpTraceExportError {
    UnsupportedEndpoint(String),
    Io(io::Error),
    Serialize(serde_json::Error),
    ResponseStatus {
        status: u16,
        status_line: String,
        body: String,
    },
}

impl fmt::Display for OtlpHttpTraceExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedEndpoint(endpoint) => {
                write!(formatter, "unsupported OTLP HTTP endpoint: {endpoint}")
            }
            Self::Io(error) => write!(formatter, "OTLP HTTP export failed: {error}"),
            Self::Serialize(error) => write!(formatter, "OTLP JSON serialization failed: {error}"),
            Self::ResponseStatus {
                status,
                status_line,
                body,
            } => write!(
                formatter,
                "OTLP HTTP export returned status {status}: {status_line}; body: {body}"
            ),
        }
    }
}

impl std::error::Error for OtlpHttpTraceExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Serialize(error) => Some(error),
            Self::UnsupportedEndpoint(_) | Self::ResponseStatus { .. } => None,
        }
    }
}

impl From<io::Error> for OtlpHttpTraceExportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for OtlpHttpTraceExportError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialize(error)
    }
}

/// Builds an OTLP/HTTP JSON trace export payload from recorded mock spans.
pub fn build_otlp_http_trace_json(
    tracer: &MockTracer,
    options: &OtlpHttpTraceExportOptions,
) -> JsonValue {
    let mut resource_attributes =
        TelemetryAttributes::from([("service.name".to_string(), json!(options.service_name))]);
    resource_attributes.extend(options.resource_attributes.clone());

    json!({
        "resourceSpans": [
            {
                "resource": {
                    "attributes": otlp_key_values(&resource_attributes),
                },
                "scopeSpans": [
                    {
                        "scope": {
                            "name": options.scope_name,
                            "version": options.scope_version,
                        },
                        "spans": tracer
                            .spans
                            .iter()
                            .enumerate()
                            .map(|(index, span)| otlp_span(index, span))
                            .collect::<Vec<_>>(),
                    }
                ]
            }
        ]
    })
}

/// Exports recorded mock spans to an OTLP/HTTP JSON endpoint.
///
/// This intentionally supports plain `http://` endpoints so local receivers and
/// local OpenTelemetry Collector instances can be used in CI without TLS setup.
pub fn export_tracer_to_otlp_http_json(
    tracer: &MockTracer,
    options: &OtlpHttpTraceExportOptions,
) -> Result<(), OtlpHttpTraceExportError> {
    let endpoint = parse_otlp_http_endpoint(&options.endpoint)?;
    let body = serde_json::to_vec(&build_otlp_http_trace_json(tracer, options))?;
    let host_header = match endpoint.port {
        80 => endpoint.host.clone(),
        port => format!("{}:{port}", endpoint.host),
    };
    let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    write!(
        stream,
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        endpoint.path,
        host_header,
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()?;

    let response = read_http_response(&mut stream)?;
    if !(200..300).contains(&response.status) {
        return Err(OtlpHttpTraceExportError::ResponseStatus {
            status: response.status,
            status_line: response.status_line,
            body: response.body,
        });
    }

    Ok(())
}

/// Emits one span through the real Rust OpenTelemetry OTLP/HTTP exporter.
///
/// This is intentionally separate from the dependency-free mock tracer export
/// path above. It verifies that the actual `opentelemetry` SDK/exporter can send
/// an OTLP/HTTP JSON payload to the same local receiver used by CI.
#[cfg(feature = "real-opentelemetry")]
pub fn export_real_opentelemetry_span_to_otlp_http_json(
    options: &OtlpHttpTraceExportOptions,
    span_name: impl Into<String>,
    attributes: TelemetryAttributes,
) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    use opentelemetry::trace::{Span as _, Tracer as _, TracerProvider as _};
    use opentelemetry::{InstrumentationScope, KeyValue};
    use opentelemetry_otlp::{Protocol, WithExportConfig};
    use opentelemetry_sdk::{Resource, trace::SdkTracerProvider};

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(options.endpoint.clone())
        .with_protocol(Protocol::HttpJson)
        .with_timeout(Duration::from_secs(5))
        .build()?;
    let resource = Resource::builder()
        .with_service_name(options.service_name.clone())
        .with_attributes(
            options
                .resource_attributes
                .clone()
                .into_iter()
                .map(real_opentelemetry_key_value),
        )
        .build();
    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_simple_exporter(exporter)
        .build();
    let scope = InstrumentationScope::builder(options.scope_name.clone())
        .with_version(options.scope_version.clone())
        .build();
    let tracer = provider.tracer_with_scope(scope);
    let mut span = tracer.start(span_name.into());
    span.set_attributes(
        attributes
            .into_iter()
            .map(|(key, value)| KeyValue::new(key, real_opentelemetry_value(value))),
    );
    span.end();
    provider.force_flush()?;
    provider.shutdown()?;

    Ok(())
}

#[cfg(feature = "real-opentelemetry")]
fn real_opentelemetry_key_value((key, value): (String, JsonValue)) -> opentelemetry::KeyValue {
    opentelemetry::KeyValue::new(key, real_opentelemetry_value(value))
}

#[cfg(feature = "real-opentelemetry")]
fn real_opentelemetry_value(value: JsonValue) -> opentelemetry::Value {
    match value {
        JsonValue::Bool(value) => value.into(),
        JsonValue::Number(value) => value
            .as_i64()
            .map(opentelemetry::Value::from)
            .or_else(|| value.as_f64().map(opentelemetry::Value::from))
            .unwrap_or_else(|| opentelemetry::Value::from(value.to_string())),
        JsonValue::String(value) => value.into(),
        JsonValue::Array(_) | JsonValue::Object(_) => opentelemetry::Value::from(value.to_string()),
        JsonValue::Null => "null".into(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OtlpHttpEndpoint {
    host: String,
    port: u16,
    path: String,
}

fn parse_otlp_http_endpoint(endpoint: &str) -> Result<OtlpHttpEndpoint, OtlpHttpTraceExportError> {
    let Some(rest) = endpoint.strip_prefix("http://") else {
        return Err(OtlpHttpTraceExportError::UnsupportedEndpoint(
            endpoint.to_string(),
        ));
    };
    let (authority, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_string()), |(authority, path)| {
            (authority, format!("/{path}"))
        });
    if authority.is_empty() {
        return Err(OtlpHttpTraceExportError::UnsupportedEndpoint(
            endpoint.to_string(),
        ));
    }
    let (host, port) = authority.split_once(':').map_or_else(
        || Ok::<_, OtlpHttpTraceExportError>((authority.to_string(), 80)),
        |(host, port)| {
            let port = port
                .parse::<u16>()
                .map_err(|_| OtlpHttpTraceExportError::UnsupportedEndpoint(endpoint.to_string()))?;
            Ok((host.to_string(), port))
        },
    )?;
    if host.is_empty() {
        return Err(OtlpHttpTraceExportError::UnsupportedEndpoint(
            endpoint.to_string(),
        ));
    }
    Ok(OtlpHttpEndpoint { host, port, path })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OtlpHttpResponse {
    status: u16,
    status_line: String,
    body: String,
}

fn read_http_response(
    stream: &mut TcpStream,
) -> Result<OtlpHttpResponse, OtlpHttpTraceExportError> {
    let mut response = String::new();
    match stream.read_to_string(&mut response) {
        Ok(_) => {}
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) && !response.is_empty() => {}
        Err(error) => return Err(error.into()),
    }

    let (head, body) = response
        .split_once("\r\n\r\n")
        .map_or((response.as_str(), ""), |(head, body)| (head, body));
    let status_line = head.lines().next().unwrap_or_default().to_string();
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| {
            OtlpHttpTraceExportError::UnsupportedEndpoint("invalid HTTP response".to_string())
        })?;
    Ok(OtlpHttpResponse {
        status,
        status_line,
        body: body.to_string(),
    })
}

fn otlp_span(index: usize, span: &MockSpan) -> JsonValue {
    let start_time = ((index as u64) + 1) * 1_000;
    let end_time = if span.ended {
        start_time + 1
    } else {
        start_time
    };
    let mut value = serde_json::Map::new();
    value.insert(
        "traceId".to_string(),
        json!("00000000000000000000000000000001"),
    );
    value.insert(
        "spanId".to_string(),
        json!(format!("{:016x}", (index as u64) + 1)),
    );
    value.insert("name".to_string(), json!(span.name));
    value.insert("kind".to_string(), json!("SPAN_KIND_INTERNAL"));
    value.insert(
        "startTimeUnixNano".to_string(),
        json!(start_time.to_string()),
    );
    value.insert("endTimeUnixNano".to_string(), json!(end_time.to_string()));
    value.insert(
        "attributes".to_string(),
        json!(otlp_key_values(&span.attributes)),
    );
    if !span.events.is_empty() {
        value.insert(
            "events".to_string(),
            json!(
                span.events
                    .iter()
                    .map(otlp_event)
                    .collect::<Vec<JsonValue>>()
            ),
        );
    }
    if let Some(status) = &span.status {
        value.insert("status".to_string(), otlp_status(status));
    }
    JsonValue::Object(value)
}

fn otlp_event(event: &SpanEvent) -> JsonValue {
    json!({
        "timeUnixNano": "1",
        "name": event.name,
        "attributes": otlp_key_values(&event.attributes.clone().unwrap_or_default()),
    })
}

fn otlp_status(status: &SpanStatus) -> JsonValue {
    match status.code {
        SpanStatusCode::Unset => json!({ "code": "STATUS_CODE_UNSET" }),
        SpanStatusCode::Ok => json!({ "code": "STATUS_CODE_OK" }),
        SpanStatusCode::Error => {
            let mut value = serde_json::Map::new();
            value.insert("code".to_string(), json!("STATUS_CODE_ERROR"));
            if let Some(message) = &status.message {
                value.insert("message".to_string(), json!(message));
            }
            JsonValue::Object(value)
        }
    }
}

fn otlp_key_values(attributes: &TelemetryAttributes) -> Vec<JsonValue> {
    attributes
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, value)| {
            json!({
                "key": key,
                "value": otlp_any_value(value),
            })
        })
        .collect()
}

fn otlp_any_value(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Null => json!({ "stringValue": "null" }),
        JsonValue::Bool(value) => json!({ "boolValue": value }),
        JsonValue::Number(value) => {
            if let Some(value) = value.as_i64() {
                json!({ "intValue": value.to_string() })
            } else if let Some(value) = value.as_u64() {
                json!({ "intValue": value.to_string() })
            } else {
                json!({ "doubleValue": value.as_f64().unwrap_or_default() })
            }
        }
        JsonValue::String(value) => json!({ "stringValue": value }),
        JsonValue::Array(values) => json!({
            "arrayValue": {
                "values": values.iter().map(otlp_any_value).collect::<Vec<_>>(),
            },
        }),
        JsonValue::Object(values) => json!({
            "kvlistValue": {
                "values": values
                    .iter()
                    .filter(|(_, value)| !value.is_null())
                    .map(|(key, value)| json!({
                        "key": key,
                        "value": otlp_any_value(value),
                    }))
                    .collect::<Vec<_>>(),
            },
        }),
    }
}

/// HTTP request captured by [`LocalOtlpTraceReceiver`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OtlpHttpTraceRequest {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

impl OtlpHttpTraceRequest {
    /// Parses the captured body as JSON.
    pub fn body_json(&self) -> Option<JsonValue> {
        serde_json::from_str(&self.body).ok()
    }
}

/// Loopback OTLP/HTTP trace receiver for local end-to-end validation.
pub struct LocalOtlpTraceReceiver {
    endpoint: String,
    requests: Arc<Mutex<Vec<OtlpHttpTraceRequest>>>,
    shutdown: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl fmt::Debug for LocalOtlpTraceReceiver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalOtlpTraceReceiver")
            .field("endpoint", &self.endpoint)
            .field("requests", &self.received_requests())
            .finish_non_exhaustive()
    }
}

impl LocalOtlpTraceReceiver {
    /// Starts a loopback receiver on `127.0.0.1` with an ephemeral port.
    pub fn start() -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let endpoint = format!("http://{address}/v1/traces");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let (shutdown, stop) = mpsc::channel();
        let thread = thread::spawn(move || {
            loop {
                if stop.try_recv().is_ok() {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = capture_otlp_http_request(stream, &server_requests);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            endpoint,
            requests,
            shutdown: Some(shutdown),
            thread: Some(thread),
        })
    }

    /// Returns the receiver's OTLP/HTTP `/v1/traces` endpoint.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Returns all captured requests.
    pub fn received_requests(&self) -> Vec<OtlpHttpTraceRequest> {
        self.requests
            .lock()
            .expect("OTLP receiver request lock is not poisoned")
            .clone()
    }

    /// Waits until the expected number of requests is captured or timeout elapses.
    pub fn wait_for_requests(
        &self,
        expected_count: usize,
        timeout: Duration,
    ) -> Vec<OtlpHttpTraceRequest> {
        let deadline = Instant::now() + timeout;
        loop {
            let requests = self.received_requests();
            if requests.len() >= expected_count || Instant::now() >= deadline {
                return requests;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for LocalOtlpTraceReceiver {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn capture_otlp_http_request(
    mut stream: TcpStream,
    requests: &Arc<Mutex<Vec<OtlpHttpTraceRequest>>>,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut buffer = Vec::new();
    let mut chunk = [0; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(header_end) = find_http_header_end(&buffer) {
            break header_end;
        }
    };

    let headers_text = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = headers_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_string();
    let path = request_parts.next().unwrap_or_default().to_string();
    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    let body_start = header_end + 4;
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    while buffer.len().saturating_sub(body_start) < content_length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body_end = body_start + content_length.min(buffer.len().saturating_sub(body_start));
    let body = String::from_utf8_lossy(&buffer[body_start..body_end]).to_string();
    requests
        .lock()
        .expect("OTLP receiver request lock is not poisoned")
        .push(OtlpHttpTraceRequest {
            method,
            path,
            headers,
            body,
        });

    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
    )?;
    stream.flush()
}

fn find_http_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

/// Span families emitted by the Rust OpenTelemetry recorder.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum OpenTelemetrySpanType {
    /// Root operation span.
    Operation,

    /// Step span.
    Step,

    /// Language model call span.
    LanguageModel,

    /// Tool execution span.
    Tool,

    /// Embedding model call span.
    Embedding,

    /// Reranking model call span.
    Reranking,
}

impl OpenTelemetrySpanType {
    /// Returns the upstream span type string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Operation => "operation",
            Self::Step => "step",
            Self::LanguageModel => "languageModel",
            Self::Tool => "tool",
            Self::Embedding => "embedding",
            Self::Reranking => "reranking",
        }
    }
}

/// Options passed to `enrichSpan`.
#[derive(Clone, Debug, PartialEq)]
pub struct EnrichSpanOptions {
    pub span_type: OpenTelemetrySpanType,
    pub operation_id: String,
    pub call_id: String,
    pub runtime_context: Option<TelemetryAttributes>,
}

/// Function that adds custom attributes when spans are created.
pub type EnrichSpan = Arc<dyn Fn(EnrichSpanOptions) -> TelemetryAttributes + Send + Sync>;

/// Options for the Rust OpenTelemetry recorder.
#[derive(Clone, Default)]
pub struct OpenTelemetryOptions {
    pub supplemental_attributes: SupplementalAttributeOptions,
    pub enrich_span: Option<EnrichSpan>,
}

impl OpenTelemetryOptions {
    /// Creates default OTel options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables supplemental attributes.
    pub fn with_supplemental_attributes(
        mut self,
        supplemental_attributes: SupplementalAttributeOptions,
    ) -> Self {
        self.supplemental_attributes = supplemental_attributes;
        self
    }

    /// Adds a span enrichment function.
    pub fn with_enrich_span(
        mut self,
        enrich_span: impl Fn(EnrichSpanOptions) -> TelemetryAttributes + Send + Sync + 'static,
    ) -> Self {
        self.enrich_span = Some(Arc::new(enrich_span));
        self
    }
}

#[derive(Clone, Debug)]
struct OpenTelemetryCallState {
    operation_id: String,
    telemetry: TelemetryOptions,
    provider: String,
    model_id: String,
    root_span: usize,
    step_span: Option<usize>,
    inference_span: Option<usize>,
    embed_spans: BTreeMap<String, usize>,
    rerank_span: Option<usize>,
    tool_spans: BTreeMap<String, usize>,
    runtime_context: Option<TelemetryAttributes>,
}

/// Start event accepted by [`OpenTelemetry::on_start`].
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryStartEvent {
    pub call_id: String,
    pub operation_id: String,
    pub provider: String,
    pub model_id: String,
    pub telemetry: TelemetryOptions,
    pub settings: TelemetryAttributes,
    pub runtime_context: Option<TelemetryAttributes>,
    pub system_instructions: Option<Vec<SemConvSystemInstruction>>,
    pub input_messages: Option<Vec<SemConvMessage>>,
}

impl OpenTelemetryStartEvent {
    /// Creates a root operation start event.
    pub fn new(
        call_id: impl Into<String>,
        operation_id: impl Into<String>,
        provider: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            operation_id: operation_id.into(),
            provider: provider.into(),
            model_id: model_id.into(),
            telemetry: TelemetryOptions::new(),
            settings: TelemetryAttributes::new(),
            runtime_context: None,
            system_instructions: None,
            input_messages: None,
        }
    }

    /// Sets telemetry recording options.
    pub fn with_telemetry(mut self, telemetry: TelemetryOptions) -> Self {
        self.telemetry = telemetry;
        self
    }

    /// Sets model call settings.
    pub fn with_settings(mut self, settings: TelemetryAttributes) -> Self {
        self.settings = settings;
        self
    }

    /// Sets runtime context values.
    pub fn with_runtime_context(mut self, runtime_context: TelemetryAttributes) -> Self {
        self.runtime_context = Some(runtime_context);
        self
    }

    /// Sets formatted system instructions.
    pub fn with_system_instructions(
        mut self,
        system_instructions: Vec<SemConvSystemInstruction>,
    ) -> Self {
        self.system_instructions = Some(system_instructions);
        self
    }

    /// Sets formatted input messages.
    pub fn with_input_messages(mut self, input_messages: Vec<SemConvMessage>) -> Self {
        self.input_messages = Some(input_messages);
        self
    }
}

/// Object-generation operation start event.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryObjectStartEvent {
    pub call_id: String,
    pub operation_id: String,
    pub provider: String,
    pub model_id: String,
    pub telemetry: TelemetryOptions,
    pub settings: TelemetryAttributes,
    pub system_instructions: Option<Vec<SemConvSystemInstruction>>,
    pub input_messages: Option<Vec<SemConvMessage>>,
    pub schema: Option<JsonValue>,
    pub schema_name: Option<String>,
    pub schema_description: Option<String>,
    pub output_mode: Option<String>,
}

impl OpenTelemetryObjectStartEvent {
    /// Creates an object-generation operation start event.
    pub fn new(
        call_id: impl Into<String>,
        operation_id: impl Into<String>,
        provider: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            operation_id: operation_id.into(),
            provider: provider.into(),
            model_id: model_id.into(),
            telemetry: TelemetryOptions::new(),
            settings: TelemetryAttributes::new(),
            system_instructions: None,
            input_messages: None,
            schema: None,
            schema_name: None,
            schema_description: None,
            output_mode: None,
        }
    }

    /// Sets telemetry recording options.
    pub fn with_telemetry(mut self, telemetry: TelemetryOptions) -> Self {
        self.telemetry = telemetry;
        self
    }

    /// Sets model call settings.
    pub fn with_settings(mut self, settings: TelemetryAttributes) -> Self {
        self.settings = settings;
        self
    }

    /// Sets formatted system instructions.
    pub fn with_system_instructions(
        mut self,
        system_instructions: Vec<SemConvSystemInstruction>,
    ) -> Self {
        self.system_instructions = Some(system_instructions);
        self
    }

    /// Sets formatted input messages.
    pub fn with_input_messages(mut self, input_messages: Vec<SemConvMessage>) -> Self {
        self.input_messages = Some(input_messages);
        self
    }

    /// Sets object schema supplemental data.
    pub fn with_schema(
        mut self,
        schema: JsonValue,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.schema = Some(schema);
        self.schema_name = Some(name.into());
        self.schema_description = Some(description.into());
        self
    }

    /// Sets the object output mode supplemental data.
    pub fn with_output_mode(mut self, output_mode: impl Into<String>) -> Self {
        self.output_mode = Some(output_mode.into());
        self
    }
}

/// Object-generation model call start event.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryObjectStepStartEvent {
    pub call_id: String,
    pub provider: String,
    pub model_id: String,
    pub settings: TelemetryAttributes,
    pub input_messages: Option<Vec<SemConvMessage>>,
}

impl OpenTelemetryObjectStepStartEvent {
    /// Creates an object step start event.
    pub fn new(
        call_id: impl Into<String>,
        provider: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            provider: provider.into(),
            model_id: model_id.into(),
            settings: TelemetryAttributes::new(),
            input_messages: None,
        }
    }

    /// Sets model call settings.
    pub fn with_settings(mut self, settings: TelemetryAttributes) -> Self {
        self.settings = settings;
        self
    }

    /// Sets formatted input messages.
    pub fn with_input_messages(mut self, input_messages: Vec<SemConvMessage>) -> Self {
        self.input_messages = Some(input_messages);
        self
    }
}

/// Object-generation model call finish event.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryObjectStepFinishEvent {
    pub call_id: String,
    pub finish_reason: String,
    pub usage: Option<TelemetryTokenUsage>,
    pub object_text: Option<String>,
}

impl OpenTelemetryObjectStepFinishEvent {
    /// Creates an object step finish event.
    pub fn new(call_id: impl Into<String>, finish_reason: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            finish_reason: finish_reason.into(),
            usage: None,
            object_text: None,
        }
    }

    /// Sets usage values.
    pub fn with_usage(mut self, usage: TelemetryTokenUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Sets object text output.
    pub fn with_object_text(mut self, object_text: impl Into<String>) -> Self {
        self.object_text = Some(object_text.into());
        self
    }
}

/// Object-generation operation end event.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryObjectEndEvent {
    pub call_id: String,
    pub finish_reason: String,
    pub usage: Option<TelemetryTokenUsage>,
    pub object: Option<JsonValue>,
}

impl OpenTelemetryObjectEndEvent {
    /// Creates an object-generation operation end event.
    pub fn new(call_id: impl Into<String>, finish_reason: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            finish_reason: finish_reason.into(),
            usage: None,
            object: None,
        }
    }

    /// Sets usage values.
    pub fn with_usage(mut self, usage: TelemetryTokenUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Sets the generated object.
    pub fn with_object(mut self, object: JsonValue) -> Self {
        self.object = Some(object);
        self
    }
}

/// Step start event accepted by [`OpenTelemetry::on_step_start`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenTelemetryStepStartEvent {
    pub call_id: String,
    pub step_number: u64,
}

impl OpenTelemetryStepStartEvent {
    /// Creates a step start event.
    pub fn new(call_id: impl Into<String>, step_number: u64) -> Self {
        Self {
            call_id: call_id.into(),
            step_number,
        }
    }
}

/// Language model call start event accepted by
/// [`OpenTelemetry::on_language_model_call_start`].
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryLanguageModelCallStartEvent {
    pub call_id: String,
    pub provider: String,
    pub model_id: String,
    pub input_messages: Option<Vec<SemConvMessage>>,
}

impl OpenTelemetryLanguageModelCallStartEvent {
    /// Creates a language model call start event.
    pub fn new(
        call_id: impl Into<String>,
        provider: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            provider: provider.into(),
            model_id: model_id.into(),
            input_messages: None,
        }
    }

    /// Sets formatted input messages.
    pub fn with_input_messages(mut self, input_messages: Vec<SemConvMessage>) -> Self {
        self.input_messages = Some(input_messages);
        self
    }
}

/// Token usage values for OTel lifecycle events.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TelemetryTokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

/// Embedding usage values for OTel lifecycle events.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OpenTelemetryEmbeddingUsage {
    pub tokens: Option<u64>,
}

/// Input value shape for high-level embedding operation telemetry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenTelemetryEmbeddingInput {
    /// One value for `embed`.
    One(String),

    /// Multiple values for `embedMany`.
    Many(Vec<String>),
}

impl OpenTelemetryEmbeddingInput {
    /// Creates a single embedding input value.
    pub fn one(value: impl Into<String>) -> Self {
        Self::One(value.into())
    }

    /// Creates multiple embedding input values.
    pub fn many(values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::Many(values.into_iter().map(Into::into).collect())
    }
}

/// Embedding output shape for high-level embedding operation telemetry.
#[derive(Clone, Debug, PartialEq)]
pub enum OpenTelemetryEmbeddingOutput {
    /// One embedding for `embed`.
    One(Vec<f64>),

    /// Multiple embeddings for `embedMany`.
    Many(Vec<Vec<f64>>),
}

impl OpenTelemetryEmbeddingOutput {
    /// Creates a single embedding output value.
    pub fn one(embedding: impl IntoIterator<Item = f64>) -> Self {
        Self::One(embedding.into_iter().collect())
    }

    /// Creates multiple embedding output values.
    pub fn many(embeddings: impl IntoIterator<Item = impl IntoIterator<Item = f64>>) -> Self {
        Self::Many(
            embeddings
                .into_iter()
                .map(|embedding| embedding.into_iter().collect())
                .collect(),
        )
    }
}

/// High-level embedding operation start event.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryEmbedStartEvent {
    pub call_id: String,
    pub operation_id: String,
    pub provider: String,
    pub model_id: String,
    pub input: OpenTelemetryEmbeddingInput,
    pub telemetry: TelemetryOptions,
}

impl OpenTelemetryEmbedStartEvent {
    /// Creates an embedding operation start event.
    pub fn new(
        call_id: impl Into<String>,
        operation_id: impl Into<String>,
        provider: impl Into<String>,
        model_id: impl Into<String>,
        input: OpenTelemetryEmbeddingInput,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            operation_id: operation_id.into(),
            provider: provider.into(),
            model_id: model_id.into(),
            input,
            telemetry: TelemetryOptions::new(),
        }
    }

    /// Sets telemetry recording options.
    pub fn with_telemetry(mut self, telemetry: TelemetryOptions) -> Self {
        self.telemetry = telemetry;
        self
    }
}

/// High-level embedding operation end event.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryEmbedEndEvent {
    pub call_id: String,
    pub embedding: OpenTelemetryEmbeddingOutput,
    pub usage: OpenTelemetryEmbeddingUsage,
}

impl OpenTelemetryEmbedEndEvent {
    /// Creates an embedding operation end event.
    pub fn new(call_id: impl Into<String>, embedding: OpenTelemetryEmbeddingOutput) -> Self {
        Self {
            call_id: call_id.into(),
            embedding,
            usage: OpenTelemetryEmbeddingUsage::default(),
        }
    }

    /// Sets usage values.
    pub fn with_usage(mut self, usage: OpenTelemetryEmbeddingUsage) -> Self {
        self.usage = usage;
        self
    }
}

/// Embedding model call start event accepted by
/// [`OpenTelemetry::on_embedding_model_call_start`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenTelemetryEmbeddingModelCallStartEvent {
    pub call_id: String,
    pub embed_call_id: String,
    pub values: Vec<String>,
}

impl OpenTelemetryEmbeddingModelCallStartEvent {
    /// Creates an embedding model call start event.
    pub fn new(
        call_id: impl Into<String>,
        embed_call_id: impl Into<String>,
        values: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            embed_call_id: embed_call_id.into(),
            values: values.into_iter().map(Into::into).collect(),
        }
    }
}

/// Embedding model call end event accepted by
/// [`OpenTelemetry::on_embedding_model_call_end`].
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryEmbeddingModelCallEndEvent {
    pub call_id: String,
    pub embed_call_id: String,
    pub embeddings: Vec<Vec<f64>>,
    pub usage: OpenTelemetryEmbeddingUsage,
}

impl OpenTelemetryEmbeddingModelCallEndEvent {
    /// Creates an embedding model call end event.
    pub fn new(
        call_id: impl Into<String>,
        embed_call_id: impl Into<String>,
        embeddings: impl IntoIterator<Item = impl IntoIterator<Item = f64>>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            embed_call_id: embed_call_id.into(),
            embeddings: embeddings
                .into_iter()
                .map(|embedding| embedding.into_iter().collect())
                .collect(),
            usage: OpenTelemetryEmbeddingUsage::default(),
        }
    }

    /// Sets usage values.
    pub fn with_usage(mut self, usage: OpenTelemetryEmbeddingUsage) -> Self {
        self.usage = usage;
        self
    }
}

/// Language model call end event accepted by
/// [`OpenTelemetry::on_language_model_call_end`].
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryLanguageModelCallEndEvent {
    pub call_id: String,
    pub finish_reason: String,
    pub usage: Option<TelemetryTokenUsage>,
    pub output_messages: Option<Vec<SemConvMessage>>,
}

impl OpenTelemetryLanguageModelCallEndEvent {
    /// Creates a language model call end event.
    pub fn new(call_id: impl Into<String>, finish_reason: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            finish_reason: finish_reason.into(),
            usage: None,
            output_messages: None,
        }
    }

    /// Sets usage values.
    pub fn with_usage(mut self, usage: TelemetryTokenUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Sets formatted output messages.
    pub fn with_output_messages(mut self, output_messages: Vec<SemConvMessage>) -> Self {
        self.output_messages = Some(output_messages);
        self
    }
}

/// Tool execution start event accepted by [`OpenTelemetry::on_tool_execution_start`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenTelemetryToolExecutionStartEvent {
    pub call_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
}

impl OpenTelemetryToolExecutionStartEvent {
    /// Creates a tool execution start event.
    pub fn new(
        call_id: impl Into<String>,
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
        }
    }
}

/// Tool execution end event accepted by [`OpenTelemetry::on_tool_execution_end`].
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryToolExecutionEndEvent {
    pub call_id: String,
    pub tool_call_id: String,
    pub output: Option<TelemetryAttributeValue>,
}

impl OpenTelemetryToolExecutionEndEvent {
    /// Creates a tool execution end event.
    pub fn new(call_id: impl Into<String>, tool_call_id: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            tool_call_id: tool_call_id.into(),
            output: None,
        }
    }

    /// Sets the tool output attribute.
    pub fn with_output(mut self, output: impl Into<TelemetryAttributeValue>) -> Self {
        self.output = Some(output.into());
        self
    }
}

/// High-level rerank operation start event.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryRerankStartEvent {
    pub call_id: String,
    pub operation_id: String,
    pub provider: String,
    pub model_id: String,
    pub documents: Vec<JsonValue>,
    pub telemetry: TelemetryOptions,
}

impl OpenTelemetryRerankStartEvent {
    /// Creates a rerank operation start event.
    pub fn new(
        call_id: impl Into<String>,
        provider: impl Into<String>,
        model_id: impl Into<String>,
        documents: impl IntoIterator<Item = JsonValue>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            operation_id: "ai.rerank".to_string(),
            provider: provider.into(),
            model_id: model_id.into(),
            documents: documents.into_iter().collect(),
            telemetry: TelemetryOptions::new(),
        }
    }

    /// Sets telemetry recording options.
    pub fn with_telemetry(mut self, telemetry: TelemetryOptions) -> Self {
        self.telemetry = telemetry;
        self
    }
}

/// High-level rerank operation end event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenTelemetryRerankEndEvent {
    pub call_id: String,
}

impl OpenTelemetryRerankEndEvent {
    /// Creates a rerank operation end event.
    pub fn new(call_id: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
        }
    }
}

/// Reranking model call start event accepted by
/// [`OpenTelemetry::on_reranking_model_call_start`].
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryRerankingModelCallStartEvent {
    pub call_id: String,
    pub documents: Vec<JsonValue>,
    pub documents_type: String,
}

impl OpenTelemetryRerankingModelCallStartEvent {
    /// Creates a reranking model call start event.
    pub fn new(
        call_id: impl Into<String>,
        documents_type: impl Into<String>,
        documents: impl IntoIterator<Item = JsonValue>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            documents: documents.into_iter().collect(),
            documents_type: documents_type.into(),
        }
    }
}

/// Reranking model call end event accepted by
/// [`OpenTelemetry::on_reranking_model_call_end`].
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryRerankingModelCallEndEvent {
    pub call_id: String,
    pub documents_type: String,
    pub ranking: Vec<JsonValue>,
}

impl OpenTelemetryRerankingModelCallEndEvent {
    /// Creates a reranking model call end event.
    pub fn new(
        call_id: impl Into<String>,
        documents_type: impl Into<String>,
        ranking: impl IntoIterator<Item = JsonValue>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            documents_type: documents_type.into(),
            ranking: ranking.into_iter().collect(),
        }
    }
}

/// Operation end event accepted by [`OpenTelemetry::on_end`].
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTelemetryEndEvent {
    pub call_id: String,
    pub finish_reason: String,
    pub usage: Option<TelemetryTokenUsage>,
    pub output_messages: Option<Vec<SemConvMessage>>,
}

impl OpenTelemetryEndEvent {
    /// Creates an operation end event.
    pub fn new(call_id: impl Into<String>, finish_reason: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            finish_reason: finish_reason.into(),
            usage: None,
            output_messages: None,
        }
    }

    /// Sets usage values.
    pub fn with_usage(mut self, usage: TelemetryTokenUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Sets formatted output messages.
    pub fn with_output_messages(mut self, output_messages: Vec<SemConvMessage>) -> Self {
        self.output_messages = Some(output_messages);
        self
    }
}

/// Error event accepted by [`OpenTelemetry::on_error`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenTelemetryErrorEvent {
    pub call_id: String,
    pub error: RecordSpanError,
}

impl OpenTelemetryErrorEvent {
    /// Creates an OTel error event.
    pub fn new(call_id: impl Into<String>, error: RecordSpanError) -> Self {
        Self {
            call_id: call_id.into(),
            error,
        }
    }
}

/// Dependency-free Rust recorder for upstream `OpenTelemetry` behavior.
///
/// This type intentionally records into [`MockTracer`] today. A future slice can
/// adapt the same event semantics to the real `opentelemetry` crate without
/// changing the tested attribute construction rules.
pub struct OpenTelemetry {
    tracer: MockTracer,
    options: OpenTelemetryOptions,
    call_states: BTreeMap<String, OpenTelemetryCallState>,
}

impl OpenTelemetry {
    /// Creates a recorder with default options.
    pub fn new(options: OpenTelemetryOptions) -> Self {
        Self {
            tracer: MockTracer::new(),
            options,
            call_states: BTreeMap::new(),
        }
    }

    /// Returns the recorded tracer.
    pub fn tracer(&self) -> &MockTracer {
        &self.tracer
    }

    /// Consumes this recorder and returns the tracer.
    pub fn into_tracer(self) -> MockTracer {
        self.tracer
    }

    /// Returns active call state count.
    pub fn active_call_count(&self) -> usize {
        self.call_states.len()
    }

    /// Starts a high-level object-generation operation span.
    pub fn on_object_operation_start(&mut self, event: OpenTelemetryObjectStartEvent) {
        let attributes = object_operation_start_attributes(
            &event.telemetry,
            &event.operation_id,
            &event.provider,
            &event.model_id,
            &event.settings,
            event.system_instructions.as_ref(),
            event.input_messages.as_ref(),
            ObjectSchemaSupplemental {
                schema: event.schema.as_ref(),
                schema_name: event.schema_name.as_deref(),
                schema_description: event.schema_description.as_deref(),
                output_mode: event.output_mode.as_deref(),
            },
            self.options.supplemental_attributes,
        );
        let span_attributes = self.span_attributes(
            attributes,
            OpenTelemetrySpanType::Operation,
            &event.operation_id,
            &event.call_id,
            None,
        );
        let span_name = format!(
            "{} {}",
            map_operation_name(&event.operation_id),
            event.model_id
        );
        let root_span = self.tracer.start_span(span_name, span_attributes);

        self.call_states.insert(
            event.call_id.clone(),
            OpenTelemetryCallState {
                operation_id: event.operation_id,
                telemetry: event.telemetry,
                provider: event.provider,
                model_id: event.model_id,
                root_span,
                step_span: None,
                inference_span: None,
                embed_spans: BTreeMap::new(),
                rerank_span: None,
                tool_spans: BTreeMap::new(),
                runtime_context: None,
            },
        );
    }

    /// Starts an object-generation model call span.
    pub fn on_object_step_start(&mut self, event: OpenTelemetryObjectStepStartEvent) {
        let Some((operation_id, telemetry, runtime_context)) =
            self.call_states.get(&event.call_id).map(|state| {
                (
                    state.operation_id.clone(),
                    state.telemetry.clone(),
                    state.runtime_context.clone(),
                )
            })
        else {
            return;
        };
        let attributes = object_model_start_attributes(
            &telemetry,
            &event.provider,
            &event.model_id,
            &event.settings,
            event.input_messages.as_ref(),
        );
        let span_attributes = self.span_attributes(
            attributes,
            OpenTelemetrySpanType::LanguageModel,
            &operation_id,
            &event.call_id,
            runtime_context.as_ref(),
        );
        let span = self
            .tracer
            .start_span(format!("chat {}", event.model_id), span_attributes);
        if let Some(state) = self.call_states.get_mut(&event.call_id) {
            state.inference_span = Some(span);
        }
    }

    /// Finishes an object-generation model call span.
    pub fn on_object_step_finish(&mut self, event: OpenTelemetryObjectStepFinishEvent) {
        let Some((span, telemetry)) = self.call_states.get(&event.call_id).and_then(|state| {
            state
                .inference_span
                .map(|span| (span, state.telemetry.clone()))
        }) else {
            return;
        };
        self.end_span_with_attributes(
            Some(span),
            object_step_finish_attributes(&telemetry, &event),
        );
        if let Some(state) = self.call_states.get_mut(&event.call_id) {
            state.inference_span = None;
        }
    }

    /// Finishes a high-level object-generation operation span.
    pub fn on_object_operation_end(&mut self, event: OpenTelemetryObjectEndEvent) {
        let Some(state) = self.call_states.remove(&event.call_id) else {
            return;
        };
        self.end_span_with_attributes(state.inference_span, TelemetryAttributes::new());
        self.end_span_with_attributes(
            Some(state.root_span),
            object_operation_end_attributes(&state.telemetry, &event),
        );
    }

    /// Starts a high-level embedding operation span.
    pub fn on_embed_operation_start(&mut self, event: OpenTelemetryEmbedStartEvent) {
        let provider_name = map_provider_name(&event.provider);
        let operation_name = map_operation_name(&event.operation_id);
        let mut attributes = select_attributes(
            Some(&event.telemetry),
            [
                (
                    "gen_ai.operation.name".to_string(),
                    AttributeSpec::value(json!(operation_name)),
                ),
                (
                    "gen_ai.provider.name".to_string(),
                    AttributeSpec::value(json!(provider_name)),
                ),
                (
                    "gen_ai.request.model".to_string(),
                    AttributeSpec::value(json!(event.model_id)),
                ),
            ],
        );
        attributes.extend(select_supplemental_attributes(
            Some(&event.telemetry),
            self.options.supplemental_attributes,
            SupplementalAttributes {
                embedding: embedding_input_attributes(&event.input),
                ..SupplementalAttributes::default()
            },
        ));

        let span_attributes = self.span_attributes(
            attributes,
            OpenTelemetrySpanType::Operation,
            &event.operation_id,
            &event.call_id,
            None,
        );
        let span_name = format!(
            "{} {}",
            map_operation_name(&event.operation_id),
            event.model_id
        );
        let root_span = self.tracer.start_span(span_name, span_attributes);

        self.call_states.insert(
            event.call_id.clone(),
            OpenTelemetryCallState {
                operation_id: event.operation_id,
                telemetry: event.telemetry,
                provider: event.provider,
                model_id: event.model_id,
                root_span,
                step_span: None,
                inference_span: None,
                embed_spans: BTreeMap::new(),
                rerank_span: None,
                tool_spans: BTreeMap::new(),
                runtime_context: None,
            },
        );
    }

    /// Finishes a high-level embedding operation span.
    pub fn on_embed_operation_end(&mut self, event: OpenTelemetryEmbedEndEvent) {
        let Some(state) = self.call_states.remove(&event.call_id) else {
            return;
        };
        for span in state.embed_spans.into_values() {
            self.end_span_with_attributes(Some(span), TelemetryAttributes::new());
        }
        self.end_span_with_attributes(
            Some(state.root_span),
            embedding_end_attributes(
                &state.telemetry,
                &event.embedding,
                event.usage,
                state.operation_id == "ai.embedMany",
                self.options.supplemental_attributes,
            ),
        );
    }

    /// Starts an inner embedding model call span.
    pub fn on_embedding_model_call_start(
        &mut self,
        event: OpenTelemetryEmbeddingModelCallStartEvent,
    ) {
        let Some((operation_id, telemetry, provider, model_id, runtime_context)) =
            self.call_states.get(&event.call_id).map(|state| {
                (
                    state.operation_id.clone(),
                    state.telemetry.clone(),
                    state.provider.clone(),
                    state.model_id.clone(),
                    state.runtime_context.clone(),
                )
            })
        else {
            return;
        };
        let attributes = embedding_model_start_attributes(
            &telemetry,
            &provider,
            &model_id,
            &event.values,
            self.options.supplemental_attributes,
        );
        let span_attributes = self.span_attributes(
            attributes,
            OpenTelemetrySpanType::Embedding,
            &operation_id,
            &event.call_id,
            runtime_context.as_ref(),
        );
        let span = self
            .tracer
            .start_span(format!("embeddings {model_id}"), span_attributes);
        if let Some(state) = self.call_states.get_mut(&event.call_id) {
            state.embed_spans.insert(event.embed_call_id, span);
        }
    }

    /// Finishes an inner embedding model call span.
    pub fn on_embedding_model_call_end(&mut self, event: OpenTelemetryEmbeddingModelCallEndEvent) {
        let Some((span, telemetry)) = self.call_states.get_mut(&event.call_id).and_then(|state| {
            state
                .embed_spans
                .remove(&event.embed_call_id)
                .map(|span| (span, state.telemetry.clone()))
        }) else {
            return;
        };
        self.end_span_with_attributes(
            Some(span),
            embedding_model_end_attributes(
                &telemetry,
                &event.embeddings,
                event.usage,
                self.options.supplemental_attributes,
            ),
        );
    }

    /// Starts a high-level rerank operation span.
    pub fn on_rerank_operation_start(&mut self, event: OpenTelemetryRerankStartEvent) {
        let provider_name = map_provider_name(&event.provider);
        let mut attributes = select_attributes(
            Some(&event.telemetry),
            [
                (
                    "gen_ai.operation.name".to_string(),
                    AttributeSpec::value(json!("rerank")),
                ),
                (
                    "gen_ai.provider.name".to_string(),
                    AttributeSpec::value(json!(provider_name)),
                ),
                (
                    "gen_ai.request.model".to_string(),
                    AttributeSpec::value(json!(event.model_id)),
                ),
            ],
        );
        attributes.extend(select_supplemental_attributes(
            Some(&event.telemetry),
            self.options.supplemental_attributes,
            SupplementalAttributes {
                reranking: reranking_document_attributes(&event.documents),
                ..SupplementalAttributes::default()
            },
        ));

        let span_attributes = self.span_attributes(
            attributes,
            OpenTelemetrySpanType::Operation,
            &event.operation_id,
            &event.call_id,
            None,
        );
        let root_span = self
            .tracer
            .start_span(format!("rerank {}", event.model_id), span_attributes);

        self.call_states.insert(
            event.call_id.clone(),
            OpenTelemetryCallState {
                operation_id: event.operation_id,
                telemetry: event.telemetry,
                provider: event.provider,
                model_id: event.model_id,
                root_span,
                step_span: None,
                inference_span: None,
                embed_spans: BTreeMap::new(),
                rerank_span: None,
                tool_spans: BTreeMap::new(),
                runtime_context: None,
            },
        );
    }

    /// Finishes a high-level rerank operation span.
    pub fn on_rerank_operation_end(&mut self, event: OpenTelemetryRerankEndEvent) {
        let Some(state) = self.call_states.remove(&event.call_id) else {
            return;
        };
        self.end_span_with_attributes(state.rerank_span, TelemetryAttributes::new());
        self.end_span_with_attributes(Some(state.root_span), TelemetryAttributes::new());
    }

    /// Starts an inner reranking model call span.
    pub fn on_reranking_model_call_start(
        &mut self,
        event: OpenTelemetryRerankingModelCallStartEvent,
    ) {
        let Some((operation_id, telemetry, provider, model_id, runtime_context)) =
            self.call_states.get(&event.call_id).map(|state| {
                (
                    state.operation_id.clone(),
                    state.telemetry.clone(),
                    state.provider.clone(),
                    state.model_id.clone(),
                    state.runtime_context.clone(),
                )
            })
        else {
            return;
        };
        let mut attributes = select_attributes(
            Some(&telemetry),
            [
                (
                    "gen_ai.operation.name".to_string(),
                    AttributeSpec::value(json!("rerank")),
                ),
                (
                    "gen_ai.provider.name".to_string(),
                    AttributeSpec::value(json!(map_provider_name(&provider))),
                ),
                (
                    "gen_ai.request.model".to_string(),
                    AttributeSpec::value(json!(model_id)),
                ),
            ],
        );
        attributes.extend(select_supplemental_attributes(
            Some(&telemetry),
            self.options.supplemental_attributes,
            SupplementalAttributes {
                reranking: reranking_document_attributes(&event.documents),
                ..SupplementalAttributes::default()
            },
        ));
        let span_attributes = self.span_attributes(
            attributes,
            OpenTelemetrySpanType::Reranking,
            &operation_id,
            &event.call_id,
            runtime_context.as_ref(),
        );
        let span = self
            .tracer
            .start_span(format!("rerank {model_id}"), span_attributes);
        if let Some(state) = self.call_states.get_mut(&event.call_id) {
            state.rerank_span = Some(span);
        }
    }

    /// Finishes an inner reranking model call span.
    pub fn on_reranking_model_call_end(&mut self, event: OpenTelemetryRerankingModelCallEndEvent) {
        let Some((span, telemetry)) = self.call_states.get_mut(&event.call_id).and_then(|state| {
            state
                .rerank_span
                .take()
                .map(|span| (span, state.telemetry.clone()))
        }) else {
            return;
        };
        self.end_span_with_attributes(
            Some(span),
            reranking_model_end_attributes(
                &telemetry,
                &event.documents_type,
                &event.ranking,
                self.options.supplemental_attributes,
            ),
        );
    }

    /// Starts a root operation span.
    pub fn on_start(&mut self, event: OpenTelemetryStartEvent) {
        let provider_name = map_provider_name(&event.provider);
        let operation_name = map_operation_name(&event.operation_id);
        let mut attributes = select_attributes(
            Some(&event.telemetry),
            vec![
                (
                    "gen_ai.operation.name".to_string(),
                    AttributeSpec::value(json!(operation_name)),
                ),
                (
                    "gen_ai.provider.name".to_string(),
                    AttributeSpec::value(json!(provider_name)),
                ),
                (
                    "gen_ai.request.model".to_string(),
                    AttributeSpec::value(json!(event.model_id)),
                ),
                (
                    "gen_ai.agent.name".to_string(),
                    event
                        .telemetry
                        .function_id
                        .as_ref()
                        .map_or(AttributeSpec::Omitted, |function_id| {
                            AttributeSpec::value(json!(function_id))
                        }),
                ),
                (
                    "gen_ai.system_instructions".to_string(),
                    event
                        .system_instructions
                        .as_ref()
                        .map_or(AttributeSpec::Omitted, |system_instructions| {
                            AttributeSpec::input(json!(system_instructions))
                        }),
                ),
                (
                    "gen_ai.input.messages".to_string(),
                    event
                        .input_messages
                        .as_ref()
                        .map_or(AttributeSpec::Omitted, |input_messages| {
                            AttributeSpec::input(json!(input_messages))
                        }),
                ),
            ],
        );

        if event.telemetry.should_record() {
            for (key, value) in event.settings.iter().filter(|(_, value)| !value.is_null()) {
                attributes.insert(format!("gen_ai.request.{key}"), value.clone());
            }
        }

        attributes.extend(select_supplemental_attributes(
            Some(&event.telemetry),
            self.options.supplemental_attributes,
            SupplementalAttributes {
                runtime_context: event
                    .runtime_context
                    .as_ref()
                    .map_or_else(Vec::new, get_runtime_context_attributes),
                ..SupplementalAttributes::default()
            },
        ));

        let span_attributes = self.span_attributes(
            attributes,
            OpenTelemetrySpanType::Operation,
            &event.operation_id,
            &event.call_id,
            event.runtime_context.as_ref(),
        );
        let span_name = format!(
            "{} {}",
            map_operation_name(&event.operation_id),
            event.model_id
        );
        let root_span = self.tracer.start_span(span_name, span_attributes);

        self.call_states.insert(
            event.call_id.clone(),
            OpenTelemetryCallState {
                operation_id: event.operation_id,
                telemetry: event.telemetry,
                provider: event.provider,
                model_id: event.model_id,
                root_span,
                step_span: None,
                inference_span: None,
                embed_spans: BTreeMap::new(),
                rerank_span: None,
                tool_spans: BTreeMap::new(),
                runtime_context: event.runtime_context,
            },
        );
    }

    /// Starts a step span.
    pub fn on_step_start(&mut self, event: OpenTelemetryStepStartEvent) {
        let Some((operation_id, runtime_context)) = self
            .call_states
            .get(&event.call_id)
            .map(|state| (state.operation_id.clone(), state.runtime_context.clone()))
        else {
            return;
        };
        let attributes = self.span_attributes(
            TelemetryAttributes::from([(
                "gen_ai.operation.step".to_string(),
                json!(event.step_number),
            )]),
            OpenTelemetrySpanType::Step,
            &operation_id,
            &event.call_id,
            runtime_context.as_ref(),
        );
        let span = self
            .tracer
            .start_span(format!("step {}", event.step_number), attributes);
        if let Some(state) = self.call_states.get_mut(&event.call_id) {
            state.step_span = Some(span);
        }
    }

    /// Starts a language model call span.
    pub fn on_language_model_call_start(
        &mut self,
        event: OpenTelemetryLanguageModelCallStartEvent,
    ) {
        let Some((operation_id, telemetry, runtime_context)) =
            self.call_states.get(&event.call_id).map(|state| {
                (
                    state.operation_id.clone(),
                    state.telemetry.clone(),
                    state.runtime_context.clone(),
                )
            })
        else {
            return;
        };
        let attributes = select_attributes(
            Some(&telemetry),
            vec![
                (
                    "gen_ai.operation.name".to_string(),
                    AttributeSpec::value(json!("chat")),
                ),
                (
                    "gen_ai.provider.name".to_string(),
                    AttributeSpec::value(json!(map_provider_name(&event.provider))),
                ),
                (
                    "gen_ai.request.model".to_string(),
                    AttributeSpec::value(json!(event.model_id)),
                ),
                (
                    "gen_ai.input.messages".to_string(),
                    event
                        .input_messages
                        .as_ref()
                        .map_or(AttributeSpec::Omitted, |input_messages| {
                            AttributeSpec::input(json!(input_messages))
                        }),
                ),
            ],
        );
        let span_attributes = self.span_attributes(
            attributes,
            OpenTelemetrySpanType::LanguageModel,
            &operation_id,
            &event.call_id,
            runtime_context.as_ref(),
        );
        let span = self
            .tracer
            .start_span(format!("chat {}", event.model_id), span_attributes);
        if let Some(state) = self.call_states.get_mut(&event.call_id) {
            state.inference_span = Some(span);
        }
    }

    /// Finishes a language model call span.
    pub fn on_language_model_call_end(&mut self, event: OpenTelemetryLanguageModelCallEndEvent) {
        let Some((span, telemetry)) = self.call_states.get(&event.call_id).and_then(|state| {
            state
                .inference_span
                .map(|span| (span, state.telemetry.clone()))
        }) else {
            return;
        };

        let mut attributes = language_model_end_attributes(&telemetry, &event);
        if let Some(span) = self.tracer.spans.get_mut(span) {
            span.set_attributes(std::mem::take(&mut attributes));
            span.end();
        }
        if let Some(state) = self.call_states.get_mut(&event.call_id) {
            state.inference_span = None;
        }
    }

    /// Starts a tool execution span.
    pub fn on_tool_execution_start(&mut self, event: OpenTelemetryToolExecutionStartEvent) {
        let Some((operation_id, runtime_context)) = self
            .call_states
            .get(&event.call_id)
            .map(|state| (state.operation_id.clone(), state.runtime_context.clone()))
        else {
            return;
        };
        let attributes = self.span_attributes(
            TelemetryAttributes::from([("gen_ai.tool.name".to_string(), json!(event.tool_name))]),
            OpenTelemetrySpanType::Tool,
            &operation_id,
            &event.call_id,
            runtime_context.as_ref(),
        );
        let span = self
            .tracer
            .start_span(format!("execute_tool {}", event.tool_name), attributes);
        if let Some(state) = self.call_states.get_mut(&event.call_id) {
            state.tool_spans.insert(event.tool_call_id, span);
        }
    }

    /// Executes a closure in the corresponding tool span context when present.
    pub fn execute_tool<T>(
        &mut self,
        call_id: &str,
        tool_call_id: &str,
        execute: impl FnOnce(Option<&mut MockSpan>) -> T,
    ) -> T {
        let span = self
            .call_states
            .get(call_id)
            .and_then(|state| state.tool_spans.get(tool_call_id).copied());
        match span.and_then(|index| self.tracer.spans.get_mut(index)) {
            Some(span) => execute(Some(span)),
            None => execute(None),
        }
    }

    /// Finishes a tool execution span.
    pub fn on_tool_execution_end(&mut self, event: OpenTelemetryToolExecutionEndEvent) {
        let Some(span) = self
            .call_states
            .get_mut(&event.call_id)
            .and_then(|state| state.tool_spans.remove(&event.tool_call_id))
        else {
            return;
        };

        let Some(state) = self.call_states.get(&event.call_id) else {
            return;
        };
        let attributes = select_attributes(
            Some(&state.telemetry),
            [(
                "gen_ai.tool.output".to_string(),
                event
                    .output
                    .map_or(AttributeSpec::Omitted, AttributeSpec::output),
            )],
        );

        if let Some(span) = self.tracer.spans.get_mut(span) {
            span.set_attributes(attributes);
            span.end();
        }
    }

    /// Finishes the current step span.
    pub fn on_step_finish(&mut self, call_id: &str) {
        let Some(span) = self
            .call_states
            .get_mut(call_id)
            .and_then(|state| state.step_span.take())
        else {
            return;
        };
        if let Some(span) = self.tracer.spans.get_mut(span) {
            span.end();
        }
    }

    /// Finishes the root operation span and cleans up call state.
    pub fn on_end(&mut self, event: OpenTelemetryEndEvent) {
        let Some(state) = self.call_states.remove(&event.call_id) else {
            return;
        };

        let end_event = OpenTelemetryLanguageModelCallEndEvent {
            call_id: event.call_id,
            finish_reason: event.finish_reason,
            usage: event.usage,
            output_messages: event.output_messages,
        };
        let attributes = language_model_end_attributes(&state.telemetry, &end_event);
        self.end_span_with_attributes(state.inference_span, TelemetryAttributes::new());
        self.end_span_with_attributes(state.step_span, TelemetryAttributes::new());
        for span in state.tool_spans.into_values() {
            self.end_span_with_attributes(Some(span), TelemetryAttributes::new());
        }
        self.end_span_with_attributes(Some(state.root_span), attributes);
    }

    /// Records an error on active spans, ends them, and cleans up call state.
    pub fn on_error(&mut self, event: OpenTelemetryErrorEvent) {
        let Some(state) = self.call_states.remove(&event.call_id) else {
            return;
        };
        let mut spans = Vec::new();
        spans.extend(state.tool_spans.into_values());
        spans.extend(state.embed_spans.into_values());
        spans.extend(
            [
                state.rerank_span,
                state.inference_span,
                state.step_span,
                Some(state.root_span),
            ]
            .into_iter()
            .flatten(),
        );
        for span in spans {
            if let Some(span) = self.tracer.spans.get_mut(span) {
                record_error_on_span(span, event.error.clone());
                span.end();
            }
        }
    }

    fn span_attributes(
        &self,
        attributes: TelemetryAttributes,
        span_type: OpenTelemetrySpanType,
        operation_id: &str,
        call_id: &str,
        runtime_context: Option<&TelemetryAttributes>,
    ) -> TelemetryAttributes {
        let mut custom_attributes = self
            .options
            .enrich_span
            .as_ref()
            .and_then(|enrich_span| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    enrich_span(EnrichSpanOptions {
                        span_type,
                        operation_id: operation_id.to_string(),
                        call_id: call_id.to_string(),
                        runtime_context: runtime_context.cloned(),
                    })
                }))
                .ok()
            })
            .unwrap_or_default();
        custom_attributes.extend(attributes);
        custom_attributes
    }

    fn end_span_with_attributes(&mut self, span: Option<usize>, attributes: TelemetryAttributes) {
        let Some(span) = span.and_then(|span| self.tracer.spans.get_mut(span)) else {
            return;
        };
        span.set_attributes(attributes);
        span.end();
    }
}

fn language_model_end_attributes(
    telemetry: &TelemetryOptions,
    event: &OpenTelemetryLanguageModelCallEndEvent,
) -> TelemetryAttributes {
    let mut attributes = select_attributes(
        Some(telemetry),
        vec![
            (
                "gen_ai.response.finish_reasons".to_string(),
                AttributeSpec::value(json!([map_finish_reason(&event.finish_reason)])),
            ),
            (
                "gen_ai.output.messages".to_string(),
                event
                    .output_messages
                    .as_ref()
                    .map_or(AttributeSpec::Omitted, |output_messages| {
                        AttributeSpec::output(json!(output_messages))
                    }),
            ),
        ],
    );

    if let Some(usage) = event.usage {
        for (key, value) in [
            ("gen_ai.usage.input_tokens", usage.input_tokens),
            ("gen_ai.usage.output_tokens", usage.output_tokens),
            ("gen_ai.usage.total_tokens", usage.total_tokens),
        ] {
            if let Some(value) = value {
                attributes.insert(key.to_string(), json!(value));
            }
        }
    }

    attributes
}

struct ObjectSchemaSupplemental<'a> {
    schema: Option<&'a JsonValue>,
    schema_name: Option<&'a str>,
    schema_description: Option<&'a str>,
    output_mode: Option<&'a str>,
}

fn object_operation_start_attributes(
    telemetry: &TelemetryOptions,
    operation_id: &str,
    provider: &str,
    model_id: &str,
    settings: &TelemetryAttributes,
    system_instructions: Option<&Vec<SemConvSystemInstruction>>,
    input_messages: Option<&Vec<SemConvMessage>>,
    schema: ObjectSchemaSupplemental<'_>,
    supplemental_attributes: SupplementalAttributeOptions,
) -> TelemetryAttributes {
    let mut attributes = select_attributes(
        Some(telemetry),
        vec![
            (
                "gen_ai.operation.name".to_string(),
                AttributeSpec::value(json!(map_operation_name(operation_id))),
            ),
            (
                "gen_ai.provider.name".to_string(),
                AttributeSpec::value(json!(map_provider_name(provider))),
            ),
            (
                "gen_ai.request.model".to_string(),
                AttributeSpec::value(json!(model_id)),
            ),
            (
                "gen_ai.agent.name".to_string(),
                telemetry
                    .function_id
                    .as_ref()
                    .map_or(AttributeSpec::Omitted, |function_id| {
                        AttributeSpec::value(json!(function_id))
                    }),
            ),
            (
                "gen_ai.output.type".to_string(),
                AttributeSpec::value(json!("json")),
            ),
            (
                "gen_ai.system_instructions".to_string(),
                system_instructions.map_or(AttributeSpec::Omitted, |system_instructions| {
                    AttributeSpec::input(json!(system_instructions))
                }),
            ),
            (
                "gen_ai.input.messages".to_string(),
                input_messages.map_or(AttributeSpec::Omitted, |input_messages| {
                    AttributeSpec::input(json!(input_messages))
                }),
            ),
        ],
    );
    append_request_settings(&mut attributes, telemetry, settings);
    attributes.extend(select_supplemental_attributes(
        Some(telemetry),
        supplemental_attributes,
        SupplementalAttributes {
            schema: object_schema_attributes(schema),
            ..SupplementalAttributes::default()
        },
    ));
    attributes
}

fn object_model_start_attributes(
    telemetry: &TelemetryOptions,
    provider: &str,
    model_id: &str,
    settings: &TelemetryAttributes,
    input_messages: Option<&Vec<SemConvMessage>>,
) -> TelemetryAttributes {
    let mut attributes = select_attributes(
        Some(telemetry),
        vec![
            (
                "gen_ai.operation.name".to_string(),
                AttributeSpec::value(json!("chat")),
            ),
            (
                "gen_ai.provider.name".to_string(),
                AttributeSpec::value(json!(map_provider_name(provider))),
            ),
            (
                "gen_ai.request.model".to_string(),
                AttributeSpec::value(json!(model_id)),
            ),
            (
                "gen_ai.output.type".to_string(),
                AttributeSpec::value(json!("json")),
            ),
            (
                "gen_ai.input.messages".to_string(),
                input_messages.map_or(AttributeSpec::Omitted, |input_messages| {
                    AttributeSpec::input(json!(input_messages))
                }),
            ),
        ],
    );
    append_request_settings(&mut attributes, telemetry, settings);
    attributes
}

fn object_step_finish_attributes(
    telemetry: &TelemetryOptions,
    event: &OpenTelemetryObjectStepFinishEvent,
) -> TelemetryAttributes {
    let output_messages = event
        .object_text
        .as_ref()
        .map(|object_text| format_object_output_messages(object_text, &event.finish_reason));
    language_model_end_attributes(
        telemetry,
        &OpenTelemetryLanguageModelCallEndEvent {
            call_id: event.call_id.clone(),
            finish_reason: event.finish_reason.clone(),
            usage: event.usage,
            output_messages,
        },
    )
}

fn object_operation_end_attributes(
    telemetry: &TelemetryOptions,
    event: &OpenTelemetryObjectEndEvent,
) -> TelemetryAttributes {
    let output_messages = event.object.as_ref().map(|object| {
        format_object_output_messages(telemetry_json_string(object), &event.finish_reason)
    });
    language_model_end_attributes(
        telemetry,
        &OpenTelemetryLanguageModelCallEndEvent {
            call_id: event.call_id.clone(),
            finish_reason: event.finish_reason.clone(),
            usage: event.usage,
            output_messages,
        },
    )
}

fn object_schema_attributes(schema: ObjectSchemaSupplemental<'_>) -> Vec<(String, AttributeSpec)> {
    vec![
        (
            "ai.schema".to_string(),
            schema.schema.map_or(AttributeSpec::Omitted, |schema| {
                AttributeSpec::input(json!(telemetry_json_string(schema)))
            }),
        ),
        (
            "ai.schema.name".to_string(),
            schema
                .schema_name
                .map_or(AttributeSpec::Omitted, |schema_name| {
                    AttributeSpec::value(json!(schema_name))
                }),
        ),
        (
            "ai.schema.description".to_string(),
            schema
                .schema_description
                .map_or(AttributeSpec::Omitted, |schema_description| {
                    AttributeSpec::value(json!(schema_description))
                }),
        ),
        (
            "ai.settings.output".to_string(),
            schema
                .output_mode
                .map_or(AttributeSpec::Omitted, |output_mode| {
                    AttributeSpec::value(json!(output_mode))
                }),
        ),
    ]
}

fn append_request_settings(
    attributes: &mut TelemetryAttributes,
    telemetry: &TelemetryOptions,
    settings: &TelemetryAttributes,
) {
    if telemetry.should_record() {
        for (key, value) in settings.iter().filter(|(_, value)| !value.is_null()) {
            attributes.insert(format!("gen_ai.request.{key}"), value.clone());
        }
    }
}

fn embedding_model_start_attributes(
    telemetry: &TelemetryOptions,
    provider: &str,
    model_id: &str,
    values: &[String],
    supplemental_attributes: SupplementalAttributeOptions,
) -> TelemetryAttributes {
    let mut attributes = select_attributes(
        Some(telemetry),
        [
            (
                "gen_ai.operation.name".to_string(),
                AttributeSpec::value(json!("embeddings")),
            ),
            (
                "gen_ai.provider.name".to_string(),
                AttributeSpec::value(json!(map_provider_name(provider))),
            ),
            (
                "gen_ai.request.model".to_string(),
                AttributeSpec::value(json!(model_id)),
            ),
        ],
    );
    attributes.extend(select_supplemental_attributes(
        Some(telemetry),
        supplemental_attributes,
        SupplementalAttributes {
            embedding: embedding_values_attributes(values),
            ..SupplementalAttributes::default()
        },
    ));
    attributes
}

fn embedding_model_end_attributes(
    telemetry: &TelemetryOptions,
    embeddings: &[Vec<f64>],
    usage: OpenTelemetryEmbeddingUsage,
    supplemental_attributes: SupplementalAttributeOptions,
) -> TelemetryAttributes {
    let mut attributes = TelemetryAttributes::new();
    if let Some(tokens) = usage.tokens {
        attributes.insert("gen_ai.usage.input_tokens".to_string(), json!(tokens));
    }
    attributes.extend(select_supplemental_attributes(
        Some(telemetry),
        supplemental_attributes,
        SupplementalAttributes {
            embedding: vec![(
                "ai.embeddings".to_string(),
                AttributeSpec::output(json!(
                    embeddings
                        .iter()
                        .map(telemetry_json_string)
                        .collect::<Vec<_>>()
                )),
            )],
            ..SupplementalAttributes::default()
        },
    ));
    attributes
}

fn embedding_end_attributes(
    telemetry: &TelemetryOptions,
    embedding: &OpenTelemetryEmbeddingOutput,
    usage: OpenTelemetryEmbeddingUsage,
    is_many: bool,
    supplemental_attributes: SupplementalAttributeOptions,
) -> TelemetryAttributes {
    let mut attributes = TelemetryAttributes::new();
    if let Some(tokens) = usage.tokens {
        attributes.insert("gen_ai.usage.input_tokens".to_string(), json!(tokens));
    }
    let embedding_attributes = match (is_many, embedding) {
        (true, OpenTelemetryEmbeddingOutput::Many(embeddings)) => vec![(
            "ai.embeddings".to_string(),
            AttributeSpec::output(json!(
                embeddings
                    .iter()
                    .map(telemetry_json_string)
                    .collect::<Vec<_>>()
            )),
        )],
        (_, OpenTelemetryEmbeddingOutput::One(embedding)) => vec![(
            "ai.embedding".to_string(),
            AttributeSpec::output(json!(telemetry_json_string(embedding))),
        )],
        (false, OpenTelemetryEmbeddingOutput::Many(embeddings)) => vec![(
            "ai.embedding".to_string(),
            AttributeSpec::output(json!(
                embeddings
                    .first()
                    .map(telemetry_json_string)
                    .unwrap_or_else(|| "[]".to_string())
            )),
        )],
    };
    attributes.extend(select_supplemental_attributes(
        Some(telemetry),
        supplemental_attributes,
        SupplementalAttributes {
            embedding: embedding_attributes,
            ..SupplementalAttributes::default()
        },
    ));
    attributes
}

fn embedding_input_attributes(input: &OpenTelemetryEmbeddingInput) -> Vec<(String, AttributeSpec)> {
    match input {
        OpenTelemetryEmbeddingInput::One(value) => vec![(
            "ai.value".to_string(),
            AttributeSpec::input(json!(telemetry_json_string(value))),
        )],
        OpenTelemetryEmbeddingInput::Many(values) => vec![(
            "ai.values".to_string(),
            AttributeSpec::input(json!(
                values.iter().map(telemetry_json_string).collect::<Vec<_>>()
            )),
        )],
    }
}

fn embedding_values_attributes(values: &[String]) -> Vec<(String, AttributeSpec)> {
    vec![(
        "ai.values".to_string(),
        AttributeSpec::input(json!(
            values.iter().map(telemetry_json_string).collect::<Vec<_>>()
        )),
    )]
}

fn reranking_document_attributes(documents: &[JsonValue]) -> Vec<(String, AttributeSpec)> {
    vec![(
        "ai.documents".to_string(),
        AttributeSpec::input(json!(
            documents
                .iter()
                .map(telemetry_json_string)
                .collect::<Vec<_>>()
        )),
    )]
}

fn reranking_model_end_attributes(
    telemetry: &TelemetryOptions,
    documents_type: &str,
    ranking: &[JsonValue],
    supplemental_attributes: SupplementalAttributeOptions,
) -> TelemetryAttributes {
    select_supplemental_attributes(
        Some(telemetry),
        supplemental_attributes,
        SupplementalAttributes {
            reranking: vec![
                (
                    "ai.ranking.type".to_string(),
                    AttributeSpec::value(json!(documents_type)),
                ),
                (
                    "ai.ranking".to_string(),
                    AttributeSpec::output(json!(
                        ranking
                            .iter()
                            .map(telemetry_json_string)
                            .collect::<Vec<_>>()
                    )),
                ),
            ],
            ..SupplementalAttributes::default()
        },
    )
}

fn telemetry_json_string<T: Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).expect("telemetry value serialization should not fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_sdk_provider::{
        LanguageModelAssistantMessage, LanguageModelFilePart, LanguageModelTextPart,
        LanguageModelToolCallPart, LanguageModelToolContentPart, LanguageModelToolMessage,
        LanguageModelToolResultPart, LanguageModelUserMessage, ProviderOptions,
    };
    use serde_json::json;

    #[test]
    fn select_attributes_matches_telemetry_recording_flags() {
        assert!(
            select_attributes(
                Some(&TelemetryOptions::new().with_enabled(false)),
                [("key", AttributeSpec::value(json!("value")))]
            )
            .is_empty()
        );

        let telemetry = TelemetryOptions::new()
            .with_record_inputs(false)
            .with_record_outputs(true);
        let selected = select_telemetry_attributes(
            Some(&telemetry),
            [
                ("simple", AttributeSpec::value(json!("value"))),
                ("input", AttributeSpec::input(json!("input value"))),
                ("output", AttributeSpec::output(json!("output value"))),
                ("missing", AttributeSpec::Input(None)),
                ("null", AttributeSpec::value(JsonValue::Null)),
            ],
        );

        assert_eq!(
            selected,
            TelemetryAttributes::from([
                ("output".to_string(), json!("output value")),
                ("simple".to_string(), json!("value")),
            ])
        );
    }

    #[test]
    fn assemble_operation_name_includes_function_id_when_present() {
        let attributes = assemble_operation_name(
            "ai.generateText",
            Some(&TelemetryOptions::new().with_function_id("weather")),
        );

        assert_eq!(
            attributes,
            TelemetryAttributes::from([
                ("ai.operationId".to_string(), json!("ai.generateText")),
                ("ai.telemetry.functionId".to_string(), json!("weather")),
                (
                    "operation.name".to_string(),
                    json!("ai.generateText weather")
                ),
                ("resource.name".to_string(), json!("weather")),
            ])
        );
    }

    #[test]
    fn maps_provider_and_operation_names_to_genai_semconv_values() {
        assert_eq!(map_provider_name("openai.chat"), "openai");
        assert_eq!(map_provider_name("google.vertex.chat"), "gcp.vertex_ai");
        assert_eq!(map_provider_name("google.generative-ai"), "gcp.gemini");
        assert_eq!(map_provider_name("amazon-bedrock.chat"), "aws.bedrock");
        assert_eq!(map_provider_name("azure-openai.chat"), "azure.ai.openai");
        assert_eq!(map_provider_name("xai.chat"), "x_ai");
        assert_eq!(
            map_provider_name("custom-provider.chat"),
            "custom-provider.chat"
        );
        assert_eq!(map_operation_name("ai.streamText"), "invoke_agent");
        assert_eq!(map_operation_name("ai.embedMany"), "embeddings");
        assert_eq!(map_operation_name("ai.rerank"), "rerank");
        assert_eq!(map_operation_name("ai.unknown"), "ai.unknown");
    }

    #[test]
    fn formats_system_and_input_messages() {
        let prompt = vec![
            LanguageModelMessage::System(ai_sdk_provider::LanguageModelSystemMessage::new(
                "Be helpful",
            )),
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64("base64data".to_string()),
                    },
                    "image/png",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Url {
                        url: "https://example.com/image.png".parse().expect("url parses"),
                    },
                    "image/png",
                )),
            ])),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call_123",
                    "get_weather",
                    json!({ "city": "Paris" }),
                )),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call_123",
                    "get_weather",
                    LanguageModelToolResultOutput::text("Sunny"),
                )),
            ])),
        ];

        assert_eq!(
            format_system_instructions("Be helpful"),
            vec![SemConvSystemInstruction {
                kind: "text".to_string(),
                content: Some("Be helpful".to_string()),
            }]
        );
        assert_eq!(
            extract_system_from_prompt(&prompt),
            Some("Be helpful".to_string())
        );
        assert_eq!(
            serde_json::to_value(format_input_messages(&prompt)).expect("serializes"),
            json!([
                {
                    "role": "user",
                    "parts": [
                        { "type": "text", "content": "Hello" },
                        { "type": "blob", "modality": "image", "mime_type": "image/png", "content": "base64data" },
                        { "type": "uri", "modality": "image", "mime_type": "image/png", "uri": "https://example.com/image.png" }
                    ]
                },
                {
                    "role": "assistant",
                    "parts": [
                        { "type": "tool_call", "id": "call_123", "name": "get_weather", "arguments": { "city": "Paris" } }
                    ]
                },
                {
                    "role": "tool",
                    "parts": [
                        { "type": "tool_call_response", "id": "call_123", "response": "Sunny" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn formats_output_messages_and_finish_reasons() {
        let output = OutputMessages::new("tool-calls")
            .with_reasoning(OutputReasoning::new("Thinking"))
            .with_text("Here is the result")
            .with_tool_call(OutputToolCall::new("tc1", "search", json!({ "q": "test" })))
            .with_file(OutputFile::new("image/jpeg", "data"));

        assert_eq!(
            serde_json::to_value(format_output_messages(output)).expect("serializes"),
            json!([
                {
                    "role": "assistant",
                    "parts": [
                        { "type": "reasoning", "content": "Thinking" },
                        { "type": "text", "content": "Here is the result" },
                        { "type": "tool_call", "id": "tc1", "name": "search", "arguments": { "q": "test" } },
                        { "type": "blob", "modality": "image", "mime_type": "image/jpeg", "content": "data" }
                    ],
                    "finish_reason": "tool_call"
                }
            ])
        );

        assert_eq!(
            serde_json::to_value(format_object_output_messages(r#"{"name":"test"}"#, "stop"))
                .expect("serializes"),
            json!([
                {
                    "role": "assistant",
                    "parts": [{ "type": "text", "content": r#"{"name":"test"}"# }],
                    "finish_reason": "stop"
                }
            ])
        );
    }

    #[test]
    fn base_and_supplemental_attributes_match_upstream_prefixes() {
        let settings = TelemetryAttributes::from([
            ("temperature".to_string(), json!(0.7)),
            ("topP".to_string(), JsonValue::Null),
        ]);
        let headers = BTreeMap::from([("x-trace".to_string(), "trace-1".to_string())]);
        let context = TelemetryAttributes::from([("tenant".to_string(), json!("acme"))]);

        let base = get_base_telemetry_attributes(
            "openai.chat",
            "gpt-4",
            settings,
            Some(&headers),
            Some(&context),
        );
        assert_eq!(base.get("ai.model.provider"), Some(&json!("openai.chat")));
        assert_eq!(base.get("ai.model.id"), Some(&json!("gpt-4")));
        assert_eq!(base.get("ai.settings.temperature"), Some(&json!(0.7)));
        assert_eq!(base.get("ai.settings.context.tenant"), Some(&json!("acme")));
        assert_eq!(
            base.get("ai.request.headers.x-trace"),
            Some(&json!("trace-1"))
        );
        assert!(!base.contains_key("ai.settings.topP"));

        let enabled = SupplementalAttributeOptions {
            runtime_context: true,
            headers: false,
            ..SupplementalAttributeOptions::default()
        };
        let selected = select_supplemental_attributes(
            None,
            enabled,
            SupplementalAttributes {
                runtime_context: get_runtime_context_attributes(&context),
                headers: get_header_attributes(&headers),
                ..SupplementalAttributes::default()
            },
        );
        assert_eq!(
            selected,
            TelemetryAttributes::from([("ai.settings.context.tenant".to_string(), json!("acme"))])
        );
    }

    #[test]
    fn stringify_for_telemetry_converts_file_data_to_strings() {
        let provider_options: ProviderOptions =
            serde_json::from_value(json!({ "anthropic": { "key": "value" } }))
                .expect("provider options");
        let prompt = vec![
            LanguageModelMessage::System(ai_sdk_provider::LanguageModelSystemMessage::new(
                "You are helpful.",
            )),
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Check this image:")),
                LanguageModelUserContentPart::File(
                    LanguageModelFilePart::new(
                        FileData::Data {
                            data: FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47, 0xff, 0xff]),
                        },
                        "image/png",
                    )
                    .with_filename("image.png")
                    .with_provider_options(provider_options),
                ),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Url {
                        url: "https://example.com/image.jpg".parse().expect("url parses"),
                    },
                    "image/jpeg",
                )),
            ])),
        ];

        assert_eq!(
            stringify_for_telemetry(&prompt),
            r#"[{"content":"You are helpful.","role":"system"},{"content":[{"text":"Check this image:","type":"text"},{"data":"iVBOR///","filename":"image.png","mediaType":"image/png","providerOptions":{"anthropic":{"key":"value"}},"type":"file"},{"data":"https://example.com/image.jpg","mediaType":"image/jpeg","type":"file"}],"role":"user"}]"#
        );
    }

    #[test]
    fn record_span_executes_function_records_attributes_and_ends_by_default() {
        let mut tracer = MockTracer::new();
        let result = record_span(
            &mut tracer,
            "test-span",
            TelemetryAttributes::from([("key".to_string(), json!("value"))]),
            true,
            |span| {
                span.set_attribute("runtime", json!(true));
                Ok::<_, RecordSpanError>("test-result")
            },
        )
        .expect("span succeeds");

        assert_eq!(result, "test-result");
        assert_eq!(tracer.spans.len(), 1);
        assert_eq!(tracer.spans[0].name, "test-span");
        assert_eq!(tracer.spans[0].attributes.get("key"), Some(&json!("value")));
        assert_eq!(
            tracer.spans[0].attributes.get("runtime"),
            Some(&json!(true))
        );
        assert!(tracer.spans[0].ended);
    }

    #[test]
    fn record_span_can_leave_successful_span_open() {
        let mut tracer = MockTracer::new();
        record_span(
            &mut tracer,
            "test-span",
            TelemetryAttributes::new(),
            false,
            |_| Ok::<_, RecordSpanError>(()),
        )
        .expect("span succeeds");

        assert_eq!(tracer.spans.len(), 1);
        assert!(!tracer.spans[0].ended);
    }

    #[test]
    fn record_span_records_exception_status_and_ends_on_error() {
        let mut tracer = MockTracer::new();
        let error = record_span(
            &mut tracer,
            "test-span",
            TelemetryAttributes::new(),
            true,
            |_| Err::<(), _>(RecordSpanError::exception("Error", "Test error")),
        )
        .expect_err("span fails");

        assert_eq!(error.message(), Some("Test error"));
        assert_eq!(tracer.spans.len(), 1);
        assert!(tracer.spans[0].ended);
        assert_eq!(
            tracer.spans[0].status,
            Some(SpanStatus::error(Some("Test error".to_string())))
        );
        assert_eq!(tracer.spans[0].events.len(), 1);
        assert_eq!(tracer.spans[0].events[0].name, "exception");
    }

    #[test]
    fn record_error_on_span_sets_status_only_for_non_error_values() {
        let mut span = MockSpan::new("test-span", TelemetryAttributes::new());
        record_error_on_span(&mut span, RecordSpanError::StatusOnly);

        assert_eq!(span.status, Some(SpanStatus::error(None)));
        assert!(span.events.is_empty());
    }

    #[test]
    fn mock_and_noop_tracers_match_upstream_test_shapes() {
        let mut tracer = MockTracer::new();
        tracer.start_active_span(
            "active",
            TelemetryAttributes::from([("start".to_string(), json!(1))]),
            |span| {
                span.add_event(
                    "event",
                    Some(TelemetryAttributes::from([("value".to_string(), json!(2))])),
                );
            },
        );

        assert_eq!(
            tracer.json_spans(),
            vec![json!({
                "name": "active",
                "attributes": { "start": 1 },
                "events": [
                    { "name": "event", "attributes": { "value": 2 } }
                ]
            })]
        );

        let result = NoopTracer.start_active_span("ignored", TelemetryAttributes::new(), |span| {
            assert_eq!(span.name, "ignored");
            "noop-result"
        });
        assert_eq!(result, "noop-result");
    }

    #[test]
    fn open_telemetry_records_generate_text_root_step_and_chat_spans() {
        let mut recorder = OpenTelemetry::new(OpenTelemetryOptions::new());
        recorder.on_start(
            OpenTelemetryStartEvent::new("call-1", "ai.generateText", "openai.chat", "gpt-4o-mini")
                .with_telemetry(TelemetryOptions::new().with_function_id("weather"))
                .with_settings(TelemetryAttributes::from([(
                    "temperature".to_string(),
                    json!(0.2),
                )]))
                .with_system_instructions(format_system_instructions("Be concise"))
                .with_input_messages(vec![SemConvMessage::input(
                    "user",
                    vec![json!({ "type": "text", "content": "Weather?" })],
                )]),
        );
        recorder.on_step_start(OpenTelemetryStepStartEvent::new("call-1", 1));
        recorder.on_language_model_call_start(OpenTelemetryLanguageModelCallStartEvent::new(
            "call-1",
            "openai.chat",
            "gpt-4o-mini",
        ));
        recorder.on_language_model_call_end(
            OpenTelemetryLanguageModelCallEndEvent::new("call-1", "stop").with_usage(
                TelemetryTokenUsage {
                    input_tokens: Some(7),
                    output_tokens: Some(4),
                    total_tokens: Some(11),
                },
            ),
        );
        recorder.on_step_finish("call-1");
        recorder.on_end(
            OpenTelemetryEndEvent::new("call-1", "stop").with_output_messages(vec![
                SemConvMessage::output(
                    vec![json!({ "type": "text", "content": "Sunny." })],
                    "stop",
                ),
            ]),
        );

        assert_eq!(recorder.active_call_count(), 0);
        let tracer = recorder.into_tracer();
        assert_eq!(tracer.spans.len(), 3);
        assert_eq!(tracer.spans[0].name, "invoke_agent gpt-4o-mini");
        assert_eq!(tracer.spans[1].name, "step 1");
        assert_eq!(tracer.spans[2].name, "chat gpt-4o-mini");
        assert!(tracer.spans.iter().all(|span| span.ended));
        assert_eq!(
            tracer.spans[0].attributes.get("gen_ai.operation.name"),
            Some(&json!("invoke_agent"))
        );
        assert_eq!(
            tracer.spans[0].attributes.get("gen_ai.provider.name"),
            Some(&json!("openai"))
        );
        assert_eq!(
            tracer.spans[0].attributes.get("gen_ai.request.temperature"),
            Some(&json!(0.2))
        );
        assert_eq!(
            tracer.spans[2].attributes.get("gen_ai.usage.total_tokens"),
            Some(&json!(11))
        );
        assert!(
            tracer
                .spans
                .iter()
                .all(|span| span.attributes.keys().all(|key| !key.starts_with("ai.")))
        );
    }

    #[test]
    fn open_telemetry_records_object_operation_and_step_spans() {
        let mut recorder =
            OpenTelemetry::new(OpenTelemetryOptions::new().with_supplemental_attributes(
                SupplementalAttributeOptions {
                    schema: true,
                    ..SupplementalAttributeOptions::default()
                },
            ));
        let input_messages = vec![SemConvMessage::input(
            "user",
            vec![json!({ "type": "text", "content": "Return a short answer." })],
        )];
        let usage = TelemetryTokenUsage {
            input_tokens: Some(5),
            output_tokens: Some(7),
            total_tokens: Some(12),
        };

        recorder.on_object_operation_start(
            OpenTelemetryObjectStartEvent::new(
                "object-call",
                "ai.generateObject",
                "openai.chat",
                "gpt-4o-mini",
            )
            .with_telemetry(TelemetryOptions::new().with_function_id("answer"))
            .with_settings(TelemetryAttributes::from([(
                "temperature".to_string(),
                json!(0.1),
            )]))
            .with_system_instructions(format_system_instructions("Return JSON only."))
            .with_input_messages(input_messages.clone())
            .with_schema(
                json!({
                    "type": "object",
                    "properties": {
                        "answer": { "type": "string" }
                    }
                }),
                "Answer",
                "Answer schema",
            )
            .with_output_mode("object"),
        );
        recorder.on_object_step_start(
            OpenTelemetryObjectStepStartEvent::new("object-call", "openai.chat", "gpt-4o-mini")
                .with_settings(TelemetryAttributes::from([(
                    "temperature".to_string(),
                    json!(0.1),
                )]))
                .with_input_messages(input_messages),
        );
        recorder.on_object_step_finish(
            OpenTelemetryObjectStepFinishEvent::new("object-call", "stop")
                .with_usage(usage)
                .with_object_text(r#"{"answer":"ok"}"#),
        );
        recorder.on_object_operation_end(
            OpenTelemetryObjectEndEvent::new("object-call", "stop")
                .with_usage(usage)
                .with_object(json!({ "answer": "ok" })),
        );

        assert_eq!(recorder.active_call_count(), 0);
        let tracer = recorder.into_tracer();
        assert_eq!(tracer.spans.len(), 2);
        assert_eq!(tracer.spans[0].name, "invoke_agent gpt-4o-mini");
        assert_eq!(tracer.spans[1].name, "chat gpt-4o-mini");
        assert!(tracer.spans.iter().all(|span| span.ended));
        assert_eq!(
            tracer.spans[0].attributes.get("gen_ai.output.type"),
            Some(&json!("json"))
        );
        assert_eq!(
            tracer.spans[0].attributes.get("gen_ai.request.temperature"),
            Some(&json!(0.1))
        );
        assert_eq!(
            tracer.spans[0].attributes.get("ai.schema.name"),
            Some(&json!("Answer"))
        );
        assert_eq!(
            tracer.spans[0].attributes.get("ai.schema.description"),
            Some(&json!("Answer schema"))
        );
        assert_eq!(
            tracer.spans[0].attributes.get("ai.settings.output"),
            Some(&json!("object"))
        );
        assert_eq!(
            tracer.spans[0]
                .attributes
                .get("gen_ai.output.messages")
                .and_then(JsonValue::as_array)
                .and_then(|messages| messages.first())
                .and_then(|message| message.get("parts"))
                .and_then(JsonValue::as_array)
                .and_then(|parts| parts.first())
                .and_then(|part| part.get("content")),
            Some(&json!("{\"answer\":\"ok\"}"))
        );
        assert_eq!(
            tracer.spans[1].attributes.get("gen_ai.operation.name"),
            Some(&json!("chat"))
        );
        assert_eq!(
            tracer.spans[1].attributes.get("gen_ai.output.type"),
            Some(&json!("json"))
        );
        assert_eq!(
            tracer.spans[1].attributes.get("gen_ai.usage.total_tokens"),
            Some(&json!(12))
        );
    }

    #[test]
    fn open_telemetry_enrichment_keeps_official_attribute_precedence() {
        let mut recorder =
            OpenTelemetry::new(OpenTelemetryOptions::new().with_enrich_span(|options| {
                assert_eq!(options.span_type, OpenTelemetrySpanType::Operation);
                TelemetryAttributes::from([
                    ("custom.tenant".to_string(), json!("acme")),
                    ("gen_ai.provider.name".to_string(), json!("wrong")),
                ])
            }));

        recorder.on_start(OpenTelemetryStartEvent::new(
            "call-1",
            "ai.generateText",
            "openai.chat",
            "gpt-4o-mini",
        ));

        assert_eq!(
            recorder.tracer().spans[0].attributes.get("custom.tenant"),
            Some(&json!("acme"))
        );
        assert_eq!(
            recorder.tracer().spans[0]
                .attributes
                .get("gen_ai.provider.name"),
            Some(&json!("openai"))
        );
    }

    #[test]
    fn open_telemetry_records_tool_span_and_wraps_execute_tool() {
        let mut recorder = OpenTelemetry::new(OpenTelemetryOptions::new());
        recorder.on_start(OpenTelemetryStartEvent::new(
            "call-1",
            "ai.generateText",
            "openai.chat",
            "gpt-4o-mini",
        ));
        recorder.on_tool_execution_start(OpenTelemetryToolExecutionStartEvent::new(
            "call-1", "tool-1", "weather",
        ));

        let output = recorder.execute_tool("call-1", "tool-1", |span| {
            span.expect("tool span is active")
                .set_attribute("custom.executed", json!(true));
            json!({ "temperature": 24 })
        });
        recorder.on_tool_execution_end(
            OpenTelemetryToolExecutionEndEvent::new("call-1", "tool-1").with_output(output),
        );
        recorder.on_end(OpenTelemetryEndEvent::new("call-1", "stop"));

        let tracer = recorder.into_tracer();
        let tool_span = tracer
            .spans
            .iter()
            .find(|span| span.name == "execute_tool weather")
            .expect("tool span is recorded");
        assert!(tool_span.ended);
        assert_eq!(
            tool_span.attributes.get("gen_ai.tool.name"),
            Some(&json!("weather"))
        );
        assert_eq!(
            tool_span.attributes.get("custom.executed"),
            Some(&json!(true))
        );
        assert_eq!(
            tool_span.attributes.get("gen_ai.tool.output"),
            Some(&json!({ "temperature": 24 }))
        );
    }

    #[test]
    fn open_telemetry_records_error_on_active_spans_and_cleans_state() {
        let mut recorder = OpenTelemetry::new(OpenTelemetryOptions::new());
        recorder.on_start(OpenTelemetryStartEvent::new(
            "call-1",
            "ai.generateText",
            "openai.chat",
            "gpt-4o-mini",
        ));
        recorder.on_step_start(OpenTelemetryStepStartEvent::new("call-1", 1));
        recorder.on_language_model_call_start(OpenTelemetryLanguageModelCallStartEvent::new(
            "call-1",
            "openai.chat",
            "gpt-4o-mini",
        ));
        recorder.on_tool_execution_start(OpenTelemetryToolExecutionStartEvent::new(
            "call-1", "tool-1", "weather",
        ));

        recorder.on_error(OpenTelemetryErrorEvent::new(
            "call-1",
            RecordSpanError::exception("Error", "provider failed"),
        ));

        assert_eq!(recorder.active_call_count(), 0);
        let tracer = recorder.into_tracer();
        assert_eq!(tracer.spans.len(), 4);
        assert!(tracer.spans.iter().all(|span| span.ended));
        assert!(tracer.spans.iter().all(|span| {
            span.status == Some(SpanStatus::error(Some("provider failed".to_string())))
                && span.events.iter().any(|event| event.name == "exception")
        }));
    }

    #[test]
    fn open_telemetry_records_embedding_operation_and_inner_span() {
        let mut recorder =
            OpenTelemetry::new(OpenTelemetryOptions::new().with_supplemental_attributes(
                SupplementalAttributeOptions {
                    embedding: true,
                    ..SupplementalAttributeOptions::default()
                },
            ));

        recorder.on_embed_operation_start(OpenTelemetryEmbedStartEvent::new(
            "embed-call",
            "ai.embedMany",
            "openai.embedding",
            "text-embedding-3-small",
            OpenTelemetryEmbeddingInput::many(["alpha", "beta"]),
        ));
        recorder.on_embedding_model_call_start(OpenTelemetryEmbeddingModelCallStartEvent::new(
            "embed-call",
            "inner-embed-1",
            ["alpha", "beta"],
        ));
        recorder.on_embedding_model_call_end(
            OpenTelemetryEmbeddingModelCallEndEvent::new(
                "embed-call",
                "inner-embed-1",
                [vec![0.1, 0.2], vec![0.3, 0.4]],
            )
            .with_usage(OpenTelemetryEmbeddingUsage { tokens: Some(6) }),
        );
        recorder.on_embed_operation_end(
            OpenTelemetryEmbedEndEvent::new(
                "embed-call",
                OpenTelemetryEmbeddingOutput::many([vec![0.1, 0.2], vec![0.3, 0.4]]),
            )
            .with_usage(OpenTelemetryEmbeddingUsage { tokens: Some(6) }),
        );

        assert_eq!(recorder.active_call_count(), 0);
        let tracer = recorder.into_tracer();
        assert_eq!(tracer.spans.len(), 2);
        assert_eq!(tracer.spans[0].name, "embeddings text-embedding-3-small");
        assert_eq!(tracer.spans[1].name, "embeddings text-embedding-3-small");
        assert!(tracer.spans.iter().all(|span| span.ended));
        assert_eq!(
            tracer.spans[0].attributes.get("gen_ai.operation.name"),
            Some(&json!("embeddings"))
        );
        assert_eq!(
            tracer.spans[0].attributes.get("gen_ai.provider.name"),
            Some(&json!("openai"))
        );
        assert_eq!(
            tracer.spans[0].attributes.get("ai.values"),
            Some(&json!(["\"alpha\"", "\"beta\""]))
        );
        assert_eq!(
            tracer.spans[0].attributes.get("ai.embeddings"),
            Some(&json!(["[0.1,0.2]", "[0.3,0.4]"]))
        );
        assert_eq!(
            tracer.spans[1].attributes.get("gen_ai.usage.input_tokens"),
            Some(&json!(6))
        );
    }

    #[test]
    fn open_telemetry_records_rerank_operation_and_inner_span() {
        let documents = vec![json!("alpha"), json!({ "id": "beta" })];
        let ranking = vec![json!({ "index": 1, "score": 0.9 })];
        let mut recorder =
            OpenTelemetry::new(OpenTelemetryOptions::new().with_supplemental_attributes(
                SupplementalAttributeOptions {
                    reranking: true,
                    ..SupplementalAttributeOptions::default()
                },
            ));

        recorder.on_rerank_operation_start(OpenTelemetryRerankStartEvent::new(
            "rerank-call",
            "cohere.rerank",
            "rerank-v3.5",
            documents.clone(),
        ));
        recorder.on_reranking_model_call_start(OpenTelemetryRerankingModelCallStartEvent::new(
            "rerank-call",
            "object",
            documents,
        ));
        recorder.on_reranking_model_call_end(OpenTelemetryRerankingModelCallEndEvent::new(
            "rerank-call",
            "object",
            ranking,
        ));
        recorder.on_rerank_operation_end(OpenTelemetryRerankEndEvent::new("rerank-call"));

        assert_eq!(recorder.active_call_count(), 0);
        let tracer = recorder.into_tracer();
        assert_eq!(tracer.spans.len(), 2);
        assert_eq!(tracer.spans[0].name, "rerank rerank-v3.5");
        assert_eq!(tracer.spans[1].name, "rerank rerank-v3.5");
        assert!(tracer.spans.iter().all(|span| span.ended));
        assert_eq!(
            tracer.spans[0].attributes.get("gen_ai.operation.name"),
            Some(&json!("rerank"))
        );
        assert_eq!(
            tracer.spans[0].attributes.get("gen_ai.provider.name"),
            Some(&json!("cohere"))
        );
        assert_eq!(
            tracer.spans[0].attributes.get("ai.documents"),
            Some(&json!(["\"alpha\"", "{\"id\":\"beta\"}"]))
        );
        assert_eq!(
            tracer.spans[1].attributes.get("ai.ranking.type"),
            Some(&json!("object"))
        );
        assert_eq!(
            tracer.spans[1].attributes.get("ai.ranking"),
            Some(&json!(["{\"index\":1,\"score\":0.9}"]))
        );
    }

    #[test]
    fn otlp_http_json_payload_uses_collector_shape() {
        let mut tracer = MockTracer::new();
        let span = tracer.start_span(
            "chat gpt-4o-mini",
            TelemetryAttributes::from([
                ("gen_ai.provider.name".to_string(), json!("openai")),
                ("gen_ai.usage.total_tokens".to_string(), json!(11)),
            ]),
        );
        tracer.spans[span].end();

        let payload = build_otlp_http_trace_json(
            &tracer,
            &OtlpHttpTraceExportOptions::new("http://127.0.0.1:4318/v1/traces")
                .with_service_name("otel-test")
                .with_resource_attribute("deployment.environment.name", json!("test")),
        );

        assert_eq!(
            payload["resourceSpans"][0]["resource"]["attributes"][0]["key"],
            "deployment.environment.name"
        );
        assert_eq!(
            payload["resourceSpans"][0]["scopeSpans"][0]["scope"]["name"],
            "ai-sdk-otel"
        );
        assert_eq!(
            payload["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["name"],
            "chat gpt-4o-mini"
        );
        let attributes = payload["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["attributes"]
            .as_array()
            .expect("span attributes are present");
        assert!(attributes.iter().any(|attribute| {
            attribute["key"] == "gen_ai.provider.name"
                && attribute["value"]["stringValue"] == "openai"
        }));
        assert!(attributes.iter().any(|attribute| {
            attribute["key"] == "gen_ai.usage.total_tokens"
                && attribute["value"]["intValue"] == "11"
        }));
    }

    #[test]
    fn local_otlp_http_receiver_captures_exported_span_payload() {
        let receiver = LocalOtlpTraceReceiver::start().expect("OTLP receiver starts");
        let mut recorder = OpenTelemetry::new(OpenTelemetryOptions::new());
        recorder.on_start(OpenTelemetryStartEvent::new(
            "call-1",
            "ai.generateText",
            "openai.chat",
            "gpt-4o-mini",
        ));
        recorder.on_end(OpenTelemetryEndEvent::new("call-1", "stop"));
        let tracer = recorder.into_tracer();

        export_tracer_to_otlp_http_json(
            &tracer,
            &OtlpHttpTraceExportOptions::new(receiver.endpoint())
                .with_service_name("ai-sdk-rust-local-otel"),
        )
        .expect("export succeeds");

        let requests = receiver.wait_for_requests(1, std::time::Duration::from_secs(2));
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/traces");
        assert_eq!(
            request.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );

        let body = request.body_json().expect("OTLP body is JSON");
        assert_eq!(
            body["resourceSpans"][0]["resource"]["attributes"][0]["value"]["stringValue"],
            "ai-sdk-rust-local-otel"
        );
        assert_eq!(
            body["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["name"],
            "invoke_agent gpt-4o-mini"
        );
        let attributes = body["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["attributes"]
            .as_array()
            .expect("span attributes are present");
        assert!(attributes.iter().any(|attribute| {
            attribute["key"] == "gen_ai.provider.name"
                && attribute["value"]["stringValue"] == "openai"
        }));
    }

    #[cfg(feature = "real-opentelemetry")]
    #[test]
    fn real_opentelemetry_http_exporter_sends_json_to_local_receiver() {
        let receiver = LocalOtlpTraceReceiver::start().expect("OTLP receiver starts");

        export_real_opentelemetry_span_to_otlp_http_json(
            &OtlpHttpTraceExportOptions::new(receiver.endpoint())
                .with_service_name("ai-sdk-rust-real-otel")
                .with_scope_name("ai-sdk-otel-real-test")
                .with_resource_attribute("deployment.environment.name", json!("test")),
            "invoke_agent gpt-4o-mini",
            TelemetryAttributes::from([
                ("gen_ai.provider.name".to_string(), json!("openai")),
                ("gen_ai.request.model".to_string(), json!("gpt-4o-mini")),
                ("ai.operationId".to_string(), json!("ai.generateText")),
            ]),
        )
        .expect("real OpenTelemetry export succeeds");

        let requests = receiver.wait_for_requests(1, std::time::Duration::from_secs(2));
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/traces");
        assert_eq!(
            request.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );

        let body = request.body_json().expect("real OTLP body is JSON");
        let resource_attributes = body["resourceSpans"][0]["resource"]["attributes"]
            .as_array()
            .expect("resource attributes are present");
        assert!(otlp_has_string_attribute(
            resource_attributes,
            "service.name",
            "ai-sdk-rust-real-otel"
        ));
        assert!(otlp_has_string_attribute(
            resource_attributes,
            "deployment.environment.name",
            "test"
        ));
        assert_eq!(
            body["resourceSpans"][0]["scopeSpans"][0]["scope"]["name"],
            "ai-sdk-otel-real-test"
        );
        assert_eq!(
            body["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["name"],
            "invoke_agent gpt-4o-mini"
        );
        let span_attributes = body["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["attributes"]
            .as_array()
            .expect("span attributes are present");
        assert!(otlp_has_string_attribute(
            span_attributes,
            "gen_ai.provider.name",
            "openai"
        ));
        assert!(otlp_has_string_attribute(
            span_attributes,
            "gen_ai.request.model",
            "gpt-4o-mini"
        ));
        assert!(otlp_has_string_attribute(
            span_attributes,
            "ai.operationId",
            "ai.generateText"
        ));
    }

    #[cfg(feature = "real-opentelemetry")]
    fn otlp_has_string_attribute(attributes: &[JsonValue], key: &str, value: &str) -> bool {
        attributes
            .iter()
            .any(|attribute| attribute["key"] == key && attribute["value"]["stringValue"] == value)
    }
}
