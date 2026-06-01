//! Deterministic security and network layer for the `just-bash` Rust backend.
//!
//! This module intentionally does not execute host processes or perform live
//! network I/O. It owns the portable policy, redaction, limit, cancellation,
//! DNS pinning, and fake-transport contracts that the eventual interpreter /
//! runtime backend can call before dispatching commands.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use url::Url;

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel-labs/just-bash";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the JB-06 security/network seam.
pub const UPSTREAM_HEAD: &str = "d64009aef6bc1556e7c84b22ed455863275ea953";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "just-bash";

/// Default execution limits mirrored from the upstream threat model.
pub const DEFAULT_MAX_SCRIPT_BYTES: usize = 1_048_576;
pub const DEFAULT_MAX_COMMAND_COUNT: usize = 10_000;
pub const DEFAULT_MAX_LOOP_ITERATIONS: usize = 10_000;
pub const DEFAULT_MAX_CALL_DEPTH: usize = 100;
pub const DEFAULT_MAX_STRING_BYTES: usize = 10_485_760;
pub const DEFAULT_MAX_ARRAY_ELEMENTS: usize = 100_000;
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 10_485_760;
pub const DEFAULT_MAX_NETWORK_RESPONSE_BYTES: usize = 10_485_760;
pub const DEFAULT_NETWORK_TIMEOUT_MS: u64 = 30_000;
pub const DEFAULT_MAX_REDIRECTS: usize = 20;

/// Result type for security seam operations.
pub type SecurityResult<T> = std::result::Result<T, SecurityDiagnostic>;

/// Security diagnostic severity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

/// Stable diagnostic codes surfaced by deterministic policy checks.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecurityDiagnosticCode {
    CommandDenied,
    CommandNotAllowed,
    PathEscape,
    PathContainsNul,
    SensitiveValueRedacted,
    LimitExceeded,
    Timeout,
    Cancelled,
    NetworkDisabled,
    NetworkDenied,
    MethodNotAllowed,
    InvalidAllowList,
    PrivateAddressBlocked,
    DnsResolutionFailed,
    ResponseTooLarge,
    TooManyRedirects,
    RuntimeSpecific,
}

/// Deterministic security diagnostic that callers can log after redaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SecurityDiagnostic {
    pub code: SecurityDiagnosticCode,
    pub severity: DiagnosticSeverity,
    pub surface: String,
    pub message: String,
}

impl SecurityDiagnostic {
    #[must_use]
    pub fn error(
        code: SecurityDiagnosticCode,
        surface: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Error,
            surface: surface.into(),
            message: message.into(),
        }
    }

    #[must_use]
    pub fn warning(
        code: SecurityDiagnosticCode,
        surface: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Warning,
            surface: surface.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for SecurityDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?} on {}: {}",
            self.code, self.surface, self.message
        )
    }
}

impl std::error::Error for SecurityDiagnostic {}

/// Command policy with explicit allow and deny lists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandSecurityPolicy {
    default_allow: bool,
    allowed_commands: BTreeSet<String>,
    denied_commands: BTreeSet<String>,
}

impl Default for CommandSecurityPolicy {
    fn default() -> Self {
        Self::allow_all()
    }
}

impl CommandSecurityPolicy {
    #[must_use]
    pub fn allow_all() -> Self {
        Self {
            default_allow: true,
            allowed_commands: BTreeSet::new(),
            denied_commands: BTreeSet::new(),
        }
    }

    #[must_use]
    pub fn allow_only(commands: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            default_allow: false,
            allowed_commands: commands
                .into_iter()
                .map(|command| normalize_command_name(&command.into()))
                .collect(),
            denied_commands: BTreeSet::new(),
        }
    }

    #[must_use]
    pub fn deny(mut self, command: impl Into<String>) -> Self {
        self.denied_commands
            .insert(normalize_command_name(&command.into()));
        self
    }

    pub fn check_command(&self, command: &str) -> SecurityResult<()> {
        let Some(name) = command_name_from_script(command) else {
            return Err(SecurityDiagnostic::error(
                SecurityDiagnosticCode::CommandNotAllowed,
                "command",
                "Command must include an executable name.",
            ));
        };
        if self.denied_commands.contains(&name) {
            return Err(SecurityDiagnostic::error(
                SecurityDiagnosticCode::CommandDenied,
                "command",
                format!("Command '{name}' is denied by policy."),
            ));
        }
        if self.default_allow || self.allowed_commands.contains(&name) {
            return Ok(());
        }
        Err(SecurityDiagnostic::error(
            SecurityDiagnosticCode::CommandNotAllowed,
            "command",
            format!("Command '{name}' is not in the allow-list."),
        ))
    }
}

