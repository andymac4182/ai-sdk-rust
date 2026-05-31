use crate::git::{
    CommitOutcome, DiffSummary, GitError, GitRemoteActionMode, GitSandbox,
    PullRequestCommandOutcome, PushOptions, PushOutcome,
};
use serde::{Deserialize, Serialize};

/// Options for optional pull-request creation after commit/push handling.
#[derive(Debug, Clone)]
pub struct PullRequestOptions {
    pub mode: GitRemoteActionMode,
    pub base: String,
    pub head: Option<String>,
    pub title: String,
    pub body: String,
    pub repository: Option<String>,
    pub credentials: Option<crate::git::GitCredentials>,
}

impl PullRequestOptions {
    /// Creates disabled PR options.
    pub fn disabled() -> Self {
        Self {
            mode: GitRemoteActionMode::Disabled,
            base: "main".to_string(),
            head: None,
            title: String::new(),
            body: String::new(),
            repository: None,
            credentials: None,
        }
    }
}

impl Default for PullRequestOptions {
    fn default() -> Self {
        Self::disabled()
    }
}

/// Post-finish git automation options.
#[derive(Debug, Clone, Default)]
pub struct GitFinishOptions {
    pub commit_message: Option<String>,
    pub push: PushOptions,
    pub pull_request: PullRequestOptions,
}

/// Overall finish status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitFinishStatus {
    NoChanges,
    Committed,
    Pushed,
    PullRequestCreated,
    Skipped,
    Error,
}

/// Result from optional pull-request creation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestOutcome {
    pub created: bool,
    pub dry_run: bool,
    pub url: Option<String>,
    pub command: Vec<String>,
    pub output: Option<String>,
}

impl From<PullRequestCommandOutcome> for PullRequestOutcome {
    fn from(value: PullRequestCommandOutcome) -> Self {
        Self {
            created: value.created,
            dry_run: value.dry_run,
            url: value.url,
            command: value.command,
            output: value.output,
        }
    }
}

/// Typed report that can be persisted to chat state and rendered to Slack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitFinishReport {
    pub status: GitFinishStatus,
    pub branch: String,
    pub head_sha: String,
    pub diff: DiffSummary,
    pub commit: Option<CommitOutcome>,
    pub push: Option<PushOutcome>,
    pub pull_request: Option<PullRequestOutcome>,
    pub error: Option<String>,
}

impl GitFinishReport {
    fn no_changes(branch: String, head_sha: String) -> Self {
        Self {
            status: GitFinishStatus::NoChanges,
            branch,
            head_sha,
            diff: DiffSummary::default(),
            commit: None,
            push: None,
            pull_request: None,
            error: None,
        }
    }

    fn with_error(mut self, error: GitError) -> Self {
        self.status = GitFinishStatus::Error;
        self.error = Some(error.to_string());
        self
    }
}

