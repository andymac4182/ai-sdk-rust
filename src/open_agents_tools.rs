use std::collections::BTreeSet;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Component, Path};

use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use url::Url;

use crate::json::{JsonObject, JsonSchema, JsonValue};
use crate::language_model::LanguageModelToolResultOutput;
use crate::provider_utils::{
    SandboxCommandOptions, SandboxCommandResult, Tool, ToolExecutionError, ToolExecutionOptions,
};

const READ_TOOL_NAME: &str = "read";
const WRITE_TOOL_NAME: &str = "write";
const EDIT_TOOL_NAME: &str = "edit";
const GREP_TOOL_NAME: &str = "grep";
const GLOB_TOOL_NAME: &str = "glob";
const BASH_TOOL_NAME: &str = "bash";
const TODO_WRITE_TOOL_NAME: &str = "todo_write";
const TASK_TOOL_NAME: &str = "task";
const ASK_USER_QUESTION_TOOL_NAME: &str = "ask_user_question";
const SKILL_TOOL_NAME: &str = "skill";
const WEB_FETCH_TOOL_NAME: &str = "web_fetch";
const DEFAULT_READ_LIMIT: usize = 2000;
const DEFAULT_GLOB_LIMIT: usize = 100;
const MAX_GREP_MATCHES: usize = 100;
const MAX_GREP_MATCHES_PER_FILE: usize = 10;
const MAX_GREP_LINE_LENGTH: usize = 200;
const BASH_TIMEOUT_MS: u64 = 120_000;
const SEARCH_TIMEOUT_MS: u64 = 30_000;
const FETCH_TIMEOUT_MS: u64 = 30_000;
const DNS_TIMEOUT_MS: u64 = 5_000;

/// Maximum number of response body bytes returned by the Open Agents web fetch tool.
pub const OPEN_AGENT_WEB_FETCH_MAX_BODY_LENGTH: usize = 10_000;

/// Open Agents tool approval policy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OpenAgentToolApprovalPolicy {
    /// Match Open Agents defaults: request approval for sensitive files, risky shell
    /// commands, and web fetch.
    #[default]
    Sensitive,

    /// Never request tool-defined approvals.
    Never,

    /// Request approval whenever a tool in this surface exposes an approval check.
    Always,
}

impl OpenAgentToolApprovalPolicy {
    fn requires_approval(self, sensitive: bool) -> bool {
        match self {
            Self::Sensitive => sensitive,
            Self::Never => false,
            Self::Always => true,
        }
    }
}

/// Options used when constructing the Open Agents Rust tool surface.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OpenAgentToolsOptions {
    /// Working directory used for sandbox commands. When omitted, the sandbox
    /// implementation's default working directory is used.
    pub working_directory: Option<String>,

    /// Tool-defined approval policy.
    pub approval_policy: OpenAgentToolApprovalPolicy,
}

impl OpenAgentToolsOptions {
    /// Creates default Open Agents tool options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the sandbox working directory used by tool commands.
    pub fn with_working_directory(mut self, working_directory: impl Into<String>) -> Self {
        self.working_directory = Some(working_directory.into());
        self
    }

    /// Sets the approval policy used by tools that classify risky inputs.
    pub fn with_approval_policy(mut self, approval_policy: OpenAgentToolApprovalPolicy) -> Self {
        self.approval_policy = approval_policy;
        self
    }
}

/// Returns the default Open Agents tool set for [`crate::ToolLoopAgent`].
pub fn open_agent_tools() -> Vec<Tool> {
    open_agent_tools_with_options(OpenAgentToolsOptions::default())
}

/// Returns the Open Agents tool set configured for a sandbox working directory.
pub fn open_agent_tools_in_workspace(working_directory: impl Into<String>) -> Vec<Tool> {
    open_agent_tools_with_options(
        OpenAgentToolsOptions::new().with_working_directory(working_directory),
    )
}

/// Returns the Open Agents tool set for [`crate::ToolLoopAgent`].
///
/// This covers the tool names used by Open Agents:
/// `todo_write`, `read`, `write`, `edit`, `grep`, `glob`, `bash`, `task`,
/// `ask_user_question`, `skill`, and `web_fetch`.
pub fn open_agent_tools_with_options(options: OpenAgentToolsOptions) -> Vec<Tool> {
    vec![
        todo_write_tool(),
        read_tool(options.clone()),
        write_tool(options.clone()),
        edit_tool(options.clone()),
        grep_tool(options.clone()),
        glob_tool(options.clone()),
        bash_tool(options.clone()),
        task_placeholder_tool(),
        ask_user_question_tool(),
        skill_placeholder_tool(),
        web_fetch_tool(options),
    ]
}

/// Returns true when a workspace path syntactically targets a dotenv file.
pub fn is_dotenv_file_path(file_path: &str) -> bool {
    file_path
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .starts_with(".env")
}

/// Classifies shell commands that should require approval before execution.
pub fn command_needs_approval(command: &str) -> bool {
    let trimmed = command.trim();
    let lowered = trimmed.to_ascii_lowercase();

    dangerous_command_patterns()
        .iter()
        .any(|pattern| pattern.is_match(trimmed))
        || sensitive_command_patterns()
            .iter()
            .any(|pattern| pattern.is_match(&lowered))
}

/// Returns true when a URL uses http(s) and does not directly name a private host.
pub fn is_allowed_web_url(value: &str) -> bool {
    let Ok(parsed) = Url::parse(value) else {
        return false;
    };

    matches!(parsed.scheme(), "http" | "https")
        && parsed
            .host_str()
            .is_some_and(|hostname| !is_private_host(hostname))
}

fn read_tool(options: OpenAgentToolsOptions) -> Tool {
    let approval_options = options.clone();
    Tool::new(READ_TOOL_NAME, read_schema())
        .with_description(
            "Read a workspace-relative file from the sandbox with line numbers and optional line slicing.",
        )
        .with_needs_approval_function(move |input, _options| {
            let approval_options = approval_options.clone();
            async move {
                let sensitive = string_field(&input, "filePath").is_some_and(is_dotenv_file_path);
                approval_options.approval_policy.requires_approval(sensitive)
            }
        })
        .with_execute(move |input, execution_options| {
            let options = options.clone();
            async move { execute_read(input, execution_options, options).await }
        })
}

fn write_tool(options: OpenAgentToolsOptions) -> Tool {
    let approval_options = options.clone();
    Tool::new(WRITE_TOOL_NAME, write_schema())
        .with_description("Write content to a workspace-relative file in the sandbox.")
        .with_needs_approval_function(move |input, _options| {
            let approval_options = approval_options.clone();
            async move {
                let sensitive = string_field(&input, "filePath").is_some_and(is_dotenv_file_path);
                approval_options
                    .approval_policy
                    .requires_approval(sensitive)
            }
        })
        .with_execute(move |input, execution_options| {
            let options = options.clone();
            async move { execute_write(input, execution_options, options).await }
        })
}

