use std::collections::{BTreeMap, BTreeSet};

use crate::encoding::{
    BufferEncoding, ByteString, FileContent, OutputPayload, bytes_to_string, content_to_bytes,
};
use crate::error::{JustBashError, JustBashErrorKind, JustBashResult};
use crate::path::{
    MAX_SYMLINK_DEPTH, dirname, join_path, normalize_path, resolve_path, resolve_symlink_target,
    validate_path,
};

/// Default directory mode used by the upstream in-memory filesystem.
pub const DEFAULT_DIR_MODE: u32 = 0o755;
/// Default file mode used by the upstream in-memory filesystem.
pub const DEFAULT_FILE_MODE: u32 = 0o644;
/// Default symlink mode used by the upstream in-memory filesystem.
pub const SYMLINK_MODE: u32 = 0o777;

#[derive(Clone, Debug, Eq, PartialEq)]
enum FsEntry {
    File(FileNode),
    Directory(DirectoryNode),
    Symlink(SymlinkNode),
}

impl FsEntry {
    const fn mode(&self) -> u32 {
        match self {
            Self::File(file) => file.mode,
            Self::Directory(directory) => directory.mode,
            Self::Symlink(symlink) => symlink.mode,
        }
    }

    fn set_mode(&mut self, mode: u32) {
        match self {
            Self::File(file) => file.mode = mode,
            Self::Directory(directory) => directory.mode = mode,
            Self::Symlink(symlink) => symlink.mode = mode,
        }
    }

    const fn mtime(&self) -> u64 {
        match self {
            Self::File(file) => file.mtime,
            Self::Directory(directory) => directory.mtime,
            Self::Symlink(symlink) => symlink.mtime,
        }
    }

    fn set_mtime(&mut self, mtime: u64) {
        match self {
            Self::File(file) => file.mtime = mtime,
            Self::Directory(directory) => directory.mtime = mtime,
            Self::Symlink(symlink) => symlink.mtime = mtime,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileNode {
    content: Vec<u8>,
    mode: u32,
    mtime: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectoryNode {
    mode: u32,
    mtime: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SymlinkNode {
    target: String,
    mode: u32,
    mtime: u64,
}

/// Directory entry with type information.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirentEntry {
    pub name: String,
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
}

/// Stat result from the virtual filesystem.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileStat {
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symbolic_link: bool,
    pub mode: u32,
    pub size: usize,
    pub mtime: u64,
}

/// Options for directory creation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MkdirOptions {
    pub recursive: bool,
}

/// Options for remove operations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RmOptions {
    pub recursive: bool,
    pub force: bool,
}

/// Options for copy operations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CpOptions {
    pub recursive: bool,
}

/// Symlink policy for virtual filesystem instances.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SymlinkPolicy {
    #[default]
    AllowVirtual,
    DenyCreation,
}

/// Default virtual mount point used by upstream OverlayFs.
pub const DEFAULT_OVERLAY_MOUNT_POINT: &str = "/home/user/project";

/// In-memory Just Bash filesystem with no host filesystem fallback.
#[derive(Clone, Debug)]
pub struct VirtualFileSystem {
    data: BTreeMap<String, FsEntry>,
    clock: u64,
    symlink_policy: SymlinkPolicy,
}

impl Default for VirtualFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualFileSystem {
    /// Creates an empty filesystem containing only `/`.
    pub fn new() -> Self {
        let mut data = BTreeMap::new();
        data.insert(
            "/".to_string(),
            FsEntry::Directory(DirectoryNode {
                mode: DEFAULT_DIR_MODE,
                mtime: 0,
            }),
        );
        Self {
            data,
            clock: 1,
            symlink_policy: SymlinkPolicy::AllowVirtual,
        }
    }

    /// Creates a filesystem that denies symlink creation.
    pub fn with_symlink_policy(mut self, policy: SymlinkPolicy) -> Self {
        self.symlink_policy = policy;
        self
    }

    /// Creates a filesystem with initial UTF-8 files.
    pub fn with_text_files(
        files: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        let mut fs = Self::new();
        for (path, content) in files {
            fs.write_file(&path.into(), content.into())
                .expect("initial text file path is valid");
        }
        fs
    }

    fn tick(&mut self) -> u64 {
        let value = self.clock;
        self.clock += 1;
        value
    }

    fn ensure_parent_dirs(&mut self, path: &str) {
        let parent = dirname(path);
        if parent == "/" {
            return;
        }
        if !self.data.contains_key(&parent) {
            self.ensure_parent_dirs(&parent);
            let mtime = self.tick();
            self.data.insert(
                parent,
                FsEntry::Directory(DirectoryNode {
                    mode: DEFAULT_DIR_MODE,
                    mtime,
                }),
            );
        }
    }

    /// Writes a file, creating parent directories when needed.
    pub fn write_file(
        &mut self,
        path: &str,
        content: impl Into<FileContent>,
    ) -> JustBashResult<()> {
        self.write_file_with_encoding(path, content, BufferEncoding::Utf8)
    }

    /// Writes a file using an explicit encoding.
    pub fn write_file_with_encoding(
        &mut self,
        path: &str,
        content: impl Into<FileContent>,
        encoding: BufferEncoding,
    ) -> JustBashResult<()> {
        validate_path(path, "write")?;
        let normalized = normalize_path(path);
        let bytes = content_to_bytes(content, encoding)?;
        self.ensure_parent_dirs(&normalized);
        let mtime = self.tick();
        self.data.insert(
            normalized,
            FsEntry::File(FileNode {
                content: bytes,
                mode: DEFAULT_FILE_MODE,
                mtime,
            }),
        );
        Ok(())
    }

    /// Appends to a file, creating it when absent and replacing final symlinks.
    pub fn append_file(
        &mut self,
        path: &str,
        content: impl Into<FileContent>,
    ) -> JustBashResult<()> {
        self.append_file_with_encoding(path, content, BufferEncoding::Utf8)
    }

    /// Appends to a file with an explicit encoding.
    pub fn append_file_with_encoding(
        &mut self,
        path: &str,
        content: impl Into<FileContent>,
        encoding: BufferEncoding,
    ) -> JustBashResult<()> {
        validate_path(path, "append")?;
        let normalized = normalize_path(path);
        if matches!(self.data.get(&normalized), Some(FsEntry::Directory(_))) {
            return Err(JustBashError::new(
                JustBashErrorKind::IsDirectory,
                "write",
                path,
                "illegal operation on a directory",
            ));
        }

        let new_bytes = content_to_bytes(content, encoding)?;
        let mtime = self.tick();
        if let Some(FsEntry::File(file)) = self.data.get_mut(&normalized) {
            file.content.extend(new_bytes);
            file.mtime = mtime;
        } else {
            self.write_file_with_encoding(
                path,
                FileContent::Bytes(new_bytes),
                BufferEncoding::Utf8,
            )?;
        }
        Ok(())
    }

    /// Reads a file as UTF-8 text by default.
    pub fn read_file(&self, path: &str) -> JustBashResult<String> {
        self.read_file_with_encoding(path, BufferEncoding::Utf8)
    }

    /// Reads a file using the requested encoding.
    pub fn read_file_with_encoding(
        &self,
        path: &str,
        encoding: BufferEncoding,
    ) -> JustBashResult<String> {
        Ok(bytes_to_string(&self.read_file_buffer(path)?, encoding))
    }

