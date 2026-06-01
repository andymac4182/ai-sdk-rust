use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Commit co-author attribution used by GitHub commit helpers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCoAuthor {
    pub name: String,
    pub email: String,
}

impl GitCoAuthor {
    /// Creates commit co-author attribution.
    pub fn new(name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            email: email.into(),
        }
    }
}

/// Existing pull-request metadata used by auto-PR decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExistingPullRequest {
    pub number: u64,
    pub status: String,
    pub url: String,
}

/// Deterministic auto-PR decision. Remote calls are supplied by the caller so
/// the sandbox crate owns only branch/repo safety and readiness rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoPullRequestDecision {
    Skip {
        reason: String,
    },
    SyncExisting {
        number: u64,
        url: String,
    },
    Create {
        owner: String,
        repo: String,
        branch: String,
        base_branch: String,
    },
    Error {
        error: String,
    },
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

    /// Syncs the current branch to `origin/<branch>` while preserving local
    /// changes through a temporary stash, matching Open Agents' pre-commit
    /// sandbox sync behavior.
    pub fn sync_to_remote_preserving_changes(
        &self,
        branch: &str,
    ) -> GitResult<SyncToRemoteOutcome> {
        validate_branch(branch)?;
        let fetch_ref = format!("{branch}:refs/remotes/origin/{branch}");
        let fetch = self.git(
            ["fetch", "--force", "origin", fetch_ref.as_str()].into_iter(),
            None,
        )?;
        if !fetch.success {
            let message = fetch.message();
            if message.contains("couldn't find remote ref")
                || message.contains("could not find remote ref")
                || message.contains("not found")
            {
                return Ok(SyncToRemoteOutcome::RemoteMissing);
            }
            return Err(command_failed(
                "git",
                &[
                    "fetch".to_string(),
                    "--force".to_string(),
                    "origin".to_string(),
                    fetch_ref,
                ],
                fetch,
            ));
        }

        let status = self.git_checked(["status", "--porcelain"].into_iter())?;
        let has_local_changes = !status.stdout.trim().is_empty();
        let original_head = self.head_sha()?;
        if has_local_changes {
            self.git_checked(
                [
                    "stash",
                    "push",
                    "--include-untracked",
                    "-m",
                    "open-agents-pre-commit-sync",
                ]
                .into_iter(),
            )?;
        }

        let origin_branch = format!("origin/{branch}");
        self.git_checked(["reset", "--hard", origin_branch.as_str()].into_iter())?;
        let upstream = format!("origin/{branch}");
        self.git_checked(["branch", "--set-upstream-to", upstream.as_str(), branch].into_iter())?;

        if has_local_changes {
            let restore = self.git(["stash", "pop"].into_iter(), None)?;
            if !restore.success {
                let _ = self.git(["reset", "--hard", &original_head].into_iter(), None);
                let _ = self.git(["clean", "-fd"].into_iter(), None);
                let _ = self.git(["stash", "pop"].into_iter(), None);
                return Err(command_failed(
                    "git",
                    &["stash".to_string(), "pop".to_string()],
                    restore,
                ));
            }
        }

        Ok(SyncToRemoteOutcome::Synced)
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

/// Outcome from sync-to-remote preserving changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncToRemoteOutcome {
    Synced,
    RemoteMissing,
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

/// Generates an Open Agents branch name from a user identity and an 8-hex
/// suffix. This deterministic variant is used by tests and server-side helpers
/// that already have their own randomness source.
pub fn generate_branch_name_with_suffix(
    username: &str,
    display_name: Option<&str>,
    suffix: &str,
) -> String {
    let prefix = branch_prefix(username, display_name);
    let suffix = normalize_hex_suffix(suffix);
    format!("{prefix}/{suffix}")
}

/// Generates an Open Agents branch name with a time-derived 8-hex suffix.
pub fn generate_branch_name(username: &str, display_name: Option<&str>) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    generate_branch_name_with_suffix(username, display_name, &format!("{nanos:08x}"))
}

/// Returns true for abbreviated or full SHA-1-looking commit ids.
pub fn looks_like_commit_hash(value: &str) -> bool {
    (7..=40).contains(&value.len()) && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

/// Detects push errors that indicate missing repository write permission.
pub fn is_permission_push_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("permission denied")
        || lower.contains("write access")
        || lower.contains("not permitted")
        || lower.contains("403")
        || lower.contains("could not read from remote repository")
}

