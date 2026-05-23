use crate::warning::Warning;
use std::sync::{LazyLock, Mutex};

#[cfg(test)]
use std::cell::RefCell;

/// Informational message emitted before the first warning batch.
pub const FIRST_WARNING_INFO_MESSAGE: &str = "AI SDK Warning System: To turn off warning logging, set the AI_SDK_LOG_WARNINGS global to false.";

/// Warning logging input, including optional provider/model scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogWarningsOptions {
    /// The warnings returned by the model provider.
    pub warnings: Vec<Warning>,

    /// The provider id used for the call, if scoped to a specific provider.
    pub provider: Option<String>,

    /// The model id used for the call, if scoped to a specific provider.
    pub model: Option<String>,
}

impl LogWarningsOptions {
    /// Creates warning logging options with no provider/model scope.
    pub fn new(warnings: Vec<Warning>) -> Self {
        Self {
            warnings,
            provider: None,
            model: None,
        }
    }

    /// Sets the provider scope.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Sets the model scope.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Sets provider and model scope together.
    pub fn with_scope(mut self, provider: impl Into<String>, model: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self.model = Some(model.into());
        self
    }
}

/// Warning sink type that should receive a formatted warning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WarningLogKind {
    /// Standard warning sink.
    Warning,

    /// Deprecation warning sink.
    DeprecationWarning,
}

/// Deterministic warning logger output record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WarningLogRecord {
    /// Informational record emitted before the first warning batch.
    Info(String),

    /// Formatted warning record.
    Warning {
        /// Formatted warning message.
        message: String,

        /// Sink type for the warning.
        kind: WarningLogKind,
    },
}

/// Stateful warning logger matching the upstream first-call behavior.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WarningLogger {
    enabled: bool,
    has_logged_before: bool,
}

static WARNING_LOGGER: LazyLock<Mutex<WarningLogger>> =
    LazyLock::new(|| Mutex::new(WarningLogger::new()));

#[cfg(test)]
thread_local! {
    static LOG_WARNING_CALLS: RefCell<Vec<LogWarningsOptions>> = const { RefCell::new(Vec::new()) };
}

impl Default for WarningLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl WarningLogger {
    /// Creates an enabled warning logger.
    pub const fn new() -> Self {
        Self {
            enabled: true,
            has_logged_before: false,
        }
    }

    /// Creates a disabled warning logger.
    pub const fn disabled() -> Self {
        Self {
            enabled: false,
            has_logged_before: false,
        }
    }

    /// Returns whether this logger emits records.
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enables or disables this logger.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Resets first-call tracking.
    pub fn reset(&mut self) {
        self.has_logged_before = false;
    }

    /// Returns formatted records for the provided warnings.
    pub fn log_warnings(&mut self, options: &LogWarningsOptions) -> Vec<WarningLogRecord> {
        if !self.enabled || options.warnings.is_empty() {
            return Vec::new();
        }

        let mut records = Vec::new();
        if !self.has_logged_before {
            self.has_logged_before = true;
            records.push(WarningLogRecord::Info(
                FIRST_WARNING_INFO_MESSAGE.to_string(),
            ));
        }

        records.extend(
            options
                .warnings
                .iter()
                .map(|warning| WarningLogRecord::Warning {
                    message: format_warning(
                        warning,
                        options.provider.as_deref(),
                        options.model.as_deref(),
                    ),
                    kind: warning_log_kind(warning),
                }),
        );

        records
    }
}

/// Formats a provider warning using the upstream AI SDK warning message shape.
pub fn format_warning(warning: &Warning, provider: Option<&str>, model: Option<&str>) -> String {
    let scope = match (provider, model) {
        (Some(provider), Some(model)) => format!(" ({provider} / {model})"),
        _ => String::new(),
    };
    let prefix = format!("AI SDK Warning{scope}:");

    match warning {
        Warning::Unsupported { feature, details } => {
            let mut message = format!("{prefix} The feature \"{feature}\" is not supported.");
            if let Some(details) = details {
                message.push(' ');
                message.push_str(details);
            }
            message
        }
        Warning::Compatibility { feature, details } => {
            let mut message =
                format!("{prefix} The feature \"{feature}\" is used in a compatibility mode.");
            if let Some(details) = details {
                message.push(' ');
                message.push_str(details);
            }
            message
        }
        Warning::Deprecated { setting, message } => {
            format!("{prefix} Deprecated: \"{setting}\". {message}")
        }
        Warning::Other { message } => format!("{prefix} {message}"),
    }
}

/// Logs warnings with the process-wide AI SDK warning logger state.
pub fn log_warnings(options: &LogWarningsOptions) -> Vec<WarningLogRecord> {
    #[cfg(test)]
    LOG_WARNING_CALLS.with(|calls| calls.borrow_mut().push(options.clone()));

    WARNING_LOGGER
        .lock()
        .expect("warning logger mutex is not poisoned")
        .log_warnings(options)
}

#[cfg(test)]
pub(crate) fn take_log_warning_calls_for_tests() -> Vec<LogWarningsOptions> {
    LOG_WARNING_CALLS.with(|calls| std::mem::take(&mut *calls.borrow_mut()))
}

/// Calls a custom warning logger with the original options when warnings exist.
pub fn log_warnings_with_custom_logger(
    options: &LogWarningsOptions,
    custom_logger: impl FnOnce(&LogWarningsOptions),
) -> bool {
    if options.warnings.is_empty() {
        return false;
    }

    custom_logger(options);
    true
}

/// Enables or disables the process-wide warning logger.
pub fn set_log_warnings_enabled(enabled: bool) {
    WARNING_LOGGER
        .lock()
        .expect("warning logger mutex is not poisoned")
        .set_enabled(enabled);
}

