use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

/// Result type used by sandbox git helpers.
pub type GitResult<T> = Result<T, GitError>;

/// Error returned by sandbox-bound git operations.
#[derive(Debug)]
pub enum GitError {
    Io {
        operation: &'static str,
        source: io::Error,
    },
    InvalidBranchName(String),
    InvalidSandboxPath {
        path: PathBuf,
        sandbox_root: PathBuf,
    },
    InvalidRelativePath(PathBuf),
    CommandFailed {
        program: String,
        args: Vec<String>,
        status: Option<i32>,
        output: String,
    },
}

impl fmt::Display for GitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(formatter, "{operation} failed: {source}"),
            Self::InvalidBranchName(branch) => {
                write!(formatter, "invalid git branch name: {branch}")
            }
            Self::InvalidSandboxPath { path, sandbox_root } => write!(
                formatter,
                "path {} is outside sandbox root {}",
                path.display(),
                sandbox_root.display()
            ),
            Self::InvalidRelativePath(path) => {
                write!(
                    formatter,
                    "path must stay inside the sandbox: {}",
                    path.display()
                )
            }
            Self::CommandFailed {
                program,
                args,
                status,
                output,
            } => write!(
                formatter,
                "{program} {} failed with status {:?}: {output}",
                args.join(" "),
                status
            ),
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Redacts secrets from command output before returning errors or summaries.
#[derive(Debug, Clone, Default)]
pub struct GitRedactor {
    secrets: Vec<String>,
}

impl GitRedactor {
    /// Creates an empty redactor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a literal secret value to redact.
    pub fn with_secret(mut self, secret: impl Into<String>) -> Self {
        let secret = secret.into();
        if !secret.is_empty() && !self.secrets.iter().any(|existing| existing == &secret) {
            self.secrets.push(secret);
        }
        self
    }

    /// Redacts registered secrets and common HTTP URL credential userinfo.
    pub fn redact(&self, value: &str) -> String {
        let mut redacted = redact_url_userinfo(value);
        for secret in &self.secrets {
            redacted = redacted.replace(secret, "<redacted>");
        }
        redacted
    }
}

/// GitHub credentials for optional remote operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCredentials {
    token: String,
    username: Option<String>,
}

impl GitCredentials {
    /// Creates credentials from an in-memory token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            username: None,
        }
    }

    /// Adds a username for credential prompts that ask for one.
    pub fn with_username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    /// Loads a token from a local file path. Tests use this path to exercise the
    /// credential boundary without any live GitHub call.
    pub fn from_token_file(path: impl AsRef<Path>) -> GitResult<Self> {
        let token = fs::read_to_string(path.as_ref()).map_err(|source| GitError::Io {
            operation: "read git credential token",
            source,
        })?;
        Ok(Self::new(token.trim().to_string()))
    }

    /// Returns a redactor that masks this credential token.
    pub fn redactor(&self) -> GitRedactor {
        GitRedactor::new().with_secret(self.token.clone())
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    pub(crate) fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }
}

/// Mode for optional remote side effects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitRemoteActionMode {
    #[default]
    Disabled,
    DryRun,
    Execute,
}

/// Raw command output after redaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl GitOutput {
    fn from_output(output: Output, redactor: &GitRedactor) -> Self {
        Self {
            success: output.status.success(),
            exit_code: output.status.code(),
            stdout: redactor.redact(&String::from_utf8_lossy(&output.stdout)),
            stderr: redactor.redact(&String::from_utf8_lossy(&output.stderr)),
        }
    }

    /// Returns stderr when present, otherwise stdout.
    pub fn message(&self) -> String {
        let message = if self.stderr.trim().is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        message.trim().to_string()
    }
}

/// Status classification for one changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Untracked,
    Unknown(String),
}

/// File-level status or diff entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub status: FileChangeStatus,
    pub old_path: Option<String>,
}

/// Numeric diff statistics for one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffFileStat {
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileChangeStatus,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub binary: bool,
}

/// Summary of the working-tree diff against HEAD.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSummary {
    pub files_changed: usize,
    pub insertions: u64,
    pub deletions: u64,
    pub files: Vec<DiffFileStat>,
}

/// Current repository status inside the sandbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitStatus {
    pub branch: String,
    pub head_sha: String,
    pub is_dirty: bool,
    pub files: Vec<FileChange>,
}

