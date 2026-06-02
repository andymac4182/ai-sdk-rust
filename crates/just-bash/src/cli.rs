//! Focused CLI planning helpers for the Just Bash package boundary.
//!
//! This module models upstream `src/cli/just-bash.ts` argument parsing and
//! execution wiring without reading host files or creating a host OverlayFS.

use std::fmt;

use serde_json::json;

use crate::{JustBashExecResult, normalize_path};

/// Virtual mount point used by upstream `OverlayFs` for CLI execution.
pub const JUST_BASH_CLI_MOUNT_POINT: &str = "/home/user/project";

/// Version output printed by upstream `just-bash --version`.
pub const JUST_BASH_CLI_VERSION_OUTPUT: &str = "just-bash 1.0.0";

/// Help output printed by upstream `just-bash --help`.
pub const JUST_BASH_CLI_HELP_OUTPUT: &str = r#"just-bash - A secure bash environment for AI agents

Usage:
  just-bash [options] [script-file]
  just-bash -c 'script' [options]
  echo 'script' | just-bash [options]

Options:
  -c <script>       Execute the script from command line argument
  -e, --errexit     Exit immediately if a command exits with non-zero status
  --root <path>     Root directory for OverlayFS (default: current directory)
  --cwd <path>      Working directory within the sandbox (default: project mount point)
  --allow-write     Allow write operations (default: read-only)
  --python          Enable python3/python commands (disabled by default)
  --javascript      Enable js-exec command (disabled by default)
  --json            Output results as JSON (stdout, stderr, exitCode)
  -h, --help        Show this help message
  -v, --version     Show version

Security:
  - Reads from the real filesystem (read-only via OverlayFS)
  - Write operations are blocked by default (use --allow-write to enable)
  - Cannot escape the root directory
  - No network access

Filesystem:
  The root directory is mounted at /home/user/project in the virtual filesystem.
  The working directory starts at this mount point.

Examples:
  # List files in current directory
  just-bash -c 'ls -la'

  # Execute with specific root
  just-bash -c 'cat package.json' --root /path/to/project

  # Pipe script from stdin
  echo 'find . -name "*.ts" | head -5' | just-bash

  # Execute a script file
  just-bash ./scripts/build.sh

  # Get JSON output for programmatic use
  just-bash -c 'echo hello' --json

  # Allow write operations (writes stay in memory)
  just-bash -c 'echo test > /tmp/file.txt && cat /tmp/file.txt' --allow-write
"#;

/// Parsed CLI options matching upstream `CliOptions`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JustBashCliOptions {
    /// Inline script from `-c`.
    pub script: Option<String>,
    /// Script path from the first positional argument.
    pub script_file: Option<String>,
    /// Host root path to mount in upstream OverlayFS.
    pub root: String,
    /// Requested virtual cwd.
    pub cwd: String,
    /// True when `--cwd` was explicitly provided.
    pub cwd_overridden: bool,
    /// True when `-e` or `--errexit` was provided.
    pub errexit: bool,
    /// True when writes are enabled for the upstream OverlayFS.
    pub allow_write: bool,
    /// True when the optional Python runtime is enabled.
    pub python: bool,
    /// True when the optional JavaScript runtime is enabled.
    pub javascript: bool,
    /// True when CLI output should be JSON.
    pub json: bool,
    /// True when help should be printed.
    pub help: bool,
    /// True when version should be printed.
    pub version: bool,
}

/// Planned top-level CLI action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JustBashCliAction {
    /// Print help and exit.
    Help,
    /// Print version and exit.
    Version,
    /// Execute a script.
    Execute,
}

impl JustBashCliAction {
    /// Stable lower-case name for JS/NAPI harness assertions.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Help => "help",
            Self::Version => "version",
            Self::Execute => "execute",
        }
    }
}

/// Planned source for a script body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JustBashCliScriptSource {
    /// Inline script from `-c`.
    Inline,
    /// Script read from a virtual file.
    ScriptFile,
    /// Script read from stdin.
    Stdin,
    /// No script source because the action is help/version.
    None,
}

