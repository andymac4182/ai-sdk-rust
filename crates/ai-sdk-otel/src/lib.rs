//! Portable OpenTelemetry helpers for the Rust port of upstream `@ai-sdk/otel`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

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
}