    /// Reads raw bytes from a file.
    pub fn read_file_buffer(&self, path: &str) -> JustBashResult<Vec<u8>> {
        validate_path(path, "open")?;
        let resolved = self.resolve_path_with_symlinks(path, true, "open")?;
        let Some(entry) = self.data.get(&resolved) else {
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "open",
                path,
                "no such file or directory",
            ));
        };
        match entry {
            FsEntry::File(file) => Ok(file.content.clone()),
            FsEntry::Directory(_) | FsEntry::Symlink(_) => Err(JustBashError::new(
                JustBashErrorKind::IsDirectory,
                "read",
                path,
                "illegal operation on a directory",
            )),
        }
    }

    /// Reads raw bytes in the pipeline `ByteString` shape.
    pub fn read_file_bytes(&self, path: &str) -> JustBashResult<ByteString> {
        self.read_file_buffer(path).map(ByteString::from_bytes)
    }

    /// Returns whether a path exists. Null-byte paths and symlink loops return false.
    pub fn exists(&self, path: &str) -> bool {
        if path.contains('\0') {
            return false;
        }
        self.resolve_path_with_symlinks(path, true, "open")
            .is_ok_and(|resolved| self.data.contains_key(&resolved))
    }

    /// Returns file or directory information, following final symlinks.
    pub fn stat(&self, path: &str) -> JustBashResult<FileStat> {
        validate_path(path, "stat")?;
        let resolved = self.resolve_path_with_symlinks(path, true, "stat")?;
        let Some(entry) = self.data.get(&resolved) else {
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "stat",
                path,
                "no such file or directory",
            ));
        };
        Ok(stat_for_entry(entry, false))
    }

    /// Returns file or directory information without following the final symlink.
    pub fn lstat(&self, path: &str) -> JustBashResult<FileStat> {
        validate_path(path, "lstat")?;
        let resolved = self.resolve_path_with_symlinks(path, false, "lstat")?;
        let Some(entry) = self.data.get(&resolved) else {
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "lstat",
                path,
                "no such file or directory",
            ));
        };
        Ok(stat_for_entry(entry, matches!(entry, FsEntry::Symlink(_))))
    }

    /// Creates a directory.
    pub fn mkdir(&mut self, path: &str, options: MkdirOptions) -> JustBashResult<()> {
        validate_path(path, "mkdir")?;
        let normalized = normalize_path(path);
        if let Some(entry) = self.data.get(&normalized) {
            if options.recursive && matches!(entry, FsEntry::Directory(_)) {
                return Ok(());
            }
            return Err(JustBashError::new(
                JustBashErrorKind::AlreadyExists,
                "mkdir",
                path,
                "file already exists",
            ));
        }

        let parent = dirname(&normalized);
        if parent != "/" && !self.data.contains_key(&parent) {
            if options.recursive {
                self.mkdir(&parent, MkdirOptions { recursive: true })?;
            } else {
                return Err(JustBashError::new(
                    JustBashErrorKind::NotFound,
                    "mkdir",
                    path,
                    "no such file or directory",
                ));
            }
        }
        if !matches!(self.data.get(&parent), Some(FsEntry::Directory(_))) {
            return Err(JustBashError::new(
                JustBashErrorKind::NotDirectory,
                "mkdir",
                path,
                "not a directory",
            ));
        }

        let mtime = self.tick();
        self.data.insert(
            normalized,
            FsEntry::Directory(DirectoryNode {
                mode: DEFAULT_DIR_MODE,
                mtime,
            }),
        );
        Ok(())
    }

    /// Reads directory child names.
    pub fn readdir(&self, path: &str) -> JustBashResult<Vec<String>> {
        Ok(self
            .readdir_with_file_types(path)?
            .into_iter()
            .map(|entry| entry.name)
            .collect())
    }

    /// Reads directory child names with entry types.
    pub fn readdir_with_file_types(&self, path: &str) -> JustBashResult<Vec<DirentEntry>> {
        validate_path(path, "scandir")?;
        let normalized = self.resolve_path_with_symlinks(path, true, "scandir")?;
        let Some(entry) = self.data.get(&normalized) else {
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "scandir",
                path,
                "no such file or directory",
            ));
        };
        if !matches!(entry, FsEntry::Directory(_)) {
            return Err(JustBashError::new(
                JustBashErrorKind::NotDirectory,
                "scandir",
                path,
                "not a directory",
            ));
        }

        let prefix = if normalized == "/" {
            "/".to_string()
        } else {
            format!("{normalized}/")
        };
        let mut names = BTreeMap::<String, DirentEntry>::new();
        for (candidate, fs_entry) in &self.data {
            if candidate == &normalized || !candidate.starts_with(&prefix) {
                continue;
            }
            let rest = &candidate[prefix.len()..];
            let Some(name) = rest.split('/').next().filter(|name| !name.is_empty()) else {
                continue;
            };
            if rest[name.len()..].contains('/') {
                continue;
            }
            names
                .entry(name.to_string())
                .or_insert_with(|| DirentEntry {
                    name: name.to_string(),
                    is_file: matches!(fs_entry, FsEntry::File(_)),
                    is_directory: matches!(fs_entry, FsEntry::Directory(_)),
                    is_symbolic_link: matches!(fs_entry, FsEntry::Symlink(_)),
                });
        }
        Ok(names.into_values().collect())
    }

    /// Removes a file, symlink, or directory.
    pub fn rm(&mut self, path: &str, options: RmOptions) -> JustBashResult<()> {
        validate_path(path, "rm")?;
        let normalized = normalize_path(path);
        let Some(entry) = self.data.get(&normalized) else {
            if options.force {
                return Ok(());
            }
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "rm",
                path,
                "no such file or directory",
            ));
        };

        if matches!(entry, FsEntry::Directory(_)) {
            let children = self.readdir(&normalized)?;
            if !children.is_empty() && !options.recursive {
                return Err(JustBashError::new(
                    JustBashErrorKind::DirectoryNotEmpty,
                    "rm",
                    path,
                    "directory not empty",
                ));
            }
            for child in children {
                self.rm(&join_path(&normalized, &child), options)?;
            }
        }
        self.data.remove(&normalized);
        Ok(())
    }

    /// Copies a file, symlink, or directory.
    pub fn cp(&mut self, src: &str, dest: &str, options: CpOptions) -> JustBashResult<()> {
        validate_path(src, "cp")?;
        validate_path(dest, "cp")?;
        let src_normalized = normalize_path(src);
        let dest_normalized = normalize_path(dest);
        let Some(src_entry) = self.data.get(&src_normalized).cloned() else {
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "cp",
                src,
                "no such file or directory",
            ));
        };

        match src_entry {
            FsEntry::File(mut file) => {
                self.ensure_parent_dirs(&dest_normalized);
                file.content = file.content.clone();
                file.mtime = self.tick();
                self.data.insert(dest_normalized, FsEntry::File(file));
            }
            FsEntry::Symlink(mut symlink) => {
                self.ensure_parent_dirs(&dest_normalized);
                symlink.mtime = self.tick();
                self.data.insert(dest_normalized, FsEntry::Symlink(symlink));
            }
            FsEntry::Directory(_) => {
                if !options.recursive {
                    return Err(JustBashError::new(
                        JustBashErrorKind::IsDirectory,
                        "cp",
                        src,
                        "is a directory",
                    ));
                }
                self.mkdir(&dest_normalized, MkdirOptions { recursive: true })?;
                let children = self.readdir(&src_normalized)?;
                for child in children {
                    self.cp(
                        &join_path(&src_normalized, &child),
                        &join_path(&dest_normalized, &child),
                        options,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Moves a file, symlink, or directory.
    pub fn mv(&mut self, src: &str, dest: &str) -> JustBashResult<()> {
        self.cp(src, dest, CpOptions { recursive: true })?;
        self.rm(
            src,
            RmOptions {
                recursive: true,
                force: false,
            },
        )
    }

    /// Returns all known virtual paths.
    pub fn get_all_paths(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }

    /// Resolves a path relative to a base directory.
    pub fn resolve_path(&self, base: &str, path: &str) -> String {
        resolve_path(base, path)
    }

    /// Changes file or directory mode.
    pub fn chmod(&mut self, path: &str, mode: u32) -> JustBashResult<()> {
        validate_path(path, "chmod")?;
        let normalized = normalize_path(path);
        let mtime = self.tick();
        let Some(entry) = self.data.get_mut(&normalized) else {
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "chmod",
                path,
                "no such file or directory",
            ));
        };
        entry.set_mode(mode);
        entry.set_mtime(mtime);
        Ok(())
    }

    /// Creates a virtual symlink.
    pub fn symlink(&mut self, target: &str, link_path: &str) -> JustBashResult<()> {
        if self.symlink_policy == SymlinkPolicy::DenyCreation {
            return Err(JustBashError::new(
                JustBashErrorKind::PermissionDenied,
                "symlink",
                link_path,
                "operation not permitted",
            ));
        }
        validate_path(target, "symlink")?;
        validate_path(link_path, "symlink")?;
        let normalized = normalize_path(link_path);
        if self.data.contains_key(&normalized) {
            return Err(JustBashError::new(
                JustBashErrorKind::AlreadyExists,
                "symlink",
                link_path,
                "file already exists",
            ));
        }
        self.ensure_parent_dirs(&normalized);
        let mtime = self.tick();
        self.data.insert(
            normalized,
            FsEntry::Symlink(SymlinkNode {
                target: target.to_string(),
                mode: SYMLINK_MODE,
                mtime,
            }),
        );
        Ok(())
    }

    /// Creates a hard-link-like file entry copy.
    pub fn link(&mut self, existing_path: &str, new_path: &str) -> JustBashResult<()> {
        validate_path(existing_path, "link")?;
        validate_path(new_path, "link")?;
        let existing_normalized = normalize_path(existing_path);
        let new_normalized = normalize_path(new_path);
        let Some(entry) = self.data.get(&existing_normalized).cloned() else {
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "link",
                existing_path,
                "no such file or directory",
            ));
        };
        let FsEntry::File(mut file) = entry else {
            return Err(JustBashError::new(
                JustBashErrorKind::PermissionDenied,
                "link",
                existing_path,
                "operation not permitted",
            ));
        };
        if self.data.contains_key(&new_normalized) {
            return Err(JustBashError::new(
                JustBashErrorKind::AlreadyExists,
                "link",
                new_path,
                "file already exists",
            ));
        }
        self.ensure_parent_dirs(&new_normalized);
        file.mtime = self.tick();
        self.data.insert(new_normalized, FsEntry::File(file));
        Ok(())
    }

    /// Reads a virtual symlink target without resolving it.
    pub fn readlink(&self, path: &str) -> JustBashResult<String> {
        validate_path(path, "readlink")?;
        let normalized = normalize_path(path);
        let Some(entry) = self.data.get(&normalized) else {
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "readlink",
                path,
                "no such file or directory",
            ));
        };
        match entry {
            FsEntry::Symlink(symlink) => Ok(symlink.target.clone()),
            FsEntry::File(_) | FsEntry::Directory(_) => Err(JustBashError::new(
                JustBashErrorKind::InvalidInput,
                "readlink",
                path,
                "invalid argument",
            )),
        }
    }

    /// Resolves all symlinks and verifies the resulting path exists.
    pub fn realpath(&self, path: &str) -> JustBashResult<String> {
        validate_path(path, "realpath")?;
        let resolved = self.resolve_path_with_symlinks(path, true, "realpath")?;
        if !self.data.contains_key(&resolved) {
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "realpath",
                path,
                "no such file or directory",
            ));
        }
        Ok(resolved)
    }

    /// Updates the modification time of a path.
    pub fn utimes(&mut self, path: &str, mtime: u64) -> JustBashResult<()> {
        validate_path(path, "utimes")?;
        let normalized = normalize_path(path);
        let Some(entry) = self.data.get_mut(&normalized) else {
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "utimes",
                path,
                "no such file or directory",
            ));
        };
        entry.set_mtime(mtime);
        Ok(())
    }

    /// Applies shell output redirection, writing text as UTF-8 and bytes verbatim.
    pub fn write_redirection(
        &mut self,
        cwd: &str,
        path: &str,
        payload: OutputPayload,
        append: bool,
    ) -> JustBashResult<()> {
        let resolved = resolve_path(cwd, path);
        let bytes = payload.into_bytes();
        if append {
            self.append_file_with_encoding(
                &resolved,
                FileContent::Bytes(bytes),
                BufferEncoding::Utf8,
            )
        } else {
            self.write_file_with_encoding(
                &resolved,
                FileContent::Bytes(bytes),
                BufferEncoding::Utf8,
            )
        }
    }

    /// Writes here-doc or here-string text as UTF-8 while preserving whitespace.
    pub fn write_here_doc(
        &mut self,
        cwd: &str,
        path: &str,
        content: &str,
        append: bool,
    ) -> JustBashResult<()> {
        self.write_redirection(cwd, path, OutputPayload::Text(content.to_string()), append)
    }

    fn resolve_path_with_symlinks(
        &self,
        path: &str,
        follow_final: bool,
        operation: &'static str,
    ) -> JustBashResult<String> {
        let normalized = normalize_path(path);
        self.resolve_normalized_path(&normalized, follow_final, operation, &mut BTreeSet::new())
    }

    fn resolve_normalized_path(
        &self,
        normalized: &str,
        follow_final: bool,
        operation: &'static str,
        seen: &mut BTreeSet<String>,
    ) -> JustBashResult<String> {
        if normalized == "/" {
            return Ok("/".to_string());
        }

        let parts: Vec<&str> = normalized.trim_start_matches('/').split('/').collect();
        let mut resolved = String::new();
        for (index, part) in parts.iter().enumerate() {
            resolved = if resolved.is_empty() {
                format!("/{part}")
            } else {
                join_path(&resolved, part)
            };
            let is_final = index == parts.len() - 1;
            if is_final && !follow_final {
                continue;
            }
            let Some(FsEntry::Symlink(symlink)) = self.data.get(&resolved) else {
                continue;
            };
            if seen.len() >= MAX_SYMLINK_DEPTH || !seen.insert(resolved.clone()) {
                return Err(JustBashError::new(
                    JustBashErrorKind::SymlinkLoop,
                    operation,
                    normalized,
                    "too many levels of symbolic links",
                ));
            }
            let target = resolve_symlink_target(&resolved, &symlink.target);
            let rest = parts[index + 1..].join("/");
            let next = if rest.is_empty() {
                target
            } else {
                join_path(&target, &rest)
            };
            return self.resolve_normalized_path(
                &normalize_path(&next),
                follow_final,
                operation,
                seen,
            );
        }
        Ok(if resolved.is_empty() {
            "/".to_string()
        } else {
            resolved
        })
    }
}

