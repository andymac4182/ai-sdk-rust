use std::fmt;

/// Result alias used by Just Bash backend operations.
pub type JustBashResult<T> = Result<T, JustBashError>;

/// POSIX-shaped error kinds emitted by the portable backend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JustBashErrorKind {
    NotFound,
    AlreadyExists,
    IsDirectory,
    NotDirectory,
    DirectoryNotEmpty,
    InvalidInput,
    SymlinkLoop,
    PermissionDenied,
    ReadOnly,
    Busy,
    CrossDevice,
}

impl JustBashErrorKind {
    fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "ENOENT",
            Self::AlreadyExists => "EEXIST",
            Self::IsDirectory => "EISDIR",
            Self::NotDirectory => "ENOTDIR",
            Self::DirectoryNotEmpty => "ENOTEMPTY",
            Self::InvalidInput => "EINVAL",
            Self::SymlinkLoop => "ELOOP",
            Self::PermissionDenied => "EPERM",
            Self::ReadOnly => "EROFS",
            Self::Busy => "EBUSY",
            Self::CrossDevice => "EXDEV",
        }
    }
}

/// Error value that keeps virtual path failures sanitized and deterministic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JustBashError {
    kind: JustBashErrorKind,
    operation: &'static str,
    path: String,
    detail: &'static str,
}

impl JustBashError {
    pub(crate) fn new(
        kind: JustBashErrorKind,
        operation: &'static str,
        path: impl Into<String>,
        detail: &'static str,
    ) -> Self {
        Self {
            kind,
            operation,
            path: path.into(),
            detail,
        }
    }

    /// Returns the POSIX-shaped error kind.
    pub const fn kind(&self) -> &JustBashErrorKind {
        &self.kind
    }

    /// Returns the virtual path associated with this error.
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl fmt::Display for JustBashError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {}, {} '{}'",
            self.kind.code(),
            self.detail,
            self.operation,
            self.path
        )
    }
}

impl std::error::Error for JustBashError {}
