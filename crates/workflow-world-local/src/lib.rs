//! Local World crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/world-local`: config resolution,
//! data-directory initialization, filesystem-backed queue/storage/stream state,
//! scoped tags, startup recovery, and lightweight telemetry helpers.

#![forbid(unsafe_code)]

use base64::Engine as _;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    env, fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub use workflow_world as world;

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the initial crate skeleton.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/world-local";

/// Upstream package version inventoried for this skeleton.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.11";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default data resolution mode used by upstream world-local.
pub const DEFAULT_RESOLVE_DATA_OPTION: ResolveData = ResolveData::All;

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Error type for local-world operations.
#[derive(Debug, thiserror::Error)]
pub enum LocalWorldError {
    #[error("{0}")]
    Message(String),
    #[error(
        "unsafe {kind} \"{value}\": must not be empty, contain '.', '/', '\\\\', or null bytes"
    )]
    UnsafeEntityId { kind: String, value: String },
    #[error("workflow run \"{0}\" not found")]
    WorkflowRunNotFound(String),
    #[error("hook \"{0}\" not found")]
    HookNotFound(String),
    #[error("entity conflict: {0}")]
    EntityConflict(String),
    #[error("run expired: {0}")]
    RunExpired(String),
    #[error("run requires newer world spec {required}, current spec is {current}")]
    RunNotSupported { required: u32, current: u32 },
    #[error("too early: {message}")]
    TooEarly {
        message: String,
        retry_after_seconds: i64,
    },
    #[error("unable to resolve base URL for workflow queue")]
    UnableToResolveBaseUrl,
    #[error("data directory \"{data_dir}\" is not accessible: {message}")]
    DataDirAccess {
        data_dir: PathBuf,
        message: String,
        code: Option<String>,
    },
    #[error("data directory version error: {message}")]
    DataDirVersion {
        message: String,
        old_version: Box<ParsedVersion>,
        new_version: Box<ParsedVersion>,
        suggested_version: Option<String>,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    TimeFormat(#[from] time::error::Format),
    #[error(transparent)]
    TimeParse(#[from] time::error::Parse),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, LocalWorldError>;

/// Whether storage reads should include serialized payload fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolveData {
    All,
    None,
}

/// Sort direction used by list operations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SortOrder {
    Asc,
    #[default]
    Desc,
}

/// Cursor pagination options.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PaginationOptions {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub sort_order: SortOrder,
}

/// Paginated response shared by storage and stream APIs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub cursor: Option<String>,
    pub has_more: bool,
}

/// Configuration for a local world instance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub data_dir: PathBuf,
    pub port: Option<u16>,
    pub base_url: Option<String>,
    pub recover_active_runs: bool,
    pub tag: Option<String>,
    pub stream_flush_interval_ms: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_dir: data_dir_from_env(),
            port: None,
            base_url: base_url_from_env(),
            recover_active_runs: true,
            tag: None,
            stream_flush_interval_ms: None,
        }
    }
}

impl Config {
    /// Build config from explicit values, preserving upstream env defaults.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            ..Self::default()
        }
    }

    /// Add a filesystem tag that scopes writes and clears.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Disable startup recovery.
    pub fn without_recovery(mut self) -> Self {
        self.recover_active_runs = false;
        self
    }
}

fn data_dir_from_env() -> PathBuf {
    env::var_os("WORKFLOW_LOCAL_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".workflow-data"))
}

fn base_url_from_env() -> Option<String> {
    env::var("WORKFLOW_LOCAL_BASE_URL").ok()
}

/// Resolve the local queue base URL using upstream precedence.
pub fn resolve_base_url(config: &Config) -> Result<String> {
    let port_env = env::var("PORT").ok();
    resolve_base_url_from_parts(
        config.base_url.as_deref(),
        env::var("WORKFLOW_LOCAL_BASE_URL").ok().as_deref(),
        config.port,
        port_env.as_deref(),
        None,
    )
}

/// Pure resolver used by tests and host integration.
pub fn resolve_base_url_from_parts(
    config_base_url: Option<&str>,
    env_base_url: Option<&str>,
    config_port: Option<u16>,
    env_port: Option<&str>,
    detected_port: Option<u16>,
) -> Result<String> {
    if let Some(base_url) = config_base_url.filter(|value| !value.is_empty()) {
        return Ok(base_url.to_string());
    }
    if let Some(base_url) = env_base_url.filter(|value| !value.is_empty()) {
        return Ok(base_url.to_string());
    }
    if let Some(port) = config_port {
        return Ok(format!("http://localhost:{port}"));
    }
    if let Some(port) = env_port.filter(|value| !value.is_empty()) {
        return Ok(format!("http://localhost:{port}"));
    }
    if let Some(port) = detected_port {
        return Ok(format!("http://localhost:{port}"));
    }
    Err(LocalWorldError::UnableToResolveBaseUrl)
}

/// Semantic version parsed from world-local version files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub prerelease: Option<String>,
    pub raw: String,
}

impl fmt::Display for ParsedVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(prerelease) = &self.prerelease {
            write!(
                formatter,
                "{}.{}.{}-{}",
                self.major, self.minor, self.patch, prerelease
            )
        } else {
            write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
        }
    }
}

/// Parse `1.2.3` or `1.2.3-beta.4`.
pub fn parse_version(version_string: &str) -> Result<ParsedVersion> {
    let (core, prerelease) = match version_string.split_once('-') {
        Some((core, prerelease)) if !prerelease.is_empty() => (core, Some(prerelease.to_string())),
        Some(_) => {
            return Err(LocalWorldError::Message(format!(
                "Invalid version string: \"{version_string}\""
            )));
        }
        None => (version_string, None),
    };
    let parts = core.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(LocalWorldError::Message(format!(
            "Invalid version string: \"{version_string}\""
        )));
    }
    let major = parts[0].parse::<u64>().map_err(|_| {
        LocalWorldError::Message(format!("Invalid version string: \"{version_string}\""))
    })?;
    let minor = parts[1].parse::<u64>().map_err(|_| {
        LocalWorldError::Message(format!("Invalid version string: \"{version_string}\""))
    })?;
    let patch = parts[2].parse::<u64>().map_err(|_| {
        LocalWorldError::Message(format!("Invalid version string: \"{version_string}\""))
    })?;
    Ok(ParsedVersion {
        major,
        minor,
        patch,
        prerelease,
        raw: version_string.to_string(),
    })
}

pub fn format_version(version: &ParsedVersion) -> String {
    version.to_string()
}

pub fn parse_version_file(content: &str) -> Result<(String, ParsedVersion)> {
    let trimmed = content.trim();
    let Some(index) = trimmed.rfind('@') else {
        return Err(LocalWorldError::Message(format!(
            "Invalid version file content: \"{content}\""
        )));
    };
    if index == 0 {
        return Err(LocalWorldError::Message(format!(
            "Invalid version file content: \"{content}\""
        )));
    }
    let package_name = trimmed[..index].to_string();
    let version = parse_version(&trimmed[index + 1..])?;
    Ok((package_name, version))
}

pub fn format_version_file(package_name: &str, version: &ParsedVersion) -> String {
    format!("{package_name}@{}", format_version(version))
}

pub fn upgrade_version(old_version: &ParsedVersion, new_version: &ParsedVersion) -> Result<()> {
    eprintln!(
        "[world-local] Upgrading from version {} to {}",
        format_version(old_version),
        format_version(new_version)
    );
    Ok(())
}

/// Ensure a data directory exists, is readable, and is writable.
pub fn ensure_data_dir(data_dir: impl AsRef<Path>) -> Result<PathBuf> {
    let absolute_path = absolute_path(data_dir.as_ref())?;
    fs::create_dir_all(&absolute_path).map_err(|error| LocalWorldError::DataDirAccess {
        data_dir: absolute_path.clone(),
        message: format!("Failed to create data directory: {error}"),
        code: error.raw_os_error().map(|code| code.to_string()),
    })?;

    fs::read_dir(&absolute_path).map_err(|error| LocalWorldError::DataDirAccess {
        data_dir: absolute_path.clone(),
        message: format!("not readable: {error}"),
        code: error.raw_os_error().map(|code| code.to_string()),
    })?;

    let test_file = absolute_path.join(format!(
        ".workflow-write-test-{}",
        ID_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    File::create(&test_file)
        .and_then(|mut file| file.write_all(b""))
        .map_err(|error| LocalWorldError::DataDirAccess {
            data_dir: absolute_path.clone(),
            message: format!("not writable: {error}"),
            code: error.raw_os_error().map(|code| code.to_string()),
        })?;
    match fs::remove_file(&test_file) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(absolute_path)
}

/// Initialize the world-local data directory and version file.
pub fn init_data_dir(data_dir: impl AsRef<Path>) -> Result<()> {
    let data_dir = ensure_data_dir(data_dir)?;
    let current_version = parse_version(CRATE_VERSION)?;
    let version_path = data_dir.join("version.txt");
    let existing = match fs::read_to_string(&version_path) {
        Ok(content) => Some(parse_version_file(&content)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };

    if let Some((_package, old_version)) = existing {
        if format_version(&old_version) == format_version(&current_version) {
            return Ok(());
        }
        upgrade_version(&old_version, &current_version)?;
    }

    fs::write(
        version_path,
        format_version_file(UPSTREAM_PACKAGE, &current_version),
    )?;
    Ok(())
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

fn truncate_for_error(value: &str) -> String {
    const MAX: usize = 48;
    if value.chars().count() > MAX {
        let prefix = value.chars().take(MAX).collect::<String>();
        format!("{prefix}...")
    } else {
        value.to_string()
    }
}

/// Validate an ID before embedding it in a local filesystem path.
pub fn assert_safe_entity_id(kind: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.starts_with('.')
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
        || value.contains('.')
    {
        return Err(LocalWorldError::UnsafeEntityId {
            kind: kind.to_string(),
            value: truncate_for_error(value),
        });
    }
    Ok(())
}

/// Resolve path segments under a base directory and reject escapes.
pub fn resolve_within_base(base_dir: impl AsRef<Path>, segments: &[&str]) -> Result<PathBuf> {
    let base = absolute_path(base_dir.as_ref())?;
    let mut joined = base.clone();
    for segment in segments {
        if Path::new(segment).components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(LocalWorldError::UnsafeEntityId {
                kind: "path".to_string(),
                value: truncate_for_error(&segments.join("/")),
            });
        }
        joined.push(segment);
    }
    let absolute_joined = absolute_path(&joined)?;
    if absolute_joined != base && !absolute_joined.starts_with(&base) {
        return Err(LocalWorldError::UnsafeEntityId {
            kind: "path".to_string(),
            value: truncate_for_error(&segments.join("/")),
        });
    }
    Ok(absolute_joined)
}

/// Strip a trailing tag suffix from a file id.
pub fn strip_tag(file_id: &str) -> String {
    let Some((prefix, suffix)) = file_id.rsplit_once('.') else {
        return file_id.to_string();
    };
    let mut chars = suffix.chars();
    if matches!(chars.next(), Some(first) if first.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        prefix.to_string()
    } else {
        file_id.to_string()
    }
}

pub fn has_tag(file_id: &str, tag: &str) -> bool {
    file_id.ends_with(&format!(".{tag}"))
}

/// Build a JSON path with an optional tag suffix.
pub fn tagged_path(
    base_dir: impl AsRef<Path>,
    entity_dir: &str,
    file_id: &str,
    tag: Option<&str>,
) -> Result<PathBuf> {
    assert_safe_entity_id("fileId", file_id)?;
    if let Some(tag) = tag {
        assert_safe_entity_id("tag", tag)?;
    }
    let filename = if let Some(tag) = tag {
        format!("{file_id}.{tag}.json")
    } else {
        format!("{file_id}.json")
    };
    resolve_within_base(base_dir, &[entity_dir, &filename])
}

pub fn clear_created_files_cache() {}

pub fn write_json<T: Serialize>(path: impl AsRef<Path>, data: &T, overwrite: bool) -> Result<()> {
    write_bytes(
        path,
        serde_json::to_string_pretty(data)?.as_bytes(),
        overwrite,
    )
}

pub fn read_json<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<Option<T>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(serde_json::from_str(&content)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn read_json_with_fallback<T: DeserializeOwned>(
    base_dir: &Path,
    entity_dir: &str,
    file_id: &str,
    tag: Option<&str>,
) -> Result<Option<T>> {
    assert_safe_entity_id("fileId", file_id)?;
    if let Some(tag) = tag {
        assert_safe_entity_id("tag", tag)?;
        if let Some(value) = read_json(resolve_within_base(
            base_dir,
            &[entity_dir, &format!("{file_id}.{tag}.json")],
        )?)? {
            return Ok(Some(value));
        }
    }
    read_json(resolve_within_base(
        base_dir,
        &[entity_dir, &format!("{file_id}.json")],
    )?)
}

fn write_bytes(path: impl AsRef<Path>, data: &[u8], overwrite: bool) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if !overwrite && path.exists() {
        return Err(LocalWorldError::EntityConflict(format!(
            "File {} already exists and overwrite is false",
            path.display()
        )));
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true);
    if overwrite {
        options.truncate(true);
    } else {
        options.create_new(true);
    }
    let temp_path = path.with_extension(format!(
        "{}.tmp.{}",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or(""),
        next_monotonic_id("tmp")
    ));
    {
        let mut file = options.open(&temp_path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                LocalWorldError::EntityConflict(format!(
                    "File {} already exists and overwrite is false",
                    path.display()
                ))
            } else {
                LocalWorldError::Io(error)
            }
        })?;
        file.write_all(data)?;
    }
    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            Err(error.into())
        }
    }
}