/// Read/write filesystem adapter that keeps upstream ReadWriteFs semantics
/// inside the deterministic virtual filesystem rather than the host disk.
#[derive(Clone, Debug, Default)]
pub struct ReadWriteFileSystem {
    inner: VirtualFileSystem,
}

impl ReadWriteFileSystem {
    /// Creates an empty read/write filesystem rooted at `/`.
    pub fn new() -> Self {
        Self {
            inner: VirtualFileSystem::new(),
        }
    }

    /// Creates a read/write filesystem with initial UTF-8 files.
    pub fn with_text_files(
        files: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self {
            inner: VirtualFileSystem::with_text_files(files),
        }
    }

    /// Returns the backing virtual filesystem.
    pub fn into_inner(self) -> VirtualFileSystem {
        self.inner
    }
}

impl std::ops::Deref for ReadWriteFileSystem {
    type Target = VirtualFileSystem;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for ReadWriteFileSystem {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Copy-on-write filesystem with a read-only lower virtual layer and a mutable
/// memory layer. It mirrors upstream OverlayFs precedence without host access.
#[derive(Clone, Debug)]
pub struct OverlayFileSystem {
    lower: VirtualFileSystem,
    upper: VirtualFileSystem,
    deleted: BTreeSet<String>,
    mount_point: String,
    read_only: bool,
}

impl OverlayFileSystem {
    /// Creates an overlay mounted at `/home/user/project`.
    pub fn new(lower: VirtualFileSystem) -> Self {
        Self::with_mount_point(lower, DEFAULT_OVERLAY_MOUNT_POINT)
            .expect("default overlay mount point is valid")
    }

    /// Creates an overlay mounted at a custom absolute virtual path.
    pub fn with_mount_point(
        lower: VirtualFileSystem,
        mount_point: impl AsRef<str>,
    ) -> JustBashResult<Self> {
        let mount_point = normalize_path(mount_point.as_ref());
        if !mount_point.starts_with('/') {
            return Err(JustBashError::new(
                JustBashErrorKind::InvalidInput,
                "mount",
                mount_point,
                "invalid mount point",
            ));
        }
        let mut overlay = Self {
            lower,
            upper: VirtualFileSystem::new(),
            deleted: BTreeSet::new(),
            mount_point,
            read_only: false,
        };
        overlay.create_mount_point_dirs()?;
        Ok(overlay)
    }

    /// Marks this overlay read-only for mutating operations.
    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Returns the virtual mount point.
    pub fn get_mount_point(&self) -> &str {
        &self.mount_point
    }

    /// Returns true when the overlay tombstone layer hides the path.
    pub fn is_deleted(&self, path: &str) -> bool {
        let normalized = normalize_path(path);
        self.deleted
            .iter()
            .any(|deleted| normalized == *deleted || normalized.starts_with(&format!("{deleted}/")))
    }

    /// Reads a file as UTF-8 text.
    pub fn read_file(&self, path: &str) -> JustBashResult<String> {
        self.read_file_with_encoding(path, BufferEncoding::Utf8)
    }

    /// Reads a file with the requested encoding.
    pub fn read_file_with_encoding(
        &self,
        path: &str,
        encoding: BufferEncoding,
    ) -> JustBashResult<String> {
        Ok(bytes_to_string(&self.read_file_buffer(path)?, encoding))
    }

    /// Reads raw file bytes, using the upper layer before the lower layer.
    pub fn read_file_buffer(&self, path: &str) -> JustBashResult<Vec<u8>> {
        validate_path(path, "open")?;
        let normalized = normalize_path(path);
        if self.is_deleted(&normalized) {
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "open",
                path,
                "no such file or directory",
            ));
        }
        if self.upper.exists(&normalized) {
            return self.upper.read_file_buffer(&normalized);
        }
        let Some(lower_path) = self.lower_path_for(&normalized) else {
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "open",
                path,
                "no such file or directory",
            ));
        };
        self.lower
            .read_file_buffer(&lower_path)
            .map_err(|error| overlay_error_for_virtual_path(error, path))
    }

    /// Writes a file to the upper memory layer.
    pub fn write_file(
        &mut self,
        path: &str,
        content: impl Into<FileContent>,
    ) -> JustBashResult<()> {
        self.write_file_with_encoding(path, content, BufferEncoding::Utf8)
    }

    /// Writes a file with an explicit encoding to the upper memory layer.
    pub fn write_file_with_encoding(
        &mut self,
        path: &str,
        content: impl Into<FileContent>,
        encoding: BufferEncoding,
    ) -> JustBashResult<()> {
        self.assert_writable("write", path)?;
        let normalized = normalize_path(path);
        self.upper
            .write_file_with_encoding(&normalized, content, encoding)?;
        self.deleted.remove(&normalized);
        Ok(())
    }

    /// Appends to a file in the upper memory layer, reading lower content first.
    pub fn append_file(
        &mut self,
        path: &str,
        content: impl Into<FileContent>,
    ) -> JustBashResult<()> {
        self.append_file_with_encoding(path, content, BufferEncoding::Utf8)
    }

    /// Appends to a file with an explicit encoding.
    pub fn append_file_with_encoding(
        &mut self,
        path: &str,
        content: impl Into<FileContent>,
        encoding: BufferEncoding,
    ) -> JustBashResult<()> {
        self.assert_writable("append", path)?;
        validate_path(path, "append")?;
        let normalized = normalize_path(path);
        let mut existing = self.read_file_buffer(&normalized).unwrap_or_default();
        existing.extend(content_to_bytes(content, encoding)?);
        self.upper.write_file_with_encoding(
            &normalized,
            FileContent::Bytes(existing),
            BufferEncoding::Utf8,
        )?;
        self.deleted.remove(&normalized);
        Ok(())
    }

    /// Returns true when a path exists in the merged view.
    pub fn exists(&self, path: &str) -> bool {
        if path.contains('\0') {
            return false;
        }
        let normalized = normalize_path(path);
        if self.is_deleted(&normalized) {
            return false;
        }
        self.upper.exists(&normalized)
            || self
                .lower_path_for(&normalized)
                .is_some_and(|lower_path| self.lower.exists(&lower_path))
    }

    /// Stats a path, following final symlinks.
    pub fn stat(&self, path: &str) -> JustBashResult<FileStat> {
        validate_path(path, "stat")?;
        let normalized = normalize_path(path);
        if self.is_deleted(&normalized) {
            return Err(not_found("stat", path));
        }
        if self.upper.exists(&normalized) {
            return self.upper.stat(&normalized);
        }
        let Some(lower_path) = self.lower_path_for(&normalized) else {
            return Err(not_found("stat", path));
        };
        self.lower
            .stat(&lower_path)
            .map_err(|error| overlay_error_for_virtual_path(error, path))
    }

    /// Stats a path without following the final symlink.
    pub fn lstat(&self, path: &str) -> JustBashResult<FileStat> {
        validate_path(path, "lstat")?;
        let normalized = normalize_path(path);
        if self.is_deleted(&normalized) {
            return Err(not_found("lstat", path));
        }
        if self.upper.exists(&normalized) {
            return self.upper.lstat(&normalized);
        }
        let Some(lower_path) = self.lower_path_for(&normalized) else {
            return Err(not_found("lstat", path));
        };
        self.lower
            .lstat(&lower_path)
            .map_err(|error| overlay_error_for_virtual_path(error, path))
    }

    /// Creates a directory in the upper layer.
    pub fn mkdir(&mut self, path: &str, options: MkdirOptions) -> JustBashResult<()> {
        self.assert_writable("mkdir", path)?;
        validate_path(path, "mkdir")?;
        let normalized = normalize_path(path);
        if self.exists(&normalized) {
            if options.recursive {
                return Ok(());
            }
            return Err(JustBashError::new(
                JustBashErrorKind::AlreadyExists,
                "mkdir",
                path,
                "file already exists",
            ));
        }
        let parent = dirname(&normalized);
        if parent != "/" && !self.exists(&parent) {
            if options.recursive {
                self.mkdir(&parent, MkdirOptions { recursive: true })?;
            } else {
                return Err(not_found("mkdir", path));
            }
        }
        self.upper.mkdir(&normalized, MkdirOptions::default())?;
        self.deleted.remove(&normalized);
        Ok(())
    }

    /// Reads merged directory child names.
    pub fn readdir(&self, path: &str) -> JustBashResult<Vec<String>> {
        Ok(self
            .readdir_with_file_types(path)?
            .into_iter()
            .map(|entry| entry.name)
            .collect())
    }

    /// Reads merged directory entries with file type metadata.
    pub fn readdir_with_file_types(&self, path: &str) -> JustBashResult<Vec<DirentEntry>> {
        validate_path(path, "scandir")?;
        let normalized = normalize_path(path);
        let stat = self.stat(&normalized)?;
        if !stat.is_directory {
            return Err(JustBashError::new(
                JustBashErrorKind::NotDirectory,
                "scandir",
                path,
                "not a directory",
            ));
        }

        let mut entries = BTreeMap::<String, DirentEntry>::new();
        let deleted_children = self.deleted_child_names(&normalized);

        if self.upper.exists(&normalized) {
            for entry in self.upper.readdir_with_file_types(&normalized)? {
                if !deleted_children.contains(&entry.name) {
                    entries.insert(entry.name.clone(), entry);
                }
            }
        }

        if let Some(lower_path) = self.lower_path_for(&normalized)
            && self.lower.exists(&lower_path)
            && self.lower.stat(&lower_path)?.is_directory
        {
            for entry in self.lower.readdir_with_file_types(&lower_path)? {
                if !deleted_children.contains(&entry.name) {
                    entries.entry(entry.name.clone()).or_insert(entry);
                }
            }
        }

        Ok(entries.into_values().collect())
    }

    /// Removes a path from the upper layer and tombstones lower-layer paths.
    pub fn rm(&mut self, path: &str, options: RmOptions) -> JustBashResult<()> {
        self.assert_writable("rm", path)?;
        validate_path(path, "rm")?;
        let normalized = normalize_path(path);
        if !self.exists(&normalized) {
            if options.force {
                return Ok(());
            }
            return Err(not_found("rm", path));
        }

        if self.stat(&normalized)?.is_directory {
            let children = self.readdir(&normalized)?;
            if !children.is_empty() && !options.recursive {
                return Err(JustBashError::new(
                    JustBashErrorKind::DirectoryNotEmpty,
                    "rm",
                    path,
                    "directory not empty",
                ));
            }
            for child in children {
                self.rm(&join_path(&normalized, &child), options)?;
            }
        }

        if self.upper.exists(&normalized) {
            self.upper.rm(
                &normalized,
                RmOptions {
                    recursive: true,
                    force: true,
                },
            )?;
        }
        if self.lower_exists(&normalized) {
            self.deleted.insert(normalized);
        }
        Ok(())
    }

    /// Copies a path into the upper layer.
    pub fn cp(&mut self, src: &str, dest: &str, options: CpOptions) -> JustBashResult<()> {
        self.assert_writable("cp", dest)?;
        validate_path(src, "cp")?;
        validate_path(dest, "cp")?;
        let src_normalized = normalize_path(src);
        let dest_normalized = normalize_path(dest);
        let stat = self.stat(&src_normalized)?;

        if stat.is_file {
            let content = self.read_file_buffer(&src_normalized)?;
            self.write_file_with_encoding(
                &dest_normalized,
                FileContent::Bytes(content),
                BufferEncoding::Utf8,
            )?;
            self.chmod(&dest_normalized, stat.mode)?;
        } else if stat.is_directory {
            if !options.recursive {
                return Err(JustBashError::new(
                    JustBashErrorKind::IsDirectory,
                    "cp",
                    src,
                    "is a directory",
                ));
            }
            self.mkdir(&dest_normalized, MkdirOptions { recursive: true })?;
            self.chmod(&dest_normalized, stat.mode)?;
            for child in self.readdir(&src_normalized)? {
                self.cp(
                    &join_path(&src_normalized, &child),
                    &join_path(&dest_normalized, &child),
                    options,
                )?;
            }
        } else if stat.is_symbolic_link {
            let target = self.readlink(&src_normalized)?;
            self.symlink(&target, &dest_normalized)?;
        }
        Ok(())
    }

    /// Moves a path into the upper layer and tombstones the source.
    pub fn mv(&mut self, src: &str, dest: &str) -> JustBashResult<()> {
        self.assert_writable("mv", dest)?;
        self.cp(src, dest, CpOptions { recursive: true })?;
        self.rm(
            src,
            RmOptions {
                recursive: true,
                force: false,
            },
        )
    }

    /// Changes mode in the upper layer, copying lower entries first.
    pub fn chmod(&mut self, path: &str, mode: u32) -> JustBashResult<()> {
        self.assert_writable("chmod", path)?;
        let normalized = normalize_path(path);
        if self.upper.exists(&normalized) {
            return self.upper.chmod(&normalized, mode);
        }
        let stat = self.stat(&normalized)?;
        if stat.is_file {
            let content = self.read_file_buffer(&normalized)?;
            self.upper.write_file_with_encoding(
                &normalized,
                FileContent::Bytes(content),
                BufferEncoding::Utf8,
            )?;
        } else if stat.is_directory {
            self.upper
                .mkdir(&normalized, MkdirOptions { recursive: true })?;
        } else if stat.is_symbolic_link {
            let target = self.readlink(&normalized)?;
            self.upper.symlink(&target, &normalized)?;
        }
        self.upper.chmod(&normalized, mode)
    }

    /// Creates a virtual symlink in the upper layer.
    pub fn symlink(&mut self, target: &str, link_path: &str) -> JustBashResult<()> {
        self.assert_writable("symlink", link_path)?;
        let normalized = normalize_path(link_path);
        if self.exists(&normalized) {
            return Err(JustBashError::new(
                JustBashErrorKind::AlreadyExists,
                "symlink",
                link_path,
                "file already exists",
            ));
        }
        self.upper.symlink(target, &normalized)?;
        self.deleted.remove(&normalized);
        Ok(())
    }

    /// Creates a hard-link-like copy in the upper layer.
    pub fn link(&mut self, existing_path: &str, new_path: &str) -> JustBashResult<()> {
        self.assert_writable("link", new_path)?;
        let stat = self.stat(existing_path)?;
        if !stat.is_file {
            return Err(JustBashError::new(
                JustBashErrorKind::PermissionDenied,
                "link",
                existing_path,
                "operation not permitted",
            ));
        }
        if self.exists(new_path) {
            return Err(JustBashError::new(
                JustBashErrorKind::AlreadyExists,
                "link",
                new_path,
                "file already exists",
            ));
        }
        let content = self.read_file_buffer(existing_path)?;
        self.write_file_with_encoding(new_path, FileContent::Bytes(content), BufferEncoding::Utf8)?;
        self.chmod(new_path, stat.mode)
    }

    /// Reads a symlink target from the upper layer or mapped lower layer.
    pub fn readlink(&self, path: &str) -> JustBashResult<String> {
        validate_path(path, "readlink")?;
        let normalized = normalize_path(path);
        if self.is_deleted(&normalized) {
            return Err(not_found("readlink", path));
        }
        if self.upper.exists(&normalized) {
            return self.upper.readlink(&normalized);
        }
        let Some(lower_path) = self.lower_path_for(&normalized) else {
            return Err(not_found("readlink", path));
        };
        self.lower
            .readlink(&lower_path)
            .map_err(|error| overlay_error_for_virtual_path(error, path))
    }

    /// Resolves a path in the merged overlay view.
    pub fn realpath(&self, path: &str) -> JustBashResult<String> {
        let normalized = normalize_path(path);
        if self.upper.exists(&normalized) {
            return self.upper.realpath(&normalized);
        }
        let Some(lower_path) = self.lower_path_for(&normalized) else {
            return Err(not_found("realpath", path));
        };
        let lower_realpath = self.lower.realpath(&lower_path)?;
        Ok(self.virtual_path_for_lower(&lower_realpath))
    }

    /// Resolves a relative path against a base path.
    pub fn resolve_path(&self, base: &str, path: &str) -> String {
        resolve_path(base, path)
    }

    /// Returns all paths visible through the overlay.
    pub fn get_all_paths(&self) -> Vec<String> {
        let mut paths = BTreeSet::new();
        for path in self.upper.get_all_paths() {
            if !self.is_deleted(&path) {
                paths.insert(path);
            }
        }
        for path in self.lower.get_all_paths() {
            let virtual_path = self.virtual_path_for_lower(&path);
            if !self.is_deleted(&virtual_path) {
                paths.insert(virtual_path);
            }
        }
        paths.into_iter().collect()
    }

    fn create_mount_point_dirs(&mut self) -> JustBashResult<()> {
        self.upper
            .mkdir(&self.mount_point, MkdirOptions { recursive: true })
    }

    fn assert_writable(&self, operation: &'static str, path: &str) -> JustBashResult<()> {
        if self.read_only {
            return Err(JustBashError::new(
                JustBashErrorKind::ReadOnly,
                operation,
                path,
                "read-only file system",
            ));
        }
        Ok(())
    }

    fn lower_path_for(&self, normalized_path: &str) -> Option<String> {
        if self.mount_point == "/" {
            return Some(normalized_path.to_string());
        }
        if normalized_path == self.mount_point {
            return Some("/".to_string());
        }
        normalized_path
            .strip_prefix(&format!("{}/", self.mount_point))
            .map(|suffix| format!("/{suffix}"))
    }

    fn virtual_path_for_lower(&self, lower_path: &str) -> String {
        let normalized = normalize_path(lower_path);
        if self.mount_point == "/" {
            normalized
        } else if normalized == "/" {
            self.mount_point.clone()
        } else {
            format!("{}{}", self.mount_point, normalized)
        }
    }

    fn lower_exists(&self, normalized_path: &str) -> bool {
        self.lower_path_for(normalized_path)
            .is_some_and(|lower_path| self.lower.exists(&lower_path))
    }

    fn deleted_child_names(&self, normalized_path: &str) -> BTreeSet<String> {
        let prefix = if normalized_path == "/" {
            "/".to_string()
        } else {
            format!("{normalized_path}/")
        };
        self.deleted
            .iter()
            .filter_map(|deleted| {
                deleted.strip_prefix(&prefix).and_then(|rest| {
                    let name = rest.split('/').next()?;
                    if name.is_empty() {
                        None
                    } else {
                        Some(name.to_string())
                    }
                })
            })
            .collect()
    }
}

