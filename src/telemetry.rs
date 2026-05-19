use std::collections::BTreeMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use serde::{Deserialize, Serialize};

use crate::json::JsonValue;
use crate::language_model::LanguageModelPrompt;

/// Recorder handle accepted by the OpenTelemetry integration adapter.
pub type OpenTelemetryRecorder = Arc<Mutex<ai_sdk_otel::OpenTelemetry>>;

/// Recorder handle accepted by the legacy OpenTelemetry integration adapter.
pub type LegacyOpenTelemetryRecorder = Arc<Mutex<ai_sdk_otel::LegacyOpenTelemetry>>;

/// Diagnostic channel name used by upstream AI SDK telemetry.
pub const AI_SDK_TELEMETRY_DIAGNOSTIC_CHANNEL: &str = "aisdk:telemetry";

/// Telemetry lifecycle callback names.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TelemetryEventKind {
    OnStart,
    OnStepStart,
    OnLanguageModelCallStart,
    OnLanguageModelCallEnd,
    OnToolExecutionStart,
    OnToolExecutionEnd,
    OnStepFinish,
    OnObjectStepStart,
    OnObjectStepFinish,
    OnEmbedStart,
    OnEmbedEnd,
    OnRerankStart,
    OnRerankEnd,
    OnEnd,
    OnError,
}

impl TelemetryEventKind {
    /// Returns the upstream callback name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OnStart => "onStart",
            Self::OnStepStart => "onStepStart",
            Self::OnLanguageModelCallStart => "onLanguageModelCallStart",
            Self::OnLanguageModelCallEnd => "onLanguageModelCallEnd",
            Self::OnToolExecutionStart => "onToolExecutionStart",
            Self::OnToolExecutionEnd => "onToolExecutionEnd",
            Self::OnStepFinish => "onStepFinish",
            Self::OnObjectStepStart => "onObjectStepStart",
            Self::OnObjectStepFinish => "onObjectStepFinish",
            Self::OnEmbedStart => "onEmbedStart",
            Self::OnEmbedEnd => "onEmbedEnd",
            Self::OnRerankStart => "onRerankStart",
            Self::OnRerankEnd => "onRerankEnd",
            Self::OnEnd => "onEnd",
            Self::OnError => "onError",
        }
    }
}

/// Telemetry event delivered to integrations and diagnostic subscribers.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryEvent {
    /// Lifecycle callback that produced this event.
    pub kind: TelemetryEventKind,

    /// Original event payload, serialized from the high-level SDK event type.
    pub event: JsonValue,

    /// Whether input recording is enabled for this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_inputs: Option<bool>,

    /// Whether output recording is enabled for this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_outputs: Option<bool>,

    /// Optional user-provided function id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_id: Option<String>,
}

impl TelemetryEvent {
    fn new(kind: TelemetryEventKind, event: JsonValue, metadata: &TelemetryMetadata) -> Self {
        Self {
            kind,
            event,
            record_inputs: metadata.record_inputs,
            record_outputs: metadata.record_outputs,
            function_id: metadata.function_id.clone(),
        }
    }
}

/// Message published to the Rust diagnostic telemetry channel.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryDiagnosticMessage {
    /// Diagnostic channel type, matching the upstream callback name.
    pub kind: TelemetryEventKind,

    /// Augmented event payload.
    pub event: TelemetryEvent,
}

type TelemetryCallback = Arc<dyn Fn(TelemetryEvent) + Send + Sync>;
type TelemetryDiagnosticCallback = Arc<dyn Fn(TelemetryDiagnosticMessage) + Send + Sync>;
type TelemetryExecuteToolCallback =
    Arc<dyn Fn(TelemetryExecuteToolOptions) -> JsonValue + Send + Sync>;

/// Options passed to telemetry execute-tool wrappers.
pub struct TelemetryExecuteToolOptions {
    pub call_id: String,
    pub tool_call_id: String,
    pub execute: Box<dyn FnOnce() -> JsonValue + Send>,
}

impl fmt::Debug for TelemetryExecuteToolOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TelemetryExecuteToolOptions")
            .field("call_id", &self.call_id)
            .field("tool_call_id", &self.tool_call_id)
            .finish_non_exhaustive()
    }
}

/// Custom telemetry integration callbacks.
#[derive(Clone, Default)]
pub struct TelemetryIntegration {
    callbacks: BTreeMap<TelemetryEventKind, TelemetryCallback>,
    execute_tool: Option<TelemetryExecuteToolCallback>,
}

impl fmt::Debug for TelemetryIntegration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TelemetryIntegration")
            .field("callbacks", &self.callbacks.keys().collect::<Vec<_>>())
            .field("execute_tool", &self.execute_tool.is_some())
            .finish()
    }
}

impl TelemetryIntegration {
    /// Creates an empty telemetry integration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a lifecycle callback.
    pub fn with_callback(
        mut self,
        kind: TelemetryEventKind,
        callback: impl Fn(TelemetryEvent) + Send + Sync + 'static,
    ) -> Self {
        self.callbacks.insert(kind, Arc::new(callback));
        self
    }

    /// Registers a tool execution wrapper.
    pub fn with_execute_tool(
        mut self,
        execute_tool: impl Fn(TelemetryExecuteToolOptions) -> JsonValue + Send + Sync + 'static,
    ) -> Self {
        self.execute_tool = Some(Arc::new(execute_tool));
        self
    }

    fn callback(&self, kind: TelemetryEventKind) -> Option<TelemetryCallback> {
        self.callbacks.get(&kind).cloned()
    }

    fn execute_tool(&self) -> Option<TelemetryExecuteToolCallback> {
        self.execute_tool.clone()
    }
}

/// Creates a root telemetry integration backed by `ai-sdk-otel`.
///
/// The returned integration translates dispatcher lifecycle events into the
/// package-owned OpenTelemetry recorder. The recorder can then be exported with
/// `ai_sdk_otel::export_tracer_to_otlp_http_json` or inspected directly in
/// tests.
pub fn create_open_telemetry_integration(recorder: OpenTelemetryRecorder) -> TelemetryIntegration {
    let mut integration = TelemetryIntegration::new();
    for kind in [
        TelemetryEventKind::OnStart,
        TelemetryEventKind::OnStepStart,
        TelemetryEventKind::OnLanguageModelCallStart,
        TelemetryEventKind::OnLanguageModelCallEnd,
        TelemetryEventKind::OnToolExecutionStart,
        TelemetryEventKind::OnToolExecutionEnd,
        TelemetryEventKind::OnStepFinish,
        TelemetryEventKind::OnObjectStepStart,
        TelemetryEventKind::OnObjectStepFinish,
        TelemetryEventKind::OnEmbedStart,
        TelemetryEventKind::OnEmbedEnd,
        TelemetryEventKind::OnRerankStart,
        TelemetryEventKind::OnRerankEnd,
        TelemetryEventKind::OnEnd,
        TelemetryEventKind::OnError,
    ] {
        let recorder = Arc::clone(&recorder);
        integration = integration.with_callback(kind, move |event| {
            dispatch_to_open_telemetry(&recorder, event);
        });
    }

    let execute_recorder = Arc::clone(&recorder);
    integration.with_execute_tool(move |options| {
        let call_id = options.call_id;
        let tool_call_id = options.tool_call_id;
        let execute = options.execute;
        match execute_recorder.lock() {
            Ok(mut recorder) => recorder.execute_tool(&call_id, &tool_call_id, |_| execute()),
            Err(_) => execute(),
        }
    })
}

