//! Deterministic route-planning helpers for Open Agents session APIs.
//!
//! The Slack-first Rust service does not expose the Next.js web routes one for
//! one, but these helpers pin their portable behavior to small typed decisions
//! that service and persistence adapters can reuse.

use std::collections::BTreeMap;

/// A process row from a sandbox process listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub command: String,
}

impl ProcessInfo {
    /// Creates a process row.
    pub fn new(pid: u32, command: impl Into<String>) -> Self {
        Self {
            pid,
            command: command.into(),
        }
    }
}

/// Result of inspecting the code-editor route state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeEditorStatus {
    pub running: bool,
    pub url: Option<String>,
    pub port: u16,
}

/// Plan for starting a code editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeEditorStartPlan {
    Reuse {
        pid: u32,
        url: String,
        port: u16,
    },
    Conflict {
        port: u16,
    },
    Launch {
        command: String,
        cwd: String,
        url: String,
        port: u16,
    },
}

/// Plan for stopping a code editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeEditorStopPlan {
    pub stopped: bool,
    pub killed_pid: Option<u32>,
}

/// Returns true when a process is the route-owned code-server instance.
pub fn is_owned_code_server_process(
    process: &ProcessInfo,
    port: u16,
    working_directory: &str,
) -> bool {
    process.command.contains("code-server")
        && process.command.contains(&format!("--port {port}"))
        && process.command.contains(working_directory)
}

/// Inspects whether the code editor is already running.
pub fn code_editor_status(
    processes: &[ProcessInfo],
    port: u16,
    working_directory: &str,
    port_responds: bool,
    domain: impl Fn(u16) -> String,
) -> CodeEditorStatus {
    let owned = processes
        .iter()
        .any(|process| is_owned_code_server_process(process, port, working_directory));
    CodeEditorStatus {
        running: owned && port_responds,
        url: (owned && port_responds).then(|| domain(port)),
        port,
    }
}

/// Builds the code-editor start plan from process and port state.
pub fn plan_code_editor_start(
    processes: &[ProcessInfo],
    port: u16,
    working_directory: &str,
    port_responds: bool,
    domain: impl Fn(u16) -> String,
) -> CodeEditorStartPlan {
    if let Some(process) = processes
        .iter()
        .find(|process| is_owned_code_server_process(process, port, working_directory))
    {
        return CodeEditorStartPlan::Reuse {
            pid: process.pid,
            url: domain(port),
            port,
        };
    }

    if port_responds {
        return CodeEditorStartPlan::Conflict { port };
    }

    CodeEditorStartPlan::Launch {
        command: format!(
            "code-server --port {port} --auth none --bind-addr 0.0.0.0:{port} {working_directory}"
        ),
        cwd: working_directory.to_string(),
        url: domain(port),
        port,
    }
}

/// Builds the code-editor stop plan.
pub fn plan_code_editor_stop(
    processes: &[ProcessInfo],
    port: u16,
    working_directory: &str,
) -> CodeEditorStopPlan {
    let owned = processes
        .iter()
        .find(|process| is_owned_code_server_process(process, port, working_directory));
    CodeEditorStopPlan {
        stopped: owned.is_some(),
        killed_pid: owned.map(|process| process.pid),
    }
}

/// A discovered package manifest relevant to the dev-server route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevServerPackage {
    pub package_path: String,
    pub dev_script: Option<String>,
    pub manifest_mtime_ms: u64,
}

impl DevServerPackage {
    /// Creates a package probe.
    pub fn new(
        package_path: impl Into<String>,
        dev_script: Option<impl Into<String>>,
        manifest_mtime_ms: u64,
    ) -> Self {
        Self {
            package_path: package_path.into(),
            dev_script: dev_script.map(Into::into),
            manifest_mtime_ms,
        }
    }
}

/// Persisted dev-server state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevServerState {
    pub package_path: String,
    pub port: u16,
    pub pid_running: bool,
}

/// Plan returned by the dev-server route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevServerPlan {
    Existing {
        package_path: String,
        port: u16,
        url: String,
    },
    Launch {
        package_path: String,
        port: u16,
        url: String,
        install_dependencies: bool,
    },
    NotFound,
}