/// Filesystem that routes paths through mount points and falls back to a base
/// virtual filesystem for unmounted paths.
#[derive(Clone, Debug)]
pub struct MountableFileSystem {
    base: VirtualFileSystem,
    mounts: BTreeMap<String, VirtualFileSystem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FsRoute {
    Base(String),
    Mount {
        mount_point: String,
        relative_path: String,
    },
}

impl Default for MountableFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl MountableFileSystem {
    /// Creates a mountable filesystem with an empty virtual base.
    pub fn new() -> Self {
        Self {
            base: VirtualFileSystem::new(),
            mounts: BTreeMap::new(),
        }
    }

    /// Creates a mountable filesystem with a provided virtual base.
    pub fn with_base(base: VirtualFileSystem) -> Self {
        Self {
            base,
            mounts: BTreeMap::new(),
        }
    }

    /// Mounts a virtual filesystem at a non-root absolute virtual path.
    pub fn mount(
        &mut self,
        mount_point: &str,
        filesystem: VirtualFileSystem,
    ) -> JustBashResult<()> {
        validate_mount_path(mount_point)?;
        let normalized = normalize_path(mount_point);
        self.validate_mount(&normalized)?;
        self.mounts.insert(normalized, filesystem);
        Ok(())
    }

    /// Unmounts a virtual filesystem.
    pub fn unmount(&mut self, mount_point: &str) -> JustBashResult<()> {
        let normalized = normalize_path(mount_point);
        if self.mounts.remove(&normalized).is_none() {
            return Err(JustBashError::new(
                JustBashErrorKind::NotFound,
                "unmount",
                mount_point,
                "no such file or directory",
            ));
        }
        Ok(())
    }

