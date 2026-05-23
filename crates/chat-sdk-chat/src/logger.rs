//! Logger types and implementations for `chat-sdk-chat`.
//!
//! 1:1 port of `packages/chat/src/logger.ts`. The upstream `Logger`
//! interface becomes a Rust `Logger` trait; `ConsoleLogger` becomes a
//! generic `ConsoleLogger<S: LogSink>` so tests can inject a captured-output
//! sink in place of `stderr`. The default `ConsoleSink` mirrors upstream
//! behavior (writes to the process stderr stream, prefixed with `[prefix]`).
//!
//! Adaptation note: upstream `...args: unknown[]` extra arguments map to
//! `&[&dyn std::fmt::Display]` here — the upstream tests that assert
//! `console.debug` was "called with extra args" are mapped to assertions
//! that the captured output includes the formatted extras.

use std::fmt::{self, Write as _};
use std::sync::{Arc, Mutex};

/// Upstream `LogLevel` discriminator, including the special `Silent` value
/// that suppresses every level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
    Silent,
}

impl LogLevel {
    fn rank(self) -> u8 {
        match self {
            Self::Debug => 0,
            Self::Info => 1,
            Self::Warn => 2,
            Self::Error => 3,
            Self::Silent => 4,
        }
    }
}

/// Upstream `Logger` interface. The four severity methods accept a message
/// plus an optional list of extras (matching upstream `...args: unknown[]`).
/// `child` returns a sub-logger whose prefix is `parent_prefix:child_prefix`.
pub trait Logger: Send + Sync {
    fn debug(&self, message: &str, extras: &[&dyn fmt::Display]);
    fn info(&self, message: &str, extras: &[&dyn fmt::Display]);
    fn warn(&self, message: &str, extras: &[&dyn fmt::Display]);
    fn error(&self, message: &str, extras: &[&dyn fmt::Display]);
    fn child(&self, prefix: &str) -> Box<dyn Logger>;
}

/// Where a [`ConsoleLogger`] emits log records. Mirrors upstream's implicit
/// `console.debug` / `console.info` / `console.warn` / `console.error`
/// dispatch by routing each severity to a distinct sink method, so tests
/// can capture per-level output the same way Vitest spies on each
/// `console.*` channel.
pub trait LogSink: Send + Sync {
    fn debug(&self, line: &str);
    fn info(&self, line: &str);
    fn warn(&self, line: &str);
    fn error(&self, line: &str);
}

/// Default sink used by [`ConsoleLogger::new`]. Writes each level to the
/// matching standard stream — debug/info to stdout, warn/error to stderr —
/// matching upstream's `console.*` semantics in Node.
#[derive(Debug, Default)]
pub struct ConsoleSink;

impl LogSink for ConsoleSink {
    fn debug(&self, line: &str) {
        println!("{line}");
    }
    fn info(&self, line: &str) {
        println!("{line}");
    }
    fn warn(&self, line: &str) {
        eprintln!("{line}");
    }
    fn error(&self, line: &str) {
        eprintln!("{line}");
    }
}

/// 1:1 port of upstream `class ConsoleLogger implements Logger`. Holds a
/// [`LogLevel`] threshold and a [`LogSink`]. Calls below the threshold are
/// dropped (level-filtering); calls at or above the threshold are formatted
/// `[prefix] message extras…` and routed to the matching sink method.
#[derive(Clone)]
pub struct ConsoleLogger {
    level: LogLevel,
    prefix: String,
    sink: Arc<dyn LogSink>,
}

impl fmt::Debug for ConsoleLogger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConsoleLogger")
            .field("level", &self.level)
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

impl ConsoleLogger {
    /// Construct a `ConsoleLogger` with default level (`Info`) and prefix
    /// (`"chat-sdk"`), matching upstream
    /// `new ConsoleLogger()` defaults.
    pub fn new() -> Self {
        Self::with_level(LogLevel::Info)
    }

    /// Construct a `ConsoleLogger` with the given level and the default
    /// `"chat-sdk"` prefix. Mirrors upstream `new ConsoleLogger(level)`.
    pub fn with_level(level: LogLevel) -> Self {
        Self::with_level_and_prefix(level, "chat-sdk")
    }

    /// Construct a `ConsoleLogger` with explicit level and prefix. Mirrors
    /// upstream `new ConsoleLogger(level, prefix)`.
    pub fn with_level_and_prefix(level: LogLevel, prefix: impl Into<String>) -> Self {
        Self {
            level,
            prefix: prefix.into(),
            sink: Arc::new(ConsoleSink),
        }
    }