fn command_name_from_script(command: &str) -> Option<String> {
    command
        .split_whitespace()
        .find(|part| {
            !part.contains('=')
                && !matches!(
                    *part,
                    "env" | "command" | "builtin" | "time" | "timeout" | "sudo"
                )
        })
        .map(normalize_command_name)
}

fn normalize_command_name(command: &str) -> String {
    command
        .rsplit('/')
        .next()
        .unwrap_or(command)
        .trim()
        .to_ascii_lowercase()
}

/// Redaction policy for paths and environment values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedactionPolicy {
    sandbox_roots: Vec<String>,
    sensitive_key_fragments: BTreeSet<String>,
    replacement: String,
}

impl Default for RedactionPolicy {
    fn default() -> Self {
        Self {
            sandbox_roots: Vec::new(),
            sensitive_key_fragments: [
                "TOKEN",
                "SECRET",
                "PASSWORD",
                "PASSWD",
                "API_KEY",
                "ACCESS_KEY",
                "PRIVATE_KEY",
                "AUTH",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            replacement: "<redacted>".to_string(),
        }
    }
}

impl RedactionPolicy {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_sandbox_root(mut self, root: impl Into<String>) -> Self {
        let root = normalize_slashes(&root.into());
        if !root.is_empty() {
            self.sandbox_roots.push(root);
        }
        self
    }

    #[must_use]
    pub fn with_sensitive_key_fragment(mut self, fragment: impl Into<String>) -> Self {
        self.sensitive_key_fragments
            .insert(fragment.into().to_ascii_uppercase());
        self
    }

    #[must_use]
    pub fn redact_message<'a>(
        &self,
        message: impl Into<String>,
        env: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> String {
        let mut redacted = normalize_slashes(&message.into());
        for root in &self.sandbox_roots {
            redacted = redacted.replace(root, "<sandbox>");
        }
        for (key, value) in env {
            if self.is_sensitive_env_key(key) && !value.is_empty() {
                redacted = redacted.replace(value, &self.replacement);
            }
        }
        redacted
    }

    #[must_use]
    pub fn redact_env<'a>(
        &self,
        env: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> (BTreeMap<String, String>, Vec<SecurityDiagnostic>) {
        let mut values = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for (key, value) in env {
            if self.is_sensitive_env_key(key) {
                values.insert(key.to_string(), self.replacement.clone());
                diagnostics.push(SecurityDiagnostic::warning(
                    SecurityDiagnosticCode::SensitiveValueRedacted,
                    "env",
                    format!("Environment value for '{key}' was redacted."),
                ));
            } else {
                values.insert(key.to_string(), value.to_string());
            }
        }
        (values, diagnostics)
    }

    #[must_use]
    pub fn is_sensitive_env_key(&self, key: &str) -> bool {
        let key = key.to_ascii_uppercase();
        self.sensitive_key_fragments
            .iter()
            .any(|fragment| key.contains(fragment))
    }
}

fn normalize_slashes(value: &str) -> String {
    value.replace('\\', "/")
}

/// Validates workspace-relative paths before a backend touches a filesystem.
pub fn validate_workspace_path(path: &str) -> SecurityResult<String> {
    if path.contains('\0') {
        return Err(SecurityDiagnostic::error(
            SecurityDiagnosticCode::PathContainsNul,
            "path",
            "Path must not contain NUL bytes.",
        ));
    }
    let normalized = normalize_slashes(path);
    if normalized.starts_with('/') || normalized.split('/').any(|part| part == "..") {
        return Err(SecurityDiagnostic::error(
            SecurityDiagnosticCode::PathEscape,
            "path",
            "Path must stay within the sandbox root.",
        ));
    }
    Ok(normalized)
}

/// Execution limits mirrored by portable upstream security tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionLimits {
    pub max_script_bytes: usize,
    pub max_command_count: usize,
    pub max_loop_iterations: usize,
    pub max_call_depth: usize,
    pub max_string_bytes: usize,
    pub max_array_elements: usize,
    pub max_output_bytes: usize,
    pub max_network_response_bytes: usize,
    pub max_timeout_ms: u64,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_script_bytes: DEFAULT_MAX_SCRIPT_BYTES,
            max_command_count: DEFAULT_MAX_COMMAND_COUNT,
            max_loop_iterations: DEFAULT_MAX_LOOP_ITERATIONS,
            max_call_depth: DEFAULT_MAX_CALL_DEPTH,
            max_string_bytes: DEFAULT_MAX_STRING_BYTES,
            max_array_elements: DEFAULT_MAX_ARRAY_ELEMENTS,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            max_network_response_bytes: DEFAULT_MAX_NETWORK_RESPONSE_BYTES,
            max_timeout_ms: DEFAULT_NETWORK_TIMEOUT_MS,
        }
    }
}