    /// Returns sorted mount point paths.
    pub fn get_mounts(&self) -> Vec<String> {
        self.mounts.keys().cloned().collect()
    }

    /// Returns true when a path is exactly a mount point.
    pub fn is_mount_point(&self, path: &str) -> bool {
        self.mounts.contains_key(&normalize_path(path))
    }

    /// Reads a file as UTF-8 text.
    pub fn read_file(&self, path: &str) -> JustBashResult<String> {
        self.read_file_with_encoding(path, BufferEncoding::Utf8)
    }

    /// Reads a file with the requested encoding.
    pub fn read_file_with_encoding(
        &self,
        path: &str,
        encoding: BufferEncoding,
    ) -> JustBashResult<String> {
        let route = self.route_path(path)?;
        match route {
            FsRoute::Base(relative_path) => {
                self.base.read_file_with_encoding(&relative_path, encoding)
            }
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => self
                .mounts
                .get(&mount_point)
                .expect("mount route points at an existing filesystem")
                .read_file_with_encoding(&relative_path, encoding),
        }
    }

    /// Reads raw file bytes.
    pub fn read_file_buffer(&self, path: &str) -> JustBashResult<Vec<u8>> {
        let route = self.route_path(path)?;
        match route {
            FsRoute::Base(relative_path) => self.base.read_file_buffer(&relative_path),
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => self
                .mounts
                .get(&mount_point)
                .expect("mount route points at an existing filesystem")
                .read_file_buffer(&relative_path),
        }
    }