/// Chooses the app package to launch, preferring direct app dev scripts over a
/// root workspace orchestrator.
pub fn choose_dev_server_package(packages: &[DevServerPackage]) -> Option<&DevServerPackage> {
    packages
        .iter()
        .filter(|package| package.dev_script.is_some())
        .find(|package| package.package_path != ".")
        .or_else(|| packages.iter().find(|package| package.dev_script.is_some()))
}

/// Returns whether dependency installation should be part of the launch.
pub fn should_install_dependencies(
    node_modules_mtime_ms: Option<u64>,
    manifest_mtimes_ms: impl IntoIterator<Item = u64>,
    lockfile_mtimes_ms: impl IntoIterator<Item = u64>,
) -> bool {
    let Some(node_modules_mtime_ms) = node_modules_mtime_ms else {
        return true;
    };
    manifest_mtimes_ms
        .into_iter()
        .chain(lockfile_mtimes_ms)
        .any(|mtime| mtime > node_modules_mtime_ms)
}

/// Plans a dev-server start request.
pub fn plan_dev_server_start(
    packages: &[DevServerPackage],
    persisted_state: Option<&DevServerState>,
    node_modules_mtime_ms: Option<u64>,
    lockfile_mtimes_ms: &[u64],
    domain: impl Fn(u16) -> String,
) -> DevServerPlan {
    let port = persisted_state.map_or(3000, |state| state.port);
    if let Some(state) = persisted_state.filter(|state| state.pid_running) {
        return DevServerPlan::Existing {
            package_path: state.package_path.clone(),
            port: state.port,
            url: domain(state.port),
        };
    }

    let Some(package) = persisted_state
        .and_then(|state| {
            packages
                .iter()
                .find(|package| package.package_path == state.package_path)
        })
        .or_else(|| choose_dev_server_package(packages))
    else {
        return DevServerPlan::NotFound;
    };

    DevServerPlan::Launch {
        package_path: package.package_path.clone(),
        port,
        url: domain(port),
        install_dependencies: should_install_dependencies(
            node_modules_mtime_ms,
            packages.iter().map(|package| package.manifest_mtime_ms),
            lockfile_mtimes_ms.iter().copied(),
        ),
    }
}

/// Plans a dev-server stop request.
pub fn plan_dev_server_stop(persisted_state: Option<&DevServerState>) -> Option<DevServerState> {
    persisted_state.filter(|state| state.pid_running).cloned()
}

/// Normalizes a requested workspace file path.
pub fn normalize_workspace_file_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    if normalized.trim().is_empty() || normalized.starts_with('/') {
        return None;
    }
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        parts.push(part);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// File stat shape needed by the preview route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilePreviewStat {
    pub is_directory: bool,
    pub is_file: bool,
    pub size: u64,
}

/// Portable file-content route classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilePreviewDecision {
    InvalidPath,
    Directory,
    NotFound,
    SandboxUnavailable,
    Preview { path: String, size: u64 },
}

/// Classifies a file preview request after path normalization and sandbox stat.
pub fn classify_file_preview(
    requested_path: &str,
    stat: Result<FilePreviewStat, FilePreviewError>,
) -> FilePreviewDecision {
    let Some(path) = normalize_workspace_file_path(requested_path) else {
        return FilePreviewDecision::InvalidPath;
    };
    match stat {
        Ok(stat) if stat.is_directory => FilePreviewDecision::Directory,
        Ok(stat) if stat.is_file => FilePreviewDecision::Preview {
            path,
            size: stat.size,
        },
        Ok(_) | Err(FilePreviewError::NotFound) => FilePreviewDecision::NotFound,
        Err(FilePreviewError::SandboxUnavailable) => FilePreviewDecision::SandboxUnavailable,
    }
}

/// File preview failures that affect route status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilePreviewError {
    NotFound,
    SandboxUnavailable,
}

/// A skill suggestion returned by the session skills route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSuggestion {
    pub name: String,
    pub description: String,
    pub user_invocable: bool,
}