fn edit_tool(options: OpenAgentToolsOptions) -> Tool {
    let approval_options = options.clone();
    Tool::new(EDIT_TOOL_NAME, edit_schema())
        .with_description("Perform exact string replacement in a workspace-relative sandbox file.")
        .with_needs_approval_function(move |input, _options| {
            let approval_options = approval_options.clone();
            async move {
                let sensitive = string_field(&input, "filePath").is_some_and(is_dotenv_file_path);
                approval_options
                    .approval_policy
                    .requires_approval(sensitive)
            }
        })
        .with_execute(move |input, execution_options| {
            let options = options.clone();
            async move { execute_edit(input, execution_options, options).await }
        })
}

fn grep_tool(options: OpenAgentToolsOptions) -> Tool {
    Tool::new(GREP_TOOL_NAME, grep_schema())
        .with_description("Search workspace files for a single-line regex pattern.")
        .with_execute(move |input, execution_options| {
            let options = options.clone();
            async move { execute_grep(input, execution_options, options).await }
        })
}

fn glob_tool(options: OpenAgentToolsOptions) -> Tool {
    Tool::new(GLOB_TOOL_NAME, glob_schema())
        .with_description("Find workspace files matching a glob pattern.")
        .with_execute(move |input, execution_options| {
            let options = options.clone();
            async move { execute_glob(input, execution_options, options).await }
        })
}

fn bash_tool(options: OpenAgentToolsOptions) -> Tool {
    let approval_options = options.clone();
    Tool::new(BASH_TOOL_NAME, bash_schema())
        .with_description("Execute a non-interactive bash command in the sandbox.")
        .with_needs_approval_function(move |input, _options| {
            let approval_options = approval_options.clone();
            async move {
                let sensitive = string_field(&input, "command").is_some_and(command_needs_approval);
                approval_options
                    .approval_policy
                    .requires_approval(sensitive)
            }
        })
        .with_execute(move |input, execution_options| {
            let options = options.clone();
            async move { execute_bash(input, execution_options, options).await }
        })
}

fn todo_write_tool() -> Tool {
    Tool::new(TODO_WRITE_TOOL_NAME, todo_write_schema())
        .with_description("Create and manage a structured task list for the current session.")
        .with_execute(|input, _options| async move {
            let todos = input.get("todos").cloned().unwrap_or_else(|| json!([]));
            let count = todos.as_array().map_or(0, Vec::len);
            Ok(json!({
                "success": true,
                "message": format!("Updated task list with {count} items"),
                "todos": todos
            }))
        })
}

fn task_placeholder_tool() -> Tool {
    Tool::new(TASK_TOOL_NAME, task_schema()).with_description(
        "Typed extension point for Open Agents subagent execution. This tool is intentionally non-executable until the subagent bucket lands.",
    )
}

fn skill_placeholder_tool() -> Tool {
    Tool::new(SKILL_TOOL_NAME, skill_schema()).with_description(
        "Typed extension point for Open Agents skill invocation. This tool is intentionally non-executable until the skill bucket lands.",
    )
}

fn ask_user_question_tool() -> Tool {
    Tool::new(ASK_USER_QUESTION_TOOL_NAME, ask_user_question_schema())
        .with_description(
            "Ask the user structured questions and pause the durable run for answers.",
        )
        .with_output_schema(ask_user_question_output_schema())
        .with_to_model_output(|options| async move {
            LanguageModelToolResultOutput::text(format_ask_user_question_model_output(
                &options.output,
            ))
        })
}

