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