/// Result from `git commit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitOutcome {
    pub committed: bool,
    pub commit_sha: Option<String>,
    pub commit_message: Option<String>,
}

/// Options for a push after committing.
#[derive(Debug, Clone)]
pub struct PushOptions {
    pub mode: GitRemoteActionMode,
    pub remote: String,
    pub branch: Option<String>,
    pub credentials: Option<GitCredentials>,
}

impl PushOptions {
    /// Creates disabled push options.
    pub fn disabled() -> Self {
        Self {
            mode: GitRemoteActionMode::Disabled,
            remote: "origin".to_string(),
            branch: None,
            credentials: None,
        }
    }

    /// Creates a dry-run push to origin.
    pub fn dry_run() -> Self {
        Self {
            mode: GitRemoteActionMode::DryRun,
            remote: "origin".to_string(),
            branch: None,
            credentials: None,
        }
    }
}

impl Default for PushOptions {
    fn default() -> Self {
        Self::disabled()
    }
}

/// Result from an optional push operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushOutcome {
    pub pushed: bool,
    pub dry_run: bool,
    pub remote: String,
    pub branch: String,
    pub output: String,
}

/// Result from a dry-run or executed `gh pr create` command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestCommandOutcome {
    pub created: bool,
    pub dry_run: bool,
    pub url: Option<String>,
    pub command: Vec<String>,
    pub output: Option<String>,
}

/// Sandbox-bound git repository wrapper.
#[derive(Debug, Clone)]
pub struct GitSandbox {
    sandbox_root: PathBuf,
    repository: PathBuf,
    redactor: GitRedactor,
}

impl GitSandbox {
    /// Opens an existing repository, verifying it is inside `sandbox_root`.
    pub fn open(sandbox_root: impl AsRef<Path>, repository: impl AsRef<Path>) -> GitResult<Self> {
        let sandbox_root =
            canonicalize_existing(sandbox_root.as_ref(), "canonicalize sandbox root")?;
        let repository = canonicalize_existing(repository.as_ref(), "canonicalize repository")?;
        ensure_inside(&sandbox_root, &repository)?;

        Ok(Self {
            sandbox_root,
            repository,
            redactor: GitRedactor::new(),
        })
    }

    /// Clones a repository under `sandbox_root` and returns an opened wrapper.
    pub fn clone_repo(
        sandbox_root: impl AsRef<Path>,
        remote_url: impl AsRef<OsStr>,
        destination: impl AsRef<Path>,
    ) -> GitResult<Self> {
        let sandbox_root =
            canonicalize_existing(sandbox_root.as_ref(), "canonicalize sandbox root")?;
        let destination = resolve_sandbox_child(&sandbox_root, destination.as_ref())?;
        let destination_arg = destination.as_os_str().to_owned();
        let args = vec![
            "clone".to_string(),
            os_to_string(remote_url.as_ref()),
            os_to_string(destination_arg.as_os_str()),
        ];
        let redactor = GitRedactor::new();
        let output = run_program("git", &args, Some(&sandbox_root), &redactor, None)?;
        if !output.success {
            return Err(command_failed("git", &args, output));
        }

        Self::open(&sandbox_root, destination)
    }

    /// Adds an extra redaction secret to this repository wrapper.
    pub fn with_redaction_secret(mut self, secret: impl Into<String>) -> Self {
        self.redactor = self.redactor.with_secret(secret);
        self
    }

    /// Sandbox root.
    pub fn sandbox_root(&self) -> &Path {
        &self.sandbox_root
    }

    /// Repository path.
    pub fn repository(&self) -> &Path {
        &self.repository
    }

    /// Creates a branch from the current HEAD.
    pub fn create_branch(&self, branch: &str) -> GitResult<()> {
        validate_branch(branch)?;
        self.git_checked(["checkout", "-b", branch].into_iter())?;
        Ok(())
    }

    /// Checks out an existing branch.
    pub fn checkout_branch(&self, branch: &str) -> GitResult<()> {
        validate_branch(branch)?;
        self.git_checked(["checkout", branch].into_iter())?;
        Ok(())
    }

