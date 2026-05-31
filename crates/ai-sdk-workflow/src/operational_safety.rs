//! Operational safety helpers for durable remote-agent workflows.
//!
//! The types in this module are intentionally runtime-agnostic. Slack, storage,
//! and sandbox adapters can share the same redaction, accounting, approval, and
//! telemetry decisions while keeping execution-specific code in their own
//! crates.

use std::collections::BTreeMap;

use ai_sdk_provider::json::JsonValue;
use ai_sdk_provider::{
    InputTokenUsage, LanguageModelAbortSignal, LanguageModelUsage, OutputTokenUsage,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Placeholder used whenever sensitive values are removed from prompts, logs,
/// tool output, or Slack-visible diagnostics.
pub const REDACTED_VALUE: &str = "[REDACTED]";

/// One model-usage contribution from a run, step, subagent, or tool task.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageAccountingEvent {
    /// Model identity that produced the usage, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,

    /// Tool call that produced nested usage, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// Provider usage payload normalized into AI SDK token buckets.
    pub usage: LanguageModelUsage,

    /// Estimated cost in micros of the billing unit, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_micros: Option<u64>,
}

impl UsageAccountingEvent {
    /// Creates a usage event.
    pub fn new(usage: LanguageModelUsage) -> Self {
        Self {
            model_id: None,
            tool_call_id: None,
            usage,
            estimated_cost_micros: None,
        }
    }

    /// Sets the producing model id.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = Some(model_id.into());
        self
    }

    /// Sets the producing tool call id.
    pub fn with_tool_call_id(mut self, tool_call_id: impl Into<String>) -> Self {
        self.tool_call_id = Some(tool_call_id.into());
        self
    }

    /// Sets the estimated cost in micros.
    pub fn with_estimated_cost_micros(mut self, estimated_cost_micros: u64) -> Self {
        self.estimated_cost_micros = Some(estimated_cost_micros);
        self
    }
}

/// Aggregated model usage for an operational run.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageAggregation {
    /// Total normalized usage.
    pub total_usage: LanguageModelUsage,

    /// Total estimated cost in micros, when any event supplied cost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_micros: Option<u64>,

    /// Source usage events retained for operator drill-down.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<UsageAccountingEvent>,
}

impl UsageAggregation {
    /// Adds one usage event to the aggregate.
    pub fn add_event(&mut self, event: UsageAccountingEvent) {
        self.total_usage = add_language_model_usage(&self.total_usage, &event.usage);
        self.estimated_cost_micros =
            add_optional_u64(self.estimated_cost_micros, event.estimated_cost_micros);
        self.events.push(event);
    }

    /// Returns input plus output token totals when either side is known.
    pub fn total_tokens(&self) -> Option<u64> {
        add_optional_u64(
            self.total_usage.input_tokens.total,
            self.total_usage.output_tokens.total,
        )
    }
}

/// Adds two AI SDK language-model usage values.
pub fn add_language_model_usage(
    usage1: &LanguageModelUsage,
    usage2: &LanguageModelUsage,
) -> LanguageModelUsage {
    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: add_optional_u64(usage1.input_tokens.total, usage2.input_tokens.total),
            no_cache: add_optional_u64(usage1.input_tokens.no_cache, usage2.input_tokens.no_cache),
            cache_read: add_optional_u64(
                usage1.input_tokens.cache_read,
                usage2.input_tokens.cache_read,
            ),
            cache_write: add_optional_u64(
                usage1.input_tokens.cache_write,
                usage2.input_tokens.cache_write,
            ),
        },
        output_tokens: OutputTokenUsage {
            total: add_optional_u64(usage1.output_tokens.total, usage2.output_tokens.total),
            text: add_optional_u64(usage1.output_tokens.text, usage2.output_tokens.text),
            reasoning: add_optional_u64(
                usage1.output_tokens.reasoning,
                usage2.output_tokens.reasoning,
            ),
        },
        raw: None,
    }
}

fn add_optional_u64(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (None, None) => None,
        (left, right) => Some(left.unwrap_or(0).saturating_add(right.unwrap_or(0))),
    }
}

/// Redaction settings for logs and user-visible diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedactionPolicy {
    /// Replacement string written in place of a secret.
    pub replacement: String,

    /// Case-insensitive object-key fragments that should redact whole values.
    pub sensitive_key_fragments: Vec<String>,

    /// Token prefixes that indicate common provider or platform credentials.
    pub secret_token_prefixes: Vec<String>,
}