/// Resource observation gathered by the eventual interpreter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResourceObservation {
    pub script_bytes: usize,
    pub command_count: usize,
    pub loop_iterations: usize,
    pub call_depth: usize,
    pub string_bytes: usize,
    pub array_elements: usize,
    pub output_bytes: usize,
}

impl ExecutionLimits {
    #[must_use]
    pub fn check(&self, observation: ResourceObservation) -> Vec<SecurityDiagnostic> {
        let mut diagnostics = Vec::new();
        push_limit(
            &mut diagnostics,
            "script",
            "script byte length",
            observation.script_bytes,
            self.max_script_bytes,
        );
        push_limit(
            &mut diagnostics,
            "interpreter",
            "command count",
            observation.command_count,
            self.max_command_count,
        );
        push_limit(
            &mut diagnostics,
            "interpreter",
            "loop iteration count",
            observation.loop_iterations,
            self.max_loop_iterations,
        );
        push_limit(
            &mut diagnostics,
            "interpreter",
            "call depth",
            observation.call_depth,
            self.max_call_depth,
        );
        push_limit(
            &mut diagnostics,
            "interpreter",
            "string byte length",
            observation.string_bytes,
            self.max_string_bytes,
        );
        push_limit(
            &mut diagnostics,
            "interpreter",
            "array element count",
            observation.array_elements,
            self.max_array_elements,
        );
        push_limit(
            &mut diagnostics,
            "output",
            "output byte length",
            observation.output_bytes,
            self.max_output_bytes,
        );
        diagnostics
    }

    pub fn check_output(&self, output: &str) -> SecurityResult<()> {
        if output.len() > self.max_output_bytes {
            return Err(SecurityDiagnostic::error(
                SecurityDiagnosticCode::LimitExceeded,
                "output",
                format!(
                    "output byte length limit exceeded: {} > {}",
                    output.len(),
                    self.max_output_bytes
                ),
            ));
        }
        Ok(())
    }
}

fn push_limit(
    diagnostics: &mut Vec<SecurityDiagnostic>,
    surface: &str,
    label: &str,
    observed: usize,
    limit: usize,
) {
    if observed > limit {
        diagnostics.push(SecurityDiagnostic::error(
            SecurityDiagnosticCode::LimitExceeded,
            surface,
            format!("{label} limit exceeded: {observed} > {limit}"),
        ));
    }
}

/// Deterministic cancellation state shared by timeout and abort paths.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CancellationState {
    #[default]
    Running,
    Cancelled,
    TimedOut {
        elapsed_ms: u64,
        timeout_ms: u64,
    },
}

impl CancellationState {
    #[must_use]
    pub fn diagnostic(self) -> Option<SecurityDiagnostic> {
        match self {
            Self::Running => None,
            Self::Cancelled => Some(SecurityDiagnostic::error(
                SecurityDiagnosticCode::Cancelled,
                "execution",
                "Execution cancelled before completion.",
            )),
            Self::TimedOut {
                elapsed_ms,
                timeout_ms,
            } => Some(SecurityDiagnostic::error(
                SecurityDiagnosticCode::Timeout,
                "execution",
                format!("Execution timeout after {elapsed_ms}ms (limit {timeout_ms}ms)."),
            )),
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

/// HTTP methods supported by the network planner.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Head,
    Post,
    Put,
    Delete,
    Patch,
    Options,
}

impl HttpMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Options => "OPTIONS",
        }
    }

    #[must_use]
    pub const fn allows_body(self) -> bool {
        !matches!(self, Self::Get | Self::Head | Self::Options)
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for HttpMethod {
    type Err = SecurityDiagnostic;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_uppercase().as_str() {
            "GET" => Ok(Self::Get),
            "HEAD" => Ok(Self::Head),
            "POST" => Ok(Self::Post),
            "PUT" => Ok(Self::Put),
            "DELETE" => Ok(Self::Delete),
            "PATCH" => Ok(Self::Patch),
            "OPTIONS" => Ok(Self::Options),
            other => Err(SecurityDiagnostic::error(
                SecurityDiagnosticCode::MethodNotAllowed,
                "network",
                format!("HTTP method '{other}' is not supported."),
            )),
        }
    }
}

