use std::collections::BTreeMap;

use just_bash::{Bash, BashOptions, JustBashExecOptions, resolve_path};
use serde::Deserialize;
use serde_json::Value;

const CONFORMANCE_CORPUS: &str = include_str!("fixtures/just-bash-conformance.json");
const DEFAULT_CWD: &str = "/workspace";
const SKIPPABLE_STATUSES: &[&str] = &["js-only-documented", "type-system-impossible"];

#[derive(Clone, Debug)]
struct CorpusCase {
    id: String,
    status: Option<String>,
    skip_reason: Option<String>,
    command: Option<String>,
    files: BTreeMap<String, String>,
    env: BTreeMap<String, String>,
    cwd: Option<String>,
    commands: Option<Vec<String>>,
    options: ExecOptionsFixture,
    expected: ExpectedFixture,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecOptionsFixture {
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    replace_env: bool,
    cwd: Option<String>,
    stdin: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Default)]
struct ExpectedFixture {
    stdout: Option<String>,
    stderr: Option<String>,
    exit_code: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorpusEnvelope {
    default_cwd: Option<String>,
    #[serde(default)]
    cases: Vec<RawCase>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCase {
    #[serde(default, alias = "caseId", alias = "upstreamId")]
    id: Option<String>,
    #[serde(default, alias = "portability", alias = "disposition")]
    status: Option<String>,
    #[serde(default, alias = "reason")]
    skip_reason: Option<String>,
    #[serde(default, alias = "script")]
    command: Option<String>,
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    cwd: Option<String>,
    #[serde(default)]
    commands: Option<Vec<String>>,
    #[serde(default, alias = "execOptions")]
    options: ExecOptionsFixture,
    #[serde(default)]
    expected: Option<RawExpected>,
    #[serde(default)]
    stdout: Option<String>,
    #[serde(default)]
    stderr: Option<String>,
    #[serde(default, alias = "exitCode")]
    exit_code: Option<i32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawExpected {
    #[serde(default)]
    stdout: Option<String>,
    #[serde(default)]
    stderr: Option<String>,
    #[serde(default, alias = "exitCode")]
    exit_code: Option<i32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CaseOutcome {
    Ran,
    Skipped,
}

#[test]
fn just_bash_runs_shared_conformance_corpus() {
    let cases = parse_corpus(CONFORMANCE_CORPUS).expect("corpus parses");
    let mut ran = 0;
    let mut failures = Vec::new();

    for case in &cases {
        match run_case(case) {
            Ok(CaseOutcome::Ran) => ran += 1,
            Ok(CaseOutcome::Skipped) => {}
            Err(error) => failures.push(error),
        }
    }

    assert!(
        !cases.is_empty(),
        "Just Bash conformance corpus must contain cases"
    );
    assert!(
        ran > 0,
        "Just Bash conformance corpus must contain at least one portable runnable case"
    );
    assert!(
        failures.is_empty(),
        "Just Bash conformance failures:\n{}",
        failures.join("\n\n")
    );
}

#[test]
fn conformance_failure_reports_upstream_case_id() {
    let mut case = CorpusCase {
        id: "packages/just-bash/src/comparison-tests/fixtures/echo.comparison.fixtures.json#d17ea61d90fa289b".to_string(),
        status: Some("portable-pending".to_string()),
        skip_reason: None,
        command: Some("echo hello".to_string()),
        files: BTreeMap::new(),
        env: BTreeMap::new(),
        cwd: Some(DEFAULT_CWD.to_string()),
        commands: None,
        options: ExecOptionsFixture::default(),
        expected: ExpectedFixture {
            stdout: Some("wrong\n".to_string()),
            stderr: Some(String::new()),
            exit_code: Some(0),
        },
    };

    let error = run_case(&case).expect_err("mismatch should fail");
    assert!(
        error.contains(&case.id),
        "failure should include upstream case id: {error}"
    );

    case.status = Some("js-only-documented".to_string());
    case.skip_reason = Some("explicitly nonportable".to_string());
    assert_eq!(run_case(&case), Ok(CaseOutcome::Skipped));
}

fn parse_corpus(input: &str) -> Result<Vec<CorpusCase>, String> {
    let value = serde_json::from_str::<Value>(input).map_err(|error| error.to_string())?;
    if let Value::Object(object) = &value
        && !object.contains_key("cases")
        && object
            .values()
            .all(|entry| entry.get("command").is_some() || entry.get("script").is_some())
    {
        return object
            .iter()
            .map(|(id, entry)| {
                let raw = serde_json::from_value::<RawCase>(entry.clone())
                    .map_err(|error| format!("{id}: {error}"))?;
                raw.into_case(id, DEFAULT_CWD)
            })
            .collect();
    }

    let envelope =
        serde_json::from_value::<CorpusEnvelope>(value).map_err(|error| error.to_string())?;
    let default_cwd = envelope.default_cwd.as_deref().unwrap_or(DEFAULT_CWD);
    envelope
        .cases
        .into_iter()
        .enumerate()
        .map(|(index, raw)| raw.into_case(&format!("case[{index}]"), default_cwd))
        .collect()
}

impl RawCase {
    fn into_case(self, fallback_id: &str, default_cwd: &str) -> Result<CorpusCase, String> {
        let id = self.id.unwrap_or_else(|| fallback_id.to_string());
        let expected = self.expected.unwrap_or_default();

        Ok(CorpusCase {
            id,
            status: self.status,
            skip_reason: self.skip_reason,
            command: self.command,
            files: self.files,
            env: self.env,
            cwd: self.cwd.or_else(|| Some(default_cwd.to_string())),
            commands: self.commands,
            options: self.options,
            expected: ExpectedFixture {
                stdout: self.stdout.or(expected.stdout),
                stderr: self.stderr.or(expected.stderr),
                exit_code: self.exit_code.or(expected.exit_code),
            },
        })
    }
}

fn run_case(case: &CorpusCase) -> Result<CaseOutcome, String> {
    if should_skip(case)? {
        return Ok(CaseOutcome::Skipped);
    }
    if case.skip_reason.is_some() {
        return Err(format!(
            "{}: portable case has skipReason but is not explicitly js-only-documented or type-system-impossible",
            case.id
        ));
    }

    let command = case
        .command
        .as_deref()
        .ok_or_else(|| format!("{}: portable case is missing command/script", case.id))?;
    let expected_stdout = case
        .expected
        .stdout
        .as_deref()
        .ok_or_else(|| format!("{}: portable case is missing expected stdout", case.id))?;
    let expected_stderr = case
        .expected
        .stderr
        .as_deref()
        .ok_or_else(|| format!("{}: portable case is missing expected stderr", case.id))?;
    let expected_exit_code = case
        .expected
        .exit_code
        .ok_or_else(|| format!("{}: portable case is missing expected exitCode", case.id))?;

    let bash = Bash::with_options(BashOptions {
        files: seed_files(case),
        env: case.env.clone(),
        cwd: case.cwd.clone(),
        commands: case.commands.clone(),
    });
    let result = bash.exec_with_options(command, exec_options(&case.options));

    let mut mismatches = Vec::new();
    if result.stdout != expected_stdout {
        mismatches.push(format!(
            "stdout expected {:?}, got {:?}",
            expected_stdout, result.stdout
        ));
    }
    if result.stderr != expected_stderr {
        mismatches.push(format!(
            "stderr expected {:?}, got {:?}",
            expected_stderr, result.stderr
        ));
    }
    if result.exit_code != expected_exit_code {
        mismatches.push(format!(
            "exitCode expected {}, got {}",
            expected_exit_code, result.exit_code
        ));
    }

    if mismatches.is_empty() {
        Ok(CaseOutcome::Ran)
    } else {
        Err(format!(
            "{} failed while running {:?}:\n{}",
            case.id,
            command,
            mismatches.join("\n")
        ))
    }
}

fn should_skip(case: &CorpusCase) -> Result<bool, String> {
    let Some(status) = case.status.as_deref() else {
        return Ok(false);
    };
    if !SKIPPABLE_STATUSES.contains(&status) {
        return Ok(false);
    }
    if case
        .skip_reason
        .as_ref()
        .is_none_or(|reason| reason.trim().is_empty())
    {
        return Err(format!(
            "{}: skipped cases must include skipReason",
            case.id
        ));
    }
    Ok(true)
}

fn seed_files(case: &CorpusCase) -> BTreeMap<String, String> {
    let cwd = case.cwd.as_deref().unwrap_or(DEFAULT_CWD);
    case.files
        .iter()
        .map(|(path, content)| {
            let resolved = if path.starts_with('/') {
                path.clone()
            } else {
                resolve_path(cwd, path)
            };
            (resolved, content.clone())
        })
        .collect()
}

fn exec_options(options: &ExecOptionsFixture) -> JustBashExecOptions {
    JustBashExecOptions::new()
        .with_envs(options.env.clone())
        .with_replace_env(options.replace_env)
        .with_args(options.args.clone())
        .apply_optional_cwd(options.cwd.clone())
        .apply_optional_stdin(options.stdin.clone())
        .apply_optional_timeout_ms(options.timeout_ms)
}

trait ApplyOptionalExecOptions {
    fn apply_optional_cwd(self, cwd: Option<String>) -> Self;
    fn apply_optional_stdin(self, stdin: Option<String>) -> Self;
    fn apply_optional_timeout_ms(self, timeout_ms: Option<u64>) -> Self;
}

impl ApplyOptionalExecOptions for JustBashExecOptions {
    fn apply_optional_cwd(self, cwd: Option<String>) -> Self {
        match cwd {
            Some(cwd) => self.with_cwd(cwd),
            None => self,
        }
    }

    fn apply_optional_stdin(self, stdin: Option<String>) -> Self {
        match stdin {
            Some(stdin) => self.with_stdin(stdin),
            None => self,
        }
    }

    fn apply_optional_timeout_ms(self, timeout_ms: Option<u64>) -> Self {
        match timeout_ms {
            Some(timeout_ms) => self.with_timeout_ms(timeout_ms),
            None => self,
        }
    }
}