impl SkillSuggestion {
    /// Creates a skill suggestion.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            user_invocable: true,
        }
    }

    /// Marks this skill hidden from user suggestions.
    pub fn hidden_from_user(mut self) -> Self {
        self.user_invocable = false;
        self
    }
}

/// Filters route-visible skill suggestions.
pub fn public_skill_suggestions(skills: &[SkillSuggestion]) -> Vec<SkillSuggestion> {
    skills
        .iter()
        .filter(|skill| skill.user_invocable)
        .cloned()
        .collect()
}

/// Returns the sandbox directories used by refresh discovery.
pub fn session_skill_directories(workspace: &str, home: &str) -> Vec<String> {
    vec![
        format!("{workspace}/.claude/skills"),
        format!("{workspace}/.agents/skills"),
        format!("{home}/.agents/skills"),
    ]
}

/// One parsed name-status git entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedNameStatus {
    pub path: String,
    pub status: ParsedFileStatus,
    pub old_path: Option<String>,
}

/// Portable file status classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedFileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
}

/// Numeric diff stats for one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDiffStat {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
}

/// Synthetic untracked file diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrackedDiffFile {
    pub path: String,
    pub line_count: usize,
    pub additions: usize,
    pub diff: String,
}

/// Unescapes the quoted path format used by non-z Git diff output.
pub fn unescape_git_path(path: &str) -> String {
    path.trim_matches('"')
        .replace("\\/", "/")
        .replace("\\ ", " ")
        .replace("\\\"", "\"")
}

/// Parses non-z `git diff --name-status` output.
pub fn parse_name_status(output: &str) -> Vec<ParsedNameStatus> {
    output
        .lines()
        .filter_map(|line| {
            let columns: Vec<_> = line.split('\t').collect();
            let status = columns.first()?.chars().next()?;
            match status {
                'M' => Some(ParsedNameStatus {
                    path: unescape_git_path(columns.get(1)?),
                    status: ParsedFileStatus::Modified,
                    old_path: None,
                }),
                'A' => Some(ParsedNameStatus {
                    path: unescape_git_path(columns.get(1)?),
                    status: ParsedFileStatus::Added,
                    old_path: None,
                }),
                'D' => Some(ParsedNameStatus {
                    path: unescape_git_path(columns.get(1)?),
                    status: ParsedFileStatus::Deleted,
                    old_path: None,
                }),
                'R' => Some(ParsedNameStatus {
                    path: unescape_git_path(columns.get(2)?),
                    status: ParsedFileStatus::Renamed,
                    old_path: Some(unescape_git_path(columns.get(1)?)),
                }),
                _ => None,
            }
        })
        .collect()
}

/// Parses non-z `git diff --numstat` output.
pub fn parse_stats(output: &str) -> Vec<ParsedDiffStat> {
    output
        .lines()
        .filter_map(|line| {
            let mut columns = line.splitn(3, '\t');
            Some(ParsedDiffStat {
                additions: columns.next()?.parse().ok()?,
                deletions: columns.next()?.parse().ok()?,
                path: unescape_git_path(columns.next()?),
            })
        })
        .collect()
}

/// Splits a unified diff into one block per file path.
pub fn split_diff_by_file(full_diff: &str) -> BTreeMap<String, String> {
    let mut blocks = BTreeMap::new();
    let mut current_path: Option<String> = None;
    let mut current_lines = Vec::new();

    for line in full_diff.lines() {
        if line.starts_with("diff --git ") {
            if let Some(path) = current_path.take() {
                blocks.insert(path, current_lines.join("\n"));
                current_lines.clear();
            }
            current_path = parse_diff_git_path(line);
        }
        current_lines.push(line);
    }

    if let Some(path) = current_path {
        blocks.insert(path, current_lines.join("\n"));
    }

    blocks
}

fn parse_diff_git_path(line: &str) -> Option<String> {
    if let Some(index) = line.rfind("\"b/") {
        return line
            .get(index + 3..line.len().saturating_sub(1))
            .map(unescape_git_path);
    }
    line.split_whitespace()
        .nth(3)
        .and_then(|path| path.strip_prefix("b/"))
        .map(unescape_git_path)
}