/// Creates a root telemetry integration backed by `ai-sdk-otel`'s legacy
/// OpenTelemetry recorder.
pub fn create_legacy_open_telemetry_integration(
    recorder: LegacyOpenTelemetryRecorder,
) -> TelemetryIntegration {
    let mut integration = TelemetryIntegration::new();
    for kind in [
        TelemetryEventKind::OnStart,
        TelemetryEventKind::OnStepStart,
        TelemetryEventKind::OnLanguageModelCallStart,
        TelemetryEventKind::OnLanguageModelCallEnd,
        TelemetryEventKind::OnToolExecutionStart,
        TelemetryEventKind::OnToolExecutionEnd,
        TelemetryEventKind::OnStepFinish,
        TelemetryEventKind::OnObjectStepStart,
        TelemetryEventKind::OnObjectStepFinish,
        TelemetryEventKind::OnEmbedStart,
        TelemetryEventKind::OnEmbedEnd,
        TelemetryEventKind::OnRerankStart,
        TelemetryEventKind::OnRerankEnd,
        TelemetryEventKind::OnEnd,
        TelemetryEventKind::OnError,
    ] {
        let recorder = Arc::clone(&recorder);
        integration = integration.with_callback(kind, move |event| {
            dispatch_to_legacy_open_telemetry(&recorder, event);
        });
    }

    let execute_recorder = Arc::clone(&recorder);
    integration.with_execute_tool(move |options| {
        let call_id = options.call_id;
        let tool_call_id = options.tool_call_id;
        let execute = options.execute;
        match execute_recorder.lock() {
            Ok(mut recorder) => recorder.execute_tool(&call_id, &tool_call_id, |_| execute()),
            Err(_) => execute(),
        }
    })
}

fn dispatch_to_open_telemetry(recorder: &OpenTelemetryRecorder, event: TelemetryEvent) {
    let Ok(mut recorder) = recorder.lock() else {
        return;
    };

    match event.kind {
        TelemetryEventKind::OnStart => {
            if is_object_operation(&event.event) {
                if let Some(start) = open_telemetry_object_start_event(&event) {
                    recorder.on_object_operation_start(start);
                }
            } else if let Some(start) = open_telemetry_start_event(&event) {
                recorder.on_start(start);
            }
        }
        TelemetryEventKind::OnStepStart => {
            if let Some(start) = open_telemetry_step_start_event(&event.event) {
                recorder.on_step_start(start);
            }
        }
        TelemetryEventKind::OnLanguageModelCallStart => {
            if let Some(start) = open_telemetry_language_model_call_start_event(&event.event) {
                recorder.on_language_model_call_start(start);
            }
        }
        TelemetryEventKind::OnLanguageModelCallEnd => {
            if let Some(end) = open_telemetry_language_model_call_end_event(&event.event) {
                recorder.on_language_model_call_end(end);
            }
        }
        TelemetryEventKind::OnToolExecutionStart => {
            if let Some(start) = open_telemetry_tool_execution_start_event(&event.event) {
                recorder.on_tool_execution_start(start);
            }
        }
        TelemetryEventKind::OnToolExecutionEnd => {
            if let Some(end) = open_telemetry_tool_execution_end_event(&event.event) {
                recorder.on_tool_execution_end(end);
            }
        }
        TelemetryEventKind::OnStepFinish => {
            if let Some(call_id) = string_field(&event.event, "callId") {
                recorder.on_step_finish(&call_id);
            }
        }
        TelemetryEventKind::OnObjectStepStart => {
            if let Some(start) = open_telemetry_object_step_start_event(&event.event) {
                recorder.on_object_step_start(start);
            }
        }
        TelemetryEventKind::OnObjectStepFinish => {
            if let Some(finish) = open_telemetry_object_step_finish_event(&event.event) {
                recorder.on_object_step_finish(finish);
            }
        }
        TelemetryEventKind::OnEmbedStart => {
            if let Some(start) = open_telemetry_embed_start_event(&event) {
                recorder.on_embed_operation_start(start);
            }
        }
        TelemetryEventKind::OnEmbedEnd => {
            if let Some(end) = open_telemetry_embed_end_event(&event.event) {
                recorder.on_embed_operation_end(end);
            }
        }
        TelemetryEventKind::OnRerankStart => {
            if let Some(start) = open_telemetry_rerank_start_event(&event) {
                recorder.on_rerank_operation_start(start);
            }
        }
        TelemetryEventKind::OnRerankEnd => {
            if let Some(call_id) = string_field(&event.event, "callId") {
                recorder.on_rerank_operation_end(ai_sdk_otel::OpenTelemetryRerankEndEvent::new(
                    call_id,
                ));
            }
        }
        TelemetryEventKind::OnEnd => {
            if event.event.get("object").is_some() || event.event.get("error").is_some() {
                if let Some(end) = open_telemetry_object_end_event(&event.event) {
                    recorder.on_object_operation_end(end);
                }
            } else if let Some(end) = open_telemetry_end_event(&event.event) {
                recorder.on_end(end);
            }
        }
        TelemetryEventKind::OnError => {
            if let Some(error) = open_telemetry_error_event(&event.event) {
                recorder.on_error(error);
            }
        }
    }
}

fn dispatch_to_legacy_open_telemetry(
    recorder: &LegacyOpenTelemetryRecorder,
    event: TelemetryEvent,
) {
    let Ok(mut recorder) = recorder.lock() else {
        return;
    };

    match event.kind {
        TelemetryEventKind::OnStart => {
            if is_object_operation(&event.event) {
                if let Some(start) = open_telemetry_object_start_event(&event) {
                    recorder.on_object_operation_start(start);
                }
            } else if let Some(start) = open_telemetry_start_event(&event) {
                recorder.on_start(start);
            }
        }
        TelemetryEventKind::OnStepStart => {
            if let Some(start) = open_telemetry_step_start_event(&event.event) {
                recorder.on_step_start(start);
            }
        }
        TelemetryEventKind::OnLanguageModelCallStart => {
            if let Some(start) = open_telemetry_language_model_call_start_event(&event.event) {
                recorder.on_language_model_call_start(start);
            }
        }
        TelemetryEventKind::OnLanguageModelCallEnd => {
            if let Some(end) = open_telemetry_language_model_call_end_event(&event.event) {
                recorder.on_language_model_call_end(end);
            }
        }
        TelemetryEventKind::OnToolExecutionStart => {
            if let Some(start) = open_telemetry_tool_execution_start_event(&event.event) {
                recorder.on_tool_execution_start(start);
            }
        }
        TelemetryEventKind::OnToolExecutionEnd => {
            if let Some(end) = open_telemetry_tool_execution_end_event(&event.event) {
                recorder.on_tool_execution_end(end);
            }
        }
        TelemetryEventKind::OnStepFinish => {
            if let Some(call_id) = string_field(&event.event, "callId") {
                recorder.on_step_finish(&call_id);
            }
        }
        TelemetryEventKind::OnObjectStepStart => {
            if let Some(start) = open_telemetry_object_step_start_event(&event.event) {
                recorder.on_object_step_start(start);
            }
        }
        TelemetryEventKind::OnObjectStepFinish => {
            if let Some(finish) = open_telemetry_object_step_finish_event(&event.event) {
                recorder.on_object_step_finish(finish);
            }
        }
        TelemetryEventKind::OnEmbedStart => {
            if let Some(start) = open_telemetry_embed_start_event(&event) {
                recorder.on_embed_operation_start(start);
            }
        }
        TelemetryEventKind::OnEmbedEnd => {
            if let Some(end) = open_telemetry_embed_end_event(&event.event) {
                recorder.on_embed_operation_end(end);
            }
        }
        TelemetryEventKind::OnRerankStart => {
            if let Some(start) = open_telemetry_rerank_start_event(&event) {
                recorder.on_rerank_operation_start(start);
            }
        }
        TelemetryEventKind::OnRerankEnd => {
            if let Some(call_id) = string_field(&event.event, "callId") {
                recorder.on_rerank_operation_end(ai_sdk_otel::OpenTelemetryRerankEndEvent::new(
                    call_id,
                ));
            }
        }
        TelemetryEventKind::OnEnd => {
            if event.event.get("object").is_some() || event.event.get("error").is_some() {
                if let Some(end) = open_telemetry_object_end_event(&event.event) {
                    recorder.on_object_operation_end(end);
                }
            } else if let Some(end) = open_telemetry_end_event(&event.event) {
                recorder.on_end(end);
            }
        }
        TelemetryEventKind::OnError => {
            if let Some(error) = open_telemetry_error_event(&event.event) {
                recorder.on_error(error);
            }
        }
    }
}

