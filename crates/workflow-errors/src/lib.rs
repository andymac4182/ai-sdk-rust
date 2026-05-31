//! Error crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/errors`. The core crate only needs a
//! narrow taxonomy for run-failure classification in this bucket, so the
//! constructors below model the error names and signal fields consumed by
//! upstream `packages/core/src/classify-error.ts` and `describe-error.ts`.

#![forbid(unsafe_code)]

use std::{error::Error, fmt};

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the initial crate skeleton.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/errors";

/// Upstream package version inventoried for this skeleton.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.6";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Persisted workflow run error codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RunErrorCode {
    UserError,
    RuntimeError,
    CorruptedEventLog,
    ReplayTimeout,
    MaxDeliveriesExceeded,
    WorldContractError,
}

impl RunErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UserError => "USER_ERROR",
            Self::RuntimeError => "RUNTIME_ERROR",
            Self::CorruptedEventLog => "CORRUPTED_EVENT_LOG",
            Self::ReplayTimeout => "REPLAY_TIMEOUT",
            Self::MaxDeliveriesExceeded => "MAX_DELIVERIES_EXCEEDED",
            Self::WorldContractError => "WORLD_CONTRACT_ERROR",
        }
    }

    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "USER_ERROR" => Some(Self::UserError),
            "RUNTIME_ERROR" => Some(Self::RuntimeError),
            "CORRUPTED_EVENT_LOG" => Some(Self::CorruptedEventLog),
            "REPLAY_TIMEOUT" => Some(Self::ReplayTimeout),
            "MAX_DELIVERIES_EXCEEDED" => Some(Self::MaxDeliveriesExceeded),
            "WORLD_CONTRACT_ERROR" => Some(Self::WorldContractError),
            _ => None,
        }
    }
}

impl fmt::Display for RunErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error family markers used by the core run-classification helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WorkflowErrorKind {
    Fatal,
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

/// Structured Workflow SDK error signal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowError {
    name: String,
    message: String,
    kind: WorkflowErrorKind,
    code: Option<String>,
    status: Option<u16>,
    cause_name: Option<String>,
    stack: Option<String>,
    fatal: bool,
}

impl WorkflowError {
    #[must_use]
    pub fn new(kind: WorkflowErrorKind, message: impl Into<String>) -> Self {
        let name = match kind {
            WorkflowErrorKind::Fatal => "FatalError",
            WorkflowErrorKind::WorkflowRuntime => "WorkflowRuntimeError",
            WorkflowErrorKind::StepNotRegistered => "StepNotRegisteredError",
            WorkflowErrorKind::WorkflowNotRegistered => "WorkflowNotRegisteredError",
            WorkflowErrorKind::CorruptedEventLog => "CorruptedEventLogError",
            WorkflowErrorKind::RuntimeDecryption => "RuntimeDecryptionError",
            WorkflowErrorKind::Serialization => "SerializationError",
            WorkflowErrorKind::WorkflowWorld => "WorkflowWorldError",
            WorkflowErrorKind::HookConflict => "HookConflictError",
            WorkflowErrorKind::Native => "Error",
        };

        Self {
            name: name.to_string(),
            message: message.into(),
            kind,
            code: None,
            status: None,
            cause_name: None,
            stack: None,
            fatal: matches!(kind, WorkflowErrorKind::Fatal),
        }
    }

    #[must_use]
    pub fn fatal(message: impl Into<String>) -> Self {
        Self::new(WorkflowErrorKind::Fatal, message)
    }

    #[must_use]
    pub fn workflow_runtime(message: impl Into<String>) -> Self {
        Self::new(WorkflowErrorKind::WorkflowRuntime, message)
    }

    #[must_use]
    pub fn step_not_registered(step_name: impl AsRef<str>) -> Self {
        Self::new(
            WorkflowErrorKind::StepNotRegistered,
            format!("Step not registered: {}", step_name.as_ref()),
        )
    }

    #[must_use]
    pub fn workflow_not_registered(workflow_name: impl AsRef<str>) -> Self {
        Self::new(
            WorkflowErrorKind::WorkflowNotRegistered,
            format!("Workflow not registered: {}", workflow_name.as_ref()),
        )
    }

    #[must_use]
    pub fn corrupted_event_log(message: impl Into<String>) -> Self {
        Self::new(WorkflowErrorKind::CorruptedEventLog, message)
    }

    #[must_use]
    pub fn runtime_decryption(message: impl Into<String>) -> Self {
        Self::new(WorkflowErrorKind::RuntimeDecryption, message)
    }

    #[must_use]
    pub fn serialization(message: impl Into<String>) -> Self {
        Self::new(WorkflowErrorKind::Serialization, message)
    }

    #[must_use]
    pub fn hook_conflict(token: impl AsRef<str>) -> Self {
        Self::new(
            WorkflowErrorKind::HookConflict,
            format!("Hook token already exists: {}", token.as_ref()),
        )
    }

    #[must_use]
    pub fn native(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message: message.into(),
            kind: WorkflowErrorKind::Native,
            code: None,
            status: None,
            cause_name: None,
            stack: None,
            fatal: false,
        }
    }

    #[must_use]
    pub fn workflow_world(message: impl Into<String>) -> Self {
        Self::new(WorkflowErrorKind::WorkflowWorld, message)
    }

    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    #[must_use]
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    #[must_use]
    pub fn with_cause_name(mut self, cause_name: impl Into<String>) -> Self {
        self.cause_name = Some(cause_name.into());
        self
    }

    #[must_use]
    pub fn with_stack(mut self, stack: impl Into<String>) -> Self {
        self.stack = Some(stack.into());
        self
    }

    #[must_use]
    pub fn with_fatal(mut self, fatal: bool) -> Self {
        self.fatal = fatal;
        self
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub const fn kind(&self) -> WorkflowErrorKind {
        self.kind
    }

    #[must_use]
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    #[must_use]
    pub const fn status(&self) -> Option<u16> {
        self.status
    }

    #[must_use]
    pub fn cause_name(&self) -> Option<&str> {
        self.cause_name.as_deref()
    }

    #[must_use]
    pub fn stack(&self) -> Option<&str> {
        self.stack.as_deref()
    }

    #[must_use]
    pub const fn is_fatal(&self) -> bool {
        self.fatal
    }

    #[must_use]
    pub fn is_runtime_error_family(&self) -> bool {
        matches!(
            self.kind,
            WorkflowErrorKind::WorkflowRuntime
                | WorkflowErrorKind::StepNotRegistered
                | WorkflowErrorKind::WorkflowNotRegistered
                | WorkflowErrorKind::RuntimeDecryption
        )
    }
}

impl fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for WorkflowError {}