    /// Returns the current branch, or `HEAD` for detached checkouts.
    pub fn current_branch(&self) -> GitResult<String> {
        let output = self.git(["symbolic-ref", "--short", "HEAD"].into_iter(), None)?;
        if output.success {
            let branch = output.stdout.trim();
            if branch.is_empty() {
                Ok("HEAD".to_string())
            } else {
                Ok(branch.to_string())
            }
        } else {
            Ok("HEAD".to_string())
        }
    }

    /// Returns the current HEAD SHA.
    pub fn head_sha(&self) -> GitResult<String> {
        let output = self.git_checked(["rev-parse", "HEAD"].into_iter())?;
        Ok(output.stdout.trim().to_string())
    }

    /// Returns current status using porcelain output.
    pub fn status(&self) -> GitResult<GitStatus> {
        let branch = self.current_branch()?;
        let head_sha = self.head_sha()?;
        let output = self
            .git_checked(["status", "--porcelain=v1", "-z", "--untracked-files=all"].into_iter())?;
        let files = parse_porcelain_status_z(&output.stdout);
        Ok(GitStatus {
            branch,
            head_sha,
            is_dirty: !files.is_empty(),
            files,
        })
    }

    /// Returns a diff summary for working-tree changes against HEAD.
    pub fn diff_summary(&self) -> GitResult<DiffSummary> {
        let changes_output =
            self.git_checked(["diff", "--name-status", "-z", "HEAD", "--"].into_iter())?;
        let stats_output = self.git_checked(["diff", "--numstat", "HEAD", "--"].into_iter())?;
        let mut changes = parse_name_status_z(&changes_output.stdout);
        let tracked_paths: std::collections::HashSet<String> =
            changes.iter().map(|change| change.path.clone()).collect();
        for change in self
            .status()?
            .files
            .into_iter()
            .filter(|change| matches!(change.status, FileChangeStatus::Untracked))
        {
            if !tracked_paths.contains(&change.path) {
                changes.push(change);
            }
        }

        Ok(build_diff_summary(
            &self.repository,
            changes,
            parse_numstat(&stats_output.stdout),
        ))
    }

    /// Stages all changes and creates a commit when there is anything staged.
    pub fn commit_all(&self, message: &str) -> GitResult<CommitOutcome> {
        let message = message.trim();
        if message.is_empty() {
            return Ok(CommitOutcome {
                committed: false,
                commit_sha: None,
                commit_message: None,
            });
        }

        if !self.status()?.is_dirty {
            return Ok(CommitOutcome {
                committed: false,
                commit_sha: None,
                commit_message: None,
            });
        }

        self.git_checked(["add", "-A"].into_iter())?;
        let staged = self.git(["diff", "--cached", "--quiet"].into_iter(), None)?;
        if staged.success {
            return Ok(CommitOutcome {
                committed: false,
                commit_sha: None,
                commit_message: None,
            });
        }

        self.git_checked(["commit", "-m", message].into_iter())?;
        Ok(CommitOutcome {
            committed: true,
            commit_sha: Some(self.head_sha()?),
            commit_message: Some(message.to_string()),
        })
    }

    /// Pushes the current branch, optionally as a dry run.
    pub fn push(&self, options: &PushOptions) -> GitResult<Option<PushOutcome>> {
        if options.mode == GitRemoteActionMode::Disabled {
            return Ok(None);
        }

        let branch = match options.branch.as_deref() {
            Some(branch) => {
                validate_branch(branch)?;
                branch.to_string()
            }
            None => self.current_branch()?,
        };
        validate_branch(&branch)?;
        if branch == "HEAD" {
            return Err(GitError::InvalidBranchName(branch));
        }

        let mut args = vec!["push".to_string()];
        if options.mode == GitRemoteActionMode::DryRun {
            args.push("--dry-run".to_string());
        }
        args.push(options.remote.clone());
        args.push(branch.clone());

        let output = self.git(
            args.iter().map(String::as_str),
            options.credentials.as_ref(),
        )?;
        if !output.success {
            return Err(command_failed("git", &args, output));
        }

        Ok(Some(PushOutcome {
            pushed: options.mode == GitRemoteActionMode::Execute,
            dry_run: options.mode == GitRemoteActionMode::DryRun,
            remote: options.remote.clone(),
            branch,
            output: output.message(),
        }))
    }