impl Default for RedactionPolicy {
    fn default() -> Self {
        Self {
            replacement: REDACTED_VALUE.to_string(),
            sensitive_key_fragments: [
                "authorization",
                "cookie",
                "secret",
                "token",
                "password",
                "api_key",
                "apikey",
                "access_key",
                "private_key",
                "client_secret",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            secret_token_prefixes: [
                "sk-",
                "xoxb-",
                "xoxp-",
                "ghp_",
                "github_pat_",
                "AKIA",
                "AIza",
                "hf_",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        }
    }
}

/// Redacted value plus the number of substitutions performed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Redacted<T> {
    /// Value after redaction.
    pub value: T,

    /// Number of redactions applied.
    pub redactions: usize,
}

impl<T> Redacted<T> {
    fn new(value: T, redactions: usize) -> Self {
        Self { value, redactions }
    }
}

/// Redacts secrets from a string.
pub fn redact_text(text: &str, policy: &RedactionPolicy) -> Redacted<String> {
    let (header_redacted, header_count) = redact_sensitive_header_lines(text, policy);
    let (assignment_redacted, assignment_count) =
        redact_sensitive_assignments(&header_redacted, policy);
    let (token_redacted, token_count) = redact_known_secret_tokens(&assignment_redacted, policy);

    Redacted::new(
        token_redacted,
        header_count
            .saturating_add(assignment_count)
            .saturating_add(token_count),
    )
}

/// Redacts secrets from a JSON value.
pub fn redact_json_value(value: &JsonValue, policy: &RedactionPolicy) -> Redacted<JsonValue> {
    match value {
        JsonValue::Object(object) => {
            let mut redactions = 0usize;
            let mut output = serde_json::Map::new();

            for (key, value) in object {
                if is_sensitive_key(key, policy) {
                    redactions = redactions.saturating_add(1);
                    output.insert(key.clone(), JsonValue::String(policy.replacement.clone()));
                    continue;
                }

                let redacted = redact_json_value(value, policy);
                redactions = redactions.saturating_add(redacted.redactions);
                output.insert(key.clone(), redacted.value);
            }

            Redacted::new(JsonValue::Object(output), redactions)
        }
        JsonValue::Array(values) => {
            let mut redactions = 0usize;
            let values = values
                .iter()
                .map(|value| {
                    let redacted = redact_json_value(value, policy);
                    redactions = redactions.saturating_add(redacted.redactions);
                    redacted.value
                })
                .collect();
            Redacted::new(JsonValue::Array(values), redactions)
        }
        JsonValue::String(value) => {
            let redacted = redact_text(value, policy);
            Redacted::new(JsonValue::String(redacted.value), redacted.redactions)
        }
        value => Redacted::new(value.clone(), 0),
    }
}

fn redact_sensitive_header_lines(text: &str, policy: &RedactionPolicy) -> (String, usize) {
    let mut redactions = 0usize;
    let mut output = String::new();

    for line in text.split_inclusive('\n') {
        let newline = line.ends_with('\n');
        let line_without_newline = if newline {
            &line[..line.len().saturating_sub(1)]
        } else {
            line
        };
        let trimmed = line_without_newline.trim_start().to_ascii_lowercase();

        if (trimmed.starts_with("authorization:") || trimmed.starts_with("cookie:"))
            && let Some(separator) = line_without_newline.find(':')
        {
            output.push_str(&line_without_newline[..=separator]);
            output.push(' ');
            output.push_str(&policy.replacement);
            if newline {
                output.push('\n');
            }
            redactions = redactions.saturating_add(1);
            continue;
        }

        output.push_str(line);
    }

    (output, redactions)
}

fn redact_sensitive_assignments(text: &str, policy: &RedactionPolicy) -> (String, usize) {
    let mut ranges = Vec::new();
    let lower = text.to_ascii_lowercase();

    for key in &policy.sensitive_key_fragments {
        let key = key.to_ascii_lowercase();
        let mut search_start = 0usize;

        while let Some(relative_key_start) = lower[search_start..].find(&key) {
            let key_start = search_start + relative_key_start;
            let key_end = key_start + key.len();
            let Some(separator) = find_assignment_separator(&lower, key_end, 24) else {
                search_start = key_end;
                continue;
            };
            let mut value_start = separator + 1;
            while value_start < text.len()
                && matches!(text.as_bytes()[value_start], b' ' | b'\t' | b'\'' | b'"')
            {
                value_start += 1;
            }
            let value_end = find_assignment_value_end(text, value_start);
            if value_start < value_end && text[value_start..value_end] != policy.replacement {
                ranges.push((value_start, value_end));
            }
            search_start = value_end.max(key_end);
        }
    }

    apply_redaction_ranges(text, ranges, &policy.replacement)
}

fn find_assignment_separator(text: &str, start: usize, max_distance: usize) -> Option<usize> {
    let end = start.saturating_add(max_distance).min(text.len());
    for (index, byte) in text.as_bytes()[start..end].iter().enumerate() {
        match byte {
            b'=' | b':' => return Some(start + index),
            b'\n' | b'\r' => return None,
            _ => {}
        }
    }
    None
}

fn find_assignment_value_end(text: &str, start: usize) -> usize {
    let mut end = start;
    for (relative_index, character) in text[start..].char_indices() {
        if character.is_whitespace() || matches!(character, ',' | ';' | '\'' | '"') {
            return start + relative_index;
        }
        end = start + relative_index + character.len_utf8();
    }
    end
}

fn redact_known_secret_tokens(text: &str, policy: &RedactionPolicy) -> (String, usize) {
    let mut output = String::new();
    let mut index = 0usize;
    let mut redactions = 0usize;

    while index < text.len() {
        let rest = &text[index..];
        if let Some(prefix) = policy
            .secret_token_prefixes
            .iter()
            .find(|prefix| rest.starts_with(prefix.as_str()))
        {
            let token_len = secret_token_len(rest);
            if token_len > prefix.len() {
                output.push_str(&policy.replacement);
                redactions = redactions.saturating_add(1);
                index += token_len;
                continue;
            }
        }

        let character = rest
            .chars()
            .next()
            .expect("index always points to a character boundary");
        output.push(character);
        index += character.len_utf8();
    }

    (output, redactions)
}

fn secret_token_len(value: &str) -> usize {
    let mut end = 0usize;
    for (index, character) in value.char_indices() {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '/') {
            end = index + character.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn apply_redaction_ranges(
    text: &str,
    mut ranges: Vec<(usize, usize)>,
    replacement: &str,
) -> (String, usize) {
    if ranges.is_empty() {
        return (text.to_string(), 0);
    }

    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some((_, last_end)) = merged.last_mut()
            && start <= *last_end
        {
            *last_end = (*last_end).max(end);
            continue;
        }
        merged.push((start, end));
    }

    let mut output = String::new();
    let mut cursor = 0usize;
    for (start, end) in &merged {
        output.push_str(&text[cursor..*start]);
        output.push_str(replacement);
        cursor = *end;
    }
    output.push_str(&text[cursor..]);

    (output, merged.len())
}

fn is_sensitive_key(key: &str, policy: &RedactionPolicy) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    policy
        .sensitive_key_fragments
        .iter()
        .any(|fragment| normalized.contains(fragment))
}

/// Scope used by rate-limit decisions.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RateLimitScope {
    /// Workspace-wide limit.
    Workspace,
    /// User-specific limit.
    User,
    /// Slack channel-specific limit.
    Channel,
    /// Slack thread-specific limit.
    Thread,
    /// Model-specific limit.
    Model,
}