    /// Test-and-extension constructor: route output to a custom sink.
    pub fn with_sink<S: LogSink + 'static>(
        level: LogLevel,
        prefix: impl Into<String>,
        sink: S,
    ) -> Self {
        Self {
            level,
            prefix: prefix.into(),
            sink: Arc::new(sink),
        }
    }

    fn should_log(&self, level: LogLevel) -> bool {
        level.rank() >= self.level.rank()
    }

    fn format_line(&self, message: &str, extras: &[&dyn fmt::Display]) -> String {
        let mut line = format!("[{}] {message}", self.prefix);
        for extra in extras {
            // Match upstream `console.debug("msg", extra, 42)` shape:
            // each extra is appended after a single space so captured output
            // can be asserted against the upstream call signature.
            let _ = write!(line, " {extra}");
        }
        line
    }
}

impl Default for ConsoleLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl Logger for ConsoleLogger {
    fn debug(&self, message: &str, extras: &[&dyn fmt::Display]) {
        if self.should_log(LogLevel::Debug) {
            self.sink.debug(&self.format_line(message, extras));
        }
    }
    fn info(&self, message: &str, extras: &[&dyn fmt::Display]) {
        if self.should_log(LogLevel::Info) {
            self.sink.info(&self.format_line(message, extras));
        }
    }
    fn warn(&self, message: &str, extras: &[&dyn fmt::Display]) {
        if self.should_log(LogLevel::Warn) {
            self.sink.warn(&self.format_line(message, extras));
        }
    }
    fn error(&self, message: &str, extras: &[&dyn fmt::Display]) {
        if self.should_log(LogLevel::Error) {
            self.sink.error(&self.format_line(message, extras));
        }
    }
    fn child(&self, prefix: &str) -> Box<dyn Logger> {
        Box::new(Self {
            level: self.level,
            prefix: format!("{}:{prefix}", self.prefix),
            sink: Arc::clone(&self.sink),
        })
    }
}

/// In-memory [`LogSink`] for tests. Records every call separated by level.
/// Mirrors upstream Vitest `vi.spyOn(console, "<level>")` capture.
#[derive(Debug, Default, Clone)]
pub struct MemorySink {
    inner: Arc<Mutex<MemorySinkState>>,
}

#[derive(Debug, Default)]
struct MemorySinkState {
    debug: Vec<String>,
    info: Vec<String>,
    warn: Vec<String>,
    error: Vec<String>,
}

impl MemorySink {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn debug_calls(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("memory sink poisoned")
            .debug
            .clone()
    }
    pub fn info_calls(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("memory sink poisoned")
            .info
            .clone()
    }
    pub fn warn_calls(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("memory sink poisoned")
            .warn
            .clone()
    }
    pub fn error_calls(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("memory sink poisoned")
            .error
            .clone()
    }
}

impl LogSink for MemorySink {
    fn debug(&self, line: &str) {
        self.inner
            .lock()
            .expect("memory sink poisoned")
            .debug
            .push(line.to_string());
    }
    fn info(&self, line: &str) {
        self.inner
            .lock()
            .expect("memory sink poisoned")
            .info
            .push(line.to_string());
    }
    fn warn(&self, line: &str) {
        self.inner
            .lock()
            .expect("memory sink poisoned")
            .warn
            .push(line.to_string());
    }
    fn error(&self, line: &str) {
        self.inner
            .lock()
            .expect("memory sink poisoned")
            .error
            .push(line.to_string());
    }
}

#[cfg(test)]
mod tests {
    //! 1:1 port of `packages/chat/src/logger.test.ts` from upstream
    //! `vercel/chat` @ `aba6aa94fe5a2ed909ec4daa7db0e21887507fa4`.
    //!
    //! Upstream uses Vitest `vi.spyOn(console, "<level>")` to capture
    //! per-level output. The Rust port injects a [`MemorySink`] into
    //! [`ConsoleLogger`] and asserts against its captured calls. Each test
    //! name mirrors the original `it(...)` description.

    use super::{ConsoleLogger, LogLevel, Logger, MemorySink};

    fn make(level: LogLevel, prefix: &str) -> (ConsoleLogger, MemorySink) {
        let sink = MemorySink::new();
        let logger = ConsoleLogger::with_sink(level, prefix, sink.clone());
        (logger, sink)
    }

    // describe("ConsoleLogger > default level (info)")

    #[test]
    fn default_level_info_should_not_log_debug_messages() {
        let (logger, sink) = make(LogLevel::Info, "chat-sdk");
        logger.debug("hidden", &[]);
        assert!(sink.debug_calls().is_empty());
    }

    #[test]
    fn default_level_info_should_log_info_messages() {
        let (logger, sink) = make(LogLevel::Info, "chat-sdk");
        logger.info("visible", &[]);
        assert_eq!(sink.info_calls(), vec!["[chat-sdk] visible".to_string()]);
    }