fn open_telemetry_start_event(
    event: &TelemetryEvent,
) -> Option<ai_sdk_otel::OpenTelemetryStartEvent> {
    let payload = &event.event;
    let mut start = ai_sdk_otel::OpenTelemetryStartEvent::new(
        string_field(payload, "callId")?,
        string_field(payload, "operationId")?,
        string_field(payload, "provider")?,
        string_field(payload, "modelId")?,
    )
    .with_telemetry(open_telemetry_options(event))
    .with_settings(settings_attributes(payload));

    if let Some(prompt) = prompt_field(payload, "messages") {
        if let Some(system) = ai_sdk_otel::extract_system_from_prompt(&prompt) {
            start = start.with_system_instructions(ai_sdk_otel::format_system_instructions(system));
        }
        start = start.with_input_messages(ai_sdk_otel::format_input_messages(&prompt));
    }
    if let Some(runtime_context) = attributes_field(payload, "runtimeContext") {
        start = start.with_runtime_context(runtime_context);
    }

    Some(start)
}

fn open_telemetry_object_start_event(
    event: &TelemetryEvent,
) -> Option<ai_sdk_otel::OpenTelemetryObjectStartEvent> {
    let payload = &event.event;
    let mut start = ai_sdk_otel::OpenTelemetryObjectStartEvent::new(
        string_field(payload, "callId")?,
        string_field(payload, "operationId")?,
        string_field(payload, "provider")?,
        string_field(payload, "modelId")?,
    )
    .with_telemetry(open_telemetry_options(event))
    .with_settings(settings_attributes(payload));

    if let Some(prompt) = prompt_field(payload, "messages") {
        if let Some(system) = ai_sdk_otel::extract_system_from_prompt(&prompt) {
            start = start.with_system_instructions(ai_sdk_otel::format_system_instructions(system));
        }
        start = start.with_input_messages(ai_sdk_otel::format_input_messages(&prompt));
    }
    start.schema = payload
        .get("schema")
        .filter(|value| !value.is_null())
        .cloned();
    start.schema_name = string_field(payload, "schemaName");
    start.schema_description = string_field(payload, "schemaDescription");
    start.output_mode = string_field(payload, "output");

    Some(start)
}

fn open_telemetry_step_start_event(
    payload: &JsonValue,
) -> Option<ai_sdk_otel::OpenTelemetryStepStartEvent> {
    Some(ai_sdk_otel::OpenTelemetryStepStartEvent::new(
        string_field(payload, "callId")?,
        u64_field(payload, "stepNumber")?,
    ))
}

fn open_telemetry_language_model_call_start_event(
    payload: &JsonValue,
) -> Option<ai_sdk_otel::OpenTelemetryLanguageModelCallStartEvent> {
    let mut start = ai_sdk_otel::OpenTelemetryLanguageModelCallStartEvent::new(
        string_field(payload, "callId")?,
        string_field(payload, "provider")?,
        string_field(payload, "modelId")?,
    );
    if let Some(prompt) = prompt_field(payload, "messages") {
        start = start.with_input_messages(ai_sdk_otel::format_input_messages(&prompt));
    }
    Some(start)
}

fn open_telemetry_language_model_call_end_event(
    payload: &JsonValue,
) -> Option<ai_sdk_otel::OpenTelemetryLanguageModelCallEndEvent> {
    let mut end = ai_sdk_otel::OpenTelemetryLanguageModelCallEndEvent::new(
        string_field(payload, "callId")?,
        finish_reason(payload)?,
    );
    if let Some(usage) = token_usage_field(payload, "usage") {
        end = end.with_usage(usage);
    }
    if let Some(output_messages) = output_messages(payload) {
        end = end.with_output_messages(output_messages);
    }
    Some(end)
}

fn open_telemetry_tool_execution_start_event(
    payload: &JsonValue,
) -> Option<ai_sdk_otel::OpenTelemetryToolExecutionStartEvent> {
    let tool_call = payload.get("toolCall")?;
    let mut start = ai_sdk_otel::OpenTelemetryToolExecutionStartEvent::new(
        string_field(payload, "callId")?,
        string_field(tool_call, "toolCallId")?,
        string_field(tool_call, "toolName")?,
    );
    if let Some(input) = tool_call.get("input").filter(|value| !value.is_null()) {
        start = start.with_input(input.clone());
    }
    Some(start)
}

fn open_telemetry_tool_execution_end_event(
    payload: &JsonValue,
) -> Option<ai_sdk_otel::OpenTelemetryToolExecutionEndEvent> {
    let tool_call = payload.get("toolCall")?;
    let mut end = ai_sdk_otel::OpenTelemetryToolExecutionEndEvent::new(
        string_field(payload, "callId")?,
        string_field(tool_call, "toolCallId")?,
    );
    if let Some(output) = payload
        .get("toolOutput")
        .and_then(|output| output.get("output"))
    {
        end = end.with_output(output.clone());
    } else if let Some(error) = payload
        .get("toolOutput")
        .and_then(|output| output.get("error"))
    {
        end = end.with_error(ai_sdk_otel::RecordSpanError::exception(
            "Error",
            error
                .as_str()
                .map_or_else(|| error.to_string(), str::to_string),
        ));
    }
    Some(end)
}

fn open_telemetry_object_step_start_event(
    payload: &JsonValue,
) -> Option<ai_sdk_otel::OpenTelemetryObjectStepStartEvent> {
    let mut start = ai_sdk_otel::OpenTelemetryObjectStepStartEvent::new(
        string_field(payload, "callId")?,
        string_field(payload, "provider")?,
        string_field(payload, "modelId")?,
    )
    .with_settings(settings_attributes(payload));
    if let Some(prompt) = prompt_field(payload, "promptMessages") {
        start = start.with_input_messages(ai_sdk_otel::format_input_messages(&prompt));
    }
    Some(start)
}

fn open_telemetry_object_step_finish_event(
    payload: &JsonValue,
) -> Option<ai_sdk_otel::OpenTelemetryObjectStepFinishEvent> {
    let mut finish = ai_sdk_otel::OpenTelemetryObjectStepFinishEvent::new(
        string_field(payload, "callId")?,
        finish_reason(payload)?,
    );
    if let Some(usage) = token_usage_field(payload, "usage") {
        finish = finish.with_usage(usage);
    }
    if let Some(object_text) = string_field(payload, "objectText") {
        finish = finish.with_object_text(object_text);
    }
    Some(finish)
}