/// One rate-limit budget check.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitCheck {
    /// Limit scope.
    pub scope: RateLimitScope,

    /// Stable workspace/user/channel/thread/model identifier.
    pub subject: String,

    /// Maximum allowed units in the current window.
    pub limit: u64,

    /// Units already consumed in the current window.
    pub used: u64,

    /// Units requested by the candidate operation.
    pub requested: u64,

    /// Time until the current window resets.
    pub window_remaining_ms: u64,
}

impl RateLimitCheck {
    /// Creates a rate-limit check for a subject.
    pub fn new(
        scope: RateLimitScope,
        subject: impl Into<String>,
        limit: u64,
        used: u64,
        requested: u64,
        window_remaining_ms: u64,
    ) -> Self {
        Self {
            scope,
            subject: subject.into(),
            limit,
            used,
            requested,
            window_remaining_ms,
        }
    }
}

/// Outcome of a rate-limit decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitDecision {
    /// Whether the operation may proceed.
    pub allowed: bool,

    /// Limit scope that was checked.
    pub scope: RateLimitScope,

    /// Subject that was checked.
    pub subject: String,

    /// Units remaining if allowed; zero when denied.
    pub remaining: u64,

    /// Retry delay when denied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,

    /// Human-readable reason suitable for logs or redacted Slack diagnostics.
    pub reason: String,
}

/// Decides whether a candidate operation fits inside a rate-limit budget.
pub fn decide_rate_limit(check: RateLimitCheck) -> RateLimitDecision {
    let projected = check.used.saturating_add(check.requested);
    if projected <= check.limit {
        return RateLimitDecision {
            allowed: true,
            scope: check.scope,
            subject: check.subject,
            remaining: check.limit.saturating_sub(projected),
            retry_after_ms: None,
            reason: "within limit".to_string(),
        };
    }

    RateLimitDecision {
        allowed: false,
        scope: check.scope,
        subject: check.subject,
        remaining: 0,
        retry_after_ms: Some(check.window_remaining_ms),
        reason: "rate limit exceeded".to_string(),
    }
}

/// Approval action for a tool call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalAction {
    /// No approval is required.
    Allow,
    /// A human or policy gate must approve before execution.
    RequireApproval,
}

/// Reason a tool call requires approval.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalReason {
    /// Shell command matched a high-risk pattern.
    DangerousShellCommand,
    /// Command or input references dotenv files or credential material.
    SensitiveFileRead,
    /// Network access is requested.
    NetworkAccess,
    /// URL targets a private, loopback, link-local, or local host.
    PrivateNetworkAccess,
    /// GitHub mutation is requested.
    GithubMutation,
    /// Destructive filesystem mutation is requested.
    DestructiveFilesystem,
}

/// Classification result for a tool call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalClassification {
    /// Decision for the call.
    pub action: ApprovalAction,

    /// Stable machine-readable reasons.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<ApprovalReason>,

    /// Redacted diagnostic text for logs or Slack.
    pub diagnostic: String,
}

impl ApprovalClassification {
    /// Returns whether this classification requires approval.
    pub fn requires_approval(&self) -> bool {
        self.action == ApprovalAction::RequireApproval
    }
}

