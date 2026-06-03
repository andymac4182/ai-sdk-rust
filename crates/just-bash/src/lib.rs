//! Portable Rust backend primitives for the Just Bash virtual filesystem.
//!
//! This crate intentionally models the in-memory and path/encoding contracts
//! without invoking a host shell or reading host filesystem paths.

#![forbid(unsafe_code)]

mod cli;
mod commands;
mod encoding;
mod error;
mod exec;
mod file_reader;
mod fs;
mod glob;
mod path;
mod runtime;
mod sanitize;
pub mod security;
mod session;
mod shell;

pub use cli::{
    JUST_BASH_CLI_HELP_OUTPUT, JUST_BASH_CLI_MOUNT_POINT, JUST_BASH_CLI_VERSION_OUTPUT,
    JustBashCliAction, JustBashCliError, JustBashCliOptions, JustBashCliPlan,
    JustBashCliScriptSource, format_just_bash_cli_json_result, plan_just_bash_cli_args,
};
pub use commands::{
    Builtin, CommandRegistry, UPSTREAM_COMMAND_REGISTRY, UPSTREAM_DEFAULT_COMMAND_NAMES,
};
pub use encoding::{
    BufferEncoding, ByteString, FileContent, OutputPayload, bytes_to_string, content_to_bytes,
};
pub use error::{JustBashError, JustBashErrorKind, JustBashResult};
pub use exec::{
    BashLogData, BashLogEntry, BashLogLevel, BashLogger, JUST_BASH_BACKEND,
    JUST_BASH_DEFAULT_MAX_OUTPUT_LENGTH, JUST_BASH_DEFAULT_TIMEOUT_MS, JUST_BASH_TIMEOUT_EXIT_CODE,
    JustBashCancelToken, JustBashCustomCommand, JustBashCustomCommandContext,
    JustBashCustomCommandResult, JustBashExecMetadata, JustBashExecOptions, JustBashExecResult,
    JustBashExecutor, JustBashExecutorTool, JustBashLanguageRuntime,
    JustBashLanguageRuntimeContext, JustBashLanguageRuntimeKind, JustBashSession,
    JustBashSessionOptions,
};
pub use file_reader::{
    ReadFileContent, ReadFilesOptions, ReadFilesResult, read_and_concat, read_files,
};
pub use fs::{
    CpOptions, DEFAULT_DIR_MODE, DEFAULT_FILE_MODE, DEFAULT_OVERLAY_MOUNT_POINT, DirentEntry,
    FileStat, MkdirOptions, MountableFileSystem, OverlayFileSystem, ReadWriteFileSystem, RmOptions,
    SYMLINK_MODE, SymlinkPolicy, VirtualFileSystem,
};
pub use glob::{GlobOptions, glob_paths, match_glob};
pub use path::{
    MAX_SYMLINK_DEPTH, SanitizedSymlinkTarget, dirname, is_path_within_root, join_path,
    normalize_path, resolve_path, resolve_symlink_target, sanitize_symlink_target, validate_path,
};
pub use runtime::{Bash, BashOptions};
pub use sanitize::{sanitize_error_message, sanitize_host_error_message};
pub use security::{
    AllowedUrlEntry, CancellationState, CommandSecurityPolicy, DiagnosticSeverity, DnsAddress,
    DnsLookupError, DnsResolver, HttpMethod, NetworkPolicy, NetworkRequest, NetworkResponse,
    NetworkTransport, PlannedNetworkRequest, RedactionPolicy, ResourceObservation,
    RuntimePortability, SecurityDiagnostic, SecurityDiagnosticCode, SecurityResult,
    SecurityViolationLog, StaticNetworkTransport, UpstreamRuntimeSurface, classify_runtime_surface,
    execute_network_request, is_private_hostname, is_url_allowed, matches_allow_list_entry,
    plan_network_request, validate_allow_list_entry, validate_workspace_path,
};
pub use session::{ExecOptions, VirtualSession};
pub use shell::*;

#[cfg(test)]
mod tests;