/// Resets the process-wide warning logger first-call state.
pub fn reset_log_warnings_state() {
    WARNING_LOGGER
        .lock()
        .expect("warning logger mutex is not poisoned")
        .reset();
}

fn warning_log_kind(warning: &Warning) -> WarningLogKind {
    match warning {
        Warning::Deprecated { .. } => WarningLogKind::DeprecationWarning,
        _ => WarningLogKind::Warning,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FIRST_WARNING_INFO_MESSAGE, LogWarningsOptions, WARNING_LOGGER, WarningLogKind,
        WarningLogRecord, WarningLogger, format_warning, log_warnings_with_custom_logger,
    };
    use crate::warning::Warning;

    #[test]
    fn format_warning_matches_upstream_warning_messages() {
        assert_eq!(
            format_warning(
                &Warning::Unsupported {
                    feature: "temperature".to_string(),
                    details: Some("Temperature not supported".to_string()),
                },
                Some("providerX"),
                Some("modelY"),
            ),
            "AI SDK Warning (providerX / modelY): The feature \"temperature\" is not supported. Temperature not supported"
        );
        assert_eq!(
            format_warning(
                &Warning::Compatibility {
                    feature: "json-mode".to_string(),
                    details: None,
                },
                None,
                None,
            ),
            "AI SDK Warning: The feature \"json-mode\" is used in a compatibility mode."
        );
        assert_eq!(
            format_warning(
                &Warning::Deprecated {
                    setting: "functionCalling".to_string(),
                    message: "Use tools instead.".to_string(),
                },
                Some("openai"),
                Some("gpt-4.1"),
            ),
            "AI SDK Warning (openai / gpt-4.1): Deprecated: \"functionCalling\". Use tools instead."
        );
        assert_eq!(
            format_warning(
                &Warning::Other {
                    message: "Provider returned a non-fatal warning.".to_string(),
                },
                Some("provider-only"),
                None,
            ),
            "AI SDK Warning: Provider returned a non-fatal warning."
        );
    }

    #[test]
    fn warning_logger_emits_info_once_for_first_non_empty_batch() {
        let mut logger = WarningLogger::new();

        assert_eq!(
            logger.log_warnings(&LogWarningsOptions::new(Vec::new())),
            Vec::new()
        );

        let first = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "first".to_string(),
            }])
            .with_scope("provider", "model"),
        );
        assert_eq!(
            first,
            vec![
                WarningLogRecord::Info(FIRST_WARNING_INFO_MESSAGE.to_string()),
                WarningLogRecord::Warning {
                    message: "AI SDK Warning (provider / model): first".to_string(),
                    kind: WarningLogKind::Warning,
                },
            ]
        );

        let second = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "second".to_string(),
            }])
            .with_scope("provider", "model"),
        );
        assert_eq!(
            second,
            vec![WarningLogRecord::Warning {
                message: "AI SDK Warning (provider / model): second".to_string(),
                kind: WarningLogKind::Warning,
            }]
        );
    }

    #[test]
    fn warning_logger_can_be_disabled_and_reset() {
        let mut logger = WarningLogger::disabled();
        let options = LogWarningsOptions::new(vec![Warning::Deprecated {
            setting: "oldSetting".to_string(),
            message: "Use newSetting.".to_string(),
        }])
        .with_scope("provider", "model");

        assert_eq!(logger.log_warnings(&options), Vec::new());

        logger.set_enabled(true);
        assert_eq!(
            logger.log_warnings(&options),
            vec![
                WarningLogRecord::Info(FIRST_WARNING_INFO_MESSAGE.to_string()),
                WarningLogRecord::Warning {
                    message:
                        "AI SDK Warning (provider / model): Deprecated: \"oldSetting\". Use newSetting."
                            .to_string(),
                    kind: WarningLogKind::DeprecationWarning,
                },
            ]
        );

        logger.reset();
        assert!(matches!(
            logger.log_warnings(&options).first(),
            Some(WarningLogRecord::Info(message)) if message == FIRST_WARNING_INFO_MESSAGE
        ));
    }

    #[test]
    fn process_wide_log_warnings_matches_upstream_first_call_state() {
        let mut logger = WARNING_LOGGER
            .lock()
            .expect("warning logger mutex is not poisoned");
        logger.reset();
        logger.set_enabled(true);

        let first = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "first".to_string(),
            }])
            .with_scope("provider", "model"),
        );
        let second = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "second".to_string(),
            }])
            .with_scope("provider", "model"),
        );

        assert!(matches!(first.first(), Some(WarningLogRecord::Info(_))));
        assert!(!matches!(second.first(), Some(WarningLogRecord::Info(_))));

        logger.set_enabled(false);
        assert_eq!(
            logger.log_warnings(&LogWarningsOptions::new(vec![Warning::Other {
                message: "suppressed".to_string(),
            }])),
            Vec::new()
        );

        logger.set_enabled(true);
        logger.reset();
    }

    #[test]
    fn custom_logger_receives_original_options_without_default_records() {
        let options = LogWarningsOptions::new(vec![Warning::Unsupported {
            feature: "voice".to_string(),
            details: Some("Voice not supported".to_string()),
        }])
        .with_scope("provider", "model");
        let mut captured = None;

        assert!(log_warnings_with_custom_logger(&options, |received| {
            captured = Some(received.clone());
        }));
        assert_eq!(captured, Some(options));

        assert!(!log_warnings_with_custom_logger(
            &LogWarningsOptions::new(Vec::new()),
            |_| panic!("empty warnings should not call the custom logger"),
        ));
    }
}
