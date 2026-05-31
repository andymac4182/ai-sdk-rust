//! Core runtime crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/core`. The Rust surface models the
//! portable public workflow, step, hook, sleep, abort, context-storage, request /
//! response, writable-stream, serialization, deterministic runtime contracts,
//! and facade re-export helpers without embedding the JavaScript VM or live
//! persistence runtime.

#![forbid(unsafe_code)]

pub mod capabilities;
pub mod classify_error;
pub mod codec;
pub mod context_errors;
pub mod define_hook;
pub mod describe_error;
pub mod encryption;
pub mod error;
mod events_consumer;
mod flushable_stream;
pub mod format;
pub mod global;
pub mod log_format;
pub mod logger;
pub mod observability;
pub mod ordering;
pub mod runtime;
pub mod schemas;
pub mod set_attributes;
pub mod source_map;
pub mod stream;
pub mod types;
pub mod util;
pub mod value;
pub mod vm;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::rc::{Rc, Weak};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;

pub use events_consumer::*;
pub use flushable_stream::*;
pub use runtime::*;
pub use workflow_errors as errors;
pub use workflow_serde as serde;
pub use workflow_world as world;

/// Browser-safe serialization-format helpers used by observability.
pub mod serialization_format {
    use serde_json::{Map, Value};

    /// Length of the upstream serialization format prefix.
    pub const FORMAT_PREFIX_LENGTH: usize = 4;

    /// Upstream devalue serialization prefix.
    pub const DEVALUE_V1_FORMAT: &str = "devl";

    /// Upstream encrypted payload prefix.
    pub const ENCRYPTED_FORMAT: &str = "encr";

    /// Placeholder displayed for encrypted data when no decryption key is
    /// supplied.
    pub const ENCRYPTED_PLACEHOLDER: &str = "\u{1F512} Encrypted";

    /// Display-friendly observability reviver registry.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct ObservabilityRevivers;

    impl ObservabilityRevivers {
        /// Returns true when the upstream observability registry exposes a
        /// reviver for the given serialized type.
        pub fn contains(&self, name: &str) -> bool {
            OBSERVABILITY_REVIVER_NAMES.contains(&name)
        }

        /// Names exposed by upstream `observabilityRevivers`.
        pub fn names(&self) -> &'static [&'static str] {
            OBSERVABILITY_REVIVER_NAMES
        }
    }

    /// Shared reviver type for the Rust observability facade.
    pub type Revivers = ObservabilityRevivers;

    /// Upstream observability reviver names.
    pub const OBSERVABILITY_REVIVER_NAMES: &[&str] = &[
        "ReadableStream",
        "WritableStream",
        "TransformStream",
        "AbortController",
        "AbortSignal",
        "DOMException",
        "StepFunction",
        "WorkflowFunction",
        "Instance",
        "Class",
    ];

    /// Upstream `observabilityRevivers` registry.
    pub const OBSERVABILITY_REVIVERS: ObservabilityRevivers = ObservabilityRevivers;

    /// Returns the observability reviver registry.
    pub fn observability_revivers() -> ObservabilityRevivers {
        OBSERVABILITY_REVIVERS
    }

    /// Hydrates serialized data for observability.
    ///
    /// The initial Rust surface faithfully preserves already-plain values. Full
    /// devalue and encrypted payload handling belongs to the broader
    /// `workflow-core` serialization bucket.
    pub fn hydrate_data(value: Value, _revivers: Revivers) -> Value {
        value
    }

    /// Hydrates the input/output-style fields of a workflow resource.
    pub fn hydrate_resource_io(resource: Value, revivers: Revivers) -> Value {
        let Value::Object(mut object) = resource else {
            return resource;
        };

        if object.contains_key("stepId") {
            hydrate_fields(&mut object, &["input", "output", "error"], revivers);
        } else if object.contains_key("hookId") {
            hydrate_fields(&mut object, &["metadata"], revivers);
        } else if object.contains_key("eventId") {
            hydrate_event_data(&mut object, revivers);
        } else {
            hydrate_fields(&mut object, &["input", "output", "error"], revivers);
        }

        strip_execution_context(&mut object);

        Value::Object(object)
    }

    fn hydrate_fields(object: &mut Map<String, Value>, fields: &[&str], revivers: Revivers) {
        for field in fields {
            let Some(value) = object.get(*field) else {
                continue;
            };
            if value.is_null() {
                continue;
            }
            object.insert((*field).to_string(), hydrate_data(value.clone(), revivers));
        }
    }

    fn hydrate_event_data(object: &mut Map<String, Value>, revivers: Revivers) {
        let Some(Value::Object(mut event_data)) = object.get("eventData").cloned() else {
            return;
        };

        hydrate_fields(
            &mut event_data,
            &["result", "input", "output", "metadata", "payload", "error"],
            revivers,
        );
        object.insert("eventData".to_string(), Value::Object(event_data));
    }

    fn strip_execution_context(object: &mut Map<String, Value>) {
        let Some(execution_context) = object.remove("executionContext") else {
            return;
        };

        let Some(workflow_core_version) = execution_context
            .as_object()
            .and_then(|context| context.get("workflowCoreVersion"))
            .filter(|value| !value.is_null())
            .cloned()
        else {
            return;
        };

        object.insert("workflowCoreVersion".to_string(), workflow_core_version);
    }
}

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the initial crate skeleton.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/core";

/// Upstream package version inventoried for this skeleton.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.10";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Milliseconds since the Unix epoch.
pub type TimestampMillis = i64;

/// Result type used by the workflow core port.
pub type Result<T> = std::result::Result<T, WorkflowCoreError>;

/// Core error taxonomy needed by the portable workflow APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowCoreError {
    CorruptedEventLog(String),
    Fatal(String),
    HookConflict {
        token: String,
        conflicting_run_id: String,
    },
    NotInStepContext {
        function_name: String,
        docs_url: String,
    },
    NotInWorkflowContext {
        function_name: String,
        docs_url: String,
    },
    NotInWorkflowOrStepContext {
        function_name: String,
        docs_url: String,
    },
    TimeoutFunctionUnavailable {
        slug: &'static str,
    },
    UnavailableInWorkflowContext {
        function_name: String,
        workflow_name: Option<String>,
        docs_url: String,
    },
    Unsupported(String),
    WorkflowNotRegistered(String),
    WorkflowRuntime {
        message: String,
        slug: Option<&'static str>,
    },
    WorkflowSuspension {
        step_count: usize,
        hook_count: usize,
        wait_count: usize,
    },
}

impl WorkflowCoreError {
    pub fn is_workflow_suspension(&self) -> bool {
        matches!(self, Self::WorkflowSuspension { .. })
    }

    pub fn slug(&self) -> Option<&'static str> {
        match self {
            Self::TimeoutFunctionUnavailable { slug } => Some(slug),
            Self::WorkflowRuntime { slug, .. } => *slug,
            _ => None,
        }
    }
}

impl fmt::Display for WorkflowCoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CorruptedEventLog(message) => write!(f, "Corrupted event log: {message}"),
            Self::Fatal(message) => write!(f, "Fatal error: {message}"),
            Self::HookConflict {
                token,
                conflicting_run_id,
            } => write!(
                f,
                "Hook token \"{token}\" is already used by workflow run \"{conflicting_run_id}\""
            ),
            Self::NotInStepContext {
                function_name,
                docs_url,
            } => write!(
                f,
                "{function_name} can only be called from a step function. See {docs_url}"
            ),
            Self::NotInWorkflowContext {
                function_name,
                docs_url,
            } => write!(
                f,
                "{function_name} can only be called from a workflow function. See {docs_url}"
            ),
            Self::NotInWorkflowOrStepContext {
                function_name,
                docs_url,
            } => write!(
                f,
                "{function_name} can only be called from a workflow or step function. See {docs_url}"
            ),
            Self::TimeoutFunctionUnavailable { .. } => write!(
                f,
                "Timeout functions like setTimeout and setInterval are not supported in workflow functions. Use sleep() instead."
            ),
            Self::UnavailableInWorkflowContext {
                function_name,
                workflow_name,
                docs_url,
            } => match workflow_name {
                Some(name) => write!(
                    f,
                    "{function_name} is unavailable inside workflow \"{name}\". See {docs_url}"
                ),
                None => write!(
                    f,
                    "{function_name} is unavailable in workflow context. See {docs_url}"
                ),
            },
            Self::Unsupported(message) => write!(f, "{message}"),
            Self::WorkflowNotRegistered(name) => {
                write!(f, "Workflow \"{name}\" is not registered")
            }
            Self::WorkflowRuntime { message, .. } => write!(f, "{message}"),
            Self::WorkflowSuspension {
                step_count,
                hook_count,
                wait_count,
            } => write!(
                f,
                "Workflow suspended with {step_count} step(s), {hook_count} hook(s), and {wait_count} timer(s) pending"
            ),
        }
    }
}

impl Error for WorkflowCoreError {}

/// Deterministic metadata available inside a workflow run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowMetadata {
    pub workflow_name: String,
    pub workflow_run_id: String,
    pub workflow_started_at: TimestampMillis,
    pub url: String,
    pub features: WorkflowFeatures,
}

impl WorkflowMetadata {
    pub fn new(
        workflow_name: impl Into<String>,
        workflow_run_id: impl Into<String>,
        workflow_started_at: TimestampMillis,
        url: impl Into<String>,
    ) -> Self {
        Self {
            workflow_name: workflow_name.into(),
            workflow_run_id: workflow_run_id.into(),
            workflow_started_at,
            url: url.into(),
            features: WorkflowFeatures::default(),
        }
    }
}

/// Feature flags exposed by upstream workflow metadata.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkflowFeatures {
    pub encryption: bool,
}

/// Metadata available inside a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepMetadata {
    pub step_name: String,
    pub step_id: String,
    pub step_started_at: TimestampMillis,
    pub attempt: u32,
}

/// A Rust representation of a `"use workflow"` export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDefinition {
    pub name: String,
    pub module_specifier: Option<String>,
    pub export_name: Option<String>,
}

impl WorkflowDefinition {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            module_specifier: None,
            export_name: None,
        }
    }

    pub fn module_specifier(mut self, module_specifier: impl Into<String>) -> Self {
        self.module_specifier = Some(module_specifier.into());
        self
    }

    pub fn export_name(mut self, export_name: impl Into<String>) -> Self {
        self.export_name = Some(export_name.into());
        self
    }
}

/// Queue item emitted by workflow primitives during replay.
#[derive(Debug, Clone, PartialEq)]
pub enum QueueItem {
    Step(StepQueueItem),
    Hook(HookQueueItem),
    Wait(WaitQueueItem),
}

