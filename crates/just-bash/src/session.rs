use std::collections::BTreeMap;

use crate::error::{JustBashError, JustBashErrorKind, JustBashResult};
use crate::fs::{MkdirOptions, VirtualFileSystem};
use crate::path::resolve_path;

pub struct ExecOptions {
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub replace_env: bool,
}

/// Persistent backend state shared across exec calls.
#[derive(Clone, Debug)]
pub struct VirtualSession {
    fs: VirtualFileSystem,
    cwd: String,
    env: BTreeMap<String, String>,
}

impl Default for VirtualSession {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualSession {
    /// Creates a session rooted at `/home/user/project`.
    pub fn new() -> Self {
        let mut fs = VirtualFileSystem::new();
        fs.mkdir("/home/user/project", MkdirOptions { recursive: true })
            .expect("default project directory is valid");
        let mut env = BTreeMap::new();
        env.insert("PWD".to_string(), "/home/user/project".to_string());
        Self {
            fs,
            cwd: "/home/user/project".to_string(),
            env,
        }
    }

    /// Returns the persistent filesystem.
    pub fn fs(&self) -> &VirtualFileSystem {
        &self.fs
    }

    /// Returns the mutable persistent filesystem.
    pub fn fs_mut(&mut self) -> &mut VirtualFileSystem {
        &mut self.fs
    }

    /// Returns the current working directory.
    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// Returns the persistent environment.
    pub fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    /// Changes the persistent current working directory.
    pub fn chdir(&mut self, path: &str) -> JustBashResult<()> {
        let resolved = resolve_path(&self.cwd, path);
        let stat = self.fs.stat(&resolved)?;
        if !stat.is_directory {
            return Err(JustBashError::new(
                JustBashErrorKind::NotDirectory,
                "chdir",
                path,
                "not a directory",
            ));
        }
        self.cwd = self.fs.realpath(&resolved).unwrap_or(resolved);
        self.env.insert("PWD".to_string(), self.cwd.clone());
        Ok(())
    }

    /// Runs a closure with scoped cwd/env overrides. Filesystem mutations persist.
    pub fn with_exec_scope<T>(
        &mut self,
        options: ExecOptions,
        run: impl FnOnce(&mut VirtualSession) -> JustBashResult<T>,
    ) -> JustBashResult<T> {
        let previous_cwd = self.cwd.clone();
        let previous_env = self.env.clone();

        if options.replace_env {
            self.env.clear();
        }
        for (key, value) in options.env {
            self.env.insert(key, value);
        }
        if let Some(cwd) = options.cwd {
            let resolved = resolve_path(&previous_cwd, &cwd);
            let stat = self.fs.stat(&resolved)?;
            if !stat.is_directory {
                return Err(JustBashError::new(
                    JustBashErrorKind::NotDirectory,
                    "cwd",
                    cwd,
                    "not a directory",
                ));
            }
            self.cwd = resolved;
            self.env.insert("PWD".to_string(), self.cwd.clone());
        }

        let result = run(self);
        self.cwd = previous_cwd;
        self.env = previous_env;
        result
    }
}
