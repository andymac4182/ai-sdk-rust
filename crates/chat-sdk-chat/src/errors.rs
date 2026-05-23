//! Error types for `chat-sdk-chat`.
//!
//! 1:1 port of `packages/chat/src/errors.ts`. The upstream `ChatError` class
//! hierarchy (`ChatError`, `RateLimitError`, `LockError`, `NotImplementedError`)
//! becomes a single Rust `ChatError` enum with one variant per subclass.
//! Tests in `tests/errors.rs` map every original `it(...)` case.

use std::error::Error;
use std::fmt;

/// Boxed cause matching the upstream `cause?: unknown` field.
pub type Cause = Box<dyn Error + Send + Sync + 'static>;

/// 1:1 port of upstream `ChatError` and its three subclasses (`RateLimitError`,
/// `LockError`, `NotImplementedError`). Each variant carries the upstream
/// `code` string, `message`, and optional `cause`. Subclass-specific data
/// (`retryAfterMs`, `feature`) lives on the matching variant.
#[derive(Debug)]
pub enum ChatError {
    /// Upstream `class ChatError`. `code` is the caller-supplied identifier.
    Base {
        message: String,
        code: String,
        cause: Option<Cause>,
    },
    /// Upstream `class RateLimitError extends ChatError`. `code` is always
    /// `"RATE_LIMITED"` (see [`Self::code`]).
    RateLimit {
        message: String,
        retry_after_ms: Option<u64>,
        cause: Option<Cause>,
    },
    /// Upstream `class LockError extends ChatError`. `code` is always
    /// `"LOCK_FAILED"`.
    Lock {
        message: String,
        cause: Option<Cause>,
    },
    /// Upstream `class NotImplementedError extends ChatError`. `code` is
    /// always `"NOT_IMPLEMENTED"`.
    NotImplemented {
        message: String,
        feature: Option<String>,
        cause: Option<Cause>,
    },
}

impl ChatError {
    /// Construct the base `ChatError` variant. Mirrors `new ChatError(message, code, cause?)`.
    pub fn new(message: impl Into<String>, code: impl Into<String>) -> Self {
        Self::Base {
            message: message.into(),
            code: code.into(),
            cause: None,
        }
    }

    /// Construct the base `ChatError` with an underlying cause.
    pub fn with_cause(message: impl Into<String>, code: impl Into<String>, cause: Cause) -> Self {
        Self::Base {
            message: message.into(),
            code: code.into(),
            cause: Some(cause),
        }
    }

    /// Construct the `RateLimitError` variant. Mirrors
    /// `new RateLimitError(message, retryAfterMs?, cause?)`.
    pub fn rate_limit(message: impl Into<String>) -> Self {
        Self::RateLimit {
            message: message.into(),
            retry_after_ms: None,
            cause: None,
        }
    }

    /// Construct a `RateLimitError` with `retryAfterMs`.
    pub fn rate_limit_after(message: impl Into<String>, retry_after_ms: u64) -> Self {
        Self::RateLimit {
            message: message.into(),
            retry_after_ms: Some(retry_after_ms),
            cause: None,
        }
    }

    /// Construct the `LockError` variant. Mirrors `new LockError(message, cause?)`.
    pub fn lock(message: impl Into<String>) -> Self {
        Self::Lock {
            message: message.into(),
            cause: None,
        }
    }

    /// Construct the `NotImplementedError` variant. Mirrors
    /// `new NotImplementedError(message, feature?, cause?)`.
    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self::NotImplemented {
            message: message.into(),
            feature: None,
            cause: None,
        }
    }

    /// Construct a `NotImplementedError` naming the missing feature.
    pub fn not_implemented_feature(message: impl Into<String>, feature: impl Into<String>) -> Self {
        Self::NotImplemented {
            message: message.into(),
            feature: Some(feature.into()),
            cause: None,
        }
    }

    /// Attach a cause to any variant. Equivalent to the upstream
    /// `cause` constructor argument.
    pub fn with_source(mut self, source: Cause) -> Self {
        match &mut self {
            Self::Base { cause, .. }
            | Self::RateLimit { cause, .. }
            | Self::Lock { cause, .. }
            | Self::NotImplemented { cause, .. } => {
                *cause = Some(source);
            }
        }
        self
    }

    /// Upstream `code` field. Constant for the typed variants; caller-supplied
    /// for the base variant.
    pub fn code(&self) -> &str {
        match self {
            Self::Base { code, .. } => code,
            Self::RateLimit { .. } => "RATE_LIMITED",
            Self::Lock { .. } => "LOCK_FAILED",
            Self::NotImplemented { .. } => "NOT_IMPLEMENTED",
        }
    }

    /// Upstream `name` field on each error class.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Base { .. } => "ChatError",
            Self::RateLimit { .. } => "RateLimitError",
            Self::Lock { .. } => "LockError",
            Self::NotImplemented { .. } => "NotImplementedError",
        }
    }

    /// Upstream `message` field.
    pub fn message(&self) -> &str {
        match self {
            Self::Base { message, .. }
            | Self::RateLimit { message, .. }
            | Self::Lock { message, .. }
            | Self::NotImplemented { message, .. } => message,
        }
    }

    /// `true` if this is a `RateLimitError`. Mirrors `err instanceof RateLimitError`.
    pub fn is_rate_limit(&self) -> bool {
        matches!(self, Self::RateLimit { .. })
    }

    /// `true` if this is a `LockError`. Mirrors `err instanceof LockError`.
    pub fn is_lock(&self) -> bool {
        matches!(self, Self::Lock { .. })
    }

    /// `true` if this is a `NotImplementedError`. Mirrors
    /// `err instanceof NotImplementedError`.
    pub fn is_not_implemented(&self) -> bool {
        matches!(self, Self::NotImplemented { .. })
    }

    /// Upstream `retryAfterMs` on `RateLimitError`. `None` for other variants.
    pub fn retry_after_ms(&self) -> Option<u64> {
        match self {
            Self::RateLimit { retry_after_ms, .. } => *retry_after_ms,
            _ => None,
        }
    }

    /// Upstream `feature` on `NotImplementedError`. `None` for other variants.
    pub fn feature(&self) -> Option<&str> {
        match self {
            Self::NotImplemented { feature, .. } => feature.as_deref(),
            _ => None,
        }
    }
}

impl fmt::Display for ChatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl Error for ChatError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        let cause = match self {
            Self::Base { cause, .. }
            | Self::RateLimit { cause, .. }
            | Self::Lock { cause, .. }
            | Self::NotImplemented { cause, .. } => cause.as_deref(),
        };
        cause.map(|c| c as &(dyn Error + 'static))
    }
}
