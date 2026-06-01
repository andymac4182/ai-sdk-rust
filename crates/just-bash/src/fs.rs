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