/// Classifies a tool call according to the Open Agents bash/fetch safety model.
pub fn classify_tool_approval(tool_name: &str, input: &JsonValue) -> ApprovalClassification {
    let normalized_tool = normalize_tool_name(tool_name);
    let mut reasons = Vec::new();

    if matches!(normalized_tool.as_str(), "bash" | "shell" | "tool-bash") {
        if let Some(command) = extract_string_field(input, "command").or_else(|| input.as_str()) {
            classify_shell_command(command, &mut reasons);
        }
    }

    if normalized_tool.contains("fetch") || normalized_tool.contains("web") {
        push_reason(&mut reasons, ApprovalReason::NetworkAccess);
        if let Some(url) = extract_string_field(input, "url")
            && is_private_or_local_url(url)
        {
            push_reason(&mut reasons, ApprovalReason::PrivateNetworkAccess);
        }
    }

    if normalized_tool.contains("github") {
        if github_input_is_mutation(input) {
            push_reason(&mut reasons, ApprovalReason::GithubMutation);
        }
    } else if let Some(command) = extract_string_field(input, "command").or_else(|| input.as_str())
    {
        if command_contains_github_mutation(command) {
            push_reason(&mut reasons, ApprovalReason::GithubMutation);
        }
    }

    reasons.sort();
    reasons.dedup();

    let action = if reasons.is_empty() {
        ApprovalAction::Allow
    } else {
        ApprovalAction::RequireApproval
    };
    let diagnostic = if reasons.is_empty() {
        "tool call allowed".to_string()
    } else {
        format!(
            "approval required: {}",
            reasons
                .iter()
                .map(approval_reason_name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    ApprovalClassification {
        action,
        reasons,
        diagnostic,
    }
}

fn normalize_tool_name(tool_name: &str) -> String {
    tool_name
        .trim()
        .trim_start_matches("tool-")
        .to_ascii_lowercase()
}

fn extract_string_field<'a>(value: &'a JsonValue, field: &str) -> Option<&'a str> {
    value.as_object()?.get(field)?.as_str()
}

fn classify_shell_command(command: &str, reasons: &mut Vec<ApprovalReason>) {
    let normalized = command.to_ascii_lowercase();
    let compact = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

    if compact.contains("rm -rf")
        || compact.contains("rm -fr")
        || compact.contains("find ") && compact.contains(" -delete")
        || compact.contains("find ") && compact.contains(" -exec rm")
        || compact.contains("mkfs")
        || compact.contains("shred")
        || compact.contains(" dd ")
        || compact.starts_with("dd ")
        || compact.contains(":(){ :|:")
    {
        push_reason(reasons, ApprovalReason::DangerousShellCommand);
        push_reason(reasons, ApprovalReason::DestructiveFilesystem);
    }

    if compact.contains("curl ")
        || compact.starts_with("curl ")
        || compact.contains("wget ")
        || compact.starts_with("wget ")
    {
        push_reason(reasons, ApprovalReason::NetworkAccess);
    }

    if references_sensitive_file(&compact) {
        push_reason(reasons, ApprovalReason::SensitiveFileRead);
    }
}

fn references_sensitive_file(command: &str) -> bool {
    [
        ".env",
        "/.env",
        "aws/credentials",
        ".ssh",
        "id_rsa",
        "id_ed25519",
        "proc/self/environ",
    ]
    .iter()
    .any(|pattern| command.contains(pattern))
}

fn github_input_is_mutation(input: &JsonValue) -> bool {
    if let Some(method) = extract_string_field(input, "method") {
        let method = method.to_ascii_uppercase();
        if matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE") {
            return true;
        }
    }

    if let Some(query) = extract_string_field(input, "query") {
        let normalized = query.to_ascii_lowercase();
        if normalized.contains("mutation") {
            return true;
        }
    }

    input
        .as_object()
        .and_then(|object| object.get("mutation"))
        .and_then(JsonValue::as_bool)
        .unwrap_or(false)
}

fn command_contains_github_mutation(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    command.contains("gh pr merge")
        || command.contains("gh release create")
        || command.contains("gh api --method post")
        || command.contains("gh api --method put")
        || command.contains("gh api --method patch")
        || command.contains("gh api --method delete")
        || command.contains("gh api -x post")
        || command.contains("git push")
}

fn is_private_or_local_url(url: &str) -> bool {
    let Some(host) = extract_url_host(url) else {
        return true;
    };
    let host = host
        .trim_matches('[')
        .trim_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase();

    if host == "localhost" || host == "::1" || host == "0:0:0:0:0:0:0:1" {
        return true;
    }

    if let Some(octets) = parse_ipv4_address(&host) {
        let [first, second, _, _] = octets;
        return first == 0
            || first == 10
            || first == 127
            || (first == 169 && second == 254)
            || (first == 172 && (16..=31).contains(&second))
            || (first == 192 && second == 168);
    }

    host.starts_with("fc") || host.starts_with("fd") || host.starts_with("fe80:")
}

fn extract_url_host(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("://")?;
    let authority = rest.split('/').next().unwrap_or(rest);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    if host_port.starts_with('[') {
        return host_port
            .strip_prefix('[')
            .and_then(|value| value.split(']').next());
    }
    Some(host_port.split(':').next().unwrap_or(host_port))
}

fn parse_ipv4_address(host: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut count = 0usize;
    for (index, part) in host.split('.').enumerate() {
        if index >= 4
            || part.is_empty()
            || !part.chars().all(|character| character.is_ascii_digit())
        {
            return None;
        }
        octets[index] = part.parse().ok()?;
        count += 1;
    }
    (count == 4).then_some(octets)
}

fn push_reason(reasons: &mut Vec<ApprovalReason>, reason: ApprovalReason) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