impl JustBashCliScriptSource {
    /// Stable lower-case name for JS/NAPI harness assertions.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::ScriptFile => "script-file",
            Self::Stdin => "stdin",
            Self::None => "none",
        }
    }
}

/// CLI plan after parsing and top-level action selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JustBashCliPlan {
    /// Parsed options.
    pub options: JustBashCliOptions,
    /// Top-level action.
    pub action: JustBashCliAction,
    /// Script source, if executing.
    pub script_source: JustBashCliScriptSource,
    /// Virtual cwd passed to `Bash`.
    pub effective_cwd: String,
    /// Text printed by help/version actions.
    pub output: Option<String>,
    /// Process-like exit code for help/version/no-script actions.
    pub exit_code: i32,
}

impl JustBashCliPlan {
    /// Returns the script body as upstream passes it to `Bash.exec`.
    pub fn script_for_execution(&self, script: &str) -> String {
        if self.options.errexit {
            format!("set -e\n{script}")
        } else {
            script.to_string()
        }
    }

    /// Resolves a relative script-file path at the upstream mount point.
    pub fn virtual_script_file_path(&self) -> Option<String> {
        let script_file = self.options.script_file.as_deref()?;
        if script_file.starts_with('/') {
            Some(normalize_path(script_file))
        } else {
            Some(normalize_path(&format!(
                "{JUST_BASH_CLI_MOUNT_POINT}/{script_file}"
            )))
        }
    }
}

/// CLI parser error mirroring upstream process-exit diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JustBashCliError {
    /// `-c` was provided without a following script.
    MissingScriptArgument,
    /// `--root` was provided without a following path.
    MissingRootPath,
    /// `--cwd` was provided without a following path.
    MissingCwdPath,
    /// Unknown option.
    UnknownOption(String),
}

impl fmt::Display for JustBashCliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingScriptArgument => {
                formatter.write_str("Error: -c requires a script argument")
            }
            Self::MissingRootPath => formatter.write_str("Error: --root requires a path argument"),
            Self::MissingCwdPath => formatter.write_str("Error: --cwd requires a path argument"),
            Self::UnknownOption(option) => write!(formatter, "Error: Unknown option: {option}"),
        }
    }
}

impl std::error::Error for JustBashCliError {}

/// Parses upstream-style `just-bash` arguments and selects the action.
pub fn plan_just_bash_cli_args<I, S>(
    args: I,
    process_cwd: &str,
    stdin_is_tty: bool,
) -> Result<JustBashCliPlan, JustBashCliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|entry| entry.as_ref().to_string())
        .collect::<Vec<_>>();
    let options = parse_just_bash_cli_args(&args, process_cwd)?;
    let effective_cwd = if options.cwd_overridden {
        normalize_path(&options.cwd)
    } else {
        JUST_BASH_CLI_MOUNT_POINT.to_string()
    };
    let (action, script_source, output, exit_code) = if options.help {
        (
            JustBashCliAction::Help,
            JustBashCliScriptSource::None,
            Some(JUST_BASH_CLI_HELP_OUTPUT.to_string()),
            0,
        )
    } else if options.version {
        (
            JustBashCliAction::Version,
            JustBashCliScriptSource::None,
            Some(JUST_BASH_CLI_VERSION_OUTPUT.to_string()),
            0,
        )
    } else if options.script.is_some() {
        (
            JustBashCliAction::Execute,
            JustBashCliScriptSource::Inline,
            None,
            0,
        )
    } else if options.script_file.is_some() {
        (
            JustBashCliAction::Execute,
            JustBashCliScriptSource::ScriptFile,
            None,
            0,
        )
    } else if !stdin_is_tty {
        (
            JustBashCliAction::Execute,
            JustBashCliScriptSource::Stdin,
            None,
            0,
        )
    } else {
        (
            JustBashCliAction::Help,
            JustBashCliScriptSource::None,
            Some(JUST_BASH_CLI_HELP_OUTPUT.to_string()),
            1,
        )
    };

    Ok(JustBashCliPlan {
        options,
        action,
        script_source,
        effective_cwd,
        output,
        exit_code,
    })
}