fn open_telemetry_object_end_event(
    payload: &JsonValue,
) -> Option<ai_sdk_otel::OpenTelemetryObjectEndEvent> {
    let mut end = ai_sdk_otel::OpenTelemetryObjectEndEvent::new(
        string_field(payload, "callId")?,
        finish_reason(payload)?,
    );
    if let Some(usage) = token_usage_field(payload, "usage") {
        end = end.with_usage(usage);
    }
    if let Some(object) = payload.get("object").filter(|value| !value.is_null()) {
        end = end.with_object(object.clone());
    }
    Some(end)
}

fn open_telemetry_embed_start_event(
    event: &TelemetryEvent,
) -> Option<ai_sdk_otel::OpenTelemetryEmbedStartEvent> {
    let payload = &event.event;
    let mut start = ai_sdk_otel::OpenTelemetryEmbedStartEvent::new(
        string_field(payload, "callId")?,
        string_field(payload, "operationId")?,
        string_field(payload, "provider")?,
        string_field(payload, "modelId")?,
        embedding_input(payload.get("value")?)?,
    );
    start = start.with_telemetry(open_telemetry_options(event));
    Some(start)
}

fn open_telemetry_embed_end_event(
    payload: &JsonValue,
) -> Option<ai_sdk_otel::OpenTelemetryEmbedEndEvent> {
    let mut end = ai_sdk_otel::OpenTelemetryEmbedEndEvent::new(
        string_field(payload, "callId")?,
        embedding_output(payload.get("embedding")?)?,
    );
    if let Some(usage) = embedding_usage(payload) {
        end = end.with_usage(usage);
    }
    Some(end)
}

fn open_telemetry_rerank_start_event(
    event: &TelemetryEvent,
) -> Option<ai_sdk_otel::OpenTelemetryRerankStartEvent> {
    let payload = &event.event;
    let mut start = ai_sdk_otel::OpenTelemetryRerankStartEvent::new(
        string_field(payload, "callId")?,
        string_field(payload, "provider")?,
        string_field(payload, "modelId")?,
        payload
            .get("documents")
            .and_then(JsonValue::as_array)
            .cloned()
            .unwrap_or_default(),
    );
    start.operation_id =
        string_field(payload, "operationId").unwrap_or_else(|| "ai.rerank".to_string());
    start = start.with_telemetry(open_telemetry_options(event));
    Some(start)
}

fn open_telemetry_end_event(payload: &JsonValue) -> Option<ai_sdk_otel::OpenTelemetryEndEvent> {
    let mut end = ai_sdk_otel::OpenTelemetryEndEvent::new(
        string_field(payload, "callId")?,
        finish_reason(payload)?,
    );
    if let Some(usage) =
        token_usage_field(payload, "totalUsage").or_else(|| token_usage_field(payload, "usage"))
    {
        end = end.with_usage(usage);
    }
    if let Some(output_messages) = output_messages(payload) {
        end = end.with_output_messages(output_messages);
    }
    Some(end)
}

fn open_telemetry_error_event(payload: &JsonValue) -> Option<ai_sdk_otel::OpenTelemetryErrorEvent> {
    Some(ai_sdk_otel::OpenTelemetryErrorEvent::new(
        string_field(payload, "callId")?,
        ai_sdk_otel::RecordSpanError::exception(
            "Error",
            payload
                .get("error")
                .and_then(JsonValue::as_str)
                .map_or_else(|| payload.to_string(), str::to_string),
        ),
    ))
}

fn is_object_operation(payload: &JsonValue) -> bool {
    matches!(
        string_field(payload, "operationId").as_deref(),
        Some("ai.generateObject" | "ai.streamObject")
    )
}

fn open_telemetry_options(event: &TelemetryEvent) -> ai_sdk_otel::TelemetryOptions {
    let mut options = ai_sdk_otel::TelemetryOptions::new();
    if let Some(record_inputs) = event.record_inputs {
        options = options.with_record_inputs(record_inputs);
    }
    if let Some(record_outputs) = event.record_outputs {
        options = options.with_record_outputs(record_outputs);
    }
    if let Some(function_id) = &event.function_id {
        options = options.with_function_id(function_id.clone());
    }
    options
}

fn settings_attributes(payload: &JsonValue) -> ai_sdk_otel::TelemetryAttributes {
    [
        "maxOutputTokens",
        "temperature",
        "topP",
        "topK",
        "presencePenalty",
        "frequencyPenalty",
        "stopSequences",
        "seed",
        "reasoning",
        "toolChoice",
        "activeTools",
        "output",
    ]
    .into_iter()
    .filter_map(|key| {
        payload
            .get(key)
            .filter(|value| !value.is_null())
            .map(|value| (key.to_string(), value.clone()))
    })
    .collect()
}

fn attributes_field(payload: &JsonValue, field: &str) -> Option<ai_sdk_otel::TelemetryAttributes> {
    payload.get(field)?.as_object().map(|object| {
        object
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    })
}

fn prompt_field(payload: &JsonValue, field: &str) -> Option<LanguageModelPrompt> {
    serde_json::from_value(payload.get(field)?.clone()).ok()
}

fn output_messages(payload: &JsonValue) -> Option<Vec<ai_sdk_otel::SemConvMessage>> {
    let finish_reason = finish_reason(payload)?;
    let mut output = ai_sdk_otel::OutputMessages::new(finish_reason);
    if let Some(text) = string_field(payload, "text") {
        output = output.with_text(text);
    }
    for tool_call in payload
        .get("toolCalls")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(tool_call_id), Some(tool_name)) = (
            string_field(tool_call, "toolCallId"),
            string_field(tool_call, "toolName"),
        ) {
            output = output.with_tool_call(ai_sdk_otel::OutputToolCall::new(
                tool_call_id,
                tool_name,
                tool_call.get("input").cloned().unwrap_or(JsonValue::Null),
            ));
        }
    }
    Some(ai_sdk_otel::format_output_messages(output))
}

fn token_usage_field(payload: &JsonValue, field: &str) -> Option<ai_sdk_otel::TelemetryTokenUsage> {
    let usage = payload.get(field)?;
    let input_tokens = usage
        .get("inputTokens")
        .and_then(|input| input.get("total"))
        .and_then(JsonValue::as_u64);
    let output_tokens = usage
        .get("outputTokens")
        .and_then(|output| output.get("total"))
        .and_then(JsonValue::as_u64);
    Some(ai_sdk_otel::TelemetryTokenUsage {
        input_tokens,
        output_tokens,
        total_tokens: input_tokens
            .zip(output_tokens)
            .map(|(input, output)| input + output),
    })
}

fn embedding_usage(payload: &JsonValue) -> Option<ai_sdk_otel::OpenTelemetryEmbeddingUsage> {
    Some(ai_sdk_otel::OpenTelemetryEmbeddingUsage {
        tokens: payload
            .get("usage")?
            .get("tokens")
            .and_then(JsonValue::as_u64),
    })
}

fn embedding_input(value: &JsonValue) -> Option<ai_sdk_otel::OpenTelemetryEmbeddingInput> {
    match value {
        JsonValue::String(value) => Some(ai_sdk_otel::OpenTelemetryEmbeddingInput::one(value)),
        JsonValue::Array(values) => Some(ai_sdk_otel::OpenTelemetryEmbeddingInput::many(
            values.iter().filter_map(JsonValue::as_str),
        )),
        _ => None,
    }
}

fn embedding_output(value: &JsonValue) -> Option<ai_sdk_otel::OpenTelemetryEmbeddingOutput> {
    match value {
        JsonValue::Array(values) if values.iter().all(JsonValue::is_number) => {
            Some(ai_sdk_otel::OpenTelemetryEmbeddingOutput::one(
                values.iter().filter_map(JsonValue::as_f64),
            ))
        }
        JsonValue::Array(values) => Some(ai_sdk_otel::OpenTelemetryEmbeddingOutput::many(
            values.iter().filter_map(|embedding| {
                embedding
                    .as_array()
                    .map(|values| values.iter().filter_map(JsonValue::as_f64))
            }),
        )),
        _ => None,
    }
}

