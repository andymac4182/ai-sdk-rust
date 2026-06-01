//! Portable Rust backend primitives for the Just Bash virtual filesystem.
//!
//! This crate intentionally models the in-memory and path/encoding contracts
//! without invoking a host shell or reading host filesystem paths.

#![forbid(unsafe_code)]

mod encoding;
mod error;
mod exec;
mod file_reader;
mod fs;
mod glob;
mod path;
mod sanitize;
mod session;

pub use encoding::{
    BufferEncoding, ByteString, FileContent, OutputPayload, bytes_to_string, content_to_bytes,
};
pub use error::{JustBashError, JustBashErrorKind, JustBashResult};
pub use exec::{
    JUST_BASH_BACKEND, JUST_BASH_DEFAULT_MAX_OUTPUT_LENGTH, JUST_BASH_DEFAULT_TIMEOUT_MS,
    JUST_BASH_TIMEOUT_EXIT_CODE, JustBashCancelToken, JustBashExecMetadata, JustBashExecOptions,
    JustBashExecResult, JustBashExecutor, JustBashExecutorTool, JustBashSession,
    JustBashSessionOptions,
};
pub use file_reader::{
    ReadFileContent, ReadFilesOptions, ReadFilesResult, read_and_concat, read_files,
};
pub use fs::{
    CpOptions, DEFAULT_DIR_MODE, DEFAULT_FILE_MODE, DirentEntry, FileStat, MkdirOptions, RmOptions,
    SYMLINK_MODE, SymlinkPolicy, VirtualFileSystem,
};
pub use glob::{GlobOptions, glob_paths, match_glob};
pub use path::{
    MAX_SYMLINK_DEPTH, dirname, is_path_within_root, join_path, normalize_path, resolve_path,
    resolve_symlink_target, validate_path,
};
pub use sanitize::{sanitize_error_message, sanitize_host_error_message};
pub use session::{ExecOptions, VirtualSession};

#[cfg(test)]
mod tests;