fn web_fetch_tool(options: OpenAgentToolsOptions) -> Tool {
    let approval_options = options.clone();
    Tool::new(WEB_FETCH_TOOL_NAME, web_fetch_schema())
        .with_description("Fetch an external http(s) URL with SSRF-resistant host checks.")
        .with_output_schema(web_fetch_output_schema())
        .with_needs_approval_function(move |_input, _options| {
            let approval_options = approval_options.clone();
            async move { approval_options.approval_policy.requires_approval(true) }
        })
        .with_execute(move |input, execution_options| {
            let options = options.clone();
            async move { execute_web_fetch(input, execution_options, options).await }
        })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadInput {
    file_path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteInput {
    file_path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditInput {
    file_path: String,
    old_string: String,
    new_string: String,
    replace_all: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrepInput {
    pattern: String,
    path: String,
    glob: Option<String>,
    case_sensitive: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GlobInput {
    pattern: String,
    path: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BashInput {
    command: String,
    cwd: Option<String>,
    detached: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebFetchInput {
    url: String,
    method: Option<WebFetchMethod>,
    headers: Option<JsonObject>,
    body: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
enum WebFetchMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
}

impl fmt::Display for WebFetchMethod {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
        })
    }
}

async fn execute_read(
    input: JsonValue,
    options: ToolExecutionOptions,
    tool_options: OpenAgentToolsOptions,
) -> Result<JsonValue, ToolExecutionError> {
    let input: ReadInput = parse_input(input)?;
    let path = match WorkspacePath::resolve_file(&input.file_path) {
        Ok(path) => path,
        Err(error) => return Ok(tool_error(error)),
    };

    if let Err(error) = ensure_realpath_safe(&options, &tool_options, &path.path, false).await {
        return Ok(tool_error(error));
    }

    let directory_check = run_command(
        &options,
        &tool_options,
        format!("[ -d {} ]", shell_escape(&path.path)),
    )
    .await?;
    if directory_check.exit_code == 0 {
        return Ok(tool_error(
            "Cannot read a directory. Use glob or ls command instead.",
        ));
    }

    let result = run_command(
        &options,
        &tool_options,
        format!("cat -- {}", shell_escape(&path.path)),
    )
    .await?;
    if result.exit_code != 0 {
        return Ok(tool_error(format!(
            "Failed to read file: {}",
            command_error_output(&result)
        )));
    }

    let lines = result.stdout.split('\n').collect::<Vec<_>>();
    let start_line = input.offset.unwrap_or(1).max(1);
    let limit = input.limit.unwrap_or(DEFAULT_READ_LIMIT);
    let start_index = start_line.saturating_sub(1).min(lines.len());
    let end_index = start_index.saturating_add(limit).min(lines.len());
    let content = lines[start_index..end_index]
        .iter()
        .enumerate()
        .map(|(index, line)| format!("{}: {line}", start_index + index + 1))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(json!({
        "success": true,
        "path": path.display,
        "totalLines": lines.len(),
        "startLine": start_index + 1,
        "endLine": end_index,
        "content": content
    }))
}

async fn execute_write(
    input: JsonValue,
    options: ToolExecutionOptions,
    tool_options: OpenAgentToolsOptions,
) -> Result<JsonValue, ToolExecutionError> {
    let input: WriteInput = parse_input(input)?;
    if input.content.contains('\0') {
        return Ok(tool_error("Cannot write content containing NUL bytes."));
    }

    let path = match WorkspacePath::resolve_file(&input.file_path) {
        Ok(path) => path,
        Err(error) => return Ok(tool_error(error)),
    };

    if let Err(error) = ensure_realpath_safe(&options, &tool_options, &path.path, true).await {
        return Ok(tool_error(error));
    }

    let parent = parent_path(&path.path);
    let command = format!(
        "mkdir -p -- {} && printf '%s' {} > {}",
        shell_escape(parent),
        shell_escape(&input.content),
        shell_escape(&path.path),
    );
    let result = run_command(&options, &tool_options, command).await?;
    if result.exit_code != 0 {
        return Ok(tool_error(format!(
            "Failed to write file: {}",
            command_error_output(&result)
        )));
    }

    Ok(json!({
        "success": true,
        "path": path.display,
        "bytesWritten": input.content.len()
    }))
}

async fn execute_edit(
    input: JsonValue,
    options: ToolExecutionOptions,
    tool_options: OpenAgentToolsOptions,
) -> Result<JsonValue, ToolExecutionError> {
    let input: EditInput = parse_input(input)?;
    if input.old_string.is_empty() {
        return Ok(tool_error("oldString must not be empty"));
    }
    if input.old_string == input.new_string {
        return Ok(tool_error("oldString and newString must be different"));
    }

    let read_output = execute_read(
        json!({
            "filePath": input.file_path,
            "offset": 1,
            "limit": usize::MAX / 2
        }),
        options.clone(),
        tool_options.clone(),
    )
    .await?;
    if !read_output["success"].as_bool().unwrap_or(false) {
        return Ok(read_output);
    }

    let path = WorkspacePath::resolve_file(
        read_output["path"]
            .as_str()
            .ok_or_else(|| ToolExecutionError::new("Read tool returned a non-string path."))?,
    )?;
    let content_result = run_command(
        &options,
        &tool_options,
        format!("cat -- {}", shell_escape(&path.path)),
    )
    .await?;
    if content_result.exit_code != 0 {
        return Ok(tool_error(format!(
            "Failed to edit file: {}",
            command_error_output(&content_result)
        )));
    }

    let content = content_result.stdout;
    if !content.contains(&input.old_string) {
        return Ok(json!({
            "success": false,
            "error": "oldString not found in file",
            "hint": "Make sure to match exact whitespace and indentation"
        }));
    }

    let occurrences = content.matches(&input.old_string).count();
    let replace_all = input.replace_all.unwrap_or(false);
    if occurrences > 1 && !replace_all {
        return Ok(tool_error(format!(
            "oldString found {occurrences} times. Use replaceAll=true or provide more context to make it unique."
        )));
    }

    let match_index = content
        .find(&input.old_string)
        .expect("contains was checked before find");
    let start_line = content[..match_index].split('\n').count();
    let new_content = if replace_all {
        content.replace(&input.old_string, &input.new_string)
    } else {
        content.replacen(&input.old_string, &input.new_string, 1)
    };

    let write_output = execute_write(
        json!({
            "filePath": path.path,
            "content": new_content
        }),
        options,
        tool_options,
    )
    .await?;
    if !write_output["success"].as_bool().unwrap_or(false) {
        return Ok(write_output);
    }

    Ok(json!({
        "success": true,
        "path": path.display,
        "replacements": if replace_all { occurrences } else { 1 },
        "startLine": start_line
    }))
}

async fn execute_grep(
    input: JsonValue,
    options: ToolExecutionOptions,
    tool_options: OpenAgentToolsOptions,
) -> Result<JsonValue, ToolExecutionError> {
    let input: GrepInput = parse_input(input)?;
    let base_path = match WorkspacePath::resolve_directory(&input.path) {
        Ok(path) => path,
        Err(error) => return Ok(tool_error(error)),
    };
    if let Err(error) = ensure_realpath_safe(&options, &tool_options, &base_path.path, false).await
    {
        return Ok(tool_error(error));
    }

    let regex = match Regex::new(&regex_pattern(
        &input.pattern,
        input.case_sensitive.unwrap_or(true),
    )) {
        Ok(regex) => regex,
        Err(error) => return Ok(tool_error(format!("Grep failed: {error}"))),
    };

    let files = list_workspace_files(&options, &tool_options, &base_path.path).await?;
    let mut matches = Vec::new();
    let mut files_with_matches = BTreeSet::new();
    let mut per_file = JsonObject::new();

    for file in files {
        if matches.len() >= MAX_GREP_MATCHES {
            break;
        }
        if input
            .glob
            .as_deref()
            .is_some_and(|pattern| !glob_matches(pattern, &file.display))
        {
            continue;
        }

        let content_result = run_command(
            &options,
            &tool_options,
            format!("cat -- {}", shell_escape(&file.path)),
        )
        .await?;
        if content_result.exit_code != 0 {
            continue;
        }

        for (line_index, line) in content_result.stdout.lines().enumerate() {
            if matches.len() >= MAX_GREP_MATCHES {
                break;
            }
            if !regex.is_match(line) {
                continue;
            }

            let current_count = per_file
                .get(&file.display)
                .and_then(JsonValue::as_u64)
                .unwrap_or(0) as usize;
            if current_count >= MAX_GREP_MATCHES_PER_FILE {
                continue;
            }

            files_with_matches.insert(file.display.clone());
            per_file.insert(file.display.clone(), json!(current_count + 1));
            matches.push(json!({
                "file": file.display,
                "line": line_index + 1,
                "content": truncate_chars(line, MAX_GREP_LINE_LENGTH)
            }));
        }
    }

    Ok(json!({
        "success": true,
        "pattern": input.pattern,
        "matchCount": matches.len(),
        "filesWithMatches": files_with_matches.len(),
        "matches": matches
    }))
}

async fn execute_glob(
    input: JsonValue,
    options: ToolExecutionOptions,
    tool_options: OpenAgentToolsOptions,
) -> Result<JsonValue, ToolExecutionError> {
    let input: GlobInput = parse_input(input)?;
    let base_path = match input.path.as_deref() {
        Some(path) => WorkspacePath::resolve_directory(path),
        None => Ok(WorkspacePath::root()),
    };
    let base_path = match base_path {
        Ok(path) => path,
        Err(error) => return Ok(tool_error(error)),
    };
    if let Err(error) = ensure_realpath_safe(&options, &tool_options, &base_path.path, false).await
    {
        return Ok(tool_error(error));
    }

    let mut files = list_workspace_files(&options, &tool_options, &base_path.path)
        .await?
        .into_iter()
        .filter(|file| glob_matches(&input.pattern, &file.display))
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        right
            .modified_at_seconds
            .cmp(&left.modified_at_seconds)
            .then_with(|| left.display.cmp(&right.display))
    });

    let limit = input.limit.unwrap_or(DEFAULT_GLOB_LIMIT);
    let files = files
        .into_iter()
        .take(limit)
        .map(|file| {
            json!({
                "path": file.display,
                "size": file.size,
                "modifiedAt": unix_seconds_to_iso(file.modified_at_seconds)
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "success": true,
        "pattern": input.pattern,
        "baseDir": base_path.display,
        "count": files.len(),
        "files": files
    }))
}

async fn execute_bash(
    input: JsonValue,
    options: ToolExecutionOptions,
    tool_options: OpenAgentToolsOptions,
) -> Result<JsonValue, ToolExecutionError> {
    let input: BashInput = parse_input(input)?;
    if input.detached.unwrap_or(false) {
        return Ok(json!({
            "success": false,
            "exitCode": JsonValue::Null,
            "stdout": "",
            "stderr": "Detached mode is not supported by the current Rust sandbox boundary."
        }));
    }

    let cwd = match input.cwd.as_deref() {
        Some(cwd) => match WorkspacePath::resolve_directory(cwd) {
            Ok(path) => Some(path.path),
            Err(error) => return Ok(tool_error(error)),
        },
        None => None,
    };

    let result = run_command_with_cwd(
        &options,
        &tool_options,
        input.command,
        cwd.as_deref(),
        BASH_TIMEOUT_MS,
    )
    .await?;

    Ok(json!({
        "success": result.exit_code == 0,
        "exitCode": result.exit_code,
        "stdout": result.stdout,
        "stderr": result.stderr
    }))
}

async fn execute_web_fetch(
    input: JsonValue,
    options: ToolExecutionOptions,
    tool_options: OpenAgentToolsOptions,
) -> Result<JsonValue, ToolExecutionError> {
    let input: WebFetchInput = parse_input(input)?;
    if !is_allowed_web_url(&input.url) {
        return Ok(tool_error("URL must use http(s) and a public host"));
    }

    let parsed_url = Url::parse(&input.url)
        .map_err(|error| ToolExecutionError::new(format!("Invalid URL: {error}")))?;
    let hostname = parsed_url
        .host_str()
        .ok_or_else(|| ToolExecutionError::new("URL is missing a host."))?;
    if resolves_to_private_host(&options, &tool_options, hostname).await? {
        return Ok(tool_error(
            "Fetch failed: URL resolves to a private or internal host",
        ));
    }

    let method = input.method.unwrap_or(WebFetchMethod::Get);
    let mut command_parts = vec![
        "curl".to_string(),
        "-sS".to_string(),
        "--proto".to_string(),
        shell_escape("=http,https"),
        "--proto-redir".to_string(),
        shell_escape("=http,https"),
        "-X".to_string(),
        method.to_string(),
        "--max-time".to_string(),
        (FETCH_TIMEOUT_MS / 1000).to_string(),
        "-w".to_string(),
        shell_escape("\n%{http_code}"),
    ];

    if let Some(headers) = input.headers {
        for (key, value) in headers {
            let Some(value) = value.as_str() else {
                return Ok(tool_error("Fetch failed: header values must be strings"));
            };
            command_parts.push("-H".to_string());
            command_parts.push(shell_escape(&format!("{key}: {value}")));
        }
    }

    if !matches!(method, WebFetchMethod::Get | WebFetchMethod::Head) {
        if let Some(body) = input.body {
            command_parts.push("-d".to_string());
            command_parts.push(shell_escape(&body));
        }
    }

    command_parts.push(shell_escape(&input.url));
    let result = run_command_with_timeout(
        &options,
        &tool_options,
        command_parts.join(" "),
        FETCH_TIMEOUT_MS,
    )
    .await?;
    if result.exit_code != 0 {
        return Ok(tool_error(format!(
            "Fetch failed: {}",
            command_error_output(&result)
        )));
    }

    let output = result.stdout;
    let Some(last_newline) = output.rfind('\n') else {
        return Ok(json!({
            "success": true,
            "status": JsonValue::Null,
            "body": truncate_chars(&output, OPEN_AGENT_WEB_FETCH_MAX_BODY_LENGTH),
            "truncated": output.len() > OPEN_AGENT_WEB_FETCH_MAX_BODY_LENGTH
        }));
    };
    let body = &output[..last_newline];
    let status = output[last_newline + 1..].trim().parse::<u16>().ok();
    let truncated = body.len() > OPEN_AGENT_WEB_FETCH_MAX_BODY_LENGTH;

    Ok(json!({
        "success": true,
        "status": status,
        "body": truncate_chars(body, OPEN_AGENT_WEB_FETCH_MAX_BODY_LENGTH),
        "truncated": truncated
    }))
}

async fn resolves_to_private_host(
    options: &ToolExecutionOptions,
    tool_options: &OpenAgentToolsOptions,
    hostname: &str,
) -> Result<bool, ToolExecutionError> {
    if is_private_host(hostname) {
        return Ok(true);
    }

    let command = format!(
        "getent ahosts {} | awk '{{print $1}}' | sort -u",
        shell_escape(hostname)
    );
    let result = run_command_with_timeout(options, tool_options, command, DNS_TIMEOUT_MS).await?;
    if result.exit_code != 0 {
        return Ok(true);
    }

    Ok(result
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .any(is_private_host))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkspacePath {
    path: String,
    display: String,
}

impl WorkspacePath {
    fn root() -> Self {
        Self {
            path: ".".to_string(),
            display: ".".to_string(),
        }
    }

    fn resolve_file(path: &str) -> Result<Self, String> {
        Self::resolve(path, false)
    }

    fn resolve_directory(path: &str) -> Result<Self, String> {
        Self::resolve(path, true)
    }

    fn resolve(path: &str, allow_root: bool) -> Result<Self, String> {
        if path.contains('\0') {
            return Err("Path must not contain NUL bytes.".to_string());
        }

        let normalized = path.replace('\\', "/");
        if normalized.trim().is_empty() {
            return Err("Path must not be empty.".to_string());
        }
        if normalized.starts_with('/') || has_windows_drive_prefix(&normalized) {
            return Err("Path must stay within the workspace.".to_string());
        }

        let mut parts = Vec::new();
        for component in Path::new(&normalized).components() {
            match component {
                Component::Normal(part) => {
                    parts.push(part.to_string_lossy().into_owned());
                }
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err("Path must stay within the workspace.".to_string());
                }
            }
        }

        if parts.is_empty() {
            if allow_root {
                return Ok(Self::root());
            }
            return Err("Path must refer to a workspace file.".to_string());
        }

        let display = parts.join("/");
        Ok(Self {
            path: display.clone(),
            display,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkspaceFile {
    path: String,
    display: String,
    size: u64,
    modified_at_seconds: i64,
}

async fn list_workspace_files(
    options: &ToolExecutionOptions,
    tool_options: &OpenAgentToolsOptions,
    base_path: &str,
) -> Result<Vec<WorkspaceFile>, ToolExecutionError> {
    let command = format!(
        "find {} -not -path '*/.*' -not -path '*/node_modules/*' -type f -print | while IFS= read -r p; do size=$(wc -c < \"$p\" | tr -d ' '); mtime=$(stat -f %m \"$p\" 2>/dev/null || stat -c %Y \"$p\" 2>/dev/null || echo 0); printf '%s\\t%s\\t%s\\n' \"$mtime\" \"$size\" \"$p\"; done",
        shell_escape(base_path)
    );
    let result =
        run_command_with_timeout(options, tool_options, command, SEARCH_TIMEOUT_MS).await?;
    if result.exit_code != 0 {
        return Err(ToolExecutionError::new(format!(
            "Failed to list files: {}",
            command_error_output(&result)
        )));
    }

    let mut files = Vec::new();
    for line in result.stdout.lines() {
        let mut parts = line.splitn(3, '\t');
        let modified_at_seconds = parts
            .next()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        let size = parts
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let Some(path) = parts.next() else {
            continue;
        };
        let display = strip_current_dir(path);
        files.push(WorkspaceFile {
            path: display.clone(),
            display,
            size,
            modified_at_seconds,
        });
    }

    Ok(files)
}

async fn ensure_realpath_safe(
    options: &ToolExecutionOptions,
    tool_options: &OpenAgentToolsOptions,
    path: &str,
    allow_missing_leaf: bool,
) -> Result<(), String> {
    let command = if allow_missing_leaf {
        format!(
            "root=$(pwd -P); dir={}; candidate=\"$dir\"; while [ \"$candidate\" != \".\" ] && [ ! -e \"$candidate\" ]; do candidate=$(dirname \"$candidate\"); done; target=$(realpath -- \"$candidate\" 2>/dev/null || true); if [ -n \"$target\" ]; then case \"$target\" in \"$root\"|\"$root\"/*) exit 0;; *) printf '%s' \"$target\"; exit 78;; esac; fi",
            shell_escape(parent_path(path))
        )
    } else {
        format!(
            "root=$(pwd -P); target=$(realpath -- {} 2>/dev/null || true); if [ -n \"$target\" ]; then case \"$target\" in \"$root\"|\"$root\"/*) exit 0;; *) printf '%s' \"$target\"; exit 78;; esac; fi",
            shell_escape(path)
        )
    };

    let result = run_command(options, tool_options, command)
        .await
        .map_err(|error| error.to_string())?;
    if result.exit_code == 78 {
        return Err("Path resolves outside the workspace.".to_string());
    }
    if result.exit_code != 0 {
        return Err(format!(
            "Failed to validate path: {}",
            command_error_output(&result)
        ));
    }

    Ok(())
}

async fn run_command(
    options: &ToolExecutionOptions,
    tool_options: &OpenAgentToolsOptions,
    command: String,
) -> Result<SandboxCommandResult, ToolExecutionError> {
    run_command_with_timeout(options, tool_options, command, SEARCH_TIMEOUT_MS).await
}

async fn run_command_with_timeout(
    options: &ToolExecutionOptions,
    tool_options: &OpenAgentToolsOptions,
    command: String,
    _timeout_ms: u64,
) -> Result<SandboxCommandResult, ToolExecutionError> {
    run_command_with_cwd(options, tool_options, command, None, _timeout_ms).await
}

async fn run_command_with_cwd(
    options: &ToolExecutionOptions,
    tool_options: &OpenAgentToolsOptions,
    command: String,
    cwd: Option<&str>,
    _timeout_ms: u64,
) -> Result<SandboxCommandResult, ToolExecutionError> {
    let sandbox = options
        .experimental_sandbox
        .as_ref()
        .ok_or_else(|| ToolExecutionError::new("Sandbox not initialized in tool execution."))?;

    let mut command_options = SandboxCommandOptions::new(command);
    if let Some(working_directory) = joined_working_directory(tool_options, cwd) {
        command_options = command_options.with_working_directory(working_directory);
    }
    if let Some(abort_signal) = &options.abort_signal {
        command_options = command_options.with_abort_signal(abort_signal.clone());
    }

    Ok(sandbox.run_command(command_options).await)
}

fn joined_working_directory(
    tool_options: &OpenAgentToolsOptions,
    relative_cwd: Option<&str>,
) -> Option<String> {
    match (&tool_options.working_directory, relative_cwd) {
        (Some(base), Some(".")) | (Some(base), None) => Some(base.clone()),
        (Some(base), Some(cwd)) => Some(format!("{}/{}", base.trim_end_matches('/'), cwd)),
        (None, Some(".")) | (None, None) => None,
        (None, Some(cwd)) => Some(cwd.to_string()),
    }
}

fn parse_input<T>(input: JsonValue) -> Result<T, ToolExecutionError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(input)
        .map_err(|error| ToolExecutionError::new(format!("Invalid tool input: {error}")))
}

fn tool_error(message: impl Into<String>) -> JsonValue {
    json!({
        "success": false,
        "error": message.into()
    })
}

fn command_error_output(result: &SandboxCommandResult) -> String {
    let output = if result.stderr.trim().is_empty() {
        result.stdout.trim()
    } else {
        result.stderr.trim()
    };
    if output.is_empty() {
        format!("exit {}", result.exit_code)
    } else {
        output.to_string()
    }
}

fn parent_path(path: &str) -> &str {
    path.rsplit_once('/').map_or(
        ".",
        |(parent, _)| {
            if parent.is_empty() { "." } else { parent }
        },
    )
}

fn strip_current_dir(path: &str) -> String {
    path.strip_prefix("./").unwrap_or(path).replace('\\', "/")
}

fn shell_escape(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn string_field<'a>(value: &'a JsonValue, field: &str) -> Option<&'a str> {
    value.get(field).and_then(JsonValue::as_str)
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

fn regex_pattern(pattern: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        pattern.to_string()
    } else {
        format!("(?i:{pattern})")
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    let normalized_pattern = pattern.replace('\\', "/");
    let normalized_path = path.replace('\\', "/");
    let pattern_segments = normalized_pattern
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>();
    let path_segments = normalized_path
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>();

    if pattern_segments.is_empty() {
        return path_segments.is_empty();
    }
    if pattern_segments.len() == 1 {
        return path_segments
            .last()
            .is_some_and(|segment| glob_segment_matches(pattern_segments[0], segment));
    }

    glob_segments_match(&pattern_segments, &path_segments)
}

fn glob_segments_match(pattern: &[&str], path: &[&str]) -> bool {
    match (pattern.split_first(), path.split_first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some((&"**", rest)), None) => glob_segments_match(rest, path),
        (Some((&"**", rest)), Some((_, path_rest))) => {
            glob_segments_match(rest, path) || glob_segments_match(pattern, path_rest)
        }
        (Some((pattern_head, rest)), Some((path_head, path_rest))) => {
            glob_segment_matches(pattern_head, path_head) && glob_segments_match(rest, path_rest)
        }
        (Some(_), None) => false,
    }
}

fn glob_segment_matches(pattern: &str, segment: &str) -> bool {
    let mut regex = String::from("^");
    for character in pattern.chars() {
        match character {
            '*' => regex.push_str("[^/]*"),
            '?' => regex.push_str("[^/]"),
            other => regex.push_str(&regex::escape(&other.to_string())),
        }
    }
    regex.push('$');
    Regex::new(&regex).is_ok_and(|regex| regex.is_match(segment))
}

fn is_private_host(hostname: &str) -> bool {
    let normalized = hostname
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if normalized == "localhost" {
        return true;
    }

    match normalized.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => is_private_ipv4(address),
        Ok(IpAddr::V6(address)) => is_private_ipv6(address),
        Err(_) => false,
    }
}

fn is_private_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, _, _] = address.octets();
    first == 0
        || first == 10
        || first == 127
        || (first == 169 && second == 254)
        || (first == 172 && (16..=31).contains(&second))
        || (first == 192 && second == 168)
}

fn is_private_ipv6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    if let Some(mapped) = ipv4_mapped_ipv6(segments) {
        return is_private_ipv4(mapped);
    }

    address.is_loopback()
        || address.is_unspecified()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
}

fn ipv4_mapped_ipv6(segments: [u16; 8]) -> Option<Ipv4Addr> {
    if segments[..5].iter().all(|segment| *segment == 0) && segments[5] == 0xffff {
        Some(Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            (segments[6] & 0xff) as u8,
            (segments[7] >> 8) as u8,
            (segments[7] & 0xff) as u8,
        ))
    } else {
        None
    }
}