/// Builds a synthetic diff for an untracked text file.
pub fn build_untracked_diff_file(path: &str, content: Option<&str>) -> Option<UntrackedDiffFile> {
    let content = content?;
    if content.is_empty() {
        return Some(UntrackedDiffFile {
            path: path.to_string(),
            line_count: 0,
            additions: 0,
            diff: [
                format!("diff --git a/{path} b/{path}"),
                "new file mode 100644".to_string(),
                "index 0000000..e69de29".to_string(),
            ]
            .join("\n"),
        });
    }

    let has_final_newline = content.ends_with('\n');
    let mut lines: Vec<&str> = content.split('\n').collect();
    if has_final_newline {
        lines.pop();
    }
    let line_count = lines.len();
    let range = if line_count == 1 {
        "+1".to_string()
    } else {
        format!("+1,{line_count}")
    };
    let mut diff_lines = vec![
        format!("diff --git a/{path} b/{path}"),
        "new file mode 100644".to_string(),
        "index 0000000..0000000".to_string(),
        "--- /dev/null".to_string(),
        format!("+++ b/{path}"),
        format!("@@ -0,0 {range} @@"),
    ];
    diff_lines.extend(lines.into_iter().map(|line| format!("+{line}")));
    if !has_final_newline {
        diff_lines.push("\\ No newline at end of file".to_string());
    }

    Some(UntrackedDiffFile {
        path: path.to_string(),
        line_count,
        additions: line_count,
        diff: diff_lines.join("\n"),
    })
}

/// Returns true for generated lock files that the route should de-emphasize.
pub fn is_generated_file(path: &str) -> bool {
    matches!(
        path.rsplit('/').next().unwrap_or(path),
        "bun.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "Cargo.lock"
    )
}

/// Resolves the base ref from the same ordered probes used upstream.
pub fn resolve_base_ref(
    remote_symbolic_ref: Result<&str, ()>,
    head_probe: Result<&str, ()>,
) -> Option<String> {
    if let Ok(remote) = remote_symbolic_ref {
        let trimmed = remote.trim();
        if let Some(branch) = trimmed.strip_prefix("refs/remotes/") {
            return Some(branch.to_string());
        }
    }
    head_probe.map(|_| "HEAD".to_string()).ok()
}

/// Vercel project metadata selected for a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VercelProjectSelection {
    pub project_id: String,
    pub project_name: String,
    pub team_id: Option<String>,
    pub team_slug: Option<String>,
}

/// Explicit request state for Vercel project linking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VercelProjectRequest {
    Omitted,
    Null,
    Selected(VercelProjectSelection),
}

/// Vercel project resolution failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VercelProjectError {
    SelectedProjectNoLongerMatches,
}

/// Resolves the Vercel project persisted on a new session.
pub fn resolve_vercel_project(
    request: VercelProjectRequest,
    saved_link: Option<VercelProjectSelection>,
    live_matches: &[VercelProjectSelection],
) -> Result<Option<VercelProjectSelection>, VercelProjectError> {
    match request {
        VercelProjectRequest::Omitted => Ok(saved_link),
        VercelProjectRequest::Null => Ok(None),
        VercelProjectRequest::Selected(selected) => live_matches
            .iter()
            .find(|project| project.project_id == selected.project_id)
            .cloned()
            .map(Some)
            .ok_or(VercelProjectError::SelectedProjectNoLongerMatches),
    }
}

/// Trial/demo restrictions for creating a new session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrialSessionBlock {
    AdditionalSession,
    RepoBackedSession,
}

/// Returns any hosted-demo trial block that applies.
pub fn trial_session_block(
    auth_provider: Option<&str>,
    hosted_demo: bool,
    existing_session_count: usize,
    repo_backed: bool,
) -> Option<TrialSessionBlock> {
    if auth_provider != Some("vercel") || !hosted_demo {
        return None;
    }
    if existing_session_count >= 1 {
        return Some(TrialSessionBlock::AdditionalSession);
    }
    repo_backed.then_some(TrialSessionBlock::RepoBackedSession)
}