    #[test]
    fn default_level_info_should_log_warn_messages() {
        let (logger, sink) = make(LogLevel::Info, "chat-sdk");
        logger.warn("warning", &[]);
        assert_eq!(sink.warn_calls(), vec!["[chat-sdk] warning".to_string()]);
    }

    #[test]
    fn default_level_info_should_log_error_messages() {
        let (logger, sink) = make(LogLevel::Info, "chat-sdk");
        logger.error("failure", &[]);
        assert_eq!(sink.error_calls(), vec!["[chat-sdk] failure".to_string()]);
    }

    // describe("ConsoleLogger > debug level")

    #[test]
    fn debug_level_should_log_all_levels_including_debug() {
        let (logger, sink) = make(LogLevel::Debug, "chat-sdk");
        logger.debug("dbg", &[]);
        logger.info("inf", &[]);
        logger.warn("wrn", &[]);
        logger.error("err", &[]);
        assert!(!sink.debug_calls().is_empty());
        assert!(!sink.info_calls().is_empty());
        assert!(!sink.warn_calls().is_empty());
        assert!(!sink.error_calls().is_empty());
    }

    // describe("ConsoleLogger > warn level")

    #[test]
    fn warn_level_should_only_log_warn_and_error() {
        let (logger, sink) = make(LogLevel::Warn, "chat-sdk");
        logger.debug("hidden", &[]);
        logger.info("hidden", &[]);
        logger.warn("visible", &[]);
        logger.error("visible", &[]);
        assert!(sink.debug_calls().is_empty());
        assert!(sink.info_calls().is_empty());
        assert!(!sink.warn_calls().is_empty());
        assert!(!sink.error_calls().is_empty());
    }

    // describe("ConsoleLogger > error level")

    #[test]
    fn error_level_should_only_log_errors() {
        let (logger, sink) = make(LogLevel::Error, "chat-sdk");
        logger.debug("hidden", &[]);
        logger.info("hidden", &[]);
        logger.warn("hidden", &[]);
        logger.error("visible", &[]);
        assert!(sink.debug_calls().is_empty());
        assert!(sink.info_calls().is_empty());
        assert!(sink.warn_calls().is_empty());
        assert!(!sink.error_calls().is_empty());
    }

    // describe("ConsoleLogger > silent level")

    #[test]
    fn silent_level_should_not_log_anything() {
        let (logger, sink) = make(LogLevel::Silent, "chat-sdk");
        logger.debug("hidden", &[]);
        logger.info("hidden", &[]);
        logger.warn("hidden", &[]);
        logger.error("hidden", &[]);
        assert!(sink.debug_calls().is_empty());
        assert!(sink.info_calls().is_empty());
        assert!(sink.warn_calls().is_empty());
        assert!(sink.error_calls().is_empty());
    }

    // describe("ConsoleLogger > prefix formatting")

    #[test]
    fn prefix_formatting_should_use_default_prefix() {
        let (logger, sink) = make(LogLevel::Info, "chat-sdk");
        logger.info("test", &[]);
        assert_eq!(sink.info_calls(), vec!["[chat-sdk] test".to_string()]);
    }

    #[test]
    fn prefix_formatting_should_use_custom_prefix() {
        let (logger, sink) = make(LogLevel::Info, "my-app");
        logger.info("test", &[]);
        assert_eq!(sink.info_calls(), vec!["[my-app] test".to_string()]);
    }

    // describe("ConsoleLogger > extra args passthrough")

    #[test]
    fn extra_args_passthrough_should_forward_extra_arguments() {
        // Adaptation: upstream Vitest asserts
        // `expect(debugSpy).toHaveBeenCalledWith("[chat-sdk] msg", extra, 42)`.
        // Rust has no variadic-arg console; the port joins the extras into
        // the formatted line and asserts the captured text contains both.
        let (logger, sink) = make(LogLevel::Debug, "chat-sdk");
        let extra_obj = "{ key: value }";
        logger.debug("msg", &[&extra_obj, &42]);
        assert_eq!(
            sink.debug_calls(),
            vec!["[chat-sdk] msg { key: value } 42".to_string()]
        );
    }

    // describe("ConsoleLogger > child logger")

    #[test]
    fn child_logger_should_create_child_with_combined_prefix() {
        let (logger, sink) = make(LogLevel::Info, "parent");
        let child = logger.child("child");
        child.info("test", &[]);
        assert_eq!(sink.info_calls(), vec!["[parent:child] test".to_string()]);
    }

    #[test]
    fn child_logger_should_inherit_log_level() {
        let (logger, sink) = make(LogLevel::Warn, "parent");
        let child = logger.child("child");
        child.info("hidden", &[]);
        child.warn("visible", &[]);
        assert!(sink.info_calls().is_empty());
        assert_eq!(
            sink.warn_calls(),
            vec!["[parent:child] visible".to_string()]
        );
    }
}