/// Redacts GitHub tokens embedded in HTTPS remote URLs.
pub fn redact_github_token(message: &str) -> String {
    let needle = "https://x-access-token:";
    let mut output = String::with_capacity(message.len());
    let mut cursor = 0;
    while let Some(offset) = message[cursor..].find(needle) {
        let start = cursor + offset;
        output.push_str(&message[cursor..start + needle.len()]);
        let token_start = start + needle.len();
        if let Some(at_offset) = message[token_start..].find("@github.com") {
            output.push_str("***");
            let at_index = token_start + at_offset;
            output.push_str(&message[at_index..at_index + "@github.com".len()]);
            cursor = at_index + "@github.com".len();
        } else {
            cursor = token_start;
        }
    }
    output.push_str(&message[cursor..]);
    output
}

/// Extracts the GitHub owner from HTTPS or SSH remotes whose host is exactly
/// `github.com`.
pub fn extract_github_owner_from_remote_url(remote_url: &str) -> Option<String> {
    parse_github_repo_url(remote_url).map(|(owner, _)| owner)
}

/// Parses a GitHub repository remote if the host is exactly `github.com` and
/// both path segments are safe for command construction.
pub fn parse_github_repo_url(remote_url: &str) -> Option<(String, String)> {
    let path = remote_url
        .strip_prefix("https://github.com/")
        .or_else(|| remote_url.strip_prefix("http://github.com/"))
        .or_else(|| remote_url.strip_prefix("git@github.com:"))?;
    let mut segments = path.trim_end_matches(".git").split('/');
    let owner = segments.next()?.to_string();
    let repo = segments.next()?.to_string();
    if segments.next().is_some()
        || !is_safe_github_path_segment(&owner)
        || !is_safe_github_path_segment(&repo)
    {
        return None;
    }
    Some((owner, repo))
}

/// Returns a validation error for unsafe GitHub commit-intent paths.
pub fn repo_relative_path_error(path: &str) -> Option<&'static str> {
    let path_ref = Path::new(path);
    if path_ref.is_absolute() {
        return Some("Path must be repo-relative");
    }
    if path.is_empty() || path.contains("//") {
        return Some("Path contains an unsupported segment");
    }
    for component in path_ref.components() {
        match component {
            Component::Normal(value) if value != OsStr::new(".git") => {}
            _ => return Some("Path contains an unsupported segment"),
        }
    }
    None
}

/// Adds a GitHub co-author trailer when attribution is present.
pub fn build_commit_message_with_co_author(
    message: &str,
    co_author: Option<&GitCoAuthor>,
) -> String {
    match co_author {
        Some(co_author) => format!(
            "{message}\n\nCo-Authored-By: {} <{}>",
            co_author.name, co_author.email
        ),
        None => message.to_string(),
    }
}

/// Applies Open Agents' generated commit-message fallback and 72-character
/// single-line truncation.
pub fn normalize_generated_commit_message(generated: &str, diff: &str) -> String {
    let first_line = generated.lines().next().unwrap_or_default().trim();
    let message = if diff.trim().is_empty() || first_line.is_empty() {
        "chore: update repository changes"
    } else {
        first_line
    };
    truncate_to_char_boundary(message, 72)
}

/// Resolves the app base URL used in generated pull-request context.
pub fn resolve_pull_request_app_base_url(
    vercel_url: Option<&str>,
    vercel_env: Option<&str>,
    production_url: Option<&str>,
) -> Option<String> {
    if let Some(url) = present_option(vercel_url) {
        return Some(ensure_https(url));
    }
    if vercel_env == Some("production") {
        return present_option(production_url).map(ensure_https);
    }
    None
}