impl QueueItem {
    pub fn correlation_id(&self) -> &str {
        match self {
            Self::Step(item) => &item.correlation_id,
            Self::Hook(item) => &item.correlation_id,
            Self::Wait(item) => &item.correlation_id,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Step(_) => "step",
            Self::Hook(_) => "hook",
            Self::Wait(_) => "wait",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepQueueItem {
    pub correlation_id: String,
    pub step_name: String,
    pub args: Vec<Value>,
    pub this_val: Option<Value>,
    pub closure_vars: Option<Value>,
    pub has_created_event: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookQueueItem {
    pub correlation_id: String,
    pub token: String,
    pub metadata: Option<Value>,
    pub is_webhook: bool,
    pub is_system: bool,
    pub disposed: bool,
    pub abort_requested: bool,
    pub abort_reason: Option<Value>,
    pub has_created_event: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitQueueItem {
    pub correlation_id: String,
    pub resume_at: TimestampMillis,
    pub has_created_event: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowEventType {
    StepCreated,
    StepStarted,
    StepRetrying,
    StepCompleted,
    StepFailed,
    HookCreated,
    HookReceived,
    HookDisposed,
    HookConflict,
    WaitCreated,
    WaitCompleted,
    RunCreated,
    RunStarted,
    Other(String),
}

/// Minimal event-log row used by the portable replay surfaces.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowEvent {
    pub event_type: WorkflowEventType,
    pub correlation_id: String,
    pub step_name: Option<String>,
    pub token: Option<String>,
    pub resume_at: Option<TimestampMillis>,
    pub result: Option<Value>,
    pub payload: Option<Value>,
    pub error: Option<WorkflowCoreError>,
    pub conflicting_run_id: Option<String>,
    pub created_at: Option<TimestampMillis>,
}

impl WorkflowEvent {
    pub fn new(event_type: WorkflowEventType, correlation_id: impl Into<String>) -> Self {
        Self {
            event_type,
            correlation_id: correlation_id.into(),
            step_name: None,
            token: None,
            resume_at: None,
            result: None,
            payload: None,
            error: None,
            conflicting_run_id: None,
            created_at: None,
        }
    }

    pub fn step_name(mut self, step_name: impl Into<String>) -> Self {
        self.step_name = Some(step_name.into());
        self
    }

    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn resume_at(mut self, resume_at: TimestampMillis) -> Self {
        self.resume_at = Some(resume_at);
        self
    }

    pub fn result(mut self, result: Value) -> Self {
        self.result = Some(result);
        self
    }

    pub fn payload(mut self, payload: Value) -> Self {
        self.payload = Some(payload);
        self
    }

    pub fn error(mut self, error: WorkflowCoreError) -> Self {
        self.error = Some(error);
        self
    }

    pub fn conflicting_run_id(mut self, run_id: impl Into<String>) -> Self {
        self.conflicting_run_id = Some(run_id.into());
        self
    }

    pub fn created_at(mut self, created_at: TimestampMillis) -> Self {
        self.created_at = Some(created_at);
        self
    }
}

#[derive(Debug, Clone)]
struct WorkflowContextInner {
    metadata: WorkflowMetadata,
    next_id: u64,
    invocations_queue: BTreeMap<String, QueueItem>,
    errors: Vec<WorkflowCoreError>,
    pending_deliveries: usize,
    timestamp: TimestampMillis,
}

/// Shared workflow replay context.
#[derive(Debug, Clone)]
pub struct WorkflowContext {
    inner: Rc<RefCell<WorkflowContextInner>>,
}

impl WorkflowContext {
    pub fn new(metadata: WorkflowMetadata) -> Self {
        let timestamp = metadata.workflow_started_at;
        Self {
            inner: Rc::new(RefCell::new(WorkflowContextInner {
                metadata,
                next_id: 1,
                invocations_queue: BTreeMap::new(),
                errors: Vec::new(),
                pending_deliveries: 0,
                timestamp,
            })),
        }
    }

    pub fn for_test() -> Self {
        Self::new(WorkflowMetadata::new(
            "test/workflow",
            "wrun_000001",
            1_700_000_000_000,
            "http://localhost:3000",
        ))
    }

    pub fn metadata(&self) -> WorkflowMetadata {
        self.inner.borrow().metadata.clone()
    }

    pub fn timestamp(&self) -> TimestampMillis {
        self.inner.borrow().timestamp
    }

    pub fn observe_event_timestamp(&self, event: &WorkflowEvent) {
        if let Some(created_at) = event.created_at {
            self.inner.borrow_mut().timestamp = created_at;
        }
    }

    pub fn queue_items(&self) -> Vec<QueueItem> {
        self.inner
            .borrow()
            .invocations_queue
            .values()
            .cloned()
            .collect()
    }

    pub fn queue_item(&self, correlation_id: &str) -> Option<QueueItem> {
        self.inner
            .borrow()
            .invocations_queue
            .get(correlation_id)
            .cloned()
    }

    pub fn queue_len(&self) -> usize {
        self.inner.borrow().invocations_queue.len()
    }

    pub fn errors(&self) -> Vec<WorkflowCoreError> {
        self.inner.borrow().errors.clone()
    }

    pub fn pending_deliveries(&self) -> usize {
        self.inner.borrow().pending_deliveries
    }

    pub fn suspension(&self) -> WorkflowCoreError {
        let (step_count, hook_count, wait_count) = self.queue_counts();
        WorkflowCoreError::WorkflowSuspension {
            step_count,
            hook_count,
            wait_count,
        }
    }

    pub fn use_step(&self, step_name: impl Into<String>) -> StepProxy {
        StepProxy::new(self.clone(), step_name.into(), None)
    }

    pub fn use_step_with_closure_vars(
        &self,
        step_name: impl Into<String>,
        closure_vars: Value,
    ) -> StepProxy {
        StepProxy::new(self.clone(), step_name.into(), Some(closure_vars))
    }

    pub fn create_hook(&self, options: HookOptions) -> Hook {
        create_hook_in_context(self.clone(), options, false)
    }

    pub fn create_webhook(&self, options: WebhookOptions) -> Result<Webhook> {
        if options.token.is_some() {
            return Err(WorkflowCoreError::WorkflowRuntime {
                message: "`createWebhook()` does not accept a `token` option. Webhook tokens are always randomly generated. Use `createHook()` with `resumeHook()` for deterministic token patterns.".to_string(),
                slug: None,
            });
        }
        let metadata = options
            .respond_with
            .as_ref()
            .map(|respond_with| serde_json::json!({ "respondWith": respond_with }));
        let hook = self.create_hook(HookOptions {
            token: None,
            metadata,
            is_webhook: true,
        });
        let url = format!(
            "{}/.well-known/workflow/v1/webhook/{}",
            self.metadata().url,
            hook.token()
        );
        Ok(Webhook { hook, url })
    }

    pub fn sleep(&self, input: SleepInput) -> Result<SleepCall> {
        let resume_at = parse_sleep_input(self.timestamp(), input)?;
        let correlation_id = self.next_id("wait");
        self.inner.borrow_mut().invocations_queue.insert(
            correlation_id.clone(),
            QueueItem::Wait(WaitQueueItem {
                correlation_id: correlation_id.clone(),
                resume_at,
                has_created_event: false,
            }),
        );
        Ok(SleepCall {
            ctx: self.clone(),
            correlation_id,
            resume_at,
        })
    }

    pub fn create_abort_controller(&self) -> WorkflowAbortController {
        WorkflowAbortController::new_in_context(self.clone())
    }

    pub fn drain_pending_queue_items(&self) -> Vec<QueueItem> {
        let mut inner = self.inner.borrow_mut();
        for item in inner.invocations_queue.values_mut() {
            if let QueueItem::Hook(hook) = item
                && hook.is_system
                && !hook.disposed
                && !hook.abort_requested
            {
                hook.disposed = true;
            }
        }
        inner.invocations_queue.values().cloned().collect()
    }

    fn next_id(&self, prefix: &str) -> String {
        let raw = self.next_raw_id();
        format!("{prefix}_{raw}")
    }

    fn next_raw_id(&self) -> String {
        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;
        format!("{id:026}")
    }

    fn push_error(&self, error: WorkflowCoreError) -> WorkflowCoreError {
        self.inner.borrow_mut().errors.push(error.clone());
        error
    }

    fn queue_counts(&self) -> (usize, usize, usize) {
        let inner = self.inner.borrow();
        inner
            .invocations_queue
            .values()
            .fold((0, 0, 0), |(steps, hooks, waits), item| match item {
                QueueItem::Step(_) => (steps + 1, hooks, waits),
                QueueItem::Hook(_) => (steps, hooks + 1, waits),
                QueueItem::Wait(_) => (steps, hooks, waits + 1),
            })
    }

    fn remove_queue_item(&self, correlation_id: &str) {
        self.inner
            .borrow_mut()
            .invocations_queue
            .remove(correlation_id);
    }
}

/// Rust equivalent of `createUseStep`.
#[derive(Debug, Clone)]
pub struct StepProxy {
    ctx: WorkflowContext,
    step_name: String,
    function_name: String,
    closure_vars: Option<Value>,
    bound_this: Option<Value>,
    bound_args: Vec<Value>,
}

impl StepProxy {
    fn new(ctx: WorkflowContext, step_name: String, closure_vars: Option<Value>) -> Self {
        let function_name = step_name
            .split("//")
            .last()
            .filter(|name| !name.is_empty())
            .unwrap_or(&step_name)
            .to_string();
        Self {
            ctx,
            step_name,
            function_name,
            closure_vars,
            bound_this: None,
            bound_args: Vec::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.function_name
    }

    pub fn step_id(&self) -> &str {
        &self.step_name
    }

    pub fn bind(&self, this_arg: Value, partial_args: Vec<Value>) -> Self {
        let mut bound = self.clone();
        bound.bound_this = Some(this_arg);
        bound.bound_args = partial_args;
        bound
    }

    pub fn invoke(&self, args: Vec<Value>) -> StepInvocation {
        let correlation_id = self.ctx.next_id("step");
        let mut combined_args = self.bound_args.clone();
        combined_args.extend(args);
        self.ctx.inner.borrow_mut().invocations_queue.insert(
            correlation_id.clone(),
            QueueItem::Step(StepQueueItem {
                correlation_id: correlation_id.clone(),
                step_name: self.step_name.clone(),
                args: combined_args,
                this_val: self.bound_this.clone(),
                closure_vars: self.closure_vars.clone(),
                has_created_event: false,
            }),
        );
        StepInvocation {
            ctx: self.ctx.clone(),
            correlation_id,
            step_name: self.step_name.clone(),
        }
    }
}

/// A queued step invocation.
#[derive(Debug, Clone)]
pub struct StepInvocation {
    ctx: WorkflowContext,
    correlation_id: String,
    step_name: String,
}

impl StepInvocation {
    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }

    pub fn finish_without_event(&self) -> Result<StepEventResult> {
        Err(self.ctx.suspension())
    }

    pub fn consume_event(&self, event: &WorkflowEvent) -> Result<StepEventResult> {
        if event.correlation_id != self.correlation_id {
            return Ok(StepEventResult::NotConsumed);
        }
        if let Some(event_step_name) = &event.step_name
            && event_step_name != &self.step_name
        {
            let error = WorkflowCoreError::CorruptedEventLog(format!(
                "step event {:?} for {} belongs to \"{}\", but the current step consumer is \"{}\"",
                event.event_type, event.correlation_id, event_step_name, self.step_name
            ));
            return Err(self.ctx.push_error(error));
        }

        match &event.event_type {
            WorkflowEventType::StepCreated => {
                if let Some(QueueItem::Step(item)) = self
                    .ctx
                    .inner
                    .borrow_mut()
                    .invocations_queue
                    .get_mut(&self.correlation_id)
                {
                    item.has_created_event = true;
                }
                Ok(StepEventResult::Consumed)
            }
            WorkflowEventType::StepStarted | WorkflowEventType::StepRetrying => {
                Ok(StepEventResult::Consumed)
            }
            WorkflowEventType::StepCompleted => {
                self.ctx.remove_queue_item(&self.correlation_id);
                Ok(StepEventResult::Resolved(
                    event.result.clone().unwrap_or(Value::Null),
                ))
            }
            WorkflowEventType::StepFailed => {
                self.ctx.remove_queue_item(&self.correlation_id);
                Err(event
                    .error
                    .clone()
                    .unwrap_or_else(|| WorkflowCoreError::WorkflowRuntime {
                        message: "Step failed".to_string(),
                        slug: None,
                    }))
            }
            other => {
                let error = WorkflowCoreError::CorruptedEventLog(format!(
                    "Unexpected event type for step {} (name: {}) \"{:?}\"",
                    self.correlation_id, self.step_name, other
                ));
                Err(self.ctx.push_error(error))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepEventResult {
    Consumed,
    NotConsumed,
    Resolved(Value),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HookOptions {
    pub token: Option<String>,
    pub metadata: Option<Value>,
    pub is_webhook: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebhookOptions {
    pub token: Option<String>,
    pub respond_with: Option<String>,
}

#[derive(Debug, Clone)]
struct HookState {
    payloads: VecDeque<Value>,
    awaiting: usize,
    disposed: bool,
    has_disposed_event: bool,
    conflict: Option<(String, String)>,
}

/// Hook handle created in workflow context.
#[derive(Debug, Clone)]
pub struct Hook {
    ctx: WorkflowContext,
    correlation_id: String,
    token: String,
    state: Rc<RefCell<HookState>>,
}

impl Hook {
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }

    pub fn await_next(&self) -> Result<Value> {
        let mut state = self.state.borrow_mut();
        if let Some((token, conflicting_run_id)) = &state.conflict {
            return Err(WorkflowCoreError::HookConflict {
                token: token.clone(),
                conflicting_run_id: conflicting_run_id.clone(),
            });
        }
        if let Some(payload) = state.payloads.pop_front() {
            return Ok(payload);
        }
        state.awaiting += 1;
        Err(self.ctx.suspension())
    }

    pub fn iterator_next(&self) -> Result<Value> {
        self.await_next()
    }

    pub fn dispose(&self) -> Result<()> {
        let mut state = self.state.borrow_mut();
        if state.disposed {
            return Ok(());
        }
        state.disposed = true;
        if state.has_disposed_event {
            return Ok(());
        }
        if let Some(QueueItem::Hook(item)) = self
            .ctx
            .inner
            .borrow_mut()
            .invocations_queue
            .get_mut(&self.correlation_id)
        {
            item.disposed = true;
        }
        if state.awaiting > 0 {
            return Err(self.ctx.suspension());
        }
        Ok(())
    }

    pub fn symbol_dispose(&self) -> Result<()> {
        self.dispose()
    }

    pub fn consume_event(&self, event: &WorkflowEvent) -> Result<HookEventResult> {
        if event.correlation_id != self.correlation_id {
            return Ok(HookEventResult::NotConsumed);
        }
        if let Some(event_token) = &event.token
            && event_token != &self.token
        {
            let error = WorkflowCoreError::CorruptedEventLog(format!(
                "hook event {:?} for {} belongs to token \"{}\", but the current hook consumer expects \"{}\"",
                event.event_type, event.correlation_id, event_token, self.token
            ));
            return Err(self.ctx.push_error(error));
        }

        match &event.event_type {
            WorkflowEventType::HookCreated => {
                if let Some(QueueItem::Hook(item)) = self
                    .ctx
                    .inner
                    .borrow_mut()
                    .invocations_queue
                    .get_mut(&self.correlation_id)
                {
                    item.has_created_event = true;
                }
                Ok(HookEventResult::Consumed)
            }
            WorkflowEventType::HookReceived => {
                self.state
                    .borrow_mut()
                    .payloads
                    .push_back(event.payload.clone().unwrap_or(Value::Null));
                Ok(HookEventResult::Consumed)
            }
            WorkflowEventType::HookDisposed => {
                self.ctx.remove_queue_item(&self.correlation_id);
                self.state.borrow_mut().has_disposed_event = true;
                Ok(HookEventResult::Finished)
            }
            WorkflowEventType::HookConflict => {
                self.ctx.remove_queue_item(&self.correlation_id);
                self.state.borrow_mut().conflict = Some((
                    self.token.clone(),
                    event
                        .conflicting_run_id
                        .clone()
                        .unwrap_or_else(|| "unknown-run".to_string()),
                ));
                Ok(HookEventResult::Consumed)
            }
            other => {
                let error = WorkflowCoreError::CorruptedEventLog(format!(
                    "Unexpected event type for hook {} (token: {}) \"{:?}\"",
                    self.correlation_id, self.token, other
                ));
                Err(self.ctx.push_error(error))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEventResult {
    Consumed,
    Finished,
    NotConsumed,
}

/// Webhook handle created from a hook with a public URL.
#[derive(Debug, Clone)]
pub struct Webhook {
    pub hook: Hook,
    pub url: String,
}

fn create_hook_in_context(ctx: WorkflowContext, options: HookOptions, is_system: bool) -> Hook {
    let correlation_id = ctx.next_id("hook");
    let token = options.token.unwrap_or_else(|| ctx.next_id("tok"));
    ctx.inner.borrow_mut().invocations_queue.insert(
        correlation_id.clone(),
        QueueItem::Hook(HookQueueItem {
            correlation_id: correlation_id.clone(),
            token: token.clone(),
            metadata: options.metadata,
            is_webhook: options.is_webhook,
            is_system,
            disposed: false,
            abort_requested: false,
            abort_reason: None,
            has_created_event: false,
        }),
    );
    Hook {
        ctx,
        correlation_id,
        token,
        state: Rc::new(RefCell::new(HookState {
            payloads: VecDeque::new(),
            awaiting: 0,
            disposed: false,
            has_disposed_event: false,
            conflict: None,
        })),
    }
}

/// A typed hook facade, modelling the TypeScript `defineHook` helper.
#[derive(Clone)]
pub struct TypedHook {
    validator: Option<Rc<dyn Fn(Value) -> Result<Value>>>,
}

impl fmt::Debug for TypedHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedHook")
            .field("has_validator", &self.validator.is_some())
            .finish()
    }
}

impl TypedHook {
    pub fn create(&self, ctx: &WorkflowContext, options: HookOptions) -> Hook {
        ctx.create_hook(options)
    }

    pub fn resume<R: HookResumer>(
        &self,
        resumer: &mut R,
        token: impl Into<String>,
        payload: Value,
    ) -> Result<HookEntity> {
        let token = token.into();
        let payload = match &self.validator {
            Some(validator) => validator(payload)?,
            None => payload,
        };
        resumer.resume_hook(token, payload)
    }
}

pub fn define_hook() -> TypedHook {
    TypedHook { validator: None }
}

pub fn define_hook_with_schema(validator: impl Fn(Value) -> Result<Value> + 'static) -> TypedHook {
    TypedHook {
        validator: Some(Rc::new(validator)),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookEntity {
    pub token: String,
    pub payload: Value,
}

pub trait HookResumer {
    fn resume_hook(&mut self, token: String, payload: Value) -> Result<HookEntity>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SleepInput {
    DurationMillis(i64),
    DurationString(String),
    Until(TimestampMillis),
}

impl From<i64> for SleepInput {
    fn from(value: i64) -> Self {
        Self::DurationMillis(value)
    }
}

impl From<&str> for SleepInput {
    fn from(value: &str) -> Self {
        Self::DurationString(value.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct SleepCall {
    ctx: WorkflowContext,
    correlation_id: String,
    resume_at: TimestampMillis,
}

impl SleepCall {
    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }

    pub fn resume_at(&self) -> TimestampMillis {
        self.resume_at
    }

    pub fn finish_without_event(&self) -> Result<()> {
        Err(self.ctx.suspension())
    }

    pub fn consume_event(&self, event: &WorkflowEvent) -> Result<SleepEventResult> {
        if event.correlation_id != self.correlation_id {
            return Ok(SleepEventResult::NotConsumed);
        }
        match event.event_type {
            WorkflowEventType::WaitCreated => {
                if let Some(QueueItem::Wait(item)) = self
                    .ctx
                    .inner
                    .borrow_mut()
                    .invocations_queue
                    .get_mut(&self.correlation_id)
                {
                    item.has_created_event = true;
                    if let Some(resume_at) = event.resume_at {
                        item.resume_at = resume_at;
                    }
                }
                Ok(SleepEventResult::Consumed)
            }
            WorkflowEventType::WaitCompleted => {
                if let Some(event_resume_at) = event.resume_at
                    && event_resume_at != self.resume_at
                {
                    let error = WorkflowCoreError::CorruptedEventLog(format!(
                        "wait_completed event for {} has resumeAt \"{}\", but the current wait consumer expects \"{}\"",
                        self.correlation_id, event_resume_at, self.resume_at
                    ));
                    return Err(self.ctx.push_error(error));
                }
                self.ctx.remove_queue_item(&self.correlation_id);
                Ok(SleepEventResult::Resolved)
            }
            ref other => {
                let error = WorkflowCoreError::CorruptedEventLog(format!(
                    "Unexpected event type for wait {} \"{:?}\"",
                    self.correlation_id, other
                ));
                Err(self.ctx.push_error(error))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SleepEventResult {
    Consumed,
    NotConsumed,
    Resolved,
}

pub fn parse_sleep_input(now: TimestampMillis, input: SleepInput) -> Result<TimestampMillis> {
    match input {
        SleepInput::DurationMillis(ms) => Ok(now + ms),
        SleepInput::Until(timestamp) => Ok(timestamp),
        SleepInput::DurationString(value) => {
            let (number, multiplier) = if let Some(number) = value.strip_suffix("ms") {
                (number, 1)
            } else if let Some(number) = value.strip_suffix('s') {
                (number, 1_000)
            } else if let Some(number) = value.strip_suffix('m') {
                (number, 60_000)
            } else if let Some(number) = value.strip_suffix('h') {
                (number, 3_600_000)
            } else if let Some(number) = value.strip_suffix('d') {
                (number, 86_400_000)
            } else {
                return Err(WorkflowCoreError::WorkflowRuntime {
                    message: format!("Invalid sleep duration \"{value}\""),
                    slug: None,
                });
            };
            let amount = number
                .parse::<i64>()
                .map_err(|_| WorkflowCoreError::WorkflowRuntime {
                    message: format!("Invalid sleep duration \"{value}\""),
                    slug: None,
                })?;
            Ok(now + amount * multiplier)
        }
    }
}

#[derive(Clone)]
pub struct WorkflowAbortSignal {
    inner: Rc<RefCell<AbortSignalState>>,
}

struct AbortSignalState {
    aborted: bool,
    reason: Option<Value>,
    stream_name: String,
    hook_token: String,
    listeners: BTreeMap<usize, Box<dyn FnMut()>>,
    next_listener_id: usize,
}

type AbortListenerRef = (Weak<RefCell<AbortSignalState>>, usize);
type AbortListenerRefs = Rc<RefCell<Vec<AbortListenerRef>>>;

impl fmt::Debug for WorkflowAbortSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.borrow();
        f.debug_struct("WorkflowAbortSignal")
            .field("aborted", &inner.aborted)
            .field("reason", &inner.reason)
            .field("stream_name", &inner.stream_name)
            .field("hook_token", &inner.hook_token)
            .field("listener_count", &inner.listeners.len())
            .finish()
    }
}

impl WorkflowAbortSignal {
    pub fn new(stream_name: impl Into<String>, hook_token: impl Into<String>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(AbortSignalState {
                aborted: false,
                reason: None,
                stream_name: stream_name.into(),
                hook_token: hook_token.into(),
                listeners: BTreeMap::new(),
                next_listener_id: 1,
            })),
        }
    }

    pub fn abort(reason: Option<Value>) -> Self {
        let signal = Self::new("", "");
        signal.set_aborted(Some(reason.unwrap_or_else(|| {
            Value::String("The operation was aborted.".to_string())
        })));
        signal
    }

    pub fn any(signals: &[WorkflowAbortSignal]) -> Self {
        let composite = Self::new("", "");
        for signal in signals {
            if signal.aborted() {
                composite.set_aborted(signal.reason());
                return composite;
            }
        }

        let listener_ids: AbortListenerRefs = Rc::new(RefCell::new(Vec::new()));
        for signal in signals {
            let composite_for_listener = composite.clone();
            let input_for_listener = signal.clone();
            let listener_ids_for_listener = Rc::clone(&listener_ids);
            let listener_id = signal.add_event_listener(move || {
                if !composite_for_listener.aborted() {
                    composite_for_listener.set_aborted(input_for_listener.reason());
                    for (weak, id) in listener_ids_for_listener.borrow_mut().drain(..) {
                        if let Some(inner) = weak.upgrade() {
                            inner.borrow_mut().listeners.remove(&id);
                        }
                    }
                }
            });
            listener_ids
                .borrow_mut()
                .push((Rc::downgrade(&signal.inner), listener_id));
        }
        composite
    }

    pub fn timeout() -> Result<Self> {
        Err(WorkflowCoreError::TimeoutFunctionUnavailable {
            slug: "abort-signal-timeout-in-workflow",
        })
    }

    pub fn aborted(&self) -> bool {
        self.inner.borrow().aborted
    }

    pub fn reason(&self) -> Option<Value> {
        self.inner.borrow().reason.clone()
    }

    pub fn stream_name(&self) -> String {
        self.inner.borrow().stream_name.clone()
    }

    pub fn hook_token(&self) -> String {
        self.inner.borrow().hook_token.clone()
    }

    pub fn listener_count(&self) -> usize {
        self.inner.borrow().listeners.len()
    }

    pub fn add_event_listener(&self, listener: impl FnMut() + 'static) -> usize {
        if self.aborted() {
            let mut listener = listener;
            listener();
            return 0;
        }
        let mut inner = self.inner.borrow_mut();
        let listener_id = inner.next_listener_id;
        inner.next_listener_id += 1;
        inner.listeners.insert(listener_id, Box::new(listener));
        listener_id
    }

    pub fn remove_event_listener(&self, listener_id: usize) {
        self.inner.borrow_mut().listeners.remove(&listener_id);
    }

    pub fn throw_if_aborted(&self) -> Result<()> {
        if self.aborted() {
            return Err(WorkflowCoreError::Fatal(format!(
                "Aborted: {}",
                self.reason()
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "The operation was aborted.".to_string())
            )));
        }
        Ok(())
    }

    pub fn receive_stream_abort(&self, reason: Option<Value>) {
        self.set_aborted(reason);
    }

    fn set_aborted(&self, reason: Option<Value>) {
        let listeners = {
            let mut inner = self.inner.borrow_mut();
            if inner.aborted {
                return;
            }
            inner.aborted = true;
            inner.reason = reason;
            std::mem::take(&mut inner.listeners)
        };
        for mut listener in listeners.into_values() {
            listener();
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowAbortController {
    pub signal: WorkflowAbortSignal,
    ctx: Option<WorkflowContext>,
    hook_correlation_id: Option<String>,
}

impl WorkflowAbortController {
    pub fn new_in_context(ctx: WorkflowContext) -> Self {
        let id = ctx.next_raw_id();
        let stream_name = get_abort_stream_id(&id);
        let hook_token = format!("abrt_{id}");
        let correlation_id = ctx.next_id("hook");
        ctx.inner.borrow_mut().invocations_queue.insert(
            correlation_id.clone(),
            QueueItem::Hook(HookQueueItem {
                correlation_id: correlation_id.clone(),
                token: hook_token.clone(),
                metadata: None,
                is_webhook: false,
                is_system: true,
                disposed: false,
                abort_requested: false,
                abort_reason: None,
                has_created_event: false,
            }),
        );
        Self {
            signal: WorkflowAbortSignal::new(stream_name, hook_token),
            ctx: Some(ctx),
            hook_correlation_id: Some(correlation_id),
        }
    }

    pub fn deserialized(stream_name: impl Into<String>, hook_token: impl Into<String>) -> Self {
        Self {
            signal: WorkflowAbortSignal::new(stream_name, hook_token),
            ctx: None,
            hook_correlation_id: None,
        }
    }

    pub fn abort(&self, reason: Option<Value>) {
        if self.signal.aborted() {
            return;
        }
        self.signal.set_aborted(reason.clone());
        if let (Some(ctx), Some(correlation_id)) = (&self.ctx, &self.hook_correlation_id)
            && let Some(QueueItem::Hook(item)) = ctx
                .inner
                .borrow_mut()
                .invocations_queue
                .get_mut(correlation_id)
        {
            item.abort_requested = true;
            item.abort_reason = reason;
        }
    }

    pub fn abort_from_step_context(&self, ctx: &StepContext, reason: Option<Value>) {
        if self.signal.aborted() {
            return;
        }
        self.signal.set_aborted(reason);
        ctx.push_op();
        ctx.push_op();
    }

    pub fn consume_hook_event(&self, event: &WorkflowEvent) -> Result<HookEventResult> {
        let Some(correlation_id) = &self.hook_correlation_id else {
            return Ok(HookEventResult::NotConsumed);
        };
        if event.correlation_id != *correlation_id {
            return Ok(HookEventResult::NotConsumed);
        }
        if let Some(event_token) = &event.token
            && event_token != &self.signal.hook_token()
        {
            let error = WorkflowCoreError::CorruptedEventLog(format!(
                "abort hook event {:?} for {} belongs to token \"{}\", but the current abort hook expects \"{}\"",
                event.event_type,
                event.correlation_id,
                event_token,
                self.signal.hook_token()
            ));
            if let Some(ctx) = &self.ctx {
                return Err(ctx.push_error(error));
            }
            return Err(error);
        }

        match event.event_type {
            WorkflowEventType::HookCreated => {
                if let Some(ctx) = &self.ctx
                    && let Some(QueueItem::Hook(item)) = ctx
                        .inner
                        .borrow_mut()
                        .invocations_queue
                        .get_mut(correlation_id)
                {
                    item.has_created_event = true;
                }
                Ok(HookEventResult::Consumed)
            }
            WorkflowEventType::HookReceived => {
                let reason = event
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.get("reason").cloned());
                self.signal.set_aborted(reason);
                if let Some(ctx) = &self.ctx {
                    ctx.remove_queue_item(correlation_id);
                }
                Ok(HookEventResult::Consumed)
            }
            WorkflowEventType::HookDisposed => {
                if let Some(ctx) = &self.ctx {
                    ctx.remove_queue_item(correlation_id);
                }
                Ok(HookEventResult::Finished)
            }
            _ => Ok(HookEventResult::NotConsumed),
        }
    }
}

/// Process-local step context.
#[derive(Clone)]
pub struct StepContext {
    pub step_metadata: StepMetadata,
    pub workflow_metadata: WorkflowMetadata,
    ops_pending: Rc<Cell<usize>>,
    writables: Rc<RefCell<BTreeMap<String, StepWritable>>>,
    registered_pipes: Rc<Cell<usize>>,
    written_chunks: Rc<RefCell<Vec<Value>>>,
}

impl fmt::Debug for StepContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StepContext")
            .field("step_metadata", &self.step_metadata)
            .field("workflow_metadata", &self.workflow_metadata)
            .field("ops_pending", &self.ops_pending())
            .field("registered_pipes", &self.registered_pipes())
            .finish()
    }
}

impl StepContext {
    pub fn new(step_metadata: StepMetadata, workflow_metadata: WorkflowMetadata) -> Self {
        Self {
            step_metadata,
            workflow_metadata,
            ops_pending: Rc::new(Cell::new(0)),
            writables: Rc::new(RefCell::new(BTreeMap::new())),
            registered_pipes: Rc::new(Cell::new(0)),
            written_chunks: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn ops_pending(&self) -> usize {
        self.ops_pending.get()
    }

    pub fn push_op(&self) {
        self.ops_pending.set(self.ops_pending.get() + 1);
    }

    pub fn complete_op(&self) {
        self.ops_pending
            .set(self.ops_pending.get().saturating_sub(1));
    }

    pub fn registered_pipes(&self) -> usize {
        self.registered_pipes.get()
    }

    pub fn written_chunks(&self) -> Vec<Value> {
        self.written_chunks.borrow().clone()
    }
}

thread_local! {
    static STEP_CONTEXT_SLOT: RefCell<Option<StepContext>> = const { RefCell::new(None) };
}

/// AsyncLocalStorage-shaped singleton for Rust step execution.
#[derive(Debug)]
pub struct ContextStorage;

impl ContextStorage {
    pub fn global() -> &'static Self {
        static STORAGE: ContextStorage = ContextStorage;
        &STORAGE
    }

    pub fn run<T>(&self, ctx: StepContext, body: impl FnOnce() -> T) -> T {
        STEP_CONTEXT_SLOT.with(|slot| {
            let previous = slot.replace(Some(ctx));
            let result = body();
            slot.replace(previous);
            result
        })
    }

    pub fn get_store(&self) -> Option<StepContext> {
        STEP_CONTEXT_SLOT.with(|slot| slot.borrow().clone())
    }
}

pub fn context_storage() -> &'static ContextStorage {
    ContextStorage::global()
}

pub fn get_step_metadata() -> Result<StepMetadata> {
    context_storage()
        .get_store()
        .map(|ctx| ctx.step_metadata)
        .ok_or_else(|| WorkflowCoreError::NotInStepContext {
            function_name: "getStepMetadata()".to_string(),
            docs_url: "https://workflow-sdk.dev/docs/api-reference/workflow/get-step-metadata"
                .to_string(),
        })
}

pub fn get_workflow_metadata_from_step() -> Result<WorkflowMetadata> {
    context_storage()
        .get_store()
        .map(|ctx| ctx.workflow_metadata)
        .ok_or_else(|| WorkflowCoreError::NotInWorkflowOrStepContext {
            function_name: "getWorkflowMetadata()".to_string(),
            docs_url: "https://workflow-sdk.dev/docs/api-reference/workflow/get-workflow-metadata"
                .to_string(),
        })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowWritableStreamOptions {
    pub namespace: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StepWritable {
    state: Rc<RefCell<StepWritableState>>,
}

#[derive(Debug)]
struct StepWritableState {
    name: String,
    run_id: String,
    locked: bool,
    ctx: StepContext,
}

impl StepWritable {
    pub fn name(&self) -> String {
        self.state.borrow().name.clone()
    }

    pub fn run_id(&self) -> String {
        self.state.borrow().run_id.clone()
    }

    pub fn same_handle(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }

    pub fn get_writer(&self) -> StepWritableWriter {
        let mut state = self.state.borrow_mut();
        if !state.locked {
            state.locked = true;
            state.ctx.ops_pending.set(state.ctx.ops_pending.get() + 1);
        }
        StepWritableWriter {
            writable: self.clone(),
            released: false,
        }
    }

    pub fn write_unlocked(&self, chunk: Value) {
        self.state
            .borrow()
            .ctx
            .written_chunks
            .borrow_mut()
            .push(chunk);
    }
}

#[derive(Debug)]
pub struct StepWritableWriter {
    writable: StepWritable,
    released: bool,
}

impl StepWritableWriter {
    pub fn write(&self, chunk: Value) {
        self.writable.write_unlocked(chunk);
    }

    pub fn release_lock(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        let mut state = self.writable.state.borrow_mut();
        if state.locked {
            state.locked = false;
            state
                .ctx
                .ops_pending
                .set(state.ctx.ops_pending.get().saturating_sub(1));
        }
    }

    pub fn close(mut self) {
        self.release_lock();
    }
}

impl Drop for StepWritableWriter {
    fn drop(&mut self) {
        self.release_lock();
    }
}

pub fn get_writable(options: WorkflowWritableStreamOptions) -> Result<StepWritable> {
    let ctx = context_storage().get_store().ok_or_else(|| {
        WorkflowCoreError::NotInWorkflowOrStepContext {
            function_name: "getWritable()".to_string(),
            docs_url: "https://workflow-sdk.dev/docs/api-reference/workflow/get-writable"
                .to_string(),
        }
    })?;
    let name = get_workflow_run_stream_id(
        &ctx.workflow_metadata.workflow_run_id,
        options.namespace.as_deref(),
    );
    if let Some(writable) = ctx.writables.borrow().get(&name) {
        return Ok(writable.clone());
    }
    ctx.registered_pipes.set(ctx.registered_pipes.get() + 1);
    let writable = StepWritable {
        state: Rc::new(RefCell::new(StepWritableState {
            name: name.clone(),
            run_id: ctx.workflow_metadata.workflow_run_id.clone(),
            locked: false,
            ctx: ctx.clone(),
        })),
    };
    ctx.writables.borrow_mut().insert(name, writable.clone());
    Ok(writable)
}

pub fn attach_abort_stream_reader(signal: &WorkflowAbortSignal, ctx: &StepContext) -> bool {
    if signal.aborted() {
        return false;
    }
    ctx.push_op();
    true
}

pub fn deliver_abort_stream_packet(
    signal: &WorkflowAbortSignal,
    ctx: &StepContext,
    reason: Option<Value>,
) {
    signal.receive_stream_abort(reason);
    ctx.complete_op();
}

/// In-memory server side used by `WorkflowServerWritableStream`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MemoryStreamWorld {
    pub writes: Vec<StreamWrite>,
    pub closes: Vec<(String, String)>,
    pub supports_write_multi: bool,
    pub fail_writes: Option<String>,
    pub fail_close: Option<String>,
    pub stream_flush_interval_ms: Option<u64>,
    pub write_calls: usize,
    pub write_multi_calls: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamWrite {
    pub run_id: String,
    pub name: String,
    pub chunk: Value,
}

#[derive(Debug, Clone)]
pub struct WorkflowServerWritableStream {
    run_id: String,
    name: String,
    world: Rc<RefCell<MemoryStreamWorld>>,
    buffer: Vec<Value>,
    aborted: bool,
}

impl WorkflowServerWritableStream {
    pub fn try_new(
        run_id: impl Into<String>,
        name: impl Into<String>,
        world: Rc<RefCell<MemoryStreamWorld>>,
    ) -> Result<Self> {
        Self::try_new_checked(Some(run_id.into()), name.into(), world)
    }

    pub fn try_new_checked(
        run_id: Option<String>,
        name: impl Into<String>,
        world: Rc<RefCell<MemoryStreamWorld>>,
    ) -> Result<Self> {
        let run_id = run_id.ok_or_else(|| WorkflowCoreError::WorkflowRuntime {
            message: "WorkflowServerWritableStream runId must be a string".to_string(),
            slug: None,
        })?;
        let name = name.into();
        if name.is_empty() {
            return Err(WorkflowCoreError::WorkflowRuntime {
                message: "WorkflowServerWritableStream name must not be empty".to_string(),
                slug: None,
            });
        }
        Ok(Self {
            run_id,
            name,
            world,
            buffer: Vec::new(),
            aborted: false,
        })
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn write(&mut self, chunk: Value) -> Result<()> {
        self.flush_values(vec![chunk])
    }

    pub fn buffer(&mut self, chunk: Value) {
        if !self.aborted {
            self.buffer.push(chunk);
        }
    }

    pub fn flush(&mut self) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let chunks = std::mem::take(&mut self.buffer);
        self.flush_values(chunks)
    }

    pub fn close(&mut self) -> Result<()> {
        if self.aborted {
            return Ok(());
        }
        self.flush()?;
        let mut world = self.world.borrow_mut();
        if let Some(message) = &world.fail_close {
            return Err(WorkflowCoreError::WorkflowRuntime {
                message: message.clone(),
                slug: None,
            });
        }
        world.closes.push((self.run_id.clone(), self.name.clone()));
        Ok(())
    }

    pub fn abort(&mut self) {
        self.aborted = true;
        self.buffer.clear();
    }

    pub fn flush_interval_ms(&self) -> u64 {
        self.world.borrow().stream_flush_interval_ms.unwrap_or(100)
    }

    fn flush_values(&mut self, chunks: Vec<Value>) -> Result<()> {
        if self.aborted || chunks.is_empty() {
            return Ok(());
        }
        let mut world = self.world.borrow_mut();
        if let Some(message) = &world.fail_writes {
            return Err(WorkflowCoreError::WorkflowRuntime {
                message: message.clone(),
                slug: None,
            });
        }
        if chunks.len() > 1 && world.supports_write_multi {
            world.write_multi_calls += 1;
        } else {
            world.write_calls += chunks.len();
        }
        for chunk in chunks {
            world.writes.push(StreamWrite {
                run_id: self.run_id.clone(),
                name: self.name.clone(),
                chunk,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowBody {
    Text(String),
    Bytes(Vec<u8>),
}

impl WorkflowBody {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    pub fn bytes(value: impl Into<Vec<u8>>) -> Self {
        Self::Bytes(value.into())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers(BTreeMap<String, String>);

impl Headers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: impl AsRef<str>, value: impl Into<String>) {
        self.0
            .insert(key.as_ref().to_ascii_lowercase(), value.into());
    }

    pub fn get(&self, key: impl AsRef<str>) -> Option<&str> {
        self.0
            .get(&key.as_ref().to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn has(&self, key: impl AsRef<str>) -> bool {
        self.0.contains_key(&key.as_ref().to_ascii_lowercase())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Headers,
    pub body: Option<WorkflowBody>,
    pub redirected: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResponseInit {
    pub status: Option<u16>,
    pub status_text: Option<String>,
    pub headers: Headers,
}

impl WorkflowResponse {
    pub fn new(body: Option<WorkflowBody>, init: ResponseInit) -> Result<Self> {
        let status = init.status.unwrap_or(200);
        if body.is_some() && matches!(status, 204 | 205 | 304) {
            return Err(WorkflowCoreError::WorkflowRuntime {
                message: format!("Response constructor: Invalid response status code {status}"),
                slug: None,
            });
        }
        Ok(Self {
            status,
            status_text: init.status_text.unwrap_or_default(),
            headers: init.headers,
            body,
            redirected: false,
        })
    }

    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn json(data: &Value, mut init: ResponseInit) -> Result<Self> {
        if !init.headers.has("content-type") {
            init.headers.set("content-type", "application/json");
        }
        Self::new(Some(WorkflowBody::Text(data.to_string())), init)
    }

    pub fn redirect(url: impl Into<String>, status: Option<u16>) -> Result<Self> {
        let status = status.unwrap_or(302);
        if !matches!(status, 301 | 302 | 303 | 307 | 308) {
            return Err(WorkflowCoreError::WorkflowRuntime {
                message: format!(
                    "Invalid redirect status code: {status}. Must be one of: 301, 302, 303, 307, 308"
                ),
                slug: None,
            });
        }
        let mut headers = Headers::new();
        headers.set("location", url.into());
        Ok(Self {
            status,
            status_text: String::new(),
            headers,
            body: None,
            redirected: false,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRequest {
    pub url: String,
    pub method: String,
    pub headers: Headers,
    pub body: Option<WorkflowBody>,
    pub mode: String,
    pub credentials: String,
    pub cache: String,
    pub redirect: String,
    pub referrer: String,
    pub referrer_policy: String,
    pub integrity: String,
    pub keepalive: bool,
    pub duplex: String,
    pub destination: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestInit {
    pub method: Option<String>,
    pub headers: Headers,
    pub body: Option<WorkflowBody>,
    pub mode: Option<String>,
    pub credentials: Option<String>,
    pub cache: Option<String>,
    pub redirect: Option<String>,
    pub referrer: Option<String>,
    pub referrer_policy: Option<String>,
    pub integrity: Option<String>,
    pub keepalive: Option<bool>,
}

impl WorkflowRequest {
    pub fn new(url: impl Into<String>, init: RequestInit) -> Result<Self> {
        let url = url.into();
        url::Url::parse(&url).map_err(|cause| WorkflowCoreError::WorkflowRuntime {
            message: format!("Failed to parse URL from {url}: {cause}"),
            slug: None,
        })?;
        let method = init
            .method
            .unwrap_or_else(|| "GET".to_string())
            .to_uppercase();
        if init.body.is_some() && matches!(method.as_str(), "GET" | "HEAD") {
            return Err(WorkflowCoreError::WorkflowRuntime {
                message: "Request with GET/HEAD method cannot have body.".to_string(),
                slug: None,
            });
        }
        Ok(Self {
            url,
            method,
            headers: init.headers,
            body: init.body,
            mode: init.mode.unwrap_or_else(|| "cors".to_string()),
            credentials: init
                .credentials
                .unwrap_or_else(|| "same-origin".to_string()),
            cache: init.cache.unwrap_or_else(|| "default".to_string()),
            redirect: init.redirect.unwrap_or_else(|| "follow".to_string()),
            referrer: init.referrer.unwrap_or_else(|| "about:client".to_string()),
            referrer_policy: init.referrer_policy.unwrap_or_default(),
            integrity: init.integrity.unwrap_or_default(),
            keepalive: init.keepalive.unwrap_or(false),
            duplex: "half".to_string(),
            destination: "document".to_string(),
        })
    }

    pub fn clone_with_init(&self, init: RequestInit) -> Result<Self> {
        let mut clone = self.clone();
        if let Some(method) = init.method {
            clone.method = method.to_uppercase();
        }
        if init.headers != Headers::default() {
            clone.headers = init.headers;
        }
        if init.body.is_some() && matches!(clone.method.as_str(), "GET" | "HEAD") {
            return Err(WorkflowCoreError::WorkflowRuntime {
                message: "Request with GET/HEAD method cannot have body.".to_string(),
                slug: None,
            });
        }
        if let Some(body) = init.body {
            clone.body = Some(body);
        }
        if let Some(mode) = init.mode {
            clone.mode = mode;
        }
        if let Some(credentials) = init.credentials {
            clone.credentials = credentials;
        }
        if let Some(cache) = init.cache {
            clone.cache = cache;
        }
        if let Some(redirect) = init.redirect {
            clone.redirect = redirect;
        }
        if let Some(referrer) = init.referrer {
            clone.referrer = referrer;
        }
        if let Some(referrer_policy) = init.referrer_policy {
            clone.referrer_policy = referrer_policy;
        }
        if let Some(integrity) = init.integrity {
            clone.integrity = integrity;
        }
        if let Some(keepalive) = init.keepalive {
            clone.keepalive = keepalive;
        }
        Ok(clone)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownError {
    pub name: String,
    pub message: String,
    pub stack: Option<String>,
    pub fatal: bool,
}

pub fn is_abort_error(value: &UnknownError) -> bool {
    value.name == "AbortError"
}

pub fn promote_abort_error_to_fatal(value: UnknownError) -> UnknownError {
    if !is_abort_error(&value) || value.fatal {
        return value;
    }
    UnknownError {
        name: "FatalError".to_string(),
        message: format!("Aborted: {}", value.message),
        stack: value.stack,
        fatal: true,
    }
}

pub fn not_in_workflow_context(function_name: impl Into<String>) -> WorkflowCoreError {
    WorkflowCoreError::NotInWorkflowContext {
        function_name: function_name.into(),
        docs_url: "https://workflow-sdk.dev/docs/api-reference/workflow".to_string(),
    }
}

pub fn unavailable_in_workflow_context(
    function_name: impl Into<String>,
    workflow_name: Option<String>,
) -> WorkflowCoreError {
    WorkflowCoreError::UnavailableInWorkflowContext {
        function_name: function_name.into(),
        workflow_name,
        docs_url: "https://workflow-sdk.dev/docs/api-reference/workflow-api/resume-hook"
            .to_string(),
    }
}

pub fn timeout_function_error() -> WorkflowCoreError {
    WorkflowCoreError::TimeoutFunctionUnavailable {
        slug: "timeout-functions-in-workflow",
    }
}

pub fn fetch_in_workflow_error() -> WorkflowCoreError {
    WorkflowCoreError::WorkflowRuntime {
        message: "Global \"fetch\" is unavailable in workflow functions. Use the \"fetch\" step function from \"workflow\" to make HTTP requests.".to_string(),
        slug: Some("fetch-in-workflow-function"),
    }
}

pub fn get_workflow_run_stream_id(run_id: &str, namespace: Option<&str>) -> String {
    let stream_id = format!("{}_user", run_id.replacen("wrun_", "strm_", 1));
    match namespace {
        Some(namespace) if !namespace.is_empty() => {
            format!("{stream_id}_{}", URL_SAFE_NO_PAD.encode(namespace))
        }
        _ => stream_id,
    }
}

pub fn get_abort_stream_id(id: &str) -> String {
    format!("strm_{id}_system_abort")
}

pub fn get_abort_stream_id_from_token(hook_token: &str) -> Result<String> {
    hook_token
        .strip_prefix("abrt_")
        .map(get_abort_stream_id)
        .ok_or_else(|| WorkflowCoreError::WorkflowRuntime {
            message: format!(
                "Invalid abort hook token format: expected \"abrt_\" prefix, got \"{hook_token}\""
            ),
            slug: None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn records_core_source_snapshot() {
        assert_eq!(UPSTREAM_PACKAGE, "@workflow/core");
        assert_eq!(UPSTREAM_VERSION, "5.0.0-beta.10");
        assert_eq!(UPSTREAM_HEAD.len(), 40);
    }

    fn event(event_id: &str, event_type: EventKind) -> Event {
        Event {
            event_id: event_id.to_string(),
            run_id: "wrun_test".to_string(),
            event_type,
            correlation_id: None,
            event_data: Value::Null,
            request_id: None,
            spec_version: SPEC_VERSION_CURRENT,
            created_at_ms: 0,
        }
    }

    fn event_with_data(
        event_id: &str,
        event_type: EventKind,
        correlation_id: &str,
        event_data: Value,
    ) -> Event {
        Event {
            correlation_id: Some(correlation_id.to_string()),
            event_data,
            ..event(event_id, event_type)
        }
    }

    fn running_run(run_id: &str) -> WorkflowRun {
        WorkflowRun {
            run_id: run_id.to_string(),
            workflow_name: "workflow".to_string(),
            status: RunStatus::Running,
            input: Value::Null,
            output: None,
            error: None,
            error_code: None,
            deployment_id: Some("deploy".to_string()),
            spec_version: SPEC_VERSION_CURRENT,
            created_at_ms: 0,
            started_at_ms: Some(0),
            completed_at_ms: None,
        }
    }

    fn world_with_running_run(run_id: &str) -> InMemoryWorld {
        let mut world = InMemoryWorld::new();
        world.insert_run(running_run(run_id));
        world
    }

    fn create_step(world: &mut InMemoryWorld, run_id: &str, step_id: &str, step_name: &str) {
        world
            .events_create(
                run_id,
                CreateEventRequest::new(EventKind::StepCreated)
                    .with_correlation_id(step_id)
                    .with_data(json!({ "stepName": step_name, "input": null })),
                EventCreateOptions::default(),
            )
            .expect("step_created");
    }

    fn step_params(run_id: &str, step_id: &str, step_name: &str) -> StepExecutorParams {
        StepExecutorParams {
            workflow_run_id: run_id.to_string(),
            workflow_name: "workflow".to_string(),
            workflow_started_at_ms: 0,
            step_id: step_id.to_string(),
            step_name: step_name.to_string(),
            request_id: Some("req_test".to_string()),
        }
    }

    #[test]
    fn events_consumer_initializes_and_subscribes_callbacks() {
        let first = event("event-1", EventKind::RunCreated);
        let mut consumer = EventsConsumer::new(vec![first.clone()]);
        assert_eq!(consumer.events(), &[first]);
        assert_eq!(consumer.event_index(), 0);

        let order = Rc::new(RefCell::new(Vec::new()));
        let first_order = Rc::clone(&order);
        consumer.subscribe(move |_| {
            first_order.borrow_mut().push("first");
            EventConsumerResult::NotConsumed
        });
        let second_order = Rc::clone(&order);
        consumer.subscribe(move |event| {
            if let Some(event) = event {
                assert_eq!(event.event_id, "event-1");
                second_order.borrow_mut().push("second");
                EventConsumerResult::Consumed
            } else {
                EventConsumerResult::NotConsumed
            }
        });

        assert_eq!(consumer.callback_count(), 2);
        assert_eq!(consumer.event_index(), 1);
        assert_eq!(&*order.borrow(), &["first", "first", "second", "first"]);
    }

    #[test]
    fn events_consumer_consumes_finished_not_consumed_and_null_events() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut empty = EventsConsumer::new(Vec::new());
        let empty_calls = Rc::clone(&calls);
        empty.subscribe(move |event| {
            empty_calls.borrow_mut().push(event.is_none());
            EventConsumerResult::NotConsumed
        });
        empty.flush_deferred_unconsumed_check();
        assert_eq!(&*calls.borrow(), &[true]);
        assert!(empty.unconsumed_events().is_empty());

        let mut consumer = EventsConsumer::new(vec![
            event("event-1", EventKind::RunCreated),
            event("event-2", EventKind::RunStarted),
        ]);
        consumer.subscribe(|event| {
            assert_eq!(event.expect("event").event_id, "event-1");
            EventConsumerResult::Finished
        });
        assert_eq!(consumer.event_index(), 1);
        assert_eq!(consumer.callback_count(), 0);

        consumer.subscribe(|event| {
            assert_eq!(event.expect("event").event_id, "event-2");
            EventConsumerResult::NotConsumed
        });
        assert_eq!(consumer.event_index(), 1);
        consumer.flush_deferred_unconsumed_check();
        assert_eq!(consumer.unconsumed_events()[0].event_id, "event-2");
    }

    #[test]
    fn events_consumer_complex_sequences_and_callback_errors() {
        let consumed = Rc::new(RefCell::new(Vec::new()));
        let mut consumer = EventsConsumer::new(vec![
            event_with_data("event-1", EventKind::HookReceived, "hook", Value::Null),
            event_with_data(
                "event-2",
                EventKind::HookReceived,
                "hook",
                json!({"payload": null}),
            ),
        ]);

        consumer.subscribe(|_| EventConsumerResult::Errored);
        let consumed_first = Rc::clone(&consumed);
        consumer.subscribe(move |event| {
            if let Some(event) = event {
                consumed_first.borrow_mut().push(event.event_id.clone());
                EventConsumerResult::Consumed
            } else {
                EventConsumerResult::NotConsumed
            }
        });

        assert_eq!(
            &*consumed.borrow(),
            &["event-1".to_string(), "event-2".to_string()]
        );
        assert_eq!(consumer.event_index(), 2);
        assert!(consumer.callback_errors().len() >= 2);
    }

    #[test]
    fn events_consumer_defers_and_cancels_unconsumed_checks() {
        let mut consumer = EventsConsumer::new(vec![event("event-1", EventKind::RunCreated)]);
        consumer.subscribe(|_| EventConsumerResult::NotConsumed);
        assert!(consumer.unconsumed_events().is_empty());
        consumer.flush_deferred_unconsumed_check();
        assert_eq!(consumer.unconsumed_events().len(), 1);

        let mut consumer = EventsConsumer::new(vec![event("event-2", EventKind::RunCreated)]);
        consumer.subscribe(|_| EventConsumerResult::NotConsumed);
        consumer.subscribe(|event| {
            assert_eq!(event.expect("event").event_id, "event-2");
            EventConsumerResult::Finished
        });
        consumer.flush_deferred_unconsumed_check();
        assert!(consumer.unconsumed_events().is_empty());
        assert_eq!(consumer.event_index(), 1);
    }

    #[test]
    fn flushable_stream_state_resolves_on_lock_release_or_close() {
        let mut state = FlushableStreamState::new();
        state.begin_write();
        state.poll_writable_lock();
        state.release_writable_lock();
        assert!(!state.is_done());
        state.finish_write();
        assert!(state.is_done());
        assert_eq!(state.pending_ops(), 0);

        let mut state = FlushableStreamState::new();
        state.close_stream();
        assert!(state.is_done());
        assert!(state.stream_ended());

        let mut state = FlushableStreamState::new();
        state.begin_write();
        state.poll_readable_lock();
        state.release_readable_lock();
        state.finish_write();
        assert!(state.is_done());
    }

    #[test]
    fn flushable_stream_state_propagates_errors_and_cancellation() {
        let mut state = FlushableStreamState::new();
        state.fail_stream("Writable stream closed prematurely");
        assert!(state.is_done());
        assert!(state.stream_ended());
        assert_eq!(
            state.rejection(),
            Some("Writable stream closed prematurely")
        );
        assert_eq!(
            state.cancel_reason(),
            Some("Writable stream closed prematurely")
        );
    }

    #[test]
    fn flushable_stream_state_tracks_concurrent_writes_and_single_pollers() {
        let mut state = FlushableStreamState::new();
        state.begin_write();
        state.begin_write();
        state.poll_writable_lock();
        state.poll_writable_lock();
        assert!(state.writable_polling_active());
        state.release_writable_lock();
        state.finish_write();
        assert!(!state.is_done());
        state.finish_write();
        assert!(state.is_done());

        let mut state = FlushableStreamState::new();
        state.poll_readable_lock();
        state.poll_readable_lock();
        assert!(state.readable_polling_active());
    }

    #[test]
    fn flushable_stream_state_handles_stream_end_while_ops_in_flight() {
        let mut state = FlushableStreamState::new();
        state.begin_write();
        state.poll_writable_lock();
        state.close_stream();
        assert!(state.is_done());
        assert!(state.stream_ended());
        assert!(!state.writable_polling_active());
    }

    #[test]
    fn replay_timeout_env_parsing_matches_upstream_bounds_and_warnings() {
        let mut warnings = ReplayTimeoutWarnings::new();
        assert_eq!(
            get_replay_timeout_ms(None, &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some(""), &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("abc"), &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("0"), &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("-1"), &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("1000"), &mut warnings),
            MIN_REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("999999"), &mut warnings),
            MAX_REPLAY_TIMEOUT_MS
        );
        assert_eq!(get_replay_timeout_ms(Some("60000"), &mut warnings), 60_000);
        assert_eq!(
            get_replay_timeout_ms(Some("30000"), &mut warnings),
            MIN_REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("780000"), &mut warnings),
            MAX_REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("Infinity"), &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("NaN"), &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        let before = warnings.warnings().len();
        let _ = get_replay_timeout_ms(Some("abc"), &mut warnings);
        assert_eq!(warnings.warnings().len(), before);
    }

    #[test]
    fn workflow_queue_name_validation_matches_upstream_safe_pattern() {
        for name in [
            "workflow",
            "abcXYZ123",
            "under_score-and-hyphen",
            "pkg.workflow",
            "folder/workflow",
            "@scope",
            "@scope/pkg.workflow",
        ] {
            assert_eq!(
                get_workflow_queue_name(name).expect("valid"),
                format!("__wkf_workflow_{name}")
            );
        }

        for name in ["has space", "bad!", ""] {
            assert!(get_workflow_queue_name(name).is_err());
        }
    }

    #[test]
    fn load_workflow_run_events_paginates_preserves_cursors_and_dedupes() {
        let mut world = InMemoryWorld::new();
        world.push_scripted_event_page(EventPage {
            data: vec![event("event-1", EventKind::RunCreated)],
            cursor: Some("cursor-1".to_string()),
            has_more: true,
        });
        world.push_scripted_event_page(EventPage {
            data: Vec::new(),
            cursor: None,
            has_more: false,
        });
        let loaded = load_workflow_run_events(&mut world, "wrun_test", None).expect("loaded");
        assert_eq!(loaded.events.len(), 1);
        assert_eq!(loaded.cursor.as_deref(), Some("cursor-1"));

        let mut world = InMemoryWorld::new();
        world.push_scripted_event_page(EventPage {
            data: vec![
                event("event-1", EventKind::RunCreated),
                event("event-2", EventKind::RunStarted),
            ],
            cursor: Some("cursor-2".to_string()),
            has_more: true,
        });
        world.push_scripted_event_page(EventPage {
            data: vec![
                event("event-2", EventKind::RunStarted),
                event("event-3", EventKind::StepCreated),
            ],
            cursor: Some("cursor-3".to_string()),
            has_more: false,
        });
        let loaded = load_workflow_run_events(&mut world, "wrun_test", None).expect("loaded");
        assert_eq!(
            loaded
                .events
                .iter()
                .map(|event| event.event_id.as_str())
                .collect::<Vec<_>>(),
            ["event-1", "event-2", "event-3"]
        );
        assert_eq!(loaded.cursor.as_deref(), Some("cursor-3"));
    }

    #[test]
    fn load_workflow_run_events_retries_bad_incremental_cursor_and_fails_bad_contracts() {
        let mut world = InMemoryWorld::new();
        world.insert_event("wrun_test", event("event-1", EventKind::RunCreated));
        world.rejected_cursors.insert("cursor_bad".to_string());
        let loaded =
            load_workflow_run_events(&mut world, "wrun_test", Some("cursor_bad".to_string()))
                .expect("retried full reload");
        assert_eq!(loaded.events.len(), 1);

        let mut world = InMemoryWorld::new();
        world.push_scripted_event_page(EventPage {
            data: vec![event("event-1", EventKind::RunCreated)],
            cursor: Some("same".to_string()),
            has_more: true,
        });
        world.push_scripted_event_page(EventPage {
            data: vec![event("event-2", EventKind::RunStarted)],
            cursor: Some("same".to_string()),
            has_more: true,
        });
        assert_eq!(
            load_workflow_run_events(&mut world, "wrun_test", None)
                .expect_err("repeat cursor")
                .kind,
            WorkflowErrorKind::WorldContract
        );

        let mut world = InMemoryWorld::new();
        world.push_scripted_event_page(EventPage {
            data: vec![event("event-1", EventKind::RunCreated)],
            cursor: None,
            has_more: true,
        });
        assert_eq!(
            load_workflow_run_events(&mut world, "wrun_test", None)
                .expect_err("missing cursor")
                .kind,
            WorkflowErrorKind::WorldContract
        );
    }

    #[test]
    fn replay_budget_accounts_for_non_step_time_and_pause_resume_cycles() {
        let mut budget = ReplayBudget::new(100, 0);
        assert_eq!(budget.elapsed(25), 25);
        budget.pause(25);
        assert_eq!(budget.elapsed(500), 25);
        budget.pause(600);
        assert_eq!(budget.elapsed(700), 25);
        budget.resume(700);
        budget.resume(750);
        assert_eq!(budget.elapsed(800), 125);
        assert!(budget.is_exhausted(800));

        let mut budget = ReplayBudget::new(REPLAY_TIMEOUT_MS, 0);
        budget.pause(0);
        assert!(!budget.is_exhausted(480_000));
    }

    #[test]
    fn replay_budget_exhaustion_uses_world_redelivery_capability() {
        let mut world = world_with_running_run("wrun_replay");
        let action = handle_replay_budget_exhausted(
            &mut world,
            "wrun_replay",
            "workflow",
            Some("req".to_string()),
            1,
            30_000,
        )
        .expect("handled");
        assert_eq!(action, ReplayTimeoutAction::Returned);
        assert_eq!(
            world.runs_get("wrun_replay").unwrap().status,
            RunStatus::Failed
        );

        let mut world = world_with_running_run("wrun_replay");
        world.process_exit_triggers_queue_redelivery = true;
        let action =
            handle_replay_budget_exhausted(&mut world, "wrun_replay", "workflow", None, 1, 30_000)
                .expect("handled");
        assert_eq!(action, ReplayTimeoutAction::ExitForRedelivery);
        assert_eq!(
            world.runs_get("wrun_replay").unwrap().status,
            RunStatus::Running
        );

        let action = handle_replay_budget_exhausted(
            &mut world,
            "wrun_replay",
            "workflow",
            None,
            REPLAY_TIMEOUT_MAX_RETRIES + 1,
            30_000,
        )
        .expect("handled");
        assert_eq!(action, ReplayTimeoutAction::ExitForRedelivery);
        assert_eq!(
            world.runs_get("wrun_replay").unwrap().status,
            RunStatus::Failed
        );
    }

    #[test]
    fn start_validates_workflow_and_resolves_spec_deployment_and_encryption_context() {
        let mut world = InMemoryWorld::new();
        assert!(start(&mut world, None, Vec::new(), StartOptions::default()).is_err());
        assert!(
            start(
                &mut world,
                Some(&runtime::WorkflowDefinition::new("")),
                Vec::new(),
                StartOptions::default(),
            )
            .is_err()
        );

        let mut world = InMemoryWorld::new();
        world.spec_version = None;
        let run = start(
            &mut world,
            Some(&runtime::WorkflowDefinition::new("test-workflow")),
            vec![json!(42)],
            StartOptions::default(),
        )
        .expect("started");
        assert!(run.run_id.starts_with("wrun_"));
        assert_eq!(
            world.last_event(&run.run_id).unwrap().event_type,
            EventKind::RunCreated
        );
        assert_eq!(
            world.last_event(&run.run_id).unwrap().spec_version,
            SPEC_VERSION_SUPPORTS_EVENT_SOURCING
        );

        let mut world = InMemoryWorld::new();
        let _ = start(
            &mut world,
            Some(&runtime::WorkflowDefinition::new("test-workflow")),
            Vec::new(),
            StartOptions {
                spec_version: Some(SPEC_VERSION_LEGACY),
                ..StartOptions::default()
            },
        )
        .expect("started");
        assert_eq!(world.last_event("wrun_000001").unwrap().spec_version, 1);

        let mut world = InMemoryWorld::new();
        let _ = start(
            &mut world,
            Some(&runtime::WorkflowDefinition::new("test-workflow")),
            Vec::new(),
            StartOptions {
                deployment_id: Some(DeploymentId::Latest),
                ..StartOptions::default()
            },
        )
        .expect("started latest");
        assert_eq!(world.resolve_latest_calls, 1);
        assert_eq!(
            world.encryption_contexts[0].1.deployment_id.as_deref(),
            Some("deploy_latest")
        );

        let mut world = InMemoryWorld::new();
        let _ = start(
            &mut world,
            Some(&runtime::WorkflowDefinition::new("test-workflow")),
            Vec::new(),
            StartOptions {
                deployment_id: Some(DeploymentId::Id("deploy_explicit".to_string())),
                ..StartOptions::default()
            },
        )
        .expect("started explicit");
        assert_eq!(world.resolve_latest_calls, 0);
        assert_eq!(
            world.encryption_contexts[0].1.deployment_id.as_deref(),
            Some("deploy_explicit")
        );
    }

    #[test]
    fn start_resilient_start_and_failure_paths_match_upstream() {
        let mut world = InMemoryWorld::new();
        world.spec_version = Some(SPEC_VERSION_SUPPORTS_CBOR_QUEUE_TRANSPORT);
        world.fail_next_event_create(
            Some(EventKind::RunCreated),
            runtime::WorkflowCoreError::world("Internal Server Error", Some(500)),
        );
        let run = start(
            &mut world,
            Some(&runtime::WorkflowDefinition::new("test-workflow")),
            vec![json!(42)],
            StartOptions::default(),
        )
        .expect("resilient start");
        assert!(run.resilient_start);
        assert!(world.queues[0].message.run_input.is_some());

        let mut world = InMemoryWorld::new();
        world.fail_next_queue(runtime::WorkflowCoreError::queue("Queue unavailable"));
        let error = start(
            &mut world,
            Some(&runtime::WorkflowDefinition::new("test-workflow")),
            Vec::new(),
            StartOptions::default(),
        )
        .expect_err("queue failure");
        assert_eq!(error.kind, WorkflowErrorKind::Queue);

        let mut world = InMemoryWorld::new();
        world.fail_next_event_create(
            Some(EventKind::RunCreated),
            runtime::WorkflowCoreError::world("Bad Request", Some(400)),
        );
        let error = start(
            &mut world,
            Some(&runtime::WorkflowDefinition::new("test-workflow")),
            Vec::new(),
            StartOptions::default(),
        )
        .expect_err("non-retryable event failure");
        assert_eq!(error.status, Some(400));
    }

    #[test]
    fn run_handle_exists_wakeup_serialization_and_return_value_errors() {
        let mut world = world_with_running_run("wrun_run");
        let handle = RunHandle::new("wrun_run");
        assert!(handle.exists(&world).expect("exists"));
        assert!(!RunHandle::new("missing").exists(&world).expect("missing"));
        world.runs_get_error = Some(runtime::WorkflowCoreError::world("backend down", Some(503)));
        assert_eq!(
            handle.exists(&world).expect_err("rethrows non-404").status,
            Some(503)
        );
        world.runs_get_error = None;

        world
            .events_create(
                "wrun_run",
                CreateEventRequest::new(EventKind::WaitCreated)
                    .with_correlation_id("wait-a")
                    .with_data(json!({ "resumeAt": 10 })),
                EventCreateOptions::default(),
            )
            .expect("wait");
        world.fail_next_event_create(
            Some(EventKind::WaitCompleted),
            runtime::WorkflowCoreError::conflict("already completed"),
        );
        assert_eq!(
            handle
                .wake_up(&mut world, StopSleepOptions::default())
                .expect("wake")
                .stopped_count,
            1
        );

        let serialized = handle.serialize();
        assert_eq!(
            RunHandle::deserialize(&serialized).expect("deserialize"),
            handle
        );

        let mut completed = running_run("wrun_done");
        completed.status = RunStatus::Completed;
        completed.output = Some(json!({"ok": true}));
        world.insert_run(completed);
        assert_eq!(
            RunHandle::new("wrun_done")
                .return_value(&world)
                .expect("value"),
            json!({"ok": true})
        );

        let mut failed = running_run("wrun_failed");
        failed.status = RunStatus::Failed;
        failed.error_code = Some(RunErrorCode::WorkflowRuntimeError);
        failed.error = Some(json!({
            "message": "outer failure",
            "cause": "inner cause",
        }));
        world.insert_run(failed);
        let error = RunHandle::new("wrun_failed")
            .return_value(&world)
            .expect_err("failed");
        assert_eq!(error.kind, WorkflowErrorKind::Fatal);
        assert_eq!(error.cause.as_deref(), Some("inner cause"));

        let mut failed = running_run("wrun_fallback");
        failed.status = RunStatus::Failed;
        failed.error = Some(json!({ "opaque": true }));
        world.insert_run(failed);
        assert!(
            RunHandle::new("wrun_fallback")
                .return_value(&world)
                .expect_err("fallback")
                .message
                .contains("Failed to hydrate workflow run error")
        );
    }

    #[test]
    fn run_wakeup_targets_pending_waits_and_queues_continuation() {
        let mut world = world_with_running_run("wrun_sleep");
        for wait_id in ["wait-a", "wait-b"] {
            world
                .events_create(
                    "wrun_sleep",
                    CreateEventRequest::new(EventKind::WaitCreated)
                        .with_correlation_id(wait_id)
                        .with_data(json!({ "resumeAt": 10 })),
                    EventCreateOptions::default(),
                )
                .expect("wait");
        }
        let result = wake_up_run(
            &mut world,
            "wrun_sleep",
            StopSleepOptions {
                correlation_ids: Some(vec!["wait-b".to_string()]),
            },
        )
        .expect("wake targeted");
        assert_eq!(result.stopped_count, 1);
        assert_eq!(world.queues.len(), 1);

        let result =
            wake_up_run(&mut world, "wrun_sleep", StopSleepOptions::default()).expect("wake all");
        assert_eq!(result.stopped_count, 1);

        let result =
            wake_up_run(&mut world, "wrun_sleep", StopSleepOptions::default()).expect("wake none");
        assert_eq!(result.stopped_count, 0);

        let mut world = world_with_running_run("wrun_sleep_error");
        world
            .events_create(
                "wrun_sleep_error",
                CreateEventRequest::new(EventKind::WaitCreated)
                    .with_correlation_id("wait-error")
                    .with_data(json!({ "resumeAt": 10 })),
                EventCreateOptions::default(),
            )
            .expect("wait");
        world.fail_next_event_create(
            Some(EventKind::WaitCompleted),
            runtime::WorkflowCoreError::world("backend failed", Some(500)),
        );
        assert_eq!(
            wake_up_run(&mut world, "wrun_sleep_error", StopSleepOptions::default())
                .expect_err("non-conflict wake error")
                .status,
            Some(500)
        );
    }

    #[test]
    fn step_executor_handles_conflicts_and_request_id_propagation() {
        let mut world = world_with_running_run("wrun_step");
        create_step(&mut world, "wrun_step", "step-1", "step");
        let mut registry = StepRegistry::new();
        registry.register("step", StepFunction::complete(json!("ok")));

        let result = execute_step(
            &mut world,
            &registry,
            step_params("wrun_step", "step-1", "step"),
        )
        .expect("step");
        assert_eq!(
            result,
            StepExecutionResult::Completed {
                has_pending_ops: false
            }
        );
        let request_ids = world
            .events
            .get("wrun_step")
            .unwrap()
            .iter()
            .filter(|event| {
                matches!(
                    event.event_type,
                    EventKind::StepStarted | EventKind::StepCompleted
                )
            })
            .map(|event| event.request_id.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(request_ids, [Some("req_test"), Some("req_test")]);

        create_step(&mut world, "wrun_step", "step-2", "step");
        world.fail_next_event_create(
            Some(EventKind::StepCompleted),
            runtime::WorkflowCoreError::conflict("already complete"),
        );
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "step-2", "step"),
            )
            .expect("conflict"),
            StepExecutionResult::Skipped
        );
    }

    #[test]
    fn step_executor_handles_missing_fatal_abort_and_retryable_failures() {
        let mut world = world_with_running_run("wrun_step");
        let registry = StepRegistry::new();
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "missing", "missing"),
            )
            .expect("missing"),
            StepExecutionResult::Failed
        );

        let mut registry = StepRegistry::new();
        registry.register_non_function("not-a-function");
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "not-fn", "not-a-function"),
            )
            .expect("not function"),
            StepExecutionResult::Failed
        );

        create_step(&mut world, "wrun_step", "fatal", "fatal");
        let mut registry = StepRegistry::new();
        registry.register(
            "fatal",
            StepFunction {
                max_retries: 3,
                behavior: StepBehavior::Fatal("fatal boom".to_string()),
            },
        );
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "fatal", "fatal"),
            )
            .expect("fatal"),
            StepExecutionResult::Failed
        );

        create_step(&mut world, "wrun_step", "retry", "retry");
        let mut registry = StepRegistry::new();
        registry.register(
            "retry",
            StepFunction {
                max_retries: 1,
                behavior: StepBehavior::Retryable {
                    message: "try again".to_string(),
                    retry_after_seconds: 2,
                },
            },
        );
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "retry", "retry"),
            )
            .expect("retry"),
            StepExecutionResult::Retry { timeout_seconds: 2 }
        );
        world.advance_ms(2_000);
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "retry", "retry"),
            )
            .expect("retry exhausted"),
            StepExecutionResult::Failed
        );

        create_step(&mut world, "wrun_step", "abort", "abort");
        let mut registry = StepRegistry::new();
        registry.register(
            "abort",
            StepFunction {
                max_retries: 3,
                behavior: StepBehavior::Abort("AbortError".to_string()),
            },
        );
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "abort", "abort"),
            )
            .expect("abort"),
            StepExecutionResult::Failed
        );
    }

    #[test]
    fn step_handler_max_deliveries_and_under_limit_paths() {
        let mut world = world_with_running_run("wrun_step");
        let registry = StepRegistry::new();
        handle_step_message(
            &mut world,
            &registry,
            StepHandlerMessage {
                workflow_run_id: "wrun_step".to_string(),
                workflow_name: "workflow".to_string(),
                workflow_started_at_ms: 0,
                step_id: "step-max".to_string(),
                step_name: "step".to_string(),
                attempt: MAX_QUEUE_DELIVERIES + 1,
                request_id: Some("req-max".to_string()),
            },
        )
        .expect("max deliveries");
        assert_eq!(world.queues.len(), 1);

        let mut world = world_with_running_run("wrun_step");
        world.fail_next_event_create(
            Some(EventKind::StepFailed),
            runtime::WorkflowCoreError::conflict("already failed"),
        );
        handle_step_message(
            &mut world,
            &registry,
            StepHandlerMessage {
                workflow_run_id: "wrun_step".to_string(),
                workflow_name: "workflow".to_string(),
                workflow_started_at_ms: 0,
                step_id: "step-max".to_string(),
                step_name: "step".to_string(),
                attempt: MAX_QUEUE_DELIVERIES + 1,
                request_id: None,
            },
        )
        .expect("conflict consumed");
        assert!(world.queues.is_empty());

        let mut world = world_with_running_run("wrun_step");
        create_step(&mut world, "wrun_step", "step-ok", "step");
        let mut registry = StepRegistry::new();
        registry.register("step", StepFunction::complete(json!("ok")));
        assert!(matches!(
            handle_step_message(
                &mut world,
                &registry,
                StepHandlerMessage {
                    workflow_run_id: "wrun_step".to_string(),
                    workflow_name: "workflow".to_string(),
                    workflow_started_at_ms: 0,
                    step_id: "step-ok".to_string(),
                    step_name: "step".to_string(),
                    attempt: MAX_QUEUE_DELIVERIES,
                    request_id: None,
                },
            )
            .expect("under limit"),
            Some(StepExecutionResult::Completed { .. })
        ));
    }

    #[test]
    fn suspension_handler_orders_hooks_steps_waits_and_detects_conflicts() {
        let mut world = world_with_running_run("wrun_suspend");
        let run = world.runs_get("wrun_suspend").unwrap();
        let suspension = WorkflowSuspension {
            items: vec![
                SuspensionItem::Hook {
                    correlation_id: "hook-a".to_string(),
                    token: "token-a".to_string(),
                    metadata: json!({"kind": "hook"}),
                    has_created_event: false,
                    disposed: false,
                    abort_requested: false,
                    abort_reason: None,
                    is_webhook: false,
                    is_system: false,
                },
                SuspensionItem::Step {
                    correlation_id: "step-a".to_string(),
                    step_name: "step".to_string(),
                    input: json!([1]),
                    has_created_event: false,
                },
                SuspensionItem::Wait {
                    correlation_id: "wait-a".to_string(),
                    resume_at_ms: 5_000,
                    has_created_event: false,
                },
            ],
        };
        let result = handle_suspension(&mut world, &run, &suspension, Some("req".to_string()))
            .expect("suspension");
        assert_eq!(result.pending_steps.len(), 1);
        assert!(result.created_step_correlation_ids.contains("step-a"));
        assert_eq!(result.timeout_seconds, Some(5));
        assert_eq!(
            world.event_kinds("wrun_suspend"),
            [
                EventKind::HookCreated,
                EventKind::StepCreated,
                EventKind::WaitCreated
            ]
        );

        let mut world = world_with_running_run("wrun_conflict");
        world
            .active_hook_tokens
            .insert("token-a".to_string(), "other-run".to_string());
        let run = world.runs_get("wrun_conflict").unwrap();
        let result = handle_suspension(
            &mut world,
            &run,
            &WorkflowSuspension {
                items: vec![SuspensionItem::Hook {
                    correlation_id: "hook-a".to_string(),
                    token: "token-a".to_string(),
                    metadata: Value::Null,
                    has_created_event: false,
                    disposed: false,
                    abort_requested: false,
                    abort_reason: None,
                    is_webhook: false,
                    is_system: false,
                }],
            },
            None,
        )
        .expect("hook conflict");
        assert!(result.has_hook_conflict);
        assert_eq!(result.timeout_seconds, Some(0));
        assert_eq!(
            world.event_kinds("wrun_conflict"),
            [EventKind::HookConflict]
        );
    }

    #[test]
    fn hook_sleep_interaction_preserves_waiters_steps_and_payload_ordering() {
        let mut world = world_with_running_run("wrun_hook_sleep");
        let run = world.runs_get("wrun_hook_sleep").unwrap();
        let suspension = WorkflowSuspension {
            items: vec![
                SuspensionItem::Wait {
                    correlation_id: "wait-early".to_string(),
                    resume_at_ms: 1_000,
                    has_created_event: false,
                },
                SuspensionItem::Step {
                    correlation_id: "step-fire-forget".to_string(),
                    step_name: "payload-step".to_string(),
                    input: json!({"payload": "A"}),
                    has_created_event: false,
                },
                SuspensionItem::Hook {
                    correlation_id: "hook-stream".to_string(),
                    token: "token-stream".to_string(),
                    metadata: Value::Null,
                    has_created_event: false,
                    disposed: false,
                    abort_requested: true,
                    abort_reason: Some("timeout".to_string()),
                    is_webhook: false,
                    is_system: true,
                },
            ],
        };
        let result = handle_suspension(&mut world, &run, &suspension, None).expect("suspend");
        assert_eq!(result.timeout_seconds, Some(1));
        assert_eq!(result.pending_steps.len(), 1);
        assert_eq!(
            world.event_kinds("wrun_hook_sleep"),
            [
                EventKind::HookCreated,
                EventKind::HookReceived,
                EventKind::WaitCreated,
                EventKind::StepCreated,
            ]
        );

        let mut consumer = EventsConsumer::new(vec![
            event_with_data(
                "event-a",
                EventKind::HookReceived,
                "hook-stream",
                json!({"token": "token-stream", "payload": "A"}),
            ),
            event_with_data(
                "event-b",
                EventKind::HookReceived,
                "hook-stream",
                json!({"token": "token-stream", "payload": "B"}),
            ),
        ]);
        consumer.subscribe(|event| {
            if let Some(event) = event {
                assert_eq!(
                    event.event_data.get("token").and_then(Value::as_str),
                    Some("token-stream")
                );
                EventConsumerResult::Consumed
            } else {
                EventConsumerResult::NotConsumed
            }
        });
        consumer.flush_deferred_unconsumed_check();
        assert!(consumer.unconsumed_events().is_empty());
    }

    #[test]
    fn wait_completion_replay_uses_incremental_cursor_and_falls_back_when_needed() {
        let base_events = vec![event_with_data(
            "event-1",
            EventKind::WaitCreated,
            "wait-a",
            json!({ "resumeAt": 10 }),
        )];

        let mut world = world_with_running_run("wrun_wait");
        world.push_scripted_event_page(EventPage {
            data: vec![event_with_data(
                "event-2",
                EventKind::WaitCompleted,
                "wait-a",
                json!({ "resumeAt": 10 }),
            )],
            cursor: Some("cursor-2".to_string()),
            has_more: false,
        });
        let refreshed = complete_elapsed_waits_and_refresh(
            &mut world,
            "wrun_wait",
            base_events.clone(),
            Some("cursor-1".to_string()),
            20,
            None,
        )
        .expect("refresh");
        assert!(refreshed.used_incremental);
        assert!(!refreshed.used_full_reload);
        assert_eq!(refreshed.events.len(), 2);

        let mut world = world_with_running_run("wrun_wait");
        world.insert_event("wrun_wait", base_events[0].clone());
        let refreshed = complete_elapsed_waits_and_refresh(
            &mut world,
            "wrun_wait",
            base_events.clone(),
            None,
            20,
            None,
        )
        .expect("full reload without cursor");
        assert!(refreshed.used_full_reload);

        let mut world = world_with_running_run("wrun_wait");
        world.insert_event("wrun_wait", base_events[0].clone());
        world.push_scripted_event_page(EventPage {
            data: vec![event("event-x", EventKind::RunStarted)],
            cursor: Some("cursor-x".to_string()),
            has_more: false,
        });
        let refreshed = complete_elapsed_waits_and_refresh(
            &mut world,
            "wrun_wait",
            base_events,
            Some("cursor-1".to_string()),
            20,
            None,
        )
        .expect("fallback when delta misses completion");
        assert!(refreshed.used_full_reload);
    }

    #[test]
    fn workflow_entrypoint_guards_record_world_contract_and_corrupted_logs() {
        let mut world = world_with_running_run("wrun_guard");
        record_run_failure(
            &mut world,
            "wrun_guard",
            Some("req".to_string()),
            RunErrorCode::WorldContractError,
            "Schema validation failed",
        )
        .expect("recorded");
        assert_eq!(
            world.runs_get("wrun_guard").unwrap().status,
            RunStatus::Failed
        );
        assert_eq!(
            world.runs_get("wrun_guard").unwrap().error_code,
            Some(RunErrorCode::WorldContractError)
        );

        let bad_waits = vec![
            event_with_data(
                "event-1",
                EventKind::WaitCreated,
                "wait-a",
                json!({ "resumeAt": 5 }),
            ),
            event_with_data(
                "event-2",
                EventKind::WaitCompleted,
                "wait-a",
                json!({ "resumeAt": 6 }),
            ),
        ];
        assert_eq!(
            validate_replay_event_log(&bad_waits)
                .expect_err("bad wait")
                .code,
            Some(RunErrorCode::CorruptedEventLog)
        );

        let bad_hook = vec![event_with_data(
            "event-1",
            EventKind::HookReceived,
            "hook-a",
            json!({ "token": "wrong-token" }),
        )];
        assert_eq!(
            validate_replay_event_log(&bad_hook)
                .expect_err("bad hook")
                .code,
            Some(RunErrorCode::CorruptedEventLog)
        );
    }

    #[test]
    fn world_init_registry_registers_lazy_world_and_preserves_prior_registration() {
        let mut registry = WorldRegistry::new();
        registry.register_get_world("host-world");
        assert_eq!(registry.get_world_lazy(), "host-world");
        assert_eq!(registry.fallback_imports(), 0);
        registry.register_get_world("replacement");
        assert_eq!(registry.get_world_lazy(), "host-world");

        let mut registry = WorldRegistry::new();
        assert_eq!(registry.get_world_lazy(), "dynamic-world");
        assert_eq!(registry.fallback_imports(), 1);
    }
}