/// Allow-list entry with optional host-provided firewall headers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AllowedUrlEntry {
    pub url: String,
    pub transform_headers: BTreeMap<String, String>,
}

impl AllowedUrlEntry {
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            transform_headers: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.transform_headers
            .insert(normalize_header_name(&key.into()), value.into());
        self
    }
}

impl From<&str> for AllowedUrlEntry {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for AllowedUrlEntry {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Network access policy. Default is disabled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkPolicy {
    pub allowed_url_prefixes: Vec<AllowedUrlEntry>,
    pub allowed_methods: BTreeSet<HttpMethod>,
    pub dangerously_allow_full_internet_access: bool,
    pub deny_private_ranges: bool,
    pub max_redirects: usize,
    pub timeout_ms: u64,
    pub max_response_bytes: usize,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            allowed_url_prefixes: Vec::new(),
            allowed_methods: [HttpMethod::Get, HttpMethod::Head].into_iter().collect(),
            dangerously_allow_full_internet_access: false,
            deny_private_ranges: false,
            max_redirects: DEFAULT_MAX_REDIRECTS,
            timeout_ms: DEFAULT_NETWORK_TIMEOUT_MS,
            max_response_bytes: DEFAULT_MAX_NETWORK_RESPONSE_BYTES,
        }
    }
}

impl NetworkPolicy {
    #[must_use]
    pub fn allow_url_prefix(prefix: impl Into<AllowedUrlEntry>) -> Self {
        Self {
            allowed_url_prefixes: vec![prefix.into()],
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_allowed_method(mut self, method: HttpMethod) -> Self {
        self.allowed_methods.insert(method);
        self
    }

    #[must_use]
    pub fn with_private_range_deny(mut self, enabled: bool) -> Self {
        self.deny_private_ranges = enabled;
        self
    }

    #[must_use]
    pub fn with_max_response_bytes(mut self, max_response_bytes: usize) -> Self {
        self.max_response_bytes = max_response_bytes;
        self
    }

    #[must_use]
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    #[must_use]
    pub fn full_internet_for_tests() -> Self {
        Self {
            dangerously_allow_full_internet_access: true,
            allowed_methods: [
                HttpMethod::Get,
                HttpMethod::Head,
                HttpMethod::Post,
                HttpMethod::Put,
                HttpMethod::Delete,
                HttpMethod::Patch,
                HttpMethod::Options,
            ]
            .into_iter()
            .collect(),
            ..Self::default()
        }
    }

    pub fn validate_allow_list(&self) -> SecurityResult<()> {
        if self.dangerously_allow_full_internet_access {
            return Ok(());
        }
        for entry in &self.allowed_url_prefixes {
            validate_allow_list_entry(&entry.url)?;
        }
        Ok(())
    }
}

/// Request passed to the deterministic network planner.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NetworkRequest {
    pub url: String,
    pub method: HttpMethod,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub timeout_ms: Option<u64>,
    pub follow_redirects: bool,
}

impl NetworkRequest {
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            method: HttpMethod::Get,
            headers: BTreeMap::new(),
            body: None,
            timeout_ms: None,
            follow_redirects: true,
        }
    }

    #[must_use]
    pub fn with_method(mut self, method: HttpMethod) -> Self {
        self.method = method;
        self
    }

    #[must_use]
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(normalize_header_name(&key.into()), value.into());
        self
    }

    #[must_use]
    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    #[must_use]
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
}

/// DNS address returned by fake or host resolvers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DnsAddress {
    pub address: String,
    pub family: u8,
}

impl DnsAddress {
    #[must_use]
    pub fn new(address: impl Into<String>, family: u8) -> Self {
        Self {
            address: address.into(),
            family,
        }
    }
}

/// DNS lookup error with a stable code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DnsLookupError {
    pub code: String,
    pub message: String,
}