/// Validates a GitHub repository owner segment before shell-facing work.
pub fn is_valid_repository_owner(owner: &str) -> bool {
    !owner.is_empty()
        && owner
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

/// Returns the persisted auto-create-PR override.
pub fn persisted_auto_create_pr(auto_commit_push: bool, auto_create_pr: bool) -> Option<bool> {
    (auto_commit_push && auto_create_pr).then_some(true)
}

/// Deprecation response used by `/api/sessions/:sessionId/share`.
pub fn deprecated_session_share_guidance() -> (u16, &'static str) {
    (
        410,
        "Use /api/sessions/:sessionId/chats/:chatId/share instead.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(port: u16) -> String {
        format!("https://sb-{port}.vercel.run")
    }

    #[test]
    fn session_code_editor_route_reuses_owned_process_and_rejects_unrelated_ports() {
        let unrelated = vec![ProcessInfo::new(4321, "python -m http.server 8000")];
        assert_eq!(
            code_editor_status(&unrelated, 8000, "/vercel/sandbox", true, domain),
            CodeEditorStatus {
                running: false,
                url: None,
                port: 8000,
            }
        );
        assert_eq!(
            plan_code_editor_start(&unrelated, 8000, "/vercel/sandbox", true, domain),
            CodeEditorStartPlan::Conflict { port: 8000 }
        );
        assert_eq!(
            plan_code_editor_stop(&unrelated, 8000, "/vercel/sandbox"),
            CodeEditorStopPlan {
                stopped: false,
                killed_pid: None,
            }
        );

        let owned = vec![ProcessInfo::new(
            9001,
            "code-server --port 8000 --auth none --bind-addr 0.0.0.0:8000 /vercel/sandbox",
        )];
        assert_eq!(
            plan_code_editor_start(&owned, 8000, "/vercel/sandbox", false, domain),
            CodeEditorStartPlan::Reuse {
                pid: 9001,
                url: "https://sb-8000.vercel.run".to_string(),
                port: 8000,
            }
        );
        assert_eq!(
            plan_code_editor_stop(&owned, 8000, "/vercel/sandbox"),
            CodeEditorStopPlan {
                stopped: true,
                killed_pid: Some(9001),
            }
        );

        let launch = plan_code_editor_start(&[], 8000, "/vercel/sandbox", false, domain);
        let CodeEditorStartPlan::Launch {
            command,
            cwd,
            url,
            port,
        } = launch
        else {
            panic!("expected launch plan");
        };
        assert_eq!(cwd, "/vercel/sandbox");
        assert_eq!(url, "https://sb-8000.vercel.run");
        assert_eq!(port, 8000);
        assert!(command.contains("code-server --port 8000"));
    }

    #[test]
    fn session_dev_server_route_prefers_app_reuses_state_and_plans_dependency_installs() {
        let packages = vec![
            DevServerPackage::new(".", Some("turbo dev"), 1_000),
            DevServerPackage::new("apps/web", Some("next dev"), 1_100),
        ];
        let selected = choose_dev_server_package(&packages).expect("package selected");
        assert_eq!(selected.package_path, "apps/web");

        let launch = plan_dev_server_start(&packages, None, None, &[900], domain);
        assert_eq!(
            launch,
            DevServerPlan::Launch {
                package_path: "apps/web".to_string(),
                port: 3000,
                url: "https://sb-3000.vercel.run".to_string(),
                install_dependencies: true,
            }
        );

        let state = DevServerState {
            package_path: "apps/web".to_string(),
            port: 3000,
            pid_running: true,
        };
        assert_eq!(
            plan_dev_server_start(&packages, Some(&state), Some(2_000), &[900], domain),
            DevServerPlan::Existing {
                package_path: "apps/web".to_string(),
                port: 3000,
                url: "https://sb-3000.vercel.run".to_string(),
            }
        );
        let later_packages = vec![
            DevServerPackage::new("apps/admin", Some("next dev"), 3_000),
            DevServerPackage::new("apps/web", Some("next dev"), 1_100),
        ];
        assert_eq!(
            plan_dev_server_start(&later_packages, Some(&state), Some(2_000), &[900], domain),
            DevServerPlan::Existing {
                package_path: "apps/web".to_string(),
                port: 3000,
                url: "https://sb-3000.vercel.run".to_string(),
            }
        );
        assert_eq!(plan_dev_server_stop(Some(&state)), Some(state));
        assert!(should_install_dependencies(Some(5_000), [6_000], [4_000]));
        assert!(!should_install_dependencies(Some(10_000), [1_000], [900]));
        assert_eq!(
            plan_dev_server_start(
                &[DevServerPackage::new(".", None::<String>, 1_000)],
                None,
                None,
                &[],
                domain
            ),
            DevServerPlan::NotFound
        );
    }

    #[test]
    fn session_diff_route_parses_git_output_and_untracked_files() {
        assert_eq!(
            unescape_git_path("\"src\\/new\\ file.ts\""),
            "src/new file.ts"
        );
        let name_status = parse_name_status(
            "M\tREADME.md\nA\t\"src\\/new\\ file.ts\"\nD\told.ts\nR100\t\"old\\/name.ts\"\t\"new\\/name.ts\"",
        );
        assert_eq!(name_status.len(), 4);
        assert_eq!(name_status[1].path, "src/new file.ts");
        assert_eq!(name_status[3].old_path.as_deref(), Some("old/name.ts"));

        let stats = parse_stats("12\t3\tsrc/app.ts\n1\t0\t\"docs\\/new\\ file.md\"");
        assert_eq!(
            stats[1],
            ParsedDiffStat {
                path: "docs/new file.md".to_string(),
                additions: 1,
                deletions: 0,
            }
        );

        let split = split_diff_by_file(
            "diff --git a/src/a.ts b/src/a.ts\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1 +1 @@\n-a\n+b\ndiff --git \"a/docs/old name.md\" \"b/docs/new name.md\"\n--- \"a/docs/old name.md\"\n+++ \"b/docs/new name.md\"",
        );
        assert!(split.contains_key("src/a.ts"));
        assert!(split.contains_key("docs/new name.md"));

        assert!(build_untracked_diff_file("file.ts", None).is_none());
        let new_file =
            build_untracked_diff_file("src/new.ts", Some("line1\nline2\n")).expect("new file diff");
        assert_eq!(new_file.line_count, 2);
        assert!(new_file.diff.contains("@@ -0,0 +1,2 @@"));
        let empty = build_untracked_diff_file("src/empty.ts", Some("")).expect("empty diff");
        assert_eq!(empty.additions, 0);
        let trailing =
            build_untracked_diff_file("src/new.ts", Some("line1\n\n")).expect("trailing diff");
        assert_eq!(trailing.line_count, 2);
        let no_final_newline =
            build_untracked_diff_file("src/new.ts", Some("line1")).expect("newline marker");
        assert!(
            no_final_newline
                .diff
                .contains("\\ No newline at end of file")
        );
        assert!(is_generated_file("pnpm-lock.yaml"));
        assert!(!is_generated_file("src/index.ts"));
        assert_eq!(
            resolve_base_ref(Ok("refs/remotes/origin/main\n"), Err(())),
            Some("origin/main".to_string())
        );
        assert_eq!(
            resolve_base_ref(Err(()), Ok("abc1234\n")),
            Some("HEAD".to_string())
        );
        assert_eq!(resolve_base_ref(Err(()), Err(())), None);
    }

    #[test]
    fn session_files_content_route_normalizes_paths_and_classifies_sandbox_failures() {
        assert_eq!(
            normalize_workspace_file_path("apps\\web\\lib\\test file.ts"),
            Some("apps/web/lib/test file.ts".to_string())
        );
        assert_eq!(normalize_workspace_file_path("../secrets.txt"), None);
        assert_eq!(
            classify_file_preview(
                "../secrets.txt",
                Ok(FilePreviewStat {
                    is_directory: false,
                    is_file: true,
                    size: 1,
                }),
            ),
            FilePreviewDecision::InvalidPath
        );
        assert_eq!(
            classify_file_preview(
                "apps/web/lib/test.ts",
                Ok(FilePreviewStat {
                    is_directory: false,
                    is_file: true,
                    size: 42,
                }),
            ),
            FilePreviewDecision::Preview {
                path: "apps/web/lib/test.ts".to_string(),
                size: 42,
            }
        );
        assert_eq!(
            classify_file_preview(
                "apps/web/components",
                Ok(FilePreviewStat {
                    is_directory: true,
                    is_file: false,
                    size: 0,
                }),
            ),
            FilePreviewDecision::Directory
        );
        assert_eq!(
            classify_file_preview("missing.ts", Err(FilePreviewError::NotFound)),
            FilePreviewDecision::NotFound
        );
        assert_eq!(
            classify_file_preview(
                "apps/web/lib/test.ts",
                Err(FilePreviewError::SandboxUnavailable)
            ),
            FilePreviewDecision::SandboxUnavailable
        );
    }

    #[test]
    fn session_skills_route_uses_cache_until_refresh_requests_discovery() {
        let cached = vec![
            SkillSuggestion::new("ship", "Deploy the current project"),
            SkillSuggestion::new("internal", "Hidden skill").hidden_from_user(),
        ];
        assert_eq!(
            public_skill_suggestions(&cached),
            vec![SkillSuggestion::new("ship", "Deploy the current project")]
        );
        assert_eq!(
            session_skill_directories("/workspace", "/root"),
            vec![
                "/workspace/.claude/skills".to_string(),
                "/workspace/.agents/skills".to_string(),
                "/root/.agents/skills".to_string(),
            ]
        );
    }

    #[test]
    fn sessions_route_enforces_trial_vercel_linking_skill_and_auto_pr_policy() {
        assert_eq!(
            trial_session_block(Some("vercel"), true, 1, false),
            Some(TrialSessionBlock::AdditionalSession)
        );
        assert_eq!(
            trial_session_block(Some("vercel"), true, 0, true),
            Some(TrialSessionBlock::RepoBackedSession)
        );

        let selected = VercelProjectSelection {
            project_id: "project-1".to_string(),
            project_name: "tampered-name".to_string(),
            team_id: Some("team-x".to_string()),
            team_slug: Some("tampered-team".to_string()),
        };
        let live = VercelProjectSelection {
            project_id: "project-1".to_string(),
            project_name: "app".to_string(),
            team_id: Some("team-1".to_string()),
            team_slug: Some("acme".to_string()),
        };
        assert_eq!(
            resolve_vercel_project(
                VercelProjectRequest::Selected(selected),
                None,
                std::slice::from_ref(&live),
            )
            .expect("selected project resolves"),
            Some(live.clone())
        );
        assert_eq!(
            resolve_vercel_project(
                VercelProjectRequest::Selected(VercelProjectSelection {
                    project_id: "project-999".to_string(),
                    project_name: "rogue-project".to_string(),
                    team_id: None,
                    team_slug: None,
                }),
                None,
                std::slice::from_ref(&live),
            ),
            Err(VercelProjectError::SelectedProjectNoLongerMatches)
        );
        assert_eq!(
            resolve_vercel_project(VercelProjectRequest::Omitted, Some(live.clone()), &[])
                .expect("saved link is reused"),
            Some(live)
        );
        assert_eq!(
            resolve_vercel_project(VercelProjectRequest::Null, None, &[])
                .expect("null suppresses linking"),
            None
        );
        assert!(is_valid_repository_owner("Vercel-Labs"));
        assert!(!is_valid_repository_owner("vercel\" && echo nope && \""));
        let skill_refs = serde_json::json!([{ "source": "vercel/ai", "skillName": "ai-sdk" }]);
        assert_eq!(skill_refs[0]["skillName"], "ai-sdk");
        assert_eq!(persisted_auto_create_pr(true, true), Some(true));
        assert_eq!(persisted_auto_create_pr(false, true), None);
    }

    #[test]
    fn deprecated_session_share_route_returns_gone_guidance() {
        let (status, guidance) = deprecated_session_share_guidance();
        assert_eq!(status, 410);
        assert!(guidance.contains("/api/sessions/:sessionId/chats/:chatId/share"));
    }
}