fn approval_reason_name(reason: &ApprovalReason) -> &'static str {
    match reason {
        ApprovalReason::DangerousShellCommand => "dangerousShellCommand",
        ApprovalReason::SensitiveFileRead => "sensitiveFileRead",
        ApprovalReason::NetworkAccess => "networkAccess",
        ApprovalReason::PrivateNetworkAccess => "privateNetworkAccess",
        ApprovalReason::GithubMutation => "githubMutation",
        ApprovalReason::DestructiveFilesystem => "destructiveFilesystem",
    }
}

/// Cancellation state derived from an abort signal.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum CancellationStatus {
    /// No cancellation was requested.
    Running,
    /// Cancellation was requested.
    Requested {
        /// Optional abort reason.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<JsonValue>,
    },
}

/// Returns true when the supplied abort signal has been cancelled.
pub fn is_cancellation_requested(signal: Option<&LanguageModelAbortSignal>) -> bool {
    signal.is_some_and(LanguageModelAbortSignal::is_aborted)
}

/// Returns cancellation status for an optional abort signal.
pub fn cancellation_status(signal: Option<&LanguageModelAbortSignal>) -> CancellationStatus {
    match signal {
        Some(signal) if signal.is_aborted() => CancellationStatus::Requested {
            reason: signal.reason(),
        },
        _ => CancellationStatus::Running,
    }
}

/// Shared metadata for run records.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEventMetadata {
    /// Workspace id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,

    /// User id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Channel id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,

    /// Thread id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,

    /// Model id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

/// Run lifecycle event kind.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RunLifecycleEventKind {
    /// Run started.
    Started,
    /// Run finished normally.
    Finished,
    /// Run was cancelled.
    Cancelled,
    /// Run failed.
    Failed,
}

impl RunLifecycleEventKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Finished => "finished",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

/// Tool lifecycle event kind.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolLifecycleEventKind {
    /// Tool execution started.
    Started,
    /// Tool execution finished.
    Finished,
    /// Tool execution was cancelled.
    Cancelled,
}

impl ToolLifecycleEventKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Finished => "finished",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Durable run lifecycle record.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEventRecord {
    /// Monotonic sequence number assigned by the recorder.
    pub sequence: u64,

    /// Run id.
    pub run_id: String,

    /// Event kind.
    pub kind: RunLifecycleEventKind,

    /// Associated Slack/workspace/model metadata.
    pub metadata: RunEventMetadata,

    /// Final finish reason, if this is a terminal event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,

    /// Aggregated usage known at this event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<LanguageModelUsage>,

    /// Estimated cost known at this event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_micros: Option<u64>,

    /// Redacted operator-facing detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Number of redactions applied while recording.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub redaction_count: usize,
}

/// Durable tool lifecycle record.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolEventRecord {
    /// Monotonic sequence number assigned by the recorder.
    pub sequence: u64,

    /// Run id.
    pub run_id: String,

    /// Workflow step number.
    pub step_number: usize,

    /// Tool call id.
    pub tool_call_id: String,

    /// Tool name.
    pub tool_name: String,

    /// Event kind.
    pub kind: ToolLifecycleEventKind,

    /// Redacted tool input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<JsonValue>,

    /// Redacted tool output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<JsonValue>,

    /// Redacted tool error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Execution duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,

    /// Whether the tool completed successfully.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,

    /// Approval decision for this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalClassification>,

    /// Number of redactions applied while recording.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub redaction_count: usize,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

/// Typed operational event record.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "eventType")]
pub enum OperationalEventRecord {
    /// Run lifecycle event.
    Run(RunEventRecord),
    /// Tool lifecycle event.
    Tool(ToolEventRecord),
}

impl OperationalEventRecord {
    /// Returns the assigned sequence number.
    pub fn sequence(&self) -> u64 {
        match self {
            Self::Run(record) => record.sequence,
            Self::Tool(record) => record.sequence,
        }
    }
}

/// In-memory event recorder useful for deterministic tests and adapter wiring.
#[derive(Clone, Debug)]
pub struct OperationalEventRecorder {
    next_sequence: u64,
    redaction_policy: RedactionPolicy,
    usage: UsageAggregation,
    events: Vec<OperationalEventRecord>,
}

impl Default for OperationalEventRecorder {
    fn default() -> Self {
        Self {
            next_sequence: 1,
            redaction_policy: RedactionPolicy::default(),
            usage: UsageAggregation::default(),
            events: Vec::new(),
        }
    }
}

impl OperationalEventRecorder {
    /// Creates a recorder with default redaction.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a recorder with custom redaction.
    pub fn with_redaction_policy(redaction_policy: RedactionPolicy) -> Self {
        Self {
            redaction_policy,
            ..Self::default()
        }
    }

    /// Records one usage event.
    pub fn record_usage(&mut self, event: UsageAccountingEvent) {
        self.usage.add_event(event);
    }

    /// Returns current usage aggregation.
    pub fn usage(&self) -> &UsageAggregation {
        &self.usage
    }

    /// Returns all recorded events.
    pub fn events(&self) -> &[OperationalEventRecord] {
        &self.events
    }

