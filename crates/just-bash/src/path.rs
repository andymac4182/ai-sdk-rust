/// Maximum symlink depth before the virtual filesystem reports `ELOOP`.
pub const MAX_SYMLINK_DEPTH: usize = 40;

use crate::error::{JustBashError, JustBashErrorKind, JustBashResult};

/// Normalize a virtual path by resolving `.` and `..` and clamping at `/`.
pub fn normalize_path(path: &str) -> String {
    if path.is_empty() || path == "/" {
        return "/".to_string();
    }

    let mut normalized = path;
    if path.ends_with('/') && path != "/" {
        normalized = &path[..path.len() - 1];
    }

    let mut parts = Vec::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop();
        } else {
            parts.push(part);
        }
    }

    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

/// Reject paths with embedded null bytes.
pub fn validate_path(path: &str, operation: &'static str) -> JustBashResult<()> {
    if path.contains('\0') {
        return Err(JustBashError::new(
            JustBashErrorKind::NotFound,
            operation,
            path,
            "path contains null byte",
        ));
    }
    Ok(())
}

/// Sanitized presentation of an upstream symlink target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SanitizedSymlinkTarget {
    /// The target stays within the virtual root and may be shown as a relative path.
    WithinRoot { relative_path: String },
    /// The target escapes the virtual root and must be reduced to a basename.
    OutsideRoot { safe_name: String },
}

/// Converts symlink targets to virtual, non-leaking display paths.
pub fn sanitize_symlink_target(raw_target: &str, canonical_root: &str) -> SanitizedSymlinkTarget {
    if !is_absolute_path(raw_target) {
        return SanitizedSymlinkTarget::WithinRoot {
            relative_path: raw_target.to_string(),
        };
    }

    let resolved = normalize_absolute_display_path(raw_target);
    if is_path_within_root(&resolved, canonical_root) {
        let relative_path = resolved
            .strip_prefix(canonical_root)
            .filter(|value| !value.is_empty())
            .unwrap_or("/")
            .replace('\\', "/");
        SanitizedSymlinkTarget::WithinRoot { relative_path }
    } else {
        SanitizedSymlinkTarget::OutsideRoot {
            safe_name: basename(raw_target),
        }
    }
}

/// Returns a normalized path resolved relative to `base`.
pub fn resolve_path(base: &str, path: &str) -> String {
    if path.starts_with('/') {
        normalize_path(path)
    } else if base == "/" {
        normalize_path(&format!("/{path}"))
    } else {
        normalize_path(&format!("{base}/{path}"))
    }
}

/// Returns whether `path` is equal to `root` or a path-boundary child of it.
pub fn is_path_within_root(path: &str, root: &str) -> bool {
    let path = path.replace('\\', "/");
    let root = root.replace('\\', "/");
    path == root || path.starts_with(&format!("{root}/"))
}

/// Returns a path's directory name.
pub fn dirname(path: &str) -> String {
    let normalized = normalize_path(path);
    if normalized == "/" {
        return normalized;
    }
    let Some(index) = normalized.rfind('/') else {
        return "/".to_string();
    };
    if index == 0 {
        "/".to_string()
    } else {
        normalized[..index].to_string()
    }
}

/// Joins a parent path with a child name.
pub fn join_path(parent: &str, child: &str) -> String {
    if parent == "/" {
        format!("/{child}")
    } else {
        format!("{parent}/{child}")
    }
}

/// Resolves a symlink target relative to the symlink location.
pub fn resolve_symlink_target(symlink_path: &str, target: &str) -> String {
    if target.starts_with('/') {
        normalize_path(target)
    } else {
        normalize_path(&join_path(&dirname(symlink_path), target))
    }
}

fn is_absolute_path(path: &str) -> bool {
    path.starts_with('/') || path.starts_with('\\') || path.as_bytes().get(1) == Some(&b':')
}

fn normalize_absolute_display_path(path: &str) -> String {
    if path.starts_with('/') {
        normalize_path(path)
    } else {
        path.replace('\\', "/")
    }
}

fn basename(path: &str) -> String {
    path.replace('\\', "/")
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("")
        .to_string()
}