    /// Writes a file to the routed filesystem.
    pub fn write_file(
        &mut self,
        path: &str,
        content: impl Into<FileContent>,
    ) -> JustBashResult<()> {
        self.write_file_with_encoding(path, content, BufferEncoding::Utf8)
    }

    /// Writes a file with explicit encoding.
    pub fn write_file_with_encoding(
        &mut self,
        path: &str,
        content: impl Into<FileContent>,
        encoding: BufferEncoding,
    ) -> JustBashResult<()> {
        let route = self.route_path(path)?;
        match route {
            FsRoute::Base(relative_path) => {
                self.base
                    .write_file_with_encoding(&relative_path, content, encoding)
            }
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => self
                .mounts
                .get_mut(&mount_point)
                .expect("mount route points at an existing filesystem")
                .write_file_with_encoding(&relative_path, content, encoding),
        }
    }

    /// Appends to a routed file.
    pub fn append_file(
        &mut self,
        path: &str,
        content: impl Into<FileContent>,
    ) -> JustBashResult<()> {
        self.append_file_with_encoding(path, content, BufferEncoding::Utf8)
    }

    /// Appends with explicit encoding.
    pub fn append_file_with_encoding(
        &mut self,
        path: &str,
        content: impl Into<FileContent>,
        encoding: BufferEncoding,
    ) -> JustBashResult<()> {
        let route = self.route_path(path)?;
        match route {
            FsRoute::Base(relative_path) => {
                self.base
                    .append_file_with_encoding(&relative_path, content, encoding)
            }
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => self
                .mounts
                .get_mut(&mount_point)
                .expect("mount route points at an existing filesystem")
                .append_file_with_encoding(&relative_path, content, encoding),
        }
    }

    /// Returns true when a routed path, mount point, or virtual mount parent exists.
    pub fn exists(&self, path: &str) -> bool {
        if path.contains('\0') {
            return false;
        }
        let normalized = normalize_path(path);
        if self.mounts.contains_key(&normalized) || !self.child_mount_points(&normalized).is_empty()
        {
            return true;
        }
        self.route_path(path).is_ok_and(|route| match route {
            FsRoute::Base(relative_path) => self.base.exists(&relative_path),
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => self
                .mounts
                .get(&mount_point)
                .expect("mount route points at an existing filesystem")
                .exists(&relative_path),
        })
    }

    /// Stats a routed path or synthetic mount directory.
    pub fn stat(&self, path: &str) -> JustBashResult<FileStat> {
        let normalized = normalize_path(path);
        if let Some(fs) = self.mounts.get(&normalized) {
            return fs.stat("/").or_else(|_| Ok(synthetic_directory_stat()));
        }
        if !self.child_mount_points(&normalized).is_empty() {
            return self
                .base
                .stat(&normalized)
                .or_else(|_| Ok(synthetic_directory_stat()));
        }
        let route = self.route_path(path)?;
        match route {
            FsRoute::Base(relative_path) => self.base.stat(&relative_path),
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => self
                .mounts
                .get(&mount_point)
                .expect("mount route points at an existing filesystem")
                .stat(&relative_path),
        }
    }