    /// Builds or executes a `gh pr create` command. Dry-run mode never calls
    /// GitHub and returns the exact argv that would be used.
    pub fn create_pull_request_command(
        &self,
        options: &crate::git_finish::PullRequestOptions,
    ) -> GitResult<Option<PullRequestCommandOutcome>> {
        if options.mode == GitRemoteActionMode::Disabled {
            return Ok(None);
        }

        let head = options
            .head
            .clone()
            .unwrap_or_else(|| self.current_branch().unwrap_or_else(|_| "HEAD".to_string()));
        validate_branch(&head)?;
        validate_branch(&options.base)?;

        let mut args = vec![
            "pr".to_string(),
            "create".to_string(),
            "--base".to_string(),
            options.base.clone(),
            "--head".to_string(),
            head,
            "--title".to_string(),
            options.title.clone(),
            "--body".to_string(),
            options.body.clone(),
        ];
        if let Some(repository) = &options.repository {
            args.push("--repo".to_string());
            args.push(repository.clone());
        }

        if options.mode == GitRemoteActionMode::DryRun {
            return Ok(Some(PullRequestCommandOutcome {
                created: false,
                dry_run: true,
                url: None,
                command: pr_command_with_program(&args),
                output: None,
            }));
        }

        let output = run_program(
            "gh",
            &args,
            Some(&self.repository),
            &self.redactor,
            options.credentials.as_ref(),
        )?;
        if !output.success {
            return Err(command_failed("gh", &args, output));
        }

        let url = output
            .stdout
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("http://") || line.starts_with("https://"))
            .map(str::to_string);

        Ok(Some(PullRequestCommandOutcome {
            created: true,
            dry_run: false,
            url,
            command: pr_command_with_program(&args),
            output: Some(output.message()),
        }))
    }

    fn git<'a>(
        &self,
        args: impl Iterator<Item = &'a str>,
        credentials: Option<&GitCredentials>,
    ) -> GitResult<GitOutput> {
        self.ensure_repository_inside_sandbox()?;
        let mut all_args = vec!["-C".to_string(), os_to_string(self.repository.as_os_str())];
        all_args.extend(args.map(str::to_string));
        let redactor = match credentials {
            Some(credentials) => self.redactor.clone().with_secret(credentials.token()),
            None => self.redactor.clone(),
        };
        run_program("git", &all_args, None, &redactor, credentials)
    }

    fn git_checked<'a>(&self, args: impl Iterator<Item = &'a str>) -> GitResult<GitOutput> {
        let args_vec: Vec<String> = args.map(str::to_string).collect();
        let output = self.git(args_vec.iter().map(String::as_str), None)?;
        if output.success {
            Ok(output)
        } else {
            Err(command_failed("git", &args_vec, output))
        }
    }

    fn ensure_repository_inside_sandbox(&self) -> GitResult<()> {
        let repository = canonicalize_existing(&self.repository, "canonicalize repository")?;
        ensure_inside(&self.sandbox_root, &repository)
    }
}

/// Branch-name predicate matching the Open Agents sandbox git helper.
pub fn is_safe_branch_name(branch: &str) -> bool {
    let mut chars = branch.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }

    branch
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/'))
        && !branch.contains("..")
        && !branch.contains("//")
        && !branch.ends_with('/')
        && !branch.ends_with(".lock")
}

fn validate_branch(branch: &str) -> GitResult<()> {
    if is_safe_branch_name(branch) {
        Ok(())
    } else {
        Err(GitError::InvalidBranchName(branch.to_string()))
    }
}

fn canonicalize_existing(path: &Path, operation: &'static str) -> GitResult<PathBuf> {
    path.canonicalize()
        .map_err(|source| GitError::Io { operation, source })
}

fn ensure_inside(sandbox_root: &Path, path: &Path) -> GitResult<()> {
    if path.starts_with(sandbox_root) {
        Ok(())
    } else {
        Err(GitError::InvalidSandboxPath {
            path: path.to_path_buf(),
            sandbox_root: sandbox_root.to_path_buf(),
        })
    }
}

fn resolve_sandbox_child(sandbox_root: &Path, child: &Path) -> GitResult<PathBuf> {
    if child.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(GitError::InvalidRelativePath(child.to_path_buf()));
    }
    Ok(sandbox_root.join(child))
}