fn format_ask_user_question_model_output(output: &JsonValue) -> String {
    if output
        .get("declined")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false)
    {
        return "User declined to answer questions. Continue without this information or ask in a different way.".to_string();
    }

    let Some(answers) = output.get("answers").and_then(JsonValue::as_object) else {
        return "User did not respond to questions.".to_string();
    };
    let formatted = answers
        .iter()
        .map(|(question, answer)| {
            let answer = if let Some(values) = answer.as_array() {
                values
                    .iter()
                    .filter_map(JsonValue::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                answer.as_str().unwrap_or_default().to_string()
            };
            format!("\"{question}\"=\"{answer}\"")
        })
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "User has answered your questions: {formatted}. You can now continue with the user's answers in mind."
    )
}

fn unix_seconds_to_iso(seconds: i64) -> String {
    let Ok(datetime) = time::OffsetDateTime::from_unix_timestamp(seconds) else {
        return "1970-01-01T00:00:00Z".to_string();
    };
    datetime
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn dangerous_command_patterns() -> &'static [Regex] {
    static PATTERNS: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"\bcurl\b",
            r"\brm\s+(?:[^\n;&|]*\s)?(?:-[A-Za-z]*r[A-Za-z]*f|-[A-Za-z]*f[A-Za-z]*r|-r\s+-f|-f\s+-r|-{1,2}recursive\b.*-{1,2}force\b|-{1,2}force\b.*-{1,2}recursive\b)",
            r"\bfind\b[^\n;&|]*(?:-delete|-exec\s+rm\b)",
            r"\b(?:shred|mkfs|dd)\b",
            r":\(\)\s*\{\s*:\|:",
        ]
        .into_iter()
        .map(|pattern| Regex::new(pattern).expect("dangerous command regex compiles"))
        .collect()
    })
}

