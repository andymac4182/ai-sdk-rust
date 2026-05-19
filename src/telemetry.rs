use std::collections::BTreeMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use serde::{Deserialize, Serialize};

use crate::json::JsonValue;

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
        assert!(diagnostics.lock().expect("diagnostics lock").is_empty());
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
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, TelemetryEventKind::OnRerankEnd);
        assert_eq!(
            diagnostics[0].event.event,
            json!({ "callId": "rerank-call" })
        );
        assert_eq!(
            diagnostics[0].event.function_id.as_deref(),
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
}