fn run_program(
    program: &str,
    args: &[String],
    current_dir: Option<&Path>,
    redactor: &GitRedactor,
    credentials: Option<&GitCredentials>,
) -> GitResult<GitOutput> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(current_dir) = current_dir {
        command.current_dir(current_dir);
    }
    command.env("GIT_TERMINAL_PROMPT", "0");
    if let Some(credentials) = credentials {
        command.env("GITHUB_TOKEN", credentials.token());
        command.env("GH_TOKEN", credentials.token());
        if let Some(username) = credentials.username() {
            command.env("GIT_USERNAME", username);
        }
    }

    let output = command.output().map_err(|source| GitError::Io {
        operation: "run git command",
        source,
    })?;
    Ok(GitOutput::from_output(output, redactor))
}

fn command_failed(program: &str, args: &[String], output: GitOutput) -> GitError {
    GitError::CommandFailed {
        program: program.to_string(),
        args: args.to_vec(),
        status: output.exit_code,
        output: output.message(),
    }
}

fn os_to_string(value: &OsStr) -> String {
    value.to_string_lossy().into_owned()
}

fn parse_porcelain_status_z(output: &str) -> Vec<FileChange> {
    let parts: Vec<&str> = output.split('\0').filter(|part| !part.is_empty()).collect();
    let mut changes = Vec::new();
    let mut index = 0;

    while index < parts.len() {
        let record = parts[index];
        let bytes = record.as_bytes();
        if bytes.len() < 3 {
            index += 1;
            continue;
        }

        let status = status_from_xy(bytes[0] as char, bytes[1] as char);
        let path = record.get(3..).unwrap_or_default().to_string();
        let old_path = if matches!(status, FileChangeStatus::Renamed | FileChangeStatus::Copied) {
            index += 1;
            parts.get(index).map(|value| (*value).to_string())
        } else {
            None
        };

        changes.push(FileChange {
            path,
            status,
            old_path,
        });
        index += 1;
    }

    changes
}

fn parse_name_status_z(output: &str) -> Vec<FileChange> {
    let parts: Vec<&str> = output.split('\0').filter(|part| !part.is_empty()).collect();
    let mut changes = Vec::new();
    let mut index = 0;

    while index < parts.len() {
        let status_field = parts[index];
        let status_char = status_field.chars().next().unwrap_or('M');
        let status = status_from_code(status_char);

        if matches!(status, FileChangeStatus::Renamed | FileChangeStatus::Copied) {
            let old_path = parts.get(index + 1).map(|value| (*value).to_string());
            let path = parts
                .get(index + 2)
                .copied()
                .unwrap_or_default()
                .to_string();
            changes.push(FileChange {
                path,
                status,
                old_path,
            });
            index += 3;
        } else {
            let path = parts
                .get(index + 1)
                .copied()
                .unwrap_or_default()
                .to_string();
            changes.push(FileChange {
                path,
                status,
                old_path: None,
            });
            index += 2;
        }
    }

    changes
}

fn parse_numstat(output: &str) -> Vec<(String, Option<u64>, Option<u64>, bool)> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let additions = parts.next()?;
            let deletions = parts.next()?;
            let path = parts.next()?.to_string();
            let binary = additions == "-" || deletions == "-";
            Some((
                path,
                additions.parse::<u64>().ok(),
                deletions.parse::<u64>().ok(),
                binary,
            ))
        })
        .collect()
}

fn build_diff_summary(
    repository: &Path,
    changes: Vec<FileChange>,
    stats: Vec<(String, Option<u64>, Option<u64>, bool)>,
) -> DiffSummary {
    let mut files = Vec::new();
    let mut insertions = 0;
    let mut deletions = 0;

    for change in changes {
        let stat = stats
            .iter()
            .find(|(path, _, _, _)| path == &change.path)
            .or_else(|| {
                change.old_path.as_ref().and_then(|old_path| {
                    stats.iter().find(|(path, _, _, _)| {
                        path.contains(old_path) && path.contains(&change.path)
                    })
                })
            });
        let (mut additions, mut removed, binary) = stat
            .map(|(_, additions, deletions, binary)| (*additions, *deletions, *binary))
            .unwrap_or((None, None, false));
        if additions.is_none() && matches!(change.status, FileChangeStatus::Untracked) {
            additions = count_text_lines(&repository.join(&change.path));
            removed = Some(0);
        }
        insertions += additions.unwrap_or(0);
        deletions += removed.unwrap_or(0);
        files.push(DiffFileStat {
            path: change.path,
            old_path: change.old_path,
            status: change.status,
            additions,
            deletions: removed,
            binary,
        });
    }

    DiffSummary {
        files_changed: files.len(),
        insertions,
        deletions,
        files,
    }
}