fn sensitive_command_patterns() -> &'static [Regex] {
    static PATTERNS: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"\.\s*env",
            r#"\.e(?:['"]{2}|\\|\$\{[^}]*\}|\$\([^)]*\))?nv"#,
            r"\.e\$\([^)]*nv[^)]*\)",
            r"\$\([^)]*env[^)]*\)",
            r"`[^`]*env[^`]*`",
            r"\b(?:aws/credentials|id_rsa|id_ed25519|\.ssh|proc/self/environ)\b",
        ]
        .into_iter()
        .map(|pattern| Regex::new(pattern).expect("sensitive command regex compiles"))
        .collect()
    })
}

fn object_schema(properties: JsonValue, required: &[&str]) -> JsonSchema {
    json!({
        "type": "object",
        "properties": properties,
        "required": required
    })
    .as_object()
    .expect("schema object")
    .clone()
}

fn read_schema() -> JsonSchema {
    object_schema(
        json!({
            "filePath": { "type": "string" },
            "offset": { "type": "number" },
            "limit": { "type": "number" }
        }),
        &["filePath"],
    )
}

fn write_schema() -> JsonSchema {
    object_schema(
        json!({
            "filePath": { "type": "string" },
            "content": { "type": "string" }
        }),
        &["filePath", "content"],
    )
}