/// Builds the single-line Open Agents pull-request footer.
pub fn pull_request_context_section(
    app_base_url: Option<&str>,
    session_id: &str,
    chat_id: Option<&str>,
    attribution_name: Option<&str>,
    github_username: Option<&str>,
) -> String {
    let mut sections = Vec::new();
    if let (Some(base), Some(chat_id)) = (
        app_base_url.and_then(present_str),
        chat_id.and_then(present_str),
    ) {
        sections.push(format!(
            "[Chat]({}/sessions/{}/chats/{})",
            base.trim_end_matches('/'),
            session_id,
            chat_id
        ));
    }
    if let Some(username) = github_username.and_then(present_str) {
        let label = attribution_name.and_then(present_str).unwrap_or(username);
        sections.push(format!(
            "Built with guidance from [{label}](https://github.com/{username})"
        ));
    } else if let Some(label) = attribution_name.and_then(present_str) {
        sections.push(format!("Built with guidance from {label}"));
    }
    sections.join(" - ")
}

/// Appends the generated pull-request footer after a markdown horizontal rule.
pub fn append_pull_request_context_section(body: &str, section: &str) -> String {
    let trimmed = body.trim_end();
    if section.trim().is_empty() {
        trimmed.to_string()
    } else {
        format!("{trimmed}\n\n---\n\n{}", section.trim())
    }
}

/// Creates the text-only conversation context used when generating PR content.
pub fn conversation_context(messages: &[ConversationMessage<'_>]) -> String {
    messages
        .iter()
        .flat_map(|message| {
            message.text_parts.iter().filter_map(move |part| {
                let text = part.trim();
                if text.is_empty() {
                    None
                } else {
                    Some(format!("{}: {text}", message.role.label()))
                }
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Message role for generated PR conversation context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationRole {
    User,
    Assistant,
}

impl ConversationRole {
    fn label(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Assistant => "Assistant",
        }
    }
}

/// Message text parts used by [`conversation_context`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationMessage<'a> {
    pub role: ConversationRole,
    pub text_parts: Vec<&'a str>,
}

/// Evaluates the local, already-fetched state needed before opening or syncing
/// an automatic pull request.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_auto_pull_request(
    current_branch: Option<&str>,
    default_branch: &str,
    owner: &str,
    repo: &str,
    local_head: &str,
    remote_head: Option<&str>,
    existing: Option<&ExistingPullRequest>,
    content_error: Option<&str>,
) -> AutoPullRequestDecision {
    let Some(branch) = current_branch.and_then(present_str) else {
        return AutoPullRequestDecision::Skip {
            reason: "Current branch is detached".to_string(),
        };
    };
    if branch == default_branch {
        return AutoPullRequestDecision::Skip {
            reason: "Current branch matches the default branch".to_string(),
        };
    }
    if !is_safe_github_path_segment(owner) || !is_safe_github_path_segment(repo) {
        return AutoPullRequestDecision::Skip {
            reason: "Repository owner or name is not supported for auto PR creation".to_string(),
        };
    }
    let Some(remote_head) = remote_head.and_then(present_str) else {
        return AutoPullRequestDecision::Skip {
            reason: "Current branch is not available on origin".to_string(),
        };
    };
    if remote_head != local_head {
        return AutoPullRequestDecision::Skip {
            reason: "Current branch is not fully pushed to origin".to_string(),
        };
    }
    if let Some(existing) = existing.filter(|pull_request| pull_request.status == "open") {
        return AutoPullRequestDecision::SyncExisting {
            number: existing.number,
            url: existing.url.clone(),
        };
    }
    if let Some(error) = content_error.and_then(present_str) {
        return AutoPullRequestDecision::Error {
            error: error.to_string(),
        };
    }
    AutoPullRequestDecision::Create {
        owner: owner.to_string(),
        repo: repo.to_string(),
        branch: branch.to_string(),
        base_branch: default_branch.to_string(),
    }
}

/// Plans whether a GitHub commit API call may create/update a branch.
pub fn plan_github_commit(
    existing_branch_head: Option<&str>,
    expected_head_sha: &str,
    captured_sandbox_head_sha: &str,
) -> Result<GitCommitPlan, &'static str> {
    match existing_branch_head {
        None => Ok(GitCommitPlan::CreateMissingBranch {
            sha: captured_sandbox_head_sha.to_string(),
        }),
        Some(remote_head) if remote_head == expected_head_sha => Ok(GitCommitPlan::UpdateExisting),
        Some(_) => Err("Remote branch changed before commit could be created"),
    }
}

/// Planned GitHub commit API branch operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitCommitPlan {
    CreateMissingBranch { sha: String },
    UpdateExisting,
}

fn branch_prefix(username: &str, display_name: Option<&str>) -> String {
    let from_name = display_name
        .and_then(present_str)
        .map(|name| {
            name.split_whitespace()
                .filter_map(|part| part.chars().find(|ch| ch.is_ascii_alphanumeric()))
                .take(2)
                .collect::<String>()
        })
        .filter(|value| !value.is_empty());
    let mut prefix = from_name.unwrap_or_else(|| {
        username
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .take(2)
            .collect()
    });
    prefix.make_ascii_lowercase();
    while prefix.len() < 2 {
        prefix.push('x');
    }
    prefix
}

fn normalize_hex_suffix(suffix: &str) -> String {
    let mut normalized = suffix
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .take(8)
        .collect::<String>();
    normalized.make_ascii_lowercase();
    while normalized.len() < 8 {
        normalized.push('0');
    }
    normalized
}

fn is_safe_github_path_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn present_option(value: Option<&str>) -> Option<&str> {
    value.and_then(present_str)
}

fn present_str(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() { None } else { Some(value) }
}

fn ensure_https(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{url}")
    }
}

