//! Standardized error types for chat adapters.
//!
//! 1:1 port of `packages/adapter-shared/src/errors.ts`. The upstream
//! `AdapterError` class hierarchy (`AdapterError` plus six subclasses)
//! becomes a single Rust `AdapterError` enum with one variant per upstream
//! class. Subclass-specific fields (`retryAfter`, `resourceType`, etc.) live
//! on the matching variant. The colocated `#[cfg(test)] mod tests` at the
//! bottom maps every `it(...)` from `errors.test.ts`.

use std::error::Error;
use std::fmt;

/// Boxed source error used by [`AdapterError::Network`] to mirror the
/// upstream `originalError?: Error` field.
pub type Source = Box<dyn Error + Send + Sync + 'static>;

/// 1:1 port of upstream `class AdapterError` and its six subclasses
/// (`AdapterRateLimitError`, `AuthenticationError`, `ResourceNotFoundError`,
/// `PermissionError`, `ValidationError`, `NetworkError`).
///
/// Each variant carries the upstream `adapter` field plus its subclass-
/// specific data. The upstream `code` and `name` fields are derived via the
/// matching accessor methods.
#[derive(Debug)]
pub enum AdapterError {
    /// Upstream `class AdapterError`. The `code` field is the caller-supplied
    /// optional identifier; the base variant otherwise carries just a message
    /// and the originating adapter name.
    Base {
        message: String,
        adapter: String,
        code: Option<String>,
    },
    /// Upstream `class AdapterRateLimitError`. `code` is always
    /// `"RATE_LIMITED"`; message is computed from `adapter` and
    /// `retry_after`.
    RateLimit {
        adapter: String,
        retry_after: Option<u64>,
    },
    /// Upstream `class AuthenticationError`. `code` is always
    /// `"AUTH_FAILED"`. `message` is caller-supplied or defaults to
    /// `"Authentication failed for <adapter>"`.
    Authentication {
        adapter: String,
        message: Option<String>,
    },
    /// Upstream `class ResourceNotFoundError`. `code` is always `"NOT_FOUND"`;
    /// message is computed from `resource_type`, optional `resource_id`, and
    /// `adapter`.
    ResourceNotFound {
        adapter: String,
        resource_type: String,
        resource_id: Option<String>,
    },
    /// Upstream `class PermissionError`. `code` is always
    /// `"PERMISSION_DENIED"`; message is computed from `action`,
    /// `adapter`, and optional `required_scope`.
    Permission {
        adapter: String,
        action: String,
        required_scope: Option<String>,
    },
    /// Upstream `class ValidationError`. `code` is always
    /// `"VALIDATION_ERROR"`. Message is caller-supplied.
    Validation { adapter: String, message: String },
    /// Upstream `class NetworkError`. `code` is always `"NETWORK_ERROR"`.
    /// `message` is caller-supplied or defaults to
    /// `"Network error communicating with <adapter>"`. `original_error`
    /// mirrors the upstream `originalError?: Error` wrapper field.
    Network {
        adapter: String,
        message: Option<String>,
        original_error: Option<Source>,
    },
}

impl AdapterError {
    /// Construct the base `AdapterError`. Mirrors
    /// `new AdapterError(message, adapter, code?)`.
    pub fn new(message: impl Into<String>, adapter: impl Into<String>) -> Self {
        Self::Base {
            message: message.into(),
            adapter: adapter.into(),
            code: None,
        }
    }

    /// Construct the base `AdapterError` with an explicit `code`.
    pub fn with_code(
        message: impl Into<String>,
        adapter: impl Into<String>,
        code: impl Into<String>,
    ) -> Self {
        Self::Base {
            message: message.into(),
            adapter: adapter.into(),
            code: Some(code.into()),
        }
    }

    /// Construct `AdapterRateLimitError`. Mirrors
    /// `new AdapterRateLimitError(adapter, retryAfter?)`.
    pub fn rate_limit(adapter: impl Into<String>) -> Self {
        Self::RateLimit {
            adapter: adapter.into(),
            retry_after: None,
        }
    }

    /// Construct `AdapterRateLimitError` with retry-after seconds.
    pub fn rate_limit_after(adapter: impl Into<String>, retry_after: u64) -> Self {
        Self::RateLimit {
            adapter: adapter.into(),
            retry_after: Some(retry_after),
        }
    }