fn edit_schema() -> JsonSchema {
    object_schema(
        json!({
            "filePath": { "type": "string" },
            "oldString": { "type": "string" },
            "newString": { "type": "string" },
            "replaceAll": { "type": "boolean" },
            "startLine": { "type": "number" }
        }),
        &["filePath", "oldString", "newString"],
    )
}

fn grep_schema() -> JsonSchema {
    object_schema(
        json!({
            "pattern": { "type": "string" },
            "path": { "type": "string" },
            "glob": { "type": "string" },
            "caseSensitive": { "type": "boolean" }
        }),
        &["pattern", "path"],
    )
}

fn glob_schema() -> JsonSchema {
    object_schema(
        json!({
            "pattern": { "type": "string" },
            "path": { "type": "string" },
            "limit": { "type": "number" }
        }),
        &["pattern"],
    )
}

fn bash_schema() -> JsonSchema {
    object_schema(
        json!({
            "command": { "type": "string" },
            "cwd": { "type": "string" },
            "detached": { "type": "boolean" }
        }),
        &["command"],
    )
}

fn todo_write_schema() -> JsonSchema {
    object_schema(
        json!({
            "todos": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "content": { "type": "string" },
                        "status": {
                            "type": "string",
                            "enum": ["pending", "in_progress", "completed"]
                        }
                    },
                    "required": ["id", "content", "status"]
                }
            }
        }),
        &["todos"],
    )
}

fn task_schema() -> JsonSchema {
    object_schema(
        json!({
            "description": { "type": "string" },
            "subagent": { "type": "string" }
        }),
        &["description"],
    )
}

fn skill_schema() -> JsonSchema {
    object_schema(
        json!({
            "skill": { "type": "string" },
            "input": { "type": "string" }
        }),
        &["skill", "input"],
    )
}