fn finish_reason(payload: &JsonValue) -> Option<String> {
    string_field(payload, "finishReason").map(|reason| match reason.as_str() {
        "Stop" | "stop" => "stop".to_string(),
        "Length" | "length" => "length".to_string(),
        "ContentFilter" | "content-filter" | "contentFilter" => "content-filter".to_string(),
        "ToolCalls" | "tool-calls" | "toolCalls" => "tool-calls".to_string(),
        "Error" | "error" => "error".to_string(),
        _ => reason,
    })
}

fn string_field(payload: &JsonValue, field: &str) -> Option<String> {
    payload
        .get(field)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
}

fn u64_field(payload: &JsonValue, field: &str) -> Option<u64> {
    payload.get(field).and_then(JsonValue::as_u64)
}

/// Telemetry settings for a single high-level SDK call.
#[derive(Clone, Default)]
pub struct TelemetryOptions {
    pub is_enabled: Option<bool>,
    pub record_inputs: Option<bool>,
    pub record_outputs: Option<bool>,
    pub function_id: Option<String>,
    pub include_runtime_context: BTreeMap<String, bool>,
    pub include_tools_context: BTreeMap<String, BTreeMap<String, bool>>,
    integrations: Option<Vec<Arc<TelemetryIntegration>>>,
}

impl fmt::Debug for TelemetryOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TelemetryOptions")
            .field("is_enabled", &self.is_enabled)
            .field("record_inputs", &self.record_inputs)
            .field("record_outputs", &self.record_outputs)
            .field("function_id", &self.function_id)
            .field("include_runtime_context", &self.include_runtime_context)
            .field("include_tools_context", &self.include_tools_context)
            .field(
                "integrations",
                &self
                    .integrations
                    .as_ref()
                    .map(|integrations| integrations.len()),
            )
            .finish()
    }
}

impl TelemetryOptions {
    /// Creates default telemetry options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables or disables telemetry for this call.
    pub fn with_enabled(mut self, is_enabled: bool) -> Self {
        self.is_enabled = Some(is_enabled);
        self
    }

    /// Enables or disables input recording.
    pub fn with_record_inputs(mut self, record_inputs: bool) -> Self {
        self.record_inputs = Some(record_inputs);
        self
    }

    /// Enables or disables output recording.
    pub fn with_record_outputs(mut self, record_outputs: bool) -> Self {
        self.record_outputs = Some(record_outputs);
        self
    }

    /// Sets the function id used to group telemetry data.
    pub fn with_function_id(mut self, function_id: impl Into<String>) -> Self {
        self.function_id = Some(function_id.into());
        self
    }

    /// Includes a top-level runtime context key in telemetry.
    pub fn with_runtime_context_key(mut self, key: impl Into<String>, include: bool) -> Self {
        self.include_runtime_context.insert(key.into(), include);
        self
    }

    /// Includes a tool context key in telemetry.
    pub fn with_tool_context_key(
        mut self,
        tool_name: impl Into<String>,
        key: impl Into<String>,
        include: bool,
    ) -> Self {
        self.include_tools_context
            .entry(tool_name.into())
            .or_default()
            .insert(key.into(), include);
        self
    }

    /// Uses one per-call integration instead of globally registered integrations.
    pub fn with_integration(mut self, integration: TelemetryIntegration) -> Self {
        self.integrations = Some(vec![Arc::new(integration)]);
        self
    }

    /// Uses per-call integrations instead of globally registered integrations.
    pub fn with_integrations(
        mut self,
        integrations: impl IntoIterator<Item = TelemetryIntegration>,
    ) -> Self {
        self.integrations = Some(integrations.into_iter().map(Arc::new).collect());
        self
    }
}

#[derive(Clone, Debug, Default)]
struct TelemetryMetadata {
    record_inputs: Option<bool>,
    record_outputs: Option<bool>,
    function_id: Option<String>,
}

/// Dispatcher that fans out lifecycle events to telemetry integrations.
#[derive(Clone, Debug)]
pub struct TelemetryDispatcher {
    is_enabled: bool,
    integrations: Vec<Arc<TelemetryIntegration>>,
    metadata: TelemetryMetadata,
}

impl TelemetryDispatcher {
    /// Returns whether this dispatcher publishes and dispatches telemetry.
    pub const fn is_enabled(&self) -> bool {
        self.is_enabled
    }

    /// Returns true when at least one execute-tool wrapper is available.
    pub fn has_execute_tool(&self) -> bool {
        self.is_enabled
            && self
                .integrations
                .iter()
                .any(|integration| integration.execute_tool.is_some())
    }

    /// Dispatches a lifecycle event.
    pub fn dispatch(&self, kind: TelemetryEventKind, event: impl Serialize) {
        if !self.is_enabled {
            return;
        }

        let event = serde_json::to_value(event).unwrap_or(JsonValue::Null);
        let telemetry_event = TelemetryEvent::new(kind, event, &self.metadata);
        publish_telemetry_diagnostic_message(TelemetryDiagnosticMessage {
            kind,
            event: telemetry_event.clone(),
        });

        for callback in self
            .integrations
            .iter()
            .filter_map(|integration| integration.callback(kind))
        {
            let event = telemetry_event.clone();
            let _ = catch_unwind(AssertUnwindSafe(|| callback(event)));
        }
    }

    pub fn on_start(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnStart, event);
    }

    pub fn on_step_start(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnStepStart, event);
    }

    pub fn on_language_model_call_start(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnLanguageModelCallStart, event);
    }

    pub fn on_language_model_call_end(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnLanguageModelCallEnd, event);
    }

    pub fn on_tool_execution_start(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnToolExecutionStart, event);
    }

    pub fn on_tool_execution_end(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnToolExecutionEnd, event);
    }

    pub fn on_step_finish(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnStepFinish, event);
    }

    pub fn on_object_step_start(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnObjectStepStart, event);
    }

    pub fn on_object_step_finish(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnObjectStepFinish, event);
    }

    pub fn on_embed_start(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnEmbedStart, event);
    }

    pub fn on_embed_end(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnEmbedEnd, event);
    }

    pub fn on_rerank_start(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnRerankStart, event);
    }

    pub fn on_rerank_end(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnRerankEnd, event);
    }

    pub fn on_end(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnEnd, event);
    }

    pub fn on_error(&self, event: impl Serialize) {
        self.dispatch(TelemetryEventKind::OnError, event);
    }

    /// Runs a tool execute function through the configured telemetry wrappers.
    pub fn execute_tool(
        &self,
        call_id: impl Into<String>,
        tool_call_id: impl Into<String>,
        execute: impl FnOnce() -> JsonValue + Send + 'static,
    ) -> JsonValue {
        if !self.is_enabled {
            return execute();
        }

        let call_id = call_id.into();
        let tool_call_id = tool_call_id.into();
        let mut execute: Box<dyn FnOnce() -> JsonValue + Send> = Box::new(execute);

        for wrapper in self
            .integrations
            .iter()
            .filter_map(|integration| integration.execute_tool())
        {
            let inner_execute = execute;
            let call_id = call_id.clone();
            let tool_call_id = tool_call_id.clone();
            execute = Box::new(move || {
                wrapper(TelemetryExecuteToolOptions {
                    call_id,
                    tool_call_id,
                    execute: inner_execute,
                })
            });
        }

        execute()
    }
}