/// Formats a backend result with upstream `--json` output field names.
pub fn format_just_bash_cli_json_result(result: &JustBashExecResult) -> String {
    serde_json::to_string(&json!({
        "stdout": result.stdout,
        "stderr": result.stderr,
        "exitCode": result.exit_code,
    }))
    .expect("static CLI JSON fields are serializable")
}

fn parse_just_bash_cli_args(
    args: &[String],
    process_cwd: &str,
) -> Result<JustBashCliOptions, JustBashCliError> {
    let process_cwd = normalize_host_path(process_cwd);
    let mut options = JustBashCliOptions {
        script: None,
        script_file: None,
        root: process_cwd.clone(),
        cwd: "/".to_string(),
        cwd_overridden: false,
        errexit: false,
        allow_write: false,
        python: false,
        javascript: false,
        json: false,
        help: false,
        version: false,
    };

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "-h" | "--help" => {
                options.help = true;
                index += 1;
            }
            "-v" | "--version" => {
                options.version = true;
                index += 1;
            }
            "-c" => {
                let Some(script) = args.get(index + 1) else {
                    return Err(JustBashCliError::MissingScriptArgument);
                };
                options.script = Some(script.clone());
                index += 2;
            }
            "-e" | "--errexit" => {
                options.errexit = true;
                index += 1;
            }
            "--root" => {
                let Some(root) = args.get(index + 1) else {
                    return Err(JustBashCliError::MissingRootPath);
                };
                options.root = resolve_host_path(&process_cwd, root);
                index += 2;
            }
            "--cwd" => {
                let Some(cwd) = args.get(index + 1) else {
                    return Err(JustBashCliError::MissingCwdPath);
                };
                options.cwd = cwd.clone();
                options.cwd_overridden = true;
                index += 2;
            }
            "--json" => {
                options.json = true;
                index += 1;
            }
            "--allow-write" => {
                options.allow_write = true;
                index += 1;
            }
            "--python" => {
                options.python = true;
                index += 1;
            }
            "--javascript" => {
                options.javascript = true;
                index += 1;
            }
            _ if arg.starts_with('-') => {
                if arg.len() > 2 && !arg.starts_with("--") {
                    let mut consumed_script_arg = false;
                    for flag in arg[1..].chars() {
                        match flag {
                            'e' => options.errexit = true,
                            'h' => options.help = true,
                            'v' => options.version = true,
                            'c' => {
                                let Some(script) = args.get(index + 1) else {
                                    return Err(JustBashCliError::MissingScriptArgument);
                                };
                                options.script = Some(script.clone());
                                consumed_script_arg = true;
                                break;
                            }
                            _ => {
                                return Err(JustBashCliError::UnknownOption(format!("-{flag}")));
                            }
                        }
                    }
                    index += 1 + usize::from(consumed_script_arg);
                } else {
                    return Err(JustBashCliError::UnknownOption(arg.clone()));
                }
            }
            _ => {
                if options.script_file.is_none() && options.script.is_none() {
                    options.script_file = Some(arg.clone());
                } else if options.script_file.is_some() && options.root == process_cwd {
                    options.root = resolve_host_path(&process_cwd, arg);
                }
                index += 1;
            }
        }
    }

    Ok(options)
}

fn resolve_host_path(process_cwd: &str, path: &str) -> String {
    let path = path.replace('\\', "/");
    if path.starts_with('/') {
        normalize_host_path(&path)
    } else {
        normalize_host_path(&format!("{process_cwd}/{path}"))
    }
}