fn write_exclusive(path: impl AsRef<Path>, data: &[u8]) -> Result<bool> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            file.write_all(data)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn delete_path(path: impl AsRef<Path>) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn list_files_by_extension(dir: impl AsRef<Path>, extension: &str) -> Result<Vec<String>> {
    let mut files = Vec::new();
    match fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(file_id) = name.strip_suffix(extension) {
                    files.push(file_id.to_string());
                }
            }
            files.sort();
            Ok(files)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(files),
        Err(error) => Err(error.into()),
    }
}

pub fn list_tagged_files(dir: impl AsRef<Path>, tag: &str) -> Result<Vec<String>> {
    let suffix = format!(".{tag}.json");
    let mut files = Vec::new();
    match fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(&suffix) {
                    files.push(name);
                }
            }
            files.sort();
            Ok(files)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(files),
        Err(error) => Err(error.into()),
    }
}

pub fn list_tagged_files_by_extension(
    dir: impl AsRef<Path>,
    tag: &str,
    extension: &str,
) -> Result<Vec<String>> {
    let suffix = format!(".{tag}{extension}");
    let mut files = Vec::new();
    match fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(&suffix) {
                    files.push(name);
                }
            }
            files.sort();
            Ok(files)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(files),
        Err(error) => Err(error.into()),
    }
}

/// Extract a timestamp from the first ten Crockford-base32 ULID chars.
pub fn ulid_to_date(id: &str) -> Option<OffsetDateTime> {
    let ulid = id.rsplit('_').next().unwrap_or(id);
    if ulid.len() < 10 {
        return None;
    }
    let mut timestamp: u64 = 0;
    for ch in ulid.chars().take(10) {
        let value = crockford_value(ch)?;
        timestamp = timestamp.checked_mul(32)?.checked_add(value as u64)?;
    }
    OffsetDateTime::from_unix_timestamp_nanos((timestamp as i128) * 1_000_000).ok()
}

fn crockford_value(ch: char) -> Option<u8> {
    match ch.to_ascii_uppercase() {
        '0' => Some(0),
        '1' | 'I' | 'L' => Some(1),
        '2' => Some(2),
        '3' => Some(3),
        '4' => Some(4),
        '5' => Some(5),
        '6' => Some(6),
        '7' => Some(7),
        '8' => Some(8),
        '9' => Some(9),
        'A' => Some(10),
        'B' => Some(11),
        'C' => Some(12),
        'D' => Some(13),
        'E' => Some(14),
        'F' => Some(15),
        'G' => Some(16),
        'H' => Some(17),
        'J' => Some(18),
        'K' => Some(19),
        'M' => Some(20),
        'N' => Some(21),
        'P' => Some(22),
        'Q' => Some(23),
        'R' => Some(24),
        'S' => Some(25),
        'T' => Some(26),
        'V' => Some(27),
        'W' => Some(28),
        'X' => Some(29),
        'Y' => Some(30),
        'Z' => Some(31),
        _ => None,
    }
}

fn encode_crockford(mut value: u128, width: usize) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut chars = vec!['0'; width];
    for index in (0..width).rev() {
        chars[index] = ALPHABET[(value & 31) as usize] as char;
        value >>= 5;
    }
    chars.into_iter().collect()
}

