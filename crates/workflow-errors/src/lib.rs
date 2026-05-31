//! Error crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/errors`.

#![forbid(unsafe_code)]

pub mod ansi;
mod codes;

use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

pub use codes::{RUN_ERROR_CODES, RunErrorCode};

const BASE_URL: &str = "https://workflow-sdk.dev/err";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorSlug {
    NodeJsModuleInWorkflow,
    StartInvalidWorkflowFunction,
    SerializationFailed,
    WebhookInvalidRespondWithValue,
    WebhookResponseNotSent,
    FetchInWorkflowFunction,
    TimeoutFunctionsInWorkflow,
    HookConflict,
    CorruptedEventLog,
    StepNotRegistered,
    WorkflowNotRegistered,
    RuntimeDecryptionFailed,
}

impl ErrorSlug {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NodeJsModuleInWorkflow => "node-js-module-in-workflow",
            Self::StartInvalidWorkflowFunction => "start-invalid-workflow-function",
            Self::SerializationFailed => "serialization-failed",
            Self::WebhookInvalidRespondWithValue => "webhook-invalid-respond-with-value",
            Self::WebhookResponseNotSent => "webhook-response-not-sent",
            Self::FetchInWorkflowFunction => "fetch-in-workflow",
            Self::TimeoutFunctionsInWorkflow => "timeout-in-workflow",
            Self::HookConflict => "hook-conflict",
            Self::CorruptedEventLog => "corrupted-event-log",
            Self::StepNotRegistered => "step-not-registered",
            Self::WorkflowNotRegistered => "workflow-not-registered",
            Self::RuntimeDecryptionFailed => "runtime-decryption-failed",
        }
    }

    pub fn docs_url(self) -> String {
        format!("{BASE_URL}/{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowErrorFamily {
    Workflow,
    Runtime,
    Build,
    Serialization,
    Fatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowErrorKind {
    Fatal,
    Workflow,
    WorkflowRuntime,
    StepNotRegistered,
    WorkflowNotRegistered,
    CorruptedEventLog,
    RuntimeDecryption,
    Serialization,
    WorkflowWorld,
    HookConflict,
    Native,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowError {
    name: String,
    message: String,
    cause: Option<String>,
    family: WorkflowErrorFamily,
    kind: WorkflowErrorKind,
    code: Option<String>,
    status: Option<u16>,
    cause_name: Option<String>,
    stack: Option<String>,
    fatal: bool,
}

impl WorkflowError {
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_options(
            "WorkflowError",
            message,
            WorkflowErrorFamily::Workflow,
            None,
            None,
            false,
        )
    }

    fn with_options(
        name: impl Into<String>,
        message: impl Into<String>,
        family: WorkflowErrorFamily,
        slug: Option<ErrorSlug>,
        cause: Option<String>,
        fatal: bool,
    ) -> Self {
        let message = append_framed_details(message.into(), build_framed_details(None, slug));
        let name = name.into();
        let kind = workflow_error_kind_for_name(&name, family);
        Self {
            name,
            message,
            cause,
            family,
            kind,
            code: None,
            status: None,
            cause_name: None,
            stack: None,
            fatal,
        }
    }

    pub fn fatal(message: impl Into<String>) -> Self {
        Self::with_options(
            "FatalError",
            message,
            WorkflowErrorFamily::Fatal,
            None,
            None,
            true,
        )
    }

    pub fn workflow_runtime(message: impl Into<String>) -> Self {
        Self::with_options(
            "WorkflowRuntimeError",
            message,
            WorkflowErrorFamily::Runtime,
            None,
            None,
            false,
        )
    }

    pub fn step_not_registered(step_name: impl AsRef<str>) -> Self {
        Self::with_options(
            "StepNotRegisteredError",
            format!("Step not registered: {}", step_name.as_ref()),
            WorkflowErrorFamily::Runtime,
            Some(ErrorSlug::StepNotRegistered),
            None,
            false,
        )
    }

    pub fn workflow_not_registered(workflow_name: impl AsRef<str>) -> Self {
        Self::with_options(
            "WorkflowNotRegisteredError",
            format!("Workflow not registered: {}", workflow_name.as_ref()),
            WorkflowErrorFamily::Runtime,
            Some(ErrorSlug::WorkflowNotRegistered),
            None,
            false,
        )
    }

    pub fn corrupted_event_log(message: impl Into<String>) -> Self {
        Self::with_options(
            "CorruptedEventLogError",
            message,
            WorkflowErrorFamily::Runtime,
            Some(ErrorSlug::CorruptedEventLog),
            None,
            false,
        )
    }

    pub fn runtime_decryption(message: impl Into<String>) -> Self {
        Self::with_options(
            "RuntimeDecryptionError",
            message,
            WorkflowErrorFamily::Runtime,
            Some(ErrorSlug::RuntimeDecryptionFailed),
            None,
            false,
        )
    }

    pub fn serialization(message: impl Into<String>) -> Self {
        Self::with_options(
            "SerializationError",
            message,
            WorkflowErrorFamily::Serialization,
            Some(ErrorSlug::SerializationFailed),
            None,
            true,
        )
    }

    pub fn hook_conflict(token: impl AsRef<str>) -> Self {
        Self::with_options(
            "HookConflictError",
            format!("Hook token already exists: {}", token.as_ref()),
            WorkflowErrorFamily::Runtime,
            Some(ErrorSlug::HookConflict),
            None,
            false,
        )
    }

    pub fn workflow_world(message: impl Into<String>) -> Self {
        let mut error = Self::with_options(
            "WorkflowWorldError",
            message,
            WorkflowErrorFamily::Runtime,
            None,
            None,
            false,
        );
        error.kind = WorkflowErrorKind::WorkflowWorld;
        error
    }

    pub fn native(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message: message.into(),
            cause: None,
            family: WorkflowErrorFamily::Workflow,
            kind: WorkflowErrorKind::Native,
            code: None,
            status: None,
            cause_name: None,
            stack: None,
            fatal: false,
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_cause_name(mut self, cause_name: impl Into<String>) -> Self {
        self.cause_name = Some(cause_name.into());
        self
    }

    pub fn with_stack(mut self, stack: impl Into<String>) -> Self {
        self.stack = Some(stack.into());
        self
    }

    pub fn with_fatal(mut self, fatal: bool) -> Self {
        self.fatal = fatal;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn cause_message(&self) -> Option<&str> {
        self.cause.as_deref()
    }

    pub fn family(&self) -> WorkflowErrorFamily {
        self.family
    }

    pub fn kind(&self) -> WorkflowErrorKind {
        self.kind
    }

    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    pub fn status(&self) -> Option<u16> {
        self.status
    }

    pub fn cause_name(&self) -> Option<&str> {
        self.cause_name.as_deref()
    }

    pub fn stack(&self) -> Option<&str> {
        self.stack.as_deref()
    }

    pub fn is_fatal(&self) -> bool {
        self.fatal
    }

    pub fn is_runtime_error_family(&self) -> bool {
        matches!(
            self.kind,
            WorkflowErrorKind::WorkflowRuntime
                | WorkflowErrorKind::StepNotRegistered
                | WorkflowErrorKind::WorkflowNotRegistered
                | WorkflowErrorKind::RuntimeDecryption
        )
    }

    pub fn is(value: &Self) -> bool {
        value.name == "WorkflowError"
    }
}

fn workflow_error_kind_for_name(name: &str, family: WorkflowErrorFamily) -> WorkflowErrorKind {
    match name {
        "FatalError" => WorkflowErrorKind::Fatal,
        "WorkflowRuntimeError" => WorkflowErrorKind::WorkflowRuntime,
        "StepNotRegisteredError" => WorkflowErrorKind::StepNotRegistered,
        "WorkflowNotRegisteredError" => WorkflowErrorKind::WorkflowNotRegistered,
        "CorruptedEventLogError" => WorkflowErrorKind::CorruptedEventLog,
        "RuntimeDecryptionError" => WorkflowErrorKind::RuntimeDecryption,
        "SerializationError" => WorkflowErrorKind::Serialization,
        "WorkflowWorldError" => WorkflowErrorKind::WorkflowWorld,
        "HookConflictError" => WorkflowErrorKind::HookConflict,
        "Error" | "TypeError" | "SyntaxError" | "ReferenceError" => WorkflowErrorKind::Native,
        _ if family == WorkflowErrorFamily::Runtime => WorkflowErrorKind::WorkflowRuntime,
        _ if family == WorkflowErrorFamily::Fatal => WorkflowErrorKind::Fatal,
        _ if family == WorkflowErrorFamily::Serialization => WorkflowErrorKind::Serialization,
        _ => WorkflowErrorKind::Workflow,
    }
}

impl fmt::Display for WorkflowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for WorkflowError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRuntimeError {
    inner: WorkflowError,
}

impl WorkflowRuntimeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_slug_and_cause(message, None, None)
    }

    fn with_slug_and_cause(
        message: impl Into<String>,
        slug: Option<ErrorSlug>,
        cause: Option<String>,
    ) -> Self {
        Self {
            inner: WorkflowError::with_options(
                "WorkflowRuntimeError",
                message,
                WorkflowErrorFamily::Runtime,
                slug,
                cause,
                false,
            ),
        }
    }

    pub fn name(&self) -> &str {
        self.inner.name()
    }

    pub fn message(&self) -> &str {
        self.inner.message()
    }

    pub fn cause_message(&self) -> Option<&str> {
        self.inner.cause_message()
    }

    pub fn as_workflow_error(&self) -> &WorkflowError {
        &self.inner
    }

    pub fn is(value: &WorkflowError) -> bool {
        value.name() == "WorkflowRuntimeError"
    }
}

impl fmt::Display for WorkflowRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl Error for WorkflowRuntimeError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBuildError {
    inner: WorkflowError,
    hint: Option<String>,
}

impl WorkflowBuildError {
    pub fn new(message: impl Into<String>, options: WorkflowBuildErrorOptions) -> Self {
        let hint = options.hint;
        let body =
            append_framed_details(message.into(), build_framed_details(hint.as_deref(), None));
        Self {
            inner: WorkflowError::with_options(
                "WorkflowBuildError",
                body,
                WorkflowErrorFamily::Build,
                None,
                options.cause,
                false,
            ),
            hint,
        }
    }

    pub fn name(&self) -> &str {
        self.inner.name()
    }

    pub fn message(&self) -> &str {
        self.inner.message()
    }

    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    pub fn cause_message(&self) -> Option<&str> {
        self.inner.cause_message()
    }

    pub fn as_workflow_error(&self) -> &WorkflowError {
        &self.inner
    }

    pub fn is(value: &WorkflowError) -> bool {
        value.name() == "WorkflowBuildError"
    }
}

impl fmt::Display for WorkflowBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl Error for WorkflowBuildError {}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBuildErrorOptions {
    pub hint: Option<String>,
    pub cause: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializationError {
    inner: WorkflowError,
    hint: Option<String>,
}

impl SerializationError {
    pub fn new(message: impl Into<String>, options: SerializationErrorOptions) -> Self {
        let hint = options.hint;
        let body =
            append_framed_details(message.into(), build_framed_details(hint.as_deref(), None));
        Self {
            inner: WorkflowError::with_options(
                "SerializationError",
                body,
                WorkflowErrorFamily::Serialization,
                None,
                options.cause,
                true,
            ),
            hint,
        }
    }

    pub fn name(&self) -> &str {
        self.inner.name()
    }

    pub fn message(&self) -> &str {
        self.inner.message()
    }

    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    pub fn cause_message(&self) -> Option<&str> {
        self.inner.cause_message()
    }

    pub fn is_fatal(&self) -> bool {
        self.inner.is_fatal()
    }

    pub fn as_workflow_error(&self) -> &WorkflowError {
        &self.inner
    }

    pub fn is(value: &WorkflowError) -> bool {
        value.name() == "SerializationError"
    }
}

impl fmt::Display for SerializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl Error for SerializationError {}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializationErrorOptions {
    pub hint: Option<String>,
    pub cause: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CryptoOperation {
    Encrypt,
    Decrypt,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeDecryptionErrorContext {
    pub operation: Option<CryptoOperation>,
    pub byte_length: Option<usize>,
    pub format_prefix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeDecryptionError {
    inner: WorkflowError,
    context: Option<RuntimeDecryptionErrorContext>,
}

impl RuntimeDecryptionError {
    pub fn new(message: impl Into<String>, options: RuntimeDecryptionErrorOptions) -> Self {
        let mut inner = WorkflowRuntimeError::with_slug_and_cause(
            message,
            Some(ErrorSlug::RuntimeDecryptionFailed),
            options.cause,
        )
        .inner;
        inner.name = "RuntimeDecryptionError".to_owned();
        inner.kind = WorkflowErrorKind::RuntimeDecryption;
        Self {
            inner,
            context: options.context,
        }
    }

    pub fn name(&self) -> &str {
        self.inner.name()
    }

    pub fn message(&self) -> &str {
        self.inner.message()
    }

    pub fn cause_message(&self) -> Option<&str> {
        self.inner.cause_message()
    }

    pub fn context(&self) -> Option<&RuntimeDecryptionErrorContext> {
        self.context.as_ref()
    }

    pub fn as_workflow_error(&self) -> &WorkflowError {
        &self.inner
    }

    pub fn is(value: &WorkflowError) -> bool {
        value.name() == "RuntimeDecryptionError"
    }
}

impl fmt::Display for RuntimeDecryptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl Error for RuntimeDecryptionError {}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeDecryptionErrorOptions {
    pub cause: Option<String>,
    pub context: Option<RuntimeDecryptionErrorContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorruptedEventLogError {
    inner: WorkflowError,
}

impl CorruptedEventLogError {
    pub fn new(message: impl Into<String>, cause: Option<String>) -> Self {
        let mut inner = WorkflowRuntimeError::with_slug_and_cause(
            message,
            Some(ErrorSlug::CorruptedEventLog),
            cause,
        )
        .inner;
        inner.name = "CorruptedEventLogError".to_owned();
        inner.kind = WorkflowErrorKind::CorruptedEventLog;
        Self { inner }
    }

    pub fn name(&self) -> &str {
        self.inner.name()
    }

    pub fn message(&self) -> &str {
        self.inner.message()
    }

    pub fn cause_message(&self) -> Option<&str> {
        self.inner.cause_message()
    }

    pub fn as_workflow_error(&self) -> &WorkflowError {
        &self.inner
    }

    pub fn is(value: &WorkflowError) -> bool {
        value.name() == "CorruptedEventLogError"
    }
}

impl fmt::Display for CorruptedEventLogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl Error for CorruptedEventLogError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FatalError {
    message: String,
}

impl FatalError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn name(&self) -> &str {
        "FatalError"
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn is(value: Option<&dyn FatalErrorCheck>) -> bool {
        value.is_some_and(|error| {
            error.workflow_error_name() == "FatalError" || error.fatal_marker() == Some(true)
        })
    }
}

impl fmt::Display for FatalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for FatalError {}

pub trait FatalErrorCheck {
    fn workflow_error_name(&self) -> &str;

    fn fatal_marker(&self) -> Option<bool> {
        None
    }
}

impl FatalErrorCheck for FatalError {
    fn workflow_error_name(&self) -> &str {
        self.name()
    }

    fn fatal_marker(&self) -> Option<bool> {
        Some(true)
    }
}

impl FatalErrorCheck for WorkflowError {
    fn workflow_error_name(&self) -> &str {
        self.name()
    }

    fn fatal_marker(&self) -> Option<bool> {
        self.is_fatal().then_some(true)
    }
}

impl FatalErrorCheck for SerializationError {
    fn workflow_error_name(&self) -> &str {
        self.name()
    }

    fn fatal_marker(&self) -> Option<bool> {
        Some(self.is_fatal())
    }
}

#[derive(Debug, Clone)]
struct FramedDetail {
    label: &'static str,
    value: String,
}

fn build_framed_details(hint: Option<&str>, slug: Option<ErrorSlug>) -> Vec<FramedDetail> {
    let mut details = Vec::new();
    if let Some(hint) = hint {
        details.push(FramedDetail {
            label: "hint",
            value: hint.to_owned(),
        });
    }
    if let Some(slug) = slug {
        details.push(FramedDetail {
            label: "docs",
            value: slug.docs_url(),
        });
    }
    details
}

fn append_framed_details(title: String, details: Vec<FramedDetail>) -> String {
    if details.is_empty() {
        return title;
    }

    let mut lines = vec![title];
    for (index, detail) in details.iter().enumerate() {
        let is_last = index == details.len() - 1;
        let head = if is_last { "╰▶ " } else { "├▶ " };
        let continuation = if is_last { "   " } else { "│  " };
        let text = format!("{}: {}", detail.label, detail.value);
        for (line_index, line) in text.lines().enumerate() {
            lines.push(format!(
                "{}{}",
                if line_index == 0 { head } else { continuation },
                line
            ));
        }
    }
    lines.join("\n")
}

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the current port.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/errors";

/// Upstream package version inventoried for this port.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.6";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_build_error_cases() {
        let err = WorkflowBuildError::new("boom", WorkflowBuildErrorOptions::default());
        assert_eq!(err.name(), "WorkflowBuildError");
        assert_eq!(err.as_workflow_error().family(), WorkflowErrorFamily::Build);

        let err = WorkflowBuildError::new(
            "Build failed during steps",
            WorkflowBuildErrorOptions {
                hint: Some("run `pnpm install workflow` and try again".to_owned()),
                cause: None,
            },
        );
        assert_eq!(
            err.hint(),
            Some("run `pnpm install workflow` and try again")
        );
        assert_eq!(
            err.message(),
            "Build failed during steps\n╰▶ hint: run `pnpm install workflow` and try again"
        );

        let err = WorkflowBuildError::new(
            "boom",
            WorkflowBuildErrorOptions {
                cause: Some("underlying esbuild failure".to_owned()),
                hint: None,
            },
        );
        assert_eq!(err.cause_message(), Some("underlying esbuild failure"));

        let err = WorkflowBuildError::new("boom", WorkflowBuildErrorOptions::default());
        let other = WorkflowError::new("boom");
        assert!(WorkflowBuildError::is(err.as_workflow_error()));
        assert!(!WorkflowBuildError::is(&other));
    }

    #[test]
    fn upstream_fatal_error_cases() {
        let fatal = FatalError::new("boom");
        assert!(FatalError::is(Some(&fatal)));

        let serialization = SerializationError::new("boom", SerializationErrorOptions::default());
        assert!(FatalError::is(Some(&serialization)));

        let plain = WorkflowError::new("boom");
        assert!(!FatalError::is(Some(&plain)));
        assert!(!FatalError::is(None));

        let mut weird = WorkflowError::new("boom");
        weird.name = "Weird".to_owned();
        weird.fatal = false;
        assert!(!FatalError::is(Some(&weird)));
    }

    #[test]
    fn upstream_runtime_decryption_error_cases() {
        let err =
            RuntimeDecryptionError::new("decrypt failed", RuntimeDecryptionErrorOptions::default());
        assert_eq!(err.name(), "RuntimeDecryptionError");
        assert_eq!(
            err.as_workflow_error().family(),
            WorkflowErrorFamily::Runtime
        );
        assert!(
            err.message()
                .contains("https://workflow-sdk.dev/err/runtime-decryption-failed")
        );

        let err = RuntimeDecryptionError::new(
            "decrypt failed",
            RuntimeDecryptionErrorOptions {
                cause: Some("underlying OperationError".to_owned()),
                context: None,
            },
        );
        assert_eq!(err.cause_message(), Some("underlying OperationError"));

        let context = RuntimeDecryptionErrorContext {
            operation: Some(CryptoOperation::Decrypt),
            byte_length: Some(42),
            format_prefix: Some("encr".to_owned()),
        };
        let err = RuntimeDecryptionError::new(
            "decrypt failed",
            RuntimeDecryptionErrorOptions {
                context: Some(context.clone()),
                cause: None,
            },
        );
        assert_eq!(err.context(), Some(&context));

        let err =
            RuntimeDecryptionError::new("decrypt failed", RuntimeDecryptionErrorOptions::default());
        assert_eq!(err.context(), None);

        let runtime_only = WorkflowRuntimeError::new("decrypt failed");
        let other = WorkflowError::new("decrypt failed");
        assert!(RuntimeDecryptionError::is(err.as_workflow_error()));
        assert!(!RuntimeDecryptionError::is(
            runtime_only.as_workflow_error()
        ));
        assert!(!RuntimeDecryptionError::is(&other));
    }

    #[test]
    fn upstream_serialization_error_cases() {
        let err = SerializationError::new("boom", SerializationErrorOptions::default());
        assert_eq!(err.name(), "SerializationError");
        assert_eq!(
            err.as_workflow_error().family(),
            WorkflowErrorFamily::Serialization
        );
        assert_eq!(err.message(), "boom");

        let err = SerializationError::new(
            "boom",
            SerializationErrorOptions {
                hint: Some("Register the class with WORKFLOW_SERIALIZE.".to_owned()),
                cause: None,
            },
        );
        assert_eq!(
            err.hint(),
            Some("Register the class with WORKFLOW_SERIALIZE.")
        );
        assert_eq!(
            err.message(),
            "boom\n╰▶ hint: Register the class with WORKFLOW_SERIALIZE."
        );

        let err = SerializationError::new(
            "boom",
            SerializationErrorOptions {
                cause: Some("underlying".to_owned()),
                hint: None,
            },
        );
        assert_eq!(err.cause_message(), Some("underlying"));

        let err = SerializationError::new("boom", SerializationErrorOptions::default());
        let other = WorkflowError::new("boom");
        assert!(SerializationError::is(err.as_workflow_error()));
        assert!(!SerializationError::is(&other));

        assert!(err.is_fatal());
        assert!(FatalError::is(Some(&err)));
    }

    #[test]
    fn upstream_corrupted_event_log_error_cases() {
        let err = CorruptedEventLogError::new("event mismatch", None);
        assert_eq!(err.name(), "CorruptedEventLogError");
        assert_eq!(
            err.as_workflow_error().family(),
            WorkflowErrorFamily::Runtime
        );
        assert!(
            err.message()
                .contains("https://workflow-sdk.dev/err/corrupted-event-log")
        );

        let err =
            CorruptedEventLogError::new("event mismatch", Some("underlying mismatch".to_owned()));
        assert_eq!(err.cause_message(), Some("underlying mismatch"));

        let other = WorkflowError::new("event mismatch");
        assert!(CorruptedEventLogError::is(err.as_workflow_error()));
        assert!(!CorruptedEventLogError::is(&other));
    }

    #[test]
    fn stable_serialization_contracts_round_trip() {
        let err = RuntimeDecryptionError::new(
            "decrypt failed",
            RuntimeDecryptionErrorOptions {
                cause: Some("cause".to_owned()),
                context: Some(RuntimeDecryptionErrorContext {
                    operation: Some(CryptoOperation::Decrypt),
                    byte_length: Some(42),
                    format_prefix: Some("encr".to_owned()),
                }),
            },
        );

        let json = serde_json::to_string(&err).unwrap();
        let decoded: RuntimeDecryptionError = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, err);
    }
}