fn normalize_host_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let absolute = path.starts_with('/');
    let mut parts = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop();
        } else {
            parts.push(part);
        }
    }

    match (absolute, parts.is_empty()) {
        (true, true) => "/".to_string(),
        (true, false) => format!("/{}", parts.join("/")),
        (false, true) => ".".to_string(),
        (false, false) => parts.join("/"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::Value;

    use crate::{Bash, BashOptions, resolve_path};

    use super::*;

    fn plan(args: &[&str]) -> JustBashCliPlan {
        plan_just_bash_cli_args(args, "/repo/project", true).unwrap()
    }

    fn execute_plan(
        plan: &JustBashCliPlan,
        script: &str,
        files: &[(&str, &str)],
    ) -> JustBashExecResult {
        let bash = Bash::with_options(BashOptions {
            files: files
                .iter()
                .map(|(path, content)| ((*path).to_string(), (*content).to_string()))
                .collect::<BTreeMap<_, _>>(),
            cwd: Some(plan.effective_cwd.clone()),
            ..BashOptions::default()
        });
        bash.exec(plan.script_for_execution(script))
    }

    #[test]
    fn jbc18_cli_help_and_version_flags_match_upstream_output() {
        for flag in ["-h", "--help"] {
            let plan = plan(&[flag]);
            assert_eq!(plan.action, JustBashCliAction::Help);
            assert_eq!(plan.script_source, JustBashCliScriptSource::None);
            assert_eq!(plan.exit_code, 0);
            let output = plan.output.as_deref().unwrap_or_default();
            assert!(output.contains("just-bash - A secure bash environment"));
            assert!(output.contains("Usage:"));
            assert!(output.contains("--json"));
            assert!(output.contains("--allow-write"));
        }

        for flag in ["-v", "--version"] {
            let plan = plan(&[flag]);
            assert_eq!(plan.action, JustBashCliAction::Version);
            assert_eq!(plan.exit_code, 0);
            assert_eq!(plan.output.as_deref(), Some(JUST_BASH_CLI_VERSION_OUTPUT));
        }
    }

    #[test]
    fn jbc18_cli_argument_parser_routes_sources_root_and_cwd() {
        let inline = plan(&["-c", "echo hello", "--root", "workspace", "--json"]);
        assert_eq!(inline.action, JustBashCliAction::Execute);
        assert_eq!(inline.script_source, JustBashCliScriptSource::Inline);
        assert_eq!(inline.options.script.as_deref(), Some("echo hello"));
        assert_eq!(inline.options.root, "/repo/project/workspace");
        assert!(inline.options.json);
        assert_eq!(inline.effective_cwd, JUST_BASH_CLI_MOUNT_POINT);

        let stdin = plan_just_bash_cli_args(["--root", "/tmp/project"], "/repo", false).unwrap();
        assert_eq!(stdin.script_source, JustBashCliScriptSource::Stdin);
        assert_eq!(stdin.options.root, "/tmp/project");

        let script_file = plan(&["scripts/build.sh", "../root"]);
        assert_eq!(
            script_file.script_source,
            JustBashCliScriptSource::ScriptFile
        );
        assert_eq!(
            script_file.options.script_file.as_deref(),
            Some("scripts/build.sh")
        );
        assert_eq!(script_file.options.root, "/repo/root");
        assert_eq!(
            script_file.virtual_script_file_path().as_deref(),
            Some("/home/user/project/scripts/build.sh")
        );

        let cwd = plan(&["-c", "pwd", "--cwd", "/home/./user/../user/project"]);
        assert!(cwd.options.cwd_overridden);
        assert_eq!(cwd.effective_cwd, "/home/user/project");

        let clamped = plan(&["-c", "pwd", "--cwd", "/home/user/project/../../.."]);
        assert_eq!(clamped.effective_cwd, "/");
    }

    #[test]
    fn jbc18_cli_argument_parser_handles_combined_flags_and_errors() {
        let combined = plan(&["-ec", "false; echo no"]);
        assert!(combined.options.errexit);
        assert_eq!(combined.options.script.as_deref(), Some("false; echo no"));
        assert_eq!(
            combined.script_for_execution("false; echo no"),
            "set -e\nfalse; echo no"
        );

        assert_eq!(
            plan_just_bash_cli_args(["-c"], "/repo", true).unwrap_err(),
            JustBashCliError::MissingScriptArgument
        );
        assert_eq!(
            plan_just_bash_cli_args(["--root"], "/repo", true).unwrap_err(),
            JustBashCliError::MissingRootPath
        );
        assert_eq!(
            plan_just_bash_cli_args(["--cwd"], "/repo", true).unwrap_err(),
            JustBashCliError::MissingCwdPath
        );
        assert_eq!(
            plan_just_bash_cli_args(["--unknown-option"], "/repo", true).unwrap_err(),
            JustBashCliError::UnknownOption("--unknown-option".to_string())
        );
        assert_eq!(
            plan_just_bash_cli_args(["-ez"], "/repo", true).unwrap_err(),
            JustBashCliError::UnknownOption("-z".to_string())
        );
    }

    #[test]
    fn jbc18_cli_invocation_shape_executes_inline_stdin_script_file_and_json_rows() {
        let inline = plan(&["-c", "cat test.txt"]);
        let cat = execute_plan(
            &inline,
            inline.options.script.as_deref().unwrap(),
            &[("/home/user/project/test.txt", "hello world")],
        );
        assert_eq!(cat.stdout, "hello world");
        assert_eq!(cat.exit_code, 0);

        let echo = execute_plan(&plan(&["-c", "echo hello"]), "echo hello", &[]);
        assert_eq!(echo.stdout, "hello\n");

        let pipe = execute_plan(
            &plan(&["-c", "cat data.txt | grep banana"]),
            "cat data.txt | grep banana",
            &[("/home/user/project/data.txt", "apple\nbanana\ncherry")],
        );
        assert_eq!(pipe.stdout, "banana\n");

        let ls = execute_plan(
            &plan(&["-c", "ls"]),
            "ls",
            &[
                ("/home/user/project/file1.txt", "a"),
                ("/home/user/project/file2.txt", "b"),
            ],
        );
        assert!(ls.stdout.contains("file1.txt"));
        assert!(ls.stdout.contains("file2.txt"));

        let absolute_mount = execute_plan(
            &plan(&["-c", "cat /home/user/project/test.txt"]),
            "cat /home/user/project/test.txt",
            &[("/home/user/project/test.txt", "mounted")],
        );
        assert_eq!(absolute_mount.stdout, "mounted");

        let stdin_plan = plan_just_bash_cli_args(["--root", "/tmp/project"], "/repo", false)
            .expect("stdin plan parses");
        let stdin_result = execute_plan(
            &stdin_plan,
            "cat file.txt",
            &[("/home/user/project/file.txt", "stdin works")],
        );
        assert_eq!(stdin_result.stdout, "stdin works");

        let script_file_plan = plan(&["script.sh"]);
        let script_path = script_file_plan.virtual_script_file_path().unwrap();
        let script = BTreeMap::from([(script_path.clone(), "echo from-script".to_string())]);
        let bash = Bash::with_options(BashOptions {
            files: script,
            cwd: Some(script_file_plan.effective_cwd.clone()),
            ..BashOptions::default()
        });
        let script_body = bash.read_file(&script_path).unwrap();
        assert_eq!(bash.exec(script_body).stdout, "from-script\n");

        let json_ok = execute_plan(
            &plan(&["-c", "cat test.txt", "--json"]),
            "cat test.txt",
            &[("/home/user/project/test.txt", "content")],
        );
        let value: Value = serde_json::from_str(&format_just_bash_cli_json_result(&json_ok))
            .expect("CLI JSON parses");
        assert_eq!(value["stdout"], "content");
        assert_eq!(value["stderr"], "");
        assert_eq!(value["exitCode"], 0);

        let json_err = execute_plan(
            &plan(&["-c", "cat nonexistent.txt", "--json"]),
            "cat nonexistent.txt",
            &[],
        );
        let value: Value = serde_json::from_str(&format_just_bash_cli_json_result(&json_err))
            .expect("CLI error JSON parses");
        assert_ne!(value["exitCode"], 0);
        assert!(
            value["stderr"]
                .as_str()
                .unwrap_or_default()
                .contains("No such file")
        );
    }

    #[test]
    fn jbc18_cli_source_rows_keep_execution_wiring_host_agnostic() {
        let no_script = plan_just_bash_cli_args(Vec::<String>::new(), "/repo", true).unwrap();
        assert_eq!(no_script.action, JustBashCliAction::Help);
        assert_eq!(no_script.exit_code, 1);
        assert_eq!(
            resolve_path(&no_script.effective_cwd, "README.md"),
            "/home/user/project/README.md"
        );
    }
}