fn ask_user_question_schema() -> JsonSchema {
    object_schema(
        json!({
            "questions": {
                "type": "array",
                "minItems": 1,
                "maxItems": 4,
                "items": {
                    "type": "object",
                    "properties": {
                        "question": { "type": "string" },
                        "header": { "type": "string", "maxLength": 12 },
                        "multiSelect": { "type": "boolean" },
                        "options": {
                            "type": "array",
                            "minItems": 2,
                            "maxItems": 4,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "label": { "type": "string" },
                                    "description": { "type": "string" }
                                },
                                "required": ["label", "description"]
                            }
                        }
                    },
                    "required": ["question", "header", "options"]
                }
            }
        }),
        &["questions"],
    )
}

fn ask_user_question_output_schema() -> JsonSchema {
    json!({
        "anyOf": [
            {
                "type": "object",
                "properties": {
                    "answers": {
                        "type": "object",
                        "additionalProperties": {
                            "anyOf": [
                                { "type": "string" },
                                {
                                    "type": "array",
                                    "items": { "type": "string" }
                                }
                            ]
                        }
                    }
                },
                "required": ["answers"]
            },
            {
                "type": "object",
                "properties": {
                    "declined": { "const": true }
                },
                "required": ["declined"]
            }
        ]
    })
    .as_object()
    .expect("schema object")
    .clone()
}

fn web_fetch_schema() -> JsonSchema {
    object_schema(
        json!({
            "url": { "type": "string" },
            "method": {
                "type": "string",
                "enum": ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"]
            },
            "headers": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            },
            "body": { "type": "string" }
        }),
        &["url"],
    )
}