/// Creates a telemetry dispatcher from per-call telemetry settings.
pub fn create_telemetry_dispatcher(telemetry: Option<TelemetryOptions>) -> TelemetryDispatcher {
    if telemetry
        .as_ref()
        .and_then(|telemetry| telemetry.is_enabled)
        == Some(false)
    {
        return TelemetryDispatcher {
            is_enabled: false,
            integrations: Vec::new(),
            metadata: TelemetryMetadata::default(),
        };
    }

    let telemetry = telemetry.unwrap_or_default();
    let integrations = telemetry
        .integrations
        .unwrap_or_else(get_global_telemetry_integrations);

    TelemetryDispatcher {
        is_enabled: true,
        integrations,
        metadata: TelemetryMetadata {
            record_inputs: telemetry.record_inputs,
            record_outputs: telemetry.record_outputs,
            function_id: telemetry.function_id,
        },
    }
}

static TELEMETRY_INTEGRATIONS: LazyLock<Mutex<Vec<Arc<TelemetryIntegration>>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// Registers one or more telemetry integrations globally.
pub fn register_telemetry(integrations: impl IntoIterator<Item = TelemetryIntegration>) {
    TELEMETRY_INTEGRATIONS
        .lock()
        .expect("telemetry registry mutex is not poisoned")
        .extend(integrations.into_iter().map(Arc::new));
}

/// Registers a single telemetry integration globally.
pub fn register_telemetry_integration(integration: TelemetryIntegration) {
    register_telemetry([integration]);
}

/// Returns the globally registered telemetry integrations.
pub fn get_global_telemetry_integrations() -> Vec<Arc<TelemetryIntegration>> {
    TELEMETRY_INTEGRATIONS
        .lock()
        .expect("telemetry registry mutex is not poisoned")
        .clone()
}

static TELEMETRY_DIAGNOSTIC_SUBSCRIBERS: LazyLock<
    Mutex<Vec<(usize, TelemetryDiagnosticCallback)>>,
> = LazyLock::new(|| Mutex::new(Vec::new()));
static NEXT_TELEMETRY_DIAGNOSTIC_SUBSCRIBER_ID: AtomicUsize = AtomicUsize::new(1);

/// Handle returned by [`subscribe_telemetry_diagnostics`].
#[derive(Debug)]
pub struct TelemetryDiagnosticSubscription {
    id: usize,
}

impl Drop for TelemetryDiagnosticSubscription {
    fn drop(&mut self) {
        TELEMETRY_DIAGNOSTIC_SUBSCRIBERS
            .lock()
            .expect("telemetry diagnostic subscriber mutex is not poisoned")
            .retain(|(id, _)| *id != self.id);
    }
}

/// Subscribes to process-local telemetry diagnostic messages.
pub fn subscribe_telemetry_diagnostics(
    subscriber: impl Fn(TelemetryDiagnosticMessage) + Send + Sync + 'static,
) -> TelemetryDiagnosticSubscription {
    let id = NEXT_TELEMETRY_DIAGNOSTIC_SUBSCRIBER_ID.fetch_add(1, Ordering::Relaxed);
    TELEMETRY_DIAGNOSTIC_SUBSCRIBERS
        .lock()
        .expect("telemetry diagnostic subscriber mutex is not poisoned")
        .push((id, Arc::new(subscriber)));
    TelemetryDiagnosticSubscription { id }
}

fn publish_telemetry_diagnostic_message(message: TelemetryDiagnosticMessage) {
    for subscriber in TELEMETRY_DIAGNOSTIC_SUBSCRIBERS
        .lock()
        .expect("telemetry diagnostic subscriber mutex is not poisoned")
        .iter()
        .map(|(_, subscriber)| Arc::clone(subscriber))
        .collect::<Vec<_>>()
    {
        let message = message.clone();
        let _ = catch_unwind(AssertUnwindSafe(|| subscriber(message)));
    }
}