/// Runs post-finish repository automation inside the sandbox boundary.
///
/// The default behavior is no-op reporting. A commit only happens when
/// `commit_message` is present and the worktree is dirty. Push and PR actions
/// are independently gated and support dry-run mode for deterministic tests.
pub fn run_git_finish(
    sandbox: &GitSandbox,
    options: &GitFinishOptions,
) -> Result<GitFinishReport, GitError> {
    let starting_status = sandbox.status()?;
    let diff = if starting_status.is_dirty {
        sandbox.diff_summary()?
    } else {
        DiffSummary::default()
    };

    if !starting_status.is_dirty
        && options.push.mode == GitRemoteActionMode::Disabled
        && options.pull_request.mode == GitRemoteActionMode::Disabled
    {
        return Ok(GitFinishReport::no_changes(
            starting_status.branch,
            starting_status.head_sha,
        ));
    }

    let mut report = GitFinishReport {
        status: if starting_status.is_dirty {
            GitFinishStatus::Skipped
        } else {
            GitFinishStatus::NoChanges
        },
        branch: starting_status.branch,
        head_sha: starting_status.head_sha,
        diff,
        commit: None,
        push: None,
        pull_request: None,
        error: None,
    };

    if starting_status.is_dirty {
        if let Some(message) = &options.commit_message {
            match sandbox.commit_all(message) {
                Ok(commit) => {
                    if commit.committed {
                        report.status = GitFinishStatus::Committed;
                        if let Some(commit_sha) = &commit.commit_sha {
                            report.head_sha = commit_sha.clone();
                        }
                    }
                    report.commit = Some(commit);
                }
                Err(error) => return Ok(report.with_error(error)),
            }
        } else {
            return Ok(report);
        }
    }

    match sandbox.push(&options.push) {
        Ok(Some(push)) => {
            if push.pushed {
                report.status = GitFinishStatus::Pushed;
            }
            report.push = Some(push);
        }
        Ok(None) => {}
        Err(error) => return Ok(report.with_error(error)),
    }

    match sandbox.create_pull_request_command(&options.pull_request) {
        Ok(Some(pull_request)) => {
            if pull_request.created {
                report.status = GitFinishStatus::PullRequestCreated;
            }
            report.pull_request = Some(pull_request.into());
        }
        Ok(None) => {}
        Err(error) => return Ok(report.with_error(error)),
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{GitRemoteActionMode, GitSandbox, PushOptions};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
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
                "open-agents-sandbox-git-finish-{name}-{}-{nanos}",
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

    fn create_remote_repo(root: &Path) -> PathBuf {
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
        remote
    }

    fn setup_repo(name: &str) -> (TestDir, GitSandbox) {
        let sandbox = TestDir::new(name);
        let remote = create_remote_repo(&sandbox.path);
        let repo =
            GitSandbox::clone_repo(&sandbox.path, remote.as_os_str(), Path::new("workspace"))
                .expect("repo cloned");
        run_git(repo.repository(), &["config", "user.name", "Agent User"]);
        run_git(
            repo.repository(),
            &["config", "user.email", "agent@example.com"],
        );
        repo.create_branch("agent/finish").expect("branch created");
        (sandbox, repo)
    }

    #[test]
    fn finish_reports_noop_for_clean_repository() {
        let (_temp, repo) = setup_repo("noop");

        let report = run_git_finish(&repo, &GitFinishOptions::default()).expect("finish succeeds");

        assert_eq!(report.status, GitFinishStatus::NoChanges);
        assert!(report.commit.is_none());
        assert!(report.error.is_none());
    }

    #[test]
    fn finish_commits_dirty_repository() {
        let (_temp, repo) = setup_repo("commit");
        write(&repo.repository().join("agent.txt"), "done\n");

        let report = run_git_finish(
            &repo,
            &GitFinishOptions {
                commit_message: Some("feat: finish agent changes".to_string()),
                ..GitFinishOptions::default()
            },
        )
        .expect("finish succeeds");

        assert_eq!(report.status, GitFinishStatus::Committed);
        assert_eq!(report.diff.files_changed, 1);
        assert!(
            report
                .commit
                .as_ref()
                .is_some_and(|commit| commit.committed)
        );
        assert!(!repo.status().expect("status").is_dirty);
    }

    #[test]
    fn finish_commits_and_pushes_in_dry_run_mode() {
        let (_temp, repo) = setup_repo("push-dry");
        write(&repo.repository().join("agent.txt"), "done\n");

        let report = run_git_finish(
            &repo,
            &GitFinishOptions {
                commit_message: Some("feat: dry push finish".to_string()),
                push: PushOptions::dry_run(),
                ..GitFinishOptions::default()
            },
        )
        .expect("finish succeeds");

        assert_eq!(report.status, GitFinishStatus::Committed);
        assert!(report.push.as_ref().is_some_and(|push| push.dry_run));
    }

    #[test]
    fn finish_builds_pr_command_in_dry_run_mode() {
        let (_temp, repo) = setup_repo("pr-dry");

        let report = run_git_finish(
            &repo,
            &GitFinishOptions {
                pull_request: PullRequestOptions {
                    mode: GitRemoteActionMode::DryRun,
                    base: "main".to_string(),
                    head: None,
                    title: "Agent changes".to_string(),
                    body: "Summary".to_string(),
                    repository: Some("acme/repo".to_string()),
                    credentials: None,
                },
                ..GitFinishOptions::default()
            },
        )
        .expect("finish succeeds");

        let pull_request = report.pull_request.expect("pr outcome");
        assert!(pull_request.dry_run);
        assert!(!pull_request.created);
        assert_eq!(pull_request.command[0], "gh");
    }
}