    /// Records a run lifecycle event.
    pub fn record_run_event(
        &mut self,
        run_id: impl Into<String>,
        kind: RunLifecycleEventKind,
        metadata: RunEventMetadata,
        message: Option<String>,
        finish_reason: Option<String>,
    ) -> u64 {
        let redacted_message = message.map(|message| redact_text(&message, &self.redaction_policy));
        let redaction_count = redacted_message
            .as_ref()
            .map(|message| message.redactions)
            .unwrap_or(0);
        let sequence = self.take_sequence();
        self.events
            .push(OperationalEventRecord::Run(RunEventRecord {
                sequence,
                run_id: run_id.into(),
                kind,
                metadata,
                finish_reason,
                usage: Some(self.usage.total_usage.clone()),
                estimated_cost_micros: self.usage.estimated_cost_micros,
                message: redacted_message.map(|message| message.value),
                redaction_count,
            }));
        sequence
    }

    /// Records a tool lifecycle event.
    pub fn record_tool_event(&mut self, input: ToolEventInput) -> u64 {
        let redacted_input = input
            .input
            .as_ref()
            .map(|input| redact_json_value(input, &self.redaction_policy));
        let redacted_output = input
            .output
            .as_ref()
            .map(|output| redact_json_value(output, &self.redaction_policy));
        let redacted_error = input
            .error
            .as_ref()
            .map(|error| redact_text(error, &self.redaction_policy));
        let redaction_count = redacted_input
            .as_ref()
            .map(|value| value.redactions)
            .unwrap_or(0)
            .saturating_add(
                redacted_output
                    .as_ref()
                    .map(|value| value.redactions)
                    .unwrap_or(0),
            )
            .saturating_add(
                redacted_error
                    .as_ref()
                    .map(|value| value.redactions)
                    .unwrap_or(0),
            );

        let sequence = self.take_sequence();
        self.events
            .push(OperationalEventRecord::Tool(ToolEventRecord {
                sequence,
                run_id: input.run_id,
                step_number: input.step_number,
                tool_call_id: input.tool_call_id,
                tool_name: input.tool_name,
                kind: input.kind,
                input: redacted_input.map(|input| input.value),
                output: redacted_output.map(|output| output.value),
                error: redacted_error.map(|error| error.value),
                duration_ms: input.duration_ms,
                success: input.success,
                approval: input.approval,
                redaction_count,
            }));
        sequence
    }

    fn take_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        sequence
    }
}

/// Input used to record one tool lifecycle event.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolEventInput {
    /// Run id.
    pub run_id: String,

    /// Workflow step number.
    pub step_number: usize,

    /// Tool call id.
    pub tool_call_id: String,

    /// Tool name.
    pub tool_name: String,

    /// Lifecycle kind.
    pub kind: ToolLifecycleEventKind,

    /// Tool input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<JsonValue>,

    /// Tool output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<JsonValue>,

    /// Tool error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Execution duration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,

    /// Execution success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,

    /// Approval classification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalClassification>,
}

impl ToolEventInput {
    /// Creates a tool event input.
    pub fn new(
        run_id: impl Into<String>,
        step_number: usize,
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        kind: ToolLifecycleEventKind,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            step_number,
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            kind,
            input: None,
            output: None,
            error: None,
            duration_ms: None,
            success: None,
            approval: None,
        }
    }

    /// Sets the tool input.
    pub fn with_input(mut self, input: impl Into<JsonValue>) -> Self {
        self.input = Some(input.into());
        self
    }

    /// Sets the tool output.
    pub fn with_output(mut self, output: impl Into<JsonValue>) -> Self {
        self.output = Some(output.into());
        self
    }

    /// Sets the tool error.
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// Sets duration and success.
    pub fn with_result(mut self, duration_ms: u64, success: bool) -> Self {
        self.duration_ms = Some(duration_ms);
        self.success = Some(success);
        self
    }

    /// Sets approval classification.
    pub fn with_approval(mut self, approval: ApprovalClassification) -> Self {
        self.approval = Some(approval);
        self
    }
}

/// Telemetry attributes following the existing AI SDK OTel key style.
pub type OperationalTelemetryAttributes = BTreeMap<String, JsonValue>;