    /// Stats without following final symlinks.
    pub fn lstat(&self, path: &str) -> JustBashResult<FileStat> {
        let normalized = normalize_path(path);
        if let Some(fs) = self.mounts.get(&normalized) {
            return fs.lstat("/").or_else(|_| Ok(synthetic_directory_stat()));
        }
        if !self.child_mount_points(&normalized).is_empty() {
            return self
                .base
                .lstat(&normalized)
                .or_else(|_| Ok(synthetic_directory_stat()));
        }
        let route = self.route_path(path)?;
        match route {
            FsRoute::Base(relative_path) => self.base.lstat(&relative_path),
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => self
                .mounts
                .get(&mount_point)
                .expect("mount route points at an existing filesystem")
                .lstat(&relative_path),
        }
    }

    /// Creates a directory in the routed filesystem.
    pub fn mkdir(&mut self, path: &str, options: MkdirOptions) -> JustBashResult<()> {
        let normalized = normalize_path(path);
        if self.mounts.contains_key(&normalized) {
            if options.recursive {
                return Ok(());
            }
            return Err(JustBashError::new(
                JustBashErrorKind::AlreadyExists,
                "mkdir",
                path,
                "file already exists",
            ));
        }
        if !self.child_mount_points(&normalized).is_empty() && options.recursive {
            return Ok(());
        }
        let route = self.route_path(path)?;
        match route {
            FsRoute::Base(relative_path) => self.base.mkdir(&relative_path, options),
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => self
                .mounts
                .get_mut(&mount_point)
                .expect("mount route points at an existing filesystem")
                .mkdir(&relative_path, options),
        }
    }

    /// Reads routed directory child names plus child mount points.
    pub fn readdir(&self, path: &str) -> JustBashResult<Vec<String>> {
        let normalized = normalize_path(path);
        let mut entries = BTreeSet::<String>::new();
        let mut error = None;

        let route = self.route_path(path)?;
        let result = match route {
            FsRoute::Base(relative_path) => self.base.readdir(&relative_path),
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => self
                .mounts
                .get(&mount_point)
                .expect("mount route points at an existing filesystem")
                .readdir(&relative_path),
        };
        match result {
            Ok(names) => entries.extend(names),
            Err(err) if err.kind() == &JustBashErrorKind::NotFound => error = Some(err),
            Err(err) => return Err(err),
        }

        entries.extend(self.child_mount_points(&normalized));
        if entries.is_empty() && !self.mounts.contains_key(&normalized) {
            if let Some(error) = error {
                return Err(error);
            }
        }
        Ok(entries.into_iter().collect())
    }

    /// Removes a routed path unless it is a mount point or mount parent.
    pub fn rm(&mut self, path: &str, options: RmOptions) -> JustBashResult<()> {
        let normalized = normalize_path(path);
        if self.mounts.contains_key(&normalized) {
            return Err(busy("rm", path, "mount point"));
        }
        if !self.child_mount_points(&normalized).is_empty() {
            return Err(busy("rm", path, "contains mount points"));
        }
        let route = self.route_path(path)?;
        match route {
            FsRoute::Base(relative_path) => self.base.rm(&relative_path, options),
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => self
                .mounts
                .get_mut(&mount_point)
                .expect("mount route points at an existing filesystem")
                .rm(&relative_path, options),
        }
    }

    /// Copies paths within or across mounted filesystems.
    pub fn cp(&mut self, src: &str, dest: &str, options: CpOptions) -> JustBashResult<()> {
        let src_route = self.route_path(src)?;
        let dest_route = self.route_path(dest)?;
        if route_owner(&src_route) == route_owner(&dest_route) {
            return match src_route {
                FsRoute::Base(src_path) => {
                    let FsRoute::Base(dest_path) = dest_route else {
                        unreachable!("route owners matched")
                    };
                    self.base.cp(&src_path, &dest_path, options)
                }
                FsRoute::Mount {
                    mount_point,
                    relative_path,
                } => {
                    let FsRoute::Mount {
                        relative_path: dest_path,
                        ..
                    } = dest_route
                    else {
                        unreachable!("route owners matched")
                    };
                    self.mounts
                        .get_mut(&mount_point)
                        .expect("mount route points at an existing filesystem")
                        .cp(&relative_path, &dest_path, options)
                }
            };
        }
        self.cross_mount_copy(src, dest, options)
    }

    /// Moves paths within or across mounted filesystems.
    pub fn mv(&mut self, src: &str, dest: &str) -> JustBashResult<()> {
        let normalized = normalize_path(src);
        if self.mounts.contains_key(&normalized) {
            return Err(busy("mv", src, "mount point"));
        }
        let src_route = self.route_path(src)?;
        let dest_route = self.route_path(dest)?;
        if route_owner(&src_route) == route_owner(&dest_route) {
            return match src_route {
                FsRoute::Base(src_path) => {
                    let FsRoute::Base(dest_path) = dest_route else {
                        unreachable!("route owners matched")
                    };
                    self.base.mv(&src_path, &dest_path)
                }
                FsRoute::Mount {
                    mount_point,
                    relative_path,
                } => {
                    let FsRoute::Mount {
                        relative_path: dest_path,
                        ..
                    } = dest_route
                    else {
                        unreachable!("route owners matched")
                    };
                    self.mounts
                        .get_mut(&mount_point)
                        .expect("mount route points at an existing filesystem")
                        .mv(&relative_path, &dest_path)
                }
            };
        }
        self.cp(src, dest, CpOptions { recursive: true })?;
        self.rm(
            src,
            RmOptions {
                recursive: true,
                force: false,
            },
        )
    }

    /// Changes mode on a routed path.
    pub fn chmod(&mut self, path: &str, mode: u32) -> JustBashResult<()> {
        let normalized = normalize_path(path);
        if let Some(fs) = self.mounts.get_mut(&normalized) {
            return fs.chmod("/", mode);
        }
        let route = self.route_path(path)?;
        match route {
            FsRoute::Base(relative_path) => self.base.chmod(&relative_path, mode),
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => self
                .mounts
                .get_mut(&mount_point)
                .expect("mount route points at an existing filesystem")
                .chmod(&relative_path, mode),
        }
    }

    /// Creates a symlink in the routed filesystem.
    pub fn symlink(&mut self, target: &str, link_path: &str) -> JustBashResult<()> {
        let route = self.route_path(link_path)?;
        match route {
            FsRoute::Base(relative_path) => self.base.symlink(target, &relative_path),
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => self
                .mounts
                .get_mut(&mount_point)
                .expect("mount route points at an existing filesystem")
                .symlink(target, &relative_path),
        }
    }

    /// Creates a hard link within the same routed filesystem.
    pub fn link(&mut self, existing_path: &str, new_path: &str) -> JustBashResult<()> {
        let existing_route = self.route_path(existing_path)?;
        let new_route = self.route_path(new_path)?;
        if route_owner(&existing_route) != route_owner(&new_route) {
            return Err(JustBashError::new(
                JustBashErrorKind::CrossDevice,
                "link",
                new_path,
                "cross-device link not permitted",
            ));
        }
        match existing_route {
            FsRoute::Base(existing) => {
                let FsRoute::Base(new) = new_route else {
                    unreachable!("route owners matched")
                };
                self.base.link(&existing, &new)
            }
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => {
                let FsRoute::Mount {
                    relative_path: new, ..
                } = new_route
                else {
                    unreachable!("route owners matched")
                };
                self.mounts
                    .get_mut(&mount_point)
                    .expect("mount route points at an existing filesystem")
                    .link(&relative_path, &new)
            }
        }
    }

