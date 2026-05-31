use crate::log_format::{LogMetadata, compose_log_line};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// Lightweight structured logger mirroring upstream `logger.ts`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Logger {
    namespace: String,
    parent_metadata: LogMetadata,
}

impl Logger {
    #[must_use]
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            parent_metadata: LogMetadata::new(),
        }
    }

    #[must_use]
    pub fn child(&self, metadata: LogMetadata) -> Self {
        let mut parent_metadata = self.parent_metadata.clone();
        parent_metadata.extend(metadata);
        Self {
            namespace: self.namespace.clone(),
            parent_metadata,
        }
    }

    #[must_use]
    pub fn for_run(
        &self,
        workflow_run_id: impl Into<String>,
        workflow_name: Option<&str>,
        extra: Option<LogMetadata>,
    ) -> Self {
        let mut metadata = LogMetadata::new();
        metadata.insert(
            "workflowRunId".to_string(),
            serde_json::Value::String(workflow_run_id.into()),
        );
        if let Some(workflow_name) = workflow_name {
            metadata.insert(
                "workflowName".to_string(),
                serde_json::Value::String(workflow_name.to_string()),
            );
        }
        if let Some(extra) = extra {
            metadata.extend(extra);
        }
        self.child(metadata)
    }

    #[must_use]
    pub fn error(&self, message: &str, metadata: Option<LogMetadata>) -> String {
        self.emit(LogLevel::Error, message, metadata)
            .expect("error logs always emit")
    }

    #[must_use]
    pub fn warn(&self, message: &str, metadata: Option<LogMetadata>) -> String {
        self.emit(LogLevel::Warn, message, metadata)
            .expect("warn logs always emit")
    }

    #[must_use]
    pub fn info(&self, message: &str, metadata: Option<LogMetadata>) -> Option<String> {
        self.emit(LogLevel::Info, message, metadata)
    }

    #[must_use]
    pub fn debug(&self, message: &str, metadata: Option<LogMetadata>) -> Option<String> {
        self.emit(LogLevel::Debug, message, metadata)
    }

    #[must_use]
    pub fn emit(
        &self,
        level: LogLevel,
        message: &str,
        metadata: Option<LogMetadata>,
    ) -> Option<String> {
        let merged = self.merge_metadata(metadata);
        if matches!(level, LogLevel::Error | LogLevel::Warn) {
            return Some(compose_log_line("[workflow-sdk]", message, merged.as_ref()));
        }

        let debug_namespace = format!("workflow:{}:{}", self.namespace, level.as_str());
        if matches_debug_namespace(&debug_namespace, std::env::var("DEBUG").ok().as_deref()) {
            Some(format!(
                "[{debug_namespace}] {message}{}",
                merged
                    .as_ref()
                    .filter(|metadata| !metadata.is_empty())
                    .map(|metadata| format!(" {}", serde_json::to_string(metadata).unwrap()))
                    .unwrap_or_default()
            ))
        } else {
            None
        }
    }

    fn merge_metadata(&self, metadata: Option<LogMetadata>) -> Option<LogMetadata> {
        let has_parent = !self.parent_metadata.is_empty();
        let has_call_site = metadata
            .as_ref()
            .is_some_and(|metadata| !metadata.is_empty());
        if !has_parent && !has_call_site {
            return None;
        }
        let mut merged = self.parent_metadata.clone();
        if let Some(metadata) = metadata {
            merged.extend(metadata);
        }
        Some(merged)
    }
}

#[must_use]
pub fn runtime_logger() -> Logger {
    Logger::new("runtime")
}

#[must_use]
pub fn step_logger() -> Logger {
    Logger::new("step")
}

#[must_use]
pub fn webhook_logger() -> Logger {
    Logger::new("webhook")
}

#[must_use]
pub fn events_logger() -> Logger {
    Logger::new("events")
}

#[must_use]
pub fn adapter_logger() -> Logger {
    Logger::new("adapter")
}

fn matches_debug_namespace(namespace: &str, pattern_list: Option<&str>) -> bool {
    let Some(pattern_list) = pattern_list else {
        return false;
    };

    let mut enabled = false;
    for raw_pattern in pattern_list.split(',') {
        let pattern = raw_pattern.trim();
        if pattern.is_empty() {
            continue;
        }
        let (is_negated, candidate) = pattern
            .strip_prefix('-')
            .map_or((false, pattern), |candidate| (true, candidate));
        if wildcard_match(candidate, namespace) {
            enabled = !is_negated;
        }
    }
    enabled
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let mut remaining = value;
    let mut parts = pattern.split('*').peekable();
    let starts_with_wildcard = pattern.starts_with('*');
    let ends_with_wildcard = pattern.ends_with('*');
    let mut last_part = "";

    if let Some(first) = parts.next() {
        last_part = first;
        if !first.is_empty() {
            if !starts_with_wildcard && !remaining.starts_with(first) {
                return false;
            }
            let Some(index) = remaining.find(first) else {
                return false;
            };
            remaining = &remaining[index + first.len()..];
        }
    }

    for part in parts {
        last_part = part;
        if part.is_empty() {
            continue;
        }
        let Some(index) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[index + part.len()..];
    }

    ends_with_wildcard || (last_part.is_empty() && starts_with_wildcard) || remaining.is_empty()
}