#[cfg(test)]
fn reset_telemetry_state_for_tests() {
    TELEMETRY_INTEGRATIONS
        .lock()
        .expect("telemetry registry mutex is not poisoned")
        .clear();
    TELEMETRY_DIAGNOSTIC_SUBSCRIBERS
        .lock()
        .expect("telemetry diagnostic subscriber mutex is not poisoned")
        .clear();
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, MutexGuard};

    use serde_json::json;

    use super::*;

    fn recorded_events() -> Arc<Mutex<Vec<TelemetryEvent>>> {
        Arc::new(Mutex::new(Vec::new()))
    }

    static TELEMETRY_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn telemetry_test_guard() -> MutexGuard<'static, ()> {
        TELEMETRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn telemetry_registry_adds_global_integrations_in_order() {
        let _guard = telemetry_test_guard();
        reset_telemetry_state_for_tests();

        register_telemetry_integration(TelemetryIntegration::new());
        register_telemetry([TelemetryIntegration::new(), TelemetryIntegration::new()]);

        assert_eq!(get_global_telemetry_integrations().len(), 3);
    }

    #[test]
    fn telemetry_dispatcher_invokes_local_integration_with_augmented_event() {
        let _guard = telemetry_test_guard();
        reset_telemetry_state_for_tests();
        let events = recorded_events();
        let captured = Arc::clone(&events);
        let integration =
            TelemetryIntegration::new().with_callback(TelemetryEventKind::OnStart, move |event| {
                captured.lock().expect("event lock").push(event);
            });

        let dispatcher = create_telemetry_dispatcher(Some(
            TelemetryOptions::new()
                .with_function_id("weather")
                .with_record_inputs(false)
                .with_record_outputs(true)
                .with_integration(integration),
        ));
        dispatcher.on_start(json!({ "callId": "call-1" }));

        let events = events.lock().expect("event lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, TelemetryEventKind::OnStart);
        assert_eq!(events[0].event, json!({ "callId": "call-1" }));
        assert_eq!(events[0].function_id.as_deref(), Some("weather"));
        assert_eq!(events[0].record_inputs, Some(false));
        assert_eq!(events[0].record_outputs, Some(true));
    }

    #[test]
    fn telemetry_dispatcher_uses_global_integrations_when_local_integrations_are_absent() {
        let _guard = telemetry_test_guard();
        reset_telemetry_state_for_tests();
        let global_events = recorded_events();
        let local_events = recorded_events();
        let captured_global = Arc::clone(&global_events);
        let captured_local = Arc::clone(&local_events);

        register_telemetry_integration(TelemetryIntegration::new().with_callback(
            TelemetryEventKind::OnStart,
            move |event| {
                captured_global.lock().expect("event lock").push(event);
            },
        ));

        create_telemetry_dispatcher(None).on_start(json!({ "callId": "global" }));
        create_telemetry_dispatcher(Some(TelemetryOptions::new().with_integration(
            TelemetryIntegration::new().with_callback(TelemetryEventKind::OnStart, move |event| {
                captured_local.lock().expect("event lock").push(event);
            }),
        )))
        .on_start(json!({ "callId": "local" }));

        assert_eq!(global_events.lock().expect("event lock").len(), 1);
        assert_eq!(
            global_events.lock().expect("event lock")[0].event,
            json!({ "callId": "global" })
        );
        assert_eq!(local_events.lock().expect("event lock").len(), 1);
        assert_eq!(
            local_events.lock().expect("event lock")[0].event,
            json!({ "callId": "local" })
        );
    }

    #[test]
    fn telemetry_dispatcher_can_use_an_empty_local_integration_set() {
        let _guard = telemetry_test_guard();
        reset_telemetry_state_for_tests();
        let events = recorded_events();
        let captured = Arc::clone(&events);
        register_telemetry_integration(TelemetryIntegration::new().with_callback(
            TelemetryEventKind::OnStart,
            move |event| {
                captured.lock().expect("event lock").push(event);
            },
        ));

        create_telemetry_dispatcher(Some(TelemetryOptions::new().with_integrations([])))
            .on_start(json!({ "callId": "local-empty" }));

        assert!(events.lock().expect("event lock").is_empty());
    }

    #[test]
    fn telemetry_dispatcher_noops_when_disabled() {
        let _guard = telemetry_test_guard();
        reset_telemetry_state_for_tests();
        let events = recorded_events();
        let captured = Arc::clone(&events);
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let captured_diagnostics = Arc::clone(&diagnostics);
        let _subscription = subscribe_telemetry_diagnostics(move |message| {
            captured_diagnostics
                .lock()
                .expect("diagnostics lock")
                .push(message);
        });

        let dispatcher = create_telemetry_dispatcher(Some(
            TelemetryOptions::new()
                .with_enabled(false)
                .with_integration(TelemetryIntegration::new().with_callback(
                    TelemetryEventKind::OnStart,
                    move |event| {
                        captured.lock().expect("event lock").push(event);
                    },
                )),
        ));

        assert!(!dispatcher.is_enabled());
        assert!(!dispatcher.has_execute_tool());
        dispatcher.on_start(json!({ "callId": "disabled" }));

        assert!(events.lock().expect("event lock").is_empty());
        assert!(
            !diagnostics
                .lock()
                .expect("diagnostics lock")
                .iter()
                .any(|diagnostic| diagnostic.event.event == json!({ "callId": "disabled" }))
        );
    }

    #[test]
    fn telemetry_dispatcher_publishes_diagnostics_without_integrations() {
        let _guard = telemetry_test_guard();
        reset_telemetry_state_for_tests();
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&diagnostics);
        let _subscription = subscribe_telemetry_diagnostics(move |message| {
            captured.lock().expect("diagnostics lock").push(message);
        });

        create_telemetry_dispatcher(Some(
            TelemetryOptions::new().with_function_id("diagnostic-function"),
        ))
        .on_rerank_end(json!({ "callId": "rerank-call" }));

        let diagnostics = diagnostics.lock().expect("diagnostics lock");
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.kind == TelemetryEventKind::OnRerankEnd
                    && diagnostic.event.event == json!({ "callId": "rerank-call" })
            })
            .expect("expected rerank diagnostic event");
        assert_eq!(
            diagnostic.event.function_id.as_deref(),
            Some("diagnostic-function")
        );
    }

    #[test]
    fn telemetry_dispatcher_swallows_callback_panics_and_continues() {
        let _guard = telemetry_test_guard();
        reset_telemetry_state_for_tests();
        let events = recorded_events();
        let captured = Arc::clone(&events);
        let first = TelemetryIntegration::new().with_callback(TelemetryEventKind::OnStart, |_| {
            panic!("sync boom");
        });
        let second =
            TelemetryIntegration::new().with_callback(TelemetryEventKind::OnStart, move |event| {
                captured.lock().expect("event lock").push(event);
            });

        create_telemetry_dispatcher(Some(
            TelemetryOptions::new().with_integrations([first, second]),
        ))
        .on_start(json!({ "callId": "call-1" }));

        assert_eq!(events.lock().expect("event lock").len(), 1);
    }

    #[test]
    fn telemetry_dispatcher_supports_all_lifecycle_methods() {
        let _guard = telemetry_test_guard();
        reset_telemetry_state_for_tests();
        let events = recorded_events();
        let mut integration = TelemetryIntegration::new();
        for kind in [
            TelemetryEventKind::OnStart,
            TelemetryEventKind::OnStepStart,
            TelemetryEventKind::OnLanguageModelCallStart,
            TelemetryEventKind::OnLanguageModelCallEnd,
            TelemetryEventKind::OnToolExecutionStart,
            TelemetryEventKind::OnToolExecutionEnd,
            TelemetryEventKind::OnStepFinish,
            TelemetryEventKind::OnObjectStepStart,
            TelemetryEventKind::OnObjectStepFinish,
            TelemetryEventKind::OnEmbedStart,
            TelemetryEventKind::OnEmbedEnd,
            TelemetryEventKind::OnRerankStart,
            TelemetryEventKind::OnRerankEnd,
            TelemetryEventKind::OnEnd,
            TelemetryEventKind::OnError,
        ] {
            let captured = Arc::clone(&events);
            integration = integration.with_callback(kind, move |event| {
                captured.lock().expect("event lock").push(event);
            });
        }

        let dispatcher = create_telemetry_dispatcher(Some(
            TelemetryOptions::new().with_integration(integration),
        ));
        dispatcher.on_start(json!({}));
        dispatcher.on_step_start(json!({}));
        dispatcher.on_language_model_call_start(json!({}));
        dispatcher.on_language_model_call_end(json!({}));
        dispatcher.on_tool_execution_start(json!({}));
        dispatcher.on_tool_execution_end(json!({}));
        dispatcher.on_step_finish(json!({}));
        dispatcher.on_object_step_start(json!({}));
        dispatcher.on_object_step_finish(json!({}));
        dispatcher.on_embed_start(json!({}));
        dispatcher.on_embed_end(json!({}));
        dispatcher.on_rerank_start(json!({}));
        dispatcher.on_rerank_end(json!({}));
        dispatcher.on_end(json!({}));
        dispatcher.on_error(json!({}));

        let events = events.lock().expect("event lock");
        assert_eq!(events.len(), 15);
        assert_eq!(events[0].kind, TelemetryEventKind::OnStart);
        assert_eq!(events[14].kind, TelemetryEventKind::OnError);
    }

    #[test]
    fn telemetry_dispatcher_wraps_execute_tool_and_prefers_local_wrappers() {
        let _guard = telemetry_test_guard();
        reset_telemetry_state_for_tests();
        let order = Arc::new(Mutex::new(Vec::<String>::new()));
        let global_order = Arc::clone(&order);
        register_telemetry_integration(TelemetryIntegration::new().with_execute_tool(
            move |options| {
                global_order
                    .lock()
                    .expect("order lock")
                    .push("global-before".to_string());
                let result = (options.execute)();
                global_order
                    .lock()
                    .expect("order lock")
                    .push("global-after".to_string());
                result
            },
        ));

        let local_order = Arc::clone(&order);
        let dispatcher =
            create_telemetry_dispatcher(Some(TelemetryOptions::new().with_integration(
                TelemetryIntegration::new().with_execute_tool(move |options| {
                    local_order
                        .lock()
                        .expect("order lock")
                        .push("local-before".to_string());
                    let result = (options.execute)();
                    local_order
                        .lock()
                        .expect("order lock")
                        .push("local-after".to_string());
                    result
                }),
            )));

        assert!(dispatcher.has_execute_tool());
        let execute_order = Arc::clone(&order);
        let result = dispatcher.execute_tool("call-1", "tool-1", move || {
            execute_order
                .lock()
                .expect("order lock")
                .push("execute".to_string());
            json!("done")
        });

        assert_eq!(result, json!("done"));
        assert_eq!(
            &*order.lock().expect("order lock"),
            &[
                "local-before".to_string(),
                "execute".to_string(),
                "local-after".to_string()
            ]
        );
    }

    #[test]
    fn open_telemetry_integration_exports_dispatcher_spans_to_local_otlp_receiver() {
        let _guard = telemetry_test_guard();
        reset_telemetry_state_for_tests();
        let receiver = ai_sdk_otel::LocalOtlpTraceReceiver::start().expect("receiver starts");
        let recorder = Arc::new(Mutex::new(ai_sdk_otel::OpenTelemetry::new(
            ai_sdk_otel::OpenTelemetryOptions::new(),
        )));
        let dispatcher = create_telemetry_dispatcher(Some(
            TelemetryOptions::new()
                .with_function_id("weather")
                .with_record_inputs(true)
                .with_record_outputs(true)
                .with_integration(create_open_telemetry_integration(Arc::clone(&recorder))),
        ));

        dispatcher.on_start(json!({
            "callId": "call-1",
            "operationId": "ai.generateText",
            "provider": "openai.chat",
            "modelId": "gpt-4o-mini",
            "messages": [
                {
                    "role": "user",
                    "content": [{ "type": "text", "text": "Weather?" }]
                }
            ],
            "temperature": 0.2,
            "runtimeContext": { "tenant": "acme" }
        }));
        dispatcher.on_step_start(json!({
            "callId": "call-1",
            "stepNumber": 1
        }));
        dispatcher.on_language_model_call_start(json!({
            "callId": "call-1",
            "provider": "openai.chat",
            "modelId": "gpt-4o-mini",
            "messages": [
                {
                    "role": "user",
                    "content": [{ "type": "text", "text": "Weather?" }]
                }
            ]
        }));
        dispatcher.on_language_model_call_end(json!({
            "callId": "call-1",
            "finishReason": "Stop",
            "usage": {
                "inputTokens": { "total": 7 },
                "outputTokens": { "total": 4 }
            },
            "text": "Sunny."
        }));
        dispatcher.on_step_finish(json!({ "callId": "call-1" }));
        dispatcher.on_end(json!({
            "callId": "call-1",
            "finishReason": "Stop",
            "totalUsage": {
                "inputTokens": { "total": 7 },
                "outputTokens": { "total": 4 }
            },
            "text": "Sunny."
        }));

        let tracer = recorder.lock().expect("recorder lock").tracer().clone();
        assert_eq!(tracer.spans.len(), 3);
        assert!(tracer.spans.iter().all(|span| span.ended));
        assert_eq!(tracer.spans[0].name, "invoke_agent gpt-4o-mini");
        assert_eq!(
            tracer.spans[0].attributes.get("gen_ai.agent.name"),
            Some(&json!("weather"))
        );
        assert_eq!(
            tracer.spans[0].attributes.get("gen_ai.request.temperature"),
            Some(&json!(0.2))
        );
        assert_eq!(
            tracer.spans[2].attributes.get("gen_ai.usage.total_tokens"),
            Some(&json!(11))
        );

        ai_sdk_otel::export_tracer_to_otlp_http_json(
            &tracer,
            &ai_sdk_otel::OtlpHttpTraceExportOptions::new(receiver.endpoint())
                .with_service_name("ai-sdk-rust-dispatcher-otel"),
        )
        .expect("export succeeds");

        let requests = receiver.wait_for_requests(1, std::time::Duration::from_secs(10));
        assert_eq!(requests.len(), 1);
        let body = requests[0].body_json().expect("OTLP body is JSON");
        assert_eq!(
            body["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["name"],
            "invoke_agent gpt-4o-mini"
        );
    }

    #[test]
    fn legacy_open_telemetry_integration_exports_dispatcher_spans_to_local_otlp_receiver() {
        let _guard = telemetry_test_guard();
        reset_telemetry_state_for_tests();
        let receiver = ai_sdk_otel::LocalOtlpTraceReceiver::start().expect("receiver starts");
        let recorder = Arc::new(Mutex::new(ai_sdk_otel::LegacyOpenTelemetry::new(
            ai_sdk_otel::LegacyOpenTelemetryOptions::new(),
        )));
        let dispatcher = create_telemetry_dispatcher(Some(
            TelemetryOptions::new()
                .with_function_id("legacy-weather")
                .with_record_inputs(true)
                .with_record_outputs(true)
                .with_integration(create_legacy_open_telemetry_integration(Arc::clone(
                    &recorder,
                ))),
        ));

        dispatcher.on_start(json!({
            "callId": "legacy-call",
            "operationId": "ai.generateText",
            "provider": "openai.chat",
            "modelId": "gpt-4o-mini",
            "messages": [
                {
                    "role": "user",
                    "content": [{ "type": "text", "text": "Weather?" }]
                }
            ],
            "temperature": 0.2,
            "maxOutputTokens": 128
        }));
        dispatcher.on_step_start(json!({
            "callId": "legacy-call",
            "stepNumber": 0
        }));
        dispatcher.on_language_model_call_start(json!({
            "callId": "legacy-call",
            "provider": "openai.chat",
            "modelId": "gpt-4o-mini",
            "messages": [
                {
                    "role": "user",
                    "content": [{ "type": "text", "text": "Weather?" }]
                }
            ]
        }));
        dispatcher.on_tool_execution_start(json!({
            "callId": "legacy-call",
            "toolCall": {
                "toolCallId": "tool-1",
                "toolName": "weather",
                "input": { "city": "Paris" }
            }
        }));
        let result =
            dispatcher.execute_tool("legacy-call", "tool-1", || json!({ "temperature": 24 }));
        dispatcher.on_tool_execution_end(json!({
            "callId": "legacy-call",
            "toolCall": {
                "toolCallId": "tool-1",
                "toolName": "weather",
                "input": { "city": "Paris" }
            },
            "toolOutput": {
                "type": "tool-result",
                "output": result
            }
        }));
        dispatcher.on_language_model_call_end(json!({
            "callId": "legacy-call",
            "finishReason": "stop",
            "usage": {
                "inputTokens": { "total": 7 },
                "outputTokens": { "total": 4 }
            },
            "text": "Sunny."
        }));
        dispatcher.on_step_finish(json!({ "callId": "legacy-call" }));
        dispatcher.on_end(json!({
            "callId": "legacy-call",
            "finishReason": "stop",
            "totalUsage": {
                "inputTokens": { "total": 7 },
                "outputTokens": { "total": 4 }
            },
            "text": "Sunny."
        }));

        let tracer = recorder.lock().expect("recorder lock").tracer().clone();
        assert_eq!(tracer.spans.len(), 3);
        assert!(tracer.spans.iter().all(|span| span.ended));
        assert_eq!(tracer.spans[0].name, "ai.generateText");
        assert_eq!(tracer.spans[1].name, "ai.generateText.doGenerate");
        assert_eq!(tracer.spans[2].name, "ai.toolCall");
        assert_eq!(
            tracer.spans[0].attributes.get("operation.name"),
            Some(&json!("ai.generateText legacy-weather"))
        );
        assert_eq!(
            tracer.spans[1].attributes.get("gen_ai.request.max_tokens"),
            Some(&json!(128))
        );
        assert_eq!(
            tracer.spans[2].attributes.get("ai.toolCall.result"),
            Some(&json!("{\"temperature\":24}"))
        );

        ai_sdk_otel::export_tracer_to_otlp_http_json(
            &tracer,
            &ai_sdk_otel::OtlpHttpTraceExportOptions::new(receiver.endpoint())
                .with_service_name("ai-sdk-rust-dispatcher-legacy-otel"),
        )
        .expect("export succeeds");

        let requests = receiver.wait_for_requests(1, std::time::Duration::from_secs(10));
        assert_eq!(requests.len(), 1);
        let body = requests[0].body_json().expect("OTLP body is JSON");
        assert_eq!(
            body["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["name"],
            "ai.generateText"
        );
    }
}