/// Converts an operational event record into stable telemetry attributes.
pub fn operational_telemetry_fields(
    event: &OperationalEventRecord,
) -> OperationalTelemetryAttributes {
    let mut attributes = OperationalTelemetryAttributes::new();
    attributes.insert(
        "ai.workflow.event.sequence".to_string(),
        json!(event.sequence()),
    );

    match event {
        OperationalEventRecord::Run(record) => {
            attributes.insert(
                "ai.workflow.event.kind".to_string(),
                json!(record.kind.as_str()),
            );
            attributes.insert("ai.workflow.run.id".to_string(), json!(record.run_id));
            insert_optional_string(
                &mut attributes,
                "ai.workflow.workspace.id",
                record.metadata.workspace_id.as_deref(),
            );
            insert_optional_string(
                &mut attributes,
                "ai.workflow.user.id",
                record.metadata.user_id.as_deref(),
            );
            insert_optional_string(
                &mut attributes,
                "ai.workflow.channel.id",
                record.metadata.channel_id.as_deref(),
            );
            insert_optional_string(
                &mut attributes,
                "ai.workflow.thread.id",
                record.metadata.thread_id.as_deref(),
            );
            insert_optional_string(
                &mut attributes,
                "ai.workflow.model.id",
                record.metadata.model_id.as_deref(),
            );
            insert_optional_string(
                &mut attributes,
                "ai.workflow.finishReason",
                record.finish_reason.as_deref(),
            );
            if let Some(usage) = &record.usage {
                insert_usage_attributes(&mut attributes, usage);
            }
            if let Some(cost) = record.estimated_cost_micros {
                attributes.insert("ai.workflow.usage.costMicros".to_string(), json!(cost));
            }
            if record.redaction_count > 0 {
                attributes.insert(
                    "ai.workflow.safety.redactionCount".to_string(),
                    json!(record.redaction_count),
                );
            }
        }
        OperationalEventRecord::Tool(record) => {
            attributes.insert(
                "ai.workflow.event.kind".to_string(),
                json!(record.kind.as_str()),
            );
            attributes.insert("ai.workflow.run.id".to_string(), json!(record.run_id));
            attributes.insert(
                "ai.workflow.step.number".to_string(),
                json!(record.step_number),
            );
            attributes.insert(
                "ai.workflow.tool.callId".to_string(),
                json!(record.tool_call_id),
            );
            attributes.insert("ai.workflow.tool.name".to_string(), json!(record.tool_name));
            if let Some(duration_ms) = record.duration_ms {
                attributes.insert(
                    "ai.workflow.tool.durationMs".to_string(),
                    json!(duration_ms),
                );
            }
            if let Some(success) = record.success {
                attributes.insert("ai.workflow.tool.success".to_string(), json!(success));
            }
            if let Some(approval) = &record.approval {
                attributes.insert(
                    "ai.workflow.safety.approvalRequired".to_string(),
                    json!(approval.requires_approval()),
                );
                attributes.insert(
                    "ai.workflow.safety.approvalReasons".to_string(),
                    json!(
                        approval
                            .reasons
                            .iter()
                            .map(approval_reason_name)
                            .collect::<Vec<_>>()
                    ),
                );
            }
            if record.redaction_count > 0 {
                attributes.insert(
                    "ai.workflow.safety.redactionCount".to_string(),
                    json!(record.redaction_count),
                );
            }
        }
    }

    attributes
}

fn insert_optional_string(
    attributes: &mut OperationalTelemetryAttributes,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        attributes.insert(key.to_string(), json!(value));
    }
}