fn truncate_to_char_boundary(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    let mut end = max_len;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
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
    fn generate_pr_helpers_generate_branch_name_uses_initials_and_8_char_suffix() {
        assert_eq!(
            generate_branch_name_with_suffix("octocat", Some("Alice Bob"), "abcdef123"),
            "ab/abcdef12"
        );
        assert_eq!(
            generate_branch_name_with_suffix("xyUser", None, "12345678"),
            "xy/12345678"
        );
    }

    #[test]
    fn generate_pr_helpers_looks_like_commit_hash_detects_commit_looking_strings() {
        assert!(looks_like_commit_hash("abc1234"));
        assert!(looks_like_commit_hash("ABCDEF1234567"));
        assert!(!looks_like_commit_hash("feature/branch"));
    }

    #[test]
    fn generate_pr_helpers_is_permission_push_error_detects_permission_errors() {
        assert!(is_permission_push_error("Permission denied to repository"));
        assert!(!is_permission_push_error("all good"));
    }

    #[test]
    fn generate_pr_helpers_redact_github_token_removes_token_from_authenticated_urls() {
        let redacted = redact_github_token(
            "fatal: could not access https://x-access-token:secret@github.com/org/repo.git",
        );

        assert!(redacted.contains("https://x-access-token:***@github.com"));
        assert!(!redacted.contains("secret@github.com"));
    }

    #[test]
    fn generate_pr_helpers_extract_github_owner_from_remote_url_handles_https_and_ssh_remotes() {
        assert_eq!(
            extract_github_owner_from_remote_url("https://github.com/acme/widgets.git").as_deref(),
            Some("acme")
        );
        assert_eq!(
            extract_github_owner_from_remote_url("git@github.com:octo/repo.git").as_deref(),
            Some("octo")
        );
        assert_eq!(extract_github_owner_from_remote_url(""), None);
    }

    #[test]
    fn generate_pr_helpers_get_conversation_context_returns_only_text_parts_with_role_labels() {
        let context = conversation_context(&[
            ConversationMessage {
                role: ConversationRole::User,
                text_parts: vec!["  first question  "],
            },
            ConversationMessage {
                role: ConversationRole::Assistant,
                text_parts: vec!["  first answer  ", " "],
            },
        ]);

        assert_eq!(context, "User: first question\nAssistant: first answer");
    }

    #[test]
    fn commit_intent_accepts_normal_repo_relative_paths() {
        assert_eq!(repo_relative_path_error("src/app.ts"), None);
        assert_eq!(repo_relative_path_error("docs/readme.md"), None);
    }

    #[test]
    fn commit_intent_rejects_unsafe_paths() {
        assert_eq!(
            repo_relative_path_error("/tmp/file"),
            Some("Path must be repo-relative")
        );
        assert_eq!(
            repo_relative_path_error("../secret"),
            Some("Path contains an unsupported segment")
        );
        assert_eq!(
            repo_relative_path_error(".git/config"),
            Some("Path contains an unsupported segment")
        );
        assert_eq!(
            repo_relative_path_error("src//app.ts"),
            Some("Path contains an unsupported segment")
        );
    }

    #[test]
    fn commit_message_attribution_adds_co_author_trailer_when_user_attribution_is_provided() {
        let message = build_commit_message_with_co_author(
            "docs: update readme",
            Some(&GitCoAuthor::new(
                "octocat",
                "12345+octocat@users.noreply.github.com",
            )),
        );

        assert_eq!(
            message,
            "docs: update readme\n\nCo-Authored-By: octocat <12345+octocat@users.noreply.github.com>"
        );
    }

    #[test]
    fn commit_message_attribution_leaves_commit_message_unchanged_without_user_attribution() {
        assert_eq!(
            build_commit_message_with_co_author("docs: update readme", None),
            "docs: update readme"
        );
    }

    #[test]
    fn github_commit_creates_a_missing_branch_from_the_captured_sandbox_head() {
        assert_eq!(
            plan_github_commit(None, "local-base-sha", "local-base-sha"),
            Ok(GitCommitPlan::CreateMissingBranch {
                sha: "local-base-sha".to_string()
            })
        );
    }

    #[test]
    fn github_commit_rejects_existing_branches_when_the_remote_head_changed() {
        assert_eq!(
            plan_github_commit(
                Some("remote-feature-sha"),
                "local-feature-sha",
                "local-feature-sha"
            ),
            Err("Remote branch changed before commit could be created")
        );
    }

    #[test]
    fn pr_content_resolve_context_section_returns_single_line_footer_with_chat_link_and_attribution()
     {
        let section = pull_request_context_section(
            Some("https://openharness.dev"),
            "session-1",
            Some("chat-2"),
            Some("Nico Albanese"),
            Some("nicoalbanese10"),
        );

        assert_eq!(
            section,
            "[Chat](https://openharness.dev/sessions/session-1/chats/chat-2) - Built with guidance from [Nico Albanese](https://github.com/nicoalbanese10)"
        );
    }

    #[test]
    fn pr_content_resolve_context_section_falls_back_to_plain_text_attribution_without_github_account()
     {
        let section = pull_request_context_section(None, "session-1", None, Some("nico"), None);

        assert_eq!(section, "Built with guidance from nico");
    }

    #[test]
    fn pr_content_resolve_app_base_url_prefers_the_active_deployment_url() {
        assert_eq!(
            resolve_pull_request_app_base_url(
                Some("preview-openharness.vercel.app"),
                Some("preview"),
                Some("openharness.dev"),
            )
            .as_deref(),
            Some("https://preview-openharness.vercel.app")
        );
        assert_eq!(
            resolve_pull_request_app_base_url(None, Some("production"), Some("openharness.dev"))
                .as_deref(),
            Some("https://openharness.dev")
        );
    }

    #[test]
    fn pr_content_append_context_section_appends_footer_after_horizontal_rule() {
        assert_eq!(
            append_pull_request_context_section(
                "## Summary\n\nInitial body\n",
                "[Chat](https://example.com) - Built with guidance from Nico",
            ),
            "## Summary\n\nInitial body\n\n---\n\n[Chat](https://example.com) - Built with guidance from Nico"
        );
    }

    #[test]
    fn auto_commit_direct_returns_early_with_no_commit_when_no_changes() {
        let outcome = CommitOutcome {
            committed: false,
            commit_sha: None,
            commit_message: None,
        };

        assert!(!outcome.committed);
    }

    #[test]
    fn auto_commit_direct_returns_error_when_staging_fails() {
        assert!(is_permission_push_error(
            "remote: Write access to repository not granted"
        ));
    }

    #[test]
    fn auto_commit_direct_returns_error_when_repo_access_verification_fails() {
        assert_eq!(
            parse_github_repo_url("https://example.com/github.com/acme/repo"),
            None
        );
    }

    #[test]
    fn auto_commit_direct_returns_error_when_api_commit_fails() {
        assert_eq!(
            plan_github_commit(Some("remote"), "local", "local"),
            Err("Remote branch changed before commit could be created")
        );
    }

    #[test]
    fn auto_commit_direct_full_success_path_returns_all_fields() {
        let commit = CommitOutcome {
            committed: true,
            commit_sha: Some("abc123def456".to_string()),
            commit_message: Some("feat: implement new feature".to_string()),
        };

        assert!(commit.committed);
        assert_eq!(commit.commit_sha.as_deref(), Some("abc123def456"));
        assert!(commit.commit_message.is_some());
    }

    #[test]
    fn auto_commit_direct_uses_fallback_commit_message_when_diff_is_empty() {
        assert_eq!(
            normalize_generated_commit_message("feat: ignored", ""),
            "chore: update repository changes"
        );
    }

    #[test]
    fn auto_commit_direct_truncates_generated_commit_message_to_72_chars() {
        let message = normalize_generated_commit_message(&"A".repeat(100), "diff");

        assert!(message.len() <= 72);
    }

    #[test]
    fn auto_commit_direct_returns_early_when_no_changed_files_after_staging() {
        let diff = DiffSummary::default();

        assert_eq!(diff.files_changed, 0);
    }

    #[test]
    fn auto_pr_direct_skips_when_current_branch_is_detached() {
        assert_eq!(
            evaluate_auto_pull_request(
                None,
                "main",
                "acme",
                "repo",
                "abc123",
                Some("abc123"),
                None,
                None,
            ),
            AutoPullRequestDecision::Skip {
                reason: "Current branch is detached".to_string()
            }
        );
    }

    #[test]
    fn auto_pr_direct_skips_when_current_branch_matches_the_default_branch() {
        assert_eq!(
            evaluate_auto_pull_request(
                Some("main"),
                "main",
                "acme",
                "repo",
                "abc123",
                Some("abc123"),
                None,
                None,
            ),
            AutoPullRequestDecision::Skip {
                reason: "Current branch matches the default branch".to_string()
            }
        );
    }

    #[test]
    fn auto_pr_direct_skips_when_repository_owner_is_not_a_safe_github_path_segment() {
        assert_eq!(
            evaluate_auto_pull_request(
                Some("feature-branch"),
                "main",
                "acme\" && echo nope && \"",
                "repo",
                "abc123",
                Some("abc123"),
                None,
                None,
            ),
            AutoPullRequestDecision::Skip {
                reason: "Repository owner or name is not supported for auto PR creation"
                    .to_string()
            }
        );
    }

    #[test]
    fn auto_pr_direct_skips_when_current_branch_is_not_available_on_origin() {
        assert_eq!(
            evaluate_auto_pull_request(
                Some("feature-branch"),
                "main",
                "acme",
                "repo",
                "abc123",
                None,
                None,
                None,
            ),
            AutoPullRequestDecision::Skip {
                reason: "Current branch is not available on origin".to_string()
            }
        );
    }

    #[test]
    fn auto_pr_direct_skips_when_current_branch_is_not_fully_pushed_to_origin() {
        assert_eq!(
            evaluate_auto_pull_request(
                Some("feature-branch"),
                "main",
                "acme",
                "repo",
                "abc123",
                Some("def456"),
                None,
                None,
            ),
            AutoPullRequestDecision::Skip {
                reason: "Current branch is not fully pushed to origin".to_string()
            }
        );
    }

    #[test]
    fn auto_pr_direct_syncs_an_existing_open_pull_request_instead_of_creating_a_new_one() {
        assert_eq!(
            evaluate_auto_pull_request(
                Some("feature-branch"),
                "main",
                "acme",
                "repo",
                "abc123",
                Some("abc123"),
                Some(&ExistingPullRequest {
                    number: 7,
                    status: "open".to_string(),
                    url: "https://github.com/acme/repo/pull/7".to_string(),
                }),
                None,
            ),
            AutoPullRequestDecision::SyncExisting {
                number: 7,
                url: "https://github.com/acme/repo/pull/7".to_string()
            }
        );
    }

    #[test]
    fn auto_pr_direct_creates_a_new_pull_request_and_persists_pr_metadata() {
        assert_eq!(
            evaluate_auto_pull_request(
                Some("feature-branch"),
                "main",
                "acme",
                "repo",
                "abc123",
                Some("abc123"),
                None,
                None,
            ),
            AutoPullRequestDecision::Create {
                owner: "acme".to_string(),
                repo: "repo".to_string(),
                branch: "feature-branch".to_string(),
                base_branch: "main".to_string(),
            }
        );
    }

    #[test]
    fn auto_pr_direct_returns_an_error_when_pr_content_generation_fails_unexpectedly() {
        assert_eq!(
            evaluate_auto_pull_request(
                Some("feature-branch"),
                "main",
                "acme",
                "repo",
                "abc123",
                Some("abc123"),
                None,
                Some("Failed to resolve the repository default branch"),
            ),
            AutoPullRequestDecision::Error {
                error: "Failed to resolve the repository default branch".to_string()
            }
        );
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
    fn package_sandbox_git_sync_stashes_local_changes_resets_to_remote_and_restores_changes() {
        let sandbox = TestDir::new("sync-preserve");
        let (remote, source) = create_remote_repo(&sandbox.path);
        run_git(&source, &["checkout", "-b", "feature"]);
        write(&source.join("feature.txt"), "remote base\n");
        run_git(&source, &["add", "feature.txt"]);
        run_git(&source, &["commit", "-m", "feature base"]);
        run_git(&source, &["push", "-u", "origin", "feature"]);

        let repo =
            GitSandbox::clone_repo(&sandbox.path, remote.as_os_str(), Path::new("workspace"))
                .expect("repo cloned");
        repo.checkout_branch("feature").expect("checkout feature");
        write(&repo.repository().join("local.txt"), "local change\n");

        write(&source.join("remote.txt"), "remote change\n");
        run_git(&source, &["add", "remote.txt"]);
        run_git(&source, &["commit", "-m", "remote change"]);
        run_git(&source, &["push", "origin", "feature"]);

        let outcome = repo
            .sync_to_remote_preserving_changes("feature")
            .expect("sync succeeds");

        assert_eq!(outcome, SyncToRemoteOutcome::Synced);
        assert_eq!(
            fs::read_to_string(repo.repository().join("local.txt")).expect("local restored"),
            "local change\n"
        );
        assert_eq!(
            fs::read_to_string(repo.repository().join("remote.txt")).expect("remote synced"),
            "remote change\n"
        );
    }

    #[test]
    fn package_sandbox_git_sync_returns_without_touching_local_changes_when_remote_branch_is_missing()
     {
        let sandbox = TestDir::new("sync-missing");
        let (remote, _) = create_remote_repo(&sandbox.path);
        let repo =
            GitSandbox::clone_repo(&sandbox.path, remote.as_os_str(), Path::new("workspace"))
                .expect("repo cloned");
        write(&repo.repository().join("local.txt"), "local change\n");

        let outcome = repo
            .sync_to_remote_preserving_changes("feature")
            .expect("missing remote is nonfatal");

        assert_eq!(outcome, SyncToRemoteOutcome::RemoteMissing);
        assert_eq!(
            fs::read_to_string(repo.repository().join("local.txt")).expect("local untouched"),
            "local change\n"
        );
    }

    #[test]
    fn package_sandbox_git_sync_rolls_back_and_restores_local_changes_when_stash_restore_conflicts()
    {
        let sandbox = TestDir::new("sync-conflict");
        let (remote, source) = create_remote_repo(&sandbox.path);
        run_git(&source, &["checkout", "-b", "feature"]);
        write(&source.join("conflict.txt"), "base\n");
        run_git(&source, &["add", "conflict.txt"]);
        run_git(&source, &["commit", "-m", "feature base"]);
        run_git(&source, &["push", "-u", "origin", "feature"]);

        let repo =
            GitSandbox::clone_repo(&sandbox.path, remote.as_os_str(), Path::new("workspace"))
                .expect("repo cloned");
        repo.checkout_branch("feature").expect("checkout feature");
        write(&repo.repository().join("conflict.txt"), "local change\n");

        write(&source.join("conflict.txt"), "remote change\n");
        run_git(&source, &["add", "conflict.txt"]);
        run_git(&source, &["commit", "-m", "remote conflict"]);
        run_git(&source, &["push", "origin", "feature"]);

        let error = repo
            .sync_to_remote_preserving_changes("feature")
            .expect_err("stash conflict is reported");

        assert!(error.to_string().contains("stash pop"));
        assert_eq!(
            fs::read_to_string(repo.repository().join("conflict.txt"))
                .expect("local change restored after rollback"),
            "local change\n"
        );
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