    /// Construct `AuthenticationError`. Mirrors
    /// `new AuthenticationError(adapter, message?)`.
    pub fn authentication(adapter: impl Into<String>) -> Self {
        Self::Authentication {
            adapter: adapter.into(),
            message: None,
        }
    }

    /// Construct `AuthenticationError` with explicit message.
    pub fn authentication_with(adapter: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Authentication {
            adapter: adapter.into(),
            message: Some(message.into()),
        }
    }

    /// Construct `ResourceNotFoundError`. Mirrors
    /// `new ResourceNotFoundError(adapter, resourceType, resourceId?)`.
    pub fn resource_not_found(
        adapter: impl Into<String>,
        resource_type: impl Into<String>,
    ) -> Self {
        Self::ResourceNotFound {
            adapter: adapter.into(),
            resource_type: resource_type.into(),
            resource_id: None,
        }
    }

    /// Construct `ResourceNotFoundError` with a specific resource id.
    pub fn resource_not_found_with_id(
        adapter: impl Into<String>,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
    ) -> Self {
        Self::ResourceNotFound {
            adapter: adapter.into(),
            resource_type: resource_type.into(),
            resource_id: Some(resource_id.into()),
        }
    }

    /// Construct `PermissionError`. Mirrors
    /// `new PermissionError(adapter, action, requiredScope?)`.
    pub fn permission(adapter: impl Into<String>, action: impl Into<String>) -> Self {
        Self::Permission {
            adapter: adapter.into(),
            action: action.into(),
            required_scope: None,
        }
    }

    /// Construct `PermissionError` with explicit required scope.
    pub fn permission_with_scope(
        adapter: impl Into<String>,
        action: impl Into<String>,
        required_scope: impl Into<String>,
    ) -> Self {
        Self::Permission {
            adapter: adapter.into(),
            action: action.into(),
            required_scope: Some(required_scope.into()),
        }
    }

    /// Construct `ValidationError`. Mirrors
    /// `new ValidationError(adapter, message)`.
    pub fn validation(adapter: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation {
            adapter: adapter.into(),
            message: message.into(),
        }
    }

    /// Construct `NetworkError`. Mirrors
    /// `new NetworkError(adapter, message?, originalError?)`.
    pub fn network(adapter: impl Into<String>) -> Self {
        Self::Network {
            adapter: adapter.into(),
            message: None,
            original_error: None,
        }
    }

    /// Construct `NetworkError` with an explicit message.
    pub fn network_with(adapter: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Network {
            adapter: adapter.into(),
            message: Some(message.into()),
            original_error: None,
        }
    }

    /// Construct `NetworkError` with both an explicit message and a wrapped
    /// originating error.
    pub fn network_wrapped(
        adapter: impl Into<String>,
        message: impl Into<String>,
        original_error: Source,
    ) -> Self {
        Self::Network {
            adapter: adapter.into(),
            message: Some(message.into()),
            original_error: Some(original_error),
        }
    }

    /// Upstream `adapter` field — the name of the originating adapter
    /// (`"slack"`, `"teams"`, …).
    pub fn adapter(&self) -> &str {
        match self {
            Self::Base { adapter, .. }
            | Self::RateLimit { adapter, .. }
            | Self::Authentication { adapter, .. }
            | Self::ResourceNotFound { adapter, .. }
            | Self::Permission { adapter, .. }
            | Self::Validation { adapter, .. }
            | Self::Network { adapter, .. } => adapter,
        }
    }