impl DnsLookupError {
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// DNS resolver seam. Tests should provide fake resolvers.
pub trait DnsResolver {
    fn resolve(&self, hostname: &str) -> Result<Vec<DnsAddress>, DnsLookupError>;
}

impl<F> DnsResolver for F
where
    F: Fn(&str) -> Result<Vec<DnsAddress>, DnsLookupError>,
{
    fn resolve(&self, hostname: &str) -> Result<Vec<DnsAddress>, DnsLookupError> {
        self(hostname)
    }
}

/// Planned request after allow-list, method, DNS, timeout, and header policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedNetworkRequest {
    pub url: String,
    pub method: HttpMethod,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub timeout_ms: u64,
    pub pinned_address: Option<DnsAddress>,
}

/// Response returned by a deterministic fake transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub url: String,
}

impl NetworkResponse {
    #[must_use]
    pub fn new(status: u16, url: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            status_text: String::new(),
            headers: BTreeMap::new(),
            body: body.into(),
            url: url.into(),
        }
    }

    #[must_use]
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(normalize_header_name(&key.into()), value.into());
        self
    }

    fn content_length(&self) -> Option<usize> {
        self.headers
            .get("content-length")
            .and_then(|value| value.parse::<usize>().ok())
    }

    fn redirect_location(&self) -> Option<&str> {
        self.headers.get("location").map(String::as_str)
    }
}

/// Transport seam. Tests should use fake transports only.
pub trait NetworkTransport {
    fn send(&mut self, request: &PlannedNetworkRequest) -> SecurityResult<NetworkResponse>;
}

/// Static fake transport keyed by URL.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StaticNetworkTransport {
    responses: BTreeMap<String, NetworkResponse>,
    calls: Vec<PlannedNetworkRequest>,
}

impl StaticNetworkTransport {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_response(mut self, response: NetworkResponse) -> Self {
        self.responses.insert(response.url.clone(), response);
        self
    }

    #[must_use]
    pub fn calls(&self) -> &[PlannedNetworkRequest] {
        &self.calls
    }
}

impl NetworkTransport for StaticNetworkTransport {
    fn send(&mut self, request: &PlannedNetworkRequest) -> SecurityResult<NetworkResponse> {
        self.calls.push(request.clone());
        self.responses.get(&request.url).cloned().ok_or_else(|| {
            SecurityDiagnostic::error(
                SecurityDiagnosticCode::NetworkDenied,
                "network",
                format!("No fake response configured for {}", request.url),
            )
        })
    }
}