fn insert_usage_attributes(
    attributes: &mut OperationalTelemetryAttributes,
    usage: &LanguageModelUsage,
) {
    if let Some(value) = usage.input_tokens.total {
        attributes.insert("ai.workflow.usage.inputTokens".to_string(), json!(value));
    }
    if let Some(value) = usage.input_tokens.cache_read {
        attributes.insert(
            "ai.workflow.usage.cachedInputTokens".to_string(),
            json!(value),
        );
    }
    if let Some(value) = usage.output_tokens.total {
        attributes.insert("ai.workflow.usage.outputTokens".to_string(), json!(value));
    }
    if let Some(value) = add_optional_u64(usage.input_tokens.total, usage.output_tokens.total) {
        attributes.insert("ai.workflow.usage.totalTokens".to_string(), json!(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_sdk_provider::LanguageModelAbortController;

    fn usage(input: Option<u64>, output: Option<u64>) -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: InputTokenUsage {
                total: input,
                no_cache: input,
                cache_read: Some(2),
                cache_write: None,
            },
            output_tokens: OutputTokenUsage {
                total: output,
                text: output,
                reasoning: Some(1),
            },
            raw: None,
        }
    }

    #[test]
    fn usage_aggregation_adds_nested_token_counts_and_costs() {
        let mut aggregate = UsageAggregation::default();
        aggregate.add_event(
            UsageAccountingEvent::new(usage(Some(10), Some(3)))
                .with_model_id("gpt-test")
                .with_estimated_cost_micros(25),
        );
        aggregate.add_event(
            UsageAccountingEvent::new(usage(None, Some(7)))
                .with_tool_call_id("tool-1")
                .with_estimated_cost_micros(75),
        );

        assert_eq!(aggregate.total_usage.input_tokens.total, Some(10));
        assert_eq!(aggregate.total_usage.input_tokens.no_cache, Some(10));
        assert_eq!(aggregate.total_usage.input_tokens.cache_read, Some(4));
        assert_eq!(aggregate.total_usage.output_tokens.total, Some(10));
        assert_eq!(aggregate.total_usage.output_tokens.reasoning, Some(2));
        assert_eq!(aggregate.estimated_cost_micros, Some(100));
        assert_eq!(aggregate.total_tokens(), Some(20));
        assert_eq!(aggregate.events.len(), 2);
    }

    #[test]
    fn redaction_removes_sensitive_json_keys_assignments_headers_and_tokens() {
        let policy = RedactionPolicy::default();
        let value = json!({
            "apiKey": "sk-live-secret",
            "nested": {
                "message": "Authorization: Bearer ghp_secret123\nOPENAI_API_KEY=sk-another-secret"
            },
            "safe": "hello"
        });

        let redacted = redact_json_value(&value, &policy);

        assert_eq!(redacted.value["apiKey"], REDACTED_VALUE);
        assert_eq!(
            redacted.value["nested"]["message"],
            "Authorization: [REDACTED]\nOPENAI_API_KEY=[REDACTED]"
        );
        assert_eq!(redacted.value["safe"], "hello");
        assert_eq!(redacted.redactions, 3);
    }

    #[test]
    fn rate_limit_decision_allows_with_remaining_and_denies_with_retry() {
        let allowed = decide_rate_limit(RateLimitCheck::new(
            RateLimitScope::User,
            "U123",
            10,
            3,
            4,
            60_000,
        ));
        assert!(allowed.allowed);
        assert_eq!(allowed.remaining, 3);
        assert_eq!(allowed.retry_after_ms, None);

        let denied = decide_rate_limit(RateLimitCheck::new(
            RateLimitScope::Thread,
            "T123:1",
            10,
            9,
            2,
            42_000,
        ));
        assert!(!denied.allowed);
        assert_eq!(denied.remaining, 0);
        assert_eq!(denied.retry_after_ms, Some(42_000));
    }

    #[test]
    fn approval_classification_matches_bash_fetch_and_github_policies() {
        let bash = classify_tool_approval("bash", &json!({ "command": "cat .env && rm -rf tmp" }));
        assert!(bash.requires_approval());
        assert!(bash.reasons.contains(&ApprovalReason::SensitiveFileRead));
        assert!(
            bash.reasons
                .contains(&ApprovalReason::DangerousShellCommand)
        );

        let fetch = classify_tool_approval("web_fetch", &json!({ "url": "http://127.0.0.1:3000" }));
        assert!(fetch.requires_approval());
        assert!(fetch.reasons.contains(&ApprovalReason::NetworkAccess));
        assert!(
            fetch
                .reasons
                .contains(&ApprovalReason::PrivateNetworkAccess)
        );

        let github = classify_tool_approval("github", &json!({ "method": "PATCH" }));
        assert!(github.requires_approval());
        assert!(github.reasons.contains(&ApprovalReason::GithubMutation));

        let read = classify_tool_approval("read", &json!({ "path": "src/lib.rs" }));
        assert!(!read.requires_approval());
    }

    #[test]
    fn cancellation_status_reports_abort_reason() {
        let controller = LanguageModelAbortController::new();
        assert_eq!(
            cancellation_status(Some(&controller.signal())),
            CancellationStatus::Running
        );

        controller.abort_with_reason(json!("client disconnected"));

        assert!(is_cancellation_requested(Some(&controller.signal())));
        assert_eq!(
            cancellation_status(Some(&controller.signal())),
            CancellationStatus::Requested {
                reason: Some(json!("client disconnected"))
            }
        );
    }

    #[test]
    fn event_recorder_orders_records_and_redacts_tool_payloads() {
        let mut recorder = OperationalEventRecorder::new();
        recorder.record_usage(UsageAccountingEvent::new(usage(Some(5), Some(8))));

        let mut metadata = RunEventMetadata::default();
        metadata.workspace_id = Some("W123".to_string());
        metadata.user_id = Some("U123".to_string());
        recorder.record_run_event(
            "run-1",
            RunLifecycleEventKind::Started,
            metadata,
            Some("starting with token=sk-secret-value".to_string()),
            None,
        );
        recorder.record_tool_event(
            ToolEventInput::new(
                "run-1",
                0,
                "call-1",
                "bash",
                ToolLifecycleEventKind::Finished,
            )
            .with_input(json!({ "command": "echo ok", "password": "secret" }))
            .with_output(json!({ "stdout": "ghp_secret_token" }))
            .with_result(15, true)
            .with_approval(classify_tool_approval(
                "bash",
                &json!({ "command": "echo ok" }),
            )),
        );

        assert_eq!(recorder.events()[0].sequence(), 1);
        assert_eq!(recorder.events()[1].sequence(), 2);

        let OperationalEventRecord::Tool(tool) = &recorder.events()[1] else {
            panic!("expected tool event");
        };
        assert_eq!(tool.input.as_ref().unwrap()["password"], REDACTED_VALUE);
        assert_eq!(tool.output.as_ref().unwrap()["stdout"], REDACTED_VALUE);
        assert_eq!(tool.redaction_count, 2);
    }

    #[test]
    fn telemetry_fields_include_usage_tool_and_safety_attributes() {
        let record = OperationalEventRecord::Tool(ToolEventRecord {
            sequence: 7,
            run_id: "run-1".to_string(),
            step_number: 2,
            tool_call_id: "call-1".to_string(),
            tool_name: "web_fetch".to_string(),
            kind: ToolLifecycleEventKind::Finished,
            input: None,
            output: None,
            error: None,
            duration_ms: Some(30),
            success: Some(false),
            approval: Some(classify_tool_approval(
                "web_fetch",
                &json!({ "url": "https://example.com" }),
            )),
            redaction_count: 1,
        });

        let fields = operational_telemetry_fields(&record);

        assert_eq!(fields["ai.workflow.event.sequence"], json!(7));
        assert_eq!(fields["ai.workflow.tool.name"], json!("web_fetch"));
        assert_eq!(fields["ai.workflow.tool.durationMs"], json!(30));
        assert_eq!(fields["ai.workflow.tool.success"], json!(false));
        assert_eq!(fields["ai.workflow.safety.approvalRequired"], json!(true));
        assert_eq!(fields["ai.workflow.safety.redactionCount"], json!(1));
    }
}