    /// Upstream `code` field. Constant for the typed variants and optional
    /// (caller-supplied) for the [`Self::Base`] variant.
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Base { code, .. } => code.as_deref(),
            Self::RateLimit { .. } => Some("RATE_LIMITED"),
            Self::Authentication { .. } => Some("AUTH_FAILED"),
            Self::ResourceNotFound { .. } => Some("NOT_FOUND"),
            Self::Permission { .. } => Some("PERMISSION_DENIED"),
            Self::Validation { .. } => Some("VALIDATION_ERROR"),
            Self::Network { .. } => Some("NETWORK_ERROR"),
        }
    }

    /// Upstream `name` field — matches the JS class name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Base { .. } => "AdapterError",
            Self::RateLimit { .. } => "AdapterRateLimitError",
            Self::Authentication { .. } => "AuthenticationError",
            Self::ResourceNotFound { .. } => "ResourceNotFoundError",
            Self::Permission { .. } => "PermissionError",
            Self::Validation { .. } => "ValidationError",
            Self::Network { .. } => "NetworkError",
        }
    }

    /// Upstream `message` field. For variants whose constructor formats the
    /// message from other fields, this method reproduces that formatted
    /// string exactly.
    pub fn message(&self) -> String {
        match self {
            Self::Base { message, .. } => message.clone(),
            Self::RateLimit {
                adapter,
                retry_after,
            } => match retry_after {
                Some(seconds) => format!("Rate limited by {adapter}, retry after {seconds}s"),
                None => format!("Rate limited by {adapter}"),
            },
            Self::Authentication { adapter, message } => match message {
                Some(custom) => custom.clone(),
                None => format!("Authentication failed for {adapter}"),
            },
            Self::ResourceNotFound {
                adapter,
                resource_type,
                resource_id,
            } => match resource_id {
                Some(id) => format!("{resource_type} '{id}' not found in {adapter}"),
                None => format!("{resource_type} not found in {adapter}"),
            },
            Self::Permission {
                adapter,
                action,
                required_scope,
            } => match required_scope {
                Some(scope) => {
                    format!("Permission denied: cannot {action} in {adapter} (requires: {scope})")
                }
                None => format!("Permission denied: cannot {action} in {adapter}"),
            },
            Self::Validation { message, .. } => message.clone(),
            Self::Network {
                adapter, message, ..
            } => match message {
                Some(custom) => custom.clone(),
                None => format!("Network error communicating with {adapter}"),
            },
        }
    }

    /// `true` if this is a `AdapterRateLimitError`.
    pub fn is_rate_limit(&self) -> bool {
        matches!(self, Self::RateLimit { .. })
    }

    /// `true` if this is an `AuthenticationError`.
    pub fn is_authentication(&self) -> bool {
        matches!(self, Self::Authentication { .. })
    }

    /// `true` if this is a `ResourceNotFoundError`.
    pub fn is_resource_not_found(&self) -> bool {
        matches!(self, Self::ResourceNotFound { .. })
    }

    /// `true` if this is a `PermissionError`.
    pub fn is_permission(&self) -> bool {
        matches!(self, Self::Permission { .. })
    }

    /// `true` if this is a `ValidationError`.
    pub fn is_validation(&self) -> bool {
        matches!(self, Self::Validation { .. })
    }

    /// `true` if this is a `NetworkError`.
    pub fn is_network(&self) -> bool {
        matches!(self, Self::Network { .. })
    }

    /// Upstream `retryAfter` on `AdapterRateLimitError`. `None` for other
    /// variants and for rate-limit errors constructed without a hint.
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            Self::RateLimit { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Upstream `resourceType` on `ResourceNotFoundError`. `None` otherwise.
    pub fn resource_type(&self) -> Option<&str> {
        match self {
            Self::ResourceNotFound { resource_type, .. } => Some(resource_type),
            _ => None,
        }
    }

    /// Upstream `resourceId` on `ResourceNotFoundError`. `None` otherwise or
    /// when the id is absent.
    pub fn resource_id(&self) -> Option<&str> {
        match self {
            Self::ResourceNotFound { resource_id, .. } => resource_id.as_deref(),
            _ => None,
        }
    }

    /// Upstream `action` on `PermissionError`. `None` otherwise.
    pub fn action(&self) -> Option<&str> {
        match self {
            Self::Permission { action, .. } => Some(action),
            _ => None,
        }
    }

    /// Upstream `requiredScope` on `PermissionError`. `None` otherwise or
    /// when the scope is absent.
    pub fn required_scope(&self) -> Option<&str> {
        match self {
            Self::Permission { required_scope, .. } => required_scope.as_deref(),
            _ => None,
        }
    }

    /// Upstream `originalError` on `NetworkError`. `None` otherwise.
    pub fn original_error(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Network { original_error, .. } => original_error
                .as_deref()
                .map(|e| e as &(dyn Error + 'static)),
            _ => None,
        }
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl Error for AdapterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.original_error()
    }
}

#[cfg(test)]
mod tests {
    //! 1:1 port of `packages/adapter-shared/src/errors.test.ts` from upstream
    //! `vercel/chat` @ `aba6aa94fe5a2ed909ec4daa7db0e21887507fa4`.
    //!
    //! Each test mirrors the original `it(...)` description. Upstream
    //! `expect(err).toBeInstanceOf(AdapterError)` becomes a Rust
    //! `matches!`/`is_*` check against the `AdapterError` enum.

    use super::*;

    // describe("AdapterError")

    #[test]
    fn adapter_error_creates_error_with_message_adapter_and_code() {
        let error = AdapterError::with_code("Something failed", "slack", "CUSTOM_CODE");
        assert_eq!(error.message(), "Something failed");
        assert_eq!(error.adapter(), "slack");
        assert_eq!(error.code(), Some("CUSTOM_CODE"));
        assert_eq!(error.name(), "AdapterError");
    }

    #[test]
    fn adapter_error_is_an_instance_of_error() {
        let error = AdapterError::new("test", "slack");
        // Rust analogue of `error instanceof Error`: the value implements
        // std::error::Error.
        let _as_error: &dyn std::error::Error = &error;
        assert!(matches!(error, AdapterError::Base { .. }));
    }

    #[test]
    fn adapter_error_works_without_code() {
        let error = AdapterError::new("test", "teams");
        assert_eq!(error.code(), None);
    }

    // describe("AdapterRateLimitError")

    #[test]
    fn rate_limit_error_creates_error_with_retry_after() {
        let error = AdapterError::rate_limit_after("slack", 30);
        assert_eq!(error.message(), "Rate limited by slack, retry after 30s");
        assert_eq!(error.adapter(), "slack");
        assert_eq!(error.code(), Some("RATE_LIMITED"));
        assert_eq!(error.retry_after(), Some(30));
        assert_eq!(error.name(), "AdapterRateLimitError");
    }

    #[test]
    fn rate_limit_error_creates_error_without_retry_after() {
        let error = AdapterError::rate_limit("teams");
        assert_eq!(error.message(), "Rate limited by teams");
        assert_eq!(error.retry_after(), None);
    }

    #[test]
    fn rate_limit_error_is_an_instance_of_adapter_error() {
        let error = AdapterError::rate_limit("slack");
        assert!(error.is_rate_limit());
    }

    // describe("AuthenticationError")

    #[test]
    fn authentication_error_creates_error_with_custom_message() {
        let error = AdapterError::authentication_with("slack", "Token expired");
        assert_eq!(error.message(), "Token expired");
        assert_eq!(error.adapter(), "slack");
        assert_eq!(error.code(), Some("AUTH_FAILED"));
        assert_eq!(error.name(), "AuthenticationError");
    }

    #[test]
    fn authentication_error_creates_error_with_default_message() {
        let error = AdapterError::authentication("teams");
        assert_eq!(error.message(), "Authentication failed for teams");
    }

    #[test]
    fn authentication_error_is_an_instance_of_adapter_error() {
        let error = AdapterError::authentication("slack");
        assert!(error.is_authentication());
    }

    // describe("ResourceNotFoundError")

    #[test]
    fn resource_not_found_error_creates_error_with_resource_type_and_id() {
        let error = AdapterError::resource_not_found_with_id("slack", "channel", "C123456");
        assert_eq!(error.message(), "channel 'C123456' not found in slack");
        assert_eq!(error.adapter(), "slack");
        assert_eq!(error.code(), Some("NOT_FOUND"));
        assert_eq!(error.resource_type(), Some("channel"));
        assert_eq!(error.resource_id(), Some("C123456"));
        assert_eq!(error.name(), "ResourceNotFoundError");
    }

    #[test]
    fn resource_not_found_error_creates_error_without_resource_id() {
        let error = AdapterError::resource_not_found("teams", "user");
        assert_eq!(error.message(), "user not found in teams");
        assert_eq!(error.resource_id(), None);
    }

    #[test]
    fn resource_not_found_error_is_an_instance_of_adapter_error() {
        let error = AdapterError::resource_not_found("slack", "thread");
        assert!(error.is_resource_not_found());
    }

    // describe("PermissionError")

    #[test]
    fn permission_error_creates_error_with_action_and_scope() {
        let error = AdapterError::permission_with_scope("slack", "send messages", "chat:write");
        assert_eq!(
            error.message(),
            "Permission denied: cannot send messages in slack (requires: chat:write)"
        );
        assert_eq!(error.adapter(), "slack");
        assert_eq!(error.code(), Some("PERMISSION_DENIED"));
        assert_eq!(error.action(), Some("send messages"));
        assert_eq!(error.required_scope(), Some("chat:write"));
        assert_eq!(error.name(), "PermissionError");
    }

    #[test]
    fn permission_error_creates_error_without_scope() {
        let error = AdapterError::permission("teams", "delete messages");
        assert_eq!(
            error.message(),
            "Permission denied: cannot delete messages in teams"
        );
        assert_eq!(error.required_scope(), None);
    }

    #[test]
    fn permission_error_is_an_instance_of_adapter_error() {
        let error = AdapterError::permission("gchat", "test");
        assert!(error.is_permission());
    }

    // describe("ValidationError")

    #[test]
    fn validation_error_creates_error_with_message() {
        let error = AdapterError::validation("slack", "Message text exceeds 40000 characters");
        assert_eq!(error.message(), "Message text exceeds 40000 characters");
        assert_eq!(error.adapter(), "slack");
        assert_eq!(error.code(), Some("VALIDATION_ERROR"));
        assert_eq!(error.name(), "ValidationError");
    }

    #[test]
    fn validation_error_is_an_instance_of_adapter_error() {
        let error = AdapterError::validation("teams", "Invalid");
        assert!(error.is_validation());
    }

    // describe("NetworkError")

    #[test]
    fn network_error_creates_error_with_custom_message() {
        let error = AdapterError::network_with("slack", "Connection timeout after 30s");
        assert_eq!(error.message(), "Connection timeout after 30s");
        assert_eq!(error.adapter(), "slack");
        assert_eq!(error.code(), Some("NETWORK_ERROR"));
        assert_eq!(error.name(), "NetworkError");
    }

    #[test]
    fn network_error_creates_error_with_default_message() {
        let error = AdapterError::network("gchat");
        assert_eq!(error.message(), "Network error communicating with gchat");
    }

    #[test]
    fn network_error_can_wrap_original_error() {
        let original: Source = Box::new(std::io::Error::other("ECONNREFUSED"));
        let error = AdapterError::network_wrapped("teams", "Connection refused", original);
        assert!(error.original_error().is_some());
        assert_eq!(error.original_error().unwrap().to_string(), "ECONNREFUSED");
    }

    #[test]
    fn network_error_is_an_instance_of_adapter_error() {
        let error = AdapterError::network("slack");
        assert!(error.is_network());
    }

    // describe("Error hierarchy")

    #[test]
    fn error_hierarchy_all_errors_extend_adapter_error() {
        let errors: Vec<AdapterError> = vec![
            AdapterError::rate_limit("slack"),
            AdapterError::authentication("slack"),
            AdapterError::resource_not_found("slack", "test"),
            AdapterError::permission("slack", "test"),
            AdapterError::validation("slack", "test"),
            AdapterError::network("slack"),
        ];
        for error in &errors {
            // Rust analogue of `instanceof AdapterError` / `instanceof Error`:
            // every variant IS an AdapterError, and the type implements
            // std::error::Error.
            let _as_error: &dyn std::error::Error = error;
        }
    }

    #[test]
    fn error_hierarchy_can_be_caught_by_adapter_name() {
        let mut slack_errors: Vec<&AdapterError> = Vec::new();
        let caught = AdapterError::rate_limit_after("slack", 30);
        if caught.adapter() == "slack" {
            slack_errors.push(&caught);
        }
        assert_eq!(slack_errors.len(), 1);
        assert_eq!(slack_errors[0].adapter(), "slack");
    }

    #[test]
    fn error_hierarchy_can_be_caught_by_error_code() {
        let errors: Vec<AdapterError> = vec![
            AdapterError::rate_limit("slack"),
            AdapterError::authentication("teams"),
            AdapterError::rate_limit("gchat"),
        ];
        let rate_limit_errors: Vec<&AdapterError> = errors
            .iter()
            .filter(|e| e.code() == Some("RATE_LIMITED"))
            .collect();
        assert_eq!(rate_limit_errors.len(), 2);
    }
}