pub fn plan_network_request<R: DnsResolver>(
    policy: &NetworkPolicy,
    request: NetworkRequest,
    resolver: &R,
) -> SecurityResult<PlannedNetworkRequest> {
    policy.validate_allow_list()?;
    let parsed = Url::parse(&request.url).map_err(|error| {
        SecurityDiagnostic::error(
            SecurityDiagnosticCode::NetworkDenied,
            "network",
            format!("Invalid URL '{}': {error}", request.url),
        )
    })?;

    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(SecurityDiagnostic::error(
            SecurityDiagnosticCode::NetworkDenied,
            "network",
            format!("Only http and https URLs are allowed: {}", request.url),
        ));
    }

    if !policy.dangerously_allow_full_internet_access
        && !is_url_allowed_by_entries(&parsed, &policy.allowed_url_prefixes)
    {
        return Err(SecurityDiagnostic::error(
            if policy.allowed_url_prefixes.is_empty() {
                SecurityDiagnosticCode::NetworkDisabled
            } else {
                SecurityDiagnosticCode::NetworkDenied
            },
            "network",
            format!(
                "Network access denied: URL not in allow-list: {}",
                request.url
            ),
        ));
    }

    if !policy.dangerously_allow_full_internet_access
        && !policy.allowed_methods.contains(&request.method)
    {
        return Err(SecurityDiagnostic::error(
            SecurityDiagnosticCode::MethodNotAllowed,
            "network",
            format!(
                "HTTP method '{}' not allowed. Allowed methods: {}",
                request.method,
                policy
                    .allowed_methods
                    .iter()
                    .map(|method| method.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }

    let pinned_address = if policy.deny_private_ranges {
        validate_dns_pin(&parsed, resolver)?
    } else {
        None
    };

    let mut headers = request.headers;
    for entry in &policy.allowed_url_prefixes {
        if matches_allow_list_entry(&parsed, &entry.url) {
            for (key, value) in &entry.transform_headers {
                headers.insert(normalize_header_name(key), value.clone());
            }
        }
    }

    let timeout_ms = request.timeout_ms.map_or(policy.timeout_ms, |requested| {
        requested.min(policy.timeout_ms)
    });
    let body = request
        .method
        .allows_body()
        .then_some(request.body)
        .flatten();

    Ok(PlannedNetworkRequest {
        url: parsed.to_string(),
        method: request.method,
        headers,
        body,
        timeout_ms,
        pinned_address,
    })
}

pub fn execute_network_request<R, T>(
    policy: &NetworkPolicy,
    request: NetworkRequest,
    resolver: &R,
    transport: &mut T,
) -> SecurityResult<NetworkResponse>
where
    R: DnsResolver,
    T: NetworkTransport,
{
    let mut current = request;
    let mut redirect_count = 0;
    loop {
        let planned = plan_network_request(policy, current.clone(), resolver)?;
        let response = transport.send(&planned)?;
        check_response_size(policy, &response)?;

        if current.follow_redirects && is_redirect_status(response.status) {
            let Some(location) = response.redirect_location() else {
                return Ok(response);
            };
            redirect_count += 1;
            if redirect_count > policy.max_redirects {
                return Err(SecurityDiagnostic::error(
                    SecurityDiagnosticCode::TooManyRedirects,
                    "network",
                    format!("Too many redirects (max: {}).", policy.max_redirects),
                ));
            }
            let base = Url::parse(&planned.url).map_err(|error| {
                SecurityDiagnostic::error(
                    SecurityDiagnosticCode::NetworkDenied,
                    "network",
                    format!("Invalid redirect base URL '{}': {error}", planned.url),
                )
            })?;
            let redirect_url = base.join(location).map_err(|error| {
                SecurityDiagnostic::error(
                    SecurityDiagnosticCode::NetworkDenied,
                    "network",
                    format!("Invalid redirect location '{location}': {error}"),
                )
            })?;
            current.url = redirect_url.to_string();
            continue;
        }

        return Ok(response);
    }
}

fn check_response_size(policy: &NetworkPolicy, response: &NetworkResponse) -> SecurityResult<()> {
    if let Some(content_length) = response.content_length() {
        if content_length > policy.max_response_bytes {
            return Err(response_too_large(policy.max_response_bytes));
        }
    }
    if response.body.len() > policy.max_response_bytes {
        return Err(response_too_large(policy.max_response_bytes));
    }
    Ok(())
}

fn response_too_large(max: usize) -> SecurityDiagnostic {
    SecurityDiagnostic::error(
        SecurityDiagnosticCode::ResponseTooLarge,
        "network",
        format!("Response body too large (max: {max} bytes)."),
    )
}

fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn validate_dns_pin<R: DnsResolver>(
    parsed: &Url,
    resolver: &R,
) -> SecurityResult<Option<DnsAddress>> {
    let hostname = parsed.host_str().ok_or_else(|| {
        SecurityDiagnostic::error(
            SecurityDiagnosticCode::NetworkDenied,
            "network",
            "URL is missing a host.",
        )
    })?;

    if is_private_hostname(hostname) {
        return Err(SecurityDiagnostic::error(
            SecurityDiagnosticCode::PrivateAddressBlocked,
            "network",
            format!("Network access denied: private/loopback IP address blocked: {hostname}"),
        ));
    }

    if !hostname
        .chars()
        .any(|character| character.is_ascii_alphabetic())
    {
        return Ok(None);
    }

    let addresses = match resolver.resolve(hostname) {
        Ok(addresses) => addresses,
        Err(error) if matches!(error.code.as_str(), "ENOTFOUND" | "ENODATA") => return Ok(None),
        Err(error) => {
            return Err(SecurityDiagnostic::error(
                SecurityDiagnosticCode::DnsResolutionFailed,
                "network",
                format!("DNS resolution failed for private IP check: {}", error.code),
            ));
        }
    };

    for address in &addresses {
        if is_private_hostname(&address.address) {
            return Err(SecurityDiagnostic::error(
                SecurityDiagnosticCode::PrivateAddressBlocked,
                "network",
                format!(
                    "Network access denied: hostname resolves to private/loopback IP address: {}",
                    address.address
                ),
            ));
        }
    }

    Ok(addresses.into_iter().next())
}

pub fn is_url_allowed(url: &str, entries: &[AllowedUrlEntry]) -> bool {
    Url::parse(url).is_ok_and(|parsed| is_url_allowed_by_entries(&parsed, entries))
}

fn is_url_allowed_by_entries(parsed: &Url, entries: &[AllowedUrlEntry]) -> bool {
    entries
        .iter()
        .any(|entry| matches_allow_list_entry(parsed, &entry.url))
}

pub fn matches_allow_list_entry(parsed: &Url, entry: &str) -> bool {
    let Ok(entry) = Url::parse(entry) else {
        return false;
    };

    if url_origin(parsed) != url_origin(&entry) {
        return false;
    }

    let entry_path = entry.path();
    if !matches!(entry_path, "/" | "") && has_ambiguous_path_separators(parsed.path()) {
        return false;
    }
    matches_path_prefix(parsed.path(), entry_path)
}

pub fn validate_allow_list_entry(entry: &str) -> SecurityResult<()> {
    let parsed = Url::parse(entry).map_err(|error| {
        SecurityDiagnostic::error(
            SecurityDiagnosticCode::InvalidAllowList,
            "network",
            format!("Invalid URL '{entry}': must be a valid URL with scheme and host ({error})."),
        )
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(SecurityDiagnostic::error(
            SecurityDiagnosticCode::InvalidAllowList,
            "network",
            format!("Only http and https URLs are allowed in the allow-list: {entry}"),
        ));
    }
    if parsed.host_str().is_none() {
        return Err(SecurityDiagnostic::error(
            SecurityDiagnosticCode::InvalidAllowList,
            "network",
            format!("Invalid allow-list entry without host: {entry}"),
        ));
    }
    if parsed.query().is_some() {
        return Err(SecurityDiagnostic::error(
            SecurityDiagnosticCode::InvalidAllowList,
            "network",
            format!("Query strings are not allowed in network allow-list entries: {entry}"),
        ));
    }
    if parsed.fragment().is_some() {
        return Err(SecurityDiagnostic::error(
            SecurityDiagnosticCode::InvalidAllowList,
            "network",
            format!("URL fragments are not allowed in network allow-list entries: {entry}"),
        ));
    }
    if has_ambiguous_path_separators(parsed.path()) {
        return Err(SecurityDiagnostic::error(
            SecurityDiagnosticCode::InvalidAllowList,
            "network",
            format!("Allow-list entry contains ambiguous path separators: {entry}"),
        ));
    }
    Ok(())
}

fn has_ambiguous_path_separators(pathname: &str) -> bool {
    let normalized = pathname.to_ascii_lowercase();
    pathname.contains('\\') || normalized.contains("%2f") || normalized.contains("%5c")
}

fn matches_path_prefix(pathname: &str, path_prefix: &str) -> bool {
    if matches!(path_prefix, "/" | "") {
        return true;
    }
    if path_prefix.ends_with('/') {
        return pathname.starts_with(path_prefix);
    }
    pathname == path_prefix || pathname.starts_with(&format!("{path_prefix}/"))
}

fn url_origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

fn normalize_header_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

/// Returns true for hostnames/IPs treated as internal by the upstream model.
#[must_use]
pub fn is_private_hostname(hostname: &str) -> bool {
    let normalized = hostname
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if normalized == "localhost" || normalized.ends_with(".localhost") {
        return true;
    }
    if let Some(ipv4) = parse_ipv4_loose(&normalized) {
        return is_private_ipv4(ipv4);
    }
    if let Ok(IpAddr::V6(ipv6)) = normalized.parse::<IpAddr>() {
        return is_private_ipv6(ipv6.segments());
    }
    false
}

fn parse_ipv4_loose(hostname: &str) -> Option<[u8; 4]> {
    if hostname.contains(':') {
        return None;
    }
    let parts = hostname.split('.').collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let values = parts
        .iter()
        .map(|part| parse_ipv4_component(part))
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [n] => Some([
            ((n >> 24) & 0xff) as u8,
            ((n >> 16) & 0xff) as u8,
            ((n >> 8) & 0xff) as u8,
            (n & 0xff) as u8,
        ]),
        [a, b] if *a <= 0xff && *b <= 0x00ff_ffff => Some([
            *a as u8,
            ((b >> 16) & 0xff) as u8,
            ((b >> 8) & 0xff) as u8,
            (b & 0xff) as u8,
        ]),
        [a, b, c] if *a <= 0xff && *b <= 0xff && *c <= 0xffff => Some([
            *a as u8,
            *b as u8,
            ((c >> 8) & 0xff) as u8,
            (c & 0xff) as u8,
        ]),
        [a, b, c, d] if values.iter().all(|value| *value <= 0xff) => {
            Some([*a as u8, *b as u8, *c as u8, *d as u8])
        }
        _ => None,
    }
}

fn parse_ipv4_component(part: &str) -> Option<u32> {
    if part.is_empty() {
        return None;
    }
    let (radix, digits) = if part.starts_with("0x") || part.starts_with("0X") {
        (16, &part[2..])
    } else if part.len() > 1 && part.starts_with('0') {
        (8, part)
    } else {
        (10, part)
    };
    if digits.is_empty() {
        return None;
    }
    u32::from_str_radix(digits, radix).ok()
}

fn is_private_ipv4(ip: [u8; 4]) -> bool {
    let [a, b, c, _d] = ip;
    a == 127
        || a == 10
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 169 && b == 254)
        || a == 0
        || (a == 100 && (64..=127).contains(&b))
        || (a == 198 && matches!(b, 18 | 19))
        || (a == 192 && b == 0 && matches!(c, 0 | 2))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 240
}

fn is_private_ipv6(hextets: [u16; 8]) -> bool {
    if hextets.iter().all(|hextet| *hextet == 0) {
        return true;
    }
    if hextets[..7].iter().all(|hextet| *hextet == 0) && hextets[7] == 1 {
        return true;
    }
    if (hextets[0] & 0xffc0) == 0xfe80 || (hextets[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    if hextets[0] == 0x2001 && hextets[1] == 0x0db8 {
        return true;
    }
    if hextets[0] == 0x0064
        && hextets[1] == 0xff9b
        && hextets[2] == 0
        && hextets[3] == 0
        && hextets[4] == 0
        && hextets[5] == 0
    {
        return is_private_ipv4(embedded_ipv4(hextets[6], hextets[7]));
    }
    if hextets[0] == 0x0064 && hextets[1] == 0xff9b && hextets[2] == 0x0001 {
        return true;
    }
    if hextets[0] == 0x2002 {
        return is_private_ipv4(embedded_ipv4(hextets[1], hextets[2]));
    }
    let is_mapped = hextets[..5].iter().all(|hextet| *hextet == 0) && hextets[5] == 0xffff;
    is_mapped && is_private_ipv4(embedded_ipv4(hextets[6], hextets[7]))
}

fn embedded_ipv4(high: u16, low: u16) -> [u8; 4] {
    [
        (high >> 8) as u8,
        (high & 0xff) as u8,
        (low >> 8) as u8,
        (low & 0xff) as u8,
    ]
}

/// Runtime surfaces that are intentionally outside this Rust seam.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpstreamRuntimeSurface {
    CoreSecurity,
    BrowserBundle,
    NodeWorker,
    QuickJs,
    PythonWasm,
    SqliteWasm,
    WasmCallback,
}

/// Classification for upstream runtime-specific tests.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimePortability {
    pub surface: UpstreamRuntimeSurface,
    pub portable_to_rust_backend: bool,
    pub reason: String,
}

#[must_use]
pub fn classify_runtime_surface(surface: UpstreamRuntimeSurface) -> RuntimePortability {
    match surface {
        UpstreamRuntimeSurface::CoreSecurity => RuntimePortability {
            surface,
            portable_to_rust_backend: true,
            reason: "portable security policy, limits, cancellation, diagnostics, and network planning"
                .to_string(),
        },
        UpstreamRuntimeSurface::BrowserBundle => RuntimePortability {
            surface,
            portable_to_rust_backend: false,
            reason: "browser bundling and stubbed Node modules are JavaScript packaging behavior"
                .to_string(),
        },
        UpstreamRuntimeSurface::NodeWorker => RuntimePortability {
            surface,
            portable_to_rust_backend: false,
            reason: "Node worker protocol and AsyncLocalStorage monkey-patching are host-runtime behavior"
                .to_string(),
        },
        UpstreamRuntimeSurface::QuickJs => RuntimePortability {
            surface,
            portable_to_rust_backend: false,
            reason: "QuickJS execution is an optional JavaScript runtime not embedded in this Rust backend"
                .to_string(),
        },
        UpstreamRuntimeSurface::PythonWasm => RuntimePortability {
            surface,
            portable_to_rust_backend: false,
            reason: "CPython/Emscripten worker behavior is an optional WASM runtime"
                .to_string(),
        },
        UpstreamRuntimeSurface::SqliteWasm => RuntimePortability {
            surface,
            portable_to_rust_backend: false,
            reason: "sql.js worker behavior is an optional WASM runtime"
                .to_string(),
        },
        UpstreamRuntimeSurface::WasmCallback => RuntimePortability {
            surface,
            portable_to_rust_backend: false,
            reason: "WASM callback bridge behavior is runtime-specific to the JavaScript package"
                .to_string(),
        },
    }
}