    /// Reads a symlink target from the routed filesystem.
    pub fn readlink(&self, path: &str) -> JustBashResult<String> {
        let route = self.route_path(path)?;
        match route {
            FsRoute::Base(relative_path) => self.base.readlink(&relative_path),
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => self
                .mounts
                .get(&mount_point)
                .expect("mount route points at an existing filesystem")
                .readlink(&relative_path),
        }
    }

    /// Resolves all symlinks and prefixes mounted realpaths.
    pub fn realpath(&self, path: &str) -> JustBashResult<String> {
        let normalized = normalize_path(path);
        if self.mounts.contains_key(&normalized) {
            return Ok(normalized);
        }
        let route = self.route_path(path)?;
        match route {
            FsRoute::Base(relative_path) => self.base.realpath(&relative_path),
            FsRoute::Mount {
                mount_point,
                relative_path,
            } => {
                let resolved = self
                    .mounts
                    .get(&mount_point)
                    .expect("mount route points at an existing filesystem")
                    .realpath(&relative_path)?;
                Ok(if resolved == "/" {
                    mount_point
                } else {
                    format!("{mount_point}{resolved}")
                })
            }
        }
    }

    /// Resolves a relative path against a base path.
    pub fn resolve_path(&self, base: &str, path: &str) -> String {
        resolve_path(base, path)
    }

    /// Returns all visible paths, prefixing mounted filesystem paths.
    pub fn get_all_paths(&self) -> Vec<String> {
        let mut paths = BTreeSet::<String>::new();
        paths.extend(self.base.get_all_paths());
        for (mount_point, fs) in &self.mounts {
            let parts: Vec<&str> = mount_point
                .split('/')
                .filter(|part| !part.is_empty())
                .collect();
            let mut current = String::new();
            for part in parts {
                current = format!("{current}/{part}");
                paths.insert(current.clone());
            }
            for path in fs.get_all_paths() {
                if path == "/" {
                    paths.insert(mount_point.clone());
                } else {
                    paths.insert(format!("{mount_point}{path}"));
                }
            }
        }
        paths.into_iter().collect()
    }

    fn validate_mount(&self, mount_point: &str) -> JustBashResult<()> {
        if mount_point == "/" {
            return Err(JustBashError::new(
                JustBashErrorKind::InvalidInput,
                "mount",
                mount_point,
                "cannot mount at root",
            ));
        }
        for existing in self.mounts.keys() {
            if existing == mount_point {
                continue;
            }
            if mount_point.starts_with(&format!("{existing}/"))
                || existing.starts_with(&format!("{mount_point}/"))
            {
                return Err(JustBashError::new(
                    JustBashErrorKind::InvalidInput,
                    "mount",
                    mount_point,
                    "nested mount points are not allowed",
                ));
            }
        }
        Ok(())
    }

    fn route_path(&self, path: &str) -> JustBashResult<FsRoute> {
        validate_path(path, "access")?;
        let normalized = normalize_path(path);
        let mut best_mount = None::<&String>;
        for mount_point in self.mounts.keys() {
            if normalized == *mount_point || normalized.starts_with(&format!("{mount_point}/")) {
                best_mount = match best_mount {
                    Some(current) if current.len() > mount_point.len() => Some(current),
                    _ => Some(mount_point),
                };
            }
        }
        if let Some(mount_point) = best_mount {
            let relative_path = if normalized == *mount_point {
                "/".to_string()
            } else {
                normalized[mount_point.len()..].to_string()
            };
            Ok(FsRoute::Mount {
                mount_point: mount_point.clone(),
                relative_path,
            })
        } else {
            Ok(FsRoute::Base(normalized))
        }
    }

    fn child_mount_points(&self, dir_path: &str) -> Vec<String> {
        let normalized = normalize_path(dir_path);
        let prefix = if normalized == "/" {
            "/".to_string()
        } else {
            format!("{normalized}/")
        };
        let mut children = BTreeSet::<String>::new();
        for mount_point in self.mounts.keys() {
            if let Some(rest) = mount_point.strip_prefix(&prefix) {
                if let Some(child) = rest.split('/').next().filter(|child| !child.is_empty()) {
                    children.insert(child.to_string());
                }
            }
        }
        children.into_iter().collect()
    }

    fn cross_mount_copy(
        &mut self,
        src: &str,
        dest: &str,
        options: CpOptions,
    ) -> JustBashResult<()> {
        let stat = self.lstat(src)?;
        if stat.is_file {
            let content = self.read_file_buffer(src)?;
            self.write_file_with_encoding(dest, FileContent::Bytes(content), BufferEncoding::Utf8)?;
            self.chmod(dest, stat.mode)?;
        } else if stat.is_directory {
            if !options.recursive {
                return Err(JustBashError::new(
                    JustBashErrorKind::IsDirectory,
                    "cp",
                    src,
                    "is a directory",
                ));
            }
            self.mkdir(dest, MkdirOptions { recursive: true })?;
            self.chmod(dest, stat.mode)?;
            for child in self.readdir(src)? {
                self.cross_mount_copy(&join_path(src, &child), &join_path(dest, &child), options)?;
            }
        } else if stat.is_symbolic_link {
            let target = self.readlink(src)?;
            self.symlink(&target, dest)?;
        }
        Ok(())
    }
}

fn not_found(operation: &'static str, path: &str) -> JustBashError {
    JustBashError::new(
        JustBashErrorKind::NotFound,
        operation,
        path,
        "no such file or directory",
    )
}

fn busy(operation: &'static str, path: &str, detail: &'static str) -> JustBashError {
    JustBashError::new(JustBashErrorKind::Busy, operation, path, detail)
}

fn overlay_error_for_virtual_path(error: JustBashError, path: &str) -> JustBashError {
    JustBashError::new(
        error.kind().clone(),
        "open",
        path,
        match error.kind() {
            JustBashErrorKind::NotFound => "no such file or directory",
            JustBashErrorKind::AlreadyExists => "file already exists",
            JustBashErrorKind::IsDirectory => "illegal operation on a directory",
            JustBashErrorKind::NotDirectory => "not a directory",
            JustBashErrorKind::DirectoryNotEmpty => "directory not empty",
            JustBashErrorKind::InvalidInput => "invalid argument",
            JustBashErrorKind::SymlinkLoop => "too many levels of symbolic links",
            JustBashErrorKind::PermissionDenied => "operation not permitted",
            JustBashErrorKind::ReadOnly => "read-only file system",
            JustBashErrorKind::Busy => "mount point",
            JustBashErrorKind::CrossDevice => "cross-device link not permitted",
        },
    )
}

fn synthetic_directory_stat() -> FileStat {
    FileStat {
        is_file: false,
        is_directory: true,
        is_symbolic_link: false,
        mode: DEFAULT_DIR_MODE,
        size: 0,
        mtime: 0,
    }
}

fn validate_mount_path(mount_point: &str) -> JustBashResult<()> {
    for segment in mount_point.split('/') {
        if segment == "." || segment == ".." {
            return Err(JustBashError::new(
                JustBashErrorKind::InvalidInput,
                "mount",
                mount_point,
                "contains '.' or '..' segments",
            ));
        }
    }
    Ok(())
}

fn route_owner(route: &FsRoute) -> String {
    match route {
        FsRoute::Base(_) => "/".to_string(),
        FsRoute::Mount { mount_point, .. } => mount_point.clone(),
    }
}

fn stat_for_entry(entry: &FsEntry, is_symlink: bool) -> FileStat {
    let size = match entry {
        FsEntry::File(file) => file.content.len(),
        FsEntry::Directory(_) => 0,
        FsEntry::Symlink(symlink) => symlink.target.len(),
    };
    FileStat {
        is_file: matches!(entry, FsEntry::File(_)) && !is_symlink,
        is_directory: matches!(entry, FsEntry::Directory(_)) && !is_symlink,
        is_symbolic_link: is_symlink,
        mode: entry.mode(),
        size,
        mtime: entry.mtime(),
    }
}
