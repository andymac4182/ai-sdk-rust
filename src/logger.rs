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

    // ---------------------------------------------------------------------
    // Upstream parity: packages/ai/src/logger/log-warnings.test.ts
    // ---------------------------------------------------------------------

    /// packages-ai-1505: when disabled, a single warning produces no records.
    #[test]
    fn disabled_logger_does_not_log_single_warning() {
        let mut logger = WarningLogger::disabled();
        let options = LogWarningsOptions::new(vec![Warning::Other {
            message: "Test warning".to_string(),
        }])
        .with_scope("providerX", "modelY");

        assert_eq!(logger.log_warnings(&options), Vec::new());
    }

    /// packages-ai-1506: when disabled, multiple warnings produce no records.
    #[test]
    fn disabled_logger_does_not_log_multiple_warnings() {
        let mut logger = WarningLogger::disabled();
        let options = LogWarningsOptions::new(vec![
            Warning::Other {
                message: "Test warning 1".to_string(),
            },
            Warning::Other {
                message: "Test warning 2".to_string(),
            },
        ])
        .with_scope("provider", "model");

        assert_eq!(logger.log_warnings(&options), Vec::new());
    }

    /// packages-ai-1507: when disabled, an empty batch does not consume the
    /// first-call state, and a following non-empty batch still logs nothing.
    #[test]
    fn disabled_logger_does_not_count_empty_arrays_as_first_call() {
        let mut logger = WarningLogger::disabled();

        assert_eq!(
            logger.log_warnings(&LogWarningsOptions::new(Vec::new()).with_scope("prov", "mod")),
            Vec::new()
        );

        assert_eq!(
            logger.log_warnings(
                &LogWarningsOptions::new(vec![Warning::Other {
                    message: "foo".to_string(),
                }])
                .with_scope("p1", "m1")
            ),
            Vec::new()
        );
    }

    /// packages-ai-1508: the custom logger receives the original options once.
    #[test]
    fn custom_logger_called_with_warning_options() {
        let options = LogWarningsOptions::new(vec![Warning::Other {
            message: "Test warning".to_string(),
        }])
        .with_scope("pp", "mm");
        let mut call_count = 0usize;
        let mut captured = None;

        let invoked = log_warnings_with_custom_logger(&options, |received| {
            call_count += 1;
            captured = Some(received.clone());
        });

        assert!(invoked);
        assert_eq!(call_count, 1);
        assert_eq!(captured, Some(options));
    }

    /// packages-ai-1509: the custom logger receives multiple warnings verbatim.
    #[test]
    fn custom_logger_called_with_multiple_warnings() {
        let options = LogWarningsOptions::new(vec![
            Warning::Unsupported {
                feature: "temperature".to_string(),
                details: Some("Temperature not supported".to_string()),
            },
            Warning::Other {
                message: "Another warning".to_string(),
            },
        ])
        .with_scope("provider", "model");
        let mut call_count = 0usize;
        let mut captured = None;

        let invoked = log_warnings_with_custom_logger(&options, |received| {
            call_count += 1;
            captured = Some(received.clone());
        });

        assert!(invoked);
        assert_eq!(call_count, 1);
        assert_eq!(captured, Some(options));
    }

    /// packages-ai-1510: the custom logger is not called for an empty batch.
    #[test]
    fn custom_logger_not_called_with_empty_warnings_array() {
        let mut call_count = 0usize;

        let invoked = log_warnings_with_custom_logger(
            &LogWarningsOptions::new(Vec::new()).with_scope("x", "y"),
            |_| {
                call_count += 1;
            },
        );

        assert!(!invoked);
        assert_eq!(call_count, 0);
    }

    /// packages-ai-1511: the first batch emits the info note once, then a
    /// formatted warning record per warning.
    #[test]
    fn default_logger_shows_info_once_then_warning_per_warning() {
        let mut logger = WarningLogger::new();

        let records = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "Test warning message".to_string(),
            }])
            .with_scope("myProvider", "myModel"),
        );

        assert_eq!(
            records,
            vec![
                WarningLogRecord::Info(FIRST_WARNING_INFO_MESSAGE.to_string()),
                WarningLogRecord::Warning {
                    message: "AI SDK Warning (myProvider / myModel): Test warning message"
                        .to_string(),
                    kind: WarningLogKind::Warning,
                },
            ]
        );
    }

    /// packages-ai-1512: only the first non-empty call emits the info note.
    #[test]
    fn default_logger_shows_info_only_on_first_non_empty_call() {
        let mut logger = WarningLogger::new();

        let first = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "1".to_string(),
            }])
            .with_scope("a", "b"),
        );
        let second = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "2".to_string(),
            }])
            .with_scope("a", "b"),
        );

        assert_eq!(
            first,
            vec![
                WarningLogRecord::Info(FIRST_WARNING_INFO_MESSAGE.to_string()),
                WarningLogRecord::Warning {
                    message: "AI SDK Warning (a / b): 1".to_string(),
                    kind: WarningLogKind::Warning,
                },
            ]
        );
        assert_eq!(
            second,
            vec![WarningLogRecord::Warning {
                message: "AI SDK Warning (a / b): 2".to_string(),
                kind: WarningLogKind::Warning,
            }]
        );
    }

    /// packages-ai-1513: empty batches interleaved with non-empty ones never
    /// log, and the info note stays a one-time event across non-empty calls.
    #[test]
    fn default_logger_only_logs_for_non_empty_warnings() {
        let mut logger = WarningLogger::new();

        assert_eq!(
            logger.log_warnings(&LogWarningsOptions::new(Vec::new()).with_scope("err", "m")),
            Vec::new()
        );

        let first = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "t1".to_string(),
            }])
            .with_scope("prov", "mod"),
        );
        assert_eq!(
            first,
            vec![
                WarningLogRecord::Info(FIRST_WARNING_INFO_MESSAGE.to_string()),
                WarningLogRecord::Warning {
                    message: "AI SDK Warning (prov / mod): t1".to_string(),
                    kind: WarningLogKind::Warning,
                },
            ]
        );

        assert_eq!(
            logger.log_warnings(&LogWarningsOptions::new(Vec::new()).with_scope("prov", "mod")),
            Vec::new()
        );

        let second = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "t2".to_string(),
            }])
            .with_scope("prov", "mod"),
        );
        assert_eq!(
            second,
            vec![WarningLogRecord::Warning {
                message: "AI SDK Warning (prov / mod): t2".to_string(),
                kind: WarningLogKind::Warning,
            }]
        );
    }

    /// packages-ai-1514: every warning variant is formatted per upstream
    /// `formatWarning`, with the deprecated variant using a deprecation sink.
    #[test]
    fn default_logger_handles_various_warning_types_per_format_warning() {
        let mut logger = WarningLogger::new();

        let records = logger.log_warnings(
            &LogWarningsOptions::new(vec![
                Warning::Unsupported {
                    feature: "mediaType".to_string(),
                    details: Some("detail".to_string()),
                },
                Warning::Unsupported {
                    feature: "voice".to_string(),
                    details: Some("detail2".to_string()),
                },
                Warning::Deprecated {
                    setting: "providerOptions key 'old-key'".to_string(),
                    message: "Use 'oldKey' instead.".to_string(),
                },
                Warning::Other {
                    message: "other msg".to_string(),
                },
            ])
            .with_scope("zzz", "MMM"),
        );

        assert_eq!(
            records,
            vec![
                WarningLogRecord::Info(FIRST_WARNING_INFO_MESSAGE.to_string()),
                WarningLogRecord::Warning {
                    message:
                        "AI SDK Warning (zzz / MMM): The feature \"mediaType\" is not supported. detail"
                            .to_string(),
                    kind: WarningLogKind::Warning,
                },
                WarningLogRecord::Warning {
                    message:
                        "AI SDK Warning (zzz / MMM): The feature \"voice\" is not supported. detail2"
                            .to_string(),
                    kind: WarningLogKind::Warning,
                },
                WarningLogRecord::Warning {
                    message:
                        "AI SDK Warning (zzz / MMM): Deprecated: \"providerOptions key 'old-key'\". Use 'oldKey' instead."
                            .to_string(),
                    kind: WarningLogKind::DeprecationWarning,
                },
                WarningLogRecord::Warning {
                    message: "AI SDK Warning (zzz / MMM): other msg".to_string(),
                    kind: WarningLogKind::Warning,
                },
            ]
        );
    }

    /// packages-ai-1515: the warning is still emitted with literal "unknown
    /// provider" / "unknown model" scope strings.
    #[test]
    fn default_logger_includes_warning_with_unknown_provider_and_model() {
        let mut logger = WarningLogger::new();

        let records = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "messx".to_string(),
            }])
            .with_scope("unknown provider", "unknown model"),
        );

        assert_eq!(
            records,
            vec![
                WarningLogRecord::Info(FIRST_WARNING_INFO_MESSAGE.to_string()),
                WarningLogRecord::Warning {
                    message: "AI SDK Warning (unknown provider / unknown model): messx".to_string(),
                    kind: WarningLogKind::Warning,
                },
            ]
        );
    }

    /// packages-ai-1516: an explicitly unset logger uses the default behavior
    /// and emits a `Warning`-kind record (the upstream `process.emitWarning`).
    #[test]
    fn default_logger_undefined_uses_default_behavior_and_emits_warning() {
        let mut logger = WarningLogger::new();

        let records = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "Test warning with undefined logger".to_string(),
            }])
            .with_scope("p1", "m1"),
        );

        assert_eq!(
            records,
            vec![
                WarningLogRecord::Info(FIRST_WARNING_INFO_MESSAGE.to_string()),
                WarningLogRecord::Warning {
                    message: "AI SDK Warning (p1 / m1): Test warning with undefined logger"
                        .to_string(),
                    kind: WarningLogKind::Warning,
                },
            ]
        );
    }

    /// packages-ai-1517: an empty batch never emits the info note.
    #[test]
    fn default_logger_does_not_display_info_message_for_empty_warnings() {
        let mut logger = WarningLogger::new();

        assert_eq!(
            logger.log_warnings(&LogWarningsOptions::new(Vec::new()).with_scope("a", "b")),
            Vec::new()
        );
    }

    /// packages-ai-1518: leading empty batches never emit the info note; it
    /// fires once on the first real (non-empty) call, then never again.
    #[test]
    fn default_logger_displays_info_note_only_on_first_real_call() {
        let mut logger = WarningLogger::new();

        assert_eq!(
            logger.log_warnings(&LogWarningsOptions::new(Vec::new()).with_scope("a", "b")),
            Vec::new()
        );
        assert_eq!(
            logger.log_warnings(&LogWarningsOptions::new(Vec::new()).with_scope("a", "b")),
            Vec::new()
        );

        let first_real = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "foo".to_string(),
            }])
            .with_scope("abc", "bbb"),
        );
        assert_eq!(
            first_real,
            vec![
                WarningLogRecord::Info(FIRST_WARNING_INFO_MESSAGE.to_string()),
                WarningLogRecord::Warning {
                    message: "AI SDK Warning (abc / bbb): foo".to_string(),
                    kind: WarningLogKind::Warning,
                },
            ]
        );

        let second_real = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "bar".to_string(),
            }])
            .with_scope("abc", "bbb"),
        );
        assert_eq!(
            second_real,
            vec![WarningLogRecord::Warning {
                message: "AI SDK Warning (abc / bbb): bar".to_string(),
                kind: WarningLogKind::Warning,
            }]
        );
    }

    /// packages-ai-1519: the custom-logger path never produces the info note.
    #[test]
    fn custom_logger_does_not_display_information_note() {
        let options = LogWarningsOptions::new(vec![Warning::Other {
            message: "Message".to_string(),
        }])
        .with_scope("provV", "modZ");
        let mut call_count = 0usize;

        let invoked = log_warnings_with_custom_logger(&options, |_| {
            call_count += 1;
        });

        // The custom-logger entry point returns no deterministic records: only
        // the user callback runs, so there is no info note to emit.
        assert!(invoked);
        assert_eq!(call_count, 1);
    }

    /// packages-ai-1520: a disabled logger never produces the info note.
    #[test]
    fn disabled_logger_does_not_display_information_note() {
        let mut logger = WarningLogger::disabled();

        let records = logger.log_warnings(
            &LogWarningsOptions::new(vec![Warning::Other {
                message: "Suppressed".to_string(),
            }])
            .with_scope("notProv", "notModel"),
        );

        assert_eq!(records, Vec::new());
        assert!(
            !records
                .iter()
                .any(|record| matches!(record, WarningLogRecord::Info(_)))
        );
    }
}