fn next_monotonic_id(prefix: &str) -> String {
    let millis = OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
    let counter = ID_COUNTER.fetch_add(1, Ordering::SeqCst) as u128;
    format!(
        "{prefix}_{}{}",
        encode_crockford(millis as u128, 10),
        encode_crockford(counter, 16)
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl WorkflowRunStatus {
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRun {
    pub run_id: String,
    pub status: WorkflowRunStatus,
    pub deployment_id: String,
    pub workflow_name: String,
    pub spec_version: Option<u32>,
    pub execution_context: Option<Value>,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub error: Option<Value>,
    pub error_code: Option<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expired_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl StepStatus {
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub run_id: String,
    pub step_id: String,
    pub step_name: String,
    pub status: StepStatus,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub error: Option<Value>,
    pub attempt: u32,
    #[serde(with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub retry_after: Option<OffsetDateTime>,
    pub spec_version: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hook {
    pub run_id: String,
    pub hook_id: String,
    pub token: String,
    pub owner_id: String,
    pub project_id: String,
    pub environment: String,
    pub metadata: Option<Value>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub spec_version: Option<u32>,
    pub is_webhook: Option<bool>,
    pub is_system: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WaitStatus {
    Waiting,
    Completed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Wait {
    pub wait_id: String,
    pub run_id: String,
    pub status: WaitStatus,
    #[serde(with = "time::serde::rfc3339::option")]
    pub resume_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub spec_version: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub event_type: String,
    pub correlation_id: Option<String>,
    pub event_data: Option<Value>,
    pub spec_version: Option<u32>,
    pub run_id: String,
    pub event_id: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributeChange {
    pub key: String,
    pub value: Option<String>,
}

impl AttributeChange {
    pub fn set(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: Some(value.into()),
        }
    }

    pub fn unset(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExperimentalSetAttributesResult {
    pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AttributeOptions {
    pub allow_reserved_attributes: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListWorkflowRunsParams {
    pub workflow_name: Option<String>,
    pub status: Option<WorkflowRunStatus>,
    pub pagination: PaginationOptions,
    pub resolve_data: ResolveData,
    pub file_id_filter_tag: Option<String>,
}

impl Default for ListWorkflowRunsParams {
    fn default() -> Self {
        Self {
            workflow_name: None,
            status: None,
            pagination: PaginationOptions::default(),
            resolve_data: ResolveData::All,
            file_id_filter_tag: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListByRunParams {
    pub run_id: String,
    pub pagination: PaginationOptions,
    pub resolve_data: ResolveData,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventResult {
    pub event: Option<Event>,
    pub run: Option<WorkflowRun>,
    pub step: Option<Step>,
    pub hook: Option<Hook>,
    pub wait: Option<Wait>,
    pub events: Option<Vec<Event>>,
    pub cursor: Option<String>,
    pub has_more: Option<bool>,
}

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn status_string_run(status: &WorkflowRunStatus) -> &'static str {
    match status {
        WorkflowRunStatus::Pending => "pending",
        WorkflowRunStatus::Running => "running",
        WorkflowRunStatus::Completed => "completed",
        WorkflowRunStatus::Failed => "failed",
        WorkflowRunStatus::Cancelled => "cancelled",
    }
}

fn status_string_step(status: &StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "pending",
        StepStatus::Running => "running",
        StepStatus::Completed => "completed",
        StepStatus::Failed => "failed",
        StepStatus::Cancelled => "cancelled",
    }
}

fn value_object(value: Option<&Value>) -> Map<String, Value> {
    value
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(Map::new)
}

fn strip_run_data(mut run: WorkflowRun, resolve_data: ResolveData) -> WorkflowRun {
    if resolve_data == ResolveData::None {
        run.input = None;
        run.output = None;
    }
    run
}

fn strip_step_data(mut step: Step, resolve_data: ResolveData) -> Step {
    if resolve_data == ResolveData::None {
        step.input = None;
        step.output = None;
    }
    step
}

fn strip_hook_data(mut hook: Hook, resolve_data: ResolveData) -> Hook {
    if resolve_data == ResolveData::None {
        hook.metadata = None;
    }
    if hook.is_webhook.is_none() {
        hook.is_webhook = Some(true);
    }
    hook
}

fn strip_event_data_refs(mut event: Event, resolve_data: ResolveData) -> Event {
    if resolve_data != ResolveData::None {
        return event;
    }
    let Some(Value::Object(mut data)) = event.event_data.take() else {
        return event;
    };
    let ref_fields: &[&str] = match event.event_type.as_str() {
        "run_created" => &["input"],
        "run_completed" => &["output"],
        "run_failed" => &["error"],
        "step_created" => &["input"],
        "step_completed" => &["result"],
        "step_failed" | "step_retrying" => &["error"],
        "hook_created" => &["metadata"],
        "hook_received" => &["payload"],
        _ => &[],
    };
    for field in ref_fields {
        data.remove(*field);
    }
    if !data.is_empty() {
        event.event_data = Some(Value::Object(data));
    }
    event
}

fn parse_cursor(cursor: &Option<String>) -> Option<(OffsetDateTime, Option<String>)> {
    let cursor = cursor.as_ref()?;
    let (timestamp, id) = match cursor.split_once('|') {
        Some((timestamp, id)) => (timestamp, Some(id.to_string())),
        None => (cursor.as_str(), None),
    };
    let timestamp = OffsetDateTime::parse(timestamp, &Rfc3339).ok()?;
    Some((timestamp, id))
}

fn create_cursor(timestamp: OffsetDateTime, id: Option<&str>) -> Result<String> {
    let timestamp = timestamp.format(&Rfc3339)?;
    Ok(match id {
        Some(id) => format!("{timestamp}|{id}"),
        None => timestamp,
    })
}

fn paginate_items<T, FCreated, FId>(
    mut items: Vec<T>,
    pagination: &PaginationOptions,
    created_at: FCreated,
    id: FId,
) -> Result<PaginatedResponse<T>>
where
    FCreated: Fn(&T) -> OffsetDateTime,
    FId: Fn(&T) -> String,
{
    let sort_order = pagination.sort_order;
    items.sort_by(|left, right| {
        let left_created = created_at(left);
        let right_created = created_at(right);
        let time_cmp = left_created.cmp(&right_created);
        let cmp = if time_cmp == std::cmp::Ordering::Equal {
            id(left).cmp(&id(right))
        } else {
            time_cmp
        };
        match sort_order {
            SortOrder::Asc => cmp,
            SortOrder::Desc => cmp.reverse(),
        }
    });

    if let Some((cursor_time, cursor_id)) = parse_cursor(&pagination.cursor) {
        items.retain(|item| {
            let item_time = created_at(item);
            let item_id = id(item);
            match sort_order {
                SortOrder::Desc => {
                    item_time < cursor_time
                        || (item_time == cursor_time
                            && cursor_id.as_ref().is_some_and(|cursor| item_id < *cursor))
                }
                SortOrder::Asc => {
                    item_time > cursor_time
                        || (item_time == cursor_time
                            && cursor_id.as_ref().is_some_and(|cursor| item_id > *cursor))
                }
            }
        });
    }

    let limit = pagination.limit.unwrap_or(20);
    let has_more = items.len() > limit;
    if has_more {
        items.truncate(limit);
    }
    let cursor = match items.last() {
        Some(item) => Some(create_cursor(created_at(item), Some(&id(item)))?),
        None => None,
    };
    Ok(PaginatedResponse {
        data: items,
        cursor,
        has_more,
    })
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let bytes = hasher.finalize();
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use fmt::Write as _;
        let _ = write!(&mut result, "{byte:02x}");
    }
    result
}

/// Filesystem-backed storage implementation.
#[derive(Clone, Debug)]
pub struct LocalStorage {
    base_dir: PathBuf,
    tag: Option<String>,
    lock: Arc<Mutex<()>>,
}

impl LocalStorage {
    pub fn new(base_dir: impl Into<PathBuf>, tag: Option<String>) -> Self {
        Self {
            base_dir: base_dir.into(),
            tag,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    pub fn runs_get(&self, run_id: &str, resolve_data: ResolveData) -> Result<WorkflowRun> {
        assert_safe_entity_id("runId", run_id)?;
        let run = read_json_with_fallback::<WorkflowRun>(
            &self.base_dir,
            "runs",
            run_id,
            self.tag.as_deref(),
        )?
        .ok_or_else(|| LocalWorldError::WorkflowRunNotFound(run_id.to_string()))?;
        Ok(strip_run_data(run, resolve_data))
    }

    pub fn runs_list(
        &self,
        params: ListWorkflowRunsParams,
    ) -> Result<PaginatedResponse<WorkflowRun>> {
        let run_dir = self.base_dir.join("runs");
        let mut runs = Vec::new();
        for file_id in list_files_by_extension(&run_dir, ".json")? {
            if let Some(tag) = &params.file_id_filter_tag {
                if !has_tag(&file_id, tag) {
                    continue;
                }
            }
            let Some(mut run) = read_json::<WorkflowRun>(run_dir.join(format!("{file_id}.json")))?
            else {
                continue;
            };
            if let Some(workflow_name) = &params.workflow_name {
                if run.workflow_name != *workflow_name {
                    continue;
                }
            }
            if let Some(status) = &params.status {
                if run.status != *status {
                    continue;
                }
            }
            run = strip_run_data(run, params.resolve_data);
            runs.push(run);
        }
        paginate_items(
            runs,
            &params.pagination,
            |run| run.created_at,
            |run| run.run_id.clone(),
        )
    }

    pub fn experimental_set_attributes(
        &self,
        run_id: &str,
        changes: &[AttributeChange],
        options: AttributeOptions,
    ) -> Result<ExperimentalSetAttributesResult> {
        assert_safe_entity_id("runId", run_id)?;
        let _guard = self.lock.lock().expect("local storage mutex poisoned");
        let mut run = self.runs_get(run_id, ResolveData::All)?;
        validate_attribute_changes(&run.attributes, changes, &options)?;
        for change in changes {
            if let Some(value) = &change.value {
                run.attributes.insert(change.key.clone(), value.clone());
            } else {
                run.attributes.remove(&change.key);
            }
        }
        run.updated_at = now();
        write_json(
            tagged_path(&self.base_dir, "runs", run_id, self.tag.as_deref())?,
            &run,
            true,
        )?;
        Ok(ExperimentalSetAttributesResult {
            attributes: run.attributes,
        })
    }

    pub fn steps_get(
        &self,
        run_id: &str,
        step_id: &str,
        resolve_data: ResolveData,
    ) -> Result<Step> {
        assert_safe_entity_id("runId", run_id)?;
        assert_safe_entity_id("stepId", step_id)?;
        let composite = format!("{run_id}-{step_id}");
        let step = read_json_with_fallback::<Step>(
            &self.base_dir,
            "steps",
            &composite,
            self.tag.as_deref(),
        )?
        .ok_or_else(|| {
            LocalWorldError::Message(format!("Step {step_id} in run {run_id} not found"))
        })?;
        Ok(strip_step_data(step, resolve_data))
    }

    pub fn steps_list(&self, params: ListByRunParams) -> Result<PaginatedResponse<Step>> {
        assert_safe_entity_id("runId", &params.run_id)?;
        let steps_dir = self.base_dir.join("steps");
        let prefix = format!("{}-", params.run_id);
        let mut steps = Vec::new();
        for file_id in list_files_by_extension(&steps_dir, ".json")? {
            if !file_id.starts_with(&prefix) {
                continue;
            }
            let Some(step) = read_json::<Step>(steps_dir.join(format!("{file_id}.json")))? else {
                continue;
            };
            steps.push(strip_step_data(step, params.resolve_data));
        }
        paginate_items(
            steps,
            &params.pagination,
            |step| step.created_at,
            |step| step.step_id.clone(),
        )
    }

    pub fn hooks_get(&self, hook_id: &str, resolve_data: ResolveData) -> Result<Hook> {
        assert_safe_entity_id("hookId", hook_id)?;
        let hook =
            read_json_with_fallback::<Hook>(&self.base_dir, "hooks", hook_id, self.tag.as_deref())?
                .ok_or_else(|| LocalWorldError::HookNotFound(hook_id.to_string()))?;
        Ok(strip_hook_data(hook, resolve_data))
    }

    pub fn hooks_get_by_token(&self, token: &str) -> Result<Hook> {
        let hooks_dir = self.base_dir.join("hooks");
        for file_id in list_files_by_extension(&hooks_dir, ".json")? {
            let Some(hook) = read_json::<Hook>(hooks_dir.join(format!("{file_id}.json")))? else {
                continue;
            };
            if hook.token == token {
                return Ok(strip_hook_data(hook, ResolveData::All));
            }
        }
        Err(LocalWorldError::HookNotFound(token.to_string()))
    }

    pub fn hooks_list(
        &self,
        run_id: Option<&str>,
        pagination: PaginationOptions,
        resolve_data: ResolveData,
    ) -> Result<PaginatedResponse<Hook>> {
        if let Some(run_id) = run_id {
            assert_safe_entity_id("runId", run_id)?;
        }
        let hooks_dir = self.base_dir.join("hooks");
        let mut hooks = Vec::new();
        for file_id in list_files_by_extension(&hooks_dir, ".json")? {
            let Some(hook) = read_json::<Hook>(hooks_dir.join(format!("{file_id}.json")))? else {
                continue;
            };
            if run_id.is_some_and(|run_id| hook.run_id != run_id) {
                continue;
            }
            hooks.push(strip_hook_data(hook, resolve_data));
        }
        paginate_items(
            hooks,
            &pagination,
            |hook| hook.created_at,
            |hook| hook.hook_id.clone(),
        )
    }

    pub fn events_create(
        &self,
        run_id: Option<&str>,
        event_type: &str,
        correlation_id: Option<&str>,
        event_data: Option<Value>,
        resolve_data: ResolveData,
    ) -> Result<EventResult> {
        if let Some(run_id) = run_id.filter(|value| !value.is_empty()) {
            assert_safe_entity_id("runId", run_id)?;
        }
        if let Some(correlation_id) = correlation_id {
            assert_safe_entity_id("correlationId", correlation_id)?;
        }
        let _guard = self.lock.lock().expect("local storage mutex poisoned");
        let event_id = next_monotonic_id("evnt");
        let created_at = now();
        let effective_run_id = if event_type == "run_created" && run_id.unwrap_or("").is_empty() {
            next_monotonic_id("wrun")
        } else {
            run_id
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    LocalWorldError::Message(
                        "runId is required for non-run_created events".to_string(),
                    )
                })?
                .to_string()
        };
        if event_type == "run_created" && run_id.is_some_and(|value| !value.is_empty()) {
            validate_ulid_timestamp(&effective_run_id, "wrun_")?;
        }

        let current_run = if event_type == "run_created" {
            None
        } else {
            read_json_with_fallback::<WorkflowRun>(
                &self.base_dir,
                "runs",
                &effective_run_id,
                self.tag.as_deref(),
            )?
        };

        if event_type == "run_failed" && current_run.is_none() {
            return Err(LocalWorldError::WorkflowRunNotFound(effective_run_id));
        }
        if let Some(run) = &current_run {
            if let Some(spec_version) = run.spec_version {
                if spec_version > world::SPEC_VERSION_CURRENT.get() {
                    return Err(LocalWorldError::RunNotSupported {
                        required: spec_version,
                        current: world::SPEC_VERSION_CURRENT.get(),
                    });
                }
            }
            if run.status.is_terminal() {
                match event_type {
                    "run_started" => {
                        return Err(LocalWorldError::RunExpired(format!(
                            "Workflow run \"{}\" is already in terminal state \"{}\"",
                            run.run_id,
                            status_string_run(&run.status)
                        )));
                    }
                    "run_cancelled" if run.status == WorkflowRunStatus::Cancelled => {}
                    "run_completed" | "run_failed" | "run_cancelled" => {
                        return Err(LocalWorldError::EntityConflict(format!(
                            "Cannot transition run from terminal state \"{}\"",
                            status_string_run(&run.status)
                        )));
                    }
                    "step_created" | "hook_created" | "wait_created" => {
                        return Err(LocalWorldError::EntityConflict(format!(
                            "Cannot create new entities on run in terminal state \"{}\"",
                            status_string_run(&run.status)
                        )));
                    }
                    _ => {}
                }
            }
        }

        let spec_version = Some(world::SPEC_VERSION_CURRENT.get());
        let mut event = Event {
            event_type: event_type.to_string(),
            correlation_id: correlation_id.map(ToOwned::to_owned),
            event_data: event_data.clone(),
            spec_version,
            run_id: effective_run_id.clone(),
            event_id,
            created_at,
        };
        let mut result = EventResult {
            event: None,
            run: None,
            step: None,
            hook: None,
            wait: None,
            events: None,
            cursor: None,
            has_more: None,
        };

        match event_type {
            "run_created" => {
                let data = value_object(event_data.as_ref());
                let run = WorkflowRun {
                    run_id: effective_run_id.clone(),
                    deployment_id: string_field(&data, "deploymentId")?,
                    workflow_name: string_field(&data, "workflowName")?,
                    status: WorkflowRunStatus::Pending,
                    spec_version,
                    execution_context: data.get("executionContext").cloned(),
                    input: data.get("input").cloned(),
                    output: None,
                    error: None,
                    error_code: None,
                    attributes: BTreeMap::new(),
                    expired_at: None,
                    started_at: None,
                    completed_at: None,
                    created_at,
                    updated_at: created_at,
                };
                let path = tagged_path(
                    &self.base_dir,
                    "runs",
                    &effective_run_id,
                    self.tag.as_deref(),
                )?;
                if !write_exclusive(path, serde_json::to_string_pretty(&run)?.as_bytes())? {
                    return Err(LocalWorldError::EntityConflict(format!(
                        "Workflow run \"{effective_run_id}\" already exists"
                    )));
                }
                result.run = Some(strip_run_data(run, resolve_data));
            }
            "run_started" => {
                let mut run = if let Some(run) = current_run {
                    if run.status == WorkflowRunStatus::Running {
                        result.run = Some(strip_run_data(run, resolve_data));
                        return Ok(result);
                    }
                    run
                } else {
                    let data = value_object(event_data.as_ref());
                    let run = WorkflowRun {
                        run_id: effective_run_id.clone(),
                        deployment_id: string_field(&data, "deploymentId")?,
                        workflow_name: string_field(&data, "workflowName")?,
                        status: WorkflowRunStatus::Pending,
                        spec_version,
                        execution_context: data.get("executionContext").cloned(),
                        input: data.get("input").cloned(),
                        output: None,
                        error: None,
                        error_code: None,
                        attributes: BTreeMap::new(),
                        expired_at: None,
                        started_at: None,
                        completed_at: None,
                        created_at,
                        updated_at: created_at,
                    };
                    let created_event = Event {
                        event_type: "run_created".to_string(),
                        correlation_id: None,
                        event_data: event_data.clone(),
                        spec_version,
                        run_id: effective_run_id.clone(),
                        event_id: next_monotonic_id("evnt"),
                        created_at,
                    };
                    write_json(
                        tagged_path(
                            &self.base_dir,
                            "events",
                            &format!("{}-{}", effective_run_id, created_event.event_id),
                            self.tag.as_deref(),
                        )?,
                        &created_event,
                        false,
                    )?;
                    run
                };
                event.event_data = None;
                run.status = WorkflowRunStatus::Running;
                run.started_at = Some(run.started_at.unwrap_or(created_at));
                run.updated_at = created_at;
                write_json(
                    tagged_path(
                        &self.base_dir,
                        "runs",
                        &effective_run_id,
                        self.tag.as_deref(),
                    )?,
                    &run,
                    true,
                )?;
                let events = self.events_list(ListByRunParams {
                    run_id: effective_run_id.clone(),
                    pagination: PaginationOptions {
                        limit: Some(1000),
                        cursor: None,
                        sort_order: SortOrder::Asc,
                    },
                    resolve_data,
                })?;
                result.cursor = events.cursor.clone();
                result.has_more = Some(events.has_more);
                result.events = Some(events.data);
                result.run = Some(strip_run_data(run, resolve_data));
            }
            "run_completed" => {
                let mut run = current_run.ok_or_else(|| {
                    LocalWorldError::WorkflowRunNotFound(effective_run_id.clone())
                })?;
                let data = value_object(event_data.as_ref());
                run.status = WorkflowRunStatus::Completed;
                run.output = data.get("output").cloned();
                run.error = None;
                run.error_code = None;
                run.completed_at = Some(created_at);
                run.updated_at = created_at;
                write_json(
                    tagged_path(
                        &self.base_dir,
                        "runs",
                        &effective_run_id,
                        self.tag.as_deref(),
                    )?,
                    &run,
                    true,
                )?;
                self.delete_all_hooks_for_run(&effective_run_id)?;
                self.delete_all_waits_for_run(&effective_run_id)?;
                result.run = Some(strip_run_data(run, resolve_data));
            }
            "run_failed" => {
                let mut run = current_run.ok_or_else(|| {
                    LocalWorldError::WorkflowRunNotFound(effective_run_id.clone())
                })?;
                let data = value_object(event_data.as_ref());
                run.status = WorkflowRunStatus::Failed;
                run.error = data.get("error").cloned();
                run.error_code = data
                    .get("errorCode")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                run.output = None;
                run.completed_at = Some(created_at);
                run.updated_at = created_at;
                write_json(
                    tagged_path(
                        &self.base_dir,
                        "runs",
                        &effective_run_id,
                        self.tag.as_deref(),
                    )?,
                    &run,
                    true,
                )?;
                self.delete_all_hooks_for_run(&effective_run_id)?;
                self.delete_all_waits_for_run(&effective_run_id)?;
                result.run = Some(strip_run_data(run, resolve_data));
            }
            "run_cancelled" => {
                let mut run = current_run.ok_or_else(|| {
                    LocalWorldError::WorkflowRunNotFound(effective_run_id.clone())
                })?;
                run.status = WorkflowRunStatus::Cancelled;
                run.output = None;
                run.error = None;
                run.completed_at = Some(run.completed_at.unwrap_or(created_at));
                run.updated_at = created_at;
                write_json(
                    tagged_path(
                        &self.base_dir,
                        "runs",
                        &effective_run_id,
                        self.tag.as_deref(),
                    )?,
                    &run,
                    true,
                )?;
                self.delete_all_hooks_for_run(&effective_run_id)?;
                self.delete_all_waits_for_run(&effective_run_id)?;
                result.run = Some(strip_run_data(run, resolve_data));
            }
            "step_created" => {
                let step_id = correlation_id.ok_or_else(|| {
                    LocalWorldError::Message("correlationId is required".to_string())
                })?;
                let data = value_object(event_data.as_ref());
                let step = Step {
                    run_id: effective_run_id.clone(),
                    step_id: step_id.to_string(),
                    step_name: string_field(&data, "stepName")?,
                    status: StepStatus::Pending,
                    input: data.get("input").cloned(),
                    output: None,
                    error: None,
                    attempt: 0,
                    started_at: None,
                    completed_at: None,
                    created_at,
                    updated_at: created_at,
                    retry_after: None,
                    spec_version,
                };
                let composite = format!("{effective_run_id}-{step_id}");
                let lock_path = resolve_within_base(
                    &self.base_dir,
                    &[".locks", "steps", &format!("{composite}.created")],
                )?;
                if !write_exclusive(lock_path, b"")? {
                    return Err(LocalWorldError::EntityConflict(format!(
                        "Step \"{step_id}\" already created"
                    )));
                }
                write_json(
                    tagged_path(&self.base_dir, "steps", &composite, self.tag.as_deref())?,
                    &step,
                    false,
                )?;
                result.step = Some(strip_step_data(step, resolve_data));
            }
            "step_started" | "step_completed" | "step_failed" | "step_retrying" => {
                let step_id = correlation_id.ok_or_else(|| {
                    LocalWorldError::Message("correlationId is required".to_string())
                })?;
                let mut step = self.steps_get(&effective_run_id, step_id, ResolveData::All)?;
                if step.status.is_terminal() {
                    return Err(LocalWorldError::EntityConflict(format!(
                        "Cannot modify step in terminal state \"{}\"",
                        status_string_step(&step.status)
                    )));
                }
                match event_type {
                    "step_started" => {
                        if let Some(retry_after) = step.retry_after {
                            if retry_after > created_at {
                                let retry_after_seconds =
                                    (retry_after - created_at).whole_seconds().max(0);
                                return Err(LocalWorldError::TooEarly {
                                    message: format!(
                                        "Cannot start step \"{step_id}\": retryAfter timestamp has not been reached yet"
                                    ),
                                    retry_after_seconds,
                                });
                            }
                        }
                        step.status = StepStatus::Running;
                        step.started_at = Some(step.started_at.unwrap_or(created_at));
                        step.attempt += 1;
                        step.retry_after = None;
                    }
                    "step_completed" => {
                        let data = value_object(event_data.as_ref());
                        step.status = StepStatus::Completed;
                        step.output = data.get("result").cloned();
                        step.completed_at = Some(created_at);
                    }
                    "step_failed" => {
                        let data = value_object(event_data.as_ref());
                        step.status = StepStatus::Failed;
                        step.error = data.get("error").cloned();
                        step.completed_at = Some(created_at);
                    }
                    "step_retrying" => {
                        let data = value_object(event_data.as_ref());
                        step.status = StepStatus::Pending;
                        step.error = data.get("error").cloned();
                        step.retry_after = data
                            .get("retryAfter")
                            .and_then(Value::as_str)
                            .and_then(|value| OffsetDateTime::parse(value, &Rfc3339).ok());
                    }
                    _ => {}
                }
                step.updated_at = created_at;
                write_json(
                    tagged_path(
                        &self.base_dir,
                        "steps",
                        &format!("{effective_run_id}-{step_id}"),
                        self.tag.as_deref(),
                    )?,
                    &step,
                    true,
                )?;
                result.step = Some(strip_step_data(step, resolve_data));
            }
            "hook_created" => {
                let hook_id = correlation_id.ok_or_else(|| {
                    LocalWorldError::Message("correlationId is required".to_string())
                })?;
                let data = value_object(event_data.as_ref());
                let token = string_field(&data, "token")?;
                let constraint_path = self
                    .base_dir
                    .join("hooks")
                    .join("tokens")
                    .join(format!("{}.json", hash_token(&token)));
                if !write_exclusive(
                    &constraint_path,
                    serde_json::to_string(&json!({
                        "token": token,
                        "hookId": hook_id,
                        "runId": effective_run_id,
                    }))?
                    .as_bytes(),
                )? {
                    let conflict_event = Event {
                        event_type: "hook_conflict".to_string(),
                        correlation_id: Some(hook_id.to_string()),
                        event_data: Some(json!({ "token": token })),
                        spec_version,
                        run_id: effective_run_id.clone(),
                        event_id: event.event_id.clone(),
                        created_at,
                    };
                    write_json(
                        tagged_path(
                            &self.base_dir,
                            "events",
                            &format!("{}-{}", effective_run_id, conflict_event.event_id),
                            self.tag.as_deref(),
                        )?,
                        &conflict_event,
                        false,
                    )?;
                    result.event = Some(strip_event_data_refs(conflict_event, resolve_data));
                    return Ok(result);
                }
                let hook = Hook {
                    run_id: effective_run_id.clone(),
                    hook_id: hook_id.to_string(),
                    token,
                    metadata: data.get("metadata").cloned(),
                    owner_id: "local-owner".to_string(),
                    project_id: "local-project".to_string(),
                    environment: "local".to_string(),
                    created_at,
                    spec_version,
                    is_webhook: data
                        .get("isWebhook")
                        .and_then(Value::as_bool)
                        .or(Some(false)),
                    is_system: data
                        .get("isSystem")
                        .and_then(Value::as_bool)
                        .or(Some(false)),
                };
                write_json(
                    tagged_path(&self.base_dir, "hooks", hook_id, self.tag.as_deref())?,
                    &hook,
                    false,
                )?;
                result.hook = Some(strip_hook_data(hook, resolve_data));
            }
            "hook_disposed" => {
                let hook_id = correlation_id.ok_or_else(|| {
                    LocalWorldError::Message("correlationId is required".to_string())
                })?;
                let hook = self.hooks_get(hook_id, ResolveData::All)?;
                let lock_path = resolve_within_base(
                    &self.base_dir,
                    &[".locks", "hooks", &format!("{hook_id}.disposed")],
                )?;
                if !write_exclusive(lock_path, b"")? {
                    return Err(LocalWorldError::EntityConflict(format!(
                        "Hook \"{hook_id}\" already disposed"
                    )));
                }
                delete_path(
                    self.base_dir
                        .join("hooks")
                        .join("tokens")
                        .join(format!("{}.json", hash_token(&hook.token))),
                )?;
                delete_path(tagged_path(
                    &self.base_dir,
                    "hooks",
                    hook_id,
                    self.tag.as_deref(),
                )?)?;
            }
            "hook_received" => {
                let hook_id = correlation_id.ok_or_else(|| {
                    LocalWorldError::Message("correlationId is required".to_string())
                })?;
                let _ = self.hooks_get(hook_id, ResolveData::All)?;
            }
            "wait_created" => {
                let correlation_id = correlation_id.ok_or_else(|| {
                    LocalWorldError::Message("correlationId is required".to_string())
                })?;
                let wait_id = format!("{effective_run_id}-{correlation_id}");
                let data = value_object(event_data.as_ref());
                let resume_at = data
                    .get("resumeAt")
                    .and_then(Value::as_str)
                    .and_then(|value| OffsetDateTime::parse(value, &Rfc3339).ok());
                let wait = Wait {
                    wait_id: wait_id.clone(),
                    run_id: effective_run_id.clone(),
                    status: WaitStatus::Waiting,
                    resume_at,
                    completed_at: None,
                    created_at,
                    updated_at: created_at,
                    spec_version,
                };
                let lock_path = resolve_within_base(
                    &self.base_dir,
                    &[".locks", "waits", &format!("{wait_id}.created")],
                )?;
                if !write_exclusive(lock_path, b"")? {
                    return Err(LocalWorldError::EntityConflict(format!(
                        "Wait \"{correlation_id}\" already exists"
                    )));
                }
                write_json(
                    tagged_path(&self.base_dir, "waits", &wait_id, self.tag.as_deref())?,
                    &wait,
                    false,
                )?;
                result.wait = Some(wait);
            }
            "wait_completed" => {
                let correlation_id = correlation_id.ok_or_else(|| {
                    LocalWorldError::Message("correlationId is required".to_string())
                })?;
                let wait_id = format!("{effective_run_id}-{correlation_id}");
                let mut wait = read_json_with_fallback::<Wait>(
                    &self.base_dir,
                    "waits",
                    &wait_id,
                    self.tag.as_deref(),
                )?
                .ok_or_else(|| {
                    LocalWorldError::Message(format!("Wait \"{correlation_id}\" not found"))
                })?;
                let lock_path = resolve_within_base(
                    &self.base_dir,
                    &[".locks", "waits", &format!("{wait_id}.completed")],
                )?;
                if !write_exclusive(lock_path, b"")? {
                    return Err(LocalWorldError::EntityConflict(format!(
                        "Wait \"{correlation_id}\" already completed"
                    )));
                }
                wait.status = WaitStatus::Completed;
                wait.completed_at = Some(created_at);
                wait.updated_at = created_at;
                write_json(
                    tagged_path(&self.base_dir, "waits", &wait_id, self.tag.as_deref())?,
                    &wait,
                    true,
                )?;
                result.wait = Some(wait);
            }
            other => {
                return Err(LocalWorldError::Message(format!(
                    "Unsupported event type: {other}"
                )));
            }
        }

        let composite_key = format!("{}-{}", effective_run_id, event.event_id);
        write_json(
            tagged_path(
                &self.base_dir,
                "events",
                &composite_key,
                self.tag.as_deref(),
            )?,
            &event,
            false,
        )?;
        result.event = Some(strip_event_data_refs(event, resolve_data));
        Ok(result)
    }

    pub fn events_get(
        &self,
        run_id: &str,
        event_id: &str,
        resolve_data: ResolveData,
    ) -> Result<Event> {
        assert_safe_entity_id("runId", run_id)?;
        assert_safe_entity_id("eventId", event_id)?;
        let composite = format!("{run_id}-{event_id}");
        let event = read_json_with_fallback::<Event>(
            &self.base_dir,
            "events",
            &composite,
            self.tag.as_deref(),
        )?
        .ok_or_else(|| {
            LocalWorldError::Message(format!("Event {event_id} in run {run_id} not found"))
        })?;
        Ok(strip_event_data_refs(event, resolve_data))
    }

    pub fn events_list(&self, params: ListByRunParams) -> Result<PaginatedResponse<Event>> {
        assert_safe_entity_id("runId", &params.run_id)?;
        let events_dir = self.base_dir.join("events");
        let prefix = format!("{}-", params.run_id);
        let mut events = Vec::new();
        for file_id in list_files_by_extension(&events_dir, ".json")? {
            if !file_id.starts_with(&prefix) {
                continue;
            }
            let Some(event) = read_json::<Event>(events_dir.join(format!("{file_id}.json")))?
            else {
                continue;
            };
            events.push(strip_event_data_refs(event, params.resolve_data));
        }
        paginate_items(
            events,
            &params.pagination,
            |event| event.created_at,
            |event| event.event_id.clone(),
        )
    }

    pub fn events_list_by_correlation_id(
        &self,
        correlation_id: &str,
        pagination: PaginationOptions,
        resolve_data: ResolveData,
    ) -> Result<PaginatedResponse<Event>> {
        assert_safe_entity_id("correlationId", correlation_id)?;
        let events_dir = self.base_dir.join("events");
        let mut events = Vec::new();
        for file_id in list_files_by_extension(&events_dir, ".json")? {
            let Some(event) = read_json::<Event>(events_dir.join(format!("{file_id}.json")))?
            else {
                continue;
            };
            if event.correlation_id.as_deref() == Some(correlation_id) {
                events.push(strip_event_data_refs(event, resolve_data));
            }
        }
        paginate_items(
            events,
            &pagination,
            |event| event.created_at,
            |event| event.event_id.clone(),
        )
    }

    fn delete_all_hooks_for_run(&self, run_id: &str) -> Result<()> {
        let hooks_dir = self.base_dir.join("hooks");
        for file_id in list_files_by_extension(&hooks_dir, ".json")? {
            let path = hooks_dir.join(format!("{file_id}.json"));
            let Some(hook) = read_json::<Hook>(&path)? else {
                continue;
            };
            if hook.run_id == run_id {
                delete_path(
                    hooks_dir
                        .join("tokens")
                        .join(format!("{}.json", hash_token(&hook.token))),
                )?;
                delete_path(path)?;
            }
        }
        Ok(())
    }

    fn delete_all_waits_for_run(&self, run_id: &str) -> Result<()> {
        let waits_dir = self.base_dir.join("waits");
        let prefix = format!("{run_id}-");
        for file_id in list_files_by_extension(&waits_dir, ".json")? {
            if file_id.starts_with(&prefix) {
                delete_path(waits_dir.join(format!("{file_id}.json")))?;
            }
        }
        Ok(())
    }
}

fn validate_attribute_changes(
    existing: &BTreeMap<String, String>,
    changes: &[AttributeChange],
    options: &AttributeOptions,
) -> Result<()> {
    const MAX_KEYS: usize = 100;
    const MAX_KEY_LEN: usize = 128;
    const MAX_VALUE_BYTES: usize = 4096;
    let mut next = existing.clone();
    for change in changes {
        if change.key.len() > MAX_KEY_LEN {
            return Err(LocalWorldError::Message(
                "attribute key exceeds max length".to_string(),
            ));
        }
        if change.key.starts_with('$') && !options.allow_reserved_attributes {
            return Err(LocalWorldError::Message(
                "attribute keys starting with '$' are reserved".to_string(),
            ));
        }
        if let Some(value) = &change.value {
            if value.len() > MAX_VALUE_BYTES {
                return Err(LocalWorldError::Message(
                    "attribute value exceeds byte limit".to_string(),
                ));
            }
            next.insert(change.key.clone(), value.clone());
        } else {
            next.remove(&change.key);
        }
    }
    if next.len() > MAX_KEYS {
        return Err(LocalWorldError::Message(
            "attribute count exceeds limit".to_string(),
        ));
    }
    Ok(())
}

fn string_field(map: &Map<String, Value>, key: &str) -> Result<String> {
    map.get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| LocalWorldError::Message(format!("missing string field {key}")))
}

fn validate_ulid_timestamp(run_id: &str, prefix: &str) -> Result<()> {
    if !run_id.starts_with(prefix) {
        return Ok(());
    }
    let Some(date) = ulid_to_date(run_id) else {
        return Ok(());
    };
    let delta = (now() - date).whole_days().abs();
    if delta > 3650 {
        return Err(LocalWorldError::Message(format!(
            "Invalid timestamp in {prefix} id"
        )));
    }
    Ok(())
}

/// Binary stream chunk compatible with upstream world-local.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chunk {
    pub eof: bool,
    pub chunk: Vec<u8>,
}

pub fn serialize_chunk(chunk: &Chunk) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(chunk.chunk.len() + 1);
    bytes.push(u8::from(chunk.eof));
    bytes.extend_from_slice(&chunk.chunk);
    bytes
}

pub fn is_eof_chunk(serialized: &[u8]) -> bool {
    serialized.first().copied() == Some(1)
}

pub fn deserialize_chunk(serialized: &[u8]) -> Chunk {
    Chunk {
        eof: is_eof_chunk(serialized),
        chunk: serialized.get(1..).unwrap_or_default().to_vec(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamChunk {
    pub index: usize,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamChunksResponse {
    pub data: Vec<StreamChunk>,
    pub cursor: Option<String>,
    pub has_more: bool,
    pub done: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamInfoResponse {
    pub tail_index: isize,
    pub done: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GetChunksOptions {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

/// Filesystem streamer.
#[derive(Clone, Debug)]
pub struct LocalStreamer {
    base_dir: PathBuf,
    tag: Option<String>,
}

impl LocalStreamer {
    pub fn new(base_dir: impl Into<PathBuf>, tag: Option<String>) -> Self {
        Self {
            base_dir: base_dir.into(),
            tag,
        }
    }

    pub fn write(&self, run_id: &str, name: &str, chunk: impl AsRef<[u8]>) -> Result<()> {
        self.write_internal(run_id, name, chunk.as_ref(), false)
    }

    pub fn write_multi<I, B>(&self, run_id: &str, name: &str, chunks: I) -> Result<()>
    where
        I: IntoIterator<Item = B>,
        B: AsRef<[u8]>,
    {
        for chunk in chunks {
            self.write(run_id, name, chunk)?;
        }
        Ok(())
    }

    pub fn close(&self, run_id: &str, name: &str) -> Result<()> {
        self.write_internal(run_id, name, &[], true)
    }

    pub fn list(&self, run_id: &str) -> Result<Vec<String>> {
        assert_safe_entity_id("runId", run_id)?;
        #[derive(Serialize, Deserialize)]
        struct RunStreams {
            streams: Vec<String>,
        }
        let data = read_json_with_fallback::<RunStreams>(
            &self.base_dir,
            "streams/runs",
            run_id,
            self.tag.as_deref(),
        )?;
        Ok(data.map(|data| data.streams).unwrap_or_default())
    }

    pub fn read_from(&self, _run_id: &str, name: &str, start_index: isize) -> Result<Vec<Vec<u8>>> {
        let files = self.chunk_files(name)?;
        let mut data_files = Vec::new();
        let mut done = false;
        for (_file_id, path) in files {
            let chunk = deserialize_chunk(&fs::read(path)?);
            if chunk.eof {
                done = true;
                break;
            }
            data_files.push(chunk.chunk);
        }
        let start = if start_index < 0 {
            data_files.len().saturating_sub(start_index.unsigned_abs())
        } else {
            start_index as usize
        };
        let start = start.min(data_files.len());
        let mut output = data_files.split_off(start);
        if !done {
            output.shrink_to_fit();
        }
        Ok(output)
    }

    pub fn get_chunks(
        &self,
        _run_id: &str,
        name: &str,
        options: GetChunksOptions,
    ) -> Result<StreamChunksResponse> {
        let limit = options.limit.unwrap_or(100);
        let start_index = options
            .cursor
            .as_ref()
            .and_then(|cursor| {
                base64::engine::general_purpose::STANDARD
                    .decode(cursor)
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .and_then(|text| text.parse::<usize>().ok())
            })
            .unwrap_or(0);
        let mut data_index = 0usize;
        let mut data = Vec::new();
        let mut done = false;
        let mut has_more = false;
        for (_file_id, path) in self.chunk_files(name)? {
            let chunk = deserialize_chunk(&fs::read(path)?);
            if chunk.eof {
                done = true;
                break;
            }
            if data_index < start_index {
                data_index += 1;
                continue;
            }
            if data.len() >= limit {
                has_more = true;
                break;
            }
            data.push(StreamChunk {
                index: data_index,
                data: chunk.chunk,
            });
            data_index += 1;
        }
        let cursor = if has_more {
            Some(
                base64::engine::general_purpose::STANDARD
                    .encode((start_index + data.len()).to_string()),
            )
        } else {
            None
        };
        Ok(StreamChunksResponse {
            data,
            cursor,
            has_more,
            done,
        })
    }

    pub fn get_info(&self, _run_id: &str, name: &str) -> Result<StreamInfoResponse> {
        let mut count = 0isize;
        let mut done = false;
        for (_file_id, path) in self.chunk_files(name)? {
            let mut file = File::open(path)?;
            let mut byte = [0u8; 1];
            let bytes_read = file.read(&mut byte)?;
            if bytes_read == 1 && byte[0] == 1 {
                done = true;
                break;
            }
            count += 1;
        }
        Ok(StreamInfoResponse {
            tail_index: count - 1,
            done,
        })
    }

    fn write_internal(&self, run_id: &str, name: &str, chunk: &[u8], eof: bool) -> Result<()> {
        assert_safe_entity_id("runId", run_id)?;
        assert_safe_entity_id("streamName", name)?;
        self.register_stream(run_id, name)?;
        let chunk_id = next_monotonic_id("chnk");
        let tag_suffix = self
            .tag
            .as_ref()
            .map(|tag| format!(".{tag}"))
            .unwrap_or_default();
        let path = self
            .base_dir
            .join("streams")
            .join("chunks")
            .join(format!("{name}-{chunk_id}{tag_suffix}.bin"));
        write_bytes(
            path,
            &serialize_chunk(&Chunk {
                eof,
                chunk: chunk.to_vec(),
            }),
            false,
        )
    }

    fn register_stream(&self, run_id: &str, name: &str) -> Result<()> {
        #[derive(Serialize, Deserialize)]
        struct RunStreams {
            streams: Vec<String>,
        }
        let path = tagged_path(&self.base_dir, "streams/runs", run_id, self.tag.as_deref())?;
        let mut data = read_json::<RunStreams>(&path)?.unwrap_or(RunStreams {
            streams: Vec::new(),
        });
        if !data.streams.iter().any(|stream| stream == name) {
            data.streams.push(name.to_string());
            write_json(path, &data, true)?;
        }
        Ok(())
    }

    fn chunk_files(&self, name: &str) -> Result<Vec<(String, PathBuf)>> {
        assert_safe_entity_id("streamName", name)?;
        let chunks_dir = self.base_dir.join("streams").join("chunks");
        let mut files = BTreeMap::<String, PathBuf>::new();
        match fs::read_dir(&chunks_dir) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry?;
                    if !entry.file_type()?.is_file() {
                        continue;
                    }
                    let filename = entry.file_name().to_string_lossy().to_string();
                    if !filename.starts_with(&format!("{name}-")) {
                        continue;
                    }
                    if let Some(tag) = &self.tag {
                        let tagged_suffix = format!(".{tag}.bin");
                        if let Some(file_id) = filename.strip_suffix(&tagged_suffix) {
                            files.insert(file_id.to_string(), entry.path());
                            continue;
                        }
                    }
                    if let Some(file_id) = filename.strip_suffix(".bin") {
                        files
                            .entry(file_id.to_string())
                            .or_insert_with(|| entry.path());
                    } else if let Some(file_id) = filename.strip_suffix(".json") {
                        files
                            .entry(file_id.to_string())
                            .or_insert_with(|| entry.path());
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(files.into_iter().collect())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueOptions {
    pub idempotency_key: Option<String>,
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueResponse {
    pub message_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueRequest {
    pub queue_name: String,
    pub message_id: String,
    pub attempt: u32,
    pub body: Value,
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueHandlerResult {
    pub timeout_seconds: Option<u64>,
    pub ok: bool,
}

type DirectHandler = Arc<dyn Fn(QueueRequest) -> Result<QueueHandlerResult> + Send + Sync>;

#[derive(Default)]
struct QueueState {
    direct_handlers: HashMap<String, DirectHandler>,
    inflight: HashMap<String, String>,
    deliveries: Vec<QueueRequest>,
}

/// Local queue with direct in-process handlers.
#[derive(Clone, Default)]
pub struct LocalQueue {
    state: Arc<Mutex<QueueState>>,
}

impl fmt::Debug for LocalQueue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("LocalQueue").finish_non_exhaustive()
    }
}

impl LocalQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_handler<F>(&self, prefix: &str, handler: F) -> Result<()>
    where
        F: Fn(QueueRequest) -> Result<QueueHandlerResult> + Send + Sync + 'static,
    {
        if prefix != "__wkf_step_" && prefix != "__wkf_workflow_" {
            return Err(LocalWorldError::Message(
                "Unknown queue name prefix".to_string(),
            ));
        }
        self.state
            .lock()
            .expect("local queue mutex poisoned")
            .direct_handlers
            .insert(prefix.to_string(), Arc::new(handler));
        Ok(())
    }

    pub fn queue(
        &self,
        queue_name: &str,
        message: Value,
        options: Option<QueueOptions>,
    ) -> Result<QueueResponse> {
        let prefix = queue_prefix(queue_name)?;
        let key = options
            .as_ref()
            .and_then(|options| options.idempotency_key.clone());
        if let Some(key) = &key {
            if let Some(existing) = self
                .state
                .lock()
                .expect("local queue mutex poisoned")
                .inflight
                .get(key)
                .cloned()
            {
                return Ok(QueueResponse {
                    message_id: existing,
                });
            }
        }
        let message_id = next_monotonic_id("msg");
        if let Some(key) = &key {
            self.state
                .lock()
                .expect("local queue mutex poisoned")
                .inflight
                .insert(key.clone(), message_id.clone());
        }
        let handler = self
            .state
            .lock()
            .expect("local queue mutex poisoned")
            .direct_handlers
            .get(prefix)
            .cloned();
        if let Some(handler) = handler {
            for attempt in 1..=256 {
                let request = QueueRequest {
                    queue_name: queue_name.to_string(),
                    message_id: message_id.clone(),
                    attempt,
                    body: message.clone(),
                    headers: options
                        .as_ref()
                        .map(|options| options.headers.clone())
                        .unwrap_or_default(),
                };
                self.state
                    .lock()
                    .expect("local queue mutex poisoned")
                    .deliveries
                    .push(request.clone());
                let result = handler(request)?;
                if let Some(timeout_seconds) = result.timeout_seconds {
                    if timeout_seconds > 0 {
                        thread::sleep(Duration::from_millis(timeout_seconds.min(1) * 10));
                    }
                    continue;
                }
                break;
            }
        }
        if let Some(key) = &key {
            self.state
                .lock()
                .expect("local queue mutex poisoned")
                .inflight
                .remove(key);
        }
        Ok(QueueResponse { message_id })
    }

    pub fn deliveries(&self) -> Vec<QueueRequest> {
        self.state
            .lock()
            .expect("local queue mutex poisoned")
            .deliveries
            .clone()
    }

    pub fn close(&self) -> Result<()> {
        Ok(())
    }
}

fn queue_prefix(queue_name: &str) -> Result<&'static str> {
    if queue_name.starts_with("__wkf_step_") {
        Ok("__wkf_step_")
    } else if queue_name.starts_with("__wkf_workflow_") {
        Ok("__wkf_workflow_")
    } else {
        Err(LocalWorldError::Message(
            "Unknown queue name prefix".to_string(),
        ))
    }
}

/// Local world composed from config, queue, storage, and streamer.
#[derive(Clone, Debug)]
pub struct LocalWorld {
    config: Config,
    queue: LocalQueue,
    storage: LocalStorage,
    streamer: LocalStreamer,
}

pub fn create_local_world(config: Config) -> LocalWorld {
    let tag = config.tag.clone();
    LocalWorld {
        storage: LocalStorage::new(config.data_dir.clone(), tag.clone()),
        streamer: LocalStreamer::new(config.data_dir.clone(), tag),
        queue: LocalQueue::new(),
        config,
    }
}

impl LocalWorld {
    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn queue(&self) -> &LocalQueue {
        &self.queue
    }

    pub fn storage(&self) -> &LocalStorage {
        &self.storage
    }

    pub fn streams(&self) -> &LocalStreamer {
        &self.streamer
    }

    pub fn register_handler<F>(&self, prefix: &str, handler: F) -> Result<()>
    where
        F: Fn(QueueRequest) -> Result<QueueHandlerResult> + Send + Sync + 'static,
    {
        self.queue.register_handler(prefix, handler)
    }

    pub fn recover_active_runs(&self) -> Result<usize> {
        let params = ListWorkflowRunsParams {
            status: None,
            file_id_filter_tag: self.config.tag.clone(),
            ..ListWorkflowRunsParams::default()
        };
        let runs = self.storage.runs_list(params)?;
        let mut recovered = 0usize;
        for run in runs.data {
            if matches!(
                run.status,
                WorkflowRunStatus::Pending | WorkflowRunStatus::Running
            ) {
                self.queue.queue(
                    &format!("__wkf_workflow_{}", run.workflow_name),
                    json!({
                        "runId": run.run_id,
                        "workflowName": run.workflow_name,
                    }),
                    None,
                )?;
                recovered += 1;
            }
        }
        Ok(recovered)
    }

    pub fn clear(&self) -> Result<()> {
        if let Some(tag) = &self.config.tag {
            for dir in ["runs", "steps", "events", "hooks", "waits", "streams/runs"] {
                let full_dir = self.config.data_dir.join(dir);
                for file in list_tagged_files(&full_dir, tag)? {
                    delete_path(full_dir.join(file))?;
                }
            }
            let chunks_dir = self.config.data_dir.join("streams").join("chunks");
            for file in list_tagged_files_by_extension(&chunks_dir, tag, ".bin")? {
                delete_path(chunks_dir.join(file))?;
            }
            let _ = fs::remove_dir_all(self.config.data_dir.join(".locks"));
        } else {
            let _ = fs::remove_dir_all(&self.config.data_dir);
            init_data_dir(&self.config.data_dir)?;
        }
        Ok(())
    }
}

impl LocalWorld {
    pub fn spec_version(&self) -> u32 {
        world::SPEC_VERSION_CURRENT.get()
    }

    pub fn start(&self) -> Result<()> {
        init_data_dir(&self.config.data_dir)?;
        if self.config.recover_active_runs {
            let _ = self.recover_active_runs()?;
        }
        Ok(())
    }

    pub fn close(&self) -> Result<()> {
        self.queue.close()
    }
}

impl world::ClearableWorld for LocalWorld {
    type Error = LocalWorldError;

    fn clear(&self) -> Result<()> {
        LocalWorld::clear(self)
    }
}

impl world::RecoverableWorld for LocalWorld {
    type Error = LocalWorldError;

    fn recover_active_runs(&self) -> Result<usize> {
        LocalWorld::recover_active_runs(self)
    }
}

/// Minimal trace result matching upstream's no-op-when-OTEL-missing behavior.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceStatus {
    Ok,
    Error(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceRecord {
    pub span_name: String,
    pub attributes: BTreeMap<String, String>,
    pub status: TraceStatus,
}

pub fn trace<T, F>(
    span_name: &str,
    attributes: BTreeMap<String, String>,
    fn_: F,
) -> (T, TraceRecord)
where
    F: FnOnce() -> T,
{
    let value = fn_();
    (
        value,
        TraceRecord {
            span_name: span_name.to_string(),
            attributes,
            status: TraceStatus::Ok,
        },
    )
}

pub fn peer_service(value: impl Into<String>) -> BTreeMap<String, String> {
    BTreeMap::from([("peer.service".to_string(), value.into())])
}

pub fn rpc_system(value: impl Into<String>) -> BTreeMap<String, String> {
    BTreeMap::from([("rpc.system".to_string(), value.into())])
}

pub fn rpc_service(value: impl Into<String>) -> BTreeMap<String, String> {
    BTreeMap::from([("rpc.service".to_string(), value.into())])
}

pub fn rpc_method(value: impl Into<String>) -> BTreeMap<String, String> {
    BTreeMap::from([("rpc.method".to_string(), value.into())])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_config() -> (TempDir, Config) {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = Config::new(temp_dir.path()).without_recovery();
        (temp_dir, config)
    }

    fn create_run(storage: &LocalStorage, run_id: &str) -> EventResult {
        storage
            .events_create(
                Some(run_id),
                "run_created",
                None,
                Some(json!({
                    "deploymentId": "deployment-123",
                    "workflowName": "workflow//./src/test//demo",
                    "input": { "hello": "world" }
                })),
                ResolveData::All,
            )
            .expect("create run")
    }

    #[test]
    fn records_world_local_source_snapshot() {
        assert_eq!(UPSTREAM_PACKAGE, "@workflow/world-local");
        assert_eq!(UPSTREAM_VERSION, "5.0.0-beta.11");
        assert_eq!(UPSTREAM_HEAD.len(), 40);
    }

    #[test]
    fn world_local_config_portable_parity() {
        let mut config = Config::new("data");
        config.base_url = Some("https://example.test".to_string());
        config.port = Some(4000);
        assert_eq!(
            resolve_base_url_from_parts(
                config.base_url.as_deref(),
                Some("https://env.test"),
                config.port,
                Some("5000"),
                Some(5173)
            )
            .unwrap(),
            "https://example.test"
        );
        assert_eq!(
            resolve_base_url_from_parts(None, Some("https://env.test"), Some(4000), None, None)
                .unwrap(),
            "https://env.test"
        );
        assert_eq!(
            resolve_base_url_from_parts(None, None, Some(0), Some("5000"), Some(5173)).unwrap(),
            "http://localhost:0"
        );
        assert_eq!(
            resolve_base_url_from_parts(None, None, None, Some("3000"), Some(5173)).unwrap(),
            "http://localhost:3000"
        );
        assert_eq!(
            resolve_base_url_from_parts(None, None, None, None, Some(5173)).unwrap(),
            "http://localhost:5173"
        );
        assert!(matches!(
            resolve_base_url_from_parts(None, None, None, None, None),
            Err(LocalWorldError::UnableToResolveBaseUrl)
        ));
    }

    #[test]
    fn world_local_init_data_dir_portable_parity() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let nested = temp_dir.path().join("a").join("b").join("workflow");
        ensure_data_dir(&nested).expect("ensure nested data dir");
        assert!(nested.is_dir());

        let parsed = parse_version("5.0.0-beta.11").expect("parse version");
        assert_eq!(parsed.major, 5);
        assert_eq!(parsed.prerelease.as_deref(), Some("beta.11"));
        assert_eq!(format_version(&parsed), "5.0.0-beta.11");
        let file = format_version_file("@workflow/world-local", &parsed);
        let (package, version) = parse_version_file(&file).expect("parse version file");
        assert_eq!(package, "@workflow/world-local");
        assert_eq!(version, parsed);
        assert!(parse_version("bad").is_err());

        init_data_dir(&nested).expect("init data dir");
        assert!(nested.join("version.txt").is_file());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let readonly = temp_dir.path().join("readonly");
            fs::create_dir(&readonly).expect("readonly dir");
            let original = fs::metadata(&readonly).unwrap().permissions();
            fs::set_permissions(&readonly, fs::Permissions::from_mode(0o555)).unwrap();
            let child = readonly.join("child");
            let result = ensure_data_dir(&child);
            fs::set_permissions(&readonly, original).unwrap();
            assert!(result.is_err());
        }
    }

    #[test]
    fn world_local_filesystem_portable_parity() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let safe_ids = [
            "wrun_01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "evnt_01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "step_0",
            "wrun_01ARZ3-step_01ARYY",
            "vitest-0",
            "strm_01ARZ3_user_bmFtZXNwYWNl",
            "a",
        ];
        for id in safe_ids {
            assert_safe_entity_id("test", id).expect("safe id accepted");
        }
        for id in [
            "",
            ".",
            "..",
            "../foo",
            "foo/bar",
            "foo\\bar",
            "/etc/passwd",
            ".hidden",
            "foo\0bar",
            "wrun_ABC.vitest-0",
        ] {
            assert!(matches!(
                assert_safe_entity_id("runId", id),
                Err(LocalWorldError::UnsafeEntityId { .. })
            ));
        }
        assert_eq!(strip_tag("wrun_ABC.vitest-0"), "wrun_ABC");
        assert_eq!(strip_tag("wrun_ABC.123"), "wrun_ABC.123");
        assert!(has_tag("wrun_ABC.vitest-0", "vitest-0"));
        let tagged = tagged_path(temp_dir.path(), "runs", "wrun_SAFE", Some("vitest-0")).unwrap();
        assert!(tagged.ends_with("runs/wrun_SAFE.vitest-0.json"));
        assert!(resolve_within_base(temp_dir.path(), &["runs", "../escape"]).is_err());

        let file = temp_dir.path().join("runs").join("item.json");
        write_json(&file, &json!({"createdAt":"2026-01-01T00:00:00Z"}), false).unwrap();
        assert!(write_json(&file, &json!({}), false).is_err());
        let data: Value = read_json(&file).unwrap().unwrap();
        assert_eq!(data["createdAt"], "2026-01-01T00:00:00Z");
        assert!(ulid_to_date("01ARZ3NDEKTSV4RRFFQ69G5FAV").is_some());
        assert_eq!(ulid_to_date("not-a-ulid"), None);
    }

    #[test]
    fn world_local_storage_event_log_portable_parity() {
        let (_temp_dir, config) = test_config();
        let world = create_local_world(config);
        world.start().unwrap();
        let storage = world.storage();
        let run_id = "wrun_01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let created = create_run(storage, run_id);
        assert_eq!(created.run.unwrap().status, WorkflowRunStatus::Pending);

        let started = storage
            .events_create(run_id.into(), "run_started", None, None, ResolveData::All)
            .unwrap();
        assert_eq!(started.run.unwrap().status, WorkflowRunStatus::Running);
        assert!(
            started
                .events
                .unwrap()
                .iter()
                .any(|event| event.event_type == "run_created")
        );

        let step_id = "step_0";
        storage
            .events_create(
                Some(run_id),
                "step_created",
                Some(step_id),
                Some(json!({"stepName":"step//./src/test//one","input":{"a":1}})),
                ResolveData::All,
            )
            .unwrap();
        let started_step = storage
            .events_create(
                Some(run_id),
                "step_started",
                Some(step_id),
                None,
                ResolveData::All,
            )
            .unwrap()
            .step
            .unwrap();
        assert_eq!(started_step.status, StepStatus::Running);
        assert_eq!(started_step.attempt, 1);
        let completed_step = storage
            .events_create(
                Some(run_id),
                "step_completed",
                Some(step_id),
                Some(json!({"result":{"ok":true}})),
                ResolveData::All,
            )
            .unwrap()
            .step
            .unwrap();
        assert_eq!(completed_step.status, StepStatus::Completed);
        assert!(
            storage
                .events_create(
                    Some(run_id),
                    "step_started",
                    Some(step_id),
                    None,
                    ResolveData::All,
                )
                .is_err()
        );

        let hook = storage
            .events_create(
                Some(run_id),
                "hook_created",
                Some("hook_1"),
                Some(json!({"token":"tok_1","metadata":{"source":"test"}})),
                ResolveData::All,
            )
            .unwrap()
            .hook
            .unwrap();
        assert_eq!(
            storage.hooks_get_by_token("tok_1").unwrap().hook_id,
            hook.hook_id
        );
        let conflict = storage
            .events_create(
                Some(run_id),
                "hook_created",
                Some("hook_2"),
                Some(json!({"token":"tok_1"})),
                ResolveData::All,
            )
            .unwrap();
        assert_eq!(conflict.event.unwrap().event_type, "hook_conflict");

        let resume_at = now().format(&Rfc3339).unwrap();
        let wait = storage
            .events_create(
                Some(run_id),
                "wait_created",
                Some("wait_1"),
                Some(json!({"resumeAt": resume_at})),
                ResolveData::All,
            )
            .unwrap()
            .wait
            .unwrap();
        assert_eq!(wait.status, WaitStatus::Waiting);
        let wait = storage
            .events_create(
                Some(run_id),
                "wait_completed",
                Some("wait_1"),
                None,
                ResolveData::All,
            )
            .unwrap()
            .wait
            .unwrap();
        assert_eq!(wait.status, WaitStatus::Completed);

        let completed = storage
            .events_create(
                Some(run_id),
                "run_completed",
                None,
                Some(json!({"output":{"done":true}})),
                ResolveData::All,
            )
            .unwrap()
            .run
            .unwrap();
        assert_eq!(completed.status, WorkflowRunStatus::Completed);
        assert!(
            storage
                .events_create(Some(run_id), "run_started", None, None, ResolveData::All)
                .is_err()
        );
        assert!(
            storage
                .runs_get("../../../package", ResolveData::All)
                .is_err()
        );
        assert!(
            storage
                .steps_list(ListByRunParams {
                    run_id: "../escape".to_string(),
                    pagination: PaginationOptions::default(),
                    resolve_data: ResolveData::All,
                })
                .is_err()
        );
    }

    #[test]
    fn world_local_run_attributes_portable_parity() {
        let (_temp_dir, config) = test_config();
        let world = create_local_world(config);
        world.start().unwrap();
        let storage = world.storage();
        let run_id = "wrun_01ARZ3NDEKTSV4RRFFQ69G5FAA";
        create_run(storage, run_id);
        let result = storage
            .experimental_set_attributes(
                run_id,
                &[
                    AttributeChange::set("env", "dev"),
                    AttributeChange::set("owner", "sdk"),
                ],
                AttributeOptions::default(),
            )
            .unwrap();
        assert_eq!(result.attributes["env"], "dev");
        let result = storage
            .experimental_set_attributes(
                run_id,
                &[
                    AttributeChange::set("env", "prod"),
                    AttributeChange::unset("owner"),
                ],
                AttributeOptions::default(),
            )
            .unwrap();
        assert_eq!(result.attributes["env"], "prod");
        assert!(!result.attributes.contains_key("owner"));
        assert!(
            storage
                .experimental_set_attributes(
                    "wrun_missing",
                    &[AttributeChange::set("a", "b")],
                    AttributeOptions::default(),
                )
                .is_err()
        );
        assert!(
            storage
                .experimental_set_attributes(
                    run_id,
                    &[AttributeChange::set("$reserved", "no")],
                    AttributeOptions::default(),
                )
                .is_err()
        );
        assert!(
            storage
                .experimental_set_attributes(
                    run_id,
                    &[AttributeChange::set("$reserved", "yes")],
                    AttributeOptions {
                        allow_reserved_attributes: true
                    },
                )
                .is_ok()
        );
    }

    #[test]
    fn world_local_queue_portable_parity() {
        let queue = LocalQueue::new();
        let attempts = Arc::new(Mutex::new(0u32));
        let attempts_for_handler = attempts.clone();
        queue
            .register_handler("__wkf_workflow_", move |request| {
                assert!(request.queue_name.starts_with("__wkf_workflow_"));
                let mut attempts = attempts_for_handler.lock().unwrap();
                *attempts += 1;
                Ok(QueueHandlerResult {
                    timeout_seconds: (*attempts == 1).then_some(0),
                    ok: true,
                })
            })
            .unwrap();
        let response = queue
            .queue(
                "__wkf_workflow_demo",
                json!({"runId":"wrun_1"}),
                Some(QueueOptions {
                    idempotency_key: Some("idem-1".to_string()),
                    headers: BTreeMap::new(),
                }),
            )
            .unwrap();
        assert!(response.message_id.starts_with("msg_"));
        assert_eq!(*attempts.lock().unwrap(), 2);
        assert_eq!(queue.deliveries().len(), 2);
        assert!(queue.queue("bad_prefix", json!({}), None).is_err());
    }

    #[test]
    fn world_local_reenqueue_active_runs_portable_parity() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let world = create_local_world(Config::new(temp_dir.path()));
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen_for_handler = seen.clone();
        world
            .register_handler("__wkf_workflow_", move |request| {
                seen_for_handler
                    .lock()
                    .unwrap()
                    .push(request.body["runId"].as_str().unwrap().to_string());
                Ok(QueueHandlerResult {
                    timeout_seconds: None,
                    ok: true,
                })
            })
            .unwrap();
        world.start().unwrap();
        create_run(world.storage(), "wrun_01ARZ3NDEKTSV4RRFFQ69G5FAB");
        world
            .storage()
            .events_create(
                Some("wrun_01ARZ3NDEKTSV4RRFFQ69G5FAB"),
                "run_started",
                None,
                None,
                ResolveData::All,
            )
            .unwrap();
        create_run(world.storage(), "wrun_01ARZ3NDEKTSV4RRFFQ69G5FAC");
        world
            .storage()
            .events_create(
                Some("wrun_01ARZ3NDEKTSV4RRFFQ69G5FAC"),
                "run_completed",
                None,
                Some(json!({"output":true})),
                ResolveData::All,
            )
            .unwrap();
        assert_eq!(world.recover_active_runs().unwrap(), 1);
        assert_eq!(
            *seen.lock().unwrap(),
            vec!["wrun_01ARZ3NDEKTSV4RRFFQ69G5FAB".to_string()]
        );
        assert_eq!(world.spec_version(), world::SPEC_VERSION_CURRENT.get());
        world.close().unwrap();
    }

    #[test]
    fn world_local_streamer_portable_parity() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let streamer = LocalStreamer::new(temp_dir.path(), None);
        let chunk = Chunk {
            eof: false,
            chunk: b"hello".to_vec(),
        };
        let serialized = serialize_chunk(&chunk);
        assert_eq!(serialized[0], 0);
        assert_eq!(deserialize_chunk(&serialized), chunk);
        assert!(is_eof_chunk(&serialize_chunk(&Chunk {
            eof: true,
            chunk: Vec::new(),
        })));

        let run_id = "wrun_01ARZ3NDEKTSV4RRFFQ69G5FAD";
        streamer.write(run_id, "output", "one").unwrap();
        streamer
            .write_multi(run_id, "output", ["two", "three"])
            .unwrap();
        streamer.close(run_id, "output").unwrap();
        assert_eq!(streamer.list(run_id).unwrap(), vec!["output".to_string()]);
        let data = streamer.read_from(run_id, "output", 0).unwrap();
        assert_eq!(
            data,
            vec![b"one".to_vec(), b"two".to_vec(), b"three".to_vec()]
        );
        assert_eq!(streamer.read_from(run_id, "output", -2).unwrap().len(), 2);
        let page = streamer
            .get_chunks(
                run_id,
                "output",
                GetChunksOptions {
                    limit: Some(2),
                    cursor: None,
                },
            )
            .unwrap();
        assert!(page.has_more);
        let second = streamer
            .get_chunks(
                run_id,
                "output",
                GetChunksOptions {
                    limit: Some(10),
                    cursor: page.cursor,
                },
            )
            .unwrap();
        assert_eq!(second.data.len(), 1);
        assert!(second.done);
        assert_eq!(
            streamer.get_info(run_id, "output").unwrap(),
            StreamInfoResponse {
                tail_index: 2,
                done: true
            }
        );
    }

    #[test]
    fn world_local_tagging_portable_parity() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let untagged = create_local_world(Config::new(temp_dir.path()).without_recovery());
        let tagged_a = create_local_world(
            Config::new(temp_dir.path())
                .with_tag("vitest-0")
                .without_recovery(),
        );
        let tagged_b = create_local_world(
            Config::new(temp_dir.path())
                .with_tag("vitest-1")
                .without_recovery(),
        );
        untagged.start().unwrap();
        tagged_a.start().unwrap();
        tagged_b.start().unwrap();

        create_run(untagged.storage(), "wrun_01ARZ3NDEKTSV4RRFFQ69G5FAE");
        create_run(tagged_a.storage(), "wrun_01ARZ3NDEKTSV4RRFFQ69G5FAF");
        create_run(tagged_b.storage(), "wrun_01ARZ3NDEKTSV4RRFFQ69G5FAG");

        assert!(
            tagged_a
                .storage()
                .runs_get("wrun_01ARZ3NDEKTSV4RRFFQ69G5FAE", ResolveData::All)
                .is_ok()
        );
        let runs = tagged_a
            .storage()
            .runs_list(ListWorkflowRunsParams {
                file_id_filter_tag: Some("vitest-0".to_string()),
                ..ListWorkflowRunsParams::default()
            })
            .unwrap();
        assert_eq!(runs.data.len(), 1);
        assert_eq!(runs.data[0].run_id, "wrun_01ARZ3NDEKTSV4RRFFQ69G5FAF");

        tagged_a.clear().unwrap();
        assert!(
            tagged_a
                .storage()
                .runs_get("wrun_01ARZ3NDEKTSV4RRFFQ69G5FAF", ResolveData::All)
                .is_err()
        );
        assert!(
            tagged_b
                .storage()
                .runs_get("wrun_01ARZ3NDEKTSV4RRFFQ69G5FAG", ResolveData::All)
                .is_ok()
        );
        assert!(
            untagged
                .storage()
                .runs_get("wrun_01ARZ3NDEKTSV4RRFFQ69G5FAE", ResolveData::All)
                .is_ok()
        );
    }

    #[test]
    fn world_local_telemetry_portable_parity() {
        let mut attrs = peer_service("workflow-local");
        attrs.extend(rpc_system("workflow"));
        attrs.extend(rpc_service("world-local"));
        attrs.extend(rpc_method("runs.get"));
        let (value, record) = trace("world.runs.get", attrs.clone(), || 42);
        assert_eq!(value, 42);
        assert_eq!(record.span_name, "world.runs.get");
        assert_eq!(record.status, TraceStatus::Ok);
        assert_eq!(record.attributes, attrs);
    }
}