fn count_text_lines(path: &Path) -> Option<u64> {
    let content = fs::read_to_string(path).ok()?;
    Some(content.lines().count() as u64)
}

fn status_from_xy(index_status: char, worktree_status: char) -> FileChangeStatus {
    if index_status == '?' && worktree_status == '?' {
        return FileChangeStatus::Untracked;
    }
    [index_status, worktree_status]
        .into_iter()
        .find(|status| !matches!(status, ' ' | '?'))
        .map(status_from_code)
        .unwrap_or_else(|| FileChangeStatus::Unknown(format!("{index_status}{worktree_status}")))
}

fn status_from_code(code: char) -> FileChangeStatus {
    match code {
        'A' => FileChangeStatus::Added,
        'M' => FileChangeStatus::Modified,
        'D' => FileChangeStatus::Deleted,
        'R' => FileChangeStatus::Renamed,
        'C' => FileChangeStatus::Copied,
        '?' => FileChangeStatus::Untracked,
        other => FileChangeStatus::Unknown(other.to_string()),
    }
}

fn pr_command_with_program(args: &[String]) -> Vec<String> {
    let mut command = vec!["gh".to_string()];
    command.extend(args.iter().cloned());
    command
}

fn redact_url_userinfo(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(relative_start) = input[cursor..].find("http") {
        let start = cursor + relative_start;
        output.push_str(&input[cursor..start]);

        let scheme_end = if input[start..].starts_with("https://") {
            start + "https://".len()
        } else if input[start..].starts_with("http://") {
            start + "http://".len()
        } else {
            output.push_str("http");
            cursor = start + "http".len();
            continue;
        };

        let url_end = input[scheme_end..]
            .find(char::is_whitespace)
            .map(|offset| scheme_end + offset)
            .unwrap_or(input.len());
        let url = &input[start..url_end];
        let after_scheme = &input[scheme_end..url_end];
        if let Some(at_index) = after_scheme.find('@') {
            output.push_str(&input[start..scheme_end]);
            output.push_str("<redacted>@");
            output.push_str(&after_scheme[at_index + 1..]);
        } else {
            output.push_str(url);
        }
        cursor = url_end;
    }

    output.push_str(&input[cursor..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time is after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "open-agents-sandbox-git-{name}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("temp dir is created");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git command runs");
        assert!(
            output.status.success(),
            "git {:?} failed: {}{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent dir is created");
        }
        fs::write(path, contents).expect("file is written");
    }

    fn create_remote_repo(root: &Path) -> (PathBuf, PathBuf) {
        let remote = root.join("remote.git");
        let source = root.join("source");
        fs::create_dir_all(&remote).expect("remote dir is created");
        fs::create_dir_all(&source).expect("source dir is created");

        run_git(&remote, &["init", "--bare"]);
        run_git(&source, &["init"]);
        run_git(&source, &["checkout", "-b", "main"]);
        run_git(&source, &["config", "user.name", "Test User"]);
        run_git(&source, &["config", "user.email", "test@example.com"]);
        write(&source.join("README.md"), "# sandbox\n");
        run_git(&source, &["add", "README.md"]);
        run_git(&source, &["commit", "-m", "initial commit"]);
        run_git(
            &source,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        run_git(&source, &["push", "-u", "origin", "main"]);
        run_git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        (remote, source)
    }

    fn configure_user(repo: &Path) {
        run_git(repo, &["config", "user.name", "Agent User"]);
        run_git(repo, &["config", "user.email", "agent@example.com"]);
    }

    #[test]
    fn branch_name_validation_matches_open_agents_constraints() {
        assert!(is_safe_branch_name("agent/feature-1"));
        assert!(is_safe_branch_name("A.B_c-d/e"));
        assert!(!is_safe_branch_name(""));
        assert!(!is_safe_branch_name("-starts-with-dash"));
        assert!(!is_safe_branch_name("feature..bad"));
        assert!(!is_safe_branch_name("feature//bad"));
        assert!(!is_safe_branch_name("feature/"));
        assert!(!is_safe_branch_name("feature.lock"));
        assert!(!is_safe_branch_name("feature;rm -rf"));
    }

    #[test]
    fn open_rejects_repositories_outside_sandbox_root() {
        let sandbox = TestDir::new("boundary-root");
        let outside = TestDir::new("boundary-outside");
        run_git(&outside.path, &["init"]);

        let error =
            GitSandbox::open(&sandbox.path, &outside.path).expect_err("outside repo rejected");
        assert!(matches!(error, GitError::InvalidSandboxPath { .. }));
    }

    #[test]
    fn clone_branch_status_diff_and_commit_stay_inside_sandbox() {
        let sandbox = TestDir::new("clone-commit");
        let (remote, _) = create_remote_repo(&sandbox.path);

        let repo =
            GitSandbox::clone_repo(&sandbox.path, remote.as_os_str(), Path::new("workspace"))
                .expect("repo cloned");
        configure_user(repo.repository());
        repo.create_branch("agent/change").expect("branch created");

        write(
            &repo.repository().join("src/lib.rs"),
            "pub fn answer() -> u8 { 42 }\n",
        );
        write(
            &repo.repository().join("README.md"),
            "# sandbox\n\nupdated\n",
        );

        let status = repo.status().expect("status is available");
        assert!(status.is_dirty);
        assert_eq!(status.branch, "agent/change");
        assert!(status.files.iter().any(|file| file.path == "src/lib.rs"));

        let diff = repo.diff_summary().expect("diff summary is available");
        assert_eq!(diff.files_changed, 2);
        assert!(diff.insertions >= 2);

        let commit = repo
            .commit_all("feat: update sandbox workspace")
            .expect("commit succeeds");
        assert!(commit.committed);
        assert!(!repo.status().expect("status after commit").is_dirty);
    }

    #[test]
    fn dry_run_push_uses_local_remote_without_live_credentials() {
        let sandbox = TestDir::new("push-dry-run");
        let (remote, _) = create_remote_repo(&sandbox.path);
        let repo =
            GitSandbox::clone_repo(&sandbox.path, remote.as_os_str(), Path::new("workspace"))
                .expect("repo cloned");
        configure_user(repo.repository());
        repo.create_branch("agent/pushable")
            .expect("branch created");
        write(&repo.repository().join("change.txt"), "change\n");
        repo.commit_all("feat: dry run push")
            .expect("commit succeeds");

        let outcome = repo
            .push(&PushOptions {
                mode: GitRemoteActionMode::DryRun,
                remote: "origin".to_string(),
                branch: Some("agent/pushable".to_string()),
                credentials: None,
            })
            .expect("push dry-run succeeds")
            .expect("push ran");

        assert!(outcome.dry_run);
        assert!(!outcome.pushed);
        assert_eq!(outcome.branch, "agent/pushable");
    }

    #[test]
    fn credential_file_tokens_are_redacted_from_outputs() {
        let temp = TestDir::new("credential-redaction");
        let token_path = temp.path.join("token");
        write(&token_path, "ghp_super_secret_token\n");
        let credentials = GitCredentials::from_token_file(&token_path).expect("token loads");

        let redacted = credentials.redactor().redact(
            "fatal: https://user:ghp_super_secret_token@github.com/acme/repo failed ghp_super_secret_token",
        );

        assert!(!redacted.contains("ghp_super_secret_token"));
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn pull_request_dry_run_returns_argv_without_calling_github() {
        let sandbox = TestDir::new("pr-dry-run");
        let (remote, _) = create_remote_repo(&sandbox.path);
        let repo =
            GitSandbox::clone_repo(&sandbox.path, remote.as_os_str(), Path::new("workspace"))
                .expect("repo cloned");
        repo.create_branch("agent/pr").expect("branch created");

        let outcome = repo
            .create_pull_request_command(&crate::git_finish::PullRequestOptions {
                mode: GitRemoteActionMode::DryRun,
                base: "main".to_string(),
                head: None,
                title: "Agent changes".to_string(),
                body: "Summary".to_string(),
                repository: Some("acme/repo".to_string()),
                credentials: None,
            })
            .expect("dry-run builds")
            .expect("pr command is present");

        assert!(outcome.dry_run);
        assert!(!outcome.created);
        assert_eq!(outcome.command[0], "gh");
        assert!(outcome.command.contains(&"agent/pr".to_string()));
    }
}