fn web_fetch_output_schema() -> JsonSchema {
    json!({
        "anyOf": [
            {
                "type": "object",
                "properties": {
                    "success": { "const": true },
                    "status": { "type": ["number", "null"] },
                    "body": { "type": "string" },
                    "truncated": { "type": "boolean" }
                },
                "required": ["success", "status", "body", "truncated"]
            },
            {
                "type": "object",
                "properties": {
                    "success": { "const": false },
                    "error": { "type": "string" }
                },
                "required": ["success", "error"]
            }
        ]
    })
    .as_object()
    .expect("schema object")
    .clone()
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::process::Command;
    use std::sync::Arc;
    use std::task::{Context, Poll, Waker};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::language_model::{LanguageModelMessage, LanguageModelTool};
    use crate::provider_utils::{ExperimentalSandbox, ToolNeedsApprovalOptions, execute_tool};

    use super::*;

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Future::poll(Pin::as_mut(&mut future), &mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("future unexpectedly pending"),
        }
    }

    #[derive(Debug)]
    struct LocalSandbox {
        root: PathBuf,
        description: String,
    }

    impl LocalSandbox {
        fn new(name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time after epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "ai-sdk-open-agent-tools-{name}-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).expect("creates temp sandbox root");
            Self {
                root,
                description: "local fake sandbox".to_string(),
            }
        }

        fn path(&self, path: &str) -> PathBuf {
            self.root.join(path)
        }

        fn write(&self, path: &str, content: &str) {
            let path = self.path(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("creates parent directory");
            }
            std::fs::write(path, content).expect("writes test file");
        }
    }

    impl Drop for LocalSandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    impl ExperimentalSandbox for LocalSandbox {
        fn description(&self) -> &str {
            &self.description
        }

        fn run_command(
            &self,
            options: SandboxCommandOptions,
        ) -> crate::provider_utils::SandboxRunCommandFuture {
            let root = self.root.clone();
            Box::pin(async move {
                let cwd = options
                    .working_directory
                    .map(PathBuf::from)
                    .map(|path| {
                        if path.is_absolute() {
                            path
                        } else {
                            root.join(path)
                        }
                    })
                    .unwrap_or(root);
                let output = Command::new("bash")
                    .arg("-lc")
                    .arg(options.command)
                    .current_dir(cwd)
                    .output()
                    .expect("runs fake sandbox command");
                SandboxCommandResult::new(output.status.code().unwrap_or(1))
                    .with_stdout(String::from_utf8_lossy(&output.stdout).into_owned())
                    .with_stderr(String::from_utf8_lossy(&output.stderr).into_owned())
            })
        }
    }

    fn sandbox() -> Arc<dyn ExperimentalSandbox> {
        Arc::new(LocalSandbox::new("case"))
    }

    fn local_sandbox() -> Arc<LocalSandbox> {
        Arc::new(LocalSandbox::new("case"))
    }

    fn execution_options(sandbox: Arc<dyn ExperimentalSandbox>) -> ToolExecutionOptions {
        ToolExecutionOptions::new("call-1", Vec::<LanguageModelMessage>::new())
            .with_experimental_sandbox(sandbox)
    }

    fn execute(tool: &Tool, input: JsonValue, sandbox: Arc<dyn ExperimentalSandbox>) -> JsonValue {
        let outputs = poll_ready(execute_tool(tool, input, execution_options(sandbox)))
            .expect("tool executes");
        outputs
            .into_iter()
            .last()
            .expect("tool returns output")
            .output()
            .clone()
    }

    fn tool_by_name<'a>(tools: &'a [Tool], name: &str) -> &'a Tool {
        tools
            .iter()
            .find(|tool| tool.name == name)
            .expect("tool exists")
    }

    #[test]
    fn open_agent_file_tools_read_write_and_edit_with_fake_sandbox() {
        let sandbox = local_sandbox();
        let tools = open_agent_tools();
        let write = tool_by_name(&tools, WRITE_TOOL_NAME);
        let read = tool_by_name(&tools, READ_TOOL_NAME);
        let edit = tool_by_name(&tools, EDIT_TOOL_NAME);

        let output = execute(
            write,
            json!({
                "filePath": "src/main.rs",
                "content": "one\ntwo\nthree"
            }),
            sandbox.clone(),
        );
        assert_eq!(output["success"], json!(true));
        assert_eq!(output["path"], json!("src/main.rs"));
        assert_eq!(output["bytesWritten"], json!(13));

        let output = execute(
            read,
            json!({
                "filePath": "src/main.rs",
                "offset": 2,
                "limit": 1
            }),
            sandbox.clone(),
        );
        assert_eq!(output["success"], json!(true));
        assert_eq!(output["content"], json!("2: two"));

        let output = execute(
            edit,
            json!({
                "filePath": "src/main.rs",
                "oldString": "two",
                "newString": "TWO"
            }),
            sandbox.clone(),
        );
        assert_eq!(output["success"], json!(true));
        assert_eq!(output["replacements"], json!(1));
        assert_eq!(
            std::fs::read_to_string(sandbox.path("src/main.rs")).expect("reads edited file"),
            "one\nTWO\nthree"
        );
    }

    #[test]
    fn open_agent_edit_tool_rejects_ambiguous_replacements() {
        let sandbox = local_sandbox();
        sandbox.write("lib.rs", "let value = 1;\nlet value = 2;\n");
        let tools = open_agent_tools();
        let edit = tool_by_name(&tools, EDIT_TOOL_NAME);

        let output = execute(
            edit,
            json!({
                "filePath": "lib.rs",
                "oldString": "let value",
                "newString": "let other"
            }),
            sandbox,
        );

        assert_eq!(output["success"], json!(false));
        assert!(output["error"].as_str().unwrap().contains("found 2 times"));
    }

    #[test]
    fn open_agent_search_bash_and_todo_tools_execute_with_fake_sandbox() {
        let sandbox = local_sandbox();
        sandbox.write("src/a.rs", "fn alpha() {}\nfn beta() {}\n");
        sandbox.write("src/b.txt", "alpha text\n");
        let tools = open_agent_tools();

        let grep_output = execute(
            tool_by_name(&tools, GREP_TOOL_NAME),
            json!({
                "pattern": "alpha",
                "path": "src",
                "glob": "*.rs"
            }),
            sandbox.clone(),
        );
        assert_eq!(grep_output["success"], json!(true));
        assert_eq!(grep_output["matchCount"], json!(1));
        assert_eq!(grep_output["matches"][0]["file"], json!("src/a.rs"));

        let glob_output = execute(
            tool_by_name(&tools, GLOB_TOOL_NAME),
            json!({
                "pattern": "**/*.rs",
                "path": "src"
            }),
            sandbox.clone(),
        );
        assert_eq!(glob_output["success"], json!(true));
        assert_eq!(glob_output["count"], json!(1));
        assert_eq!(glob_output["files"][0]["path"], json!("src/a.rs"));

        let bash_output = execute(
            tool_by_name(&tools, BASH_TOOL_NAME),
            json!({ "command": "printf 'ok'" }),
            sandbox.clone(),
        );
        assert_eq!(bash_output["success"], json!(true));
        assert_eq!(bash_output["stdout"], json!("ok"));

        let todo_output = execute(
            tool_by_name(&tools, TODO_WRITE_TOOL_NAME),
            json!({
                "todos": [
                    { "id": "1", "content": "read", "status": "completed" },
                    { "id": "2", "content": "write", "status": "in_progress" }
                ]
            }),
            sandbox,
        );
        assert_eq!(todo_output["success"], json!(true));
        assert_eq!(todo_output["todos"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn open_agent_path_security_blocks_escape_dotenv_and_symlink_escape() {
        let sandbox = local_sandbox();
        sandbox.write(".env", "TOKEN=secret\n");
        let outside = sandbox.root.with_extension("outside");
        std::fs::write(&outside, "secret").expect("writes outside file");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, sandbox.path("link")).expect("creates symlink");

        let tools = open_agent_tools();
        let read = tool_by_name(&tools, READ_TOOL_NAME);
        let output = execute(
            read,
            json!({ "filePath": "../secret.txt" }),
            sandbox.clone(),
        );
        assert_eq!(output["success"], json!(false));
        assert!(output["error"].as_str().unwrap().contains("workspace"));

        #[cfg(unix)]
        {
            let output = execute(read, json!({ "filePath": "link" }), sandbox.clone());
            assert_eq!(output["success"], json!(false));
            assert!(output["error"].as_str().unwrap().contains("outside"));
        }

        let needs_approval = poll_ready(
            read.resolve_needs_approval(
                json!({ "filePath": ".env" }),
                ToolNeedsApprovalOptions::new("call-1", Vec::<LanguageModelMessage>::new()),
            )
            .expect("approval function exists"),
        );
        assert!(needs_approval);

        let _ = std::fs::remove_file(outside);
    }

    #[test]
    fn open_agent_risky_commands_need_approval() {
        assert!(command_needs_approval("rm -rf target"));
        assert!(command_needs_approval("curl https://example.com"));
        assert!(command_needs_approval("cat .env"));
        assert!(command_needs_approval("cat ~/.ssh/id_ed25519"));
        assert!(!command_needs_approval("cargo test --all-features"));

        let tools = open_agent_tools();
        let bash = tool_by_name(&tools, BASH_TOOL_NAME);
        let needs_approval = poll_ready(
            bash.resolve_needs_approval(
                json!({ "command": "find . -delete" }),
                ToolNeedsApprovalOptions::new("call-1", Vec::<LanguageModelMessage>::new()),
            )
            .expect("approval function exists"),
        );
        assert!(needs_approval);
    }

    #[test]
    fn open_agent_web_fetch_blocks_private_hosts() {
        assert!(!is_allowed_web_url("file:///etc/passwd"));
        assert!(!is_allowed_web_url("http://localhost:3000"));
        assert!(!is_allowed_web_url("http://127.0.0.1"));
        assert!(!is_allowed_web_url("http://[::1]"));
        assert!(is_allowed_web_url("https://example.com"));

        let tools = open_agent_tools();
        let output = execute(
            tool_by_name(&tools, WEB_FETCH_TOOL_NAME),
            json!({ "url": "http://127.0.0.1:8080" }),
            sandbox(),
        );
        assert_eq!(output["success"], json!(false));
        assert!(output["error"].as_str().unwrap().contains("public host"));
    }

    #[test]
    fn open_agent_tool_schemas_serialize_for_tool_loop_agent() {
        let tools = open_agent_tools();
        let names = tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                TODO_WRITE_TOOL_NAME,
                READ_TOOL_NAME,
                WRITE_TOOL_NAME,
                EDIT_TOOL_NAME,
                GREP_TOOL_NAME,
                GLOB_TOOL_NAME,
                BASH_TOOL_NAME,
                TASK_TOOL_NAME,
                ASK_USER_QUESTION_TOOL_NAME,
                SKILL_TOOL_NAME,
                WEB_FETCH_TOOL_NAME,
            ]
        );

        let serialized = tools
            .iter()
            .map(|tool| {
                serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes")
            })
            .collect::<Vec<_>>();

        assert!(serialized.iter().any(|tool| {
            tool["type"] == json!("function")
                && tool["name"] == json!(READ_TOOL_NAME)
                && tool["inputSchema"]["properties"]["filePath"]["type"] == json!("string")
        }));
        assert!(matches!(
            tool_by_name(&tools, ASK_USER_QUESTION_TOOL_NAME).to_language_model_tool(),
            LanguageModelTool::Function(_)
        ));
        assert!(!tool_by_name(&tools, ASK_USER_QUESTION_TOOL_NAME).is_executable());
        assert!(!tool_by_name(&tools, TASK_TOOL_NAME).is_executable());
        assert!(!tool_by_name(&tools, SKILL_TOOL_NAME).is_executable());
    }

    #[test]
    fn open_agent_ask_user_question_formats_model_output() {
        let output = format_ask_user_question_model_output(&json!({
            "answers": {
                "Which mode?": "Fast",
                "Which checks?": ["fmt", "test"]
            }
        }));

        assert!(output.contains("\"Which mode?\"=\"Fast\""));
        assert!(output.contains("\"Which checks?\"=\"fmt, test\""));
        assert!(
            format_ask_user_question_model_output(&json!({ "declined": true }))
                .contains("declined")
        );
    }
}
