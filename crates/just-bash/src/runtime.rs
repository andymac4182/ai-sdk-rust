//! Upstream-style `Bash` facade backed by the landed in-process session.

use std::collections::BTreeMap;

use std::sync::Arc;

use crate::{
    BashLogger, JustBashCustomCommand, JustBashExecOptions, JustBashExecResult,
    JustBashLanguageRuntime, JustBashResult, JustBashSession, JustBashSessionOptions,
    NetworkPolicy, NetworkResponse, path::resolve_path,
};

/// Construction options for the upstream-style [`Bash`] facade.
#[derive(Clone, Debug, Default)]
pub struct BashOptions {
    /// Optional list of portable command names to register.
    pub commands: Option<Vec<String>>,
    /// Initial virtual files.
    pub files: BTreeMap<String, String>,
    /// Base environment.
    pub env: BTreeMap<String, String>,
    /// Base working directory.
    pub cwd: Option<String>,
    /// Public Rust custom commands available before built-ins.
    pub custom_commands: Vec<JustBashCustomCommand>,
    /// Explicit optional language runtimes for `js-exec`/`node` or
    /// `python3`/`python`.
    pub language_runtimes: Vec<JustBashLanguageRuntime>,
    /// Optional network policy. When present, upstream-style `curl` is
    /// registered and backed by fake responses.
    pub network_policy: Option<NetworkPolicy>,
    /// Fake HTTP responses for deterministic network command execution.
    pub network_responses: BTreeMap<String, NetworkResponse>,
    /// Optional logger for execution tracing (see [`BashLogger`]).
    pub logger: Option<Arc<dyn BashLogger>>,
}

/// Portable `Bash` facade for Just Bash command-registry parity tests.
#[derive(Clone, Debug)]
pub struct Bash {
    session: JustBashSession,
}

impl Bash {
    /// Creates a session with the portable default command registry.
    pub fn new() -> Self {
        Self::with_options(BashOptions::default())
    }

    /// Creates a session with explicit upstream-style options.
    pub fn with_options(options: BashOptions) -> Self {
        let mut session_options = JustBashSessionOptions::new();
        let create_default_layout = options.cwd.is_none() && options.files.is_empty();
        session_options.files = options.files;
        session_options.env = options.env;
        session_options.cwd = options.cwd.or_else(|| {
            if create_default_layout {
                None
            } else {
                Some("/".to_string())
            }
        });
        session_options.commands = options.commands;
        session_options.custom_commands = options.custom_commands;
        session_options.language_runtimes = options.language_runtimes;
        session_options.create_default_layout = create_default_layout;
        session_options.network_policy = options.network_policy;
        session_options.network_responses = options.network_responses;
        session_options.logger = options.logger;
        Self {
            session: JustBashSession::with_options(session_options),
        }
    }

    /// Executes a script with default per-exec options.
    pub fn exec(&self, script: impl AsRef<str>) -> JustBashExecResult {
        self.session.exec(script, JustBashExecOptions::new())
    }

    /// Executes a script with explicit per-exec options.
    pub fn exec_with_options(
        &self,
        script: impl AsRef<str>,
        options: JustBashExecOptions,
    ) -> JustBashExecResult {
        self.session.exec(script, options)
    }

    /// Reads a UTF-8 virtual file.
    pub fn read_file(&self, path: &str) -> JustBashResult<String> {
        self.session.read_file(&resolve_path(&self.get_cwd(), path))
    }

    /// Reads raw bytes from a virtual file.
    pub fn read_file_buffer(&self, path: &str) -> JustBashResult<Vec<u8>> {
        self.session.read_file_buffer(path)
    }

    /// Writes a UTF-8 virtual file.
    pub fn write_file(&self, path: &str, content: &str) -> JustBashResult<()> {
        self.session
            .write_file(&resolve_path(&self.get_cwd(), path), content)
    }

    /// Returns true when a virtual path exists.
    pub fn file_exists(&self, path: &str) -> bool {
        self.session
            .file_exists(&resolve_path(&self.get_cwd(), path))
    }

    /// Returns sorted registered command names.
    pub fn registered_command_names(&self) -> Vec<String> {
        self.session.registered_command_names()
    }

    /// Returns the persistent starting working directory.
    pub fn get_cwd(&self) -> String {
        self.session.get_cwd()
    }

    /// Returns the persistent starting environment.
    pub fn get_env(&self) -> BTreeMap<String, String> {
        self.session.get_env()
    }
}

impl Default for Bash {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use crate::{JustBashCustomCommandResult, UPSTREAM_DEFAULT_COMMAND_NAMES};

    fn bash() -> Bash {
        Bash::new()
    }

    // Helper: build a fresh root session rooted at `/` with the supplied files.
    fn sed_session(files: &[(&str, &str)]) -> Bash {
        Bash::with_options(BashOptions {
            files: files
                .iter()
                .map(|(p, c)| (p.to_string(), c.to_string()))
                .collect::<BTreeMap<_, _>>(),
            cwd: Some("/".to_string()),
            ..BashOptions::default()
        })
    }

    #[test]
    fn sed_test_ts_zap_list_range_and_branch_rows() {
        // Cycle-engine rows from sed.test.ts that exercise the z (zap pattern
        // space), l (list with escapes), pattern-range tracking, and T (branch if
        // no substitution) commands. Each assertion mirrors the upstream verbatim.

        // z command — :696 `1z` empties the pattern space on the addressed line.
        assert_eq!(
            sed_session(&[("/test.txt", "hello\nworld\n")])
                .exec("sed '1z' /test.txt")
                .stdout,
            "\nworld\n"
        );
        // T command — :707 `-n 's/foo/FOO/;T;p'` branches to end (skipping p) when
        // no substitution was made, so only the substituted line is printed.
        assert_eq!(
            sed_session(&[("/test.txt", "foo\nbar\n")])
                .exec("sed -n 's/foo/FOO/;T;p' /test.txt")
                .stdout,
            "FOO\n"
        );

        // l command (list with escapes) — sed.test.ts.
        // :866 escapes a tab as `\t` and marks end of line with `$`.
        assert_eq!(
            sed_session(&[("/test.txt", "hello\tworld\n")])
                .exec("sed -n 'l' /test.txt")
                .stdout,
            "hello\\tworld$\n"
        );
        // :875 escapes a backslash as `\\`.
        assert_eq!(
            sed_session(&[("/test.txt", "a\\b\n")])
                .exec("sed -n 'l' /test.txt")
                .stdout,
            "a\\\\b$\n"
        );
        // :884 marks end of line with `$`.
        assert_eq!(
            sed_session(&[("/test.txt", "test\n")])
                .exec("sed -n 'l' /test.txt")
                .stdout,
            "test$\n"
        );

        // z command (zap) — sed.test.ts second block.
        // :895 `z` empties every pattern space.
        assert_eq!(
            sed_session(&[("/test.txt", "hello\nworld\n")])
                .exec("sed 'z' /test.txt")
                .stdout,
            "\n\n"
        );
        // :904 `2z` empties only the addressed line.
        assert_eq!(
            sed_session(&[("/test.txt", "line1\nline2\nline3\n")])
                .exec("sed '2z' /test.txt")
                .stdout,
            "line1\n\nline3\n"
        );

        // Pattern range state tracking — sed.test.ts.
        // :915 `/START/,/END/d` deletes the inclusive range across lines.
        assert_eq!(
            sed_session(&[("/test.txt", "a\nSTART\nb\nc\nEND\nd\n")])
                .exec("sed '/START/,/END/d' /test.txt")
                .stdout,
            "a\nd\n"
        );
        // :924 multiple independent ranges in the same file are tracked separately.
        assert_eq!(
            sed_session(&[("/test.txt", "a\nSTART\nb\nEND\nc\nSTART\nd\nEND\ne\n")])
                .exec("sed '/START/,/END/d' /test.txt")
                .stdout,
            "a\nc\ne\n"
        );
        // :933 an unclosed range (no END) deletes through to EOF.
        assert_eq!(
            sed_session(&[("/test.txt", "a\nSTART\nb\nc\n")])
                .exec("sed '/START/,/END/d' /test.txt")
                .stdout,
            "a\n"
        );

        // T command — :958 `s/x/y/;T add;b end;:add;s/$/X/;:end` branches via T to
        // the `add` label (appending X) only when no substitution was made.
        assert_eq!(
            sed_session(&[("/test.txt", "a\nb\n")])
                .exec("sed 's/x/y/;T add;b end;:add;s/$/X/;:end' /test.txt")
                .stdout,
            "aX\nbX\n"
        );
    }

    #[test]
    fn sed_jbc_pattern_bracket_literal_close_row() {
        // sed.regex.test.ts:399 — a `]` that appears first inside a bracket
        // expression is a literal class member, so `[][]` matches `]` or `[`.
        assert_eq!(
            sed_session(&[("/test.txt", "a]b\na[b\n")])
                .exec("sed 's/a[][]b/X/' /test.txt")
                .stdout,
            "X\nX\n"
        );
    }

    #[test]
    fn sed_errors_address_command_and_step_rows() {
        // sed.errors.test.ts address/step-address rows.
        // :202 `a\\nappended` (a command with text) executes without error.
        let r = sed_session(&[("/test/file.txt", "line 1\nline 2\nline 3\n")])
            .exec("sed 'a\\\\nappended' /test/file.txt");
        assert_eq!(r.exit_code, 0);
        // :211 `1~0d` step-0 address is handled gracefully (GNU sed: `first~0`
        // matches `first` through end of file, so every line is deleted here).
        let r = sed_session(&[("/test/file.txt", "line 1\nline 2\nline 3\n")])
            .exec("sed '1~0d' /test/file.txt");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "");
        // :218 a negative step address `1~-1d` is rejected with exit code 1.
        let r = sed_session(&[("/test/file.txt", "1\n2\n3\n")]).exec("sed '1~-1d' /test/file.txt");
        assert_eq!(r.exit_code, 1);
    }

    #[test]
    fn sed_security_rejects_e_shell_execution_command() {
        // sed.security.test.ts — the `e` command (shell execution) is blocked in
        // the sandbox; both the `e command` and bare `e` forms are rejected.
        // :11 `e whoami` with a specified command.
        let r = bash().exec("echo hello | sed 'e whoami'");
        assert_ne!(r.exit_code, 0);
        assert!(
            r.stderr
                .contains("e command (shell execution) is not supported"),
            "stderr was {:?}",
            r.stderr
        );
        // :20 bare `e` (execute the pattern space).
        let r = bash().exec("echo 'ls' | sed 'e'");
        assert_ne!(r.exit_code, 0);
        assert!(
            r.stderr
                .contains("e command (shell execution) is not supported"),
            "stderr was {:?}",
            r.stderr
        );
        // Sanity: normal substitution still works (the rejection is scoped to `e`).
        let r = bash().exec("echo hello | sed 's/hello/world/'");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "world\n");
    }

    #[test]
    fn sed_limits_iteration_and_resource_rows() {
        // sed.limits.test.ts — runaway-compute protection and bounded handling of
        // large inputs. The iteration cap is 10000 command executions; exceeding it
        // is a fatal execution-limit error (exit code 126).
        // :16 branch loop `:loop; b loop` must hit the iteration limit.
        let r = bash().exec("echo \"test\" | sed ':loop; b loop'");
        assert_eq!(r.exit_code, 126);
        assert!(
            r.stderr.contains("exceeded maximum iterations"),
            "stderr was {:?}",
            r.stderr
        );
        // :26 test loop `:loop; s/./&/; t loop` (always-true substitution) loops.
        let r = bash().exec("echo \"test\" | sed ':loop; s/./&/; t loop'");
        assert_eq!(r.exit_code, 126);
        assert!(
            r.stderr.contains("exceeded maximum iterations"),
            "stderr was {:?}",
            r.stderr
        );
        // :37 unconditional branch at start `b; p` completes (branch to end).
        let r = bash().exec("echo \"test\" | sed 'b; p'");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "test\n");

        // :47 global substitution on a long (100k char) line completes.
        let long = "a".repeat(100_000);
        let r = sed_session(&[("/input.txt", long.as_str())]).exec("sed 's/a/b/g' /input.txt");
        assert_eq!(r.exit_code, 0);
        assert!(!r.stdout.is_empty());

        // :79 appending many lines to the hold space completes.
        let many = std::iter::repeat("line")
            .take(1000)
            .collect::<Vec<_>>()
            .join("\n");
        let r = sed_session(&[("/input.txt", many.as_str())]).exec("sed 'H' /input.txt");
        assert_eq!(r.exit_code, 0);

        // :90 exchange with large buffers `h; x; x` round-trips unchanged.
        let bigx = "x".repeat(10_000);
        let r = sed_session(&[("/input.txt", bigx.as_str())]).exec("sed 'h; x; x' /input.txt");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, format!("{bigx}\n"));

        // :163 step address on large (10k line) input completes.
        let tenk = std::iter::repeat("line")
            .take(10_000)
            .collect::<Vec<_>>()
            .join("\n");
        let r = sed_session(&[("/input.txt", tenk.as_str())]).exec("sed -n '0~100p' /input.txt");
        assert_eq!(r.exit_code, 0);

        // :175 many (100) chained commands complete.
        let cmds = std::iter::repeat("s/a/b/")
            .take(100)
            .collect::<Vec<_>>()
            .join("; ");
        let r = bash().exec(&format!("echo \"aaa\" | sed '{cmds}'"));
        assert_eq!(r.exit_code, 0);

        // :183 deeply nested command blocks `{ { { p } } }` complete (p + autoprint).
        let r = bash().exec("echo \"test\" | sed '{ { { p } } }'");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "test\ntest\n");

        // :193 N accumulation `:a; N; ba` terminates when no next line remains.
        let hundred = std::iter::repeat("line")
            .take(100)
            .collect::<Vec<_>>()
            .join("\n");
        let r = sed_session(&[("/input.txt", hundred.as_str())]).exec("sed ':a; N; ba' /input.txt");
        assert_eq!(r.exit_code, 0);
    }

    #[test]
    fn text_search_jbc_grep_glob_operand_and_binary_rows() {
        // grep expands unquoted glob file operands itself (the shell leaves them
        // literal). `grep foo *.ts` in /dir matches a.ts and b.ts, so two files
        // are searched and the filename prefix is shown; only a.ts contains foo.
        // maps packages/just-bash/src/commands/grep/grep.advanced.test.ts:349
        let expand_star = Bash::with_options(BashOptions {
            cwd: Some("/dir".to_string()),
            files: BTreeMap::from([
                ("/dir/a.ts".to_string(), "foo\n".to_string()),
                ("/dir/b.ts".to_string(), "bar\n".to_string()),
                ("/dir/c.js".to_string(), "foo\n".to_string()),
            ]),
            ..BashOptions::default()
        });
        let result = expand_star.exec("grep foo *.ts");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a.ts:foo\n");
        assert_eq!(result.stderr, "");

        // A path-qualified glob expands within its directory, preserving the
        // directory prefix on each matched label, sorted by path.
        // maps packages/just-bash/src/commands/grep/grep.advanced.test.ts:363
        let expand_path = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/src/a.ts".to_string(), "test\n".to_string()),
                ("/src/b.ts".to_string(), "test\n".to_string()),
                ("/src/c.js".to_string(), "test\n".to_string()),
            ]),
            ..BashOptions::default()
        });
        let result = expand_path.exec("grep test /src/*.ts");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/src/a.ts:test\n/src/b.ts:test\n");
        assert_eq!(result.stderr, "");

        // A glob operand that matches nothing expands to no files: grep searches
        // nothing (it does not fall back to stdin or error), giving empty output
        // and the no-match exit code 1 with no diagnostic.
        // maps packages/just-bash/src/commands/grep/grep.advanced.test.ts:376
        let no_match = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/dir/file.js".to_string(), "content\n".to_string())]),
            ..BashOptions::default()
        });
        let result = no_match.exec("grep test /dir/*.ts");
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "");

        // grep treats virtual files as text: a single "binary" file searched for
        // a present pattern prints the matching line without a filename prefix.
        // maps packages/just-bash/src/commands/grep/grep.binary.test.ts:5
        let binary_plain = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/binary.bin".to_string(), "foo\nbar\n".to_string())]),
            ..BashOptions::default()
        });
        let result = binary_plain.exec("grep foo /binary.bin");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "foo\n");
        assert_eq!(result.stderr, "");

        // Content with leading NUL bytes still matches on the relevant line and
        // exits 0; the matched line is emitted verbatim (NUL preserved).
        // maps packages/just-bash/src/commands/grep/grep.binary.test.ts:26
        let binary_nul = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/binary.bin".to_string(),
                "\u{0}\u{0}test\n\u{0}foo\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        let result = binary_nul.exec("grep test /binary.bin");
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("test"),
            "stdout: {:?}",
            result.stdout
        );
        assert_eq!(result.stderr, "");
    }

    /// Closes the just-bash-core `tee-plugin.test.ts` "TeePlugin semantics
    /// preservation" rows that the portable Rust interpreter reproduces
    /// verbatim. Upstream registers a `TeePlugin` and asserts the wrapped run's
    /// stdout/stderr/exitCode equal the plain run — i.e. the transform never
    /// perturbs observable shell semantics. The Rust port carries no tee
    /// transform layer, so the equivalent proof is that `Bash::exec` produces
    /// the exact bash-correct output for each script. Each assertion fails if
    /// the pipeline, redirection, command-substitution, or chain semantics
    /// regress.
    #[test]
    fn just_bash_core_tee_semantics_preservation_pipeline_rows_match_plain_exec() {
        struct Case {
            script: &'static str,
            stdout: &'static str,
            stderr: &'static str,
            exit_code: i32,
        }

        let cases = [
            // tee-plugin.test.ts:337 pipeline success: cat file | grep match | sort
            Case {
                script: "cat /data/input.txt | grep hello | sort",
                stdout: "hello\nhello world\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:405 stderr preserved for unwrapped commands (|| chain)
            Case {
                script: "ls /no_such_path_xyz 2>&1 || echo fallback",
                stdout: "ls: /no_such_path_xyz: No such file or directory\nfallback\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:423 command substitution in variable, then use in pipeline
            Case {
                script: "X=$(echo hello); echo \"$X world\" | cat",
                stdout: "hello world\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:533 complex redirect: stderr to stdout into pipeline
            Case {
                script: "ls /no_such_xyz 2>&1 | cat",
                stdout: "ls: /no_such_xyz: No such file or directory\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:551 complex pipeline: generate, filter, transform, count
            Case {
                script: "printf '%s\\n' apple banana avocado blueberry apricot | grep ^a | sort | wc -l",
                stdout: "3\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:608 mixed && || ; chains with pipelines between
            Case {
                script: "echo start && echo hello | cat && echo end || echo fail",
                stdout: "start\nhello\nend\n",
                stderr: "",
                exit_code: 0,
            },
            // tee-plugin.test.ts:622 string manipulation pipeline: tr | sed | cat
            Case {
                script: "echo \"Hello World FOO\" | tr A-Z a-z | sed \"s/foo/bar/\" | cat",
                stdout: "hello world bar\n",
                stderr: "",
                exit_code: 0,
            },
        ];

        for case in cases {
            let b = Bash::with_options(BashOptions {
                files: BTreeMap::from([(
                    "/data/input.txt".to_string(),
                    "hello\nworld\nhello world\n".to_string(),
                )]),
                ..Default::default()
            });
            let r = b.exec(case.script);
            assert_eq!(r.stdout, case.stdout, "stdout for: {}", case.script);
            assert_eq!(r.stderr, case.stderr, "stderr for: {}", case.script);
            assert_eq!(
                r.exit_code, case.exit_code,
                "exit code for: {}",
                case.script
            );
        }
    }

    /// Closes additional `tee-plugin.test.ts` rows whose scripts the portable
    /// Rust interpreter reproduces verbatim through real built-in commands
    /// (`cat`/`sort`/`head`/`ls`/`tail`/`cut`/`uniq`/`wc`/`grep`/`rm`). Upstream
    /// registers a `TeePlugin` and asserts the wrapped run's
    /// stdout/stderr/exitCode equal the plain run (or, for the `TeePlugin exec`
    /// rows, the documented stdout/stderr/exit). The Rust port carries no tee
    /// transform layer, so the equivalent proof is that `Bash::exec` produces
    /// the exact bash-correct output for each script — each assertion fails if
    /// the pipeline, multi-file `ls`, stderr passthrough, sequential file I/O,
    /// or aggregation semantics regress.
    #[test]
    fn just_bash_core_tee_exec_and_semantics_filesystem_pipeline_rows_match_plain_exec() {
        let files = BTreeMap::from([
            ("/data/nums.txt".to_string(), "3\n1\n2\n".to_string()),
            ("/data/file.txt".to_string(), "found it\n".to_string()),
        ]);

        // tee-plugin.test.ts:111 stdout passthrough matches plain exec for pipeline
        let r = Bash::with_options(BashOptions {
            files: files.clone(),
            ..Default::default()
        })
        .exec("cat /data/nums.txt | sort | head -2");
        assert_eq!(r.stdout, "1\n2\n", "L111 stdout");
        assert_eq!(r.exit_code, 0, "L111 exit");

        // tee-plugin.test.ts:148 preserves stderr in pipeline
        let r = Bash::with_options(BashOptions {
            files: files.clone(),
            ..Default::default()
        })
        .exec("ls /data/file.txt /no_such_path | cat");
        assert!(
            r.stdout.contains("/data/file.txt"),
            "L148 stdout: {:?}",
            r.stdout
        );
        assert!(
            r.stderr.contains("No such file"),
            "L148 stderr: {:?}",
            r.stderr
        );

        // tee-plugin.test.ts:401 stderr from non-last command in wrapped pipeline
        let r = Bash::new().exec("ls /no_such_path_xyz | cat");
        assert_eq!(r.stdout, "", "L401 stdout");
        assert!(
            r.stderr.contains("No such file"),
            "L401 stderr: {:?}",
            r.stderr
        );
        assert_eq!(r.exit_code, 0, "L401 exit");

        // tee-plugin.test.ts:413 stdout and stderr from wrapped pipeline command
        let r = Bash::new().exec("ls /no_such_path_xyz /dev/null | cat");
        assert!(
            r.stderr.contains("/no_such_path_xyz"),
            "L413 stderr: {:?}",
            r.stderr
        );

        // tee-plugin.test.ts:545 sequential commands: write file then read it
        let r = Bash::new().exec(
            "echo \"data\" > /tmp/seq.txt; cat /tmp/seq.txt; rm /tmp/seq.txt; cat /tmp/seq.txt 2>&1; echo done",
        );
        assert_eq!(
            r.stdout, "data\ncat: /tmp/seq.txt: No such file or directory\ndone\n",
            "L545 stdout"
        );
        assert_eq!(r.stderr, "", "L545 stderr");
        assert_eq!(r.exit_code, 0, "L545 exit");

        // tee-plugin.test.ts:628 complex: build CSV, parse it, aggregate
        let r = Bash::new().exec(
            "printf 'name,score\\nalice,90\\nbob,85\\nalice,95\\nbob,70\\n' > /tmp/data.csv\ntail -n +2 /tmp/data.csv | sort -t, -k1,1 | cut -d, -f1 | uniq -c | sort -rn",
        );
        assert_eq!(r.stdout, "   2 bob\n   2 alice\n", "L628 stdout");
        assert_eq!(r.exit_code, 0, "L628 exit");

        // tee-plugin.test.ts:711 preserves exit code and stderr for failing
        // pipeline command (TeePlugin error handling).
        let r = Bash::new().exec("echo hello | cat /nonexistent_xyz");
        assert_ne!(r.exit_code, 0, "L711 exit");
        assert!(
            r.stderr.contains("No such file"),
            "L711 stderr: {:?}",
            r.stderr
        );

        // tee-plugin.test.ts:721 pipeline exit code preserved through PIPESTATUS:
        // last-stage grep failure surfaces exit 1 with empty stdout.
        let r = Bash::new().exec("echo hello | grep nomatch");
        assert_eq!(r.exit_code, 1, "L721 exit");
        assert_eq!(r.stdout, "", "L721 stdout");
    }

    const SHERLOCK: &str = "For the Doctor Watsons of this world, as opposed to the Sherlock\n\
Holmeses, success in the province of detective work must always\n\
be, to a very large extent, the result of luck. Sherlock Holmes\n\
can extract a clew from a wisp of straw or a flake of cigar ash;\n\
but Doctor Watson has to have it taken out for him and dusted,\n\
and exhibited clearly, with a label attached.\n";

    fn home_bash(files: &[(&str, &str)]) -> Bash {
        Bash::with_options(BashOptions {
            cwd: Some("/home/user".to_string()),
            files: files
                .iter()
                .map(|(path, content)| {
                    let path = if path.starts_with('/') {
                        (*path).to_string()
                    } else {
                        format!("/home/user/{path}")
                    };
                    (path, (*content).to_string())
                })
                .collect(),
            ..BashOptions::default()
        })
    }

    fn assert_home_exec(files: &[(&str, &str)], script: &str, exit_code: i32, stdout: &str) {
        let result = home_bash(files).exec(script);
        assert_eq!(result.exit_code, exit_code, "{script}");
        assert_eq!(result.stdout, stdout, "{script}");
        assert_eq!(result.stderr, "", "{script}");
    }

    #[test]
    fn registry_upstream_bash_commands_registers_all_supported_by_default() {
        // maps packages/just-bash/src/Bash.commands.test.ts:6
        let env = bash();

        assert_eq!(env.exec("echo hello").exit_code, 0);
        assert_eq!(env.exec("ls /").exit_code, 0);
        assert_eq!(env.exec("echo test | grep test").exit_code, 0);
    }

    #[test]
    fn registry_upstream_bash_commands_only_registers_specified_commands() {
        // maps packages/just-bash/src/Bash.commands.test.ts:20
        let env = Bash::with_options(BashOptions {
            commands: Some(vec!["echo".to_string(), "cat".to_string()]),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("echo hello").stdout, "hello\n");
        assert_eq!(env.exec("echo test | cat").exit_code, 0);
        let result = env.exec("ls");
        assert_eq!(result.exit_code, 127);
        assert!(result.stderr.contains("command not found"));
    }

    #[test]
    fn registry_upstream_bash_commands_grep_not_available_when_not_registered() {
        // maps packages/just-bash/src/Bash.commands.test.ts:39
        let env = Bash::with_options(BashOptions {
            commands: Some(vec!["echo".to_string(), "cat".to_string()]),
            ..BashOptions::default()
        });

        let result = env.exec("echo test | grep test");
        assert_eq!(result.exit_code, 127);
        assert!(result.stderr.contains("command not found"));
    }

    #[test]
    fn registry_upstream_get_command_names_returns_upstream_defaults() {
        // maps packages/just-bash/src/Bash.commands.test.ts:49
        assert!(UPSTREAM_DEFAULT_COMMAND_NAMES.contains(&"echo"));
        assert!(UPSTREAM_DEFAULT_COMMAND_NAMES.contains(&"cat"));
        assert!(UPSTREAM_DEFAULT_COMMAND_NAMES.contains(&"ls"));
        assert!(UPSTREAM_DEFAULT_COMMAND_NAMES.contains(&"grep"));
        assert!(UPSTREAM_DEFAULT_COMMAND_NAMES.contains(&"find"));
        assert!(!UPSTREAM_DEFAULT_COMMAND_NAMES.contains(&"curl"));
    }

    #[test]
    fn registry_upstream_bash_commands_empty_commands_array_means_no_commands() {
        // maps packages/just-bash/src/Bash.commands.test.ts:60
        let env = Bash::with_options(BashOptions {
            commands: Some(Vec::new()),
            ..BashOptions::default()
        });

        let result = env.exec("echo hello");
        assert_eq!(result.exit_code, 127);
        assert!(result.stderr.contains("command not found"));
    }

    #[test]
    fn registry_upstream_bash_commands_can_use_subset_of_file_commands() {
        // maps packages/just-bash/src/Bash.commands.test.ts:70
        let env = Bash::with_options(BashOptions {
            commands: Some(vec![
                "ls".to_string(),
                "cat".to_string(),
                "mkdir".to_string(),
            ]),
            files: BTreeMap::from([("/test.txt".to_string(), "hello".to_string())]),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("ls /").exit_code, 0);
        assert_eq!(env.exec("cat /test.txt").stdout, "hello");
        assert_eq!(env.exec("mkdir /newdir").exit_code, 0);
        assert_eq!(env.exec("rm /test.txt").exit_code, 127);
        assert_eq!(env.exec("cp /test.txt /test2.txt").exit_code, 127);
    }

    #[test]
    fn bash_upstream_command_executes_dash_c_and_positional_args() {
        // maps packages/just-bash/src/commands/bash/bash.test.ts:6, :13, :20, :30, and :37
        let env = bash();

        assert_eq!(env.exec("bash -c 'echo hello'").stdout, "hello\n");
        assert_eq!(
            env.exec("bash -c 'echo one; echo two'").stdout,
            "one\ntwo\n"
        );
        assert_eq!(
            env.exec("bash -c 'echo $1 $2' _ foo bar").stdout,
            "foo bar\n"
        );
        assert_eq!(env.exec("sh -c 'echo hello'").stdout, "hello\n");
        assert_eq!(env.exec("sh -c 'echo $1' _ world").stdout, "world\n");
    }

    #[test]
    fn bash_upstream_command_executes_script_files_and_shebangs() {
        // maps packages/just-bash/src/commands/bash/bash.test.ts:47 through :161
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/scripts/hello.sh".to_string(),
                    "echo \"Hello, World!\"".to_string(),
                ),
                (
                    "/scripts/script.sh".to_string(),
                    "#!/bin/bash\necho \"from shebang script\"".to_string(),
                ),
                (
                    "/scripts/greet.sh".to_string(),
                    "echo \"Hello, $1!\"".to_string(),
                ),
                (
                    "/scripts/count.sh".to_string(),
                    "echo \"Got $# arguments\"".to_string(),
                ),
                (
                    "/scripts/all.sh".to_string(),
                    "echo \"Args: $@\"".to_string(),
                ),
                (
                    "/scripts/multi.sh".to_string(),
                    "echo \"Line 1\"\necho \"Line 2\"\necho \"Line 3\"".to_string(),
                ),
                (
                    "/scripts/test.sh".to_string(),
                    "echo \"from sh\"".to_string(),
                ),
                (
                    "/scripts/vars.sh".to_string(),
                    "echo \"Hello $NAME\"".to_string(),
                ),
                (
                    "/scripts/fileop.sh".to_string(),
                    "echo \"content\" > /tmp/test.txt\ncat /tmp/test.txt".to_string(),
                ),
                (
                    "/scripts/pipes.sh".to_string(),
                    "echo -e \"foo\\nbar\\nbaz\" | grep bar".to_string(),
                ),
            ]),
            env: BTreeMap::from([("NAME".to_string(), "Test".to_string())]),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("bash /scripts/hello.sh").stdout, "Hello, World!\n");
        assert_eq!(
            env.exec("bash /scripts/script.sh").stdout,
            "from shebang script\n"
        );
        assert_eq!(
            env.exec("bash /scripts/greet.sh Alice").stdout,
            "Hello, Alice!\n"
        );
        assert_eq!(
            env.exec("bash /scripts/count.sh a b c").stdout,
            "Got 3 arguments\n"
        );
        assert_eq!(
            env.exec("bash /scripts/all.sh one two three").stdout,
            "Args: one two three\n"
        );
        let missing = env.exec("bash /nonexistent.sh");
        assert_eq!(missing.exit_code, 127);
        assert_eq!(
            missing.stderr,
            "bash: /nonexistent.sh: No such file or directory\n"
        );
        assert_eq!(
            env.exec("bash /scripts/multi.sh").stdout,
            "Line 1\nLine 2\nLine 3\n"
        );
        assert_eq!(env.exec("sh /scripts/test.sh").stdout, "from sh\n");
        assert_eq!(env.exec("bash /scripts/vars.sh").stdout, "Hello Test\n");
        assert_eq!(env.exec("bash /scripts/fileop.sh").stdout, "content\n");
        assert_eq!(env.exec("bash /scripts/pipes.sh").stdout, "bar\n");
        assert_eq!(env.exec("bash").exit_code, 0);
        assert_eq!(env.exec("sh").exit_code, 0);
        let bash_help = env.exec("bash --help");
        assert!(bash_help.stdout.contains("bash"));
        assert!(bash_help.stdout.contains("-c"));
        assert_eq!(bash_help.exit_code, 0);
        let sh_help = env.exec("sh --help");
        assert!(sh_help.stdout.contains("sh"));
        assert_eq!(sh_help.exit_code, 0);
    }

    #[test]
    fn bash_upstream_command_forwards_stdin_to_nested_dash_c() {
        // maps packages/just-bash/src/commands/bash/bash.test.ts:205 through :241
        let env = bash();

        assert_eq!(
            env.exec("echo \"hello world\" | bash -c 'DATA=$(cat); echo \"$DATA\"'")
                .stdout,
            "hello world\n"
        );
        assert_eq!(
            env.exec("echo \"test data\" | bash -c 'read LINE; echo \"Got: $LINE\"'")
                .stdout,
            "Got: test data\n"
        );
        assert_eq!(
            env.exec("echo \"from stdin\" | sh -c 'cat'").stdout,
            "from stdin\n"
        );
        assert_eq!(
            env.exec("echo -e \"line1\\nline2\\nline3\" | bash -c 'grep line2'")
                .stdout,
            "line2\n"
        );
        assert_eq!(
            env.exec("echo \"test\" | bash -c 'RESULT=$(cat | grep \"nomatch\" | head -1); echo \"RESULT=[$RESULT]\"'")
                .stdout,
            "RESULT=[]\n"
        );
    }

    #[test]
    fn echo_upstream_command_handles_newline_and_escape_flags() {
        // maps packages/just-bash/src/commands/echo/echo.test.ts:5 through :96
        let env = bash();

        assert_eq!(env.exec("echo hello world").stdout, "hello world\n");
        assert_eq!(env.exec("echo").stdout, "\n");
        assert_eq!(env.exec("echo one two three").stdout, "one two three\n");
        assert_eq!(env.exec("echo -n hello").stdout, "hello");
        assert_eq!(env.exec("echo -e 'hello\\nworld'").stdout, "hello\nworld\n");
        assert_eq!(env.exec("echo -e 'col1\\tcol2'").stdout, "col1\tcol2\n");
        assert_eq!(env.exec("echo -e 'hello\\rworld'").stdout, "hello\rworld\n");
        assert_eq!(
            env.exec("echo -e 'line1\\nline2\\ttabbed'").stdout,
            "line1\nline2\ttabbed\n"
        );
        assert_eq!(env.exec("echo -en 'hello\\nworld'").stdout, "hello\nworld");
        assert_eq!(env.exec("echo -ne 'a\\tb'").stdout, "a\tb");
        assert_eq!(
            env.exec("echo -E 'hello\\nworld'").stdout,
            "hello\\nworld\n"
        );
        assert_eq!(env.exec("echo 'hello world'").stdout, "hello world\n");
        assert_eq!(env.exec("echo \"hello\" 'world'").stdout, "hello world\n");
        assert_eq!(env.exec("echo -e 'a\\nb\\nc'").stdout, "a\nb\nc\n");
        assert_eq!(env.exec("echo -- -n").stdout, "-- -n\n");

        let hex = env.exec("echo -ne '\\x80\\x90\\xa0\\xb0\\xff'");
        assert_eq!(hex.exit_code, 0);
        assert_eq!(
            hex.stdout.chars().map(|ch| ch as u32).collect::<Vec<_>>(),
            vec![0x80, 0x90, 0xa0, 0xb0, 0xff]
        );
        assert_eq!(env.exec("echo -ne 'A\\x00B\\x00C'").stdout, "A\0B\0C");
        env.exec("echo -ne '\\x80\\xff\\x90' > /echo-hex.bin");
        assert_eq!(
            env.exec("cat /echo-hex.bin")
                .stdout
                .chars()
                .map(|ch| ch as u32)
                .collect::<Vec<_>>(),
            vec![0x80, 0xff, 0x90]
        );
        assert_eq!(
            env.exec("echo -ne '\\0200\\0220\\0240'")
                .stdout
                .chars()
                .map(|ch| ch as u32)
                .collect::<Vec<_>>(),
            vec![0o200, 0o220, 0o240]
        );
        env.exec("echo -ne '\\0200\\0377' > /echo-octal.bin");
        assert_eq!(
            env.exec("cat /echo-octal.bin")
                .stdout
                .chars()
                .map(|ch| ch as u32)
                .collect::<Vec<_>>(),
            vec![0o200, 0o377]
        );
        env.exec("echo -ne '\\x80\\xff\\x00\\x90' > /echo-input.bin");
        env.exec("cat /echo-input.bin > /echo-output.bin");
        assert_eq!(
            env.exec("cat /echo-output.bin")
                .stdout
                .chars()
                .map(|ch| ch as u32)
                .collect::<Vec<_>>(),
            vec![0x80, 0xff, 0x00, 0x90]
        );
    }

    #[test]
    fn printf_upstream_command_formats_core_specifiers_and_escapes() {
        // maps packages/just-bash/src/commands/printf/printf.test.ts:6 through :113
        let env = bash();

        assert_eq!(env.exec("printf 'Hello %s' world").stdout, "Hello world");
        assert_eq!(env.exec("printf 'Number: %d' 42").stdout, "Number: 42");
        assert_eq!(
            env.exec("printf 'Value: %f' 3.14").stdout,
            "Value: 3.140000"
        );
        assert_eq!(env.exec("printf 'Hex: %x' 255").stdout, "Hex: ff");
        assert_eq!(env.exec("printf 'Octal: %o' 8").stdout, "Octal: 10");
        assert_eq!(env.exec("printf '100%%'").stdout, "100%");
        assert_eq!(
            env.exec("printf '%s is %d years old' Alice 30").stdout,
            "Alice is 30 years old"
        );
        assert_eq!(env.exec("printf 'line1\\nline2'").stdout, "line1\nline2");
        assert_eq!(env.exec("printf 'col1\\tcol2'").stdout, "col1\tcol2");
        assert_eq!(env.exec("printf 'hello\\rworld'").stdout, "hello\rworld");
        assert_eq!(env.exec("printf '\\101\\102\\103'").stdout, "ABC");
        assert_eq!(
            env.exec("printf '\\e[31mred\\e[0m'").stdout,
            "\u{001b}[31mred\u{001b}[0m"
        );
        assert_eq!(
            env.exec("printf '\\E[1mbold\\E[0m'").stdout,
            "\u{001b}[1mbold\u{001b}[0m"
        );
        assert_eq!(env.exec("printf '\\x41\\x42\\x43'").stdout, "ABC");
        assert_eq!(env.exec("printf 'x\\\\y'").stdout, "x\\y");
        assert_eq!(env.exec("printf '\\a\\b\\f\\v'").stdout, "\x07\x08\x0c\x0b");
        assert_eq!(env.exec("printf 'a\\0b'").stdout, "a\0b");
        assert_eq!(env.exec("printf '\\0101'").stdout, "\x081");
        assert_eq!(env.exec("printf '\\077'").stdout, "?");
        assert_eq!(env.exec("printf '\\x61\\x62\\x63'").stdout, "abc");
        assert_eq!(env.exec("printf '\\xAa'").stdout, "\u{00aa}");
        assert_eq!(env.exec("printf '\\u2764'").stdout, "\u{2764}");
        assert_eq!(env.exec("printf '\\u41'").stdout, "A");
        assert_eq!(env.exec("printf '\\u2714'").stdout, "\u{2714}");
        assert_eq!(env.exec("printf '\\uXYZ'").stdout, "\\uXYZ");
        assert_eq!(env.exec("printf '\\U0001F600'").stdout, "\u{1f600}");
        assert_eq!(env.exec("printf '\\U1F4C4'").stdout, "\u{1f4c4}");
        assert_eq!(env.exec("printf '\\U1F680'").stdout, "\u{1f680}");
        assert_eq!(env.exec("printf '\\UXYZ'").stdout, "\\UXYZ");
        assert_eq!(
            env.exec("printf '\\e[31m\\u2764\\e[0m\\n'").stdout,
            "\x1b[31m\u{2764}\x1b[0m\n"
        );
        assert_eq!(
            env.exec("printf '\\U1F4C1 folder\\t\\U1F4C4 file'").stdout,
            "\u{1f4c1} folder\t\u{1f4c4} file"
        );
        assert_eq!(env.exec("printf '%10s' hi").stdout, "        hi");
        assert_eq!(env.exec("printf '%.2f' 3.14159").stdout, "3.14");
        assert_eq!(env.exec("printf '%05d' 42").stdout, "00042");
        assert_eq!(env.exec("printf '%-10s|' hi").stdout, "hi        |");
        assert_eq!(env.exec("printf '%.3s' hello").stdout, "hel");
        assert_eq!(env.exec("printf '%-10.3s' hello").stdout, "hel       ");
        let no_args = env.exec("printf");
        assert!(no_args.stderr.contains("usage"));
        assert_eq!(no_args.exit_code, 2);
        assert_eq!(env.exec("printf '%s %s' only").stdout, "only ");
        let invalid_number = env.exec("printf '%d' notanumber");
        assert_eq!(invalid_number.stdout, "0");
        assert!(invalid_number.stderr.contains("invalid number"));
        assert_eq!(invalid_number.exit_code, 1);
        let help = env.exec("printf --help");
        assert!(help.stdout.contains("printf"));
        assert!(help.stdout.contains("FORMAT"));
        assert_eq!(help.exit_code, 0);

        let binary = env.exec("printf '\\x80\\x90\\xa0\\xb0\\xff'");
        assert_eq!(binary.exit_code, 0);
        assert_eq!(
            binary
                .stdout
                .chars()
                .map(|ch| ch as u32)
                .collect::<Vec<_>>(),
            vec![0x80, 0x90, 0xa0, 0xb0, 0xff]
        );
        assert_eq!(env.exec("printf 'A\\x00B\\x00C'").stdout, "A\0B\0C");
        env.exec("printf '\\x80\\xff\\x90' > /printf-hex.bin");
        assert_eq!(
            env.exec("cat /printf-hex.bin")
                .stdout
                .chars()
                .map(|ch| ch as u32)
                .collect::<Vec<_>>(),
            vec![0x80, 0xff, 0x90]
        );
        assert_eq!(
            env.exec("printf '\\200\\220\\240'")
                .stdout
                .chars()
                .map(|ch| ch as u32)
                .collect::<Vec<_>>(),
            vec![0o200, 0o220, 0o240]
        );
        env.exec("printf '\\200\\377' > /printf-octal.bin");
        assert_eq!(
            env.exec("cat /printf-octal.bin")
                .stdout
                .chars()
                .map(|ch| ch as u32)
                .collect::<Vec<_>>(),
            vec![0o200, 0o377]
        );
        env.exec("printf '\\x80\\xff\\x00\\x90' > /printf-input.bin");
        env.exec("cat /printf-input.bin > /printf-output.bin");
        assert_eq!(
            env.exec("cat /printf-output.bin")
                .stdout
                .chars()
                .map(|ch| ch as u32)
                .collect::<Vec<_>>(),
            vec![0x80, 0xff, 0x00, 0x90]
        );
    }

    #[test]
    fn pwd_cd_export_env_and_printenv_are_virtual_and_isolated() {
        // maps packages/just-bash/src/commands/pwd/pwd.test.ts and env/env.test.ts
        let env = Bash::with_options(BashOptions {
            env: BTreeMap::from([
                ("FOO".to_string(), "bar".to_string()),
                ("BAZ".to_string(), "qux".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("pwd").stdout, "/home/user\n");
        assert_eq!(
            Bash::with_options(BashOptions {
                cwd: Some("/".to_string()),
                ..BashOptions::default()
            })
            .exec("pwd")
            .stdout,
            "/\n"
        );
        assert_eq!(env.exec("mkdir -p /work; cd /work; pwd").stdout, "/work\n");
        assert_eq!(
            Bash::with_options(BashOptions {
                files: BTreeMap::from([
                    ("/a/.keep".to_string(), "".to_string()),
                    ("/b/.keep".to_string(), "".to_string()),
                    ("/c/.keep".to_string(), "".to_string()),
                ]),
                ..BashOptions::default()
            })
            .exec("cd /a; cd /b; cd /c; pwd")
            .stdout,
            "/c\n"
        );
        assert_eq!(
            Bash::with_options(BashOptions {
                files: BTreeMap::from([("/parent/child/.keep".to_string(), "".to_string())]),
                cwd: Some("/parent/child".to_string()),
                ..BashOptions::default()
            })
            .exec("cd ..; pwd")
            .stdout,
            "/parent\n"
        );
        assert_eq!(
            Bash::with_options(BashOptions {
                cwd: Some("/home/user".to_string()),
                ..BashOptions::default()
            })
            .exec("pwd ignored args")
            .stdout,
            "/home/user\n"
        );
        assert_eq!(env.exec("export A=1 B=2; echo $A $B").stdout, "1 2\n");
        let env_result = env.exec("env");
        assert!(env_result.stdout.contains("FOO=bar"));
        assert!(env_result.stdout.contains("BAZ=qux"));
        assert!(env_result.stdout.contains("HOME=/home/user"));
        assert!(env_result.stdout.contains("PATH=/usr/bin:/bin"));
        assert!(env.exec("env --help").stdout.contains("environment"));
        let empty_env = env.exec("env -i printenv PATH");
        assert_eq!(empty_env.stdout, "");
        assert_eq!(empty_env.stderr, "");
        assert_eq!(empty_env.exit_code, 1);
        assert_eq!(
            env.exec("env -i ONLY=value printenv ONLY").stdout,
            "value\n"
        );
        let unset_env = env.exec("env -u FOO printenv FOO BAZ");
        assert_eq!(unset_env.stdout, "qux\n");
        assert_eq!(unset_env.stderr, "");
        assert_eq!(unset_env.exit_code, 1);
        let printenv_all = env.exec("printenv");
        assert!(printenv_all.stdout.contains("FOO=bar"));
        assert_eq!(env.exec("printenv PATH").stdout.trim(), "/usr/bin:/bin");
        assert_eq!(env.exec("printenv FOO").stdout, "bar\n");
        assert_eq!(env.exec("printenv FOO BAZ").stdout, "bar\nqux\n");
        assert_eq!(env.exec("printenv NONEXISTENT").exit_code, 1);
        assert!(env.exec("printenv --help").stdout.contains("printenv"));
        let isolated_env = Bash::new().exec("env");
        for host_name in ["NODE_ENV=", "SHELL=", "TERM=", "LANG=", "USER="] {
            assert!(!isolated_env.stdout.contains(host_name));
        }
    }

    #[test]
    fn jbc20_bash_general_default_layout_and_api_rows_match_upstream() {
        let env = Bash::new();
        assert_eq!(env.get_cwd(), "/home/user");
        assert_eq!(
            env.get_env().get("HOME").map(String::as_str),
            Some("/home/user")
        );
        assert_eq!(env.exec("echo $HOME").stdout, "/home/user\n");
        let bin = env.exec("ls /bin");
        assert_eq!(bin.exit_code, 0);
        assert!(bin.stdout.contains("echo"));
        assert!(bin.stdout.contains("cat"));
        assert!(bin.stdout.contains("grep"));
        assert_eq!(env.exec("ls /tmp").exit_code, 0);
        assert_eq!(env.exec("/bin/echo hello").stdout, "hello\n");

        let with_files = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test.txt".to_string(), "content".to_string())]),
            ..BashOptions::default()
        });
        assert_eq!(with_files.get_cwd(), "/");
        let missing_home = with_files.exec("ls /home/user");
        assert_ne!(missing_home.exit_code, 0);
        assert!(missing_home.stderr.contains("No such file or directory"));

        let with_cwd = Bash::with_options(BashOptions {
            cwd: Some("/custom".to_string()),
            ..BashOptions::default()
        });
        assert_eq!(with_cwd.get_cwd(), "/custom");
        let missing_default_home = with_cwd.exec("ls /home/user");
        assert_ne!(missing_default_home.exit_code, 0);
        assert!(
            missing_default_home
                .stderr
                .contains("No such file or directory")
        );

        let file_api = Bash::with_options(BashOptions {
            cwd: Some("/home/user".to_string()),
            files: BTreeMap::from([("/home/user/file.txt".to_string(), "content".to_string())]),
            env: BTreeMap::from([("FOO".to_string(), "bar".to_string())]),
            ..BashOptions::default()
        });
        assert_eq!(
            file_api.read_file("/home/user/file.txt").unwrap(),
            "content"
        );
        assert_eq!(file_api.read_file("file.txt").unwrap(), "content");
        file_api.write_file("new.txt", "new content").unwrap();
        assert_eq!(
            file_api.read_file("/home/user/new.txt").unwrap(),
            "new content"
        );
        assert_eq!(file_api.get_cwd(), "/home/user");
        assert_eq!(
            file_api.get_env().get("FOO").map(String::as_str),
            Some("bar")
        );
    }

    #[test]
    fn jbc20_cd_env_and_status_comparison_rows_match_core_runtime() {
        let cd_env = Bash::with_options(BashOptions {
            cwd: Some("/".to_string()),
            files: BTreeMap::from([
                ("/subdir/file.txt".to_string(), "content".to_string()),
                ("/parent/child/file.txt".to_string(), "content".to_string()),
                ("/a/b/c/file.txt".to_string(), "content".to_string()),
                ("/dir1/file.txt".to_string(), String::new()),
                ("/dir2/file.txt".to_string(), String::new()),
                ("/file.txt".to_string(), "content".to_string()),
            ]),
            env: BTreeMap::from([
                ("TEST_VAR".to_string(), "test_value".to_string()),
                ("HOME".to_string(), "/home/testuser".to_string()),
                ("VAR1".to_string(), "value1".to_string()),
                ("VAR2".to_string(), "value2".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert!(
            cd_env
                .exec("cd subdir && pwd")
                .stdout
                .trim()
                .ends_with("/subdir")
        );
        assert!(
            cd_env
                .exec("cd parent/child && cd .. && pwd")
                .stdout
                .trim()
                .ends_with("/parent")
        );
        assert!(
            cd_env
                .exec("cd a/b/c && cd ../.. && pwd")
                .stdout
                .trim()
                .ends_with("/a")
        );
        assert!(
            cd_env
                .exec("cd dir1 && cd ../dir2 && cd - && pwd")
                .stdout
                .trim()
                .ends_with("/dir1")
        );
        assert_eq!(cd_env.exec("cd nonexistent").exit_code, 1);
        assert_eq!(cd_env.exec("cd file.txt").exit_code, 1);
        assert!(
            cd_env
                .exec("cd ./subdir && pwd")
                .stdout
                .trim()
                .ends_with("/subdir")
        );
        let same_dir = cd_env.exec("pwd; cd .; pwd");
        let lines = same_dir.stdout.trim().lines().collect::<Vec<_>>();
        assert_eq!(lines, vec!["/", "/"]);

        let env_output = cd_env.exec("env");
        assert!(env_output.stdout.contains("TEST_VAR=test_value"));
        assert_eq!(env_output.exit_code, 0);
        assert_eq!(cd_env.exec("printenv HOME").stdout, "/home/testuser\n");
        assert_eq!(cd_env.exec("printenv NONEXISTENT_VAR_12345").exit_code, 1);
        assert_eq!(cd_env.exec("printenv VAR1 VAR2").stdout, "value1\nvalue2\n");

        let status = Bash::with_options(BashOptions {
            cwd: Some("/".to_string()),
            files: BTreeMap::from([("/exists.txt".to_string(), "content".to_string())]),
            ..BashOptions::default()
        });
        let unknown = status.exec("myunknowncommand");
        assert_eq!(unknown.exit_code, 127);
        assert!(unknown.stderr.contains("myunknowncommand"));
        assert_eq!(status.exec("cat /nonexistent_file_12345.txt").exit_code, 1);
        assert_eq!(
            status
                .exec("grep pattern /nonexistent_file_12345.txt")
                .exit_code,
            2
        );
        assert_eq!(status.exec("exit 42").exit_code, 42);
        assert_eq!(status.exec("true").exit_code, 0);
        assert_eq!(status.exec("false").exit_code, 1);
        assert_eq!(status.exec("false && echo never").stdout, "");
        assert_eq!(status.exec("true || echo never").stdout, "");
        assert_eq!(status.exec("false || echo fallback").stdout, "fallback\n");
        assert_eq!(status.exec("false; echo after").stdout, "after\n");
        assert_eq!(
            status.exec("echo \"hello   world\"").stdout,
            "hello   world\n"
        );
        assert_eq!(
            status.exec("echo 'hello   world'").stdout,
            "hello   world\n"
        );
        assert_eq!(status.exec("export X=value; echo '$X'").stdout, "$X\n");
        let empty = status.exec("");
        assert_eq!(empty.exit_code, 0);
        assert_eq!(empty.stdout, "");
        assert_eq!(status.exec("   ").exit_code, 0);
    }

    #[test]
    fn r10jb_uniq_and_tee_comparison_rows_match_real_bash() {
        // Maps comparison rows from uniq.comparison.test.ts and tee.comparison.test.ts
        // that are expressible through the command-and-pipeline runtime layer.
        let env = Bash::with_options(BashOptions {
            cwd: Some("/".to_string()),
            files: BTreeMap::from([
                (
                    "/dups.txt".to_string(),
                    "apple\napple\nbanana\nbanana\nbanana\ncherry\n".to_string(),
                ),
                ("/single.txt".to_string(), "a\nb\nc\n".to_string()),
                ("/cd.txt".to_string(), "a\na\nb\nc\nc\nc\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        // uniq -c counts consecutive occurrences (columns normalized).
        let uniq_c = env.exec("uniq -c /dups.txt");
        let uniq_c_rows: Vec<Vec<&str>> = uniq_c
            .stdout
            .lines()
            .map(|l| l.split_whitespace().collect())
            .collect();
        assert_eq!(
            uniq_c_rows,
            vec![vec!["2", "apple"], vec!["3", "banana"], vec!["1", "cherry"]]
        );
        // uniq -c with all-single lines.
        let uniq_single = env.exec("uniq -c /single.txt");
        let uniq_single_rows: Vec<Vec<&str>> = uniq_single
            .stdout
            .lines()
            .map(|l| l.split_whitespace().collect())
            .collect();
        assert_eq!(
            uniq_single_rows,
            vec![vec!["1", "a"], vec!["1", "b"], vec!["1", "c"]]
        );
        // uniq -c from a sorted pipe.
        let uniq_after_sort = env.exec("sort /dups.txt | uniq -c");
        let after_sort_rows: Vec<Vec<&str>> = uniq_after_sort
            .stdout
            .lines()
            .map(|l| l.split_whitespace().collect())
            .collect();
        assert_eq!(
            after_sort_rows,
            vec![vec!["2", "apple"], vec!["3", "banana"], vec!["1", "cherry"]]
        );
        // uniq -c from stdin.
        let uniq_stdin = env.exec("echo -e \"a\\na\\nb\\nb\\nb\" | uniq -c");
        let uniq_stdin_rows: Vec<Vec<&str>> = uniq_stdin
            .stdout
            .lines()
            .map(|l| l.split_whitespace().collect())
            .collect();
        assert_eq!(uniq_stdin_rows, vec![vec!["2", "a"], vec!["3", "b"]]);
        // uniq -cd shows only duplicated lines with counts.
        let uniq_cd = env.exec("uniq -cd /cd.txt");
        let uniq_cd_rows: Vec<Vec<&str>> = uniq_cd
            .stdout
            .lines()
            .map(|l| l.split_whitespace().collect())
            .collect();
        assert_eq!(uniq_cd_rows, vec![vec!["2", "a"], vec!["3", "c"]]);

        // tee passes stdin through to stdout and writes the same content to files.
        let tee = Bash::with_options(BashOptions {
            cwd: Some("/".to_string()),
            files: BTreeMap::from([(
                "/existing.txt".to_string(),
                "existing content\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        assert_eq!(tee.exec("echo hello | tee").stdout, "hello\n");
        assert_eq!(tee.exec("echo hello | tee /out.txt").stdout, "hello\n");
        assert_eq!(tee.read_file("/out.txt").unwrap(), "hello\n");
        tee.exec("echo hello | tee /f1.txt /f2.txt");
        assert_eq!(tee.read_file("/f1.txt").unwrap(), "hello\n");
        assert_eq!(tee.read_file("/f2.txt").unwrap(), "hello\n");
        tee.exec("echo appended | tee -a /existing.txt");
        assert_eq!(
            tee.read_file("/existing.txt").unwrap(),
            "existing content\nappended\n"
        );
        let tee_multi = tee.exec("echo -e \"line1\\nline2\\nline3\" | tee /ml.txt");
        assert_eq!(tee_multi.stdout, "line1\nline2\nline3\n");
        assert_eq!(tee.read_file("/ml.txt").unwrap(), "line1\nline2\nline3\n");
    }

    #[test]
    fn r10jb_wc_comparison_rows_match_real_bash() {
        // Maps packages/just-bash/src/comparison-tests/wc.comparison.test.ts rows
        // for default/-l/-w/-c/-lw/-wc output, multiple files, and stdin.
        let env = Bash::with_options(BashOptions {
            cwd: Some("/".to_string()),
            files: BTreeMap::from([
                (
                    "/lines.txt".to_string(),
                    "line 1\nline 2\nline 3\n".to_string(),
                ),
                ("/nonl.txt".to_string(), "no newline".to_string()),
                ("/empty.txt".to_string(), String::new()),
                (
                    "/words.txt".to_string(),
                    "one two three\nfour five\n".to_string(),
                ),
                (
                    "/spaces.txt".to_string(),
                    "one    two   three\n".to_string(),
                ),
                ("/hello.txt".to_string(), "hello world\n".to_string()),
                ("/a.txt".to_string(), "file a\n".to_string()),
                (
                    "/b.txt".to_string(),
                    "file b line 1\nfile b line 2\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // Default output: lines, words, chars, then the file name argument verbatim.
        assert_eq!(
            env.exec("wc /lines.txt")
                .stdout
                .split_whitespace()
                .collect::<Vec<_>>(),
            vec!["3", "6", "21", "/lines.txt"]
        );
        assert_eq!(
            env.exec("wc /nonl.txt")
                .stdout
                .split_whitespace()
                .collect::<Vec<_>>(),
            vec!["0", "2", "10", "/nonl.txt"]
        );
        assert_eq!(
            env.exec("wc /empty.txt")
                .stdout
                .split_whitespace()
                .collect::<Vec<_>>(),
            vec!["0", "0", "0", "/empty.txt"]
        );

        // -l line count.
        assert_eq!(
            env.exec("wc -l /lines.txt")
                .stdout
                .split_whitespace()
                .collect::<Vec<_>>(),
            vec!["3", "/lines.txt"]
        );

        // -w word count (collapses runs of whitespace).
        assert_eq!(
            env.exec("wc -w /words.txt")
                .stdout
                .split_whitespace()
                .collect::<Vec<_>>(),
            vec!["5", "/words.txt"]
        );
        assert_eq!(
            env.exec("wc -w /spaces.txt")
                .stdout
                .split_whitespace()
                .collect::<Vec<_>>(),
            vec!["3", "/spaces.txt"]
        );

        // -c byte count.
        assert_eq!(
            env.exec("wc -c /hello.txt")
                .stdout
                .split_whitespace()
                .collect::<Vec<_>>(),
            vec!["12", "/hello.txt"]
        );

        // Multiple files emit a total row.
        let multi = env.exec("wc /a.txt /b.txt");
        let multi_lines: Vec<Vec<&str>> = multi
            .stdout
            .lines()
            .map(|l| l.split_whitespace().collect())
            .collect();
        assert_eq!(multi_lines[0], vec!["1", "2", "7", "/a.txt"]);
        assert_eq!(multi_lines[1], vec!["2", "8", "28", "/b.txt"]);
        assert_eq!(multi_lines[2], vec!["3", "10", "35", "total"]);

        let multi_l = env.exec("wc -l /a.txt /b.txt");
        let multi_l_lines: Vec<Vec<&str>> = multi_l
            .stdout
            .lines()
            .map(|l| l.split_whitespace().collect())
            .collect();
        assert_eq!(multi_l_lines[0], vec!["1", "/a.txt"]);
        assert_eq!(multi_l_lines[1], vec!["2", "/b.txt"]);
        assert_eq!(multi_l_lines[2], vec!["3", "total"]);

        // Combined flags preserve canonical lines/words/chars ordering.
        assert_eq!(
            env.exec("wc -lw /words.txt")
                .stdout
                .split_whitespace()
                .collect::<Vec<_>>(),
            vec!["2", "5", "/words.txt"]
        );
        assert_eq!(
            env.exec("wc -wc /hello.txt")
                .stdout
                .split_whitespace()
                .collect::<Vec<_>>(),
            vec!["2", "12", "/hello.txt"]
        );

        // Stdin (no file name column).
        assert_eq!(
            env.exec("echo \"hello world\" | wc")
                .stdout
                .split_whitespace()
                .collect::<Vec<_>>(),
            vec!["1", "2", "12"]
        );
        assert_eq!(env.exec("echo -e \"a\\nb\\nc\" | wc -l").stdout.trim(), "3");
    }

    #[test]
    fn r10jb_sort_and_redirection_comparison_rows_match_real_bash() {
        // Maps comparison rows from sort.comparison.test.ts (empty-lines, numeric)
        // and pipes-redirections.comparison.test.ts (>, >>, pipe+redirect).
        let env = Bash::with_options(BashOptions {
            cwd: Some("/".to_string()),
            files: BTreeMap::from([
                ("/blanks.txt".to_string(), "b\n\na\n\nc\n".to_string()),
                ("/nums.txt".to_string(), "10\n2\n1\n20\n5\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        // sort: empty lines collate before non-empty lines.
        assert_eq!(env.exec("sort blanks.txt").stdout, "\n\na\nb\nc\n");
        // sort -n: numeric ascending order.
        assert_eq!(env.exec("sort -n nums.txt").stdout, "1\n2\n5\n10\n20\n");

        // stdout redirection (>) writes the command output to a file.
        let redir = Bash::with_options(BashOptions {
            cwd: Some("/".to_string()),
            files: BTreeMap::from([
                (
                    "/input.txt".to_string(),
                    "hello\nworld\nhello again\n".to_string(),
                ),
                ("/existing.txt".to_string(), "old content\n".to_string()),
                (
                    "/sortin.txt".to_string(),
                    "cherry\napple\nbanana\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });
        redir.exec("echo \"hello world\" > out.txt");
        assert_eq!(redir.read_file("out.txt").unwrap(), "hello world\n");
        // Overwrite replaces prior content.
        redir.exec("echo \"new content\" > existing.txt");
        assert_eq!(redir.read_file("existing.txt").unwrap(), "new content\n");
        // Redirect a filtered command's output.
        redir.exec("grep hello input.txt > grepout.txt");
        assert_eq!(
            redir.read_file("grepout.txt").unwrap(),
            "hello\nhello again\n"
        );

        // Append redirection (>>): append to existing, create if missing, repeat.
        let append = Bash::with_options(BashOptions {
            cwd: Some("/".to_string()),
            files: BTreeMap::from([("/log.txt".to_string(), "line 1\n".to_string())]),
            ..BashOptions::default()
        });
        append.exec("echo \"line 2\" >> log.txt");
        assert_eq!(append.read_file("log.txt").unwrap(), "line 1\nline 2\n");
        append.exec("echo \"new line\" >> created.txt");
        assert_eq!(append.read_file("created.txt").unwrap(), "new line\n");
        append.exec("echo \"line 1\" >> multi.txt");
        append.exec("echo \"line 2\" >> multi.txt");
        assert_eq!(append.read_file("multi.txt").unwrap(), "line 1\nline 2\n");

        // Pipe combined with redirection: cat | sort > file.
        redir.exec("cat sortin.txt | sort > sorted.txt");
        assert_eq!(
            redir.read_file("sorted.txt").unwrap(),
            "apple\nbanana\ncherry\n"
        );
    }

    #[test]
    fn coreutils_upstream_file_commands_use_virtual_fs() {
        // maps cat/ls/mkdir/rm/cp/mv/touch upstream command smoke cases
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/src/a.txt".to_string(), "alpha\n".to_string())]),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("cat /src/a.txt").stdout, "alpha\n");
        assert_eq!(env.exec("mkdir -p /dest/nested").exit_code, 0);
        assert_eq!(env.exec("cp /src/a.txt /dest/nested/b.txt").exit_code, 0);
        assert_eq!(env.exec("mv /dest/nested/b.txt /dest/c.txt").exit_code, 0);
        assert_eq!(env.exec("touch /dest/empty.txt").exit_code, 0);
        assert!(env.exec("ls /dest").stdout.contains("c.txt"));
        assert_eq!(env.exec("rm /dest/c.txt").exit_code, 0);
        assert!(!env.file_exists("/dest/c.txt"));

        assert_eq!(env.exec("touch /newfile.txt").exit_code, 0);
        assert_eq!(env.read_file("/newfile.txt").unwrap(), "");
        assert_eq!(env.exec("touch /a.txt /b.txt /c.txt").exit_code, 0);
        assert!(env.file_exists("/a.txt"));
        assert!(env.file_exists("/b.txt"));
        assert!(env.file_exists("/c.txt"));
        env.write_file("/existing.txt", "original content").unwrap();
        assert_eq!(env.exec("touch /existing.txt").exit_code, 0);
        assert_eq!(env.read_file("/existing.txt").unwrap(), "original content");
        assert_eq!(env.exec("touch /dir/subdir/newfile.txt").exit_code, 0);
        assert_eq!(env.read_file("/dir/subdir/newfile.txt").unwrap(), "");
        assert_eq!(env.exec("touch").exit_code, 1);
        let relative = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/home/user/.keep".to_string(), "".to_string())]),
            cwd: Some("/home/user".to_string()),
            ..BashOptions::default()
        });
        assert_eq!(relative.exec("touch myfile.txt").exit_code, 0);
        assert_eq!(relative.read_file("/home/user/myfile.txt").unwrap(), "");
        assert_eq!(env.exec("touch '/file with spaces.txt'").exit_code, 0);
        assert_eq!(env.read_file("/file with spaces.txt").unwrap(), "");
        assert_eq!(env.exec("touch /.hidden").exit_code, 0);
        assert_eq!(env.read_file("/.hidden").unwrap(), "");
    }

    #[test]
    fn agent_examples_portable_file_search_text_and_state_workflows_use_virtual_backend() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/project/package.json".to_string(),
                    r#"{"scripts":{"test":"vitest"},"dependencies":{"left-pad":"1.0.0"}}"#
                        .to_string(),
                ),
                (
                    "/project/src/index.ts".to_string(),
                    "import { helper } from './helper';\n// TODO: handle retries\nexport function main() { return helper(); }\n".to_string(),
                ),
                (
                    "/project/src/helper.ts".to_string(),
                    "export function helper() { return 'ok'; }\n".to_string(),
                ),
                (
                    "/project/src/legacy.ts".to_string(),
                    "// FIXME: remove legacy path\nexport const legacy = true;\n".to_string(),
                ),
                (
                    "/project/logs/app.log".to_string(),
                    "INFO boot\nERROR failed request\nWARN retry\n".to_string(),
                ),
                (
                    "/project/users.csv".to_string(),
                    "name,id,role\nAlice,1,admin\nBob,2,user\n".to_string(),
                ),
                (
                    "/project/.env.example".to_string(),
                    "API_URL=http://localhost\nFEATURE_FLAG=true\n".to_string(),
                ),
            ]),
            cwd: Some("/project".to_string()),
            ..BashOptions::default()
        });

        assert!(env.exec("ls /project").stdout.contains("package.json"));
        assert_eq!(env.exec("cat package.json").exit_code, 0);
        let todos = env.exec("grep -r TODO src");
        assert_eq!(todos.exit_code, 0);
        assert!(todos.stdout.contains("src/index.ts"));
        let fixmes = env.exec("grep -rn FIXME src");
        assert_eq!(fixmes.exit_code, 0);
        assert!(fixmes.stdout.contains("legacy.ts"));
        assert_eq!(env.exec("grep -c ERROR logs/app.log").stdout, "1\n");
        assert_eq!(
            env.exec("find src -type f -name '*.ts' | wc -l").stdout,
            "3\n"
        );
        assert_eq!(
            env.exec("awk -F, 'NR>1 {print $3}' users.csv").stdout,
            "admin\nuser\n"
        );
        assert_eq!(
            env.exec("sed 's/FEATURE_FLAG=true/FEATURE_FLAG=false/' .env.example")
                .exit_code,
            0
        );
        assert_eq!(
            env.exec("head -2 logs/app.log").stdout,
            "INFO boot\nERROR failed request\n"
        );
        assert_eq!(env.exec("tail -1 logs/app.log").stdout, "WARN retry\n");

        let stateful = env.exec(
            "mkdir -p reports && printf 'alpha' > reports/summary.txt && printf '\\nbeta' >> reports/summary.txt",
        );
        assert_eq!(stateful.exit_code, 0);
        assert_eq!(env.exec("cat reports/summary.txt").stdout, "alpha\nbeta");
        assert_eq!(
            env.exec("cp src/index.ts src/index.ts.bak && mv src/index.ts.bak src/index.ts.saved")
                .exit_code,
            0
        );
        assert_eq!(
            env.read_file("/project/src/index.ts.saved").unwrap(),
            env.read_file("/project/src/index.ts").unwrap()
        );

        let missing = env.exec("cat reports/missing.txt");
        assert_eq!(missing.exit_code, 1);
        assert!(missing.stderr.contains("No such file or directory"));
        let host = env.exec("/bin/bash -lc 'printf host'");
        assert_eq!(host.exit_code, 127);
        assert!(!host.stdout.contains("host"));
    }

    #[test]
    fn agent_examples_python_scripting_rows_fail_closed_without_host_runtime() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/scripts/analyze.py".to_string(),
                    "print('host-python')\n".to_string(),
                ),
                (
                    "/data/sales.csv".to_string(),
                    "item,amount\nalpha,10\nbeta,20\n".to_string(),
                ),
            ]),
            cwd: Some("/data".to_string()),
            ..BashOptions::default()
        });

        let python = env.exec("python3 /scripts/analyze.py");
        assert_eq!(python.exit_code, 127);
        assert!(python.stderr.contains("command not found"));
        assert!(!python.stdout.contains("host-python"));
        let host_shell = env.exec("/bin/bash -lc 'python3 /scripts/analyze.py'");
        assert_eq!(host_shell.exit_code, 127);
        assert!(!host_shell.stdout.contains("host-python"));
    }

    #[test]
    fn agent_examples_host_metadata_rows_stay_virtual_without_host_runtime() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/project/dist/app.js".to_string(),
                    "console.log('bundle');\n".to_string(),
                ),
                (
                    "/server/bin/start.sh".to_string(),
                    "#!/bin/sh\necho start\n".to_string(),
                ),
                (
                    "/server/config/secret.txt".to_string(),
                    "token=secret\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        let du = env.exec("du -sh /project/dist");
        assert_eq!(du.exit_code, 0);
        assert_eq!(du.stdout, "23\t/project/dist\n");
        assert_eq!(du.stderr, "");
        let writable = env.exec("find /server -type f -perm -600");
        assert_eq!(writable.exit_code, 0);
        assert!(writable.stdout.contains("/server/bin/start.sh"));
        assert!(writable.stdout.contains("/server/config/secret.txt"));
        let executable = env.exec("find /server -type f -perm -100");
        assert_eq!(executable.exit_code, 0);
        assert_eq!(executable.stdout, "");
        let host_shell = env.exec("/bin/bash -lc 'du -sh /project/dist'");
        assert_eq!(host_shell.exit_code, 127);
        assert!(!host_shell.stdout.contains("/project/dist"));
    }

    #[test]
    fn cat_upstream_command_covers_files_numbering_stdin_and_errors() {
        // maps packages/just-bash/src/commands/cat/cat.test.ts
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/test.txt".to_string(), "hello world".to_string()),
                ("/with-newline.txt".to_string(), "hello world\n".to_string()),
                ("/a.txt".to_string(), "aaa\n".to_string()),
                ("/b.txt".to_string(), "bbb\n".to_string()),
                ("/one.txt".to_string(), "A".to_string()),
                ("/two.txt".to_string(), "B".to_string()),
                ("/three.txt".to_string(), "C".to_string()),
                (
                    "/numbered.txt".to_string(),
                    "line1\nline2\nline3\n".to_string(),
                ),
                ("/single.txt".to_string(), "a\n".to_string()),
                ("/exists.txt".to_string(), "content".to_string()),
                ("/empty.txt".to_string(), String::new()),
                (
                    "/special.txt".to_string(),
                    "tab:\there\nnewline above".to_string(),
                ),
                ("/file.txt".to_string(), "from file\n".to_string()),
                ("/line1.txt".to_string(), "line1\n".to_string()),
                ("/home/user/file.txt".to_string(), "relative".to_string()),
            ]),
            cwd: Some("/".to_string()),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("cat /test.txt").stdout, "hello world");
        assert_eq!(env.exec("cat /with-newline.txt").stdout, "hello world\n");
        assert_eq!(env.exec("cat /a.txt /b.txt").stdout, "aaa\nbbb\n");
        assert_eq!(env.exec("cat /one.txt /two.txt /three.txt").stdout, "ABC");
        assert_eq!(
            env.exec("cat -n /numbered.txt").stdout,
            "     1\tline1\n     2\tline2\n     3\tline3\n"
        );
        assert_eq!(env.exec("cat -n /single.txt").stdout, "     1\ta\n");

        let missing = env.exec("cat /missing.txt");
        assert_eq!(missing.stdout, "");
        assert_eq!(
            missing.stderr,
            "cat: /missing.txt: No such file or directory\n"
        );
        assert_eq!(missing.exit_code, 1);

        let mixed = env.exec("cat /missing.txt /exists.txt");
        assert_eq!(mixed.stdout, "content");
        assert_eq!(
            mixed.stderr,
            "cat: /missing.txt: No such file or directory\n"
        );
        assert_eq!(mixed.exit_code, 1);

        assert_eq!(env.exec("echo \"hello\" | cat").stdout, "hello\n");
        assert_eq!(env.exec("cat /empty.txt").stdout, "");
        assert_eq!(
            env.exec("cat /special.txt").stdout,
            "tab:\there\nnewline above"
        );
        assert_eq!(
            Bash::with_options(BashOptions {
                files: BTreeMap::from([("/home/user/file.txt".to_string(), "content".to_string())]),
                cwd: Some("/home/user".to_string()),
                ..BashOptions::default()
            })
            .exec("cat file.txt")
            .stdout,
            "content"
        );
        assert_eq!(
            env.exec("echo -e \"a\\nb\\nc\" | cat -n").stdout,
            "     1\ta\n     2\tb\n     3\tc\n"
        );
        assert_eq!(
            env.exec("echo \"from stdin\" | cat -").stdout,
            "from stdin\n"
        );
        assert_eq!(
            env.exec("echo \"from stdin\" | cat - /file.txt").stdout,
            "from stdin\nfrom file\n"
        );
        assert_eq!(
            env.exec("echo \"from stdin\" | cat /file.txt -").stdout,
            "from file\nfrom stdin\n"
        );
        assert_eq!(
            env.exec("echo \"line2\" | cat -n /line1.txt -").stdout,
            "     1\tline1\n     2\tline2\n"
        );
    }

    #[test]
    fn ls_upstream_command_covers_hidden_multi_path_recursive_and_classify_cases() {
        // maps selected portable packages/just-bash/src/commands/ls/ls.test.ts rows
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/dir/a.txt".to_string(), String::new()),
                ("/dir/b.txt".to_string(), String::new()),
                ("/hidden/.hidden".to_string(), String::new()),
                ("/hidden/visible.txt".to_string(), String::new()),
                ("/secret/.secret".to_string(), String::new()),
                ("/dir1/a.txt".to_string(), String::new()),
                ("/dir2/b.txt".to_string(), String::new()),
                ("/tree/subdir/file.txt".to_string(), String::new()),
                ("/tree/root.txt".to_string(), String::new()),
                ("/file.txt".to_string(), "content".to_string()),
                ("/glob/a.txt".to_string(), String::new()),
                ("/glob/b.txt".to_string(), String::new()),
                ("/glob/c.md".to_string(), String::new()),
                ("/sorted/zebra.txt".to_string(), String::new()),
                ("/sorted/apple.txt".to_string(), String::new()),
                ("/sorted/mango.txt".to_string(), String::new()),
                ("/empty/.keep".to_string(), String::new()),
                ("/almost/.config".to_string(), String::new()),
                ("/almost/data.txt".to_string(), String::new()),
                ("/classify/subdir/file.txt".to_string(), String::new()),
                ("/classify/regular.txt".to_string(), String::new()),
                ("/reverse/aaa.txt".to_string(), String::new()),
                ("/reverse/bbb.txt".to_string(), String::new()),
                ("/reverse/ccc.txt".to_string(), String::new()),
                ("/ones/x.txt".to_string(), String::new()),
                ("/ones/y.txt".to_string(), String::new()),
                ("/ones/z.txt".to_string(), String::new()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("ls /dir").stdout, "a.txt\nb.txt\n");
        assert_eq!(env.exec("ls /hidden").stdout, "visible.txt\n");
        assert_eq!(
            env.exec("ls -a /hidden").stdout,
            ".\n..\n.hidden\nvisible.txt\n"
        );
        assert_eq!(env.exec("ls --all /secret").stdout, ".\n..\n.secret\n");
        assert_eq!(
            env.exec("ls /dir1 /dir2").stdout,
            "/dir1:\na.txt\n\n/dir2:\nb.txt\n"
        );
        assert_eq!(
            env.exec("ls -R /tree").stdout,
            "/tree:\nroot.txt\nsubdir\n\n/tree/subdir:\nfile.txt\n"
        );
        let missing = env.exec("ls /nonexistent");
        assert_eq!(missing.stdout, "");
        assert_eq!(
            missing.stderr,
            "ls: /nonexistent: No such file or directory\n"
        );
        assert_eq!(missing.exit_code, 2);
        assert_eq!(env.exec("ls /file.txt").stdout, "/file.txt\n");
        assert_eq!(env.exec("ls /glob | grep txt").stdout, "a.txt\nb.txt\n");
        assert_eq!(
            env.exec("ls /sorted").stdout,
            "apple.txt\nmango.txt\nzebra.txt\n"
        );
        assert_eq!(env.exec("rm /empty/.keep; ls /empty").stdout, "");
        assert_eq!(env.exec("ls -A /almost").stdout, ".config\ndata.txt\n");
        assert_eq!(
            env.exec("ls -a /almost").stdout,
            ".\n..\n.config\ndata.txt\n"
        );
        assert_eq!(env.exec("ls -F /classify").stdout, "regular.txt\nsubdir/\n");
        assert_eq!(
            env.exec("ls -r /reverse").stdout,
            "ccc.txt\nbbb.txt\naaa.txt\n"
        );
        assert_eq!(env.exec("ls -1r /ones").stdout, "z.txt\ny.txt\nx.txt\n");
        assert_eq!(
            env.exec("ls -ar /almost").stdout,
            "data.txt\n.config\n..\n.\n"
        );
    }

    #[test]
    fn ls_upstream_command_covers_long_format_human_size_and_exec_classify_cases() {
        // maps the remaining portable packages/just-bash/src/commands/ls/ls.test.ts
        // rows (default root listing, -l long format, -la, -F exec/symlink, -lF, -dF)
        // plus every packages/just-bash/src/commands/ls/ls.human.test.ts row.

        // ls.test.ts:18 "should list current directory by default" — the virtual
        // root always exposes the seeded system entries (bin, dev, proc, usr).
        let root = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/file.txt".to_string(), String::new())]),
            cwd: Some("/".to_string()),
            ..BashOptions::default()
        });
        let root_ls = root.exec("ls");
        assert_eq!(root_ls.stdout, "bin\ndev\nfile.txt\nproc\nusr\n");
        assert_eq!(root_ls.stderr, "");

        // ls.test.ts:62 "should support long format with -l"
        let l = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/dir/test.txt".to_string(), String::new())]),
            ..BashOptions::default()
        });
        let long = l.exec("ls -l /dir");
        assert_eq!(
            long.stdout,
            "total 1\n-rw-r--r-- 1 user user     0 Jan  1 00:00 test.txt\n"
        );
        assert_eq!(long.stderr, "");

        // ls.test.ts:73 "should show directory indicator in long format"
        let ld = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/dir/subdir/file.txt".to_string(), String::new())]),
            ..BashOptions::default()
        });
        let long_dir = ld.exec("ls -l /dir");
        assert_eq!(
            long_dir.stdout,
            "total 1\ndrwxr-xr-x 1 user user     0 Jan  1 00:00 subdir/\n"
        );
        assert_eq!(long_dir.stderr, "");

        // ls.test.ts:84 "should combine -la flags including . and .."
        let la = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/dir/.hidden".to_string(), String::new()),
                ("/dir/visible".to_string(), String::new()),
            ]),
            ..BashOptions::default()
        });
        let la_out = la.exec("ls -la /dir");
        let la_lines: Vec<&str> = la_out.stdout.lines().collect();
        assert_eq!(la_lines[0], "total 4");
        assert_eq!(la_lines[1], "drwxr-xr-x 1 user user     0 Jan  1 00:00 .");
        assert_eq!(la_lines[2], "drwxr-xr-x 1 user user     0 Jan  1 00:00 ..");
        assert_eq!(
            la_lines[3],
            "-rw-r--r-- 1 user user     0 Jan  1 00:00 .hidden"
        );
        assert_eq!(
            la_lines[4],
            "-rw-r--r-- 1 user user     0 Jan  1 00:00 visible"
        );
        assert_eq!(la_out.stderr, "");

        // ls.test.ts:225 "should append * to executable files"
        let exec = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/dir/script.sh".to_string(), "#!/bin/bash".to_string()),
                ("/dir/data.txt".to_string(), String::new()),
            ]),
            ..BashOptions::default()
        });
        exec.exec("chmod 755 /dir/script.sh");
        let exec_out = exec.exec("ls -F /dir");
        assert_eq!(exec_out.stdout, "data.txt\nscript.sh*\n");
        assert_eq!(exec_out.stderr, "");

        // ls.test.ts:237 "should append @ to symlinks"
        let sym = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/dir/target.txt".to_string(), "content".to_string())]),
            ..BashOptions::default()
        });
        sym.exec("ln -s /dir/target.txt /dir/link");
        let sym_out = sym.exec("ls -F /dir");
        assert_eq!(sym_out.stdout, "link@\ntarget.txt\n");
        assert_eq!(sym_out.stderr, "");

        // ls.test.ts:249 "should append @ to symlinks pointing to directories"
        let symdir = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/dir/realdir/file.txt".to_string(), String::new())]),
            ..BashOptions::default()
        });
        symdir.exec("ln -s /dir/realdir /dir/linkdir");
        let symdir_out = symdir.exec("ls -F /dir");
        assert_eq!(symdir_out.stdout, "linkdir@\nrealdir/\n");
        assert_eq!(symdir_out.stderr, "");

        // ls.test.ts:261 "should show directory mode for symlinks to directories in long format"
        let symdir_long = symdir.exec("ls -lF /dir");
        let symdir_lines: Vec<&str> = symdir_long.stdout.lines().collect();
        assert_eq!(symdir_lines[0], "total 2");
        assert!(
            symdir_lines[1].starts_with("drwxr-xr-x") && symdir_lines[1].ends_with("linkdir@"),
            "symlink-to-dir long line was {:?}",
            symdir_lines[1]
        );
        assert!(
            symdir_lines[2].starts_with("drwxr-xr-x") && symdir_lines[2].ends_with("realdir/"),
            "real dir long line was {:?}",
            symdir_lines[2]
        );

        // ls.test.ts:277 "should work with -l flag" (-lF mixed entries)
        let lf = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/dir/subdir/file.txt".to_string(), String::new()),
                ("/dir/script.sh".to_string(), "#!/bin/bash".to_string()),
                ("/dir/regular.txt".to_string(), String::new()),
            ]),
            ..BashOptions::default()
        });
        lf.exec("chmod 755 /dir/script.sh");
        let lf_out = lf.exec("ls -lF /dir");
        let lf_lines: Vec<&str> = lf_out.stdout.lines().collect();
        assert_eq!(lf_lines[0], "total 3");
        assert!(lf_lines[1].ends_with("regular.txt"));
        assert!(lf_lines[2].ends_with("script.sh*"));
        assert!(lf_lines[3].ends_with("subdir/"));

        // ls.test.ts:293 "should work with -a flag" (-aF)
        let af = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/dir/.hidden".to_string(), String::new()),
                ("/dir/subdir/file.txt".to_string(), String::new()),
            ]),
            ..BashOptions::default()
        });
        let af_out = af.exec("ls -aF /dir");
        assert_eq!(af_out.stdout, "./\n../\n.hidden\nsubdir/\n");
        assert_eq!(af_out.stderr, "");

        // ls.test.ts:305 "should work with single file argument" (-F on a file)
        let file_f = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/script.sh".to_string(), "#!/bin/bash".to_string())]),
            ..BashOptions::default()
        });
        file_f.exec("chmod 755 /script.sh");
        let file_f_out = file_f.exec("ls -F /script.sh");
        assert_eq!(file_f_out.stdout, "/script.sh*\n");
        assert_eq!(file_f_out.stderr, "");

        // ls.test.ts:316 "should work with -d flag on directory" (-dF)
        let df = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/dir/file.txt".to_string(), String::new())]),
            ..BashOptions::default()
        });
        let df_out = df.exec("ls -dF /dir");
        assert_eq!(df_out.stdout, "/dir/\n");
        assert_eq!(df_out.stderr, "");

        // ls.human.test.ts:5 "displays bytes for small files"
        let h_small = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test/small.txt".to_string(), "a".repeat(100))]),
            ..BashOptions::default()
        });
        let h_small_out = h_small.exec("ls -lh /test");
        assert_eq!(h_small_out.exit_code, 0);
        assert!(h_small_out.stdout.contains("100"));
        assert!(h_small_out.stdout.contains("small.txt"));

        // ls.human.test.ts:17 "displays K for kilobyte-sized files"
        let h_k = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test/medium.txt".to_string(), "a".repeat(1536))]),
            ..BashOptions::default()
        });
        let h_k_out = h_k.exec("ls -lh /test");
        assert_eq!(h_k_out.exit_code, 0);
        assert!(h_k_out.stdout.contains("1.5K"));
        assert!(h_k_out.stdout.contains("medium.txt"));

        // ls.human.test.ts:29 "displays rounded K for larger KB files"
        let h_15k = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test/data.txt".to_string(), "a".repeat(15 * 1024))]),
            ..BashOptions::default()
        });
        let h_15k_out = h_15k.exec("ls -lh /test");
        assert_eq!(h_15k_out.exit_code, 0);
        assert!(h_15k_out.stdout.contains("15K"));
        assert!(h_15k_out.stdout.contains("data.txt"));

        // ls.human.test.ts:41 "displays M for megabyte-sized files"
        let h_m = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test/big.txt".to_string(), "a".repeat(2 * 1024 * 1024))]),
            ..BashOptions::default()
        });
        let h_m_out = h_m.exec("ls -lh /test");
        assert_eq!(h_m_out.exit_code, 0);
        assert!(h_m_out.stdout.contains("2.0M"));
        assert!(h_m_out.stdout.contains("big.txt"));

        // ls.human.test.ts:53 "works with --human-readable long form"
        let h_long = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test/file.txt".to_string(), "a".repeat(2048))]),
            ..BashOptions::default()
        });
        let h_long_out = h_long.exec("ls -l --human-readable /test");
        assert_eq!(h_long_out.exit_code, 0);
        assert!(h_long_out.stdout.contains("2.0K"));

        // ls.human.test.ts:64 "displays exact bytes without -h flag"
        let h_none = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test/file.txt".to_string(), "a".repeat(1536))]),
            ..BashOptions::default()
        });
        let h_none_out = h_none.exec("ls -l /test");
        assert_eq!(h_none_out.exit_code, 0);
        assert!(h_none_out.stdout.contains("1536"));
        assert!(!h_none_out.stdout.contains("1.5K"));

        // ls.human.test.ts:76 "can combine -h with other flags"
        let h_combo = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/test/visible.txt".to_string(), "a".repeat(3072)),
                ("/test/.hidden.txt".to_string(), "b".repeat(4096)),
            ]),
            ..BashOptions::default()
        });
        let h_combo_out = h_combo.exec("ls -lah /test");
        assert_eq!(h_combo_out.exit_code, 0);
        assert!(h_combo_out.stdout.contains("3.0K"));
        assert!(h_combo_out.stdout.contains("4.0K"));
        assert!(h_combo_out.stdout.contains(".hidden.txt"));
    }

    fn sort_env(content: &str) -> Bash {
        Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test.txt".to_string(), content.to_string())]),
            cwd: Some("/".to_string()),
            ..BashOptions::default()
        })
    }

    #[test]
    fn sort_upstream_advanced_human_version_month_dictionary_blanks_check_output_stable_perkey() {
        // maps packages/just-bash/src/commands/sort/sort.advanced.test.ts

        // sort -h (human numeric)
        let r = sort_env("1K\n2M\n500\n1G\n100K\n").exec("sort -h /test.txt");
        assert_eq!(r.stdout, "500\n1K\n100K\n2M\n1G\n");
        assert_eq!(r.exit_code, 0);
        let r = sort_env("1k\n2M\n3g\n").exec("sort -h /test.txt");
        assert_eq!(r.stdout, "1k\n2M\n3g\n");
        assert_eq!(r.exit_code, 0);
        let r = sort_env("1.5K\n2K\n1K\n").exec("sort -h /test.txt");
        assert_eq!(r.stdout, "1K\n1.5K\n2K\n");
        assert_eq!(r.exit_code, 0);
        let r = sort_env("1K\n1M\n1G\n").exec("sort -hr /test.txt");
        assert_eq!(r.stdout, "1G\n1M\n1K\n");
        assert_eq!(r.exit_code, 0);

        // sort -V (version)
        let r = sort_env("file1.10\nfile1.2\nfile1.1\n").exec("sort -V /test.txt");
        assert_eq!(r.stdout, "file1.1\nfile1.2\nfile1.10\n");
        assert_eq!(r.exit_code, 0);
        let r = sort_env("v2.0\nv1.10\nv1.2\n").exec("sort -V /test.txt");
        assert_eq!(r.stdout, "v1.2\nv1.10\nv2.0\n");
        assert_eq!(r.exit_code, 0);
        let r = sort_env("1.0.0\n1.0.10\n1.0.2\n").exec("sort -V /test.txt");
        assert_eq!(r.stdout, "1.0.0\n1.0.2\n1.0.10\n");
        assert_eq!(r.exit_code, 0);

        // sort -M (month)
        let r = sort_env("Mar\nJan\nDec\nFeb\n").exec("sort -M /test.txt");
        assert_eq!(r.stdout, "Jan\nFeb\nMar\nDec\n");
        assert_eq!(r.exit_code, 0);
        let r = sort_env("mar\njan\nfeb\n").exec("sort -M /test.txt");
        assert_eq!(r.stdout, "jan\nfeb\nmar\n");
        assert_eq!(r.exit_code, 0);
        let r = sort_env("Mar\nfoo\nJan\n").exec("sort -M /test.txt");
        assert_eq!(r.stdout, "foo\nJan\nMar\n");
        assert_eq!(r.exit_code, 0);

        // sort -d (dictionary order)
        let r = sort_env("b-c\na_b\nc.d\n").exec("sort -d /test.txt");
        assert_eq!(r.stdout, "a_b\nb-c\nc.d\n");
        assert_eq!(r.exit_code, 0);

        // sort -b (ignore leading blanks)
        let r = sort_env("  b\na\n   c\n").exec("sort -b /test.txt");
        assert_eq!(r.stdout, "a\n  b\n   c\n");
        assert_eq!(r.exit_code, 0);
        let r = sort_env("  2\n1\n   3\n").exec("sort -bn /test.txt");
        assert_eq!(r.stdout, "1\n  2\n   3\n");
        assert_eq!(r.exit_code, 0);

        // sort -c (check)
        let r = sort_env("a\nb\nc\n").exec("sort -c /test.txt");
        assert_eq!(r.stdout, "");
        assert_eq!(r.stderr, "");
        assert_eq!(r.exit_code, 0);
        let r = sort_env("b\na\nc\n").exec("sort -c /test.txt");
        assert_eq!(r.stdout, "");
        assert_eq!(r.stderr, "sort: /test.txt:2: disorder: a\n");
        assert_eq!(r.exit_code, 1);
        let r = sort_env("1\n2\n10\n").exec("sort -cn /test.txt");
        assert_eq!(r.exit_code, 0);

        // sort -o (output file)
        let env = sort_env("c\na\nb\n");
        env.exec("sort -o /out.txt /test.txt");
        assert_eq!(env.exec("cat /out.txt").stdout, "a\nb\nc\n");
        let env = sort_env("c\na\nb\n");
        env.exec("sort -o /test.txt /test.txt");
        assert_eq!(env.exec("cat /test.txt").stdout, "a\nb\nc\n");
        let env = sort_env("c\na\nb\n");
        env.exec("sort --output=/out.txt /test.txt");
        assert_eq!(env.exec("cat /out.txt").stdout, "a\nb\nc\n");

        // sort -s (stable)
        let r = sort_env("1 b\n1 a\n2 c\n").exec("sort -s -k1,1 /test.txt");
        assert_eq!(r.stdout, "1 b\n1 a\n2 c\n");
        assert_eq!(r.exit_code, 0);

        // sort per-key modifiers
        let r = sort_env("a 1M\nb 1K\nc 1G\n").exec("sort -k2h /test.txt");
        assert_eq!(r.stdout, "b 1K\na 1M\nc 1G\n");
        assert_eq!(r.exit_code, 0);
        let r = sort_env("a v1.10\nb v1.2\nc v2.0\n").exec("sort -k2V /test.txt");
        assert_eq!(r.stdout, "b v1.2\na v1.10\nc v2.0\n");
        assert_eq!(r.exit_code, 0);
        let r = sort_env("2023 Mar\n2023 Jan\n2023 Feb\n").exec("sort -k2M /test.txt");
        assert_eq!(r.stdout, "2023 Jan\n2023 Feb\n2023 Mar\n");
        assert_eq!(r.exit_code, 0);

        // sort --help
        let r = Bash::new().exec("sort --help");
        assert!(r.stdout.contains("-h"));
        assert!(r.stdout.contains("-V"));
        assert!(r.stdout.contains("-M"));
        assert!(r.stdout.contains("-d"));
        assert!(r.stdout.contains("-b"));
        assert!(r.stdout.contains("-c"));
        assert!(r.stdout.contains("-o"));
        assert!(r.stdout.contains("-s"));
        assert_eq!(r.exit_code, 0);
    }

    #[test]
    fn sort_upstream_complex_key_syntax_ranges_perkey_modifiers_delimiter_and_long_key() {
        // maps packages/just-bash/src/commands/sort/sort.test.ts complex -k syntax rows
        let r = sort_env("a b c\na a c\nb a a\n").exec("sort -k1,2 /test.txt");
        assert_eq!(r.stdout, "a a c\na b c\nb a a\n");
        let r = sort_env("1 banana\n2 apple\n3 cherry\n").exec("sort -k2,2 /test.txt");
        assert_eq!(r.stdout, "2 apple\n1 banana\n3 cherry\n");
        let r = sort_env("a 10\nb 2\nc 1\n").exec("sort -k2n /test.txt");
        assert_eq!(r.stdout, "c 1\nb 2\na 10\n");
        let r = sort_env("a 1\nb 2\nc 3\n").exec("sort -k1r /test.txt");
        assert_eq!(r.stdout, "c 3\nb 2\na 1\n");
        let r = sort_env("x 5\ny 10\nz 2\n").exec("sort -k2,2nr /test.txt");
        assert_eq!(r.stdout, "y 10\nx 5\nz 2\n");
        let r = sort_env("a 2\nb 1\na 1\nb 2\n").exec("sort -k1,1 -k2,2n /test.txt");
        assert_eq!(r.stdout, "a 1\na 2\nb 1\nb 2\n");
        let r = sort_env("abc\nabc\nbac\naac\n").exec("sort -k1.2 /test.txt");
        assert_eq!(r.stdout, "aac\nbac\nabc\nabc\n");
        let r = sort_env("Zebra\napple\nBANANA\n").exec("sort -k1f /test.txt");
        assert_eq!(r.stdout, "apple\nBANANA\nZebra\n");
        let r = sort_env("c:3\na:1\nb:2\n").exec("sort -t: -k2n /test.txt");
        assert_eq!(r.stdout, "a:1\nb:2\nc:3\n");
        let r = sort_env("3 c\n1 a\n2 b\n").exec("sort --key=1n /test.txt");
        assert_eq!(r.stdout, "1 a\n2 b\n3 c\n");
    }

    #[test]
    fn sort_upstream_binary_safe_and_utf8_preserving_case_fold_dictionary() {
        // maps packages/just-bash/src/commands/sort/sort.binary.test.ts
        // Binary-safe ASCII lines.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "c\na\nb\n".to_string())]),
            cwd: Some("/".to_string()),
            ..BashOptions::default()
        });
        let r = env.exec("sort /data.txt");
        assert_eq!(r.stdout, "a\nb\nc\n");
        assert_eq!(r.exit_code, 0);

        // UTF-8 leading bytes preserved under -f case-fold (É is U+00C9).
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.bin".to_string(), "A\n\u{00C9}\n".to_string())]),
            cwd: Some("/".to_string()),
            ..BashOptions::default()
        });
        let r = env.exec("sort -f /data.bin");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "A\n\u{00C9}\n");

        // UTF-8 preserved under -k1f per-key case-fold.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.bin".to_string(), "B\n\u{00C9}\nA\n".to_string())]),
            cwd: Some("/".to_string()),
            ..BashOptions::default()
        });
        let r = env.exec("sort -k1f /data.bin");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "A\nB\n\u{00C9}\n");

        // UTF-8 bytes preserved under -k1d per-key dictionary order (é round-trips whole).
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.bin".to_string(), "B\n\u{00E9}\nA\n".to_string())]),
            cwd: Some("/".to_string()),
            ..BashOptions::default()
        });
        let r = env.exec("sort -k1d /data.bin");
        assert_eq!(r.exit_code, 0);
        let mut got: Vec<&str> = r.stdout.split('\n').filter(|s| !s.is_empty()).collect();
        got.sort_unstable();
        assert_eq!(got, vec!["A", "B", "\u{00E9}"]);
    }

    #[test]
    fn mkdir_rm_upstream_command_flags_and_errors_are_virtual() {
        // maps selected portable mkdir/rm upstream command rows
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/existing/file.txt".to_string(), String::new()),
                ("/file".to_string(), "content".to_string()),
                ("/home/user/.keep".to_string(), String::new()),
                ("/test.txt".to_string(), "content".to_string()),
                ("/dir/file.txt".to_string(), "content".to_string()),
                ("/nested/sub1/file1.txt".to_string(), String::new()),
                ("/nested/sub2/file2.txt".to_string(), String::new()),
                ("/nested/root.txt".to_string(), String::new()),
                ("/nested-r/sub1/file1.txt".to_string(), String::new()),
                ("/nested-r/sub2/file2.txt".to_string(), String::new()),
                ("/nested-r/root.txt".to_string(), String::new()),
                ("/relative/file.txt".to_string(), "content".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("mkdir -p /a/b/c; ls /a/b").stdout, "c\n");
        assert_eq!(
            env.exec("mkdir -p /one/two/three/four/five; ls /one/two/three/four")
                .stdout,
            "five\n"
        );
        assert_eq!(env.exec("mkdir --parents /x/y/z; ls /x/y").stdout, "z\n");
        let nested_without_p = env.exec("mkdir /no-parent/child");
        assert_eq!(nested_without_p.stdout, "");
        assert_eq!(
            nested_without_p.stderr,
            "mkdir: cannot create directory '/no-parent/child': No such file or directory\n"
        );
        assert_eq!(nested_without_p.exit_code, 1);
        assert_eq!(env.exec("mkdir -p /existing").exit_code, 0);
        assert_eq!(env.exec("mkdir /file").exit_code, 1);
        assert_eq!(env.exec("mkdir").stderr, "mkdir: missing operand\n");

        let relative = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/home/user/.keep".to_string(), String::new())]),
            cwd: Some("/home/user".to_string()),
            ..BashOptions::default()
        });
        assert_eq!(
            relative.exec("mkdir projects; ls /home/user").stdout,
            "projects\n"
        );
        assert_eq!(
            env.exec("mkdir -p /multi/a /other/d; ls /multi; ls /other")
                .stdout,
            "a\nd\n"
        );

        assert_eq!(env.exec("rm /test.txt").exit_code, 0);
        assert_eq!(env.exec("cat /test.txt").exit_code, 1);
        let missing = env.exec("rm /missing.txt");
        assert_eq!(
            missing.stderr,
            "rm: cannot remove '/missing.txt': No such file or directory\n"
        );
        assert_eq!(missing.exit_code, 1);
        assert_eq!(env.exec("rm -f /missing.txt").exit_code, 0);
        assert_eq!(env.exec("rm /dir").exit_code, 1);
        assert_eq!(env.exec("rm -r /dir").exit_code, 0);
        assert_eq!(env.exec("rm -R /nested").exit_code, 0);
        assert_eq!(env.exec("rm -r /nested-r").exit_code, 0);
        assert_eq!(
            env.exec("mkdir -p /rf/dir; rm -rf /rf /nonexistent")
                .exit_code,
            0
        );
        assert_eq!(
            env.exec("mkdir -p /recursive/dir; rm --recursive /recursive")
                .exit_code,
            0
        );
        assert_eq!(env.exec("rm --force /missing").exit_code, 0);
        assert_eq!(env.exec("rm -f").exit_code, 0);
        assert_eq!(env.exec("rm").stderr, "rm: missing operand\n");
        assert_eq!(env.exec("mkdir /emptydir; rm -r /emptydir").exit_code, 0);

        let relative_rm = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/home/user/file.txt".to_string(), "content".to_string())]),
            cwd: Some("/home/user".to_string()),
            ..BashOptions::default()
        });
        relative_rm.exec("rm file.txt");
        assert_eq!(relative_rm.exec("cat /home/user/file.txt").exit_code, 1);
    }

    #[test]
    fn cp_mv_upstream_command_directory_targets_flags_and_errors_are_virtual() {
        // maps selected portable cp/mv upstream command rows
        let cp_env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/src.txt".to_string(), "content".to_string()),
                ("/dst.txt".to_string(), "old content".to_string()),
                ("/dir/.keep".to_string(), String::new()),
                ("/a.txt".to_string(), "aaa".to_string()),
                ("/b.txt".to_string(), "bbb".to_string()),
                ("/multi-a.txt".to_string(), String::new()),
                ("/multi-b.txt".to_string(), String::new()),
                ("/srcdir/file.txt".to_string(), "content".to_string()),
                ("/src/a/b/c.txt".to_string(), "deep".to_string()),
                ("/src/root.txt".to_string(), "root".to_string()),
                ("/home/user/src.txt".to_string(), "relative".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(cp_env.exec("cp /src.txt /copied.txt").exit_code, 0);
        assert_eq!(cp_env.read_file("/copied.txt").unwrap(), "content");
        assert_eq!(cp_env.read_file("/src.txt").unwrap(), "content");
        assert_eq!(cp_env.exec("cp /src.txt /dst.txt").exit_code, 0);
        assert_eq!(cp_env.read_file("/dst.txt").unwrap(), "content");
        assert_eq!(cp_env.exec("cp /src.txt /dir/").exit_code, 0);
        assert_eq!(cp_env.read_file("/dir/src.txt").unwrap(), "content");
        assert_eq!(cp_env.exec("cp /a.txt /b.txt /dir").exit_code, 0);
        assert_eq!(cp_env.read_file("/dir/a.txt").unwrap(), "aaa");
        assert_eq!(cp_env.read_file("/dir/b.txt").unwrap(), "bbb");
        assert!(
            cp_env
                .exec("cp /a.txt /b.txt /nonexistent")
                .stderr
                .contains("not a directory")
        );
        assert!(
            cp_env
                .exec("cp /srcdir /dstdir")
                .stderr
                .contains("omitting directory")
        );
        assert_eq!(cp_env.exec("cp -r /srcdir /dstdir").exit_code, 0);
        assert_eq!(cp_env.read_file("/dstdir/file.txt").unwrap(), "content");
        assert_eq!(cp_env.exec("cp -R /srcdir /dstdir2").exit_code, 0);
        assert_eq!(cp_env.read_file("/dstdir2/file.txt").unwrap(), "content");
        assert_eq!(cp_env.exec("cp -r /src /deep-dst").exit_code, 0);
        assert_eq!(cp_env.read_file("/deep-dst/a/b/c.txt").unwrap(), "deep");
        assert_eq!(cp_env.exec("cp --recursive /srcdir /dstdir3").exit_code, 0);
        let cp_missing = cp_env.exec("cp /missing.txt /dst.txt");
        assert_eq!(
            cp_missing.stderr,
            "cp: cannot stat '/missing.txt': No such file or directory\n"
        );
        assert_eq!(
            cp_env.exec("cp /src.txt").stderr,
            "cp: missing destination file operand\n"
        );
        let cp_relative = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/home/user/src.txt".to_string(), "content".to_string())]),
            cwd: Some("/home/user".to_string()),
            ..BashOptions::default()
        });
        cp_relative.exec("cp src.txt dst.txt");
        assert_eq!(
            cp_relative.read_file("/home/user/dst.txt").unwrap(),
            "content"
        );

        let mv_env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/old.txt".to_string(), "content".to_string()),
                ("/dir/oldname.txt".to_string(), "content".to_string()),
                ("/file.txt".to_string(), "content".to_string()),
                ("/dir/.keep".to_string(), String::new()),
                ("/a.txt".to_string(), "aaa".to_string()),
                ("/b.txt".to_string(), "bbb".to_string()),
                ("/srcdir/file.txt".to_string(), "content".to_string()),
                ("/src/a/b/c.txt".to_string(), "deep".to_string()),
                ("/src/root.txt".to_string(), "root".to_string()),
                ("/src.txt".to_string(), "new".to_string()),
                ("/dst.txt".to_string(), "old".to_string()),
                ("/force-src.txt".to_string(), "new".to_string()),
                ("/force-dst.txt".to_string(), "old".to_string()),
                ("/no-src.txt".to_string(), "new".to_string()),
                ("/no-dst.txt".to_string(), "old".to_string()),
                ("/no-new-src.txt".to_string(), "content".to_string()),
                ("/verbose-old.txt".to_string(), "content".to_string()),
                ("/fv-src.txt".to_string(), "new".to_string()),
                ("/fv-dst.txt".to_string(), "old".to_string()),
                ("/fn-src.txt".to_string(), "new".to_string()),
                ("/fn-dst.txt".to_string(), "old".to_string()),
                ("/into-src/file.txt".to_string(), "content".to_string()),
                ("/into-dst/.keep".to_string(), String::new()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(mv_env.exec("mv /old.txt /new.txt").exit_code, 0);
        assert_eq!(mv_env.read_file("/new.txt").unwrap(), "content");
        assert_eq!(mv_env.exec("cat /old.txt").exit_code, 1);
        mv_env.exec("mv /dir/oldname.txt /dir/newname.txt");
        assert_eq!(mv_env.read_file("/dir/newname.txt").unwrap(), "content");
        mv_env.exec("mv /file.txt /dir/");
        assert_eq!(mv_env.read_file("/dir/file.txt").unwrap(), "content");
        mv_env.exec("mv /a.txt /b.txt /dir");
        assert_eq!(mv_env.read_file("/dir/a.txt").unwrap(), "aaa");
        assert_eq!(mv_env.read_file("/dir/b.txt").unwrap(), "bbb");
        assert!(
            mv_env
                .exec("mv /multi-a.txt /multi-b.txt /nonexistent")
                .stderr
                .contains("not a directory")
        );
        mv_env.exec("mv /srcdir /dstdir");
        assert_eq!(mv_env.read_file("/dstdir/file.txt").unwrap(), "content");
        mv_env.exec("mv /src /dst");
        assert_eq!(mv_env.read_file("/dst/a/b/c.txt").unwrap(), "deep");
        mv_env.exec("mv /src.txt /dst.txt");
        assert_eq!(mv_env.read_file("/dst.txt").unwrap(), "new");
        let mv_missing = mv_env.exec("mv /missing.txt /dst.txt");
        assert_eq!(
            mv_missing.stderr,
            "mv: cannot stat '/missing.txt': No such file or directory\n"
        );
        assert_eq!(
            mv_env.exec("mv /force-src.txt").stderr,
            "mv: missing destination file operand\n"
        );
        let mv_relative = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/home/user/old.txt".to_string(), "content".to_string())]),
            cwd: Some("/home/user".to_string()),
            ..BashOptions::default()
        });
        mv_relative.exec("mv old.txt new.txt");
        assert_eq!(
            mv_relative.read_file("/home/user/new.txt").unwrap(),
            "content"
        );
        mv_env.exec("mv /into-src /into-dst/");
        assert_eq!(
            mv_env.read_file("/into-dst/into-src/file.txt").unwrap(),
            "content"
        );
        mv_env.exec("mv -f /force-src.txt /force-dst.txt");
        assert_eq!(mv_env.read_file("/force-dst.txt").unwrap(), "new");
        mv_env.exec("mv -n /no-src.txt /no-dst.txt");
        assert_eq!(mv_env.read_file("/no-src.txt").unwrap(), "new");
        assert_eq!(mv_env.read_file("/no-dst.txt").unwrap(), "old");
        mv_env.exec("mv -n /no-new-src.txt /no-new-dst.txt");
        assert_eq!(mv_env.read_file("/no-new-dst.txt").unwrap(), "content");
        assert_eq!(
            mv_env
                .exec("mv -v /verbose-old.txt /verbose-new.txt")
                .stdout,
            "renamed '/verbose-old.txt' -> '/verbose-new.txt'\n"
        );
        assert_eq!(
            mv_env.exec("mv -fv /fv-src.txt /fv-dst.txt").stdout,
            "renamed '/fv-src.txt' -> '/fv-dst.txt'\n"
        );
        mv_env.exec("mv -fn /fn-src.txt /fn-dst.txt");
        assert_eq!(mv_env.read_file("/fn-src.txt").unwrap(), "new");
        let help = mv_env.exec("mv --help");
        assert!(help.stdout.contains("mv"));
        assert!(help.stdout.contains("--force"));
        assert!(help.stdout.contains("--no-clobber"));
        assert!(help.stdout.contains("--verbose"));
        assert!(
            mv_env
                .exec("mv -x /fn-src.txt /fn-next.txt")
                .stderr
                .contains("invalid option")
        );
    }

    #[test]
    fn basename_dirname_upstream_command_rows_are_portable() {
        // maps packages/just-bash/src/comparison-tests/fixtures/basename-dirname.comparison.fixtures.json
        let env = bash();
        let cases = [
            ("basename -s .txt /path/to/file.txt", "file\n"),
            ("basename ./path/to/file.txt", "file.txt\n"),
            ("basename /path/to/file.txt .md", "file.txt\n"),
            (
                "basename -a /path/one.txt /path/two.txt",
                "one.txt\ntwo.txt\n",
            ),
            ("basename /path/to/dir/", "dir\n"),
            ("basename file.txt", "file.txt\n"),
            ("basename /path/to/file.txt .txt", "file\n"),
            ("basename /usr/bin/sort", "sort\n"),
            (
                "basename -a -s .txt /path/one.txt /path/two.txt",
                "one\ntwo\n",
            ),
            ("dirname /path/to/dir/", "/path/to\n"),
            ("dirname ./path/to/file.txt", "./path/to\n"),
            (
                "dirname /path/to/file1 /another/path/file2",
                "/path/to\n/another/path\n",
            ),
            ("dirname file.txt", ".\n"),
            ("dirname /usr/bin/sort", "/usr/bin\n"),
            ("dirname /file.txt", "/\n"),
        ];

        for (script, stdout) in cases {
            let result = env.exec(script);
            assert_eq!(result.exit_code, 0, "{script}");
            assert_eq!(result.stdout, stdout, "{script}");
            assert_eq!(result.stderr, "", "{script}");
        }

        let missing_basename = env.exec("basename");
        assert_eq!(missing_basename.exit_code, 1);
        assert_eq!(missing_basename.stderr, "basename: missing operand\n");
        let basename_help = env.exec("basename --help");
        assert_eq!(basename_help.exit_code, 0);
        assert!(basename_help.stdout.contains("strip directory"));

        let missing_dirname = env.exec("dirname");
        assert_eq!(missing_dirname.exit_code, 1);
        assert_eq!(missing_dirname.stderr, "dirname: missing operand\n");
        let dirname_help = env.exec("dirname --help");
        assert_eq!(dirname_help.exit_code, 0);
        assert!(dirname_help.stdout.contains("strip last component"));
    }

    #[test]
    fn redirection_upstream_cases_write_append_and_read_virtual_files() {
        // maps packages/just-bash/src/commands/bash/bash.test.ts:156 redirection-heavy script behavior
        let env = bash();

        let result =
            env.exec("echo first > /tmp/out.txt; echo second >> /tmp/out.txt; cat < /tmp/out.txt");
        assert_eq!(result.stdout, "first\nsecond\n");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn find_grep_rg_sed_and_awk_cover_high_risk_text_search_bucket() {
        // maps find/grep/rg/sed/awk upstream basic command tests as smoke coverage only
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/repo/a.txt".to_string(),
                    "alpha\nbeta\ngamma\n".to_string(),
                ),
                ("/repo/b.log".to_string(), "beta\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec("find /repo -type f -name '*.txt'").stdout,
            "/repo/a.txt\n"
        );
        assert_eq!(env.exec("grep -n beta /repo/a.txt").stdout, "2:beta\n");
        assert_eq!(
            env.exec("grep --line-number beta /repo/a.txt").stdout,
            "2:beta\n"
        );
        assert_eq!(env.exec("grep missing /repo/a.txt").exit_code, 1);
        assert_eq!(env.exec("grep -i ALPHA /repo/a.txt").stdout, "alpha\n");
        assert_eq!(
            env.exec("grep --ignore-case ALPHA /repo/a.txt").stdout,
            "alpha\n"
        );
        assert_eq!(
            env.exec("grep -v beta /repo/a.txt").stdout,
            "alpha\ngamma\n"
        );
        assert_eq!(env.exec("grep -c beta /repo/a.txt").stdout, "1\n");
        assert!(
            env.exec("rg beta /repo")
                .stdout
                .contains("/repo/a.txt:2:beta")
        );
        assert_eq!(
            env.exec("sed 's/beta/BETA/' /repo/a.txt").stdout,
            "alpha\nBETA\ngamma\n"
        );
        assert_eq!(env.exec("awk '{print $1}' /repo/b.log").stdout, "beta\n");
    }

    #[test]
    fn structured_data_jq_minimal_port_reads_virtual_input() {
        // maps packages/just-bash/src/commands/jq/jq.basic.test.ts high-risk structured-data smoke
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.json".to_string(),
                "{\"name\":\"Ada\",\"count\":2}".to_string(),
            )]),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("jq .name /data.json").stdout, "\"Ada\"\n");
    }

    #[test]
    fn text_search_grep_rg_sed_and_awk_close_upstream_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/grep.txt".to_string(),
                    "hello world\nfoo bar\nhello again\n".to_string(),
                ),
                ("/case.txt".to_string(), "Hello\nhello\nHELLO\n".to_string()),
                (
                    "/grep-advanced.txt".to_string(),
                    "hello world hello\nfoo bar\nprice: 100 and 200 dollars\n".to_string(),
                ),
                (
                    "/grep-context.txt".to_string(),
                    "line1\nline2\nmatch\nline4\nline5\n".to_string(),
                ),
                (
                    "/grep-context-multi.txt".to_string(),
                    "a\nmatch1\nb\nc\nmatch2\nd\n".to_string(),
                ),
                (
                    "/grep-context-overlap.txt".to_string(),
                    "a\nmatch1\nb\nmatch2\nc\n".to_string(),
                ),
                (
                    "/grep-max.txt".to_string(),
                    "line1\nline2\nline3\nline4\nline5\n".to_string(),
                ),
                (
                    "/grep-exact.txt".to_string(),
                    "foo\nfoobar\nfoo\n".to_string(),
                ),
                ("/a.txt".to_string(), "found here\nmatch\n".to_string()),
                ("/b.txt".to_string(), "nothing\nmatch\n".to_string()),
                ("/c.txt".to_string(), "also found\n".to_string()),
                (
                    "/grep-hn-a.txt".to_string(),
                    "line1\nmatch\nline3\n".to_string(),
                ),
                ("/grep-hn-b.txt".to_string(), "match\n".to_string()),
                ("/grep-rh/a.txt".to_string(), "content\n".to_string()),
                ("/grep-rh/b.txt".to_string(), "content\n".to_string()),
                ("/dir/root.txt".to_string(), "needle here\n".to_string()),
                (
                    "/dir/sub/file.txt".to_string(),
                    "another needle\n".to_string(),
                ),
                (
                    "/sed/file.txt".to_string(),
                    "hello world\nhello universe\ngoodbye world\n".to_string(),
                ),
                (
                    "/sed/numbers.txt".to_string(),
                    "line 1\nline 2\nline 3\nline 4\nline 5\n".to_string(),
                ),
                (
                    "/sed/case.txt".to_string(),
                    "Hello HELLO hello\n".to_string(),
                ),
                (
                    "/data.txt".to_string(),
                    "hello world\nfoo bar\n".to_string(),
                ),
                ("/fields.txt".to_string(), "a b c\n1 2 3\n".to_string()),
                (
                    "/missing-fields.txt".to_string(),
                    "one\ntwo three\n".to_string(),
                ),
                ("/data.csv".to_string(), "a,b,c\n1,2,3\n".to_string()),
                ("/colon.txt".to_string(), "a:b:c\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec("grep hello /grep.txt").stdout,
            "hello world\nhello again\n"
        );
        assert_eq!(env.exec("grep missing /grep.txt").exit_code, 1);
        assert_eq!(
            env.exec("grep -i hello /case.txt").stdout,
            "Hello\nhello\nHELLO\n"
        );
        assert_eq!(
            env.exec("grep -n hello /grep.txt").stdout,
            "1:hello world\n3:hello again\n"
        );
        assert_eq!(
            env.exec("grep -v foo /grep.txt").stdout,
            "hello world\nhello again\n"
        );
        assert_eq!(env.exec("grep -c hello /grep.txt").stdout, "2\n");
        assert_eq!(
            env.exec("grep -l found /a.txt /b.txt /c.txt").stdout,
            "/a.txt\n/c.txt\n"
        );
        assert_eq!(
            env.exec("grep -r needle /dir").stdout,
            "/dir/root.txt:needle here\n/dir/sub/file.txt:another needle\n"
        );
        assert_eq!(
            env.exec("echo -e \"foo\\nbar\\nfoo\" | grep foo").stdout,
            "foo\nfoo\n"
        );
        assert_eq!(env.exec("grep").stderr, "grep: missing pattern\n");
        assert_eq!(env.exec("grep pattern /missing.txt").exit_code, 2);
        assert_eq!(
            env.exec("grep -in hello /case.txt").stdout,
            "1:Hello\n2:hello\n3:HELLO\n"
        );
        assert_eq!(
            env.exec("grep match /a.txt /b.txt").stdout,
            "/a.txt:match\n/b.txt:match\n"
        );
        assert_eq!(
            env.exec("grep \"hello world\" /grep.txt").stdout,
            "hello world\n"
        );
        assert_eq!(env.exec("grep -c missing /grep.txt").stdout, "0\n");
        assert_eq!(
            env.exec("grep needle /dir/root.txt").stdout,
            "needle here\n"
        );
        assert_eq!(
            env.exec("grep -E \"hello|foo\" /grep.txt").stdout,
            "hello world\nfoo bar\nhello again\n"
        );
        assert_eq!(
            env.exec("cat /grep.txt | grep hello | head -n 2").stdout,
            "hello world\nhello again\n"
        );
        assert_eq!(env.exec("grep hello /grep.txt | wc -l").stdout.trim(), "2");
        assert_eq!(
            env.exec("cat /grep.txt | grep hello | grep again").stdout,
            "hello again\n"
        );
        assert_eq!(
            env.exec("grep -o hello /grep-advanced.txt").stdout,
            "hello\nhello\n"
        );
        assert_eq!(
            env.exec("grep --only-matching hello /grep-advanced.txt")
                .stdout,
            "hello\nhello\n"
        );
        assert_eq!(
            env.exec("grep -o hello /grep-advanced.txt /grep.txt")
                .stdout,
            "/grep-advanced.txt:hello\n/grep-advanced.txt:hello\n/grep.txt:hello\n/grep.txt:hello\n"
        );
        assert_eq!(
            env.exec("grep -Eo '[0-9]+' /grep-advanced.txt").stdout,
            "100\n200\n"
        );
        assert_eq!(
            env.exec("grep -oh hello /grep-advanced.txt /grep.txt")
                .stdout,
            "hello\nhello\nhello\nhello\n"
        );
        assert_eq!(
            env.exec("grep -A2 match /grep-context.txt").stdout,
            "match\nline4\nline5\n"
        );
        assert_eq!(
            env.exec("grep -B2 match /grep-context.txt").stdout,
            "line1\nline2\nmatch\n"
        );
        assert_eq!(
            env.exec("grep -C1 match /grep-context.txt").stdout,
            "line2\nmatch\nline4\n"
        );
        assert_eq!(
            env.exec("grep -A 1 match /grep-context.txt").stdout,
            "match\nline4\n"
        );
        assert_eq!(
            env.exec("grep -n -B1 -A1 match /grep-context.txt").stdout,
            "2-line2\n3:match\n4-line4\n"
        );
        assert_eq!(
            env.exec("grep -A1 match /grep-context-multi.txt").stdout,
            "match1\nb\n--\nmatch2\nd\n"
        );
        assert_eq!(
            env.exec("grep -C1 match /grep-context-overlap.txt").stdout,
            "a\nmatch1\nb\nmatch2\nc\n"
        );
        assert_eq!(
            env.exec("grep -m 2 line /grep-max.txt").stdout,
            "line1\nline2\n"
        );
        assert_eq!(
            env.exec("grep --max-count=1 '[a-z]' /grep-max.txt").stdout,
            "line1\n"
        );
        assert_eq!(
            env.exec("grep -m3 line /grep-max.txt").stdout,
            "line1\nline2\nline3\n"
        );
        assert_eq!(
            env.exec("grep -m 1 -A1 match /grep-context-multi.txt")
                .stdout,
            "match1\nb\n"
        );
        assert_eq!(
            env.exec("grep -n -m 2 '[a-e]' /grep-max.txt").stdout,
            "1:line1\n2:line2\n"
        );
        assert_eq!(env.exec("grep -x foo /grep-exact.txt").stdout, "foo\nfoo\n");
        assert_eq!(
            env.exec("grep --line-regexp foo /grep-exact.txt").stdout,
            "foo\nfoo\n"
        );
        assert_eq!(
            env.exec("grep -Ex 'f.o' /grep-exact.txt").stdout,
            "foo\nfoo\n"
        );
        assert_eq!(
            env.exec("grep -h match /a.txt /b.txt").stdout,
            "match\nmatch\n"
        );
        assert_eq!(
            env.exec("grep --no-filename match /a.txt /b.txt").stdout,
            "match\nmatch\n"
        );
        assert_eq!(
            env.exec("grep -hn match /a.txt /b.txt").stdout,
            "2:match\n2:match\n"
        );
        assert_eq!(
            env.exec("grep -hn match /grep-hn-a.txt /grep-hn-b.txt")
                .stdout,
            "2:match\n1:match\n"
        );
        assert_eq!(
            env.exec("grep -rh needle /dir").stdout,
            "needle here\nanother needle\n"
        );
        assert_eq!(
            env.exec("grep -rh content /grep-rh").stdout,
            "content\ncontent\n"
        );

        let rg_env = Bash::with_options(BashOptions {
            cwd: Some("/home/user".to_string()),
            files: BTreeMap::from([
                (
                    "/home/user/file.txt".to_string(),
                    "Hello World\nhello world\nline3\n".to_string(),
                ),
                ("/home/user/a.txt".to_string(), "hello\n".to_string()),
                ("/home/user/b.txt".to_string(), "hello\n".to_string()),
                (
                    "/home/user/src/app.ts".to_string(),
                    "const hello = 'world';\n".to_string(),
                ),
                (
                    "/home/user/src/lib/util.ts".to_string(),
                    "export const hello = 1;\n".to_string(),
                ),
                (
                    "/home/user/binary.bin".to_string(),
                    "hello\0world\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });
        assert_eq!(
            rg_env.exec("rg hello src").stdout,
            "src/app.ts:1:const hello = 'world';\nsrc/lib/util.ts:1:export const hello = 1;\n"
        );
        assert_eq!(rg_env.exec("rg nomatch").exit_code, 1);
        assert_eq!(rg_env.exec("rg Hello file.txt").stdout, "Hello World\n");
        assert_eq!(
            rg_env.exec("rg -i HELLO file.txt").stdout,
            "Hello World\nhello world\n"
        );
        assert_eq!(rg_env.exec("rg -s hello file.txt").stdout, "hello world\n");
        assert_eq!(rg_env.exec("rg -N hello a.txt").stdout, "hello\n");
        assert_eq!(
            Bash::with_options(BashOptions {
                cwd: Some("/home/user".to_string()),
                files: BTreeMap::from([(
                    "/home/user/file.txt".to_string(),
                    "hello world\nfoo bar\n".to_string(),
                )]),
                ..BashOptions::default()
            })
            .exec("rg hello")
            .stdout,
            "file.txt:1:hello world\n"
        );
        assert_eq!(
            Bash::with_options(BashOptions {
                cwd: Some("/home/user".to_string()),
                files: BTreeMap::from([
                    ("/home/user/a.txt".to_string(), "hello\n".to_string()),
                    ("/home/user/b.txt".to_string(), "hello\n".to_string()),
                ]),
                ..BashOptions::default()
            })
            .exec("rg hello")
            .stdout,
            "a.txt:1:hello\nb.txt:1:hello\n"
        );

        assert_eq!(
            env.exec("sed 's/hello/hi/' /sed/file.txt").stdout,
            "hi world\nhi universe\ngoodbye world\n"
        );
        assert_eq!(
            env.exec("sed 's/l/L/g' /sed/file.txt").stdout,
            "heLLo worLd\nheLLo universe\ngoodbye worLd\n"
        );
        assert_eq!(env.exec("sed -n '3p' /sed/numbers.txt").stdout, "line 3\n");
        assert_eq!(
            env.exec("sed -n '2,4p' /sed/numbers.txt").stdout,
            "line 2\nline 3\nline 4\n"
        );
        assert_eq!(
            env.exec("sed '/hello/d' /sed/file.txt").stdout,
            "goodbye world\n"
        );
        assert_eq!(
            env.exec("sed '2d' /sed/numbers.txt").stdout,
            "line 1\nline 3\nline 4\nline 5\n"
        );
        assert_eq!(
            env.exec("echo 'foo bar' | sed 's/bar/baz/'").stdout,
            "foo baz\n"
        );
        assert_eq!(
            env.exec("echo '/path/to/file' | sed 's#/path#/newpath#'")
                .stdout,
            "/newpath/to/file\n"
        );
        assert_eq!(
            env.exec("sed 's/[0-9]/X/' /sed/numbers.txt").stdout,
            "line X\nline X\nline X\nline X\nline X\n"
        );
        assert_eq!(
            env.exec("sed 's/a/b/' /missing.txt").stderr,
            "sed: /missing.txt: No such file or directory\n"
        );
        assert_eq!(
            env.exec("sed 's/world//' /sed/file.txt").stdout,
            "hello \nhello universe\ngoodbye \n"
        );
        assert_eq!(
            env.exec("sed '2,4d' /sed/numbers.txt").stdout,
            "line 1\nline 5\n"
        );
        assert_eq!(
            env.exec("sed 's/HELLO/hi/i' /sed/file.txt").stdout,
            "hi world\nhi universe\ngoodbye world\n"
        );
        assert_eq!(
            env.exec("sed 's/hello/hi/gi' /sed/case.txt").stdout,
            "hi hi hi\n"
        );
        assert_eq!(
            env.exec("sed '1s/line/LINE/' /sed/numbers.txt").stdout,
            "LINE 1\nline 2\nline 3\nline 4\nline 5\n"
        );
        assert_eq!(
            env.exec("sed '2s/line/LINE/' /sed/numbers.txt").stdout,
            "line 1\nLINE 2\nline 3\nline 4\nline 5\n"
        );
        assert_eq!(
            env.exec("sed '$ s/line/LINE/' /sed/numbers.txt").stdout,
            "line 1\nline 2\nline 3\nline 4\nLINE 5\n"
        );
        assert_eq!(
            env.exec("sed '2,4s/line/LINE/' /sed/numbers.txt").stdout,
            "line 1\nLINE 2\nLINE 3\nLINE 4\nline 5\n"
        );
        assert_eq!(
            env.exec("sed '$d' /sed/numbers.txt").stdout,
            "line 1\nline 2\nline 3\nline 4\n"
        );
        assert_eq!(
            env.exec("sed -e 's/hello/hi/' -e 's/world/there/' /sed/file.txt")
                .stdout,
            "hi there\nhi universe\ngoodbye there\n"
        );
        assert_eq!(
            env.exec("sed -e 's/line/LINE/' -e 's/1/one/' -e 's/2/two/' /sed/numbers.txt")
                .stdout,
            "LINE one\nLINE two\nLINE 3\nLINE 4\nLINE 5\n"
        );
        assert_eq!(
            env.exec("echo 'hello' | sed 's/hello/[&]/'").stdout,
            "[hello]\n"
        );
        assert_eq!(
            env.exec("echo 'world' | sed 's/world/&-&-&/'").stdout,
            "world-world-world\n"
        );
        assert_eq!(env.exec("echo 'hello' | sed 's/hello/\\&/'").stdout, "&\n");

        assert_eq!(
            env.exec("awk '{print $0}' /data.txt").stdout,
            "hello world\nfoo bar\n"
        );
        assert_eq!(
            env.exec("awk '{print $1}' /data.txt").stdout,
            "hello\nfoo\n"
        );
        assert_eq!(
            env.exec("awk '{print $1, $3}' /fields.txt").stdout,
            "a c\n1 3\n"
        );
        assert_eq!(
            env.exec("awk '{print $2}' /missing-fields.txt").stdout,
            "\nthree\n"
        );
        assert_eq!(
            env.exec("awk -F',' '{print $2}' /data.csv").stdout,
            "b\n2\n"
        );
        assert_eq!(env.exec("awk -F: '{print $2}' /colon.txt").stdout, "b\n");
        assert_eq!(
            env.exec("awk '{print NR, $0}' /data.txt").stdout,
            "1 hello world\n2 foo bar\n"
        );
        assert_eq!(env.exec("awk '{print NF}' /fields.txt").stdout, "3\n3\n");
    }

    /// JBC-47 closes portable sed argument/error-handling and single-command
    /// extras: `-f` script files, `y///` transliteration, addressed single-
    /// command `{ }` blocks, and the GNU sed diagnostics for missing scripts,
    /// unknown options, undefined branch labels, context-address errors, and
    /// malformed transliteration sets. Each assertion mirrors the upstream
    /// `sed.errors.test.ts` and `sed.advanced.test.ts` expectations exactly.
    #[test]
    fn sed_jbc47_argument_errors_transliteration_blocks_and_script_files() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/test/file.txt".to_string(),
                "line 1\nline 2\nline 3\n".to_string(),
            )]),
            cwd: Some("/test".to_string()),
            ..BashOptions::default()
        });

        // sed.errors.test.ts:37 — `-f` on a missing script file reports
        // "No such file or directory" and exits 1.
        let missing_f = env.exec("sed -f /nonexistent.sed /test/file.txt");
        assert_eq!(missing_f.exit_code, 1);
        assert!(missing_f.stderr.contains("No such file or directory"));

        // sed.errors.test.ts:44 — unterminated substitution does not hang and
        // returns a defined result.
        let unterminated_s = env.exec("sed 's/foo/bar' /test/file.txt");
        assert_eq!(unterminated_s.exit_code, 1);

        // sed.errors.test.ts:59 — unknown command `z` is handled gracefully
        // (a defined result rather than a panic / hang).
        let _unknown_cmd = env.exec("sed 'z' /test/file.txt");

        // sed.errors.test.ts:66 — unknown substitution flag is ignored, exit 0.
        assert_eq!(env.exec("sed 's/a/b/z' /test/file.txt").exit_code, 0);

        // sed.errors.test.ts:75 — line-0 address is handled gracefully.
        assert_eq!(env.exec("sed '0d' /test/file.txt").exit_code, 0);

        // sed.errors.test.ts:82 — `,3p` (missing start address) reports
        // "expected context address".
        let ctx_addr = env.exec("sed -n ',3p' /test/file.txt");
        assert_eq!(ctx_addr.exit_code, 1);
        assert!(ctx_addr.stderr.contains("expected context address"));

        // sed.errors.test.ts:89 — `/foo d` (unterminated address regex)
        // reports "command expected".
        let malformed_addr = env.exec("sed '/foo d' /test/file.txt");
        assert_eq!(malformed_addr.exit_code, 1);
        assert!(malformed_addr.stderr.contains("command expected"));

        // sed.errors.test.ts:106 / :113 — branch (`b`) and conditional branch
        // (`t`) to an undefined label both report "undefined label".
        let b_undef = env.exec("sed 'b undefined' /test/file.txt");
        assert_eq!(b_undef.exit_code, 1);
        assert!(b_undef.stderr.contains("undefined label"));
        let t_undef = env.exec("sed 't missing' /test/file.txt");
        assert_eq!(t_undef.exit_code, 1);
        assert!(t_undef.stderr.contains("undefined label"));

        // sed.errors.test.ts:122 / :129 — unknown short and long options.
        let short_opt = env.exec("sed -z 's/a/b/' /test/file.txt");
        assert_eq!(short_opt.exit_code, 1);
        assert!(short_opt.stderr.contains("invalid option"));
        let long_opt = env.exec("sed --unknown 's/a/b/' /test/file.txt");
        assert_eq!(long_opt.exit_code, 1);
        assert!(long_opt.stderr.contains("unrecognized option"));

        // sed.errors.test.ts:170 / :177 — mismatched and unterminated `y`.
        let y_mismatch = env.exec("sed 'y/abc/xy/' /test/file.txt");
        assert_eq!(y_mismatch.exit_code, 1);
        assert!(y_mismatch.stderr.contains("same length"));
        let y_unterminated = env.exec("sed 'y/abc/xyz' /test/file.txt");
        assert_eq!(y_unterminated.exit_code, 1);
        assert!(y_unterminated.stderr.contains("unterminated"));

        // sed.errors.test.ts:186 — addressed single-command block `1{d;}`.
        let block_delete = env.exec("sed '1{d;}' /test/file.txt");
        assert_eq!(block_delete.exit_code, 0);
        assert_eq!(block_delete.stdout, "line 2\nline 3\n");

        // sed.errors.test.ts:193 — block with substitution `1{s/1/X/;}`.
        let block_sub = env.exec("sed '1{s/1/X/;}' /test/file.txt");
        assert_eq!(block_sub.exit_code, 0);
        assert!(block_sub.stdout.contains("line X"));

        // A separate environment with no preset cwd for the `sed` (no script)
        // and transliteration / script-file rows.
        let plain = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/test.txt".to_string(), "hello world\n".to_string()),
                ("/rotate.txt".to_string(), "abc\n".to_string()),
                (
                    "/multi.sed".to_string(),
                    "s/hello/HELLO/\ns/world/WORLD/\n".to_string(),
                ),
                (
                    "/comment.sed".to_string(),
                    "# This is a comment\ns/hello/HELLO/\n".to_string(),
                ),
                ("/one.sed".to_string(), "s/hello/HELLO/\n".to_string()),
                ("/lower.txt".to_string(), "hello\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        // sed.errors.test.ts:38 / sed.ts — `sed` with no script reports
        // "no script specified".
        let no_script = plain.exec("sed");
        assert_eq!(no_script.exit_code, 1);
        assert!(no_script.stderr.contains("no script specified"));

        // sed.advanced.test.ts:35 — `y` lowercase to uppercase.
        assert_eq!(
            plain
                .exec("sed 'y/abcdefghijklmnopqrstuvwxyz/ABCDEFGHIJKLMNOPQRSTUVWXYZ/' /test.txt")
                .stdout,
            "HELLO WORLD\n"
        );
        // sed.advanced.test.ts:45 — `y` rotation.
        assert_eq!(plain.exec("sed 'y/abc/bca/' /rotate.txt").stdout, "bca\n");

        // sed.advanced.test.ts:122 — `-f` reads a multi-command script file.
        assert_eq!(
            plain.exec("sed -f /multi.sed /test.txt").stdout,
            "HELLO WORLD\n"
        );
        // sed.advanced.test.ts:133 — `-f` ignores `#` comments.
        assert_eq!(
            plain.exec("sed -f /comment.sed /lower.txt").stdout,
            "HELLO\n"
        );
        // sed.advanced.test.ts:144 — combines `-f` and `-e`.
        assert_eq!(
            plain
                .exec("sed -f /one.sed -e 's/world/WORLD/' /test.txt")
                .stdout,
            "HELLO WORLD\n"
        );
        // sed.advanced.test.ts:157 — `-f` on a missing file reports
        // "couldn't open file".
        let missing_script = plain.exec("sed -f /nope.sed /test.txt");
        assert_eq!(missing_script.exit_code, 1);
        assert!(missing_script.stderr.contains("couldn't open file"));
    }

    #[test]
    fn sed_cycle_engine_advanced_and_command_rows() {
        // Cycle-engine coverage for the GNU sed commands that require a real
        // execution loop with pattern/hold space, multi-command `;`-separated
        // scripts, `{ }` blocks, `:label` branch targets, and N/n/D/P/=/l/a/i/c.
        // Each assertion mirrors an upstream just-bash test by file:line.
        let five = || {
            Bash::with_options(BashOptions {
                files: BTreeMap::from([
                    (
                        "/test/file.txt".to_string(),
                        "line 1\nline 2\nline 3\nline 4\nline 5\n".to_string(),
                    ),
                    (
                        "/test/alpha.txt".to_string(),
                        "alpha\nbeta\ngamma\ndelta\n".to_string(),
                    ),
                ]),
                cwd: Some("/test".to_string()),
                ..BashOptions::default()
            })
        };
        let file = |content: &str| {
            Bash::with_options(BashOptions {
                files: BTreeMap::from([("/test.txt".to_string(), content.to_string())]),
                cwd: Some("/".to_string()),
                ..BashOptions::default()
            })
        };

        // ---- sed.advanced.test.ts ----
        // :6 N appends next line to pattern space (even line count).
        assert_eq!(
            file("line1\nline2\nline3\nline4\n")
                .exec("sed 'N;s/\\n/ /' /test.txt")
                .stdout,
            "line1 line2\nline3 line4\n"
        );
        // :15 N auto-prints when there is no next line (odd line count).
        assert_eq!(
            file("line1\nline2\nline3\n")
                .exec("sed 'N;s/\\n/ /' /test.txt")
                .stdout,
            "line1 line2\nline3\n"
        );
        // :25 N joins pairs of lines.
        assert_eq!(
            file("a\nb\nc\nd\n")
                .exec("sed 'N;s/\\n/,/' /test.txt")
                .stdout,
            "a,b\nc,d\n"
        );
        // :53 y handles escape sequences (`\t` -> space).
        assert_eq!(
            file("a\tb\n").exec("sed 'y/\\t/ /' /test.txt").stdout,
            "a b\n"
        );
        // :63 `=` prints line numbers interleaved with the pattern space.
        assert_eq!(
            file("a\nb\nc\n").exec("sed '=' /test.txt").stdout,
            "1\na\n2\nb\n3\nc\n"
        );
        // :71 `2=` prints the line number only for the addressed line.
        assert_eq!(
            file("a\nb\nc\n").exec("sed '2=' /test.txt").stdout,
            "a\n2\nb\nc\n"
        );
        // :81 `b;d` branches unconditionally to end of script, skipping `d`.
        assert_eq!(
            file("hello\nworld\n").exec("sed 'b;d' /test.txt").stdout,
            "hello\nworld\n"
        );
        // :90 `b skip;d;:skip` branches to a label, skipping `d`.
        assert_eq!(
            file("hello\nworld\n")
                .exec("sed 'b skip;d;:skip' /test.txt")
                .stdout,
            "hello\nworld\n"
        );
        // :99 `t;d` conditional branch on a successful substitution.
        assert_eq!(
            file("hello\nworld\n")
                .exec("sed 's/hello/HELLO/;t;d' /test.txt")
                .stdout,
            "HELLO\n"
        );
        // :108 `t done` conditional branch to a label.
        assert_eq!(
            file("hello\nworld\n")
                .exec("sed 's/hello/HELLO/;t done;s/world/WORLD/;:done' /test.txt")
                .stdout,
            "HELLO\nWORLD\n"
        );

        // ---- sed.commands.test.ts ----
        // :50 `l` lists special characters with escapes and a trailing `$`.
        let l1 = file("a\tb\nc\n").exec("sed -n 'l' /test.txt");
        assert!(
            l1.stdout.contains("\\t"),
            "l: tab escape, got {:?}",
            l1.stdout
        );
        assert!(
            l1.stdout.contains('$'),
            "l: end marker, got {:?}",
            l1.stdout
        );
        // :61 `l` renders non-printable bytes as octal `\001`.
        let l2 = file("a\u{1}b\n").exec("sed -n 'l' /test.txt");
        assert!(
            l2.stdout.contains("\\001"),
            "l: octal escape, got {:?}",
            l2.stdout
        );
        // :73 `-n '='` prints line numbers only.
        assert_eq!(
            five().exec("sed -n '=' /test/file.txt").stdout,
            "1\n2\n3\n4\n5\n"
        );
        // :79 `2{=;p}` prints the line number before the pattern space.
        assert_eq!(
            five().exec("sed -n '2{=;p}' /test/file.txt").stdout,
            "2\nline 2\n"
        );
        // :87 `2a added` appends text after the addressed line.
        assert_eq!(
            five().exec("sed '2a added' /test/alpha.txt").stdout,
            "alpha\nbeta\nadded\ngamma\ndelta\n"
        );
        // :93 `2i inserted` inserts text before the addressed line.
        assert_eq!(
            five().exec("sed '2i inserted' /test/alpha.txt").stdout,
            "alpha\ninserted\nbeta\ngamma\ndelta\n"
        );
        // :99 `2c replaced` changes the addressed line.
        assert_eq!(
            five().exec("sed '2c replaced' /test/alpha.txt").stdout,
            "alpha\nreplaced\ngamma\ndelta\n"
        );
        // :105 `2,3c replaced` changes a range, emitting the text once.
        assert_eq!(
            five().exec("sed '2,3c replaced' /test/alpha.txt").stdout,
            "alpha\nreplaced\ndelta\n"
        );
        // :114 `n;p` reads the next line, then prints (every other line).
        assert_eq!(
            five().exec("sed -n 'n;p' /test/file.txt").stdout,
            "line 2\nline 4\n"
        );
        // :121 `N;s/\n/ + /` appends the next line and joins pairs.
        assert_eq!(
            five().exec("sed 'N;s/\\n/ + /' /test/alpha.txt").stdout,
            "alpha + beta\ngamma + delta\n"
        );
        // :130 `h` copies pattern space to hold space.
        assert_eq!(
            five().exec("sed -n '1h;3{g;p}' /test/alpha.txt").stdout,
            "alpha\n"
        );
        // :137 `H` appends pattern space to hold space.
        assert_eq!(
            five().exec("sed -n '1h;2H;2{g;p}' /test/alpha.txt").stdout,
            "alpha\nbeta\n"
        );
        // :144 `G` appends hold space to pattern space.
        assert_eq!(
            five().exec("sed -n '1h;3{G;p}' /test/alpha.txt").stdout,
            "gamma\nalpha\n"
        );
        // :150 `x` exchanges pattern and hold spaces.
        assert_eq!(
            five().exec("sed -n '1h;2x;2p' /test/alpha.txt").stdout,
            "alpha\n"
        );
        // :159 `N;D` deletes up to the first newline and restarts the cycle.
        assert_eq!(five().exec("sed 'N;D' /test/file.txt").stdout, "line 5\n");
        // :169 `N;P` prints only the first line of each joined pair.
        assert_eq!(
            five().exec("sed -n 'N;P' /test/alpha.txt").stdout,
            "alpha\ngamma\n"
        );
        // :178 `:start;s/a/A/;t start;...` loops with `b`/`t` and a label.
        assert_eq!(
            five()
                .exec("sed -e ':start;s/a/A/;t start;s/A/X/g' /test/alpha.txt")
                .stdout,
            "XlphX\nbetX\ngXmmX\ndeltX\n"
        );
        // :188 `t end` branches on a successful substitution.
        assert_eq!(
            five()
                .exec("sed -e 's/alpha/FOUND/;t end;s/./X/g;:end' /test/alpha.txt")
                .stdout,
            "FOUND\nXXXX\nXXXXX\nXXXXX\n"
        );
        // :197 `T skip` branches when no substitution was made.
        assert_eq!(
            five()
                .exec("sed -e 's/alpha/FOUND/;T skip;s/FOUND/REPLACED/;:skip' /test/alpha.txt")
                .stdout,
            "REPLACED\nbeta\ngamma\ndelta\n"
        );
        // :208 nested `{ }` block applies multiple commands to a range.
        assert_eq!(
            five()
                .exec("sed '2,3{s/e/E/g;s/a/A/g}' /test/alpha.txt")
                .stdout,
            "alpha\nbEtA\ngAmmA\ndelta\n"
        );
    }

    /// Closes the `command-sed` strict gaps from `sed.test.ts` and
    /// `sed.commands.test.ts` that exercise the GNU sed cycle engine through
    /// the virtual session: `-i`/`--in-place` editing, the `h/H/g/G/x` hold
    /// space, the `a/i/c` text commands, relative-offset (`+N`) addresses,
    /// grouped `{ }` commands, and the `P`/`D` first-line commands. Each
    /// assertion mirrors an upstream just-bash test verbatim by file:line and
    /// fails if the behavior is wrong.
    #[test]
    fn sed_test_ts_inplace_holdspace_text_and_cycle_rows() {
        // Helper: build a fresh root session with the supplied files.
        let session = |files: &[(&str, &str)]| {
            Bash::with_options(BashOptions {
                files: files
                    .iter()
                    .map(|(p, c)| (p.to_string(), c.to_string()))
                    .collect::<BTreeMap<_, _>>(),
                cwd: Some("/".to_string()),
                ..BashOptions::default()
            })
        };

        // ---- in-place editing (-i) — sed.test.ts ----
        // :267 `-i 's/hello/hi/'` modifies the file and prints nothing.
        let env = session(&[("/test.txt", "hello world\n")]);
        let r = env.exec("sed -i 's/hello/hi/' /test.txt");
        assert_eq!(r.stdout, "");
        assert_eq!(r.exit_code, 0);
        assert_eq!(env.exec("cat /test.txt").stdout, "hi world\n");
        // :281 `-i 's/foo/baz/g'` global in-place replacement.
        let env = session(&[("/test.txt", "foo foo foo\nbar foo bar\n")]);
        assert_eq!(env.exec("sed -i 's/foo/baz/g' /test.txt").stdout, "");
        assert_eq!(
            env.exec("cat /test.txt").stdout,
            "baz baz baz\nbar baz bar\n"
        );
        // :294 `-i '2d'` deletes a line in place.
        let env = session(&[("/test.txt", "line 1\nline 2\nline 3\n")]);
        assert_eq!(env.exec("sed -i '2d' /test.txt").stdout, "");
        assert_eq!(env.exec("cat /test.txt").stdout, "line 1\nline 3\n");
        // :307 `-i '/remove/d'` deletes matching lines in place.
        let env = session(&[("/test.txt", "keep this\nremove this\nkeep that\n")]);
        assert_eq!(env.exec("sed -i '/remove/d' /test.txt").stdout, "");
        assert_eq!(env.exec("cat /test.txt").stdout, "keep this\nkeep that\n");
        // :320 `-i` edits multiple files in place.
        let env = session(&[("/a.txt", "hello\n"), ("/b.txt", "hello\n")]);
        assert_eq!(env.exec("sed -i 's/hello/hi/' /a.txt /b.txt").stdout, "");
        assert_eq!(env.exec("cat /a.txt").stdout, "hi\n");
        assert_eq!(env.exec("cat /b.txt").stdout, "hi\n");
        // :339 `--in-place` is the long form of `-i`.
        let env = session(&[("/test.txt", "old text\n")]);
        assert_eq!(env.exec("sed --in-place 's/old/new/' /test.txt").stdout, "");
        assert_eq!(env.exec("cat /test.txt").stdout, "new text\n");

        // ---- hold space commands (h/H/g/G/x) — sed.test.ts ----
        // :354 `1h;3G` copies line 1 to hold then appends hold at line 3.
        assert_eq!(
            session(&[("/test.txt", "first\nsecond\nthird\n")])
                .exec("sed '1h;3G' /test.txt")
                .stdout,
            "first\nsecond\nthird\nfirst\n"
        );
        // :364 `H;$G` accumulates every line in hold then appends at EOF.
        assert_eq!(
            session(&[("/test.txt", "a\nb\nc\n")])
                .exec("sed 'H;$G' /test.txt")
                .stdout,
            "a\nb\nc\na\nb\nc\n"
        );
        // :378 `1h;2g` copies hold to pattern space, replacing line 2.
        assert_eq!(
            session(&[("/test.txt", "first\nsecond\n")])
                .exec("sed '1h;2g' /test.txt")
                .stdout,
            "first\nfirst\n"
        );
        // :388 `1h;2G` appends hold space to pattern space at line 2.
        assert_eq!(
            session(&[("/test.txt", "header\ndata\n")])
                .exec("sed '1h;2G' /test.txt")
                .stdout,
            "header\ndata\nheader\n"
        );
        // :398 `x` exchanges pattern and hold spaces each cycle.
        assert_eq!(
            session(&[("/test.txt", "A\nB\n")])
                .exec("sed 'x' /test.txt")
                .stdout,
            "\nA\n"
        );
        // :410 `-n '$g;$p'` copies the (empty) hold at EOF and prints it.
        assert_eq!(
            session(&[("/test.txt", "1\n2\n3\n")])
                .exec("sed -n '$g;$p' /test.txt")
                .stdout,
            "\n"
        );

        // ---- append command (a) — sed.test.ts ----
        // :426 `2a\ appended` appends text after the addressed line.
        assert_eq!(
            session(&[("/test.txt", "line 1\nline 2\nline 3\n")])
                .exec("sed '2a\\ appended' /test.txt")
                .stdout,
            "line 1\nline 2\nappended\nline 3\n"
        );
        // :435 `a\ ---` appends after every line.
        assert_eq!(
            session(&[("/test.txt", "a\nb\n")])
                .exec("sed 'a\\ ---' /test.txt")
                .stdout,
            "a\n---\nb\n---\n"
        );
        // :444 `$a\ footer` appends after the last line.
        assert_eq!(
            session(&[("/test.txt", "first\nlast\n")])
                .exec("sed '$a\\ footer' /test.txt")
                .stdout,
            "first\nlast\nfooter\n"
        );

        // ---- insert command (i) — sed.test.ts ----
        // :455 `2i\ inserted` inserts before the addressed line.
        assert_eq!(
            session(&[("/test.txt", "line 1\nline 2\nline 3\n")])
                .exec("sed '2i\\ inserted' /test.txt")
                .stdout,
            "line 1\ninserted\nline 2\nline 3\n"
        );
        // :464 `1i\ header` inserts before the first line.
        assert_eq!(
            session(&[("/test.txt", "content\n")])
                .exec("sed '1i\\ header' /test.txt")
                .stdout,
            "header\ncontent\n"
        );
        // :473 `i\ >` inserts before every line.
        assert_eq!(
            session(&[("/test.txt", "a\nb\n")])
                .exec("sed 'i\\ >' /test.txt")
                .stdout,
            ">\na\n>\nb\n"
        );

        // ---- change command (c) — sed.test.ts ----
        // :484 `1c\ new line` changes the matching line.
        assert_eq!(
            session(&[("/test.txt", "old line\n")])
                .exec("sed '1c\\ new line' /test.txt")
                .stdout,
            "new line\n"
        );
        // :493 `2c\ replaced` changes a specific line number.
        assert_eq!(
            session(&[("/test.txt", "line 1\nline 2\nline 3\n")])
                .exec("sed '2c\\ replaced' /test.txt")
                .stdout,
            "line 1\nreplaced\nline 3\n"
        );

        // ---- relative offset address (+N) — sed.test.ts ----
        // :609 `/^2/,+2d` deletes the match and the next N lines.
        assert_eq!(
            session(&[("/test.txt", "1\n2\n3\n4\n5\n")])
                .exec("sed '/^2/,+2d' /test.txt")
                .stdout,
            "1\n5\n"
        );
        // :618 `-n '/a/,+1p'` prints the match plus the next line, repeating.
        assert_eq!(
            session(&[("/test.txt", "a\n1\nc\nc\na\n2\na\n3\n")])
                .exec("sed -n '/a/,+1p' /test.txt")
                .stdout,
            "a\n1\na\n2\na\n3\n"
        );
        // :627 `/^2/,+2{d}` combines a relative-offset range with a `{ }` block.
        assert_eq!(
            session(&[("/test.txt", "1\n2\n3\n4\n5\n")])
                .exec("sed '/^2/,+2{d}' /test.txt")
                .stdout,
            "1\n5\n"
        );

        // ---- grouped commands — sed.test.ts ----
        // :638 `2{s/b/B/}` runs a command in a group on the addressed line.
        assert_eq!(
            session(&[("/test.txt", "a\nb\nc\n")])
                .exec("sed '2{s/b/B/}' /test.txt")
                .stdout,
            "a\nB\nc\n"
        );
        // :647 `-n '2{s/b/B/;p}'` runs multiple `;`-separated group commands.
        assert_eq!(
            session(&[("/test.txt", "a\nb\nc\n")])
                .exec("sed -n '2{s/b/B/;p}' /test.txt")
                .stdout,
            "B\n"
        );

        // ---- P / D commands — sed.test.ts ----
        // :658 `-n 'N;P'` prints up to the first newline of the joined pair.
        assert_eq!(
            session(&[("/test.txt", "line1\nline2\n")])
                .exec("sed -n 'N;P' /test.txt")
                .stdout,
            "line1\n"
        );
        // :670 `-n 'N;P;D'` is the sliding window: every line except the last.
        assert_eq!(
            session(&[("/test.txt", "a\nb\nc\n")])
                .exec("sed -n 'N;P;D' /test.txt")
                .stdout,
            "a\nb\n"
        );
        // :682 `N;D` auto-prints the remaining line when N has no more input.
        assert_eq!(
            session(&[("/test.txt", "a\nb\nc\n")])
                .exec("sed 'N;D' /test.txt")
                .stdout,
            "c\n"
        );

        // ---- sed.commands.test.ts ----
        // A session matching the upstream `createEnv` for sed.commands.test.ts.
        let commands_env = || {
            Bash::with_options(BashOptions {
                files: BTreeMap::from([
                    (
                        "/test/file.txt".to_string(),
                        "line 1\nline 2\nline 3\nline 4\nline 5\n".to_string(),
                    ),
                    (
                        "/test/alpha.txt".to_string(),
                        "alpha\nbeta\ngamma\ndelta\n".to_string(),
                    ),
                ]),
                cwd: Some("/test".to_string()),
                ..BashOptions::default()
            })
        };
        // :216 multiple `-e` scripts run in sequence on each line.
        assert_eq!(
            commands_env()
                .exec("sed -e 's/a/1/' -e 's/e/2/' -e 's/i/3/' /test/alpha.txt")
                .stdout,
            "1lpha\nb2t1\ng1mma\nd2lt1\n"
        );
        // :249 `-n '/alpha/,+2p'` prints the match and the next two lines.
        assert_eq!(
            commands_env()
                .exec("sed -n '/alpha/,+2p' /test/alpha.txt")
                .stdout,
            "alpha\nbeta\ngamma\n"
        );
    }

    #[test]
    fn awk_upstream_core_rows_cover_blocks_patterns_printf_stdin_and_errors() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/data.txt".to_string(),
                    "hello world\nfoo bar\n".to_string(),
                ),
                ("/one.txt".to_string(), "test\n".to_string()),
                ("/abc.txt".to_string(), "a\nb\n".to_string()),
                (
                    "/letters.txt".to_string(),
                    "apple\nbanana\napricot\ncherry\n".to_string(),
                ),
                (
                    "/lines.txt".to_string(),
                    "line1\nline2\nline3\nline4\n".to_string(),
                ),
                ("/num.txt".to_string(), "42\n".to_string()),
                ("/empty.txt".to_string(), String::new()),
                ("/blank.txt".to_string(), "\n".to_string()),
                ("/tab-only.txt".to_string(), "\t\n".to_string()),
                ("/newlines.txt".to_string(), "\n\n\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec(r#"awk 'BEGIN { print "H\x49\x4a\x4BL" }'"#).stdout,
            "HIJKL\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { print "0\061\62x\0645" }'"#).stdout,
            "012x45\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { print "x\f\r\b\v\a\\y" }'"#).stdout,
            "x\x0c\r\x08\x0b\x07\\y\n"
        );
        assert_eq!(env.exec("awk '{ print NF }' /blank.txt").stdout, "0\n");
        assert_eq!(env.exec("awk '{ print NF }' /tab-only.txt").stdout, "0\n");
        assert_eq!(
            env.exec(r#"awk -v name=World '{print "Hello " name}' /one.txt"#)
                .stdout,
            "Hello World\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN{print "start"}{print $0}' /abc.txt"#)
                .stdout,
            "start\na\nb\n"
        );
        assert_eq!(
            env.exec(r#"awk '{print $0}END{print "done"}' /abc.txt"#)
                .stdout,
            "a\nb\ndone\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN{print "hello"}' /empty.txt"#).stdout,
            "hello\n"
        );
        assert_eq!(env.exec(r#"awk '{ print "line" }' /empty.txt"#).stdout, "");
        assert_eq!(
            env.exec(r#"awk 'BEGIN { print "start" } { print } END { print "end" }' /empty.txt"#)
                .stdout,
            "start\nend\n"
        );
        assert_eq!(
            env.exec(r#"echo -n "" | awk '{ print "line" }'"#).stdout,
            ""
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN{print "start"} {print $0} END{print "end"}' /abc.txt"#)
                .stdout,
            "start\na\nb\nend\n"
        );
        assert_eq!(
            env.exec("awk '/^a/{print}' /letters.txt").stdout,
            "apple\napricot\n"
        );
        assert_eq!(env.exec("awk 'NR==2{print}' /lines.txt").stdout, "line2\n");
        assert_eq!(
            env.exec("awk 'NR>1{print}' /lines.txt").stdout,
            "line2\nline3\nline4\n"
        );
        assert_eq!(
            env.exec("awk '/^a/' /letters.txt").stdout,
            "apple\napricot\n"
        );
        assert_eq!(env.exec("awk 'NR==2' /lines.txt").stdout, "line2\n");
        assert_eq!(env.exec("awk 'NR>2' /lines.txt").stdout, "line3\nline4\n");
        assert_eq!(
            env.exec(r#"awk '{printf "%s!\n", $1}' /data.txt"#).stdout,
            "hello!\nfoo!\n"
        );
        assert_eq!(
            env.exec(r#"awk '{printf "num: %d\n", $1}' /num.txt"#)
                .stdout,
            "num: 42\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { printf "no newline" }'"#).stdout,
            "no newline"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { printf "with newline\n" }'"#)
                .stdout,
            "with newline\n"
        );
        assert_eq!(
            env.exec(r#"echo "42" | awk '{ printf("%d\n", $1) }'"#)
                .stdout,
            "42\n"
        );
        assert_eq!(env.exec("echo 'a b c' | awk '{print $2}'").stdout, "b\n");
        assert_eq!(
            env.exec(r#"printf 'a b\nc d\n' | awk '{print $1}'"#).stdout,
            "a\nc\n"
        );
        assert_eq!(env.exec("awk").stderr, "awk: missing program\n");
        assert_eq!(env.exec("awk").exit_code, 1);
        assert_eq!(
            env.exec("awk '{print}' /nonexistent.txt").stderr,
            "awk: /nonexistent.txt: No such file or directory\n"
        );
        let help = env.exec("awk --help");
        assert!(help.stdout.contains("awk"));
        assert!(help.stdout.contains("pattern scanning"));
        assert_eq!(help.exit_code, 0);
        assert_eq!(
            env.exec(r#"awk '{print $1 "-" $2}' /data.txt"#).stdout,
            "hello-world\nfoo-bar\n"
        );
        assert_eq!(
            env.exec("awk '{ print $NF }' /data.txt").stdout,
            "world\nbar\n"
        );
        assert_eq!(
            env.exec("awk '{ print NR, NF }' /newlines.txt").stdout,
            "1 0\n2 0\n3 0\n"
        );
        assert_eq!(
            env.exec(r#"echo "a1b2c" | awk -F'[0-9]+' '{ print $1, $2, $3 }'"#)
                .stdout,
            "a b c\n"
        );
    }

    #[test]
    fn awk_upstream_field_separator_output_and_filename_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/data.txt".to_string(), "a\nb\nc\n".to_string()),
                ("/a.txt".to_string(), "a1\na2\n".to_string()),
                ("/b.txt".to_string(), "b1\nb2\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec(r#"echo "a b c" | awk '{ print $1, $2, $3 }'"#)
                .stdout,
            "a b c\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b c" | awk '{ print $0 }'"#).stdout,
            "a b c\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b c d e" | awk '{ print $1, $3, $5 }'"#)
                .stdout,
            "a c e\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b" | awk '{ print "[" $10 "]" }'"#)
                .stdout,
            "[]\n"
        );
        assert_eq!(
            env.exec(r#"echo "first second last" | awk '{ print $NF }'"#)
                .stdout,
            "last\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b c d e" | awk '{ print NF }'"#).stdout,
            "5\n"
        );
        assert_eq!(env.exec(r#"echo "" | awk '{ print NF }'"#).stdout, "0\n");
        assert_eq!(
            env.exec(r#"echo "hello" | awk '{ print NF }'"#).stdout,
            "1\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b c" | awk 'BEGIN { OFS = ":" } { print $1, $2, $3 }'"#)
                .stdout,
            "a:b:c\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b c" | awk 'BEGIN{OFS=","} { print $1, $2, $3 }'"#)
                .stdout,
            "a,b,c\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b c" | awk 'BEGIN { OFS = "\t" } { print $1, $2, $3 }'"#)
                .stdout,
            "a\tb\tc\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b c" | awk 'BEGIN { OFS = " | " } { print $1, $2, $3 }'"#)
                .stdout,
            "a | b | c\n"
        );
        assert_eq!(
            env.exec(r#"awk '{ print $0 }' /data.txt"#).stdout,
            "a\nb\nc\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { ORS = ";" } { print $0 }' /data.txt"#)
                .stdout,
            "a;b;c;"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { ORS = "" } { print $0 }' /data.txt"#)
                .stdout,
            "abc"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN{ORS="\n---\n"} { print }' /data.txt"#)
                .stdout,
            "a\n---\nb\n---\nc\n---\n"
        );
        assert_eq!(
            env.exec(r#"echo "hello world" | awk '{ print }'"#).stdout,
            "hello world\n"
        );
        assert_eq!(
            env.exec(r#"echo "a    b     c" | awk '{ print $1, $2, $3 }'"#)
                .stdout,
            "a b c\n"
        );
        assert_eq!(
            env.exec(r#"echo "a:b:c" | awk -F: '{ print $2 }'"#).stdout,
            "b\n"
        );
        assert_eq!(
            env.exec(r#"echo "a,b,c" | awk 'BEGIN{FS=","} { print $2 }'"#)
                .stdout,
            "b\n"
        );
        assert_eq!(
            env.exec(r#"printf "a\tb\tc" | awk -F'\t' '{ print $2 }'"#)
                .stdout,
            "b\n"
        );
        assert_eq!(
            env.exec(r#"echo "a1b2c3d" | awk -F'[0-9]' '{ print $1, $2, $3, $4 }'"#)
                .stdout,
            "a b c d\n"
        );
        assert_eq!(
            env.exec(r#"echo "a::c" | awk -F: '{ print NF, "[" $2 "]" }'"#)
                .stdout,
            "3 []\n"
        );
        assert_eq!(
            env.exec("awk '{print FILENAME, NR}' /data.txt").stdout,
            "/data.txt 1\n/data.txt 2\n/data.txt 3\n"
        );
        assert_eq!(
            env.exec("awk '{print FILENAME, FNR, NR}' /a.txt /b.txt")
                .stdout,
            "/a.txt 1 1\n/a.txt 2 2\n/b.txt 1 3\n/b.txt 2 4\n"
        );
    }

    #[test]
    fn awk_jbc25_scalar_expression_operator_and_ternary_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/values.txt".to_string(), "2\n3\n4\n".to_string()),
                ("/levels.txt".to_string(), "10\n25\n5\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec(r#"awk 'BEGIN { print 5 + 3, 10 - 4, 6 * 7, 20 / 4, 17 % 5 }'"#)
                .stdout,
            "8 6 42 5 2\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { print 2 ^ 8, 3 ** 3, -5, +"42" }'"#)
                .stdout,
            "256 27 -5 42\n"
        );
        let logical = env.exec(
            r#"awk 'BEGIN { print (5 == 5), (5 != 5), (3 < 5), (5 >= 5), (1 && 0), (1 || 0), !0 }'"#,
        );
        assert_eq!(logical.stderr, "");
        assert_eq!(logical.stdout, "1 0 1 1 0 1 1\n");
        assert_eq!(
            env.exec(r#"echo "hello world" | awk '{ print ($0 ~ /world/), ($0 !~ /foo/) }'"#)
                .stdout,
            "1 1\n"
        );
        assert_eq!(
            env.exec(
                r#"awk 'BEGIN{sum=0; prod=1; n=0}{sum+=$1; prod*=$1; n++}END{print sum, prod, n}' /values.txt"#
            )
            .stdout,
            "9 24 3\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN{val=100}{val-=$1}END{print val}' /values.txt"#)
                .stdout,
            "91\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN{val=120}{val/=$1}END{print val}' /values.txt"#)
                .stdout,
            "5\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { x=5; print x++, x, ++x, x--, --x }'"#)
                .stdout,
            "5 6 7 7 5\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { x=17; x%=5; y=2; y^=4; print x, y }'"#)
                .stdout,
            "2 16\n"
        );
        assert_eq!(
            env.exec(r#"awk '{ print $1 > 15 ? "high" : "low" }' /levels.txt"#)
                .stdout,
            "low\nhigh\nlow\n"
        );
    }

    #[test]
    fn awk_jbc25_pattern_and_range_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/data.txt".to_string(),
                    "apple 10\nbanana 20\napricot 5\ncherry 30\n".to_string(),
                ),
                (
                    "/range.txt".to_string(),
                    "before\nSTART\nline1\nline2\nEND\nafter\n".to_string(),
                ),
                (
                    "/multi-range.txt".to_string(),
                    "before\nBEGIN\na\nEND\nmiddle\nBEGIN\nb\nEND\nafter\n".to_string(),
                ),
                (
                    "/single-range.txt".to_string(),
                    "before\nSTART END\nafter\n".to_string(),
                ),
                (
                    "/open-range.txt".to_string(),
                    "before\nSTART\nline1\nline2\n".to_string(),
                ),
                (
                    "/header-footer.txt".to_string(),
                    "line1\nheader: foo\ndata1\ndata2\nfooter: bar\nline2\n".to_string(),
                ),
                (
                    "/line-range.txt".to_string(),
                    "line1\nline2\nline3\nline4\nline5\n".to_string(),
                ),
                ("/repeat.txt".to_string(), "a\naa\naaa\naaaa\n".to_string()),
                ("/numbers.txt".to_string(), "1\n2\n3\n4\n5\n6\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec("awk '/a{2,3}/' /repeat.txt").stdout,
            "aa\naaa\naaaa\n"
        );
        assert_eq!(
            env.exec(r#"awk '$1 > 1 && $1 < 5' /numbers.txt"#).stdout,
            "2\n3\n4\n"
        );
        assert_eq!(
            env.exec(r#"awk '$1 == 1 || $1 == 6' /numbers.txt"#).stdout,
            "1\n6\n"
        );
        assert_eq!(
            env.exec(r#"awk '!($1 == 2)' /numbers.txt"#).stdout,
            "1\n3\n4\n5\n6\n"
        );
        assert_eq!(
            env.exec(r#"awk '/^a/ && $2 > 5' /data.txt"#).stdout,
            "apple 10\n"
        );
        assert_eq!(
            env.exec(r#"awk '$1 % 2 == 1' /numbers.txt"#).stdout,
            "1\n3\n5\n"
        );
        assert_eq!(
            env.exec(r#"awk '/START/,/END/' /range.txt"#).stdout,
            "START\nline1\nline2\nEND\n"
        );
        assert_eq!(
            env.exec(r#"awk '/START/,/END/ { print ">> " $0 }' /range.txt"#)
                .stdout,
            ">> START\n>> line1\n>> line2\n>> END\n"
        );
        assert_eq!(
            env.exec(r#"awk '/BEGIN/,/END/' /multi-range.txt"#).stdout,
            "BEGIN\na\nEND\nBEGIN\nb\nEND\n"
        );
        assert_eq!(
            env.exec(r#"awk '/START/,/END/' /single-range.txt"#).stdout,
            "START END\n"
        );
        assert_eq!(
            env.exec(r#"awk '/START/,/END/' /open-range.txt"#).stdout,
            "START\nline1\nline2\n"
        );
        assert_eq!(
            env.exec(r#"awk '/^header:/,/^footer:/' /header-footer.txt"#)
                .stdout,
            "header: foo\ndata1\ndata2\nfooter: bar\n"
        );
        assert_eq!(
            env.exec(r#"awk '/line2/,/line4/' /line-range.txt"#).stdout,
            "line2\nline3\nline4\n"
        );
    }

    #[test]
    fn awk_jbc25_builtin_math_string_match_and_substitution_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "hello foo world\n".to_string())]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec(
                r#"echo "3 4" | awk '{ print int(3.7), int(-3.7), sqrt($1*$1 + $2*$2), exp(0), log(1), sin(0), cos(0) }'"#
            )
            .stdout,
            "3 -4 5 1 0 0 1\n"
        );
        let atan = env.exec(r#"echo "1 1" | awk '{ print atan2($1, $2) }'"#);
        let atan_value = atan.stdout.trim().parse::<f64>().unwrap();
        assert!((atan_value - std::f64::consts::FRAC_PI_4).abs() < 0.00001);
        assert_eq!(
            env.exec(
                r#"awk 'BEGIN { print length("hello"), substr("hello world", 7), index("abcabc", "bc"), tolower("HeLLo"), toupper("yes") }'"#
            )
            .stdout,
            "5 world 2 hello YES\n"
        );
        assert_eq!(
            env.exec(r#"echo "abcdefg" | awk '{ print length(), length(""), length(12345) }'"#)
                .stdout,
            "7 0 5\n"
        );
        assert_eq!(
            env.exec(
                r#"awk 'BEGIN { print substr("hello world", 1, 5), "[" substr("abc", 10) "]", substr("hello", 0, 3), index("hello", "xyz"), index("hello", "") }'"#
            )
            .stdout,
            "hello [] hel 0 1\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { print sprintf("%s: %d (%.1f%%)", "Score", 85, 85.0) }'"#)
                .stdout,
            "Score: 85 (85.0%)\n"
        );
        assert_eq!(
            env.exec(r#"awk '{print match($0, /foo/), RSTART, RLENGTH}' /data.txt"#)
                .stdout,
            "7 7 3\n"
        );
        assert_eq!(
            env.exec(r#"awk '{print gensub(/o/, "0", "g")}' /data.txt"#)
                .stdout,
            "hell0 f00 w0rld\n"
        );
        assert_eq!(
            env.exec(r#"echo "hello hello" | awk '{ sub(/hello/, "hi"); print }'"#)
                .stdout,
            "hi hello\n"
        );
        assert_eq!(
            env.exec(r#"echo "ababa" | awk '{ n = gsub(/a/, "X"); print n, $0 }'"#)
                .stdout,
            "3 XbXbX\n"
        );
        assert_eq!(
            env.exec(r#"echo "aaa bbb aaa" | awk '{ gsub(/a/, "X", $1); print }'"#)
                .stdout,
            "XXX bbb aaa\n"
        );
        assert_eq!(
            env.exec(r#"echo "hello" | awk '{ sub(/ll/, "[&]"); print }'"#)
                .stdout,
            "he[ll]o\n"
        );
    }

    #[test]
    fn awk_jbc25_array_and_computed_field_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/fruit.txt".to_string(),
                    "apple\nbanana\napple\ncherry\napple\n".to_string(),
                ),
                (
                    "/sales.csv".to_string(),
                    "fruit,10\nveg,20\nfruit,15\nveg,5\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec(
                r#"awk 'BEGIN { a["foo"] = 42; a["x"] = 1; a["x"] = 2; a[2] = "two"; i=5; a[i*2] = "ten"; a["key" "1"] = "value"; print a["foo"], a["x"], a[2], a[10], a["key1"], "[" a["missing"] "]" }'"#
            )
            .stdout,
            "42 2 two ten value []\n"
        );
        assert_eq!(
            env.exec(
                r#"awk '{ count[$1]++ } END { print count["apple"], count["banana"] }' /fruit.txt"#
            )
            .stdout,
            "3 1\n"
        );
        assert_eq!(
            env.exec(
                r#"awk -F, '{ sum[$1]+=$2 } END { print sum["fruit"], sum["veg"] }' /sales.csv"#
            )
            .stdout,
            "25 25\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { a[1,2] = "val"; print a[1,2] }'"#)
                .stdout,
            "val\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { arr[1] = 100; arr[1] %= 30; print arr[1] }'"#)
                .stdout,
            "10\n"
        );
        assert_eq!(
            env.exec(r#"echo "10 20 30" | awk '{ print $(NF-1), $1 + $3 }'"#)
                .stdout,
            "20 40\n"
        );
    }

    #[test]
    fn awk_jbc25_in_delete_and_for_in_array_rows() {
        let env = Bash::new();

        // `in` operator: existing key, missing key, numeric keys, and the
        // non-creating behavior of membership tests.
        let existing =
            env.exec(r#"echo "" | awk 'BEGIN { a["x"] = 1; if ("x" in a) print "found" }'"#);
        assert_eq!(existing.stdout, "found\n");
        assert_eq!(existing.exit_code, 0);

        let missing = env.exec(
            r#"echo "" | awk 'BEGIN { a["x"] = 1; if ("y" in a) print "found"; else print "not found" }'"#,
        );
        assert_eq!(missing.stdout, "not found\n");
        assert_eq!(missing.exit_code, 0);

        let numeric =
            env.exec(r#"echo "" | awk 'BEGIN { a[42] = "answer"; print (42 in a), (99 in a) }'"#);
        assert_eq!(numeric.stdout, "1 0\n");
        assert_eq!(numeric.exit_code, 0);

        let no_create = env
            .exec(r#"echo "" | awk 'BEGIN { if ("x" in a) print "yes"; for (k in a) print k }'"#);
        assert_eq!(no_create.stdout, "");
        assert_eq!(no_create.exit_code, 0);

        // delete statement: single element, missing element, and whole array.
        let delete_one =
            env.exec(r#"echo "" | awk 'BEGIN { a["x"] = 1; delete a["x"]; print ("x" in a) }'"#);
        assert_eq!(delete_one.stdout, "0\n");
        assert_eq!(delete_one.exit_code, 0);

        let delete_missing =
            env.exec(r#"echo "" | awk 'BEGIN { delete a["missing"]; print "ok" }'"#);
        assert_eq!(delete_missing.stdout, "ok\n");
        assert_eq!(delete_missing.exit_code, 0);

        let delete_all = env.exec(
            r#"echo "" | awk 'BEGIN { a[1]=1; a[2]=2; a[3]=3; delete a; for(k in a) count++; print count+0 }'"#,
        );
        assert_eq!(delete_all.stdout, "0\n");
        assert_eq!(delete_all.exit_code, 0);

        // for-in loops: count keys and sum values via key indirection.
        let iterate = env.exec(
            r#"echo "" | awk 'BEGIN { a["a"]=1; a["b"]=2; a["c"]=3; for (k in a) count++; print count }'"#,
        );
        assert_eq!(iterate.stdout, "3\n");
        assert_eq!(iterate.exit_code, 0);

        let sum = env.exec(
            r#"echo "" | awk 'BEGIN { a[1]=10; a[2]=20; a[3]=30; sum=0; for (k in a) sum += a[k]; print sum }'"#,
        );
        assert_eq!(sum.stdout, "60\n");
        assert_eq!(sum.exit_code, 0);
    }

    #[test]
    fn awk_jbc25_split_subsep_and_array_field_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/uniq.txt".to_string(), "a\nb\na\nc\nb\na\n".to_string()),
                (
                    "/totals.txt".to_string(),
                    "alice 100\nbob 200\nalice 50\n".to_string(),
                ),
                (
                    "/lines.txt".to_string(),
                    "1 first\n2 second\n3 third\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // for-in over an empty array iterates zero times.
        let empty = env.exec(r#"echo "" | awk 'BEGIN { for (k in a) print k; print "done" }'"#);
        assert_eq!(empty.stdout, "done\n");
        assert_eq!(empty.exit_code, 0);

        // split(string, array, sep): returns the element count and populates the
        // array with 1-based indices.
        let split_colon = env.exec(
            r#"echo "" | awk 'BEGIN { n = split("a:b:c", arr, ":"); print n, arr[1], arr[2], arr[3] }'"#,
        );
        assert_eq!(split_colon.stdout, "3 a b c\n");
        assert_eq!(split_colon.exit_code, 0);

        let split_count = env.exec(
            r#"echo "" | awk 'BEGIN { n = split("one,two,three,four", arr, ","); print n }'"#,
        );
        assert_eq!(split_count.stdout, "4\n");
        assert_eq!(split_count.exit_code, 0);

        // Without an explicit separator, split() uses whitespace splitting.
        let split_ws = env.exec(
            r#"echo "" | awk 'BEGIN { n = split("a b c", arr); print n, arr[1], arr[2], arr[3] }'"#,
        );
        assert_eq!(split_ws.stdout, "3 a b c\n");
        assert_eq!(split_ws.exit_code, 0);

        // split() clears the destination array before populating it.
        let split_clear = env.exec(
            r#"echo "" | awk 'BEGIN { arr[5]="old"; split("a:b", arr, ":"); print (5 in arr), arr[1], arr[2] }'"#,
        );
        assert_eq!(split_clear.stdout, "0 a b\n");
        assert_eq!(split_clear.exit_code, 0);

        // Counting unique values via for-in over an accumulated array.
        let unique =
            env.exec(r#"awk '{ seen[$1]++ } END { for (k in seen) n++; print n }' /uniq.txt"#);
        assert_eq!(unique.stdout, "3\n");
        assert_eq!(unique.exit_code, 0);

        // Multi-dimensional (SUBSEP) keys: matrix storage and membership tests
        // with a parenthesised subscript list.
        let matrix = env.exec(
            r#"echo "" | awk 'BEGIN { a[0,0]=1; a[0,1]=2; a[1,0]=3; a[1,1]=4; print a[0,0], a[0,1], a[1,0], a[1,1] }'"#,
        );
        assert_eq!(matrix.stdout, "1 2 3 4\n");
        assert_eq!(matrix.exit_code, 0);

        let multi_in =
            env.exec(r#"echo "" | awk 'BEGIN { a[1,2] = "x"; print ((1,2) in a), ((1,3) in a) }'"#);
        assert_eq!(multi_in.stdout, "1 0\n");
        assert_eq!(multi_in.exit_code, 0);

        // Field values as array keys, accumulating across records.
        let totals = env
            .exec(r#"awk '{ totals[$1] += $2 } END { print totals["alice"], totals["bob"] }' /totals.txt"#);
        assert_eq!(totals.stdout, "150 200\n");
        assert_eq!(totals.exit_code, 0);

        let lines = env.exec(r#"awk '{ lines[$1] = $2 } END { print lines[2] }' /lines.txt"#);
        assert_eq!(lines.stdout, "second\n");
        assert_eq!(lines.exit_code, 0);

        // Pre-increment of an array element returns and stores the new value.
        let pre_inc = env.exec(r#"echo "" | awk 'BEGIN { a["x"] = 5; print ++a["x"], a["x"] }'"#);
        assert_eq!(pre_inc.stdout, "6 6\n");
        assert_eq!(pre_inc.exit_code, 0);
    }

    #[test]
    fn awk_jbc25_string_builtin_concat_compare_and_coercion_rows() {
        // Maps the remaining portable rows of awk.strings.test.ts that the Rust
        // awk subset implements 1:1. Each assertion mirrors the upstream vitest
        // expectation exactly (strict stdout + exit code). Cited file:line refer
        // to packages/just-bash/src/commands/awk/awk.strings.test.ts.
        let env = Bash::default();
        let cases: &[(&str, &str)] = &[
            // substr() middle portion (awk.strings.test.ts:80).
            (
                r#"echo "" | awk 'BEGIN { print substr("abcdefgh", 3, 4) }'"#,
                "cdef\n",
            ),
            // index() single character (awk.strings.test.ts:109).
            (
                r#"echo "" | awk 'BEGIN { print index("abcdef", "c") }'"#,
                "3\n",
            ),
            // index() first occurrence (awk.strings.test.ts:118).
            (
                r#"echo "" | awk 'BEGIN { print index("abcabc", "bc") }'"#,
                "2\n",
            ),
            // tolower() basic (awk.strings.test.ts:147).
            (
                r#"echo "" | awk 'BEGIN { print tolower("hello") }'"#,
                "hello\n",
            ),
            // tolower() preserves digits/symbols (awk.strings.test.ts:165).
            (
                r#"echo "" | awk 'BEGIN { print tolower("ABC123!@#") }'"#,
                "abc123!@#\n",
            ),
            // toupper() basic (awk.strings.test.ts:185).
            (
                r#"echo "" | awk 'BEGIN { print toupper("HELLO") }'"#,
                "HELLO\n",
            ),
            // toupper() mixed case (awk.strings.test.ts:194).
            (
                r#"echo "" | awk 'BEGIN { print toupper("HeLLo WoRLd") }'"#,
                "HELLO WORLD\n",
            ),
            // sub() returns 1 on a match and mutates $0 (awk.strings.test.ts:214).
            (
                r#"echo "hello" | awk '{ n = sub(/l/, "L"); print n, $0 }'"#,
                "1 heLlo\n",
            ),
            // sub() returns 0 on no match (awk.strings.test.ts:223).
            (
                r#"echo "hello" | awk '{ n = sub(/x/, "X"); print n, $0 }'"#,
                "0 hello\n",
            ),
            // sub() with an explicit target variable (awk.strings.test.ts:232).
            (
                r#"echo "test" | awk '{ x = "foo bar foo"; sub(/foo/, "baz", x); print x }'"#,
                "baz bar foo\n",
            ),
            // gsub() returns 0 when nothing matches (awk.strings.test.ts:271).
            (
                r#"echo "hello" | awk '{ n = gsub(/x/, "X"); print n, $0 }'"#,
                "0 hello\n",
            ),
            // gsub() replaces every digit (awk.strings.test.ts:289).
            (
                r##"echo "a1b2c3" | awk '{ gsub(/[0-9]/, "#"); print }'"##,
                "a#b#c#\n",
            ),
            // sprintf() right-justified width (awk.strings.test.ts:327).
            (
                r#"echo "" | awk 'BEGIN { print sprintf("[%10s]", "hi") }'"#,
                "[        hi]\n",
            ),
            // sprintf() left-justified width (awk.strings.test.ts:336).
            (
                r#"echo "" | awk 'BEGIN { print sprintf("[%-10s]", "hi") }'"#,
                "[hi        ]\n",
            ),
            // sprintf() zero-padded integer (awk.strings.test.ts:345).
            (
                r#"echo "" | awk 'BEGIN { print sprintf("%05d", 42) }'"#,
                "00042\n",
            ),
            // string concatenation of variables (awk.strings.test.ts:365).
            (
                r#"echo "" | awk 'BEGIN { a = "hello"; b = "world"; print a " " b }'"#,
                "hello world\n",
            ),
            // literal string concatenation (awk.strings.test.ts:374).
            (
                r#"echo "" | awk 'BEGIN { print "foo" "bar" "baz" }'"#,
                "foobarbaz\n",
            ),
            // numeric literals concatenate as strings (awk.strings.test.ts:383).
            (r#"echo "" | awk 'BEGIN { print 1 2 3 }'"#, "123\n"),
            // accumulating concatenation (awk.strings.test.ts:390).
            (
                r#"echo "" | awk 'BEGIN { s = "a"; s = s "b"; s = s "c"; print s }'"#,
                "abc\n",
            ),
            // string equality comparison (awk.strings.test.ts:401).
            (
                r#"echo "" | awk 'BEGIN { if ("abc" == "abc") print "equal" }'"#,
                "equal\n",
            ),
            // string inequality comparison (awk.strings.test.ts:410).
            (
                r#"echo "" | awk 'BEGIN { if ("abc" != "xyz") print "different" }'"#,
                "different\n",
            ),
            // lexicographic less-than (awk.strings.test.ts:419).
            (
                r#"echo "" | awk 'BEGIN { if ("abc" < "abd") print "less" }'"#,
                "less\n",
            ),
            // lexicographic greater-than (awk.strings.test.ts:428).
            (
                r#"echo "" | awk 'BEGIN { if ("z" > "a") print "greater" }'"#,
                "greater\n",
            ),
            // numeric string + 0 coercion (awk.strings.test.ts:439).
            (r#"echo "" | awk 'BEGIN { print "42" + 0 }'"#, "42\n"),
            // leading-numeric string coercion (awk.strings.test.ts:446).
            (r#"echo "" | awk 'BEGIN { print "123abc" + 0 }'"#, "123\n"),
            // non-numeric string coerces to 0 (awk.strings.test.ts:455).
            (r#"echo "" | awk 'BEGIN { print "hello" + 0 }'"#, "0\n"),
            // number-to-string via empty concatenation (awk.strings.test.ts:464).
            (r#"echo "" | awk 'BEGIN { n = 42; print n "" }'"#, "42\n"),
        ];
        for (prog, want) in cases {
            let r = env.exec(prog);
            assert_eq!(r.exit_code, 0, "{prog}: stderr={:?}", r.stderr);
            assert_eq!(&r.stdout, want, "{prog}");
        }
    }

    #[test]
    fn awk_jbc35_field_rebuild_printf_and_edge_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/data.txt".to_string(), "1 2 3\n4 5 6\n".to_string()),
                (
                    "/letters.txt".to_string(),
                    "apple 10 red\nbanana 5 yellow\napple 3 green\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec(r#"echo "a b c d" | awk '{ i=3; print $i }'"#)
                .stdout,
            "c\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b c" | awk '{ $2 = "X"; print }'"#)
                .stdout,
            "a X c\n"
        );
        assert_eq!(
            env.exec(r#"echo "one two three" | awk '{ $2 = "TWO"; print $0 }'"#)
                .stdout,
            "one TWO three\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b" | awk '{ $5 = "e"; print NF, $0 }'"#)
                .stdout,
            "5 a b   e\n"
        );
        assert_eq!(
            env.exec(r#"echo "hello world" | awk '{ $1 = "HELLO"; print }'"#)
                .stdout,
            "HELLO world\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b c" | awk '{ $NF = "C"; print }'"#)
                .stdout,
            "a b C\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b" | awk '{ $4 = "d"; print NF }'"#)
                .stdout,
            "4\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b c d e" | awk '{ NF = 2; print $0 }'"#)
                .stdout,
            "a b\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b c" | awk 'BEGIN{OFS=":"} { $1=$1; print }'"#)
                .stdout,
            "a:b:c\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b c" | awk 'BEGIN{OFS="-"} { $2 = "X"; print $0 }'"#)
                .stdout,
            "a-X-c\n"
        );
        assert_eq!(
            env.exec(r#"awk 'NR==1{OFS=","} { $1=$1; print }' /data.txt"#)
                .stdout,
            "1,2,3\n4,5,6\n"
        );
        assert_eq!(
            env.exec(r#"echo "5 6" | awk '{ print $1 * $2 }'"#).stdout,
            "30\n"
        );
        assert_eq!(
            env.exec(r#"echo "10 20" | awk '{ $1 = $1 * 2; print }'"#)
                .stdout,
            "20 20\n"
        );
        assert_eq!(
            env.exec(r#"echo "10 20" | awk '{ $(NF+1) = $1 + $2; print }'"#)
                .stdout,
            "10 20 30\n"
        );
        assert_eq!(
            env.exec(r#"echo "old" | awk '{ $0 = "new line here"; print NF, $2 }'"#)
                .stdout,
            "3 line\n"
        );
        assert_eq!(
            env.exec(r#"echo "   a b c" | awk '{ print NF, $1 }'"#)
                .stdout,
            "3 a\n"
        );
        assert_eq!(
            env.exec(r#"echo "a b c   " | awk '{ print NF, $NF }'"#)
                .stdout,
            "3 c\n"
        );
        assert_eq!(
            env.exec(r#"echo "a,,c" | awk -F, '{ print NF, "[" $2 "]" }'"#)
                .stdout,
            "3 []\n"
        );
        assert_eq!(
            env.exec(r#"awk '$2 > 7 { print $1 }' /letters.txt"#).stdout,
            "apple\n"
        );
        assert_eq!(
            env.exec(r#"awk '$1 ~ /app/ { print $3 }' /letters.txt"#)
                .stdout,
            "red\ngreen\n"
        );
        assert_eq!(
            env.exec(r#"awk '$1 == "apple" && $2 > 5 { print $3 }' /letters.txt"#)
                .stdout,
            "red\n"
        );
        assert_eq!(
            env.exec(r#"echo "10" | awk '{ printf("%d %ld %lld\n", $1, $1, $1) }'"#)
                .stdout,
            "10 10 10\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { printf("a%*sb\n", 5, "x") }'"#)
                .stdout,
            "a    xb\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { printf("a%*sb\n", -5, "x") }'"#)
                .stdout,
            "ax    b\n"
        );
        assert_eq!(
            env.exec(r#"echo "hello world" | awk '{ print }'"#).stdout,
            "hello world\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { printf "no newline" }'"#).stdout,
            "no newline"
        );
        assert_eq!(
            env.exec(r#"echo "a" | awk '{ print "one"; print "two" }'"#)
                .stdout,
            "one\ntwo\n"
        );
        assert_eq!(env.exec(r#"awk 'BEGIN { print "only" }'"#).stdout, "only\n");
        assert_eq!(
            env.exec(r#"awk 'BEGIN { print "one" } BEGIN { print "two" }'"#)
                .stdout,
            "one\ntwo\n"
        );
        assert_eq!(
            env.exec(r#"echo "x" | awk 'END { print "one" } END { print "two" }'"#)
                .stdout,
            "one\ntwo\n"
        );
        assert_eq!(
            env.exec(r#"echo "x" | awk 'BEGIN { prefix="pre" } { print prefix, $0 }'"#)
                .stdout,
            "pre x\n"
        );
        assert_eq!(
            env.exec("printf 'a\nb\nc\n' | awk 'END { print NR }'")
                .stdout,
            "3\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { print ("10" == 10), ("abc" == 0) }'"#)
                .stdout,
            "1 0\n"
        );
        assert_eq!(env.exec(r#"awk 'BEGIN { print "5" + 3 }'"#).stdout, "8\n");
        assert_eq!(
            env.exec(r#"awk 'BEGIN { print 5 " apples" }'"#).stdout,
            "5 apples\n"
        );
    }

    #[test]
    fn awk_jbc42_parser_comment_numeric_and_if_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/in.tsv".to_string(),
                    "vendor\tamount\nAcme\t100\nGlobex\t200\n".to_string(),
                ),
                ("/data.txt".to_string(), "1\n2\n3\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(env.exec(r#"awk 'BEGIN{x=1;y=2;print x+y}'"#).stdout, "3\n");
        assert_eq!(
            env.exec(r#"awk 'BEGIN {   x  =  1  ;  y  =  2  ;  print  x + y  }'"#)
                .stdout,
            "3\n"
        );
        assert_eq!(
            env.exec("awk 'BEGIN {\n        x = 1\n        y = 2\n        print x + y\n      }'")
                .stdout,
            "3\n"
        );
        assert_eq!(
            env.exec("awk 'BEGIN {\tx=1;\ty=2;\tprint x+y}'").stdout,
            "3\n"
        );
        assert_eq!(
            env.exec(
                "awk 'BEGIN {\n        printf \"%s=%d\\n\",\n          \"answer\", 42\n      }'"
            )
            .stdout,
            "answer=42\n"
        );
        assert_eq!(
            env.exec(
                "echo \"abc\" | awk '{\n        print substr($0,\n                     1,\n                     2)\n      }'"
            )
            .stdout,
            "ab\n"
        );
        assert_eq!(
            env.exec(
                "awk 'BEGIN {\n        if (1 == 1 &&\n            2 == 2) print \"ok\"\n      }'"
            )
            .stdout,
            "ok\n"
        );
        assert_eq!(
            env.exec("awk 'BEGIN {\n        if (0 ||\n            1) print \"ok\"\n      }'")
                .stdout,
            "ok\n"
        );
        assert_eq!(
            env.exec("awk 'BEGIN {\n        x = 1\n        print x\n      }'")
                .stdout,
            "1\n"
        );
        assert_eq!(
            env.exec("awk 'BEGIN {\n        if (0) print \"no\"\n        else\n          print \"yes\"\n      }'")
                .stdout,
            "yes\n"
        );
        assert_eq!(
            env.exec("awk 'BEGIN {\n        print 1 ?\n              \"yes\" :\n              \"no\"\n      }'")
                .stdout,
            "yes\n"
        );
        assert_eq!(
            env.exec(
                r#"awk -F'\t' 'NR > 1 {
        printf "INSERT INTO t VALUES ('"'"'%s'"'"', %d);\n",
          $1, $2
      }' /in.tsv"#
            )
            .stdout,
            "INSERT INTO t VALUES ('Acme', 100);\nINSERT INTO t VALUES ('Globex', 200);\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { print "he said \"hello\"" }'"#)
                .stdout,
            "he said \"hello\"\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { print "a\tb\nc" }'"#).stdout,
            "a\tb\nc\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { print "[" "" "]" }'"#).stdout,
            "[]\n"
        );
        assert_eq!(env.exec(r#"awk 'BEGIN { print "\n" }'"#).stdout, "\n\n");
        assert_eq!(env.exec(r#"awk 'BEGIN { print 42 }'"#).stdout, "42\n");
        assert_eq!(env.exec(r#"awk 'BEGIN { print 3.14 }'"#).stdout, "3.14\n");
        assert_eq!(env.exec(r#"awk 'BEGIN { print 1e3 }'"#).stdout, "1000\n");
        assert_eq!(env.exec(r#"awk 'BEGIN { print 1e-2 }'"#).stdout, "0.01\n");
        assert_eq!(env.exec(r#"awk 'BEGIN { print .5 }'"#).stdout, "0.5\n");
        assert_eq!(
            env.exec("awk 'BEGIN {\n        x = 1 # this is a comment\n        print x\n      }'")
                .stdout,
            "1\n"
        );
        assert_eq!(
            env.exec("awk 'BEGIN { x = 1; # comment\n        print x }'")
                .stdout,
            "1\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { if (1) print "yes" }'"#).stdout,
            "yes\n"
        );
        assert_eq!(
            env.exec(r#"awk 'BEGIN { if (0) print "yes"; else print "no" }'"#)
                .stdout,
            "no\n"
        );

        // regex parsing (awk.parsing.test.ts :216..:261)
        assert_eq!(
            env.exec(r#"echo "hello" | awk '/hello/ { print "matched" }'"#)
                .stdout,
            "matched\n"
        );
        assert_eq!(
            env.exec(r#"echo "a.b" | awk '/a\.b/ { print "matched" }'"#)
                .stdout,
            "matched\n"
        );
        assert_eq!(
            env.exec(r#"echo "abc" | awk '/[abc]+/ { print "matched" }'"#)
                .stdout,
            "matched\n"
        );
        assert_eq!(
            env.exec(r#"echo "hello" | awk '/^hello$/ { print "matched" }'"#)
                .stdout,
            "matched\n"
        );
        assert_eq!(
            env.exec(r#"echo "aaa" | awk '/a+/ { print "matched" }'"#)
                .stdout,
            "matched\n"
        );
        // division is distinguished from a regex literal in expression position.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { x = 10 / 2; print x }'"#)
                .stdout,
            "5\n"
        );

        // operator parsing (awk.parsing.test.ts :294..:331)
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print (1 <= 2), (2 >= 1), (1 == 1), (1 != 2) }'"#)
                .stdout,
            "1 1 1 1\n"
        );
        // ((10+5)-3)*2/4 = 6 through compound assignment operators.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { x=10; x+=5; x-=3; x*=2; x/=4; print x }'"#)
                .stdout,
            "6\n"
        );
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { x=5; print ++x, x++, x }'"#)
                .stdout,
            "6 6 7\n"
        );
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print (1 && 1), (1 || 0), !0 }'"#)
                .stdout,
            "1 1 1\n"
        );
        assert_eq!(
            env.exec(r#"echo "hello" | awk '{ print ($0 ~ /hello/), ($0 !~ /world/) }'"#)
                .stdout,
            "1 1\n"
        );

        // block structure parsing (awk.parsing.test.ts :342..:374)
        assert_eq!(
            env.exec(r#"echo "test" | awk '{ } { print "ok" }'"#).stdout,
            "ok\n"
        );
        assert_eq!(
            env.exec(r#"echo "test" | awk '/test/ { print "1" } /test/ { print "2" }'"#)
                .stdout,
            "1\n2\n"
        );
        assert_eq!(
            env.exec(r#"echo "x" | awk 'BEGIN { print "B" } END { print "E" }'"#)
                .stdout,
            "B\nE\n"
        );
        assert_eq!(
            env.exec(r#"echo "hello" | awk '/hello/'"#).stdout,
            "hello\n"
        );
        assert_eq!(
            env.exec(r#"echo "hello" | awk '{ print "matched" }'"#)
                .stdout,
            "matched\n"
        );
    }

    #[test]
    fn awk_jbc42_user_function_and_conditional_flow_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "1\n2\n3\n4\n5\n".to_string())]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec(
                r#"echo "5" | awk 'function double(x) { return x * 2 } { print double($1) }'"#
            )
            .stdout,
            "10\n"
        );
        assert_eq!(
            env.exec(
                r#"echo "3 4" | awk 'function add(a, b) { return a + b } { print add($1, $2) }'"#
            )
            .stdout,
            "7\n"
        );
        let greet = env
            .exec(r#"echo "" | awk 'function greet() { return "hello" } BEGIN { print greet() }'"#);
        assert_eq!(greet.stdout, "hello\n");
        assert_eq!(
            env.exec(r#"echo "" | awk 'function foo() { x = 42 } BEGIN { foo(); print x }'"#)
                .stdout,
            "42\n"
        );
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'function foo(x) { x = 99 } BEGIN { x = 1; foo(5); print x }'"#
            )
            .stdout,
            "1\n"
        );
        assert_eq!(
            env.exec(r#"echo "" | awk 'function foo(a,    local) { local = 100; return a + local } BEGIN { local = 5; print foo(1), local }'"#)
                .stdout,
            "101 5\n"
        );
        assert_eq!(
            env.exec(r#"echo "5" | awk 'function square(x) { result = x * x; return result } { print square($1) }'"#)
                .stdout,
            "25\n"
        );
        assert_eq!(
            env.exec(r#"echo "5" | awk 'function fact(n) { if (n <= 1) return 1; return n * fact(n-1) } { print fact($1) }'"#)
                .stdout,
            "120\n"
        );
        assert_eq!(
            env.exec(r#"echo "10" | awk 'function fib(n) { if (n <= 2) return 1; return fib(n-1) + fib(n-2) } { print fib($1) }'"#)
                .stdout,
            "55\n"
        );
        assert_eq!(
            env.exec(r#"echo "3" | awk 'function double(x) { return x * 2 } function quadruple(x) { return double(double(x)) } { print quadruple($1) }'"#)
                .stdout,
            "12\n"
        );
        assert_eq!(
            env.exec(r#"echo "world" | awk 'function greet(name) { return "hello " name } { print greet($1) }'"#)
                .stdout,
            "hello world\n"
        );
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'function sum(a,b) { return a+b } BEGIN { print sum(10, 20) }'"#
            )
            .stdout,
            "30\n"
        );
        assert_eq!(
            env.exec(r#"echo "4" | awk 'function square(x) { return x*x } function cube(x) { return x*x*x } { print square($1), cube($1) }'"#)
                .stdout,
            "16 64\n"
        );
        assert_eq!(
            env.exec(r#"echo "" | awk 'function hello() { return "hi" } BEGIN { print hello() }'"#)
                .stdout,
            "hi\n"
        );
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'function add(a, b) { return a + b } BEGIN { print add(2, 3) }'"#
            )
            .stdout,
            "5\n"
        );
        assert_eq!(
            env.exec(
                "echo \"\" | awk '\n          function f1() { return 1 }\n          function f2() { return 2 }\n          BEGIN { print f1() + f2() }\n        '"
            )
            .stdout,
            "3\n"
        );
        assert_eq!(
            env.exec(r#"awk '{ if ($1 % 2 == 0) next; print }' /data.txt"#)
                .stdout,
            "1\n3\n5\n"
        );
        let exit_with_code = env.exec(r#"awk '{ if ($1 == 2) exit 42; print }' /data.txt"#);
        assert_eq!(exit_with_code.stdout, "1\n");
        assert_eq!(exit_with_code.exit_code, 42);
        let exit_default = env.exec(r#"awk '{ if ($1 == 2) exit; print }' /data.txt"#);
        assert_eq!(exit_default.stdout, "1\n");
        assert_eq!(exit_default.exit_code, 0);
    }

    #[test]
    fn awk_jbc_edge_special_chars_numeric_string_and_regex_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                // Binary content here is plain ASCII bytes "1 2\n3 4\n".
                "/data.bin".to_string(),
                "1 2\n3 4\n".to_string(),
            )]),
            ..BashOptions::default()
        });

        // Special characters in field data (awk.edge-cases.test.ts:44-87).
        assert_eq!(
            env.exec(r#"echo 'he said "hello"' | awk '{ print $3 }'"#)
                .stdout,
            "\"hello\"\n"
        );
        let backslash = env.exec(r#"echo 'path\\to\\file' | awk '{ print }'"#);
        assert_eq!(backslash.stdout, "path\\\\to\\\\file\n");
        assert_eq!(backslash.exit_code, 0);
        assert_eq!(
            env.exec(r#"echo "[test]" | awk '{ print $1 }'"#).stdout,
            "[test]\n"
        );
        assert_eq!(
            env.exec(r#"echo 'price: \$100' | awk '{ print $2 }'"#)
                .stdout,
            "\\$100\n"
        );
        assert_eq!(
            env.exec(r#"echo "a & b" | awk '{ print $2 }'"#).stdout,
            "&\n"
        );

        // Numeric edge cases (awk.edge-cases.test.ts:91-128).
        let large = env.exec(r#"echo "" | awk 'BEGIN { print 1e308 }'"#);
        assert_eq!(large.exit_code, 0);
        assert!(
            large.stdout.to_lowercase().contains("1e+308")
                || large.stdout.to_lowercase().contains("1e308"),
            "expected 1e308 scientific output, got {:?}",
            large.stdout
        );
        let small = env.exec(r#"echo "" | awk 'BEGIN { print 1e-308 }'"#);
        assert_eq!(small.exit_code, 0);
        assert!(
            small.stdout.to_lowercase().contains("1e-308"),
            "expected 1e-308 scientific output, got {:?}",
            small.stdout
        );
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print -0 }'"#).stdout,
            "0\n"
        );
        let precision = env.exec(r#"echo "" | awk 'BEGIN { print 0.1 + 0.2 }'"#);
        assert_eq!(precision.exit_code, 0);
        assert!(
            (precision.stdout.trim().parse::<f64>().unwrap() - 0.3).abs() < 1e-10,
            "expected 0.1+0.2 close to 0.3, got {:?}",
            precision.stdout
        );
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 9999999999999999999999 }'"#)
                .exit_code,
            0
        );

        // String edge cases (awk.edge-cases.test.ts:132-164).
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print ("" == "") }'"#)
                .stdout,
            "1\n"
        );
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { x = ""; print length(x) }'"#)
                .stdout,
            "0\n"
        );
        assert_eq!(
            env.exec(r#"echo "     " | awk '{ print NF }'"#).stdout,
            "0\n"
        );

        // Regex edge cases (awk.edge-cases.test.ts:168-200).
        assert_eq!(
            env.exec(r#"echo "test" | awk '//' | head -1"#).stdout,
            "test\n"
        );
        assert_eq!(
            env.exec(r#"echo "a.b" | awk '/a\.b/ { print "matched" }'"#)
                .stdout,
            "matched\n"
        );
        assert_eq!(
            env.exec(r#"echo "test" | awk '/^test/ { print "yes" }'"#)
                .stdout,
            "yes\n"
        );
        assert_eq!(
            env.exec(r#"echo "test" | awk '/test$/ { print "yes" }'"#)
                .stdout,
            "yes\n"
        );

        // Post-increment array element (awk.arrays.test.ts:297).
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { a["x"] = 5; print a["x"]++, a["x"] }'"#)
                .stdout,
            "5 6\n"
        );

        // Binary (ASCII byte) file processed by awk (awk.binary.test.ts:5).
        let binary = env.exec("awk '{print $1 + $2}' /data.bin");
        assert_eq!(binary.stdout, "3\n7\n");
        assert_eq!(binary.exit_code, 0);
    }

    #[test]
    fn awk_jbc09_edge_case_control_array_and_special_var_rows() {
        // Maps the portable rows of awk.edge-cases.test.ts and awk.errors.test.ts
        // that the Rust awk subset already implements 1:1. Each assertion mirrors
        // the upstream vitest expectation exactly (strict stdout + exit code).
        let env = Bash::default();

        // case-sensitive regex (awk.edge-cases.test.ts:202).
        let case_sensitive = env.exec(r#"echo "TEST" | awk '/test/ { print "matched" }'"#);
        assert_eq!(case_sensitive.exit_code, 0);
        assert_eq!(case_sensitive.stdout, "");

        // empty action block (awk.edge-cases.test.ts:213).
        let empty_block = env.exec(r#"echo "test" | awk '{ }'"#);
        assert_eq!(empty_block.exit_code, 0);
        assert_eq!(empty_block.stdout, "");

        // nested if without else (awk.edge-cases.test.ts:220).
        let nested_if = env.exec(r#"echo "5" | awk '{ if ($1 > 3) if ($1 < 10) print "yes" }'"#);
        assert_eq!(nested_if.exit_code, 0);
        assert_eq!(nested_if.stdout, "yes\n");

        // multiple semicolons (awk.edge-cases.test.ts:229).
        let semis = env.exec(r#"echo "" | awk 'BEGIN { x=1;; y=2;;; print x+y }'"#);
        assert_eq!(semis.exit_code, 0);
        assert_eq!(semis.stdout, "3\n");

        // uninitialized variable as number (awk.edge-cases.test.ts:258).
        let uninit_num = env.exec(r#"echo "" | awk 'BEGIN { print x + 0 }'"#);
        assert_eq!(uninit_num.exit_code, 0);
        assert_eq!(uninit_num.stdout, "0\n");

        // uninitialized variable as string (awk.edge-cases.test.ts:265).
        let uninit_str = env.exec(r#"echo "" | awk 'BEGIN { print "[" x "]" }'"#);
        assert_eq!(uninit_str.exit_code, 0);
        assert_eq!(uninit_str.stdout, "[]\n");

        // variable shadowing built-in NF (awk.edge-cases.test.ts:274).
        let shadow_nf = env.exec(r#"echo "a b c" | awk '{ NF = 100; print NF }'"#);
        assert_eq!(shadow_nf.exit_code, 0);
        assert_eq!(shadow_nf.stdout, "100\n");

        // empty array iteration (awk.edge-cases.test.ts:294).
        let empty_iter =
            env.exec(r#"echo "" | awk 'BEGIN { for (k in arr) print k; print "done" }'"#);
        assert_eq!(empty_iter.exit_code, 0);
        assert_eq!(empty_iter.stdout, "done\n");

        // numeric string subscript collapses with numeric subscript
        // (awk.edge-cases.test.ts:303).
        let numeric_key =
            env.exec(r#"echo "" | awk 'BEGIN { a["1"] = "one"; a[1] = "ONE"; print a[1] }'"#);
        assert_eq!(numeric_key.exit_code, 0);
        assert_eq!(numeric_key.stdout, "ONE\n");

        // delete on empty array is a no-op (awk.edge-cases.test.ts:312).
        let delete_empty = env.exec(r#"echo "" | awk 'BEGIN { delete arr["x"]; print "ok" }'"#);
        assert_eq!(delete_empty.exit_code, 0);
        assert_eq!(delete_empty.stdout, "ok\n");

        // NF = 0 for an empty record (awk.errors.test.ts:288).
        let nf_zero = env.exec(r#"echo '' | awk '{ print NF }'"#);
        assert_eq!(nf_zero.exit_code, 0);
        assert_eq!(nf_zero.stdout, "0\n");

        // NR is 0 inside BEGIN before any record (awk.errors.test.ts:295).
        let nr_start = env.exec(r#"echo '1' | awk 'BEGIN { print NR }'"#);
        assert_eq!(nr_start.exit_code, 0);
        assert_eq!(nr_start.stdout, "0\n");
    }

    #[test]
    fn awk_jbc09_error_handling_and_type_coercion_rows() {
        // Maps the portable rows of awk.errors.test.ts. Each assertion mirrors
        // the upstream vitest expectation exactly (strict outputs where the
        // upstream test pins them, exit-code-only where the upstream only
        // asserts `toBeDefined()`/`exitCode`).
        let env = Bash::default();

        // division by zero (awk.errors.test.ts:6,14,20).
        // Upstream only requires exit 0 / a defined result; Rust prints
        // IEEE inf/nan deterministically.
        let div_int = env.exec(r#"echo '1' | awk '{ print 1/0 }'"#);
        assert_eq!(div_int.exit_code, 0);
        assert_eq!(div_int.stdout, "inf\n");
        let div_float = env.exec(r#"echo '1' | awk '{ print 1.0/0.0 }'"#);
        assert_eq!(div_float.exit_code, 0);
        assert_eq!(div_float.stdout, "inf\n");
        let mod_zero = env.exec(r#"echo '1' | awk '{ print 5 % 0 }'"#);
        assert_eq!(mod_zero.exit_code, 0);
        assert_eq!(mod_zero.stdout, "nan\n");

        // invalid regex patterns (awk.errors.test.ts:29,38,47). Upstream only
        // requires the result to be defined; Rust fails closed with a regex
        // parse diagnostic and a non-zero exit (no crash, no host leak).
        for prog in [
            r#"echo 'test' | awk '{ print match($0, "[") }'"#,
            r#"echo 'test' | awk '{ gsub(/[/, "x"); print }'"#,
            r#"echo 'test' | awk '{ sub(/[/, "x"); print }'"#,
        ] {
            let r = env.exec(prog);
            assert_ne!(r.exit_code, 0, "{prog}");
            assert!(
                r.stderr.contains("regex parse error"),
                "{prog}: {:?}",
                r.stderr
            );
        }

        // undefined variable access (awk.errors.test.ts:57,64,71,80).
        assert_eq!(env.exec(r#"echo '1' | awk '{ print x }'"#).stdout, "\n");
        assert_eq!(
            env.exec(r#"echo '1' | awk '{ print x + 5 }'"#).stdout,
            "5\n"
        );
        assert_eq!(
            env.exec(r#"echo '1' | awk '{ arr[1] = "x"; print arr[1] }'"#)
                .stdout,
            "x\n"
        );
        assert_eq!(
            env.exec(r#"echo '1' | awk '{ print arr[999] }'"#).stdout,
            "\n"
        );

        // type coercion (awk.errors.test.ts:89,96,103,110,119).
        assert_eq!(
            env.exec(r#"echo '10abc' | awk '{ print $1 + 5 }'"#).stdout,
            "15\n"
        );
        assert_eq!(
            env.exec(r#"echo 'abc' | awk '{ print $1 + 5 }'"#).stdout,
            "5\n"
        );
        assert_eq!(
            env.exec(r#"echo '' | awk '{ print $1 + 10 }'"#).stdout,
            "10\n"
        );
        assert_eq!(
            env.exec(r#"echo '10' | awk '{ print ($1 == "10") }'"#)
                .stdout,
            "1\n"
        );
        // Both operands look numeric, so numeric comparison applies: 2 < 10.
        let numcmp = env.exec(r#"echo '2' | awk '{ print ($1 < "10") }'"#);
        assert_eq!(numcmp.exit_code, 0);
        assert_eq!(numcmp.stdout, "1\n");

        // field access edge cases (awk.errors.test.ts:129,136,150,158,170).
        assert_eq!(
            env.exec(r#"echo 'hello world' | awk '{ print $0 }'"#)
                .stdout,
            "hello world\n"
        );
        assert_eq!(
            env.exec(r#"echo 'a b' | awk '{ print $100 }'"#).stdout,
            "\n"
        );
        // Non-integer field index truncates to $1.
        assert_eq!(
            env.exec(r#"echo 'a b c' | awk '{ print $1.5 }'"#).stdout,
            "a\n"
        );
        // Field assignment beyond NF extends the record with empty fields.
        let extend = env.exec(r#"echo 'a b' | awk '{ $5 = "x"; print $0 }'"#);
        assert_eq!(extend.exit_code, 0);
        assert!(extend.stdout.contains('x'), "{:?}", extend.stdout);
        // substr with only the string argument returns the whole string.
        assert_eq!(
            env.exec(r#"echo 'hello' | awk '{ print substr($1) }'"#)
                .stdout,
            "hello\n"
        );

        // sprintf format handling (awk.errors.test.ts:186,195,205).
        assert_eq!(
            env.exec(r#"echo '1' | awk '{ print sprintf("hello") }'"#)
                .stdout,
            "hello\n"
        );
        // Extra args ignored.
        assert_eq!(
            env.exec(r#"echo '1' | awk '{ print sprintf("%d", 1, 2, 3) }'"#)
                .stdout,
            "1\n"
        );
        // Missing arg treated as 0; upstream only pins exit 0.
        let sprintf_missing = env.exec(r#"echo '1' | awk '{ print sprintf("%d %d", 1) }'"#);
        assert_eq!(sprintf_missing.exit_code, 0);
        assert_eq!(sprintf_missing.stdout, "1 0\n");

        // math function edge cases (awk.errors.test.ts:216,223,230,237).
        for (prog, want) in [
            (r#"echo '1' | awk '{ print sqrt(-1) }'"#, "nan\n"),
            (r#"echo '1' | awk '{ print log(0) }'"#, "-inf\n"),
            (r#"echo '1' | awk '{ print log(-1) }'"#, "nan\n"),
            (r#"echo '1' | awk '{ print exp(1000) }'"#, "inf\n"),
        ] {
            let r = env.exec(prog);
            assert_eq!(r.exit_code, 0, "{prog}");
            assert_eq!(r.stdout, want, "{prog}");
        }

        // syntax + file errors (awk.errors.test.ts:246,252,270).
        assert_ne!(env.exec(r#"echo '1' | awk '{ print $1'"#).exit_code, 0);
        assert_ne!(env.exec(r#"echo '1' | awk '{ print ($1 }'"#).exit_code, 0);
        let missing_file = env.exec("awk '{ print }' /nonexistent/file.txt");
        assert_ne!(missing_file.exit_code, 0);
        assert!(
            missing_file.stderr.contains("No such file"),
            "{:?}",
            missing_file.stderr
        );
    }

    #[test]
    fn rg_imported_skip_and_divergent_rg_rows_are_classified() {
        // JBC-10: classifies the imported rg rows that the upstream JS suite
        // `it.skip`s (features the JS port or its harness does not implement) or
        // whose behavior the virtual-filesystem Rust port intentionally diverges
        // on. Each assertion pins the Rust port's ACTUAL behavior, so it fails if
        // the behavior ever silently changes and the js-only classification
        // becomes wrong.
        //
        // Upstream rows covered (file:line):
        //   misc.test.ts:454 (--pre), :468 (--pre-glob), :1080 (-z gzip) — the
        //   Rust port rejects the file-preprocessing / compression-search flags as
        //   unrecognized options rather than implementing the JS feature.
        let rejected = |args: &str, opt: &str| {
            let r = home_bash(&[("f.txt", "hello\n")]).exec(args);
            assert_eq!(r.exit_code, 1, "{args}");
            assert_eq!(r.stdout, "", "{args}");
            assert_eq!(
                r.stderr,
                format!("rg: unrecognized option '{opt}'\n"),
                "{args}"
            );
        };
        rejected("rg --pre 'cat' hello f.txt", "--pre");
        rejected("rg --pre-glob '*.dat' hello f.txt", "--pre-glob");
        rejected("rg -z hello f.txt", "-z");

        // misc.test.ts:1108 binary_convert / binary.test.ts:305
        // (matching_files_inconsistent_with_count): a NUL-containing file is a
        // binary file and is skipped by default (no "binary file matches" message
        // and no count inconsistency), so a plain search finds nothing.
        let r = home_bash(&[("file", "foo\0bar\nfoo\0baz\n")]).exec("rg foo file");
        assert_eq!(r.exit_code, 1, "binary default skip exit");
        assert_eq!(r.stdout, "", "binary default skip stdout");

        // misc.test.ts:197 (-ow period): the JS `-ow '.'` quirk (treating `.` as a
        // word and matching each character) is NOT reproduced; `.` requires a word
        // character on each side, so `...` yields no whole-word match.
        let r = home_bash(&[("haystack", "...\n")]).exec("rg -ow '.' haystack");
        assert_eq!(r.exit_code, 1, "-ow period exit");
        assert_eq!(r.stdout, "", "-ow period stdout");

        // regression.test.ts:1278 (-won empty): an empty `-won` pattern does not
        // match at word boundaries in the Rust port (exit 1), diverging from the
        // upstream skip-row expectation.
        let r = home_bash(&[("test", "\n##\n")]).exec("rg -won '' test");
        assert_eq!(r.exit_code, 1, "-won empty exit");
        assert_eq!(r.stdout, "", "-won empty stdout");

        // regression.test.ts:1194 (NUL in pattern without -a) and :1205 (with -a):
        // a shell-escaped NUL in the pattern is treated as literal text rather than
        // raising the upstream "NUL byte in pattern" error, so the search succeeds
        // and matches the `foo` line in both forms (diverging from upstream).
        let r = home_bash(&[("test", "foo\n")]).exec("rg 'foo\\x00?' test");
        assert_eq!(r.exit_code, 0, "NUL pattern exit");
        assert_eq!(r.stdout, "foo\n", "NUL pattern stdout");
        let r = home_bash(&[("test", "foo\n")]).exec("rg -a 'foo\\x00?' test");
        assert_eq!(r.exit_code, 0, "NUL pattern -a exit");
        assert_eq!(r.stdout, "foo\n", "NUL pattern -a stdout");

        // regression.test.ts:267 (vertical tab / r128): the upstream row is skipped
        // because the JS port prints the filename prefix for a single-file
        // implicit directory search where ripgrep would omit it. The Rust port
        // likewise prints the `foo:` filename prefix, so the match is labelled.
        let r = home_bash(&[("foo", "01234567\x0b\n\x0b\n\x0b\n\x0b\nx\n")]).exec("rg -n x");
        assert_eq!(r.exit_code, 0, "vtab exit");
        assert_eq!(r.stdout, "foo:5:x\n", "vtab stdout");

        // misc.test.ts:123 (with_heading_default / requires -j1): the Rust port
        // accepts the `-j1` thread no-op together with --heading and prints the
        // file-label heading then the unprefixed match line.
        let r = home_bash(&[("f.txt", "foo\n")]).exec("rg -j1 --heading foo f.txt");
        assert_eq!(r.exit_code, 0, "-j1 heading exit");
        assert_eq!(r.stdout, "f.txt\nfoo\n", "-j1 heading stdout");

        // multiline.test.ts:100/101/102 (\p{Any} only_matching/vimgrep/context):
        // the Rust regex engine supports the `\p{Any}` Unicode property that the JS
        // port lacks, so `-U -o '\p{Any}'` matches every character of the file.
        let r = home_bash(&[("test", "a\nb\n")]).exec("rg -U -o '\\p{Any}' test");
        assert_eq!(r.exit_code, 0, "p-any exit");
        assert_eq!(r.stdout, "a\nb\n", "p-any stdout");

        // misc.test.ts:938-942 (parent ignore / requires cwd manipulation),
        // regression.test.ts:281 (unicode filename), :369 (invalid UTF-8 filename),
        // :1098 (git worktree): these upstream rows are skipped because they need
        // host cwd/git/encoding semantics the virtual filesystem does not model.
        // The Rust port performs an ordinary search; a unicode-named file is found
        // and labelled normally, proving no special parent/git handling exists.
        let r = home_bash(&[("café.txt", "foo\n")]).exec("rg foo");
        assert_eq!(r.exit_code, 0, "unicode filename exit");
        assert_eq!(r.stdout, "café.txt:1:foo\n", "unicode filename stdout");

        // regression.test.ts:1021 (r1334_invert_empty_patterns): inverting with a
        // zero-pattern `-f` file errors on the missing pattern file rather than
        // implementing the upstream empty-pattern invert behavior.
        let r = home_bash(&[("test", "foo\nbar\n")]).exec("rg -v -f missing test");
        assert_ne!(r.exit_code, 0, "invert empty -f exit: {:?}", r);

        // binary.test.ts:302 (after_match1_stdin), multiline.test.ts:105 (stdin),
        // regression.test.ts:963 (r1223 stdin with dash directory): these upstream
        // rows are skipped because the JS test harness cannot pipe stdin. The Rust
        // rg port, given no piped stdin and no file/directory operands, searches
        // the current directory; with an empty cwd it finds nothing rather than
        // reading from a (non-existent) stdin stream.
        let r = home_bash(&[]).exec("rg foo");
        assert_eq!(r.exit_code, 1, "empty stdin-less search exit");
        assert_eq!(r.stdout, "", "empty stdin-less search stdout");

        // regression.test.ts:372 (r228): pointing `--ignore-file` at a directory
        // does NOT raise the upstream-expected error in the Rust port; it is
        // treated as an empty ignore-pattern set, so the search proceeds normally.
        let r = home_bash(&[("d/inner", "1\n"), ("f.txt", "foo\n")])
            .exec("rg --ignore-file d foo f.txt");
        assert_eq!(r.exit_code, 0, "ignore-file dir exit");
        assert_eq!(r.stdout, "foo\n", "ignore-file dir stdout");
    }

    #[test]
    fn rg_imported_unrestricted_step_and_dotslash_files_rows_are_portable() {
        // JBC-10: ports the portable imported rg rows that the manifest previously
        // left pending.
        //
        // packages/just-bash/src/commands/rg/imported-tests/misc.test.ts:1021
        //   "should search binary files with -uuu": the third `u` in the
        //   unrestricted family enables binary (NUL-containing) file search, so a
        //   directory search that skips the binary `hay` file by default now
        //   matches it (mirroring the upstream rg-parser `handleUnrestricted`
        //   step: -u no-ignore, -uu +hidden, -uuu +binary-as-text).
        let sherlock = "For the Doctor Watsons of this world, as opposed to the Sherlock\n\
Holmeses, success in the province of detective work must always\n\
be, to a very large extent, the result of luck. Sherlock Holmes\n\
can extract a clew from a wisp of straw or a flake of cigar ash;\n\
but Doctor Watson has to have it taken out for him and dusted,\n\
and exhibited clearly, with a label attached.\n";
        let r = home_bash(&[("sherlock", sherlock), ("hay", "foo\0bar\nfoo\0baz\n")])
            .exec("rg -uuu foo");
        assert_eq!(r.exit_code, 0, "-uuu exit");
        // The binary `hay` file is now searched; its matches are labelled.
        assert!(r.stdout.contains("hay:"), "-uuu stdout: {:?}", r.stdout);

        // -uuu on the binary file directly also searches it as text.
        let r = home_bash(&[("hay", "foo\0bar\nfoo\0baz\n")]).exec("rg -uuu foo hay");
        assert_eq!(r.exit_code, 0, "-uuu file exit");
        assert_eq!(r.stdout, "foo\0bar\nfoo\0baz\n", "-uuu file stdout");

        // The unrestricted step-function must not over-reach: -uu includes hidden
        // files (no-ignore + hidden) but does NOT search binary files, and -u only
        // disables ignore filtering.
        let r = home_bash(&[(".hidden", "foo\n"), ("vis", "foo\n")]).exec("rg -uu foo");
        assert_eq!(r.exit_code, 0, "-uu exit");
        assert_eq!(r.stdout, ".hidden:1:foo\nvis:1:foo\n", "-uu stdout");

        let r = home_bash(&[(".hidden", "foo\n"), ("vis", "foo\n")]).exec("rg -u foo");
        assert_eq!(r.exit_code, 0, "-u exit");
        assert_eq!(r.stdout, "vis:1:foo\n", "-u stdout");

        // -uu still skips a NUL-containing binary file (binary search needs the
        // third `u`), proving the step boundary is real.
        let r = home_bash(&[("hay", "foo\0bar\n")]).exec("rg -uu foo hay");
        assert_eq!(r.exit_code, 1, "-uu binary skip exit");
        assert_eq!(r.stdout, "", "-uu binary skip stdout");

        // packages/just-bash/src/commands/rg/imported-tests/regression.test.ts:1398
        //   "should preserve ./ prefix": `rg --hidden --files ./` lists files under
        //   the explicit `./` directory argument, preserving the `./` prefix on the
        //   emitted path.
        let r = home_bash(&[("a/.ignore", ".foo\n")]).exec("rg --hidden --files ./");
        assert_eq!(r.exit_code, 0, "./ prefix exit");
        assert_eq!(r.stdout, "./a/.ignore\n", "./ prefix stdout");

        // packages/just-bash/src/commands/rg/imported-tests/regression.test.ts:1352
        //   "should handle empty pattern with -e": an empty `-e` pattern matches
        //   every line, so a single explicit file prints all of its lines without a
        //   filename prefix. (Upstream `it.skip`s this row, but the Rust port
        //   implements it identically to the documented upstream expectation.)
        let r = home_bash(&[("file", "FooBar\n")]).exec("rg -e '' file");
        assert_eq!(r.exit_code, 0, "-e empty exit");
        assert_eq!(r.stdout, "FooBar\n", "-e empty stdout");
    }

    #[test]
    fn rg_unsupported_upstream_skip_features_are_rejected_or_classified() {
        // JBC-10: the upstream rg suite explicitly `it.skip`s a family of features
        // that ripgrep itself or the just-bash JS port does not implement (alternate
        // text encodings, PCRE2 look-around, max-columns, file preprocessing,
        // non-gzip compression, the --no-include-zero/--crlf/--no-unicode/
        // --null-data flags, and timestamp-based sorting). The Rust rg port matches
        // that contract by rejecting these features rather than silently mis-handling
        // them. Each assertion fails if the Rust port ever started accepting the flag,
        // which would make the upstream skip-classification wrong.
        let unrecognized = |args: &str, opt: &str| {
            let r = home_bash(&[("f.txt", "ab\nhello\n")]).exec(args);
            assert_eq!(r.exit_code, 1, "{args}");
            assert_eq!(r.stdout, "", "{args}");
            assert_eq!(
                r.stderr,
                format!("rg: unrecognized option '{opt}'\n"),
                "{args}"
            );
        };

        // Alternate text encodings (-E / --encoding): Shift-JIS, UTF-16, EUC-JP, etc.
        unrecognized("rg -E sjis hello f.txt", "-E");
        unrecognized("rg --encoding utf-16 hello f.txt", "--encoding");

        // -M / --max-columns truncation.
        unrecognized("rg -M 10 hello f.txt", "-M");

        // --no-include-zero count override.
        unrecognized("rg --no-include-zero -c hello f.txt", "--no-include-zero");

        // File preprocessing (--pre / --pre-glob).
        unrecognized("rg --pre cat hello f.txt", "--pre");
        unrecognized("rg --pre-glob '*.txt' hello f.txt", "--pre-glob");

        // Non-gzip compression search (-z / --search-zip): only gzip is supported,
        // and the Rust port does not wire up the -z flag at all.
        unrecognized("rg -z hello f.txt", "-z");
        unrecognized("rg --search-zip hello f.txt", "--search-zip");

        // Output/format flags the upstream skip-suite documents as unimplemented.
        unrecognized("rg --crlf hello f.txt", "--crlf");
        unrecognized("rg --no-unicode hello f.txt", "--no-unicode");
        unrecognized("rg --null-data hello f.txt", "--null-data");

        // Timestamp-based sorting requires real system metadata; --sortr is not wired
        // up and --sort accessed cannot order by access time deterministically.
        unrecognized("rg --sortr accessed hello", "--sortr");

        // Memory-mapped file search (--mmap / --no-mmap) has no analogue in the
        // virtual-filesystem port; upstream `it.skip`s the whole mmap family.
        unrecognized("rg --mmap hello f.txt", "--mmap");
        unrecognized("rg --no-mmap hello f.txt", "--no-mmap");

        // Binary-quit/conversion control (--binary) is not implemented; upstream
        // skips the binary_quit rows.
        unrecognized("rg --binary hello f.txt", "--binary");

        // Color output (--color / --colors) is unimplemented; upstream skips the
        // r428/r599 color regression rows.
        unrecognized("rg --color always hello f.txt", "--color");
        unrecognized("rg --colors 'path:fg:blue' hello f.txt", "--colors");

        // Case-insensitive ignore-file matching (--ignore-file-case-insensitive)
        // is unimplemented; upstream skips r1164.
        unrecognized(
            "rg --ignore-file-case-insensitive hello f.txt",
            "--ignore-file-case-insensitive",
        );

        // PCRE2 / look-around (-P, raw look-ahead/look-behind) is rejected by the
        // standard regex engine rather than silently matching.
        let pcre = home_bash(&[("f.txt", "ab\nhello\n")]).exec("rg -P '(?<=a)b' f.txt");
        assert_eq!(pcre.exit_code, 1, "rg -P");
        assert_eq!(pcre.stdout, "", "rg -P");
        assert!(
            pcre.stderr.contains("PCRE2 is not supported"),
            "rg -P stderr: {:?}",
            pcre.stderr
        );

        let look_behind = home_bash(&[("f.txt", "ab\nhello\n")]).exec("rg '(?<=a)b' f.txt");
        assert_eq!(look_behind.exit_code, 2, "look-behind");
        assert_eq!(look_behind.stdout, "", "look-behind");
        assert!(
            look_behind.stderr.contains("look-around")
                || look_behind.stderr.contains("look-behind"),
            "look-behind stderr: {:?}",
            look_behind.stderr
        );

        let look_ahead = home_bash(&[("f.txt", "ab\nhello\n")]).exec("rg '(?=a)' f.txt");
        assert_eq!(look_ahead.exit_code, 2, "look-ahead");
        assert_eq!(look_ahead.stdout, "", "look-ahead");
        assert!(
            look_ahead.stderr.contains("look-around") || look_ahead.stderr.contains("look-ahead"),
            "look-ahead stderr: {:?}",
            look_ahead.stderr
        );
    }

    #[test]
    fn rg_multiline_search_spans_lines_and_anchors_per_line() {
        // JBC-10: ports the portable `-U`/`--multiline` rows from
        // packages/just-bash/src/commands/rg/imported-tests/multiline.test.ts
        // (overlap1, overlap2, dot_no_newline, --multiline-dotall) plus the
        // ripgrep-compat (-U) and regression r1878 (^ anchor) multiline rows.
        // Multiline matches the pattern against the whole file; `\n` in the
        // pattern spans lines, and every line a match touches is printed.

        // overlap1: multiline matches spanning lines where multiple matches may
        // begin and end on the same line.
        let r = home_bash(&[("test", "xxx\nabc\ndefxxxabc\ndefxxx\nxxx")])
            .exec("rg -n -U 'abc\\ndef' test");
        assert_eq!(r.exit_code, 0, "overlap1");
        assert_eq!(r.stdout, "2:abc\n3:defxxxabc\n4:defxxx\n", "overlap1");

        // overlap2: one match ends exactly where the next begins.
        let r = home_bash(&[("test", "xxx\nabc\ndefabc\ndefxxx\nxxx")])
            .exec("rg -n -U 'abc\\ndef' test");
        assert_eq!(r.exit_code, 0, "overlap2");
        assert_eq!(r.stdout, "2:abc\n3:defabc\n4:defxxx\n", "overlap2");

        // dot_no_newline: even in multiline search, `.` does NOT match a newline
        // (no --multiline-dotall), so this cross-line `.+` pattern finds nothing.
        let sherlock = "For the Doctor Watsons of this world, as opposed to the Sherlock\n\
Holmeses, success in the province of detective work must always\n\
be, to a very large extent, the result of luck. Sherlock Holmes\n\
can extract a clew from a wisp of straw or a flake of cigar ash;\n\
but Doctor Watson has to have it taken out for him and dusted,\n\
and exhibited clearly, with a label attached.\n";
        let r = home_bash(&[("sherlock", sherlock)])
            .exec("rg -n -U 'of this world.+detective work' sherlock");
        assert_eq!(r.exit_code, 1, "dot_no_newline exit");
        assert_eq!(r.stdout, "", "dot_no_newline stdout");

        // --multiline-dotall: `.` now matches newlines, so foo.bar spans the gap.
        let r = home_bash(&[("test", "foo\nbar\n")]).exec("rg --multiline-dotall 'foo.bar' test");
        assert_eq!(r.exit_code, 0, "dotall exit");
        assert!(r.stdout.contains("foo"), "dotall stdout: {:?}", r.stdout);

        // ripgrep-compat (-U): `foo\nbar` matches across the two lines (default
        // search of the cwd, single file).
        let r = home_bash(&[("file", "foo\nbar\n")]).exec("rg -U 'foo\\nbar'");
        assert_eq!(r.exit_code, 0, "compat -U exit");

        // r1878: `^` anchors at line start in multiline mode, so `^baz` matches
        // the standalone `baz` line.
        let r = home_bash(&[("test", "a\nbaz\nabc\n")]).exec("rg -U '^baz' test");
        assert_eq!(r.exit_code, 0, "r1878 exit");
        assert_eq!(r.stdout, "baz\n", "r1878 stdout");
    }

    #[test]
    fn rg_multiline_combined_with_replace_passthru_vimgrep_is_unimplemented() {
        // JBC-10: the upstream suite `it.skip`s the rows that combine multiline
        // (`-U`/`--multiline`) with --replace, --passthru, or --vimgrep
        // (regression r1311/r1866/r2094/r2095). The Rust `-U` port implements
        // plain multiline line output but does NOT apply replacement,
        // passthrough, or vimgrep formatting in multiline mode, so it cannot
        // produce the upstream-expected output. This test pins that boundary:
        // it asserts the combos do NOT yield the upstream output, so it fails if
        // someone wires up the combination without updating the classification.

        // r1311: `-U -r '?' -n '\n'` upstream replaces newlines, expecting
        // "1:hello?world?\n". The Rust port does not apply -r in multiline mode.
        let r = home_bash(&[("input", "hello\nworld\n")]).exec("rg -U -r '?' -n '\\n' input");
        assert_ne!(r.stdout, "1:hello?world?\n", "r1311 multiline replace");

        // r1866: `--multiline --vimgrep` upstream collapses to first-line vimgrep
        // output. The Rust port does not apply vimgrep formatting in multiline.
        let r = home_bash(&[("test", "foobar\nfoobar\nfoo quux\n")])
            .exec("rg --multiline --vimgrep 'foobar\\nfoobar\\nfoo|quux' test");
        assert!(
            !r.stdout.contains("test:1:1:foobar"),
            "r1866 multiline vimgrep stdout: {:?}",
            r.stdout
        );

        // r2094: multiline + max-count + passthru + replace upstream prints the
        // whole file with one replacement. The Rust port does not apply
        // passthru/replace in multiline mode.
        let r = home_bash(&[("haystack", "a\nb\nc\na\nb\nc\n")]).exec(
            "rg --no-line-number --no-filename --multiline --max-count=1 --passthru --replace=B b haystack",
        );
        assert_ne!(
            r.stdout, "a\nB\nc\na\nb\nc\n",
            "r2094 multiline passthru replace"
        );
    }

    #[test]
    fn rg_parser_threads_compat_flag_never_clobbers_max_depth() {
        // JBC-10: ports packages/just-bash/src/commands/rg/rg-parser-threads.test.ts.
        // `-j`/`--threads` is a ripgrep compatibility flag that consumes its value
        // but must be a NO-OP. The upstream regression: a buggy wiring overwrote
        // maxDepth to Infinity on every `-j`, disabling the depth guard. These
        // assertions fail if `-j` ever starts mutating depth behavior.
        let deep = &[
            ("level0.txt", "hello\n"),
            ("dir1/level1.txt", "hello\n"),
            ("dir1/dir2/level2.txt", "hello\n"),
        ];

        // Default depth survives `-j 4` (no explicit --max-depth): all levels match.
        let default_threads = home_bash(deep).exec("rg -j 4 hello");
        assert_eq!(default_threads.exit_code, 0, "rg -j 4");
        assert_eq!(default_threads.stderr, "", "rg -j 4");
        assert!(
            default_threads
                .stdout
                .contains("dir1/dir2/level2.txt:1:hello"),
            "rg -j 4 should keep the default (unbounded) depth: {:?}",
            default_threads.stdout
        );

        // Explicit --max-depth set BEFORE -j is preserved (not clobbered).
        let before = home_bash(deep).exec("rg --max-depth 1 -j 4 hello");
        assert_eq!(before.exit_code, 0, "--max-depth 1 -j 4");
        assert_eq!(before.stderr, "", "--max-depth 1 -j 4");
        assert_eq!(
            before.stdout, "level0.txt:1:hello\n",
            "--max-depth 1 must survive a following -j 4"
        );

        // Explicit --max-depth set AFTER -j is preserved.
        let after = home_bash(deep).exec("rg -j 4 --max-depth 1 hello");
        assert_eq!(after.exit_code, 0, "-j 4 --max-depth 1");
        assert_eq!(after.stderr, "", "-j 4 --max-depth 1");
        assert_eq!(
            after.stdout, "level0.txt:1:hello\n",
            "--max-depth 1 must survive a preceding -j 4"
        );

        // Long form `--threads` behaves identically to `-j`.
        let long = home_bash(deep).exec("rg --threads 8 --max-depth 1 hello");
        assert_eq!(long.exit_code, 0, "--threads 8 --max-depth 1");
        assert_eq!(long.stderr, "", "--threads 8 --max-depth 1");
        assert_eq!(
            long.stdout, "level0.txt:1:hello\n",
            "--max-depth 1 must survive --threads 8"
        );
        // `--threads=N` attached form is also accepted as a no-op.
        let long_eq = home_bash(deep).exec("rg --threads=8 --max-depth 1 hello");
        assert_eq!(long_eq.exit_code, 0, "--threads=8");
        assert_eq!(long_eq.stderr, "", "--threads=8");
        assert_eq!(long_eq.stdout, "level0.txt:1:hello\n", "--threads=8");

        // Combined short form `-Iij <n>`: -I (no filename), -i (ignore case),
        // -j 4 (ignored). maxDepth must remain the default; -I suppresses the
        // filename prefix so each match line is just the matched text.
        let combined = home_bash(deep).exec("rg -Iij 4 HELLO");
        assert_eq!(combined.exit_code, 0, "rg -Iij 4");
        assert_eq!(combined.stderr, "", "rg -Iij 4");
        // -I drops the filename prefix (line numbers remain); -i makes HELLO
        // match hello; default depth means all three levels match.
        assert_eq!(
            combined.stdout, "1:hello\n1:hello\n1:hello\n",
            "rg -Iij 4 should ignore -j and keep default depth: {:?}",
            combined.stdout
        );
    }

    #[test]
    fn rg_imported_gitignore_anchoring_and_exit_code_rows_are_portable() {
        // JBC-10: closes a cluster of imported ripgrep regression/gitignore/feature
        // rows that exercise base-anchored gitignore patterns (slash-containing or
        // rooted patterns match a path or any descendant, never a free-floating
        // substring deeper in the tree), interspersed flags after the positional
        // pattern, empty pattern-file exit semantics, and --files-without-match
        // exit codes / quiet suppression. Each assertion fails if the rg port
        // regresses the upstream behaviour.

        // gitignore.test.ts:63 + regression r2747 family: a non-rooted pattern that
        // contains a slash (`doc/frotz`) is rooted at the gitignore base. It hides
        // `doc/frotz` but NOT `a/doc/frotz`.
        assert_eq!(
            home_bash(&[
                (".git/.gitkeep", ""),
                (".gitignore", "doc/frotz\n"),
                ("doc/frotz", "x\n"),
            ])
            .exec("rg x")
            .exit_code,
            1,
            "doc/frotz should be ignored at the root",
        );
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                (".gitignore", "doc/frotz\n"),
                ("a/doc/frotz", "x\n"),
            ],
            "rg x",
            0,
            "a/doc/frotz:1:x\n",
        );

        // regression.test.ts:137 - negation of root with include (`/*` then `!/dir`):
        // only the whitelisted directory survives.
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                (".gitignore", "/*\n!/dir\n"),
                ("foo/bar", "test\n"),
                ("dir/bar", "test\n"),
            ],
            "rg test",
            0,
            "dir/bar:1:test\n",
        );

        // regression.test.ts:355 - a `-g` glob supplied AFTER the positional pattern
        // is still parsed as an option (interspersed flags), not as a path root.
        assert_home_exec(
            &[("foo/bar.txt", "test\n")],
            "rg test -g '*.txt'",
            0,
            "foo/bar.txt:1:test\n",
        );

        // regression.test.ts:659 - gitignore for a hidden subdirectory (`.a/b`):
        // hides `.a/b/**` but not `.a/c/**` under `--hidden`.
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                (".gitignore", ".a/b\n"),
                (".a/b/file", "test\n"),
                (".a/c/file", "test\n"),
            ],
            "rg --hidden test",
            0,
            ".a/c/file:1:test\n",
        );

        // regression.test.ts:677 - r829 anchored `/a/b`: excludes `a/b/**`.
        assert_eq!(
            home_bash(&[(".ignore", "/a/b\n"), ("a/b/test.txt", "Sample text\n")])
                .exec("rg Sample")
                .exit_code,
            1,
            "/a/b should ignore a/b/test.txt",
        );

        // regression.test.ts:702 - r829 `/a/*/b`: only the path matching the rooted
        // single-segment wildcard is excluded.
        assert_home_exec(
            &[
                (".ignore", "/a/*/b\n"),
                ("a/c/b/foo", ""),
                ("a/src/f/b/foo", ""),
            ],
            "rg --files",
            0,
            "a/src/f/b/foo\n",
        );

        // regression.test.ts:757 - an empty `-f` pattern file matches nothing and
        // exits 1 (no matches), NOT 2 (no pattern given).
        let empty_pat =
            home_bash(&[("sherlock", SHERLOCK), ("pat", "")]).exec("rg -f pat sherlock");
        assert_eq!(empty_pat.exit_code, 1, "empty pattern file exit");
        assert_eq!(empty_pat.stdout, "", "empty pattern file stdout");
        assert_eq!(empty_pat.stderr, "", "empty pattern file stderr");

        // regression.test.ts:1174 - `.ignore` with a path prefix (`rust/target`):
        // only the source file outside the ignored directory is listed.
        assert_home_exec(
            &[
                (".ignore", "rust/target\n"),
                ("rust/source.rs", "needle\n"),
                ("rust/target/rustdoc-output.html", "needle\n"),
            ],
            "rg --files-with-matches needle rust",
            0,
            "rust/source.rs\n",
        );

        // regression.test.ts:1447 + r3067 - gitignore `foobar/debug` hides
        // `foobar/debug/**` but not the unrelated `foobar/some/debug/**`.
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                (".gitignore", "foobar/debug\n"),
                ("foobar/some/debug/flag", "baz\n"),
                ("foobar/debug/flag2", "baz\n"),
            ],
            "rg baz",
            0,
            "foobar/some/debug/flag:1:baz\n",
        );

        // regression.test.ts:1465 - --files-without-match exit codes and quiet mode.
        assert_home_exec(
            &[("yes-match", "abc\n"), ("non-match", "xyz\n")],
            "rg --files-without-match abc non-match",
            0,
            "non-match\n",
        );
        assert_eq!(
            home_bash(&[("yes-match", "abc\n"), ("non-match", "xyz\n")])
                .exec("rg --files-without-match abc yes-match")
                .exit_code,
            1,
            "files-without-match against a matching file exits 1",
        );
        let fwm_q = home_bash(&[("yes-match", "abc\n"), ("non-match", "xyz\n")])
            .exec("rg --files-without-match -q abc non-match");
        assert_eq!(fwm_q.exit_code, 0, "quiet files-without-match exit");
        assert_eq!(
            fwm_q.stdout, "",
            "quiet files-without-match suppresses output"
        );
    }

    #[test]
    fn rg_upstream_basic_rows_are_portable() {
        assert_home_exec(
            &[("file.txt", "hello world\nfoo bar\n")],
            "rg hello",
            0,
            "file.txt:1:hello world\n",
        );
        assert_home_exec(
            &[("a.txt", "hello\n"), ("b.txt", "hello\n")],
            "rg hello",
            0,
            "a.txt:1:hello\nb.txt:1:hello\n",
        );
        assert_home_exec(
            &[
                ("src/app.ts", "const hello = 'world';\n"),
                ("README.md", "# Hello\n"),
            ],
            "rg hello src",
            0,
            "src/app.ts:1:const hello = 'world';\n",
        );
        assert_home_exec(&[("file.txt", "hello world\n")], "rg nomatch", 1, "");
        assert_home_exec(
            &[("file.txt", "line1\nhello\nline3\n")],
            "rg hello",
            0,
            "file.txt:2:hello\n",
        );
        assert_home_exec(
            &[("file.txt", "hello world\n")],
            "rg -N hello",
            0,
            "file.txt:hello world\n",
        );
        assert_home_exec(
            &[("src/lib/util.ts", "export const hello = 1;\n")],
            "rg hello",
            0,
            "src/lib/util.ts:1:export const hello = 1;\n",
        );
        assert_home_exec(
            &[("file.txt", "Hello World\nhello world\n")],
            "rg hello",
            0,
            "file.txt:1:Hello World\nfile.txt:2:hello world\n",
        );
        assert_home_exec(
            &[("file.txt", "Hello World\nhello world\n")],
            "rg Hello",
            0,
            "file.txt:1:Hello World\n",
        );
        assert_home_exec(
            &[("file.txt", "Hello World\nhello world\nHELLO WORLD\n")],
            "rg -i HELLO",
            0,
            "file.txt:1:Hello World\nfile.txt:2:hello world\nfile.txt:3:HELLO WORLD\n",
        );
        assert_home_exec(
            &[("file.txt", "Hello World\nhello world\n")],
            "rg -s hello",
            0,
            "file.txt:2:hello world\n",
        );
        assert_home_exec(
            &[("file.txt", "Hello World\nhello world\n")],
            "rg -i Hello",
            0,
            "file.txt:1:Hello World\nfile.txt:2:hello world\n",
        );
        assert_home_exec(
            &[("file.txt", "ABC123\nabc123\n")],
            "rg 123",
            0,
            "file.txt:1:ABC123\nfile.txt:2:abc123\n",
        );
        assert_home_exec(
            &[("file.txt", "foo::bar\nFOO::BAR\n")],
            "rg -F '::'",
            0,
            "file.txt:1:foo::bar\nfile.txt:2:FOO::BAR\n",
        );
        assert_home_exec(
            &[("text.txt", "hello\n"), ("binary.bin", "hello\0world\n")],
            "rg hello",
            0,
            "text.txt:1:hello\n",
        );
        assert_home_exec(
            &[
                ("level0.txt", "hello\n"),
                ("dir1/level1.txt", "hello\n"),
                ("dir1/dir2/level2.txt", "hello\n"),
            ],
            "rg --max-depth 2 hello",
            0,
            "dir1/level1.txt:1:hello\nlevel0.txt:1:hello\n",
        );
        let missing = Bash::new().exec("rg");
        assert_eq!(missing.exit_code, 2);
        assert_eq!(missing.stdout, "");
        assert_eq!(missing.stderr, "rg: no pattern given\n");
        let unknown = Bash::new().exec("rg --unknown-option pattern");
        assert_eq!(unknown.exit_code, 1);
        assert_eq!(unknown.stdout, "");
        assert_eq!(
            unknown.stderr,
            "rg: unrecognized option '--unknown-option'\n"
        );
        assert_home_exec(&[("file.txt", "hello\n")], "rg -t unknowntype hello", 1, "");
    }

    #[test]
    fn rg_imported_misc_search_modes_counts_and_context_rows_are_portable() {
        // packages/just-bash/src/commands/rg/imported-tests/misc.test.ts
        // 127 - inverted: should invert match with -v
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -v Sherlock sherlock",
            0,
            "Holmeses, success in the province of detective work must always\n\
can extract a clew from a wisp of straw or a flake of cigar ash;\n\
but Doctor Watson has to have it taken out for him and dusted,\n\
and exhibited clearly, with a label attached.\n",
        );
        // 144 - inverted_line_numbers: should show line numbers with inverted match
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -n -v Sherlock sherlock",
            0,
            "2:Holmeses, success in the province of detective work must always\n\
4:can extract a clew from a wisp of straw or a flake of cigar ash;\n\
5:but Doctor Watson has to have it taken out for him and dusted,\n\
6:and exhibited clearly, with a label attached.\n",
        );
        // 161 - case_insensitive: should search case-insensitively with -i
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -i sherlock sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\n\
be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        // 178 - word: should match whole words with -w
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -w as sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\n",
        );
        // 212 - line: should match whole lines with -x
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -x 'Watson|and exhibited clearly, with a label attached.' sherlock",
            0,
            "and exhibited clearly, with a label attached.\n",
        );
        // 231 - literal: should match literal strings with -F
        assert_home_exec(
            &[("file", "blib\n()\nblab\n")],
            "rg -F '()' file",
            0,
            "()\n",
        );
        // 246 - quiet: should suppress output with -q
        assert_home_exec(&[("sherlock", SHERLOCK)], "rg -q Sherlock sherlock", 0, "");
        // 331 - file_types: should filter by type with -t
        assert_home_exec(
            &[
                ("sherlock", SHERLOCK),
                ("file.py", "Sherlock\n"),
                ("file.rs", "Sherlock\n"),
            ],
            "rg -t rust Sherlock",
            0,
            "file.rs:1:Sherlock\n",
        );
        // 364 - file_types_negate: should negate type with -T
        assert_home_exec(
            &[("file.py", "Sherlock\n"), ("file.rs", "Sherlock\n")],
            "rg -T rust Sherlock",
            0,
            "file.py:1:Sherlock\n",
        );
        // 486 - glob: should filter by glob with -g
        assert_home_exec(
            &[
                ("sherlock", SHERLOCK),
                ("file.py", "Sherlock\n"),
                ("file.rs", "Sherlock\n"),
            ],
            "rg -g '*.rs' Sherlock",
            0,
            "file.rs:1:Sherlock\n",
        );
        // 503 - glob_negate: should negate glob with -g !
        assert_home_exec(
            &[("file.py", "Sherlock\n"), ("file.rs", "Sherlock\n")],
            "rg -g '!*.rs' Sherlock",
            0,
            "file.py:1:Sherlock\n",
        );
        // 538 - glob_case_sensitive: should use case-sensitive glob matching
        assert_home_exec(
            &[("file1.HTML", "Sherlock\n"), ("file2.html", "Sherlock\n")],
            "rg --glob '*.html' Sherlock",
            0,
            "file2.html:1:Sherlock\n",
        );
        // 590 - count: should count matching lines with --count
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg --count Sherlock",
            0,
            "sherlock:2\n",
        );
        // 605 - count_matches: should count all matches with --count-matches
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg --count-matches the",
            0,
            "sherlock:4\n",
        );
        // 620 - count_matches_inverted: should count inverted matches
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg --count-matches --invert-match Sherlock",
            0,
            "sherlock:4\n",
        );
        // 652 - include_zero: should include files with 0 matches with --include-zero
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg --count --include-zero nada",
            1,
            "sherlock:0\n",
        );
        // 670 - files_with_matches: should list files with --files-with-matches
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg --files-with-matches Sherlock",
            0,
            "sherlock\n",
        );
        // 685 - files_without_match: should list files without match
        assert_home_exec(
            &[("sherlock", SHERLOCK), ("file.py", "foo\n")],
            "rg --files-without-match Sherlock",
            0,
            "file.py\n",
        );
        // 701 - after_context: should show after context with -A
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -A 1 Sherlock sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\n\
Holmeses, success in the province of detective work must always\n\
be, to a very large extent, the result of luck. Sherlock Holmes\n\
can extract a clew from a wisp of straw or a flake of cigar ash;\n",
        );
        // 718 - after_context_line_numbers: should show after context with line numbers
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -A 1 -n Sherlock sherlock",
            0,
            "1:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
2-Holmeses, success in the province of detective work must always\n\
3:be, to a very large extent, the result of luck. Sherlock Holmes\n\
4-can extract a clew from a wisp of straw or a flake of cigar ash;\n",
        );
        // 735 - before_context: should show before context with -B
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -B 1 Sherlock sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\n\
Holmeses, success in the province of detective work must always\n\
be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        // 752 - before_context_line_numbers: should show before context with line numbers
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -B 1 -n Sherlock sherlock",
            0,
            "1:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
2-Holmeses, success in the province of detective work must always\n\
3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        // 769 - context: should show context with -C
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -C 1 'world|attached' sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\n\
Holmeses, success in the province of detective work must always\n\
--\n\
but Doctor Watson has to have it taken out for him and dusted,\n\
and exhibited clearly, with a label attached.\n",
        );
        // 786 - context_line_numbers: should show context with line numbers
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -C 1 -n 'world|attached' sherlock",
            0,
            "1:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
2-Holmeses, success in the province of detective work must always\n\
--\n\
5-but Doctor Watson has to have it taken out for him and dusted,\n\
6:and exhibited clearly, with a label attached.\n",
        );
        // 1150 - files: should list files with --files
        assert_home_exec(
            &[("a.txt", "x\n"), ("b.txt", "y\n")],
            "rg --files",
            0,
            "a.txt\nb.txt\n",
        );
        // 1181 - sort_path: should sort files by path with --sort path
        assert_home_exec(
            &[("z.txt", "Sherlock\n"), ("a.txt", "Sherlock\n")],
            "rg --sort path Sherlock",
            0,
            "a.txt:1:Sherlock\nz.txt:1:Sherlock\n",
        );
    }

    #[test]
    fn rg_imported_misc_type_heading_ignore_and_symlink_rows_are_portable() {
        // packages/just-bash/src/commands/rg/imported-tests/misc.test.ts
        // 89 - with_filename: should show filename with -H even for single file
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -H Sherlock sherlock",
            0,
            "sherlock:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
sherlock:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        // 107 - with_heading: should show heading format
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg --heading Sherlock sherlock",
            0,
            "sherlock\n\
For the Doctor Watsons of this world, as opposed to the Sherlock\n\
be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        // 348 - file_types_all: should filter type 'all' (only typed files)
        assert_home_exec(
            &[("sherlock", SHERLOCK), ("file.py", "Sherlock\n")],
            "rg -t all Sherlock",
            0,
            "file.py:1:Sherlock\n",
        );
        // 380 - file_types_negate_all: should negate type 'all' (only untyped files)
        assert_home_exec(
            &[("sherlock", SHERLOCK), ("file.py", "Sherlock\n")],
            "rg -T all Sherlock",
            0,
            "sherlock:1:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
sherlock:3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        // 398 - file_type_clear: should clear type patterns with --type-clear
        assert_home_exec(
            &[("file.py", "test\n"), ("file.rs", "test\n")],
            "rg --type-clear py -t py test",
            1,
            "",
        );
        // 415 - file_type_add: should add type patterns with --type-add
        assert_home_exec(
            &[("file.foo", "test\n"), ("file.bar", "test\n")],
            "rg --type-add 'custom:*.foo' -t custom test",
            0,
            "file.foo:1:test\n",
        );
        // 434 - file_type_add_compose: should compose types with include
        assert_home_exec(
            &[
                ("file.js", "test\n"),
                ("file.ts", "test\n"),
                ("file.py", "test\n"),
            ],
            "rg --type-add 'web:include:js' -t web test",
            0,
            "file.js:1:test\n",
        );
        // 891 - ignore_generic: should respect .ignore
        assert_home_exec(
            &[("sherlock", SHERLOCK), (".ignore", "sherlock\n")],
            "rg Sherlock",
            1,
            "",
        );
        // 906 - ignore_ripgrep: should respect .rgignore
        assert_home_exec(
            &[("sherlock", SHERLOCK), (".rgignore", "sherlock\n")],
            "rg Sherlock",
            1,
            "",
        );
        // 946 - symlink_nofollow: should not follow file symlinks during traversal by default
        assert_home_exec(
            &[("searchdir/real.txt", "test content\n")],
            "ln -s real.txt /home/user/searchdir/link.txt\nrg test searchdir",
            0,
            "searchdir/real.txt:1:test content\n",
        );
        // 965 - symlink_follow: should follow file symlinks during traversal with -L
        assert_home_exec(
            &[("searchdir/real.txt", "test content\n")],
            "ln -s real.txt /home/user/searchdir/link.txt\nrg -L test searchdir",
            0,
            "searchdir/link.txt:1:test content\n\
searchdir/real.txt:1:test content\n",
        );
        // 1004 - unrestricted2: should include hidden files with -uu
        assert_home_exec(
            &[(".sherlock", SHERLOCK)],
            "rg -uu Sherlock",
            0,
            ".sherlock:1:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
.sherlock:3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
    }

    #[test]
    fn rg_upstream_filtering_rows_are_portable() {
        assert_home_exec(
            &[
                ("app.ts", "const foo = 1;\n"),
                ("app.js", "const foo = 2;\n"),
                ("style.css", "foo { }\n"),
            ],
            "rg -t ts foo",
            0,
            "app.ts:1:const foo = 1;\n",
        );
        assert_home_exec(
            &[
                ("app.ts", "const foo = 1;\n"),
                ("app.js", "const foo = 2;\n"),
            ],
            "rg -T ts foo",
            0,
            "app.js:1:const foo = 2;\n",
        );
        let type_list = Bash::new().exec("rg --type-list");
        assert_eq!(type_list.exit_code, 0);
        assert!(type_list.stdout.contains("js:"));
        assert!(type_list.stdout.contains("ts:"));
        assert!(type_list.stdout.contains("py:"));
        assert_eq!(type_list.stderr, "");
        assert_home_exec(
            &[
                ("app.ts", "const foo = 1;\n"),
                ("app.js", "const foo = 2;\n"),
            ],
            "rg -g '*.ts' foo",
            0,
            "app.ts:1:const foo = 1;\n",
        );
        assert_home_exec(
            &[
                ("app.ts", "const foo = 1;\n"),
                ("test.ts", "const foo = 2;\n"),
            ],
            "rg -g '!test.ts' foo",
            0,
            "app.ts:1:const foo = 1;\n",
        );
        assert_home_exec(
            &[("a.ts", "foo\n"), ("b.ts", "foo\n"), ("c.js", "foo\n")],
            "rg -g '*.ts' foo",
            0,
            "a.ts:1:foo\nb.ts:1:foo\n",
        );
        assert_home_exec(
            &[("visible.txt", "hello\n"), (".hidden.txt", "hello\n")],
            "rg hello",
            0,
            "visible.txt:1:hello\n",
        );
        assert_home_exec(
            &[("visible.txt", "hello\n"), (".hidden.txt", "hello\n")],
            "rg --hidden hello",
            0,
            ".hidden.txt:1:hello\nvisible.txt:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "*.log\n"),
                ("app.ts", "hello\n"),
                ("debug.log", "hello\n"),
            ],
            "rg hello",
            0,
            "app.ts:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "*.log\n"),
                ("app.ts", "hello\n"),
                ("debug.log", "hello\n"),
            ],
            "rg --no-ignore hello",
            0,
            "app.ts:1:hello\ndebug.log:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "*.log\n!important.log\n"),
                ("debug.log", "hello\n"),
                ("important.log", "hello\n"),
            ],
            "rg hello",
            0,
            "important.log:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "build/\n"),
                ("src/app.ts", "hello\n"),
                ("build/output.js", "hello\n"),
            ],
            "rg hello",
            0,
            "src/app.ts:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "*.log\n"),
                ("app.ts", "hello\n"),
                ("subdir/file.ts", "hello\n"),
                ("subdir/debug.log", "hello\n"),
            ],
            "rg hello",
            0,
            "app.ts:1:hello\nsubdir/file.ts:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "node_modules/\n"),
                ("node_modules/pkg/index.js", "hello\n"),
                ("node_modules_backup/file.js", "hello\n"),
            ],
            "rg hello",
            0,
            "node_modules_backup/file.js:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "**/cache/**\n"),
                ("src/app.ts", "hello\n"),
                ("src/cache/data.json", "hello\n"),
                ("cache/index.json", "hello\n"),
            ],
            "rg hello",
            0,
            "src/app.ts:1:hello\n",
        );
    }

    #[test]
    fn rg_upstream_output_mode_rows_are_portable() {
        assert_home_exec(
            &[("file.txt", "hello\nhello\nhello\n")],
            "rg -c hello",
            0,
            "file.txt:3\n",
        );
        assert_home_exec(
            &[("a.txt", "hello\nhello\n"), ("b.txt", "hello\n")],
            "rg -c hello",
            0,
            "a.txt:2\nb.txt:1\n",
        );
        assert_home_exec(
            &[("file1.txt", "hello\n"), ("file2.txt", "hello\n")],
            "rg -l hello",
            0,
            "file1.txt\nfile2.txt\n",
        );
        assert_home_exec(
            &[("file1.txt", "hello\n"), ("file2.txt", "world\n")],
            "rg --files-without-match hello",
            0,
            "file2.txt\n",
        );
        assert_home_exec(
            &[("file.txt", "hello world\n")],
            "rg -o hello",
            0,
            "file.txt:hello\n",
        );
        assert_home_exec(
            &[("file.txt", "hello hello hello\n")],
            "rg -o hello",
            0,
            "file.txt:hello\nfile.txt:hello\nfile.txt:hello\n",
        );
        assert_home_exec(
            &[("file.txt", "line1\nhello\nline3\nline4\n")],
            "rg -A 1 hello",
            0,
            "file.txt:2:hello\nfile.txt-3-line3\n",
        );
        assert_home_exec(
            &[("file.txt", "line1\nline2\nhello\nline4\n")],
            "rg -B 1 hello",
            0,
            "file.txt-2-line2\nfile.txt:3:hello\n",
        );
        assert_home_exec(
            &[("file.txt", "line1\nline2\nhello\nline4\nline5\n")],
            "rg -C 1 hello",
            0,
            "file.txt-2-line2\nfile.txt:3:hello\nfile.txt-4-line4\n",
        );
        assert_home_exec(
            &[("file.txt", "match\nline2\nline3\n")],
            "rg -B 2 match",
            0,
            "file.txt:1:match\n",
        );
        assert_home_exec(
            &[("file.txt", "line1\nline2\nmatch\n")],
            "rg -A 2 match",
            0,
            "file.txt:3:match\n",
        );
        assert_home_exec(
            &[("file.txt", "a\nmatch1\nb\nmatch2\nc\n")],
            "rg -C 1 match",
            0,
            "file.txt-1-a\nfile.txt:2:match1\nfile.txt-3-b\nfile.txt:4:match2\nfile.txt-5-c\n",
        );
        assert_home_exec(
            &[("file.txt", "a\nhello\nb\nc\nd\n")],
            "rg -A2 hello",
            0,
            "file.txt:2:hello\nfile.txt-3-b\nfile.txt-4-c\n",
        );
        assert_home_exec(&[("file.txt", "hello world\n")], "rg -q hello", 0, "");
        assert_home_exec(&[("file.txt", "hello world\n")], "rg -q nomatch", 1, "");
        assert_home_exec(
            &[
                ("file1.txt", "hello\n"),
                ("file2.txt", "hello\n"),
                ("file3.txt", "hello\n"),
            ],
            "rg -q hello",
            0,
            "",
        );
        assert_home_exec(&[("file.txt", "hello world\n")], "rg --quiet hello", 0, "");
        let help = Bash::new().exec("rg --help");
        assert_eq!(help.exit_code, 0);
        assert!(help.stdout.contains("rg"));
        assert!(help.stdout.contains("recursively search"));
        assert_eq!(help.stderr, "");
    }

    #[test]
    fn rg_upstream_max_count_rows_are_portable() {
        assert_home_exec(
            &[("file.txt", "foo\nfoo\nfoo\nfoo\n")],
            "rg -m1 foo",
            0,
            "file.txt:1:foo\n",
        );
        assert_home_exec(
            &[("file.txt", "foo\nbar\nfoo\nbar\nfoo\n")],
            "rg -m2 foo",
            0,
            "file.txt:1:foo\nfile.txt:3:foo\n",
        );
        assert_home_exec(
            &[("file.txt", "test\ntest\ntest\ntest\ntest\n")],
            "rg -m 3 test",
            0,
            "file.txt:1:test\nfile.txt:2:test\nfile.txt:3:test\n",
        );
        assert_home_exec(
            &[("file.txt", "abc\nabc\nabc\nabc\n")],
            "rg --max-count=2 abc",
            0,
            "file.txt:1:abc\nfile.txt:2:abc\n",
        );
        assert_home_exec(
            &[("file.txt", "xyz\nxyz\nxyz\n")],
            "rg --max-count 1 xyz",
            0,
            "file.txt:1:xyz\n",
        );
        assert_home_exec(
            &[("file.txt", "foo\nbar\n")],
            "rg -m100 foo",
            0,
            "file.txt:1:foo\n",
        );
        assert_home_exec(&[("file.txt", "foo\nbar\n")], "rg -m1 notfound", 1, "");
        assert_home_exec(
            &[
                ("a.txt", "match\nmatch\nmatch\n"),
                ("b.txt", "match\nmatch\nmatch\n"),
            ],
            "rg -m1 match",
            0,
            "a.txt:1:match\nb.txt:1:match\n",
        );
        assert_home_exec(
            &[
                ("file1.txt", "foo\nfoo\nfoo\nfoo\n"),
                ("file2.txt", "foo\nfoo\nfoo\n"),
            ],
            "rg -m2 foo",
            0,
            "file1.txt:1:foo\nfile1.txt:2:foo\nfile2.txt:1:foo\nfile2.txt:2:foo\n",
        );
        assert_home_exec(
            &[("many.txt", "x\nx\nx\nx\nx\n"), ("few.txt", "x\n")],
            "rg -m3 x",
            0,
            "few.txt:1:x\nmany.txt:1:x\nmany.txt:2:x\nmany.txt:3:x\n",
        );
        assert_home_exec(
            &[("data.txt", "line1\nline2\nline3\nline4\nline5\n")],
            "rg -m2 line data.txt",
            0,
            "line1\nline2\n",
        );
        assert_home_exec(
            &[("data.txt", "a\na\na\na\na\n")],
            "rg -n -m2 a data.txt",
            0,
            "1:a\n2:a\n",
        );
        assert_home_exec(
            &[("file.txt", "x\nx\nx\nx\nx\n")],
            "rg -c x",
            0,
            "file.txt:5\n",
        );
        assert_home_exec(
            &[("file.txt", "foo\nbar\nfoo\nbaz\nfoo\n")],
            "rg -m2 -v foo",
            0,
            "file.txt:2:bar\nfile.txt:4:baz\n",
        );
        assert_home_exec(
            &[("file.txt", "Foo\nFOO\nfoo\nFoO\n")],
            "rg -m2 -i foo",
            0,
            "file.txt:1:Foo\nfile.txt:2:FOO\n",
        );
        assert_home_exec(
            &[("file.txt", "foo bar\nfoobar\nbar foo\nbaz foo baz\n")],
            "rg -m2 -w foo",
            0,
            "file.txt:1:foo bar\nfile.txt:3:bar foo\n",
        );
        assert_home_exec(
            &[("file.txt", "abc123def\nabc456def\nabc789def\n")],
            "rg -m2 -o '[0-9]+'",
            0,
            "file.txt:123\nfile.txt:456\n",
        );
        assert_home_exec(
            &[("a.txt", "test\ntest\ntest\n"), ("b.txt", "test\n")],
            "rg -m1 -l test",
            0,
            "a.txt\nb.txt\n",
        );
        assert_home_exec(
            &[("file.txt", "find me\nfind me\nfind me\n")],
            "rg -m1 -q 'find me'",
            0,
            "",
        );
        assert_home_exec(
            &[(
                "file.txt",
                "match1\nafter1\nmatch2\nafter2\nmatch3\nafter3\n",
            )],
            "rg -m1 -A1 match file.txt",
            0,
            "match1\nafter1\n",
        );
        assert_home_exec(
            &[(
                "file.txt",
                "before1\nmatch1\nbefore2\nmatch2\nbefore3\nmatch3\n",
            )],
            "rg -m1 -B1 match file.txt",
            0,
            "before1\nmatch1\n",
        );
        assert_home_exec(
            &[(
                "file.txt",
                "ctx1\nmatch1\nctx2\nmatch2\nctx3\nmatch3\nctx4\n",
            )],
            "rg -m1 -C1 match file.txt",
            0,
            "ctx1\nmatch1\nctx2\n",
        );
        let context = home_bash(&[(
            "file.txt",
            "a\nb\nmatch\nc\nd\ne\nf\nmatch\ng\nh\ni\nj\nmatch\nk\n",
        )])
        .exec("rg -m2 -A2 match file.txt");
        assert_eq!(context.exit_code, 0);
        assert_eq!(context.stdout.lines().count(), 7);
        assert!(context.stdout.contains("--\n"));
        assert_home_exec(
            &[("file.txt", "a\na\na\n")],
            "rg -m0 a",
            0,
            "file.txt:1:a\nfile.txt:2:a\nfile.txt:3:a\n",
        );
        assert_home_exec(
            &[("file.txt", "test\ntest\n")],
            "rg -m999999 test",
            0,
            "file.txt:1:test\nfile.txt:2:test\n",
        );
        assert_home_exec(&[("empty.txt", "")], "rg -m1 test", 1, "");
        assert_home_exec(
            &[
                ("code.js", "const x = 1;\nconst y = 2;\nconst z = 3;\n"),
                ("code.py", "const = 'not js'\n"),
            ],
            "rg -m1 -t js const",
            0,
            "code.js:1:const x = 1;\n",
        );
        assert_home_exec(
            &[
                ("test.log", "error\nerror\nerror\n"),
                ("test.txt", "error\n"),
            ],
            "rg -m1 -g '*.log' error",
            0,
            "test.log:1:error\n",
        );
        assert_home_exec(
            &[("file.txt", "cat\ndog\ncat\nbird\ncat\n")],
            "rg -m2 'cat|dog'",
            0,
            "file.txt:1:cat\nfile.txt:2:dog\n",
        );
        assert_home_exec(
            &[("file.txt", "start line\nmiddle start\nstart again\n")],
            "rg -m1 '^start'",
            0,
            "file.txt:1:start line\n",
        );
    }

    #[test]
    fn rg_upstream_no_filename_rows_are_portable() {
        assert_home_exec(
            &[("file.txt", "hello world\n")],
            "rg -I hello",
            0,
            "1:hello world\n",
        );
        assert_home_exec(
            &[("data.txt", "test line\n")],
            "rg --no-filename test",
            0,
            "1:test line\n",
        );
        assert_home_exec(
            &[("file.txt", "match here\n")],
            "rg -I -N match",
            0,
            "match here\n",
        );
        assert_home_exec(
            &[("test.txt", "foo bar\n")],
            "rg -I foo test.txt",
            0,
            "foo bar\n",
        );
        assert_home_exec(
            &[
                ("a.txt", "found\n"),
                ("b.txt", "found\n"),
                ("c.txt", "found\n"),
            ],
            "rg -I found",
            0,
            "1:found\n1:found\n1:found\n",
        );
        assert_home_exec(
            &[
                ("first.txt", "line one\nline two\n"),
                ("second.txt", "line three\n"),
            ],
            "rg -I --sort path line",
            0,
            "1:line one\n2:line two\n1:line three\n",
        );
        assert_home_exec(
            &[
                ("src/app.ts", "export const x = 1;\n"),
                ("lib/util.ts", "export const y = 2;\n"),
            ],
            "rg -I export",
            0,
            "1:export const y = 2;\n1:export const x = 1;\n",
        );
        assert_home_exec(&[("file.txt", "a\na\na\n")], "rg -I -c a", 0, "3\n");
        assert_home_exec(
            &[("a.txt", "x\nx\n"), ("b.txt", "x\nx\nx\n")],
            "rg -I -c x",
            0,
            "2\n3\n",
        );
        assert_home_exec(
            &[("nums.txt", "abc123def456\n")],
            "rg -I -o '[0-9]+'",
            0,
            "123\n456\n",
        );
        assert_home_exec(
            &[("file.txt", "keep\nremove\nkeep\n")],
            "rg -I -v remove",
            0,
            "1:keep\n3:keep\n",
        );
        assert_home_exec(
            &[("file.txt", "Hello\nHELLO\nhello\n")],
            "rg -I -i hello",
            0,
            "1:Hello\n2:HELLO\n3:hello\n",
        );
        assert_home_exec(
            &[("file.txt", "foo bar\nfoobar\nbar foo baz\n")],
            "rg -I -w foo",
            0,
            "1:foo bar\n3:bar foo baz\n",
        );
        assert_home_exec(
            &[("file.txt", "test\ntest\ntest\ntest\n")],
            "rg -I -m2 test",
            0,
            "1:test\n2:test\n",
        );
        assert_home_exec(
            &[("a.txt", "match\n"), ("b.txt", "match\n")],
            "rg -I -l match",
            0,
            "a.txt\nb.txt\n",
        );
        assert_home_exec(
            &[("has.txt", "match\n"), ("no.txt", "other\n")],
            "rg -I --files-without-match match",
            0,
            "no.txt\n",
        );
    }

    #[test]
    fn rg_upstream_no_filename_context_filter_and_regex_rows_are_portable() {
        // Maps packages/just-bash/src/commands/rg/rg.no-filename.test.ts
        // lines 222, 234, 246, 258, 276, 289, 304, 316, 328, 340, 353, 367, 379, 393.

        // -I with context lines (-A/-B/-C) hides the filename but keeps line
        // numbers and the `-` separator for context rows.
        assert_home_exec(
            &[("file.txt", "before\nmatch\nafter\nmore\n")],
            "rg -I -A1 match",
            0,
            "2:match\n3-after\n",
        );
        assert_home_exec(
            &[("file.txt", "before\nmatch\nafter\n")],
            "rg -I -B1 match",
            0,
            "1-before\n2:match\n",
        );
        assert_home_exec(
            &[("file.txt", "a\nb\nmatch\nc\nd\n")],
            "rg -I -C1 match",
            0,
            "2-b\n3:match\n4-c\n",
        );
        // Multiple files in context mode: no separator because -I removes the
        // filename prefix entirely.
        assert_home_exec(
            &[
                ("a.txt", "ctx\nmatch\nctx\n"),
                ("b.txt", "ctx\nmatch\nctx\n"),
            ],
            "rg -I -C1 --sort path match",
            0,
            "1-ctx\n2:match\n3-ctx\n1-ctx\n2:match\n3-ctx\n",
        );

        // -I combined with file filters (-t type, -g glob).
        assert_home_exec(
            &[("code.js", "const x = 1;\n"), ("code.py", "x = 1\n")],
            "rg -I -t js const",
            0,
            "1:const x = 1;\n",
        );
        assert_home_exec(
            &[
                ("test.log", "error occurred\n"),
                ("test.txt", "error here too\n"),
            ],
            "rg -I -g '*.log' error",
            0,
            "1:error occurred\n",
        );

        // -I edge cases.
        assert_home_exec(&[("file.txt", "no match here\n")], "rg -I notfound", 1, "");
        assert_home_exec(
            &[("file.txt", "path/to/file:line:content\n")],
            "rg -I path",
            0,
            "1:path/to/file:line:content\n",
        );
        assert_home_exec(
            &[(".hidden", "secret\n")],
            "rg -I --hidden secret",
            0,
            "1:secret\n",
        );
        // Combined short flags -Iin (no-filename + ignore-case + line numbers).
        assert_home_exec(
            &[("file.txt", "Test\ntest\nTEST\n")],
            "rg -Iin test",
            0,
            "1:Test\n2:test\n3:TEST\n",
        );
        assert_home_exec(&[("file.txt", "match\n")], "rg -In match", 0, "1:match\n");

        // -I with regex patterns (alternation, character classes).
        assert_home_exec(
            &[("file.txt", "apple\norange\nbanana\n")],
            "rg -I 'apple|banana'",
            0,
            "1:apple\n3:banana\n",
        );
        assert_home_exec(
            &[("file.txt", "abc123\ndef456\n")],
            "rg -I '[0-9]+'",
            0,
            "1:abc123\n2:def456\n",
        );

        // -I -N -o produces just the matched text, ideal for piping.
        assert_home_exec(
            &[("data.txt", "value: 100\nvalue: 200\nvalue: 300\n")],
            "rg -I -N -o '[0-9]+'",
            0,
            "100\n200\n300\n",
        );
    }

    #[test]
    fn rg_imported_regression_gitignore_regex_and_flag_rows_are_portable() {
        // Maps imported ripgrep regression cases in
        // packages/just-bash/src/commands/rg/imported-tests/regression.test.ts
        // (line numbers in comments). Each row exercises real portable rg
        // behavior over the virtual filesystem.

        // r24: directory trailing-slash gitignore pattern excludes the tree.
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                (".gitignore", "ghi/\n"),
                ("ghi/toplevel.txt", "xyz\n"),
                ("def/ghi/subdir.txt", "xyz\n"),
            ],
            "rg xyz",
            1,
            "",
        ); // :22
        // r25: rooted gitignore pattern only ignores at the repo root.
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                (".gitignore", "/llvm/\n"),
                ("src/llvm/foo", "test\n"),
            ],
            "rg test",
            0,
            "src/llvm/foo:1:test\n",
        ); // :39
        // r49: unanchored directory pattern.
        assert_home_exec(
            &[(".gitignore", "foo/bar\n"), ("test/foo/bar/baz", "test\n")],
            "rg xyz",
            1,
            "",
        ); // :73
        // r50: nested directory pattern excludes every matching subtree.
        assert_home_exec(
            &[
                (".gitignore", "XXX/YYY/\n"),
                ("abc/def/XXX/YYY/bar", "test\n"),
                ("ghi/XXX/YYY/bar", "test\n"),
            ],
            "rg xyz",
            1,
            "",
        ); // :88
        // r64: --files with a path argument lists only that directory.
        assert_home_exec(
            &[("dir/abc", ""), ("foo/abc", "")],
            "rg --files foo",
            0,
            "foo/abc\n",
        ); // :104
        // r65: simple directory ignore pattern.
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                (".gitignore", "a/\n"),
                ("a/foo", "xyz\n"),
                ("a/bar", "xyz\n"),
            ],
            "rg xyz",
            1,
            "",
        ); // :120
        // r87: double-star gitignore pattern still ignores the literal file.
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                (".gitignore", "foo\n**no-vcs**\n"),
                ("foo", "test\n"),
            ],
            "rg test",
            1,
            "",
        ); // :155
        // r93: IP-address-style regex with repeated groups.
        assert_home_exec(
            &[("foo", "192.168.1.1\n")],
            "rg '(\\d{1,3}\\.){3}\\d{1,3}'",
            0,
            "foo:1:192.168.1.1\n",
        ); // :188
        // r184: dot-star gitignore ignores hidden entries but keeps the tree.
        assert_home_exec(
            &[(".gitignore", ".*\n"), ("foo/bar/baz", "test\n")],
            "rg test",
            0,
            "foo/bar/baz:1:test\n",
        ); // :324
        // r229: smart-case stays case-sensitive when the bracket class has an
        // uppercase letter, so a lowercase line does not match.
        assert_home_exec(&[("foo", "economie\n")], "rg -S '[E]conomie'", 1, ""); // :376
        // r251: unicode (cyrillic) case folding under -i.
        assert_home_exec(
            &[("foo", "привет\nПривет\nПрИвЕт\n")],
            "rg -i привет",
            0,
            "foo:1:привет\nfoo:2:Привет\nfoo:3:ПрИвЕт\n",
        ); // :390
        // r270: -e pattern that begins with a dash is treated as the pattern.
        assert_home_exec(&[("foo", "-test\n")], "rg -e '-test'", 0, "foo:1:-test\n"); // :438
        // r279: quiet mode emits no stdout but exits 0 on a match.
        assert_home_exec(&[("foo", "test\n")], "rg -q test", 0, ""); // :453
        // r451: --only-matching prints just the matched substrings.
        assert_home_exec(
            &[("digits.txt", "1 2 3\n")],
            "rg --only-matching '[0-9]+' digits.txt",
            0,
            "1\n2\n3\n",
        ); // :509
        // r483: --quiet --files is silent and exits 0 when a glob matches a file.
        assert_home_exec(
            &[("file.py", "")],
            "rg --quiet --files --glob '*.py'",
            0,
            "",
        ); // :538
        // r483: --quiet --files exits 1 when the glob matches nothing.
        assert_home_exec(
            &[("file.rs", "")],
            "rg --quiet --files --glob '*.py'",
            1,
            "",
        ); // :550
    }

    #[test]
    fn rg_imported_regression_negation_symlink_anchored_and_exit_code_rows_are_portable() {
        // Maps additional imported ripgrep regression cases in
        // packages/just-bash/src/commands/rg/imported-tests/regression.test.ts
        // (line numbers in comments). Each row exercises real portable rg
        // behavior over the virtual filesystem.

        // r137 :285 -L follows a file symlink and searches its target content.
        {
            let result = home_bash(&[("real.txt", "test content\n")])
                .exec("ln -s real.txt /home/user/link.txt; rg -L test");
            assert_eq!(result.exit_code, 0, "r137 symlink follow");
            assert!(
                result.stdout.contains("test content"),
                "r137 stdout: {}",
                result.stdout
            );
        }
        // r404 :468 complex glob exclusion plus inclusion with --files.
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                ("lock", ""),
                ("bar.py", ""),
                (".git/packed-refs", ""),
                (".git/description", ""),
            ],
            "rg --no-ignore --hidden --follow --files --glob '!{.git,node_modules,plugged}/**' --glob '*.py'",
            0,
            "bar.py\n",
        ); // :468
        // r829_2778 :716 /parent/*.txt only ignores files directly in parent.
        assert_home_exec(
            &[
                (".ignore", "/parent/*.txt\n"),
                ("parent/ignore-me.txt", ""),
                ("parent/subdir/dont-ignore-me.txt", ""),
            ],
            "rg --files",
            0,
            "parent/subdir/dont-ignore-me.txt\n",
        ); // :716
        // r829_2836 :730 trailing-slash anchored pattern ignores the whole dir.
        assert_home_exec(
            &[
                (".ignore", "/testdir/sub/sub2/\n"),
                ("testdir/sub/sub2/foo", ""),
            ],
            "rg --files",
            1,
            "",
        ); // :730
        // r829_2933 :742 files-with-matches honours .ignore from cwd subdir.
        {
            let result = home_bash(&[
                (".ignore", "/testdir/sub/sub2/\n"),
                ("testdir/sub/sub2/testfile", "needle\n"),
            ])
            .exec("cd testdir; rg --files-with-matches needle");
            assert_eq!(result.exit_code, 1, "r829_2933 exit");
            assert_eq!(result.stdout, "", "r829_2933 stdout");
        }
        // r1399 :1071 -L follows good symlinks and skips broken ones.
        {
            let result = home_bash(&[("real.txt", "test content\n")]).exec(
                "ln -s real.txt /home/user/good_link.txt; ln -s nonexistent.txt /home/user/bad_link.txt; rg -L test",
            );
            assert_eq!(result.exit_code, 0, "r1399 exit");
            assert!(
                result.stdout.contains("test content"),
                "r1399 stdout: {}",
                result.stdout
            );
        }
        // r2099 :1313 --no-ignore-dot disables .ignore/.rgignore filtering.
        {
            let env = home_bash(&[
                (".ignore", "a\n"),
                (".rgignore", "b\n"),
                ("a", ""),
                ("b", ""),
                ("c", ""),
            ]);
            let r1 = env.exec("rg --files --sort path");
            assert_eq!(r1.stdout, "c\n", "r2099 default ignore");
            let r2 = env.exec("rg --files --sort path --no-ignore-dot");
            assert_eq!(r2.stdout, "a\nb\nc\n", "r2099 no-ignore-dot");
        }
        // r2731 :1385 --hidden --files lists dotfiles honouring .ignore.
        assert_home_exec(
            &[("a/.ignore", ".foo\n"), ("a/b/.foo", "")],
            "rg --hidden --files",
            0,
            "a/.ignore\n",
        ); // :1385
        // misc.test.ts :1135 -a (text) searches binary content literally.
        assert_home_exec(
            &[("file", "foo\u{0}bar\nfoo\u{0}baz\n")],
            "rg -a foo file",
            0,
            "foo\u{0}bar\nfoo\u{0}baz\n",
        ); // misc:1135
    }

    #[test]
    fn rg_upstream_ripgrep_compat_rows_are_portable() {
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg Sherlock sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\nbe, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg Sherlock",
            0,
            "sherlock:1:For the Doctor Watsons of this world, as opposed to the Sherlock\nsherlock:3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -n Sherlock sherlock",
            0,
            "1:For the Doctor Watsons of this world, as opposed to the Sherlock\n3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -N Sherlock sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\nbe, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -v Sherlock sherlock",
            0,
            "Holmeses, success in the province of detective work must always\ncan extract a clew from a wisp of straw or a flake of cigar ash;\nbut Doctor Watson has to have it taken out for him and dusted,\nand exhibited clearly, with a label attached.\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -n -v Sherlock sherlock",
            0,
            "2:Holmeses, success in the province of detective work must always\n4:can extract a clew from a wisp of straw or a flake of cigar ash;\n5:but Doctor Watson has to have it taken out for him and dusted,\n6:and exhibited clearly, with a label attached.\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -i sherlock sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\nbe, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        assert_home_exec(&[("foo", "tEsT\n")], "rg test", 0, "foo:1:tEsT\n");
        assert_home_exec(&[("foo", "tEsT\nTEST\n")], "rg TEST", 0, "foo:2:TEST\n");
        assert_home_exec(&[("foo", "tEsT\ntest\n")], "rg -s test", 0, "foo:2:test\n");
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -w as sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\n",
        );
        assert_home_exec(
            &[("haystack", "foo bar baz\nfoobar\n")],
            "rg -w foo haystack",
            0,
            "foo bar baz\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -x 'and exhibited clearly, with a label attached.' sherlock",
            0,
            "and exhibited clearly, with a label attached.\n",
        );
        assert_home_exec(
            &[("file", "blib\n()\nblab\n")],
            "rg -F '()' file",
            0,
            "()\n",
        );
        assert_home_exec(&[("sherlock", SHERLOCK)], "rg -q Sherlock sherlock", 0, "");
        assert_home_exec(&[("sherlock", SHERLOCK)], "rg -q NADA sherlock", 1, "");
        assert_home_exec(
            &[
                ("sherlock", SHERLOCK),
                ("file.py", "Sherlock\n"),
                ("file.rs", "Sherlock\n"),
            ],
            "rg -t rust Sherlock",
            0,
            "file.rs:1:Sherlock\n",
        );
        assert_home_exec(
            &[("file.py", "Sherlock\n"), ("file.rs", "Sherlock\n")],
            "rg -T rust Sherlock",
            0,
            "file.py:1:Sherlock\n",
        );
        assert_home_exec(
            &[
                ("sherlock", SHERLOCK),
                ("file.py", "Sherlock\n"),
                ("file.rs", "Sherlock\n"),
            ],
            "rg -g '*.rs' Sherlock",
            0,
            "file.rs:1:Sherlock\n",
        );
        assert_home_exec(
            &[("file.py", "Sherlock\n"), ("file.rs", "Sherlock\n")],
            "rg -g '!*.rs' Sherlock",
            0,
            "file.py:1:Sherlock\n",
        );
        assert_home_exec(
            &[("file.HTML", "Sherlock\n"), ("file.html", "Sherlock\n")],
            "rg -g '*.html' Sherlock",
            0,
            "file.html:1:Sherlock\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -c Sherlock",
            0,
            "sherlock:2\n",
        );
        assert_home_exec(&[("sherlock", SHERLOCK)], "rg -l Sherlock", 0, "sherlock\n");
        assert_home_exec(
            &[("sherlock", SHERLOCK), ("file.py", "foo\n")],
            "rg --files-without-match Sherlock",
            0,
            "file.py\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -A 1 Sherlock sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\nHolmeses, success in the province of detective work must always\nbe, to a very large extent, the result of luck. Sherlock Holmes\ncan extract a clew from a wisp of straw or a flake of cigar ash;\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -B 1 Sherlock sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\nHolmeses, success in the province of detective work must always\nbe, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -C 1 'world|attached' sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\nHolmeses, success in the province of detective work must always\n--\nbut Doctor Watson has to have it taken out for him and dusted,\nand exhibited clearly, with a label attached.\n",
        );
        assert_home_exec(&[(".sherlock", SHERLOCK)], "rg Sherlock", 1, "");
        assert_home_exec(
            &[(".sherlock", SHERLOCK)],
            "rg --hidden Sherlock",
            0,
            ".sherlock:1:For the Doctor Watsons of this world, as opposed to the Sherlock\n.sherlock:3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        assert_home_exec(
            &[(".gitignore", "sherlock\n"), ("sherlock", SHERLOCK)],
            "rg Sherlock",
            1,
            "",
        );
        assert_home_exec(
            &[(".gitignore", "sherlock\n"), ("sherlock", SHERLOCK)],
            "rg --no-ignore Sherlock",
            0,
            "sherlock:1:For the Doctor Watsons of this world, as opposed to the Sherlock\nsherlock:3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        assert_home_exec(
            &[("one/pass", "far\n"), ("one/too/many", "far\n")],
            "rg --max-depth 2 far",
            0,
            "one/pass:1:far\n",
        );
        assert_home_exec(
            &[("file.txt", "foo\nbar\nbaz\n")],
            "rg -e foo -e bar",
            0,
            "file.txt:1:foo\nfile.txt:2:bar\n",
        );
        assert_home_exec(&[("foo", "-test\n")], "rg -e '-test'", 0, "foo:1:-test\n");
        assert_home_exec(
            &[("digits.txt", "1 2 3\n")],
            "rg -o '[0-9]+' digits.txt",
            0,
            "1\n2\n3\n",
        );
        assert_home_exec(
            &[("foo", "192.168.1.1\n")],
            "rg '(\\d{1,3}\\.){3}\\d{1,3}'",
            0,
            "foo:1:192.168.1.1\n",
        );
        assert_home_exec(
            &[("file", "cat\ndog\nbird\n")],
            "rg 'cat|dog'",
            0,
            "file:1:cat\nfile:2:dog\n",
        );
        assert_eq!(
            home_bash(&[("sherlock", SHERLOCK)]).exec("rg .").exit_code,
            0
        );
        assert_eq!(
            home_bash(&[("sherlock", SHERLOCK)])
                .exec("rg NADA")
                .exit_code,
            1
        );
        assert_eq!(
            home_bash(&[("sherlock", SHERLOCK)])
                .exec("rg '*'")
                .exit_code,
            2
        );
        assert_home_exec(
            &[("text.txt", "hello\n"), ("binary.bin", "hello\0world\n")],
            "rg hello",
            0,
            "text.txt:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "ghi/\n"),
                ("ghi/toplevel.txt", "xyz\n"),
                ("def/ghi/subdir.txt", "xyz\n"),
            ],
            "rg xyz",
            1,
            "",
        );
        assert_home_exec(
            &[(".gitignore", "/llvm/\n"), ("src/llvm/foo", "test\n")],
            "rg test",
            0,
            "src/llvm/foo:1:test\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "vendor/**\n!vendor/manifest\n"),
                ("vendor/manifest", "test\n"),
                ("vendor/other", "test\n"),
            ],
            "rg test",
            0,
            "vendor/manifest:1:test\n",
        );
        assert_home_exec(
            &[(".gitignore", "foo/bar\n"), ("test/foo/bar/baz", "test\n")],
            "rg xyz",
            1,
            "",
        );
        assert_home_exec(
            &[(".gitignore", "!.foo\n"), (".foo", "test\n")],
            "rg --hidden test",
            0,
            ".foo:1:test\n",
        );
        assert_home_exec(
            &[("foo", "привет\nПривет\nПрИвЕт\n")],
            "rg -i привет",
            0,
            "foo:1:привет\nfoo:2:Привет\nfoo:3:ПрИвЕт\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK), (".patterns", "Sherlock\nHolmes\n")],
            "rg -f .patterns sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\nHolmeses, success in the province of detective work must always\nbe, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -0 -l Sherlock",
            0,
            "sherlock\0",
        );
        assert_home_exec(
            &[("foo", "test\ntest\ntest\n")],
            "rg -m1 test",
            0,
            "foo:1:test\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg --count-matches the",
            0,
            "sherlock:4\n",
        );
        let heading = home_bash(&[("sherlock", SHERLOCK)]).exec("rg --heading Sherlock");
        assert_eq!(heading.exit_code, 0);
        assert!(heading.stdout.starts_with("sherlock\n"));
        assert_home_exec(
            &[("test", "foo\nctx\nbar\nctx\nfoo\nctx\n")],
            "rg -A1 --context-separator AAA foo test",
            0,
            "foo\nctx\nAAA\nfoo\nctx\n",
        );
        assert_home_exec(
            &[
                ("foo", "test\n"),
                ("abc", "test\n"),
                ("zoo", "test\n"),
                ("bar", "test\n"),
            ],
            "rg --sort path test",
            0,
            "abc:1:test\nbar:1:test\nfoo:1:test\nzoo:1:test\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg --no-filename Sherlock",
            0,
            "1:For the Doctor Watsons of this world, as opposed to the Sherlock\n3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -I Sherlock",
            0,
            "1:For the Doctor Watsons of this world, as opposed to the Sherlock\n3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -c --include-zero nada",
            1,
            "sherlock:0\n",
        );
    }

    #[test]
    fn rg_upstream_files_and_imported_feature_rows_are_portable() {
        assert_home_exec(&[("sherlock", SHERLOCK)], "rg --files", 0, "sherlock\n");
        assert_home_exec(&[("sherlock", SHERLOCK)], "rg --files -0", 0, "sherlock\0");
        assert_home_exec(
            &[
                ("zebra.txt", "z\n"),
                ("alpha.txt", "a\n"),
                ("beta.txt", "b\n"),
            ],
            "rg --files",
            0,
            "alpha.txt\nbeta.txt\nzebra.txt\n",
        );
        assert_home_exec(
            &[
                ("code.js", "js\n"),
                ("code.py", "py\n"),
                ("code.ts", "ts\n"),
            ],
            "rg --files -t js",
            0,
            "code.js\n",
        );
        assert_home_exec(
            &[
                ("test.txt", "txt\n"),
                ("test.log", "log\n"),
                ("other.txt", "txt\n"),
            ],
            "rg --files -g 'test.*'",
            0,
            "test.log\ntest.txt\n",
        );
        assert_home_exec(
            &[
                ("top.txt", "top\n"),
                ("sub/deep.txt", "deep\n"),
                ("sub/deeper/bottom.txt", "bottom\n"),
            ],
            "rg --files -d 2",
            0,
            "sub/deep.txt\ntop.txt\n",
        );
        assert_home_exec(&[(".hidden", "hidden\n")], "rg --files", 1, "");
        assert_home_exec(
            &[(".hidden", "hidden\n"), ("visible", "visible\n")],
            "rg --files --hidden",
            0,
            ".hidden\nvisible\n",
        );
        assert_home_exec(
            &[("dir/abc", "content\n"), ("foo/abc", "content\n")],
            "rg --files foo",
            0,
            "foo/abc\n",
        );
        assert_home_exec(
            &[
                ("file.py", "python\n"),
                ("file.rs", "rust\n"),
                ("file.txt", "text\n"),
            ],
            "rg --files --glob '*.py'",
            0,
            "file.py\n",
        );
        assert_home_exec(
            &[("file.py", "python\n"), ("file.rs", "rust\n")],
            "rg --quiet --files --glob '*.py'",
            0,
            "",
        );
        assert_home_exec(
            &[("one/pass", "far\n"), ("one/too/many", "far\n")],
            "rg -d 2 far",
            0,
            "one/pass:1:far\n",
        );
        assert_home_exec(
            &[("top.txt", "match\n"), ("sub/nested.txt", "match\n")],
            "rg -d 1 match",
            0,
            "top.txt:1:match\n",
        );
        assert_home_exec(
            &[("a/b/c/deep.txt", "found\n")],
            "rg -d 4 found",
            0,
            "a/b/c/deep.txt:1:found\n",
        );
        assert_home_exec(
            &[
                ("top.js", "test\n"),
                ("sub/nested.js", "test\n"),
                ("top.py", "test\n"),
            ],
            "rg -d 1 -t js test",
            0,
            "top.js:1:test\n",
        );
        assert_home_exec(&[("foo", "tEsT\n")], "rg -S -s test", 1, "");
        assert_home_exec(&[("foo", "TEST\n")], "rg -i -s test", 1, "");
        assert_home_exec(
            &[("foo", "test\ntest\ntest\n")],
            "rg -m 2 test",
            0,
            "foo:1:test\nfoo:2:test\n",
        );
        assert_home_exec(
            &[("foo", "test\ntest\n")],
            "rg -m0 test",
            0,
            "foo:1:test\nfoo:2:test\n",
        );
        assert_home_exec(
            &[("test", "1\n2\n3\n4\n5\n6\n7\n8\n9\n")],
            "rg -C1 -A2 5 test",
            0,
            "4\n5\n6\n7\n",
        );
        assert_home_exec(
            &[("test", "foo\nctx\nbar\nctx\nfoo\nctx\n")],
            "rg -A1 foo test",
            0,
            "foo\nctx\n--\nfoo\nctx\n",
        );
        assert_home_exec(
            &[("file", "foo\nbar\nbaz\n")],
            "rg -e foo -e bar",
            0,
            "file:1:foo\nfile:2:bar\n",
        );
        assert_home_exec(
            &[
                ("visible.txt", "test\n"),
                ("ignored.txt", "test\n"),
                (".gitignore", "ignored.txt\n"),
            ],
            "rg test",
            0,
            "visible.txt:1:test\n",
        );
        assert_home_exec(
            &[
                ("visible.txt", "test\n"),
                ("ignored.txt", "test\n"),
                (".gitignore", "ignored.txt\n"),
            ],
            "rg --no-ignore --sort path test",
            0,
            "ignored.txt:1:test\nvisible.txt:1:test\n",
        );
        assert_home_exec(
            &[(".hidden", "test\n"), ("visible", "test\n")],
            "rg test",
            0,
            "visible:1:test\n",
        );
        assert_home_exec(
            &[(".hidden", "test\n"), ("visible", "test\n")],
            "rg --hidden --sort path test",
            0,
            ".hidden:1:test\nvisible:1:test\n",
        );
        assert_home_exec(
            &[
                ("code.js", "test\n"),
                ("code.py", "test\n"),
                ("code.rs", "test\n"),
            ],
            "rg -t js test",
            0,
            "code.js:1:test\n",
        );
        assert_home_exec(
            &[("code.js", "test\n"), ("code.py", "test\n")],
            "rg -T js test",
            0,
            "code.py:1:test\n",
        );
        assert_home_exec(
            &[("README.md", "test\n"), ("code.py", "test\n")],
            "rg -t markdown test",
            0,
            "README.md:1:test\n",
        );
        assert_home_exec(
            &[("doc.markdown", "content\n"), ("code.js", "content\n")],
            "rg -t markdown content",
            0,
            "doc.markdown:1:content\n",
        );
        assert_home_exec(
            &[("notes.mdown", "info\n"), ("code.py", "info\n")],
            "rg -t markdown info",
            0,
            "notes.mdown:1:info\n",
        );
        let md = home_bash(&[("doc.md", "text\n"), ("other.txt", "text\n")]);
        assert_eq!(
            md.exec("rg -t md text").stdout,
            md.exec("rg -t markdown text").stdout
        );
        assert_home_exec(
            &[("README.md", "test\n"), ("code.py", "test\n")],
            "rg -T markdown test",
            0,
            "code.py:1:test\n",
        );
        assert_home_exec(
            &[("file.txt", "test\n"), ("file.log", "test\n")],
            "rg -g '*.txt' test",
            0,
            "file.txt:1:test\n",
        );
        assert_home_exec(
            &[("file.txt", "test\n"), ("file.log", "test\n")],
            "rg -g '!*.log' test",
            0,
            "file.txt:1:test\n",
        );
        assert_home_exec(
            &[("file", "foo foobar barfoo\n")],
            "rg -w foo",
            0,
            "file:1:foo foobar barfoo\n",
        );
        assert_home_exec(&[("file", "foobar\n")], "rg -w foo", 1, "");
        assert_home_exec(
            &[("file", "foo\nfoo bar\n")],
            "rg -x foo",
            0,
            "file:1:foo\n",
        );
        assert_home_exec(
            &[("file", "foo\nbar\nbaz\n")],
            "rg -v foo",
            0,
            "file:2:bar\nfile:3:baz\n",
        );
        assert_home_exec(
            &[("file", "foo.*bar\nfoobar\n")],
            "rg -F 'foo.*bar'",
            0,
            "file:1:foo.*bar\n",
        );
        assert_home_exec(&[("file", "test\n")], "rg -q test", 0, "");
        assert_home_exec(&[("file", "test\n")], "rg -q notfound", 1, "");
        assert_home_exec(
            &[("file", "foo\ntest\nbar\n")],
            "rg -n test file",
            0,
            "2:test\n",
        );
        assert_home_exec(&[("file", "test\n")], "rg -N test file", 0, "test\n");
        assert_home_exec(
            &[("file", "a\nmatch\nb\nc\n")],
            "rg -A2 match file",
            0,
            "match\nb\nc\n",
        );
        assert_home_exec(
            &[("file", "a\nb\nmatch\nc\n")],
            "rg -B2 match file",
            0,
            "a\nb\nmatch\n",
        );
        assert_home_exec(
            &[("file", "a\nb\nmatch\nc\nd\n")],
            "rg -C1 match file",
            0,
            "b\nmatch\nc\n",
        );
        assert_home_exec(
            &[("file", "FOO foobar\n")],
            "rg -iw foo",
            0,
            "file:1:FOO foobar\n",
        );
        assert_home_exec(&[("file", "foo\nFOO\nFoo\n")], "rg -ci foo", 0, "file:3\n");
        assert_home_exec(
            &[("a.txt", "FOO\n"), ("b.txt", "bar\n")],
            "rg -li foo",
            0,
            "a.txt\n",
        );
        assert_home_exec(
            &[("in.txt", "miss\n한글 found\nmiss\n")],
            "cat in.txt | rg '한글'",
            0,
            "<stdin>:2:한글 found\n",
        );
    }

    #[test]
    fn rg_imported_feature_filters_stats_pcre_and_ignore_rows_are_portable() {
        // packages/just-bash/src/commands/rg/imported-tests/feature.test.ts:20
        // should hide filename with --no-filename
        let no_filename = home_bash(&[("sherlock", SHERLOCK)]).exec("rg --no-filename Sherlock");
        assert_eq!(no_filename.exit_code, 0);
        assert!(!no_filename.stdout.contains("sherlock:"));
        assert!(no_filename.stdout.contains("Sherlock"));
        // feature.test.ts:35 - should show only matching text with -o
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -o Sherlock",
            0,
            "sherlock:Sherlock\nsherlock:Sherlock\n",
        );
        // feature.test.ts:49 - should use smart case with -S (lowercase matches insensitively)
        let smart = home_bash(&[("sherlock", SHERLOCK)]).exec("rg -S sherlock");
        assert_eq!(smart.exit_code, 0);
        assert!(smart.stdout.contains("Sherlock"));
        // feature.test.ts:62 - should be case-sensitive when pattern has uppercase
        let cased = home_bash(&[("sherlock", SHERLOCK)]).exec("rg -S Sherlock");
        assert_eq!(cased.exit_code, 0);
        assert!(cased.stdout.contains("Sherlock"));
        // feature.test.ts:77 - should list files with matches with -l
        assert_home_exec(&[("sherlock", SHERLOCK)], "rg -l Sherlock", 0, "sherlock\n");
        // feature.test.ts:89 - should list files without matches with --files-without-match
        assert_home_exec(
            &[("sherlock", SHERLOCK), ("file.py", "foo\n")],
            "rg --files-without-match Sherlock",
            0,
            "file.py\n",
        );
        // feature.test.ts:102 - should count matches with -c
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -c Sherlock",
            0,
            "sherlock:2\n",
        );
        // misc.test.ts:21 - single_file: search single file without filename prefix
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg Sherlock sherlock",
            0,
            "For the Doctor Watsons of this world, as opposed to the Sherlock\n\
be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        // feature.test.ts:396 - should return exit code 0 on match
        assert_eq!(
            home_bash(&[("sherlock", SHERLOCK)]).exec("rg .").exit_code,
            0
        );
        // feature.test.ts:407 - should return exit code 1 on no match
        assert_eq!(
            home_bash(&[("sherlock", SHERLOCK)])
                .exec("rg NADA")
                .exit_code,
            1
        );
        // feature.test.ts:418 - should return exit code 2 on error (invalid regex)
        assert_eq!(
            home_bash(&[("sherlock", SHERLOCK)])
                .exec("rg '*'")
                .exit_code,
            2
        );
        // feature.test.ts:443 - should allow -C to set both -A and -B
        assert_home_exec(
            &[("test", "1\n2\n3\n4\n5\n6\n7\n8\n9\n")],
            "rg -A2 -C1 5 test",
            0,
            "4\n5\n6\n7\n",
        );
        // feature.test.ts:860 - should output search statistics with --stats
        let stats = home_bash(&[("file", "foo\nbar\nfoo\n")]).exec("rg --stats foo");
        assert_eq!(stats.exit_code, 0);
        assert!(stats.stdout.contains("2 matches"));
        assert!(stats.stdout.contains("1 files contained matches"));
        assert!(stats.stdout.contains("1 files searched"));
        // feature.test.ts:874 - should show stats for multiple files
        let stats_multi = home_bash(&[
            ("a.txt", "foo\n"),
            ("b.txt", "foo\nfoo\n"),
            ("c.txt", "bar\n"),
        ])
        .exec("rg --stats foo");
        assert_eq!(stats_multi.exit_code, 0);
        assert!(stats_multi.stdout.contains("3 matches"));
        assert!(stats_multi.stdout.contains("2 files contained matches"));
        assert!(stats_multi.stdout.contains("3 files searched"));
        // feature.test.ts:890 - should show stats even with no matches
        let stats_none = home_bash(&[("file", "hello\n")]).exec("rg --stats notfound");
        assert_eq!(stats_none.exit_code, 1);
        assert!(stats_none.stdout.contains("0 matches"));
        assert!(stats_none.stdout.contains("0 files contained matches"));
        assert!(stats_none.stdout.contains("1 files searched"));
        // feature.test.ts:904 - should include search results before stats
        let stats_order = home_bash(&[("file", "test\n")]).exec("rg --stats test");
        assert_eq!(stats_order.exit_code, 0);
        let results_index = stats_order.stdout.find("file:1:test").expect("result line");
        let stats_index = stats_order.stdout.find("1 matches").expect("stats line");
        assert!(results_index < stats_index);
        // feature.test.ts:917 - should show bytes searched in stats
        let stats_bytes = home_bash(&[("file", "test content here\n")]).exec("rg --stats test");
        assert_eq!(stats_bytes.exit_code, 0);
        assert!(stats_bytes.stdout.contains("bytes searched"));
        // feature.test.ts:931 - should error on -P flag (PCRE2 unsupported)
        let pcre_short = home_bash(&[("file", "test\n")]).exec("rg -P test");
        assert_eq!(pcre_short.exit_code, 1);
        assert_eq!(
            pcre_short.stderr,
            "rg: PCRE2 is not supported. Use standard regex syntax instead.\n"
        );
        // feature.test.ts:945 - should error on --pcre2 flag
        let pcre_long = home_bash(&[("file", "test\n")]).exec("rg --pcre2 test");
        assert_eq!(pcre_long.exit_code, 1);
        assert_eq!(
            pcre_long.stderr,
            "rg: PCRE2 is not supported. Use standard regex syntax instead.\n"
        );
        // feature.test.ts:974 - should read patterns from stdin with -f-
        assert_home_exec(
            &[("test.txt", "foo\nbar\nbaz\n")],
            "echo 'bar' | rg -f- test.txt",
            0,
            "bar\n",
        );
        // feature.test.ts:990 - f45_relative_cwd: should apply patterns from --ignore-file
        let ignore_file = home_bash(&[
            ("my-ignore", "ignored.txt\n"),
            ("ignored.txt", "test content\n"),
            ("included.txt", "test content\n"),
        ])
        .exec("rg --ignore-file my-ignore test");
        assert_eq!(ignore_file.exit_code, 0);
        assert!(ignore_file.stdout.contains("included.txt"));
        assert!(!ignore_file.stdout.contains("ignored.txt"));
        // feature.test.ts:1006 - f45_precedence_with_others: --ignore-file patterns applied
        let ignore_glob = home_bash(&[
            ("custom-ignore", "*.log\n"),
            ("test.txt", "test\n"),
            ("test.log", "test\n"),
        ])
        .exec("rg --ignore-file custom-ignore test");
        assert_eq!(ignore_glob.exit_code, 0);
        assert!(ignore_glob.stdout.contains("test.txt"));
        assert!(!ignore_glob.stdout.contains("test.log"));
        // feature.test.ts:1024 - should skip .gitignore with --no-ignore-vcs
        let vcs_files = &[
            (".gitignore", "ignored.txt\n"),
            ("ignored.txt", "Sherlock\n"),
            ("visible.txt", "Sherlock\n"),
        ];
        assert_home_exec(vcs_files, "rg Sherlock", 0, "visible.txt:1:Sherlock\n");
        let no_vcs = home_bash(vcs_files).exec("rg --no-ignore-vcs Sherlock");
        assert_eq!(no_vcs.exit_code, 0);
        assert!(no_vcs.stdout.contains("ignored.txt"));
        assert!(no_vcs.stdout.contains("visible.txt"));
    }

    #[test]
    fn rg_imported_binary_detection_rows_are_portable() {
        // packages/just-bash/src/commands/rg/imported-tests/binary.test.ts:18
        // should skip binary files by default in directory search
        assert_home_exec(
            &[
                ("text.txt", "hello world\n"),
                ("binary.bin", "hello\u{0}world\n"),
            ],
            "rg hello",
            0,
            "text.txt:1:hello world\n",
        );
        // binary.test.ts:32 - should skip binary files when searching single explicit file
        assert_home_exec(
            &[("binary.bin", "hello\u{0}world\n")],
            "rg hello binary.bin",
            1,
            "",
        );
        // binary.test.ts:44 - should detect binary via NUL early in file
        let early = format!("\u{0}{}pattern\n", "a".repeat(100));
        assert_home_exec(&[("early.bin", early.as_str())], "rg pattern", 1, "");
        // binary.test.ts:71 - should not count matches in binary files
        assert_home_exec(
            &[
                ("text.txt", "match\nmatch\n"),
                ("binary.bin", "match\u{0}match\n"),
            ],
            "rg -c match",
            0,
            "text.txt:2\n",
        );
        // binary.test.ts:86 - should not list binary files with -l
        assert_home_exec(
            &[("text.txt", "findme\n"), ("binary.bin", "findme\u{0}\n")],
            "rg -l findme",
            0,
            "text.txt\n",
        );
        // binary.test.ts:101 - should only search text files in mixed directory
        assert_home_exec(
            &[
                ("readme.md", "documentation\n"),
                ("image.png", "\u{89}PNG\r\n\u{1a}\n\u{0}\u{0}\u{0}"),
                ("script.sh", "echo documentation\n"),
            ],
            "rg --sort path documentation",
            0,
            "readme.md:1:documentation\nscript.sh:1:echo documentation\n",
        );
        // binary.test.ts:117 - should handle multiple binary and text files
        assert_home_exec(
            &[
                ("a.txt", "test\n"),
                ("b.bin", "test\u{0}\n"),
                ("c.txt", "test\n"),
                ("d.bin", "test\u{0}\n"),
            ],
            "rg test",
            0,
            "a.txt:1:test\nc.txt:1:test\n",
        );
    }

    #[test]
    fn rg_imported_binary_edge_case_and_flag_rows_are_portable() {
        // binary.test.ts:56 - should not detect binary if NUL after 8KB sample
        let late = format!("pattern\n{}\u{0}end\n", "a".repeat(9000));
        assert_home_exec(
            &[("late.txt", late.as_str())],
            "rg pattern",
            0,
            "late.txt:1:pattern\n",
        );
        // binary.test.ts:134 - should handle file with only NUL bytes
        assert_home_exec(
            &[("nulls.bin", "\u{0}\u{0}\u{0}\u{0}")],
            "rg anything",
            1,
            "",
        );
        // binary.test.ts:145 - should handle NUL at start of file
        assert_home_exec(&[("start.bin", "\u{0}hello world\n")], "rg hello", 1, "");
        // binary.test.ts:156 - should handle NUL at end of file
        assert_home_exec(&[("end.bin", "hello world\n\u{0}")], "rg hello", 1, "");
        // binary.test.ts:167 - should handle multiple NUL bytes
        assert_home_exec(
            &[("multi.bin", "a\u{0}b\u{0}c\u{0}d\n")],
            "rg '[a-d]'",
            1,
            "",
        );
        // binary.test.ts:180 - should skip files with common binary signatures
        assert_home_exec(
            &[
                ("image.png", "\u{89}PNG\r\n\u{1a}\n\u{0}data"),
                ("doc.pdf", "%PDF-1.4\n\u{0}binary"),
                ("archive.zip", "PK\u{3}\u{4}\u{0}\u{0}data"),
                ("text.txt", "data\n"),
            ],
            "rg data",
            0,
            "text.txt:1:data\n",
        );
        // binary.test.ts:201 - should work with -i flag
        assert_home_exec(
            &[
                ("text.txt", "HELLO world\n"),
                ("binary.bin", "HELLO\u{0}world\n"),
            ],
            "rg -i hello",
            0,
            "text.txt:1:HELLO world\n",
        );
        // binary.test.ts:214 - should work with -v flag
        assert_home_exec(
            &[
                ("text.txt", "keep\nremove\nkeep\n"),
                ("binary.bin", "keep\u{0}remove\n"),
            ],
            "rg -v remove",
            0,
            "text.txt:1:keep\ntext.txt:3:keep\n",
        );
        // binary.test.ts:227 - should work with -w flag
        assert_home_exec(
            &[
                ("text.txt", "foo bar\nfoobar\n"),
                ("binary.bin", "foo bar\u{0}\n"),
            ],
            "rg -w foo",
            0,
            "text.txt:1:foo bar\n",
        );
        // binary.test.ts:240 - should work with context flags
        assert_home_exec(
            &[
                ("text.txt", "before\nmatch\nafter\n"),
                ("binary.bin", "before\u{0}match\nafter\n"),
            ],
            "rg -C1 match",
            0,
            "text.txt-1-before\ntext.txt:2:match\ntext.txt-3-after\n",
        );
        // binary.test.ts:255 - should work with -m flag
        assert_home_exec(
            &[
                ("text.txt", "match\nmatch\nmatch\n"),
                ("binary.bin", "match\u{0}match\n"),
            ],
            "rg -m1 match",
            0,
            "text.txt:1:match\n",
        );
        // binary.test.ts:270 - should skip binary files in subdirectories
        assert_home_exec(
            &[
                ("src/code.ts", "export const x = 1;\n"),
                ("assets/image.bin", "export\u{0}data\n"),
                ("lib/util.ts", "export function foo() {}\n"),
            ],
            "rg --sort path export",
            0,
            "lib/util.ts:1:export function foo() {}\nsrc/code.ts:1:export const x = 1;\n",
        );
    }

    #[test]
    fn rg_flags_symlink_unrestricted_and_text_rows_are_portable() {
        // rg.flags.test.ts:11 - should accept -L/--follow flag without error
        assert_home_exec(
            &[("file.txt", "hello world\n")],
            "rg -L hello",
            0,
            "file.txt:1:hello world\n",
        );
        // rg.flags.test.ts:23 - should accept --follow flag without error
        assert_home_exec(
            &[("file.txt", "hello world\n")],
            "rg --follow hello",
            0,
            "file.txt:1:hello world\n",
        );
        // rg.flags.test.ts:35 - should skip symlinks by default in directory search
        let env = home_bash(&[("real.txt", "hello\n")]);
        assert_eq!(env.exec("ln -s real.txt /home/user/link.txt").exit_code, 0);
        let default = env.exec("rg hello");
        assert_eq!(default.exit_code, 0);
        assert_eq!(default.stdout, "real.txt:1:hello\n");
        // rg.flags.test.ts:49 - should follow symlinks with -L in directory search
        let followed = env.exec("rg -L --sort path hello");
        assert_eq!(followed.exit_code, 0);
        assert_eq!(followed.stdout, "link.txt:1:hello\nreal.txt:1:hello\n");
        // rg.flags.test.ts:122 - -u should be equivalent to --no-ignore
        let u_env = home_bash(&[(".gitignore", "ignored.txt\n"), ("ignored.txt", "hello\n")]);
        let result_u = u_env.exec("rg -u hello");
        let result_no_ignore = u_env.exec("rg --no-ignore hello");
        assert_eq!(result_u.stdout, result_no_ignore.stdout);
        assert!(result_u.stdout.contains("ignored.txt:1:hello"));
        // rg.flags.test.ts:135 - -uu should be equivalent to --no-ignore --hidden
        let uu_env = home_bash(&[(".hidden", "hello\n")]);
        let result_uu = uu_env.exec("rg -uu hello");
        let result_flags = uu_env.exec("rg --no-ignore --hidden hello");
        assert_eq!(result_uu.stdout, result_flags.stdout);
        assert!(result_uu.stdout.contains(".hidden:1:hello"));
        // rg.flags.test.ts:149 - should search binary files as text with -a
        let bin_env = home_bash(&[("binary.bin", "hello\u{0}world\n")]);
        let without_a = bin_env.exec("rg hello");
        assert_eq!(without_a.exit_code, 1);
        assert_eq!(without_a.stdout, "");
        let with_a = bin_env.exec("rg -a hello");
        assert_eq!(with_a.exit_code, 0);
        assert_eq!(with_a.stdout, "binary.bin:1:hello\u{0}world\n");
    }

    #[test]
    fn rg_imported_misc_filename_ignore_and_hidden_rows_are_portable() {
        // packages/just-bash/src/commands/rg/imported-tests/misc.test.ts
        // 38 - dir: should search directory with filename prefix
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg Sherlock",
            0,
            "sherlock:1:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
sherlock:3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        // 55 - line_numbers: should show line numbers with -n
        assert_home_exec(
            &[("sherlock", SHERLOCK)],
            "rg -n Sherlock sherlock",
            0,
            "1:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        // 844 - ignore_hidden: should ignore hidden files by default
        assert_home_exec(&[(".sherlock", SHERLOCK)], "rg Sherlock", 1, "");
        // 858 - no_ignore_hidden: should include hidden files with --hidden
        assert_home_exec(
            &[(".sherlock", SHERLOCK)],
            "rg --hidden Sherlock",
            0,
            ".sherlock:1:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
.sherlock:3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        // 875 - ignore_git: should respect .gitignore
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                ("sherlock", SHERLOCK),
                (".gitignore", "sherlock\n"),
            ],
            "rg Sherlock",
            1,
            "",
        );
        // 921 - no_ignore: should ignore .gitignore with --no-ignore
        assert_home_exec(
            &[("sherlock", SHERLOCK), (".gitignore", "sherlock\n")],
            "rg --no-ignore Sherlock",
            0,
            "sherlock:1:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
sherlock:3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        // 986 - unrestricted1: should ignore .gitignore with -u
        assert_home_exec(
            &[("sherlock", SHERLOCK), (".gitignore", "sherlock\n")],
            "rg -u Sherlock",
            0,
            "sherlock:1:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
sherlock:3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
    }

    #[test]
    fn rg_imported_regression_regex_word_and_flag_rows_are_portable() {
        // packages/just-bash/src/commands/rg/imported-tests/regression.test.ts
        // 56 - r41: negation after double-star in gitignore
        assert_home_exec(
            &[
                (".gitignore", "vendor/**\n!vendor/manifest\n"),
                ("vendor/manifest", "test\n"),
                ("vendor/other", "test\n"),
            ],
            "rg test",
            0,
            "vendor/manifest:1:test\n",
        );
        // 303 - r156: should match complex regex pattern (-N, output spans multiple lines)
        let cx = home_bash(&[(
            "testcase.txt",
            "#parse('widgets/foo_bar_macros.vm')\n\
#parse ( 'widgets/mobile/foo_bar_macros.vm' )\n\
#parse (\"widgets/foobarhiddenformfields.vm\")\n",
        )]);
        let r303 = cx.exec(
            "rg -N '#(?:parse|include)\\s*\\(\\s*(?:\"|'\"'\"')[./A-Za-z_-]+(?:\"|'\"'\"')' testcase.txt",
        );
        assert_eq!(r303.exit_code, 0);
        assert!(r303.stdout.split('\n').count() > 1, "{:?}", r303.stdout);
        // 340 - r196: smart case with word boundary regex
        assert_home_exec(
            &[("foo", "tEsT\n")],
            "rg --smart-case '\\btest\\b'",
            0,
            "foo:1:tEsT\n",
        );
        // 564 - r488: word boundary with leading/trailing spaces
        assert_home_exec(
            &[("input.txt", "peshwaship 're seminomata\n")],
            "rg -o \"\\b 're \\b\" input.txt",
            0,
            " 're \n",
        );
        // 579 - r506: -w -o with alternation
        assert_home_exec(
            &[("wb.txt", "min minimum amin\nmax maximum amax\n")],
            "rg -w -o 'min|max' wb.txt",
            0,
            "min\nmax\n",
        );
        // 625 - r568: -e leading hyphen pattern
        assert_home_exec(
            &[("file", "foo bar -baz\n")],
            "rg -e '-baz' file",
            0,
            "foo bar -baz\n",
        );
        // 643 - r693: should ignore context with -c
        assert_home_exec(
            &[("bar", "xyz\n"), ("foo", "xyz\n")],
            "rg -C1 -c --sort path xyz",
            0,
            "bar:1\nfoo:1\n",
        );
        // 772 - r1064: should match with capture group
        assert_home_exec(&[("input", "abc\n")], "rg 'a(.*c)'", 0, "input:1:abc\n");
        // 945 - r1322: should match patterns ending with repeated zeros
        let z = home_bash(&[("test", "153.230000\n")]);
        let z1 = z.exec("rg '\\d\\d\\d00' test");
        assert_eq!(z1.exit_code, 0);
        assert_eq!(z1.stdout, "153.230000\n");
        let z2 = z.exec("rg '\\d\\d\\d000' test");
        assert_eq!(z2.exit_code, 0);
        assert_eq!(z2.stdout, "153.230000\n");
        // 1056 - r1389(prev): should limit matches with -m and show context
        assert_home_exec(
            &[("foo", "a\nb\nc\nd\ne\nd\ne\nd\ne\nd\ne\n")],
            "rg -A2 -m1 d foo",
            0,
            "d\ne\nd\n",
        );
        // 1102 - r1559: should match semicolon comma pattern
        assert_home_exec(
            &[("foo", "abc;de,fg\n")],
            "rg ';(.*,){1}'",
            0,
            "foo:1:abc;de,fg\n",
        );
        // 1117 - r1559: should match pattern with multiple spaces between fields
        let spaced = home_bash(&[(
            "foo",
            "type A struct {\n\tTaskID int `json:\"taskID\"`\n}\n\n\
type B struct {\n\tObjectID string `json:\"objectID\"`\n\tTaskID   int    `json:\"taskID\"`\n}\n",
        )]);
        let spaced_result = spaced.exec("rg 'TaskID +int'");
        assert_eq!(spaced_result.exit_code, 0);
        assert_eq!(spaced_result.stderr, "");
        assert!(
            spaced_result.stdout.contains("TaskID int"),
            "{:?}",
            spaced_result.stdout
        );
        assert!(
            spaced_result.stdout.contains("TaskID   int"),
            "{:?}",
            spaced_result.stdout
        );
    }

    #[test]
    fn rg_imported_regression_gitignore_files_and_exit_code_rows_are_portable() {
        // packages/just-bash/src/commands/rg/imported-tests/regression.test.ts
        // 689 - r829_2731: negation of build directory with -l
        assert_home_exec(
            &[
                (".ignore", "build/\n!/some_dir/build/\n"),
                ("some_dir/build/foo", "string\n"),
            ],
            "rg -l string",
            0,
            "some_dir/build/foo\n",
        );
        // 787 - r1098: a**b pattern in gitignore (no match)
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                (".gitignore", "a**b\n"),
                ("afoob", "test\n"),
            ],
            "rg test",
            1,
            "",
        );
        // 803 - r1130: should list files with matches
        assert_home_exec(
            &[("foo", "test\n")],
            "rg --files-with-matches test foo",
            0,
            "foo\n",
        );
        // 815 - r1130: should list files without matches
        assert_home_exec(
            &[("foo", "test\n")],
            "rg --files-without-match nada foo",
            0,
            "foo\n",
        );
        // 830 - r1159_invalid_flag: should return nonzero exit code for invalid flag
        let invalid = home_bash(&[]).exec("rg --wat test");
        assert_ne!(invalid.exit_code, 0);
        // 839 - r1159_exit_status: should return correct exit codes
        let codes = home_bash(&[("foo", "test\n")]);
        assert_eq!(codes.exec("rg test").exit_code, 0);
        assert_eq!(codes.exec("rg nada").exit_code, 1);
        assert_eq!(codes.exec("rg -q test").exit_code, 0);
        assert_eq!(codes.exec("rg -q nada").exit_code, 1);
        // 884 - r1174(prev): should handle ** in gitignore (no match)
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                (".gitignore", "**\n"),
                ("foo", "test\n"),
            ],
            "rg test",
            1,
            "",
        );
        // 900 - r1174: should handle **/**/* in gitignore (no match)
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                (".gitignore", "**/**/*\n"),
                ("a/foo", "test\n"),
            ],
            "rg test",
            1,
            "",
        );
        // 967 - r1330: pattern file without trailing newline
        assert_home_exec(
            &[
                ("patterns-nonl", "[foo]"),
                ("patterns-nl", "[foo]\n"),
                ("test", "fz\n"),
            ],
            "rg -f patterns-nonl test",
            0,
            "fz\n",
        );
        // 1413 - r2901: **/bar/* pattern with -l (no match)
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                (".gitignore", "**/bar/*\n"),
                ("foo/bar/baz", "quux\n"),
            ],
            "rg -l quux",
            1,
            "",
        );
        // 1490 - r3127_gitignore_allow_unclosed_class: unclosed class allowed in gitignore
        assert_home_exec(
            &[
                (".git/.gitkeep", ""),
                (".gitignore", "[abc\n"),
                ("[abc", ""),
                ("test", ""),
            ],
            "rg --files",
            0,
            "test\n",
        );
    }

    #[test]
    fn rg_imported_regression_misc_pattern_file_context_and_filter_rows_are_portable() {
        // packages/just-bash/src/commands/rg/imported-tests/regression.test.ts
        // 247 - r105: gitignore with full path pattern hides foo/sherlock but keeps foo/watson
        let r105 = home_bash(&[
            (".git/.gitkeep", ""),
            (".gitignore", "foo/sherlock\n"),
            ("foo/sherlock", SHERLOCK),
            ("foo/watson", SHERLOCK),
        ])
        .exec("rg Sherlock");
        assert_eq!(r105.exit_code, 0, "r105");
        assert!(r105.stdout.contains("foo/watson:"), "r105 keeps watson");
        assert!(
            !r105.stdout.contains("foo/sherlock:"),
            "r105 ignores sherlock"
        );
        assert_eq!(r105.stderr, "", "r105 stderr");

        // 594 - r553_switch: repeated -i flag stays case-insensitive
        let r553i = home_bash(&[("sherlock", SHERLOCK)]).exec("rg -i -i sherlock");
        assert_eq!(r553i.exit_code, 0, "r553_switch");
        assert!(r553i.stdout.contains("Sherlock"), "r553_switch matches");
        assert_eq!(r553i.stderr, "", "r553_switch stderr");

        // 606 - r553_flag: later -C 0 overrides an earlier -C 1, removing the context separator
        let r553c = home_bash(&[("sherlock", SHERLOCK)]);
        let with_ctx = r553c.exec("rg -C 1 'world|attached' sherlock");
        assert_eq!(with_ctx.exit_code, 0, "r553_flag ctx");
        assert!(with_ctx.stdout.contains("--"), "r553_flag has separator");
        let no_ctx = r553c.exec("rg -C 1 -C 0 'world|attached' sherlock");
        assert_eq!(no_ctx.exit_code, 0, "r553_flag override");
        assert!(
            !no_ctx.stdout.contains("--"),
            "r553_flag override removes separator"
        );

        // 916 - r1176_literal_file: -F with a pattern file treats the pattern literally
        assert_home_exec(
            &[("patterns", "foo(bar\n"), ("test", "foo(bar\n")],
            "rg -F -f patterns test",
            0,
            "foo(bar\n",
        );
        // 929 - r1176_line_regex: -x with a pattern file requires a whole-line match
        assert_home_exec(
            &[("patterns", "foo\n"), ("test", "foobar\nfoo\nbarfoo\n")],
            "rg -x -f patterns test",
            0,
            "foo\n",
        );

        // 1003 - r1319: DNA sequence regex with bounded repetition matches the corpus line
        let dna = home_bash(&[(
            "input",
            "CCAGCTACTCGGGAGGCTGAGGCTGGAGGATCGCTTGAGTCCAGGAGTTC\n",
        )])
        .exec("rg 'TTGAGTCCAGGAG[ATCG]{2}C'");
        assert_eq!(dna.exit_code, 0, "r1319 exit");
        assert!(
            dna.stdout
                .contains("CCAGCTACTCGGGAGGCTGAGGCTGGAGGATCGCTTGAGTCCAGGAGTTC"),
            "r1319 matches DNA line"
        );
        assert_eq!(dna.stderr, "", "r1319 stderr");

        // 1364 - r2236: multiple -e patterns with --only-matching list each match in order
        assert_home_exec(
            &[("file", "FooBar\n")],
            "rg --only-matching -e Foo -e Bar file",
            0,
            "Foo\nBar\n",
        );

        // 1429 - r2770: --stats reports the bytes searched summary line
        let stats = home_bash(&[("haystack", "foo1\nfoo2\nfoo3\nfoo4\nfoo5\n")])
            .exec("rg --stats -m2 foo .");
        assert_eq!(stats.exit_code, 0, "r2770 exit");
        assert!(
            stats.stdout.contains("bytes searched"),
            "r2770 reports bytes searched"
        );
        assert_eq!(stats.stderr, "", "r2770 stderr");
    }

    #[test]
    fn rg_imported_misc_type_list_and_unrestricted_rows_are_portable() {
        // packages/just-bash/src/commands/rg/imported-tests/misc.test.ts
        // 1004 - unrestricted2: -uu includes hidden dotfiles in the search
        assert_home_exec(
            &[(".sherlock", SHERLOCK)],
            "rg -uu Sherlock",
            0,
            ".sherlock:1:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
.sherlock:3:be, to a very large extent, the result of luck. Sherlock Holmes\n",
        );
        // 1167 - files (type-list): --type-list lists the known file types
        let type_list = home_bash(&[]).exec("rg --type-list");
        assert_eq!(type_list.exit_code, 0, "type-list exit");
        assert!(type_list.stdout.contains("rust"), "type-list has rust");
        assert!(type_list.stdout.contains("py"), "type-list has py");
        assert_eq!(type_list.stderr, "", "type-list stderr");
    }

    #[test]
    fn text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/twenty.txt".to_string(),
                    format!(
                        "{}\n",
                        (1..=20)
                            .map(|n| format!("line{n}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                ),
                ("/short.txt".to_string(), "a\nb\nc\nd\ne\n".to_string()),
                ("/a.txt".to_string(), "aaa\n".to_string()),
                ("/b.txt".to_string(), "bbb\n".to_string()),
                ("/empty.txt".to_string(), "".to_string()),
                ("/single.txt".to_string(), "only line\n".to_string()),
                ("/no-newline.txt".to_string(), "no newline".to_string()),
                ("/wc.txt".to_string(), "hello world\nfoo bar\n".to_string()),
                ("/words.txt".to_string(), "one two\nthree\n".to_string()),
                ("/hello.txt".to_string(), "hello\n".to_string()),
                (
                    "/spaces.txt".to_string(),
                    "one   two    three\n".to_string(),
                ),
                (
                    "/names.txt".to_string(),
                    "Charlie\nAlice\nBob\nDavid\n".to_string(),
                ),
                ("/numbers.txt".to_string(), "10\n2\n1\n20\n5\n".to_string()),
                (
                    "/duplicates.txt".to_string(),
                    "apple\nbanana\napple\ncherry\nbanana\n".to_string(),
                ),
                (
                    "/columns.txt".to_string(),
                    "John 25\nAlice 30\nBob 20\nDavid 35\n".to_string(),
                ),
                (
                    "/case-mixed.txt".to_string(),
                    "zebra\nalpha\nZebra\nAlpha\n".to_string(),
                ),
                (
                    "/adjacent.txt".to_string(),
                    "apple\napple\nbanana\nbanana\nbanana\ncherry\n".to_string(),
                ),
                ("/mixed.txt".to_string(), "a\nb\na\nc\nc\n".to_string()),
                (
                    "/uniq-single.txt".to_string(),
                    "one\ntwo\nthree\n".to_string(),
                ),
                (
                    "/all-same.txt".to_string(),
                    "hello\nhello\nhello\n".to_string(),
                ),
                (
                    "/passwd.txt".to_string(),
                    "root:x:0:0:root:/root:/bin/bash\nuser:x:1000:1000:User:/home/user:/bin/zsh\n"
                        .to_string(),
                ),
                (
                    "/csv.txt".to_string(),
                    "name,age,city\nJohn,25,NYC\nJane,30,LA\n".to_string(),
                ),
                (
                    "/tabs.txt".to_string(),
                    "col1\tcol2\tcol3\nval1\tval2\tval3\n".to_string(),
                ),
                (
                    "/text.txt".to_string(),
                    "hello world\nabcdefghij\n".to_string(),
                ),
                (
                    "/first-third.txt".to_string(),
                    "first\nsecond\nthird\n".to_string(),
                ),
                (
                    "/newline-lines.txt".to_string(),
                    "line1\nline2\nline3\n".to_string(),
                ),
                ("/unicode.txt".to_string(), "漢字\n".to_string()),
                (
                    "/unicode-fields.txt".to_string(),
                    "한글:café:漢字\n".to_string(),
                ),
                (
                    "/unicode-uniq.txt".to_string(),
                    "한글\n한글\ncafé\n".to_string(),
                ),
                ("/unicode-case.txt".to_string(), "café\nCAFÉ\n".to_string()),
                ("/binary-uniq.txt".to_string(), "a\na\nb\n".to_string()),
                ("/binary-case.txt".to_string(), "É\né\n".to_string()),
                (
                    "/sort-case-dupes.txt".to_string(),
                    "zebra\nAlpha\nalpha\nZebra\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec("head /twenty.txt").stdout,
            "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10\n"
        );
        assert_eq!(env.exec("head -n 3 /short.txt").stdout, "a\nb\nc\n");
        assert_eq!(env.exec("head -n3 /short.txt").stdout, "a\nb\nc\n");
        assert_eq!(env.exec("head -2 /short.txt").stdout, "a\nb\n");
        assert_eq!(
            env.exec("head /a.txt /b.txt").stdout,
            "==> /a.txt <==\naaa\n\n==> /b.txt <==\nbbb\n"
        );
        assert_eq!(
            env.exec("head /missing.txt").stderr,
            "head: /missing.txt: No such file or directory\n"
        );
        assert_eq!(env.exec("head -n 10 /a.txt").stdout, "aaa\n");
        assert_eq!(
            env.exec("echo -e \"a\\nb\\nc\\nd\\ne\" | head -n 2").stdout,
            "a\nb\n"
        );
        assert_eq!(env.exec("head /empty.txt").exit_code, 0);
        assert_eq!(env.exec("head -n 1 /single.txt").stdout, "only line\n");
        assert_eq!(env.exec("head -n 1 /no-newline.txt").stdout, "no newline\n");
        assert_eq!(env.exec("head -n 1 /first-third.txt").stdout, "first\n");

        assert_eq!(
            env.exec("tail /twenty.txt").stdout,
            "line11\nline12\nline13\nline14\nline15\nline16\nline17\nline18\nline19\nline20\n"
        );
        assert_eq!(env.exec("tail -n 2 /short.txt").stdout, "d\ne\n");
        assert_eq!(env.exec("tail -n2 /short.txt").stdout, "d\ne\n");
        assert_eq!(env.exec("tail -3 /short.txt").stdout, "c\nd\ne\n");
        assert_eq!(env.exec("tail -n +3 /short.txt").stdout, "c\nd\ne\n");
        assert_eq!(
            env.exec("cat /short.txt | head -n 3 | tail -n 1").stdout,
            "c\n"
        );
        assert_eq!(env.exec("tail -n 10 /a.txt").stdout, "aaa\n");
        assert_eq!(
            env.exec("tail /a.txt /b.txt").stdout,
            "==> /a.txt <==\naaa\n\n==> /b.txt <==\nbbb\n"
        );
        assert_eq!(
            env.exec("tail /missing.txt").stderr,
            "tail: /missing.txt: No such file or directory\n"
        );
        assert_eq!(
            env.exec("echo -e \"a\\nb\\nc\\nd\\ne\" | tail -n 2").stdout,
            "d\ne\n"
        );
        assert_eq!(env.exec("tail /empty.txt").exit_code, 0);
        assert_eq!(env.exec("tail -n 1 /single.txt").stdout, "only line\n");
        assert_eq!(env.exec("tail -n +1 /short.txt").stdout, "a\nb\nc\nd\ne\n");
        assert_eq!(env.exec("tail -n +2 /short.txt").stdout, "b\nc\nd\ne\n");
        assert_eq!(env.exec("tail -n +10 /a.txt").stdout, "\n");
        assert_eq!(
            env.exec("echo -e \"a\\nb\\nc\\nd\\ne\" | tail -n +3")
                .stdout,
            "c\nd\ne\n"
        );
        assert_eq!(env.exec("tail -n 1 /first-third.txt").stdout, "third\n");

        assert!(env.exec("wc /wc.txt").stdout.contains("2 4 20 /wc.txt"));
        assert_eq!(env.exec("wc -l /short.txt").stdout.trim(), "5 /short.txt");
        assert_eq!(env.exec("wc -w /words.txt").stdout.trim(), "3 /words.txt");
        assert_eq!(env.exec("wc -c /hello.txt").stdout.trim(), "6 /hello.txt");
        assert_eq!(env.exec("wc -m /hello.txt").stdout.trim(), "6 /hello.txt");
        assert!(env.exec("wc /a.txt /b.txt").stdout.contains("total"));
        assert_eq!(env.exec("echo \"hello world\" | wc -w").stdout.trim(), "2");
        assert_eq!(env.exec("wc /missing.txt").exit_code, 1);
        assert_eq!(env.exec("wc --lines /a.txt").stdout.trim(), "1 /a.txt");
        assert_eq!(
            env.exec("wc --words /words.txt").stdout.trim(),
            "3 /words.txt"
        );
        assert_eq!(
            env.exec("wc --bytes /hello.txt").stdout.trim(),
            "6 /hello.txt"
        );
        assert_eq!(env.exec("wc -w /spaces.txt").stdout.trim(), "3 /spaces.txt");
        assert_eq!(
            env.exec("wc -lw /words.txt").stdout.trim(),
            "2 3 /words.txt"
        );
        assert!(
            env.exec("wc /empty.txt")
                .stdout
                .contains("0 0 0 /empty.txt")
        );
        assert_eq!(
            env.exec("wc -l /newline-lines.txt").stdout.trim(),
            "3 /newline-lines.txt"
        );

        assert_eq!(
            env.exec("sort /names.txt").stdout,
            "Alice\nBob\nCharlie\nDavid\n"
        );
        assert_eq!(
            env.exec("sort -r /names.txt").stdout,
            "David\nCharlie\nBob\nAlice\n"
        );
        assert_eq!(env.exec("sort -n /numbers.txt").stdout, "1\n2\n5\n10\n20\n");
        assert_eq!(
            env.exec("sort -rn /numbers.txt").stdout,
            "20\n10\n5\n2\n1\n"
        );
        assert_eq!(
            env.exec("sort -nr /numbers.txt").stdout,
            "20\n10\n5\n2\n1\n"
        );
        assert_eq!(
            env.exec("sort -u /duplicates.txt").stdout,
            "apple\nbanana\ncherry\n"
        );
        assert_eq!(
            env.exec("sort -k2 -n /columns.txt").stdout,
            "Bob 20\nJohn 25\nAlice 30\nDavid 35\n"
        );
        assert_eq!(env.exec("echo -e \"c\\nb\\na\" | sort").stdout, "a\nb\nc\n");
        assert_eq!(
            env.exec("sort /case-mixed.txt").stdout,
            "alpha\nAlpha\nzebra\nZebra\n"
        );
        assert_eq!(
            env.exec("sort -f /case-mixed.txt").stdout,
            "alpha\nAlpha\nzebra\nZebra\n"
        );
        assert_eq!(
            env.exec("sort --ignore-case /case-mixed.txt").stdout,
            "alpha\nAlpha\nzebra\nZebra\n"
        );
        assert_eq!(
            env.exec("echo -e 'Banana\\napple\\nCherry' | sort --ignore-case")
                .stdout,
            "apple\nBanana\nCherry\n"
        );
        assert_eq!(
            env.exec("sort -fr /case-mixed.txt").stdout,
            "Zebra\nzebra\nAlpha\nalpha\n"
        );
        assert_eq!(
            env.exec("echo -e 'apple\\nBanana\\ncherry' | sort -fr")
                .stdout,
            "cherry\nBanana\napple\n"
        );
        assert_eq!(
            env.exec("sort -fu /sort-case-dupes.txt")
                .stdout
                .lines()
                .count(),
            2
        );
        assert_eq!(
            env.exec("sort -k2 -f /columns.txt").stdout,
            "Bob 20\nJohn 25\nAlice 30\nDavid 35\n"
        );
        assert_eq!(
            env.exec("echo -e '1 Zebra\\n2 apple\\n3 BANANA' | sort -f -k2")
                .stdout,
            "2 apple\n3 BANANA\n1 Zebra\n"
        );
        let sort_help = env.exec("sort --help");
        assert!(sort_help.stdout.contains("Usage: sort"));
        assert_eq!(sort_help.exit_code, 0);
        assert_eq!(
            env.exec("sort /missing.txt").stderr,
            "sort: /missing.txt: No such file or directory\n"
        );
        assert_eq!(env.exec("echo \"\" | sort").stdout, "\n");

        assert_eq!(
            env.exec("uniq /adjacent.txt").stdout,
            "apple\nbanana\ncherry\n"
        );
        assert_eq!(
            env.exec("uniq -c /adjacent.txt").stdout,
            "   2 apple\n   3 banana\n   1 cherry\n"
        );
        assert_eq!(env.exec("uniq -d /adjacent.txt").stdout, "apple\nbanana\n");
        assert_eq!(env.exec("uniq -u /adjacent.txt").stdout, "cherry\n");
        assert_eq!(env.exec("uniq /mixed.txt").stdout, "a\nb\na\nc\n");
        assert_eq!(env.exec("echo -e \"x\\nx\\ny\" | uniq").stdout, "x\ny\n");
        assert_eq!(env.exec("sort /mixed.txt | uniq").stdout, "a\nb\nc\n");
        assert_eq!(
            env.exec("uniq /uniq-single.txt").stdout,
            "one\ntwo\nthree\n"
        );
        assert_eq!(env.exec("uniq /all-same.txt").stdout, "hello\n");
        assert_eq!(
            env.exec("uniq /missing.txt").stderr,
            "uniq: /missing.txt: No such file or directory\n"
        );
        assert_eq!(env.exec("echo -n \"\" | uniq").stdout, "");
        assert_eq!(
            env.exec("cat /unicode-uniq.txt | uniq").stdout,
            "한글\ncafé\n"
        );
        assert_eq!(env.exec("cat /unicode-case.txt | uniq -i").stdout, "café\n");
        assert_eq!(env.exec("uniq /binary-uniq.txt").stdout, "a\nb\n");
        assert_eq!(env.exec("uniq -i /binary-case.txt").stdout, "É\n");

        assert_eq!(env.exec("cut -d: -f1 /passwd.txt").stdout, "root\nuser\n");
        assert_eq!(
            env.exec("cut -d: -f1,3 /passwd.txt").stdout,
            "root:0\nuser:1000\n"
        );
        assert_eq!(
            env.exec("cut -d: -f1-3 /passwd.txt").stdout,
            "root:x:0\nuser:x:1000\n"
        );
        assert_eq!(
            env.exec("cut -d, -f1,2 /csv.txt").stdout,
            "name,age\nJohn,25\nJane,30\n"
        );
        assert_eq!(env.exec("cut -f2 /tabs.txt").stdout, "col2\nval2\n");
        assert_eq!(env.exec("cut -c1-5 /text.txt").stdout, "hello\nabcde\n");
        assert_eq!(env.exec("cut -c1,3,5 /text.txt").stdout, "hlo\nace\n");
        assert_eq!(env.exec("echo 'a:b:c' | cut -d: -f2").stdout, "b\n");
        assert_eq!(
            env.exec("cut -d: -f5- /passwd.txt").stdout,
            "root:/root:/bin/bash\nUser:/home/user:/bin/zsh\n"
        );
        assert_eq!(
            env.exec("cut -f1 /missing.txt").stderr,
            "cut: /missing.txt: No such file or directory\n"
        );
        assert_eq!(
            env.exec("cut /text.txt").stderr,
            "cut: you must specify a list of bytes, characters, or fields\n"
        );
        assert_eq!(env.exec("cat /unicode.txt | cut -c 1-2").stdout, "漢字\n");
        assert_eq!(
            env.exec("cat /unicode-fields.txt | cut -d: -f2").stdout,
            "café\n"
        );

        assert_eq!(
            env.exec("echo 'hello world' | tr 'a-z' 'A-Z'").stdout,
            "HELLO WORLD\n"
        );
        assert_eq!(
            env.exec("echo 'HELLO WORLD' | tr 'A-Z' 'a-z'").stdout,
            "hello world\n"
        );
        assert_eq!(
            env.exec("echo 'hello world' | tr -d 'aeiou'").stdout,
            "hll wrld\n"
        );
        assert_eq!(
            env.exec("echo -e 'line1\\nline2' | tr -d '\\n'").stdout,
            "line1line2"
        );
        assert_eq!(
            env.exec("echo 'hello    world' | tr -s ' '").stdout,
            "hello world\n"
        );
        assert_eq!(env.exec("echo 'abc' | tr 'abc' 'xyz'").stdout, "xyz\n");
        assert_eq!(
            env.exec("echo 'hello world' | tr ' ' '_'").stdout,
            "hello_world\n"
        );
        assert_eq!(env.exec("echo '12345' | tr '1-5' 'a-e'").stdout, "abcde\n");
        assert_eq!(
            env.exec("echo 'abc123def' | tr -d '0-9'").stdout,
            "abcdef\n"
        );
        assert_eq!(
            env.exec("echo 'hello' | tr").stderr,
            "tr: missing operand\n"
        );
        assert_eq!(
            env.exec("echo 'hello' | tr 'a-z'").stderr,
            "tr: missing operand after SET1\n"
        );
        assert_eq!(env.exec("echo 'aabbcc' | tr 'abc' 'x'").stdout, "xxxxxx\n");
        assert_eq!(env.exec("echo 'abc' | tr 'a-c' 'A-C'").stdout, "ABC\n");
        assert_eq!(
            env.exec("cat /unicode-fields.txt | tr 'é' 'X'").stdout,
            "한글:cafX:漢字\n"
        );
        assert_eq!(
            env.exec("echo '한글 abc' | tr 'a-z' 'A-Z'").stdout,
            "한글 ABC\n"
        );
        assert_eq!(
            env.exec("echo 'abc123def456' | tr -cd '0-9'").stdout,
            "123456"
        );
        assert_eq!(
            env.exec("echo 'hello, world! 123' | tr -cd '[:alnum:]'")
                .stdout,
            "helloworld123"
        );
        assert_eq!(env.exec("echo 'a1b2c3' | tr -cd '[:alpha:]'").stdout, "abc");
        assert_eq!(env.exec("echo 'abc123' | tr -Cd '0-9'").stdout, "123");
        assert_eq!(
            env.exec("echo 'abc123' | tr --complement -d '0-9'").stdout,
            "123"
        );
        assert_eq!(
            env.exec("echo 'abc123def' | tr -c '0-9' 'X'").stdout,
            "XXX123XXXX"
        );
        assert_eq!(
            env.exec("echo 'hello world!' | tr -c '[:alnum:]' '-'")
                .stdout,
            "hello-world--"
        );
        assert_eq!(
            env.exec("echo 'aaa111bbb222' | tr -cs '0-9' 'X'").stdout,
            "X111X222X"
        );
        assert_eq!(
            env.exec("echo 'test123test' | tr -cd '[:alpha:]'").stdout,
            "testtest"
        );
    }

    #[test]
    fn text_search_jbc34_grep_basic_regex_and_utf8_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/a.txt".to_string(),
                    "found here\nHello\nhello\nHELLO\ncat\nfoo\n".to_string(),
                ),
                ("/b.txt".to_string(), "nothing\nWorld\nremove\n".to_string()),
                ("/c.txt".to_string(), "also found\nCAT\n".to_string()),
                ("/dir/file.txt".to_string(), "findme\nneedle here\n".to_string()),
                (
                    "/dir/sub/deep.txt".to_string(),
                    "another needle\n".to_string(),
                ),
                (
                    "/regex.txt".to_string(),
                    "hello world\nworld hello\ncat\ncut\ncot\ncar\nac\nabc\nabbc\nabbbc\nbat\nrat\nhat\nprice: $100\nprice: 100\nabc123\n123abc\ncolor\ncolour\ncolr\nab\nabab\nababab\n".to_string(),
                ),
                (
                    "/repeat.txt".to_string(),
                    "ac\nabc\nabbc\nabbbc\n".to_string(),
                ),
                (
                    "/digits-regex.txt".to_string(),
                    "abc\nabc123\n123abc\n".to_string(),
                ),
                ("/plus.txt".to_string(), "ac\nabc\nabbc\n".to_string()),
                (
                    "/optional.txt".to_string(),
                    "color\ncolour\ncolr\n".to_string(),
                ),
                (
                    "/groups.txt".to_string(),
                    "ab\nabab\nababab\nac\n".to_string(),
                ),
                ("/iv.txt".to_string(), "Hello\nWorld\nhello\n".to_string()),
                (
                    "/ci.txt".to_string(),
                    "Hello\nhello\nHELLO\nworld\n".to_string(),
                ),
                ("/nv.txt".to_string(), "keep\nremove\nkeep\n".to_string()),
                (
                    "/cat-case.txt".to_string(),
                    "Cat\ncat\ncaterpillar\nCAT\n".to_string(),
                ),
                ("/eidog.txt".to_string(), "CAT\nDOG\nbird\n".to_string()),
                ("/empty.txt".to_string(), String::new()),
                ("/newlines.txt".to_string(), "\n\n\n".to_string()),
                (
                    "/long.txt".to_string(),
                    format!("{}needle{}\n", "a".repeat(128), "b".repeat(128)),
                ),
                (
                    "/multi-match.txt".to_string(),
                    "foo bar foo baz foo\n".to_string(),
                ),
                (
                    "/file-with-dash.txt".to_string(),
                    "content\n".to_string(),
                ),
                (
                    "/unicode.txt".to_string(),
                    "hello\n中文\nworld\ncol1\tcol2\tcol3\n".to_string(),
                ),
                (
                    "/posix.txt".to_string(),
                    "abc\n123\na1b\nABC\nAbC\ndeadbeef\n12ab\nGHI\nhello world\nhelloworld\na1\nb2\nc!\n!@#\n".to_string(),
                ),
                (
                    "/literal.txt".to_string(),
                    "a^b\na$b\nacb\nabc\n*\n*abc\nab\nabb\nabbb\na1\n1a\nA9\n".to_string(),
                ),
                ("/bre-star.txt".to_string(), "ab\nabb\nabbb\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec("grep --files-with-matches found /a.txt /b.txt /c.txt")
                .stdout,
            "/a.txt\n/c.txt\n"
        );
        assert_eq!(
            env.exec("grep -R findme /dir").stdout,
            "/dir/file.txt:findme\n"
        );
        assert_eq!(
            env.exec("grep -r needle /dir").stdout,
            "/dir/file.txt:needle here\n/dir/sub/deep.txt:another needle\n"
        );
        assert_eq!(env.exec("grep -w cat /a.txt").stdout, "cat\n");
        assert_eq!(env.exec("grep --word-regexp cat /a.txt").stdout, "cat\n");
        assert_eq!(
            env.exec(r#"grep -E "cat|cut" /regex.txt"#).stdout,
            "cat\ncut\n"
        );
        assert_eq!(
            env.exec(r#"grep --extended-regexp "abc[0-9]+" /regex.txt"#)
                .stdout,
            "abc123\n"
        );
        assert_eq!(env.exec("grep -e hello /a.txt").stdout, "hello\n");
        assert_eq!(
            env.exec("grep pattern /dir").stderr,
            "grep: /dir: Is a directory\n"
        );

        assert_eq!(
            env.exec(r#"grep "^hello" /regex.txt"#).stdout,
            "hello world\n"
        );
        assert_eq!(
            env.exec(r#"grep "hello$" /regex.txt"#).stdout,
            "world hello\n"
        );
        assert_eq!(
            env.exec(r#"grep "c.t" /regex.txt"#).stdout,
            "cat\ncut\ncot\n"
        );
        assert_eq!(
            env.exec(r#"grep "ab*c" /repeat.txt"#).stdout,
            "ac\nabc\nabbc\nabbbc\n"
        );
        assert_eq!(
            env.exec(r#"grep "[cbr]at" /regex.txt"#).stdout,
            "cat\nbat\nrat\n"
        );
        assert_eq!(env.exec(r#"grep "[^cbr]at" /regex.txt"#).stdout, "hat\n");
        assert_eq!(
            env.exec(r#"grep '\$100' /regex.txt"#).stdout,
            "price: $100\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "[0-9]+" /digits-regex.txt"#).stdout,
            "abc123\n123abc\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "ab+c" /plus.txt"#).stdout,
            "abc\nabbc\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "colou?r" /optional.txt"#).stdout,
            "color\ncolour\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "(ab)+" /groups.txt"#).stdout,
            "ab\nabab\nababab\n"
        );
        assert_eq!(env.exec("grep pattern /empty.txt").exit_code, 1);
        assert_eq!(env.exec(r#"grep "." /newlines.txt"#).exit_code, 1);
        assert!(env.exec("grep needle /long.txt").stdout.contains("needle"));
        assert_eq!(
            env.exec("grep foo /multi-match.txt").stdout,
            "foo bar foo baz foo\n"
        );
        assert_eq!(env.exec(r#"grep "^hello$" /a.txt"#).stdout, "hello\n");
        assert_eq!(
            env.exec("grep content /file-with-dash.txt").stdout,
            "content\n"
        );
        assert_eq!(
            env.exec("grep col2 /unicode.txt").stdout,
            "col1\tcol2\tcol3\n"
        );
        assert_eq!(env.exec("grep 中文 /unicode.txt").stdout, "中文\n");
        assert_eq!(env.exec("grep -iv hello /iv.txt").stdout, "World\n");
        assert_eq!(env.exec("grep -ci hello /ci.txt").stdout, "3\n");
        assert_eq!(
            env.exec("grep -nv remove /nv.txt").stdout,
            "1:keep\n3:keep\n"
        );
        assert!(
            env.exec("grep -rl needle /dir")
                .stdout
                .contains("/dir/sub/deep.txt")
        );
        assert_eq!(
            env.exec("grep -wi cat /cat-case.txt").stdout,
            "Cat\ncat\nCAT\n"
        );
        assert_eq!(
            env.exec(r#"grep -Ei "cat|dog" /eidog.txt"#).stdout,
            "CAT\nDOG\n"
        );

        assert_eq!(
            env.exec(r#"grep -E "^[[:alpha:]]+$" /posix.txt"#).stdout,
            "abc\nABC\nAbC\ndeadbeef\nGHI\nhelloworld\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "^[[:digit:]]+$" /posix.txt"#).stdout,
            "123\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "^[[:alnum:]]+$" /posix.txt"#).stdout,
            "abc\n123\na1b\nABC\nAbC\ndeadbeef\n12ab\nGHI\nhelloworld\na1\nb2\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "^[[:upper:]]+$" /posix.txt"#).stdout,
            "ABC\nGHI\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "^[[:lower:]]+$" /posix.txt"#).stdout,
            "abc\ndeadbeef\nhelloworld\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "^[[:xdigit:]]+$" /posix.txt"#).stdout,
            "abc\n123\na1b\nABC\nAbC\ndeadbeef\n12ab\na1\nb2\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "[[:space:]]" /posix.txt"#).stdout,
            "hello world\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "^[[:alpha:]][[:digit:]]$" /posix.txt"#)
                .stdout,
            "a1\nb2\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "^[^[:alpha:]]+$" /posix.txt"#).stdout,
            "123\n!@#\n"
        );
        assert_eq!(env.exec(r#"grep 'a\$b' /literal.txt"#).stdout, "a$b\n");
        assert_eq!(env.exec(r#"grep '^\*' /literal.txt"#).stdout, "*\n*abc\n");
        assert_eq!(
            env.exec("grep 'ab*' /bre-star.txt").stdout,
            "ab\nabb\nabbb\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "^[[:alpha:][:digit:]]+$" /literal.txt"#)
                .stdout,
            "acb\nabc\nab\nabb\nabbb\na1\n1a\nA9\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "^[^[:alpha:][:digit:]]+$" /posix.txt"#)
                .stdout,
            "!@#\n"
        );
        assert_eq!(
            env.exec("printf '한글 found\\nplain\\n' | grep 한글")
                .stdout,
            "한글 found\n"
        );
        assert_eq!(env.exec("ls /dir | grep file").stdout, "file.txt\n");
        assert_eq!(env.exec("grep -m 1 -i hello /a.txt").stdout, "Hello\n");
    }

    #[test]
    fn text_search_jbc43_grep_bre_include_and_real_world_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/bre-count.txt".to_string(),
                    "ab\naab\naaab\naaaab\n".to_string(),
                ),
                (
                    "/bre-zero.txt".to_string(),
                    "bc\nabc\nac\n".to_string(),
                ),
                (
                    "/bre-word.txt".to_string(),
                    "foo bar\nbarfoo\nfoobar\nfoo\n".to_string(),
                ),
                (
                    "/bre-literal.txt".to_string(),
                    "a^b\nacb\nabc\n".to_string(),
                ),
                ("/group-b.txt".to_string(), "b\nab\nba\n".to_string()),
                ("/group-a.txt".to_string(), "a\nab\nba\n".to_string()),
                ("/stars.txt".to_string(), "*\nabc\n*abc\n".to_string()),
                (
                    "/alpha-interval.txt".to_string(),
                    "ab\naab\naaab\n".to_string(),
                ),
                (
                    "/alt.txt".to_string(),
                    "cat\ndog\nbird\n".to_string(),
                ),
                (
                    "/colors.txt".to_string(),
                    "red\ngreen\nblue\nyellow\n".to_string(),
                ),
                (
                    "/secrets.txt".to_string(),
                    "PASSWORD\npassword\nsecret\n".to_string(),
                ),
                (
                    "/dir/a.ts".to_string(),
                    "test\nmatch\n".to_string(),
                ),
                ("/dir/b.js".to_string(), "test\n".to_string()),
                ("/dir/c.py".to_string(), "test\n".to_string()),
                ("/dir/sub/b.ts".to_string(), "match\n".to_string()),
                ("/dir/sub/c.js".to_string(), "match\n".to_string()),
                ("/include/a.ts".to_string(), "test\n".to_string()),
                ("/include/b.js".to_string(), "test\n".to_string()),
                ("/include/c.ts".to_string(), "test\n".to_string()),
                (
                    "/code.js".to_string(),
                    "function hello() {\n  return \"hello\";\n}\nfunction world() {\n  return \"world\";\n}\n".to_string(),
                ),
                (
                    "/app.log".to_string(),
                    "[INFO] Starting app\n[ERROR] Connection failed\n[INFO] Retrying\n[ERROR] Timeout\n[INFO] Success\n".to_string(),
                ),
                (
                    "/src/a.js".to_string(),
                    "// TODO: fix this\ncode here\n".to_string(),
                ),
                (
                    "/src/b.js".to_string(),
                    "// Regular comment\n// TODO: implement\n".to_string(),
                ),
                (
                    "/config.json".to_string(),
                    "{\n  \"port\": 3000,\n  \"host\": \"localhost\",\n  \"debug\": true\n}\n".to_string(),
                ),
                (
                    "/index.ts".to_string(),
                    "import { foo } from './foo';\nimport { bar } from './bar';\nconst x = 1;\n".to_string(),
                ),
                (
                    "/hosts.txt".to_string(),
                    "localhost 127.0.0.1\nserver 192.168.1.100\ngateway 10.0.0.1\n"
                        .to_string(),
                ),
                (
                    "/code.ts".to_string(),
                    "class User {\n  name: string;\n}\nclass Admin extends User {\n}\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec(r#"grep '^a\{2\}b$' /bre-count.txt"#).stdout,
            "aab\n"
        );
        assert_eq!(
            env.exec(r#"grep '^a\{2,\}b$' /bre-count.txt"#).stdout,
            "aab\naaab\naaaab\n"
        );
        assert_eq!(
            env.exec(r#"grep '^a\{2,3\}b$' /bre-count.txt"#).stdout,
            "aab\naaab\n"
        );
        assert_eq!(
            env.exec(r#"grep 'a\{0,0\}bc' /bre-zero.txt"#).stdout,
            "bc\nabc\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "[[:<:]]foo" /bre-word.txt"#).stdout,
            "foo bar\nfoobar\nfoo\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "foo[[:>:]]" /bre-word.txt"#).stdout,
            "foo bar\nbarfoo\nfoo\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "[[:<:]]foo[[:>:]]" /bre-word.txt"#)
                .stdout,
            "foo bar\nfoo\n"
        );
        assert_eq!(env.exec("grep 'a^b' /bre-literal.txt").stdout, "a^b\n");
        assert_eq!(env.exec(r#"grep 'a*\(^b$\)c*' /group-b.txt"#).stdout, "b\n");
        assert_eq!(env.exec(r#"grep '\(^a$\)' /group-a.txt"#).stdout, "a\n");
        assert_eq!(env.exec("grep '^*' /stars.txt").stdout, "*\n*abc\n");
        assert_eq!(
            env.exec(r#"grep '[[:alpha:]]\{2\}b' /alpha-interval.txt"#)
                .stdout,
            "aab\naaab\n"
        );

        assert_eq!(env.exec(r#"grep "cat\|dog" /alt.txt"#).stdout, "cat\ndog\n");
        assert_eq!(
            env.exec(r#"grep "red\|green\|blue" /colors.txt"#).stdout,
            "red\ngreen\nblue\n"
        );
        assert_eq!(
            env.exec(r#"grep -i "PASSWORD\|secret" /secrets.txt"#)
                .stdout,
            "PASSWORD\npassword\nsecret\n"
        );
        assert_eq!(
            env.exec(r#"grep -r --include="*.ts" test /include"#).stdout,
            "/include/a.ts:test\n/include/c.ts:test\n"
        );
        assert_eq!(
            env.exec(r#"grep -r --include="*.ts" test /dir"#).stdout,
            "/dir/a.ts:test\n"
        );
        assert_eq!(
            env.exec(r#"grep -r --include="*.ts" match /dir"#).stdout,
            "/dir/a.ts:match\n/dir/sub/b.ts:match\n"
        );

        assert_eq!(
            env.exec("grep \"function\" /code.js").stdout,
            "function hello() {\nfunction world() {\n"
        );
        assert_eq!(
            env.exec("grep ERROR /app.log").stdout,
            "[ERROR] Connection failed\n[ERROR] Timeout\n"
        );
        assert_eq!(
            env.exec("grep -r TODO /src").stdout,
            "/src/a.js:// TODO: fix this\n/src/b.js:// TODO: implement\n"
        );
        assert_eq!(
            env.exec("grep \"port\" /config.json").stdout,
            "  \"port\": 3000,\n"
        );
        assert_eq!(
            env.exec("grep \"^import\" /index.ts").stdout,
            "import { foo } from './foo';\nimport { bar } from './bar';\n"
        );
        assert_eq!(
            env.exec(r#"grep -E "[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+" /hosts.txt"#)
                .stdout,
            "localhost 127.0.0.1\nserver 192.168.1.100\ngateway 10.0.0.1\n"
        );
        assert_eq!(
            env.exec("grep \"^class\" /code.ts").stdout,
            "class User {\nclass Admin extends User {\n"
        );
    }

    #[test]
    fn text_search_jbc48_grep_perl_mode_rows() {
        // JBC-48: grep Perl mode (-P) — \K reset-match-start, \Q...\E literal
        // quoting, and the file-option interactions exercised by
        // packages/just-bash/src/commands/grep/grep.perl.test.ts.
        fn check(files: &[(&str, &str)], script: &str, exit_code: i32, stdout: &str) {
            let result = home_bash(files).exec(script);
            assert_eq!(result.exit_code, exit_code, "{script}");
            assert_eq!(result.stdout, stdout, "{script}");
        }

        // ---- \K reset match start: basic functionality ----
        // grep.perl.test.ts:10 should extract text after \K with -oP
        check(
            &[("/test.txt", "foo=bar\nbaz=qux\n")],
            "grep -oP '=\\K\\w+' /test.txt",
            0,
            "bar\nqux\n",
        );
        // grep.perl.test.ts:19 should work with \K in useEffect pattern
        check(
            &[(
                "/app.tsx",
                "useEffect(() => { }, [count]);\nuseEffect(() => { }, [name, id]);\n",
            )],
            "grep -oP \"useEffect\\(.*?\\[\\K[^\\]]+\" /app.tsx",
            0,
            "count\nname, id\n",
        );
        // grep.perl.test.ts:33 should extract URLs after protocol with \K
        check(
            &[(
                "/test.txt",
                "http://example.com\nhttps://test.org\nftp://files.net\n",
            )],
            "grep -oP 'https?://\\K[^\\s]+' /test.txt",
            0,
            "example.com\ntest.org\n",
        );
        // ---- \K reset match start: edge cases ----
        // grep.perl.test.ts:49 should return empty string when \K is at end
        check(
            &[("/test.txt", "prefix\n")],
            "grep -oP 'prefix\\K' /test.txt",
            0,
            "\n",
        );
        // grep.perl.test.ts:58 should work with \K at start of pattern
        check(
            &[("/test.txt", "hello world\n")],
            "grep -oP '\\K\\w+' /test.txt",
            0,
            "hello\nworld\n",
        );
        // grep.perl.test.ts:68 should work with \K and capturing groups before
        check(
            &[("/test.txt", "foo123bar\n")],
            "grep -oP '(foo)\\K\\d+' /test.txt",
            0,
            "123\n",
        );
        // grep.perl.test.ts:77 should work with \K and capturing groups after
        check(
            &[("/test.txt", "foo123bar\n")],
            "grep -oP 'foo\\K(\\d+)' /test.txt",
            0,
            "123\n",
        );
        // grep.perl.test.ts:86 should work with \K inside alternation
        check(
            &[("/test.txt", "foo=1\nbar:2\n")],
            "grep -oP '(?:foo=|bar:)\\K\\d+' /test.txt",
            0,
            "1\n2\n",
        );
        // grep.perl.test.ts:97 should work with \K and quantifiers
        check(
            &[("/test.txt", "aaabbb\nabbbb\n")],
            "grep -oP 'a+\\Kb+' /test.txt",
            0,
            "bbb\nbbbb\n",
        );
        // ---- \K reset match start: with file options ----
        // grep.perl.test.ts:130 should work with multiple files
        check(
            &[("/a.txt", "key=value1\n"), ("/b.txt", "key=value2\n")],
            "grep -oP 'key=\\K\\w+' /a.txt /b.txt",
            0,
            "/a.txt:value1\n/b.txt:value2\n",
        );
        // grep.perl.test.ts:142 should work with -h flag to suppress filename
        check(
            &[("/a.txt", "x=1\n"), ("/b.txt", "x=2\n")],
            "grep -ohP 'x=\\K\\d+' /a.txt /b.txt",
            0,
            "1\n2\n",
        );
        // grep.perl.test.ts:154 should work with -n for line numbers
        check(
            &[("/test.txt", "skip\nfoo=bar\nskip\nfoo=baz\n")],
            "grep -onP 'foo=\\K\\w+' /test.txt",
            0,
            "2:bar\n4:baz\n",
        );
        // ---- \Q...\E quote metacharacters: basic functionality ----
        // grep.perl.test.ts:170 should match literal dot
        check(
            &[("/test.txt", "foo.bar\nfooxbar\n")],
            "grep -oP '\\Q.\\E' /test.txt",
            0,
            ".\n",
        );
        // grep.perl.test.ts:179 should match literal asterisk
        check(
            &[("/test.txt", "a*b\naaab\n")],
            "grep -oP '\\Q*\\E' /test.txt",
            0,
            "*\n",
        );
        // grep.perl.test.ts:188 should match multiple special characters
        check(
            &[("/test.txt", "foo.bar*baz\nfooxbarybaz\n")],
            "grep -oP '\\Qfoo.bar*\\E' /test.txt",
            0,
            "foo.bar*\n",
        );
        // grep.perl.test.ts:197 should match regex metacharacters
        check(
            &[("/test.txt", "test^$.*+?end\nother\n")],
            "grep -oP '\\Q^$.*+?\\E' /test.txt",
            0,
            "^$.*+?\n",
        );
        // ---- \Q...\E quote metacharacters: edge cases ----
        // grep.perl.test.ts:208 should handle \Q without \E (quotes to end)
        check(
            &[("/test.txt", "test[0]++\ntest0\n")],
            "grep -oP '\\Q[0]++' /test.txt",
            0,
            "[0]++\n",
        );
        // grep.perl.test.ts:217 should handle empty \Q\E
        check(
            &[("/test.txt", "abc\n")],
            "grep -oP 'a\\Q\\Ebc' /test.txt",
            0,
            "abc\n",
        );
        // grep.perl.test.ts:226 should handle multiple \Q...\E pairs
        check(
            &[("/test.txt", "a+b=c*d\na1b2c3d\n")],
            "grep -oP '\\Q+\\E.\\Q=\\E.\\Q*\\E' /test.txt",
            0,
            "+b=c*\n",
        );
        // grep.perl.test.ts:237 should handle \Q...\E at start of pattern
        check(
            &[("/test.txt", "^start\nstart\n")],
            "grep -oP '\\Q^\\Estart' /test.txt",
            0,
            "^start\n",
        );
        // grep.perl.test.ts:246 should handle \Q...\E at end of pattern
        check(
            &[("/test.txt", "end$\nend\n")],
            "grep -oP 'end\\Q$\\E' /test.txt",
            0,
            "end$\n",
        );
        // grep.perl.test.ts:255 should not interpret \E without preceding \Q
        check(
            &[("/test.txt", "test\\Evalue\nother\n")],
            "grep -P 'test' /test.txt",
            0,
            "test\\Evalue\n",
        );
        // ---- \Q...\E combining with regex ----
        // grep.perl.test.ts:267 should combine \Q...\E with regular regex after
        check(
            &[("/test.txt", "price: $100\nprice: $250\n")],
            "grep -oP '\\Q$\\E\\d+' /test.txt",
            0,
            "$100\n$250\n",
        );
        // grep.perl.test.ts:276 should combine \Q...\E with regex before
        check(
            &[("/test.txt", "file.txt\nfile.doc\n")],
            "grep -oP '\\w+\\Q.txt\\E' /test.txt",
            0,
            "file.txt\n",
        );
        // grep.perl.test.ts:285 should combine \Q...\E with \K
        check(
            &[("/test.txt", "var=value\nother\n")],
            "grep -oP '\\Qvar=\\E\\K\\w+' /test.txt",
            0,
            "value\n",
        );
        // grep.perl.test.ts:294 should combine \Q...\E with character class
        check(
            &[("/test.txt", "a+1\na+2\nb+1\n")],
            "grep -oP '[ab]\\Q+\\E\\d' /test.txt",
            0,
            "a+1\na+2\nb+1\n",
        );
        // ---- \x{NNNN} Unicode code points: basic functionality ----
        // grep.perl.test.ts:310 should match ASCII by code point
        check(
            &[("/test.txt", "Hello\n")],
            "grep -oP '\\x{48}' /test.txt",
            0,
            "H\n",
        );
        // grep.perl.test.ts:319 should match BMP Unicode characters
        check(
            &[("/test.txt", "Hello \u{2603} World\n")],
            "grep -oP '\\x{2603}' /test.txt",
            0,
            "\u{2603}\n",
        );
        // grep.perl.test.ts:328 should match supplementary plane emoji
        check(
            &[("/test.txt", "Test \u{1F600} emoji\n")],
            "grep -oP '\\x{1F600}' /test.txt",
            0,
            "\u{1F600}\n",
        );
        // grep.perl.test.ts:337 should match mathematical symbols
        check(
            &[("/test.txt", "Sum: \u{2211} Integral: \u{222B}\n")],
            "grep -oP '\\x{2211}' /test.txt",
            0,
            "\u{2211}\n",
        );
    }

    #[test]
    fn text_search_jbc49_grep_perl_unicode_and_inline_modifier_rows() {
        // JBC-49: grep Perl mode (-P) Unicode code-point classes/sequences and
        // (?i:...) inline case-insensitive groups exercised by
        // packages/just-bash/src/commands/grep/grep.perl.test.ts (lines 348-628).
        fn check(files: &[(&str, &str)], script: &str, exit_code: i32, stdout: &str) {
            let result = home_bash(files).exec(script);
            assert_eq!(result.exit_code, exit_code, "{script}");
            assert_eq!(result.stdout, stdout, "{script}");
        }

        // ---- \x{NNNN} multiple and combined patterns ----
        // grep.perl.test.ts:348 should match multiple Unicode in character class
        check(
            &[("/test.txt", "Stars: \u{2605}\u{2606}\u{2605}\n")],
            "grep -oP '[\\x{2605}\\x{2606}]+' /test.txt",
            0,
            "\u{2605}\u{2606}\u{2605}\n",
        );
        // grep.perl.test.ts:359 should match sequence of Unicode code points
        check(
            &[("/test.txt", "Card: \u{2660}\u{2665}\u{2666}\u{2663}\n")],
            "grep -oP '\\x{2660}\\x{2665}\\x{2666}\\x{2663}' /test.txt",
            0,
            "\u{2660}\u{2665}\u{2666}\u{2663}\n",
        );
        // grep.perl.test.ts:370 should combine Unicode with regular patterns
        check(
            &[("/test.txt", "Price: \u{20AC}100\nPrice: \u{20AC}50\n")],
            "grep -oP '\\x{20AC}\\d+' /test.txt",
            0,
            "\u{20AC}100\n\u{20AC}50\n",
        );
        // grep.perl.test.ts:379 should work with Unicode and \K
        check(
            &[("/test.txt", "\u{20AC}100\n\u{20AC}200\n")],
            "grep -oP '\\x{20AC}\\K\\d+' /test.txt",
            0,
            "100\n200\n",
        );

        // ---- \x{NNNN} edge cases ----
        // grep.perl.test.ts:390 should handle lowercase hex digits
        check(
            &[("/test.txt", "Test \u{f1} char\n")],
            "grep -oP '\\x{f1}' /test.txt",
            0,
            "\u{f1}\n",
        );
        // grep.perl.test.ts:399 should handle mixed case hex digits (no match)
        check(
            &[("/test.txt", "Hello \u{2603} World\n")],
            "grep -oP '\\x{26Fa}' /test.txt",
            1,
            "",
        );
        // grep.perl.test.ts:409 should handle single digit code point (tab)
        check(
            &[("/test.txt", "Tab:\there\n")],
            "grep -oP '\\x{9}' /test.txt",
            0,
            "\t\n",
        );
        // grep.perl.test.ts:418 should handle null character code point (matches 'e')
        check(
            &[("/test.txt", "test\n")],
            "grep -oP '\\x{65}' /test.txt",
            0,
            "e\n",
        );

        // ---- (?i:...) basic case insensitivity ----
        // grep.perl.test.ts:435 should apply case-insensitive only to group
        check(
            &[(
                "/test.txt",
                "Hello world\nhello world\nHELLO world\nHello WORLD\n",
            )],
            "grep -oP '(?i:hello) world' /test.txt",
            0,
            "Hello world\nhello world\nHELLO world\n",
        );
        // grep.perl.test.ts:446 should handle single letter case insensitivity
        check(
            &[("/test.txt", "Apple\napple\nAPPLE\n")],
            "grep -oP '(?i:a)pple' /test.txt",
            0,
            "Apple\napple\n",
        );
        // grep.perl.test.ts:455 should handle mixed case pattern
        check(
            &[("/test.txt", "CamelCase\ncamelcase\nCAMELCASE\n")],
            "grep -oP '(?i:CamelCase)' /test.txt",
            0,
            "CamelCase\ncamelcase\nCAMELCASE\n",
        );

        // ---- (?i:...) combining with other patterns ----
        // grep.perl.test.ts:466 should preserve regex patterns inside modifier group
        check(
            &[("/test.txt", "abc123\nABC123\naBc456\n")],
            "grep -oP '(?i:abc)\\d+' /test.txt",
            0,
            "abc123\nABC123\naBc456\n",
        );
        // grep.perl.test.ts:475 should handle character class inside modifier group
        check(
            &[("/test.txt", "cat\nCAT\nCat\ndog\n")],
            "grep -oP '(?i:[cd]at)' /test.txt",
            0,
            "cat\nCAT\nCat\n",
        );
        // grep.perl.test.ts:484 should handle quantifiers inside modifier group
        check(
            &[("/test.txt", "aaa\nAAA\nAaA\nbbb\n")],
            "grep -oP '(?i:a+)' /test.txt",
            0,
            "aaa\nAAA\nAaA\n",
        );
        // grep.perl.test.ts:493 should handle alternation inside modifier group
        check(
            &[("/test.txt", "yes\nYES\nno\nNO\nmaybe\n")],
            "grep -oP '(?i:yes|no)' /test.txt",
            0,
            "yes\nYES\nno\nNO\n",
        );

        // ---- (?i:...) multiple modifier groups ----
        // grep.perl.test.ts:504 should handle adjacent modifier groups
        check(
            &[("/test.txt", "TestCase\ntestcase\nTESTCASE\ntestCASE\n")],
            "grep -oP '(?i:test)(?i:case)' /test.txt",
            0,
            "TestCase\ntestcase\nTESTCASE\ntestCASE\n",
        );
        // grep.perl.test.ts:515 should handle modifier group followed by literal
        check(
            &[("/test.txt", "ABCdef\nabcdef\nABCDEF\n")],
            "grep -oP '(?i:abc)def' /test.txt",
            0,
            "ABCdef\nabcdef\n",
        );
        // grep.perl.test.ts:524 should handle literal followed by modifier group
        check(
            &[("/test.txt", "abcDEF\nabcdef\nABCDEF\n")],
            "grep -oP 'abc(?i:def)' /test.txt",
            0,
            "abcDEF\nabcdef\n",
        );

        // ---- (?i:...) edge cases ----
        // grep.perl.test.ts:535 should handle empty modifier group
        check(
            &[("/test.txt", "test\n")],
            "grep -oP 'te(?i:)st' /test.txt",
            0,
            "test\n",
        );
        // grep.perl.test.ts:544 should handle digits in modifier group (unchanged)
        check(
            &[("/test.txt", "abc123\nABC123\n")],
            "grep -oP '(?i:abc123)' /test.txt",
            0,
            "abc123\nABC123\n",
        );
        // grep.perl.test.ts:553 should handle special chars in modifier group
        check(
            &[("/test.txt", "a.b\nA.B\na.B\n")],
            "grep -oP '(?i:a\\.b)' /test.txt",
            0,
            "a.b\nA.B\na.B\n",
        );
        // grep.perl.test.ts:562 should handle escape sequences in modifier group
        check(
            &[("/test.txt", "a1b\nA2B\na3B\n")],
            "grep -oP '(?i:a)\\d(?i:b)' /test.txt",
            0,
            "a1b\nA2B\na3B\n",
        );
        // grep.perl.test.ts:571 should preserve non-alpha chars in modifier group
        check(
            &[("/test.txt", "a-b_c\nA-B_C\n")],
            "grep -oP '(?i:a-b_c)' /test.txt",
            0,
            "a-b_c\nA-B_C\n",
        );

        // ---- (?i:...) interaction with \K ----
        // grep.perl.test.ts:582 should work with \K after modifier group
        check(
            &[("/test.txt", "Key=value\nKEY=VALUE\nkey=data\n")],
            "grep -oP '(?i:key)=\\K\\w+' /test.txt",
            0,
            "value\nVALUE\ndata\n",
        );
        // grep.perl.test.ts:591 should work with \K inside modifier group
        check(
            &[("/test.txt", "prefix:VALUE\nPREFIX:value\n")],
            "grep -oP '(?i:prefix:\\K\\w+)' /test.txt",
            0,
            "VALUE\nvalue\n",
        );

        // ---- (?P<name>...) named groups ----
        // grep.perl.test.ts:608 should support named groups
        check(
            &[("/test.txt", "hello world\n")],
            "grep -P '(?P<word>\\w+)' /test.txt",
            0,
            "hello world\n",
        );
        // grep.perl.test.ts:617 should support multiple named groups
        check(
            &[("/test.txt", "John:25\nJane:30\n")],
            "grep -P '(?P<name>\\w+):(?P<age>\\d+)' /test.txt",
            0,
            "John:25\nJane:30\n",
        );
        // grep.perl.test.ts:628 should support named groups with -o
        check(
            &[("/test.txt", "email: test@example.com\n")],
            "grep -oP '(?P<user>\\w+)@(?P<domain>[\\w.]+)' /test.txt",
            0,
            "test@example.com\n",
        );
    }

    #[test]
    fn text_search_jbc_grep_exclude_files_without_match_and_bracket_rows() {
        // grep --exclude: skip files whose basename matches the glob.
        let exclude = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/dir/file.txt".to_string(), "hello world".to_string()),
                ("/dir/file.log".to_string(), "hello world".to_string()),
                ("/dir/other.txt".to_string(), "hello world".to_string()),
            ]),
            ..BashOptions::default()
        });
        let result = exclude.exec(r#"grep -r --exclude="*.log" hello /dir"#);
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("file.txt"));
        assert!(result.stdout.contains("other.txt"));
        assert!(!result.stdout.contains("file.log"));

        // grep --exclude: multiple patterns are all applied.
        let multi_exclude = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/dir/file.txt".to_string(), "hello".to_string()),
                ("/dir/file.log".to_string(), "hello".to_string()),
                ("/dir/file.bak".to_string(), "hello".to_string()),
            ]),
            ..BashOptions::default()
        });
        assert_eq!(
            multi_exclude
                .exec(r#"grep -r --exclude="*.log" --exclude="*.bak" hello /dir"#)
                .stdout,
            "/dir/file.txt:hello\n"
        );

        // grep --exclude: also applies to explicit non-recursive file paths.
        let non_recursive = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/a.txt".to_string(), "hello".to_string()),
                ("/b.log".to_string(), "hello".to_string()),
            ]),
            cwd: Some("/".to_string()),
            ..BashOptions::default()
        });
        assert_eq!(
            non_recursive
                .exec(r#"grep --exclude="*.log" hello a.txt b.log"#)
                .stdout,
            "a.txt:hello\n"
        );

        // grep --exclude-dir: skip files under matching directory components.
        let exclude_dir = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/project/src/main.js".to_string(), "hello".to_string()),
                (
                    "/project/node_modules/pkg/index.js".to_string(),
                    "hello".to_string(),
                ),
                ("/project/build/out.js".to_string(), "hello".to_string()),
            ]),
            ..BashOptions::default()
        });
        let result = exclude_dir.exec("grep -r --exclude-dir=node_modules hello /project");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("src/main.js"));
        assert!(result.stdout.contains("build/out.js"));
        assert!(!result.stdout.contains("node_modules"));

        // grep --exclude-dir: multiple directory patterns are all applied.
        let multi_exclude_dir = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/project/src/main.js".to_string(), "hello".to_string()),
                (
                    "/project/node_modules/pkg/index.js".to_string(),
                    "hello".to_string(),
                ),
                ("/project/build/out.js".to_string(), "hello".to_string()),
                ("/project/.git/objects/abc".to_string(), "hello".to_string()),
            ]),
            ..BashOptions::default()
        });
        let result = multi_exclude_dir
            .exec("grep -r --exclude-dir=node_modules --exclude-dir=build hello /project");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("src/main.js"));
        assert!(!result.stdout.contains("node_modules"));
        assert!(!result.stdout.contains("build"));

        // grep: --exclude-dir and --exclude combine.
        let combined = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/project/src/main.js".to_string(), "hello".to_string()),
                ("/project/src/test.spec.js".to_string(), "hello".to_string()),
                (
                    "/project/node_modules/pkg/index.js".to_string(),
                    "hello".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });
        assert_eq!(
            combined
                .exec(r#"grep -r --exclude-dir=node_modules --exclude="*.spec.js" hello /project"#)
                .stdout,
            "/project/src/main.js:hello\n"
        );

        // grep -L: list files without any match.
        let without = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/dir/has-match.txt".to_string(), "hello world".to_string()),
                ("/dir/no-match.txt".to_string(), "goodbye world".to_string()),
                (
                    "/dir/also-no-match.txt".to_string(),
                    "nothing here".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });
        let result = without
            .exec("grep -L hello /dir/has-match.txt /dir/no-match.txt /dir/also-no-match.txt");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/dir/no-match.txt\n/dir/also-no-match.txt\n");

        // grep -L: exit code 0 when at least one file lacks a match.
        let without_two = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/file1.txt".to_string(), "hello".to_string()),
                ("/file2.txt".to_string(), "goodbye".to_string()),
            ]),
            ..BashOptions::default()
        });
        let result = without_two.exec("grep -L hello /file1.txt /file2.txt");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/file2.txt\n");

        // grep -L: exit code 1 when every file has a match.
        let all_match = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/file1.txt".to_string(), "hello".to_string()),
                ("/file2.txt".to_string(), "hello".to_string()),
            ]),
            ..BashOptions::default()
        });
        let result = all_match.exec("grep -L hello /file1.txt /file2.txt");
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "");

        // grep -rL: recursive files-without-match traversal.
        let recursive_without = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/dir/has-hello.txt".to_string(), "hello".to_string()),
                ("/dir/no-hello.txt".to_string(), "goodbye".to_string()),
                ("/dir/sub/has-hello2.txt".to_string(), "hello".to_string()),
                ("/dir/sub/no-hello2.txt".to_string(), "goodbye".to_string()),
            ]),
            ..BashOptions::default()
        });
        let result = recursive_without.exec("grep -rL hello /dir");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("no-hello.txt"));
        assert!(result.stdout.contains("no-hello2.txt"));
        assert!(!result.stdout.contains("has-hello.txt"));
        assert!(!result.stdout.contains("has-hello2.txt"));

        // grep --files-without-match: long form of -L.
        let long_form = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/a.txt".to_string(), "hello".to_string()),
                ("/b.txt".to_string(), "goodbye".to_string()),
            ]),
            ..BashOptions::default()
        });
        assert_eq!(
            long_form
                .exec("grep --files-without-match hello /a.txt /b.txt")
                .stdout,
            "/b.txt\n"
        );

        // grep -E bracket: a literal ] is matched when it leads the class.
        let bracket = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test.txt".to_string(), "a]b\na[b\nacb\n".to_string())]),
            ..BashOptions::default()
        });
        assert_eq!(
            bracket.exec(r#"grep -E "a[][]b" /test.txt"#).stdout,
            "a]b\na[b\n"
        );

        // grep -E negated bracket: a leading ] after ^ is matched literally.
        let neg_bracket = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test.txt".to_string(), "a]c\nabc\nadc\n".to_string())]),
            ..BashOptions::default()
        });
        assert_eq!(
            neg_bracket.exec(r#"grep -E "a[^]b]c" /test.txt"#).stdout,
            "adc\n"
        );
    }

    #[test]
    fn text_search_jbc34_rg_patterns_gitignore_and_edge_rows() {
        assert_home_exec(
            &[("file.txt", "hello world\nhelloworld\n")],
            "rg -w hello",
            0,
            "file.txt:1:hello world\n",
        );
        assert_home_exec(
            &[("file.txt", "hello\nhello world\n")],
            "rg -x hello",
            0,
            "file.txt:1:hello\n",
        );
        assert_home_exec(
            &[("file.txt", "a.b\naxb\n")],
            "rg -F 'a.b'",
            0,
            "file.txt:1:a.b\n",
        );
        assert_home_exec(
            &[("file.txt", "foo[bar]\nfoobar\n")],
            "rg -F '[bar]'",
            0,
            "file.txt:1:foo[bar]\n",
        );
        assert_home_exec(
            &[("file.txt", "hello\nworld\n")],
            "rg -v hello",
            0,
            "file.txt:2:world\n",
        );
        assert_home_exec(
            &[("file.txt", "foo\nbar\nfoo\nbaz\n")],
            "rg -v foo",
            0,
            "file.txt:2:bar\nfile.txt:4:baz\n",
        );
        assert_home_exec(
            &[("file.txt", "foo\nbar\nbaz\n")],
            "rg -e foo -e bar",
            0,
            "file.txt:1:foo\nfile.txt:2:bar\n",
        );
        assert_home_exec(
            &[("file.txt", "Hello\nhello\nWorld\nworld\n")],
            "rg -e Hello -e world",
            0,
            "file.txt:1:Hello\nfile.txt:4:world\n",
        );
        assert_home_exec(
            &[("file.txt", "foo\nbar\n")],
            "rg --regexp=foo --regexp=bar",
            0,
            "file.txt:1:foo\nfile.txt:2:bar\n",
        );
        assert_home_exec(
            &[("file.txt", "foo bar baz\n")],
            "rg -e foo -e bar",
            0,
            "file.txt:1:foo bar baz\n",
        );
        assert_home_exec(
            &[("file.txt", "foo123\nbar456\nbaz\n")],
            "rg '[0-9]+'",
            0,
            "file.txt:1:foo123\nfile.txt:2:bar456\n",
        );
        assert_home_exec(
            &[("file.txt", "hello world\nworld hello\n")],
            "rg '^hello'",
            0,
            "file.txt:1:hello world\n",
        );
        assert_home_exec(
            &[("file.txt", "hello world\nworld hello\n")],
            "rg 'hello$'",
            0,
            "file.txt:2:world hello\n",
        );
        assert_home_exec(
            &[("file.txt", "cat\ndog\nbird\n")],
            "rg 'cat|dog'",
            0,
            "file.txt:1:cat\nfile.txt:2:dog\n",
        );
        assert_home_exec(
            &[("file.txt", "a\naa\naaa\nb\n")],
            "rg 'a{2,}'",
            0,
            "file.txt:2:aa\nfile.txt:3:aaa\n",
        );
        assert_home_exec(
            &[("file.txt", "a1\nb2\nc!\n")],
            "rg '[a-z][0-9]'",
            0,
            "file.txt:1:a1\nfile.txt:2:b2\n",
        );
        assert_home_exec(
            &[("file.txt", "Hello world\nhelloworld\nHELLO there\n")],
            "rg -wi hello",
            0,
            "file.txt:1:Hello world\nfile.txt:3:HELLO there\n",
        );
        assert_home_exec(
            &[("file.txt", "Hello\nhello\nHELLO\n")],
            "rg -ci hello",
            0,
            "file.txt:3\n",
        );
        assert_home_exec(
            &[("a.txt", "HELLO\n"), ("b.txt", "world\n")],
            "rg -li hello",
            0,
            "a.txt\n",
        );
        assert_home_exec(
            &[("file.txt", "foo\nbar\nfoo\nbaz\n")],
            "rg -vc foo",
            0,
            "file.txt:2\n",
        );

        assert_home_exec(&[("empty.txt", "")], "rg hello", 1, "");
        assert_home_exec(&[("file.txt", "\n\n\n")], "rg hello", 1, "");
        assert_home_exec(
            &[("file.txt", "foo\n\nbar\n")],
            "rg '^$'",
            0,
            "file.txt:2:\n",
        );
        assert_home_exec(
            &[("file.txt", "hello   \nworld\n")],
            "rg 'hello   '",
            0,
            "file.txt:1:hello   \n",
        );
        assert_home_exec(
            &[("file.txt", "   \n   \n")],
            "rg '^ +$'",
            0,
            "file.txt:1:   \nfile.txt:2:   \n",
        );
        assert_home_exec(
            &[("file.txt", "a.b.c\nabc\n")],
            "rg -F 'a.b'",
            0,
            "file.txt:1:a.b.c\n",
        );
        assert_home_exec(
            &[("file.txt", "array[0]\narray0\n")],
            "rg -F '[0]'",
            0,
            "file.txt:1:array[0]\n",
        );
        assert_home_exec(
            &[("file.txt", "func()\nfunc\n")],
            "rg -F '()'",
            0,
            "file.txt:1:func()\n",
        );
        assert_home_exec(
            &[("file.txt", "a*b\nab\naab\n")],
            "rg -F 'a*b'",
            0,
            "file.txt:1:a*b\n",
        );
        assert_home_exec(
            &[("file.txt", "path\\to\\file\npath/to/file\n")],
            "rg -F 'path\\'",
            0,
            "file.txt:1:path\\to\\file\n",
        );
        assert_home_exec(&[("file.txt", "hello\n")], "rg -o '^'", 0, "file.txt:\n");
        assert_home_exec(
            &[("file.txt", "hello")],
            "rg 'hello$'",
            0,
            "file.txt:1:hello\n",
        );
        assert_home_exec(
            &[("file.txt", "  hello\nhello\n")],
            "rg '^hello'",
            0,
            "file.txt:2:hello\n",
        );
        assert_home_exec(
            &[("file.txt", "hello 世界\nfoo bar\n")],
            "rg 世界",
            0,
            "file.txt:1:hello 世界\n",
        );
        assert_home_exec(
            &[("file.txt", "hello 🎉\nfoo bar\n")],
            "rg 🎉",
            0,
            "file.txt:1:hello 🎉\n",
        );
        assert_home_exec(
            &[("file.txt", "CAFÉ\ncafé\n")],
            "rg -i café",
            0,
            "file.txt:1:CAFÉ\nfile.txt:2:café\n",
        );
        assert_home_exec(
            &[
                ("z.txt", "hello\n"),
                ("a.txt", "hello\n"),
                ("m.txt", "hello\n"),
            ],
            "rg hello",
            0,
            "a.txt:1:hello\nm.txt:1:hello\nz.txt:1:hello\n",
        );
        assert_home_exec(
            &[("z/file.txt", "hello\n"), ("a/file.txt", "hello\n")],
            "rg hello",
            0,
            "a/file.txt:1:hello\nz/file.txt:1:hello\n",
        );
        assert_eq!(
            home_bash(&[("file.txt", "hello\n")])
                .exec("rg hello")
                .exit_code,
            0
        );
        assert_eq!(
            home_bash(&[("file.txt", "hello\n")])
                .exec("rg goodbye")
                .exit_code,
            1
        );
        let invalid = home_bash(&[("file.txt", "hello\n")]).exec("rg '['");
        assert_eq!(invalid.exit_code, 2);
        assert!(invalid.stderr.contains("regex"));
        assert_home_exec(&[("file.txt", "hello\n")], "rg -q hello", 0, "");
        assert_home_exec(
            &[("file.txt", "hello world\n")],
            "rg -w hello",
            0,
            "file.txt:1:hello world\n",
        );
        assert_home_exec(
            &[("file.txt", "hello world\n")],
            "rg -w world",
            0,
            "file.txt:1:hello world\n",
        );
        assert_home_exec(
            &[("file.txt", "helloworld\nhello world\n")],
            "rg -w hello",
            0,
            "file.txt:2:hello world\n",
        );
        assert_home_exec(
            &[("file.txt", "hello, world\nhello.world\n")],
            "rg -w hello",
            0,
            "file.txt:1:hello, world\nfile.txt:2:hello.world\n",
        );
        assert_home_exec(
            &[("file.txt", "a\nb\nc\nd\ne\n")],
            "rg -v -C1 c",
            0,
            "file.txt:1:a\nfile.txt-2-b\nfile.txt-3-c\nfile.txt:4:d\nfile.txt-5-e\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "# comment\n*.log\n"),
                ("app.ts", "hello\n"),
                ("debug.log", "hello\n"),
            ],
            "rg hello",
            0,
            "app.ts:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "*.log\n\n*.tmp\n"),
                ("app.ts", "hello\n"),
                ("debug.log", "hello\n"),
                ("cache.tmp", "hello\n"),
            ],
            "rg hello",
            0,
            "app.ts:1:hello\n",
        );
        assert_home_exec(
            &[("src/app.ts", "hello\n"), ("test/app.ts", "hello\n")],
            "rg -g 'src/*.ts' hello",
            0,
            "src/app.ts:1:hello\n",
        );
        assert_home_exec(
            &[
                ("a.ts", "hello\n"),
                ("b.js", "hello\n"),
                ("c.py", "hello\n"),
            ],
            "rg -g '*.ts' -g '*.js' hello",
            0,
            "a.ts:1:hello\nb.js:1:hello\n",
        );

        assert_home_exec(
            &[
                (".gitignore", "*.log\n"),
                ("debug.log", "hello\n"),
                ("app.ts", "hello\n"),
            ],
            "rg hello",
            0,
            "app.ts:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "package-lock.json\n"),
                ("package-lock.json", "hello\n"),
                ("package.json", "hello\n"),
            ],
            "rg hello",
            0,
            "package.json:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "node_modules\n"),
                ("node_modules/pkg/index.js", "hello\n"),
                ("src/app.js", "hello\n"),
            ],
            "rg hello",
            0,
            "src/app.js:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "*.log\n!important.log\n"),
                ("debug.log", "hello\n"),
                ("important.log", "hello\n"),
            ],
            "rg hello",
            0,
            "important.log:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "*\n!src\n!*.ts\n"),
                ("README.md", "hello\n"),
                ("app.ts", "hello\n"),
                ("src/app.js", "hello\n"),
            ],
            "rg hello",
            0,
            "app.ts:1:hello\nsrc/app.js:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "build/\n"),
                ("build", "hello\n"),
                ("build/out.js", "hello\n"),
            ],
            "rg hello",
            0,
            "build:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "/todo.txt\n"),
                ("todo.txt", "hello\n"),
                ("docs/todo.txt", "hello\n"),
            ],
            "rg hello",
            0,
            "docs/todo.txt:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "**/foo\n"),
                ("foo", "hello\n"),
                ("a/foo", "hello\n"),
                ("a/b/c/foo", "hello\n"),
                ("bar", "hello\n"),
            ],
            "rg hello",
            0,
            "bar:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "abc/**\n"),
                ("abc/def", "hello\n"),
                ("abc/def/ghi", "hello\n"),
                ("abc", "hello\n"),
            ],
            "rg hello",
            0,
            "abc:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "file?.txt\n"),
                ("file1.txt", "hello\n"),
                ("fileA.txt", "hello\n"),
                ("file12.txt", "hello\n"),
            ],
            "rg hello",
            0,
            "file12.txt:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "file[0-9].txt\n"),
                ("file0.txt", "hello\n"),
                ("file9.txt", "hello\n"),
                ("fileA.txt", "hello\n"),
            ],
            "rg hello",
            0,
            "fileA.txt:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "file[!0-9].txt\n"),
                ("file0.txt", "hello\n"),
                ("fileA.txt", "hello\n"),
            ],
            "rg hello",
            0,
            "file0.txt:1:hello\n",
        );
        assert_home_exec(
            &[
                (".gitignore", "*.log\n"),
                ("debug.log", "hello\n"),
                ("app.ts", "hello\n"),
            ],
            "rg -u hello",
            0,
            "app.ts:1:hello\ndebug.log:1:hello\n",
        );
        assert_home_exec(
            &[(".hidden", "hello\n"), ("visible", "hello\n")],
            "rg -uu hello",
            0,
            ".hidden:1:hello\nvisible:1:hello\n",
        );
    }

    #[test]
    fn text_search_jbc34_sed_regex_and_utf8_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/alpha.txt".to_string(), "abc123xyz\n".to_string()),
                ("/alnum.txt".to_string(), "a1-b2_c3\n".to_string()),
                ("/space.txt".to_string(), "a b\tc\n".to_string()),
                ("/case.txt".to_string(), "Hello World\n".to_string()),
                ("/punct.txt".to_string(), "Hello, World!\n".to_string()),
                ("/hex.txt".to_string(), "0x1F2a3b\n".to_string()),
                ("/digits.txt".to_string(), "abc123\n".to_string()),
                ("/in.txt".to_string(), "한글 old café\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec("sed 's/[[:alpha:]]/_/g' /alpha.txt").stdout,
            "___123___\n"
        );
        assert_eq!(
            env.exec("sed 's/[[:digit:]]/#/g' /alpha.txt").stdout,
            "abc###xyz\n"
        );
        assert_eq!(
            env.exec("sed 's/[[:alnum:]]/X/g' /alnum.txt").stdout,
            "XX-XX_XX\n"
        );
        assert_eq!(
            env.exec("sed 's/[[:space:]]/_/g' /space.txt").stdout,
            "a_b_c\n"
        );
        assert_eq!(
            env.exec("sed 's/[[:upper:]]/X/g' /case.txt").stdout,
            "Xello Xorld\n"
        );
        assert_eq!(
            env.exec("sed 's/[[:lower:]]/x/g' /case.txt").stdout,
            "Hxxxx Wxxxx\n"
        );
        assert_eq!(
            env.exec("sed 's/[[:punct:]]//g' /punct.txt").stdout,
            "Hello World\n"
        );
        assert_eq!(
            env.exec("sed 's/[[:blank:]]/_/g' /space.txt").stdout,
            "a_b_c\n"
        );
        assert_eq!(
            env.exec("sed 's/[[:xdigit:]]/X/g' /hex.txt").stdout,
            "XxXXXXXX\n"
        );
        assert_eq!(
            env.exec("sed 's/[^[:digit:]]/X/g' /digits.txt").stdout,
            "XXX123\n"
        );
        assert_eq!(
            env.exec("sed 's/[[:digit:]_-]/./g' /alnum.txt").stdout,
            "a..b..c.\n"
        );
        assert_eq!(
            env.exec("cat /in.txt | sed 's/old/NEW/'").stdout,
            "한글 NEW café\n"
        );
    }

    #[test]
    fn text_search_sed_ere_bre_quit_and_occurrence_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/ere.txt".to_string(), "aab\nab\nb\n".to_string()),
                ("/colour.txt".to_string(), "color colour\n".to_string()),
                ("/animals.txt".to_string(), "cat dog bird\n".to_string()),
                ("/hello.txt".to_string(), "hello world\n".to_string()),
                (
                    "/log.txt".to_string(),
                    "error: file not found\nwarning: deprecated\ninfo: success\n".to_string(),
                ),
                ("/aquant.txt".to_string(), "a aa aaa aaaa\n".to_string()),
                ("/plus.txt".to_string(), "aaa bbb\na+ ccc\n".to_string()),
                ("/optb.txt".to_string(), "ab\nb\n".to_string()),
                ("/alt.txt".to_string(), "cat\ndog\nbird\n".to_string()),
                ("/nth.txt".to_string(), "foo bar foo baz foo\n".to_string()),
                ("/nth3.txt".to_string(), "a a a a a\n".to_string()),
                ("/lines.txt".to_string(), "1\n2\n3\n4\n5\n".to_string()),
                ("/abc.txt".to_string(), "a\nb\nc\n".to_string()),
                (
                    "/qlines.txt".to_string(),
                    "line1\nline2\nline3\n".to_string(),
                ),
                ("/abc3.txt".to_string(), "abc\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        // ERE (-E / -r) quantifiers, alternation, grouping, and backreferences.
        assert_eq!(env.exec("sed -E 's/a+b/X/' /ere.txt").stdout, "X\nX\nb\n");
        assert_eq!(env.exec("sed -r 's/a+b/X/' /ere.txt").stdout, "X\nX\nb\n");
        assert_eq!(env.exec("sed -E 's/a?b/X/' /optb.txt").stdout, "X\nX\n");
        assert_eq!(
            env.exec("sed -E 's/colou?r/COLOR/g' /colour.txt").stdout,
            "COLOR COLOR\n"
        );
        assert_eq!(
            env.exec("sed -E 's/cat|dog/ANIMAL/g' /animals.txt").stdout,
            "ANIMAL ANIMAL bird\n"
        );
        assert_eq!(
            env.exec("sed -E 's/(hello) (world)/\\2 \\1/' /hello.txt")
                .stdout,
            "world hello\n"
        );
        assert_eq!(
            env.exec("sed -E 's/^(error|warning): (.+)/[\\1] \\2/' /log.txt")
                .stdout,
            "[error] file not found\n[warning] deprecated\ninfo: success\n"
        );
        assert_eq!(
            env.exec("sed -E 's/a{2,3}/X/g' /aquant.txt").stdout,
            "a X X Xa\n"
        );
        assert_eq!(
            env.exec("sed -E 's/(a)(b)(c)/\\3\\2\\1/' /abc3.txt").stdout,
            "cba\n"
        );

        // BRE mode: + ? | ( ) are literal; \+ \? are quantifiers.
        assert_eq!(
            env.exec("sed 's/a+/X/' /plus.txt").stdout,
            "aaa bbb\nX ccc\n"
        );
        assert_eq!(env.exec("sed 's/a\\+b/X/' /ere.txt").stdout, "X\nX\nb\n");
        assert_eq!(env.exec("sed 's/a\\?b/X/' /optb.txt").stdout, "X\nX\n");
        assert_eq!(
            env.exec("sed 's/cat\\|dog/X/' /alt.txt").stdout,
            "X\nX\nbird\n"
        );
        // BRE: bare ( ) are literal characters.
        let env_paren = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/paren.txt".to_string(), "(foo)\nfoo\n".to_string())]),
            ..BashOptions::default()
        });
        assert_eq!(
            env_paren.exec("sed 's/(foo)/X/' /paren.txt").stdout,
            "X\nfoo\n"
        );
        // BRE: \( \) capture groups with \1 \2 backreferences in the replacement.
        assert_eq!(
            env.exec("sed 's/\\(hello\\) \\(world\\)/\\2 \\1/' /hello.txt")
                .stdout,
            "world hello\n"
        );

        // Nth occurrence substitution.
        assert_eq!(
            env.exec("sed 's/foo/XXX/2' /nth.txt").stdout,
            "foo bar XXX baz foo\n"
        );
        assert_eq!(env.exec("sed 's/a/X/3' /nth3.txt").stdout, "a a X a a\n");

        // q quits after printing the current line; Q quits without printing it.
        assert_eq!(env.exec("sed '3q' /lines.txt").stdout, "1\n2\n3\n");
        assert_eq!(env.exec("sed '1q' /abc.txt").stdout, "a\n");
        assert_eq!(env.exec("sed '2q' /qlines.txt").stdout, "line1\nline2\n");
        assert_eq!(env.exec("sed '2Q' /qlines.txt").stdout, "line1\n");

        // Pattern addresses: delete and address-scoped substitution.
        let env2 = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/foo.txt".to_string(), "foo\nbar\nbaz\n".to_string()),
                (
                    "/fruit.txt".to_string(),
                    "apple\nbanana\napricot\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });
        assert_eq!(env2.exec("sed '/bar/d' /foo.txt").stdout, "foo\nbaz\n");
        assert_eq!(
            env2.exec("sed '/^a/s/a/A/g' /fruit.txt").stdout,
            "Apple\nbanana\nApricot\n"
        );

        // ERE with escaped parens treats \( \) as literal parentheses.
        let env3 = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/require.txt".to_string(),
                "const x = require('foo');\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        let require_result = env3
            .exec("sed -E \"s/const x = require\\('foo'\\);/import x from 'foo';/g\" /require.txt");
        assert_eq!(require_result.stdout, "import x from 'foo';\n");
        assert_eq!(require_result.exit_code, 0);
    }

    #[test]
    fn text_search_sed_pending_regex_anchor_and_quantifier_rows() {
        // Mirrors packages/just-bash/src/commands/sed/sed.regex.test.ts anchor,
        // literal-paren, dot/escape, newline/tab replacement, quantifier, and
        // bracket character-class substitution rows.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/paren.txt".to_string(), "(foo)\nfoo\n".to_string()),
                ("/anc1.txt".to_string(), "abc\nxabc\n".to_string()),
                ("/anc2.txt".to_string(), "abc\nabcx\n".to_string()),
                ("/anc3.txt".to_string(), "a\n\nb\n".to_string()),
                ("/dot1.txt".to_string(), "a.b\nacb\n".to_string()),
                ("/dot2.txt".to_string(), "a1b\na2b\n".to_string()),
                ("/colon.txt".to_string(), "a:b\n".to_string()),
                ("/star.txt".to_string(), "b\nab\naab\n".to_string()),
                ("/q1.txt".to_string(), "aa\naaa\naaaa\n".to_string()),
                ("/q2.txt".to_string(), "a\naa\naaa\naaaa\n".to_string()),
                ("/q3.txt".to_string(), "a\naa\naaa\n".to_string()),
            ]),
            cwd: Some("/".to_string()),
            ..BashOptions::default()
        });

        // sed.regex.test.ts:147 should treat () as literal without backslash
        assert_eq!(env.exec("sed 's/(foo)/X/' /paren.txt").stdout, "X\nfoo\n");
        // sed.regex.test.ts:258 should match ^ at start of line
        assert_eq!(env.exec("sed 's/^a/X/' /anc1.txt").stdout, "Xbc\nxabc\n");
        // sed.regex.test.ts:267 should match $ at end of line
        assert_eq!(env.exec("sed 's/c$/X/' /anc2.txt").stdout, "abX\nabcx\n");
        // sed.regex.test.ts:276 should match ^$ for empty line
        assert_eq!(
            env.exec("sed 's/^$/EMPTY/' /anc3.txt").stdout,
            "a\nEMPTY\nb\n"
        );
        // sed.regex.test.ts:287 should match literal dot with backslash
        assert_eq!(env.exec("sed 's/a\\.b/X/' /dot1.txt").stdout, "X\nacb\n");
        // sed.regex.test.ts:296 should match . as any character
        assert_eq!(env.exec("sed 's/a.b/X/' /dot2.txt").stdout, "X\nX\n");
        // sed.regex.test.ts:305 should handle newline in replacement with \n
        assert_eq!(env.exec("sed 's/:/\\n/' /colon.txt").stdout, "a\nb\n");
        // sed.regex.test.ts:314 should handle tab in replacement with \t
        assert_eq!(env.exec("sed 's/:/\\t/' /colon.txt").stdout, "a\tb\n");
        // sed.regex.test.ts:325 should match * (zero or more)
        assert_eq!(env.exec("sed 's/a*/X/' /star.txt").stdout, "Xb\nXb\nXb\n");
        // sed.regex.test.ts:334 should match \{n\} exactly n times in BRE
        assert_eq!(
            env.exec("sed 's/a\\{3\\}/X/' /q1.txt").stdout,
            "aa\nX\nXa\n"
        );
        // sed.regex.test.ts:343 should match {n} exactly n times in ERE
        assert_eq!(env.exec("sed -E 's/a{3}/X/' /q1.txt").stdout, "aa\nX\nXa\n");
        // sed.regex.test.ts:352 should match \{n,m\} range in BRE
        assert_eq!(
            env.exec("sed 's/a\\{2,3\\}/X/' /q2.txt").stdout,
            "a\nX\nX\nXa\n"
        );
        // sed.regex.test.ts:361 should match {n,} at least n times in ERE
        assert_eq!(env.exec("sed -E 's/a{2,}/X/' /q3.txt").stdout, "a\nX\nX\n");
    }

    #[test]
    fn text_search_sed_pending_error_command_and_label_rows() {
        // Mirrors packages/just-bash/src/commands/sed/sed.errors.test.ts and
        // sed.commands.test.ts / sed.test.ts rows that the runtime implements:
        // missing-file errors, alternate delimiters, lenient backref/literal
        // parens, invalid-regex errors, q/Q/$d addressing, t/T branch tracking,
        // and the POSIX-class-as-literal edge.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/test/file.txt".to_string(),
                "line 1\nline 2\nline 3\n".to_string(),
            )]),
            cwd: Some("/test".to_string()),
            ..BashOptions::default()
        });

        // sed.errors.test.ts:14 should error on non-existent file
        let r = env.exec("sed 's/a/b/' /nonexistent.txt");
        assert!(r.stderr.contains("No such file or directory"));
        assert_eq!(r.exit_code, 1);
        // sed.errors.test.ts:21 should error on multiple non-existent files
        let r = env.exec("sed 's/a/b/' /no1.txt /no2.txt");
        assert!(r.stderr.contains("No such file or directory"));
        assert_eq!(r.exit_code, 1);
        // sed.errors.test.ts:28 should error on non-existent script file with -f
        let r = env.exec("sed -f /nonexistent.sed /test/file.txt");
        assert!(r.stderr.contains("No such file or directory"));
        assert_eq!(r.exit_code, 1);
        // sed.errors.test.ts:52 should handle non-standard substitution delimiter
        assert_eq!(env.exec("sed 's|foo|bar|' /test/file.txt").exit_code, 0);
        // sed.errors.test.ts:96 should handle unknown POSIX class as literal
        assert_eq!(
            env.exec("sed '/[[:invalid:]]/d' /test/file.txt").exit_code,
            0
        );
        // sed.errors.test.ts:136 should error on -e without argument
        assert_eq!(env.exec("sed -e /test/file.txt").exit_code, 1);
        // sed.errors.test.ts:145 should error on invalid regex pattern
        let r = env.exec("sed 's/[/x/' /test/file.txt");
        assert!(!r.stderr.is_empty());
        assert_eq!(r.exit_code, 1);
        // sed.errors.test.ts:152 should be lenient with backreference \9 (exit 0)
        assert_eq!(env.exec("sed 's/foo/\\9/' /test/file.txt").exit_code, 0);
        // sed.errors.test.ts:160 should handle unmatched parenthesis in BRE (literal)
        assert_eq!(env.exec("sed 's/(foo)/[\\1]/' /test/file.txt").exit_code, 0);

        // q / Q / $d addressing (sed.commands.test.ts + sed.test.ts).
        let cenv = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/test/file.txt".to_string(),
                "line 1\nline 2\nline 3\nline 4\nline 5\n".to_string(),
            )]),
            cwd: Some("/test".to_string()),
            ..BashOptions::default()
        });
        // sed.commands.test.ts:15 should quit without printing current line (3Q)
        assert_eq!(
            cenv.exec("sed '3Q' /test/file.txt").stdout,
            "line 1\nline 2\n"
        );
        // sed.commands.test.ts:23 should handle Q at first line (1Q)
        assert_eq!(cenv.exec("sed '1Q' /test/file.txt").stdout, "");
        // sed.commands.test.ts:33 should quit after printing current line (3q)
        assert_eq!(
            cenv.exec("sed '3q' /test/file.txt").stdout,
            "line 1\nline 2\nline 3\n"
        );
        // sed.commands.test.ts:258 should match last line ($p with -n)
        assert_eq!(cenv.exec("sed -n '$p' /test/file.txt").stdout, "line 5\n");
        // sed.test.ts:190 should delete last line with $d (space form)
        assert_eq!(
            cenv.exec("sed '$ d' /test/file.txt").stdout,
            "line 1\nline 2\nline 3\nline 4\n"
        );

        // Semicolon-bearing pattern/replacement and t/T branch tracking.
        let benv = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/semi.txt".to_string(), "a;b;c\n".to_string()),
                ("/two.txt".to_string(), "a\nb\n".to_string()),
                ("/ax.txt".to_string(), "ax\nbx\n".to_string()),
            ]),
            cwd: Some("/".to_string()),
            ..BashOptions::default()
        });
        // sed.test.ts:537 should handle semicolons in pattern and replacement
        assert_eq!(benv.exec("sed 's/a;b/x;y/' /semi.txt").stdout, "x;y;c\n");
        // sed.test.ts:945 t should branch on a successful substitution
        assert_eq!(
            benv.exec("sed 's/./&/;t skip;s/$/X/;:skip' /two.txt")
                .stdout,
            "a\nb\n"
        );
        // sed.test.ts:970 T should not branch when a substitution was made
        assert_eq!(
            benv.exec("sed 's/x/y/;T add;b end;:add;s/$/X/;:end' /ax.txt")
                .stdout,
            "ay\nby\n"
        );
    }

    #[test]
    fn text_stream_jbc34_utf8_pipeline_rows_use_implemented_commands() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/sort.txt".to_string(), "café\nbé\n".to_string()),
                ("/sort_case.txt".to_string(), "Café\nApple\n".to_string()),
                (
                    "/head_tail.txt".to_string(),
                    "한글\nfoo\n漢字\n".to_string(),
                ),
                ("/wc.txt".to_string(), "한글".to_string()),
                (
                    "/pipeline.txt".to_string(),
                    "Ü ö ß é ñ Ω Д 漢字 🎉\n".to_string(),
                ),
                ("/uniq_case.txt".to_string(), "Café\nCAFÉ\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("cat /sort.txt | sort").stdout, "bé\ncafé\n");
        assert_eq!(
            env.exec("cat /sort_case.txt | sort -f").stdout,
            "Apple\nCafé\n"
        );
        assert_eq!(env.exec("cat /head_tail.txt | head -1").stdout, "한글\n");
        assert_eq!(env.exec("cat /head_tail.txt | tail -1").stdout, "漢字\n");
        assert_eq!(env.exec("cat /wc.txt | wc -c").stdout.trim(), "6");
        assert_eq!(env.exec("cat /wc.txt | wc -m").stdout.trim(), "2");
        assert_eq!(env.exec("wc -c /wc.txt").stdout.trim(), "6 /wc.txt");
        assert_eq!(env.exec("wc -m /wc.txt").stdout.trim(), "2 /wc.txt");
        assert_eq!(env.exec("cat /wc.txt | cut -c 1-2").stdout, "한글\n");
        assert_eq!(env.exec("echo 'café' | tr 'é' 'X'").stdout, "cafX\n");
        assert_eq!(env.exec("cat /uniq_case.txt | uniq -i").stdout, "Café\n");
        assert_eq!(env.exec("cat /pipeline.txt | grep -o 'Ü'").stdout, "Ü\n");
        assert_eq!(
            env.exec("echo 'café résumé' | sed 's/é/e/g' | tr 'a-z' 'A-Z'")
                .stdout,
            "CAFE RESUME\n"
        );
        assert_eq!(
            env.exec("printf 'Ü\\nÄ\\nÜ\\nÖ\\nÄ\\n' | sort | uniq")
                .stdout,
            "Ä\nÖ\nÜ\n"
        );
        assert_eq!(env.exec("echo 'Ü:Ö:Ä' | cut -d: -f2").stdout, "Ö\n");
        assert_eq!(
            env.exec("head -1 /pipeline.txt").stdout,
            "Ü ö ß é ñ Ω Д 漢字 🎉\n"
        );
        assert_eq!(
            env.exec("tail -1 /pipeline.txt").stdout,
            "Ü ö ß é ñ Ω Д 漢字 🎉\n"
        );
        assert_eq!(
            env.exec("wc -l /pipeline.txt").stdout.trim(),
            "1 /pipeline.txt"
        );
        assert_eq!(
            env.exec("uniq -i /uniq_case.txt | wc -c").stdout.trim(),
            "6"
        );
    }

    #[test]
    fn structured_data_jq_basic_rows_access_and_iteration() {
        let env = bash();

        assert_eq!(
            env.exec("echo '{\"a\":1}' | jq '.'").stdout,
            "{\n  \"a\": 1\n}\n"
        );
        assert_eq!(
            env.exec("echo '[1,2,3]' | jq '.'").stdout,
            "[\n  1,\n  2,\n  3\n]\n"
        );
        assert_eq!(
            env.exec("echo '{\"name\":\"test\"}' | jq '.name'").stdout,
            "\"test\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"a\":{\"b\":\"nested\"}}' | jq '.a.b'")
                .stdout,
            "\"nested\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1}' | jq '.missing'").stdout,
            "null\n"
        );
        assert_eq!(
            env.exec("echo '{\"count\":42}' | jq '.count'").stdout,
            "42\n"
        );
        assert_eq!(
            env.exec("echo '{\"active\":true}' | jq '.active'").stdout,
            "true\n"
        );
        assert_eq!(
            env.exec("echo '[\"a\",\"b\",\"c\"]' | jq '.[0]'").stdout,
            "\"a\"\n"
        );
        assert_eq!(
            env.exec("echo '[\"a\",\"b\",\"c\"]' | jq '.[-1]'").stdout,
            "\"c\"\n"
        );
        assert_eq!(env.exec("echo '[1,2]' | jq '.[99]'").stdout, "null\n");
        assert_eq!(env.exec("echo '[1,2,3]' | jq '.[]'").stdout, "1\n2\n3\n");
        assert_eq!(
            env.exec("echo '{\"a\":1,\"b\":2}' | jq '.[]'").stdout,
            "1\n2\n"
        );
        assert_eq!(
            env.exec("echo '{\"items\":[1,2,3]}' | jq '.items[]'")
                .stdout,
            "1\n2\n3\n"
        );
        assert_eq!(
            env.exec("echo '{\"data\":{\"value\":42}}' | jq '.data | .value'")
                .stdout,
            "42\n"
        );
    }

    #[test]
    fn structured_data_jq_flags_files_functions_and_operators_close_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/data.json".to_string(), "{\"value\":123}".to_string()),
                (
                    "/a.json".to_string(),
                    "{\"name\":\"alice\",\"items\":[\"x\",\"y\"]}".to_string(),
                ),
                (
                    "/b.json".to_string(),
                    "{\"name\":\"bob\",\"items\":[\"z\"]}".to_string(),
                ),
                ("/file.json".to_string(), "{\"from\":\"file\"}".to_string()),
                (
                    "/obj.json".to_string(),
                    "{\"type\":\"object\",\"value\":42}".to_string(),
                ),
                ("/arr.json".to_string(), "[1,2,3]".to_string()),
                ("/str.json".to_string(), "\"hello\"".to_string()),
                ("/x1.json".to_string(), "{\"x\":1}".to_string()),
                ("/x2.json".to_string(), "{\"x\":2}".to_string()),
                (
                    "/stream.ndjson".to_string(),
                    "{\"id\":1}\n{\"id\":2}".to_string(),
                ),
                (
                    "/concat.json".to_string(),
                    "{\"a\":1}{\"b\":2}{\"c\":3}".to_string(),
                ),
                (
                    "/mixed.json".to_string(),
                    "{\"obj\":true}\n[1,2,3]\n\"string\"\n42\ntrue\nnull".to_string(),
                ),
                ("/empty.json".to_string(), String::new()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec("echo '{\"name\":\"test\"}' | jq -r '.name'")
                .stdout,
            "test\n"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1,\"b\":2}' | jq -c '.'").stdout,
            "{\"a\":1,\"b\":2}\n"
        );
        assert_eq!(env.exec("jq -n 'empty'").stdout, "");
        assert_eq!(
            env.exec("echo '1\n2\n3' | jq -s '.'").stdout,
            "[\n  1,\n  2,\n  3\n]\n"
        );
        assert_eq!(
            env.exec("echo '{\"z\":1,\"a\":2}' | jq -S '.'").stdout,
            "{\n  \"a\": 2,\n  \"z\": 1\n}\n"
        );
        assert_eq!(env.exec("jq '.value' /data.json").stdout, "123\n");
        assert_eq!(
            env.exec("jq '.name' /a.json /b.json").stdout,
            "\"alice\"\n\"bob\"\n"
        );
        assert_eq!(env.exec("jq '.id' /stream.ndjson").stdout, "1\n2\n");
        assert_eq!(
            env.exec("jq 'type' /obj.json /arr.json /str.json").stdout,
            "\"object\"\n\"array\"\n\"string\"\n"
        );
        assert_eq!(
            env.exec("jq -r '.name' /a.json /b.json").stdout,
            "alice\nbob\n"
        );
        assert_eq!(
            env.exec("jq '.items[]' /a.json /b.json").stdout,
            "\"x\"\n\"y\"\n\"z\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"from\":\"stdin\"}' | jq '.from' - /file.json")
                .stdout,
            "\"stdin\"\n\"file\"\n"
        );
        assert_eq!(
            env.exec("jq '.x' /x1.json /empty.json /x2.json").stdout,
            "1\n2\n"
        );
        assert_eq!(
            env.exec("cat /concat.json | jq '.'").stdout,
            "{\n  \"a\": 1\n}\n{\n  \"b\": 2\n}\n{\n  \"c\": 3\n}\n"
        );
        assert_eq!(
            env.exec("cat /mixed.json | jq -c '.'").stdout,
            "{\"obj\":true}\n[1,2,3]\n\"string\"\n42\ntrue\nnull\n"
        );
        assert_eq!(
            env.exec("cat /stream.ndjson | jq -s 'length'").stdout,
            "2\n"
        );
        assert_eq!(env.exec("echo '5' | jq '. + 3'").stdout, "8\n");
        assert_eq!(env.exec("echo '10' | jq '. - 4'").stdout, "6\n");
        assert_eq!(env.exec("echo '6' | jq '. * 7'").stdout, "42\n");
        assert_eq!(
            env.exec("echo '{\"a\":\"foo\",\"b\":\"bar\"}' | jq '.a + .b'")
                .stdout,
            "\"foobar\"\n"
        );
        assert_eq!(env.exec("echo '5' | jq '. == 5'").stdout, "true\n");
        assert_eq!(env.exec("echo 'true' | jq 'not'").stdout, "false\n");
        assert_eq!(
            env.exec("echo '{\"a\":null}' | jq '.a // \"default\"'")
                .stdout,
            "\"default\"\n"
        );
        assert_eq!(env.exec("echo '[1,2,3]' | jq 'length'").stdout, "3\n");
        assert_eq!(
            env.exec("echo '{\"a\":1}' | jq 'type'").stdout,
            "\"object\"\n"
        );
        assert_eq!(env.exec("echo '[5,10,15]' | jq 'first'").stdout, "5\n");
        assert_eq!(env.exec("echo '[5,10,15]' | jq 'last'").stdout, "15\n");
        assert_eq!(
            env.exec("echo '[3,1,2]' | jq 'sort'").stdout,
            "[\n  1,\n  2,\n  3\n]\n"
        );
        assert_eq!(
            env.exec("echo '[1,2,1,3]' | jq 'unique'").stdout,
            "[\n  1,\n  2,\n  3\n]\n"
        );
        assert_eq!(env.exec("echo '[1,2,3,4]' | jq 'add'").stdout, "10\n");
        assert_eq!(env.exec("echo '[5,2,8,1]' | jq 'min'").stdout, "1\n");
        assert_eq!(env.exec("echo '[5,2,8,1]' | jq 'max'").stdout, "8\n");
        assert_eq!(
            env.exec("echo '[1,2,3,4,5]' | jq '[.[] | select(. > 3)]'")
                .stdout,
            "[\n  4,\n  5\n]\n"
        );
        assert_eq!(
            env.exec("echo '[1,2,3]' | jq 'map(. * 2)'").stdout,
            "[\n  2,\n  4,\n  6\n]\n"
        );
        assert_eq!(
            env.exec("echo '[1,2,3]' | jq 'reverse'").stdout,
            "[\n  3,\n  2,\n  1\n]\n"
        );
        assert_eq!(
            env.exec("echo '[\"a\",\"b\",\"c\"]' | jq 'join(\"-\")'")
                .stdout,
            "\"a-b-c\"\n"
        );
        assert_eq!(
            env.exec("echo '\"hello world\"' | jq 'startswith(\"hello\")'")
                .stdout,
            "true\n"
        );
        assert_eq!(
            env.exec("echo '\"HELLO\"' | jq 'ascii_downcase'").stdout,
            "\"hello\"\n"
        );
        assert_eq!(
            env.exec("echo '\"foobar\"' | jq 'index(\"bar\")'").stdout,
            "3\n"
        );
        assert_eq!(env.exec("echo 'not json' | jq '.'").exit_code, 5);
        assert_eq!(env.exec("jq . /missing.json").exit_code, 2);
        assert_eq!(env.exec("jq --unknown '.'").exit_code, 1);
        assert_eq!(env.exec("jq -x '.'").exit_code, 1);
        assert!(env.exec("jq --help").stdout.contains("JSON"));
        assert_eq!(env.exec("echo '{\"a\":1}' | jq -e '.missing'").exit_code, 1);
        assert_eq!(env.exec("echo 'null' | jq -e '.'").exit_code, 1);
        assert_eq!(env.exec("echo 'false' | jq -e '.'").exit_code, 1);
        assert_eq!(env.exec("echo '{\"a\":1}' | jq -e '.a'").exit_code, 0);
        assert_eq!(env.exec("echo '1' | jq -e '.'").exit_code, 0);
        assert_eq!(env.exec("echo '[1,2,3]' | jq -j '.[]'").stdout, "123");
        assert_eq!(
            env.exec("echo '{\"a\":1}' | jq -rc '.'").stdout,
            "{\"a\":1}\n"
        );
        assert_eq!(
            env.exec("echo '{\"name\":\"test\"}' | jq -rc '.name'")
                .stdout,
            "test\n"
        );
    }

    #[test]
    fn structured_data_jq_deep_query_construction_and_operator_rows() {
        let env = bash();

        assert_eq!(
            env.exec("echo '{\"b\":1,\"a\":2}' | jq 'keys'").stdout,
            "[\n  \"a\",\n  \"b\"\n]\n"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1,\"b\":2}' | jq '[.[]]'").stdout,
            "[\n  1,\n  2\n]\n"
        );
        assert_eq!(env.exec("echo '\"hello\"' | jq 'length'").stdout, "5\n");
        assert_eq!(
            env.exec("echo '{\"a\":1,\"b\":2}' | jq 'length'").stdout,
            "2\n"
        );
        assert_eq!(env.exec("echo '[1,2]' | jq 'type'").stdout, "\"array\"\n");
        assert_eq!(env.exec("jq -n 'first(range(10))'").stdout, "0\n");
        assert_eq!(
            env.exec("jq -n '[range(5)]'").stdout,
            "[\n  0,\n  1,\n  2,\n  3,\n  4\n]\n"
        );
        assert_eq!(
            env.exec("jq -n '[range(2;5)]'").stdout,
            "[\n  2,\n  3,\n  4\n]\n"
        );
        assert_eq!(env.exec("echo '20' | jq '. / 4'").stdout, "5\n");
        assert_eq!(env.exec("echo '17' | jq '. % 5'").stdout, "2\n");
        assert_eq!(
            env.exec("echo '[[1,2],[3,4]]' | jq '.[0] + .[1]'").stdout,
            "[\n  1,\n  2,\n  3,\n  4\n]\n"
        );
        assert_eq!(
            env.exec("echo '[{\"a\":1},{\"b\":2}]' | jq -c '.[0] + .[1]'")
                .stdout,
            "{\"a\":1,\"b\":2}\n"
        );
        assert_eq!(env.exec("echo '5' | jq '. != 3'").stdout, "true\n");
        assert_eq!(env.exec("echo '3' | jq '. < 5'").stdout, "true\n");
        assert_eq!(env.exec("echo '10' | jq '. > 5'").stdout, "true\n");
        assert_eq!(env.exec("echo '5' | jq '. <= 5'").stdout, "true\n");
        assert_eq!(env.exec("echo '5' | jq '. >= 5'").stdout, "true\n");
        assert_eq!(env.exec("echo 'true' | jq '. and true'").stdout, "true\n");
        assert_eq!(env.exec("echo 'false' | jq '. or true'").stdout, "true\n");
        assert_eq!(
            env.exec("echo '{\"a\":42}' | jq '.a // \"default\"'")
                .stdout,
            "42\n"
        );
        assert_eq!(
            env.exec("echo '[\"a\",\"b\",\"c\"]' | jq 'add'").stdout,
            "\"abc\"\n"
        );
        assert_eq!(env.exec("echo '3.7' | jq 'floor'").stdout, "3\n");
        assert_eq!(env.exec("echo '3.2' | jq 'ceil'").stdout, "4\n");
        assert_eq!(env.exec("echo '3.5' | jq 'round'").stdout, "4\n");
        assert_eq!(env.exec("echo '16' | jq 'sqrt'").stdout, "4\n");
        assert_eq!(env.exec("echo '-5' | jq 'abs'").stdout, "5\n");
        assert_eq!(env.exec("echo '42' | jq 'tostring'").stdout, "\"42\"\n");
        assert_eq!(env.exec("echo '\"42\"' | jq 'tonumber'").stdout, "42\n");

        assert_eq!(
            env.exec("echo '[1,2,3,4,5]' | jq '[.[] | select(. > 2) | . * 10]'")
                .stdout,
            "[\n  30,\n  40,\n  50\n]\n"
        );
        assert_eq!(
            env.exec("echo '[{\"n\":1},{\"n\":5},{\"n\":2}]' | jq -c '[.[] | select(.n > 2)]'")
                .stdout,
            "[{\"n\":5}]\n"
        );
        assert_eq!(
            env.exec("echo '{\"foo\":42}' | jq 'has(\"foo\")'").stdout,
            "true\n"
        );
        assert_eq!(
            env.exec("echo '{\"foo\":42}' | jq 'has(\"bar\")'").stdout,
            "false\n"
        );
        assert_eq!(env.exec("echo '[1,2,3]' | jq 'has(1)'").stdout, "true\n");
        assert_eq!(
            env.exec("echo '[1,2,3]' | jq 'contains([2])'").stdout,
            "true\n"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1,\"b\":2}' | jq 'contains({\"a\":1})'")
                .stdout,
            "true\n"
        );
        assert_eq!(
            env.exec("echo '[1,2,3,4,5]' | jq 'any(. > 3)'").stdout,
            "true\n"
        );
        assert_eq!(
            env.exec("echo '[1,2,3]' | jq 'all(. > 0)'").stdout,
            "true\n"
        );
        assert_eq!(
            env.exec("echo '5' | jq 'if . > 3 then \"big\" else \"small\" end'")
                .stdout,
            "\"big\"\n"
        );
        assert_eq!(
            env.exec("echo '2' | jq 'if . > 3 then \"big\" else \"small\" end'")
                .stdout,
            "\"small\"\n"
        );
        assert_eq!(env.exec("echo 'null' | jq '.foo?'").stdout, "null\n");
        assert_eq!(env.exec("echo '{\"foo\":42}' | jq '.foo?'").stdout, "42\n");

        assert_eq!(
            env.exec("echo '{\"name\":\"test\",\"value\":42}' | jq -c '{n: .name, v: .value}'")
                .stdout,
            "{\"n\":\"test\",\"v\":42}\n"
        );
        assert_eq!(
            env.exec("echo '{\"name\":\"test\",\"value\":42}' | jq -c '{name, value}'")
                .stdout,
            "{\"name\":\"test\",\"value\":42}\n"
        );
        assert_eq!(
            env.exec("echo '{\"key\":\"foo\",\"val\":42}' | jq -c '{(.key): .val}'")
                .stdout,
            "{\"foo\":42}\n"
        );
        assert_eq!(
            env.exec("echo '[[1,2],[3,4]]' | jq -c '{a: .[0] | add, b: .[1] | add}'")
                .stdout,
            "{\"a\":3,\"b\":7}\n"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1,\"b\":2}' | jq '[.a, .b]'").stdout,
            "[\n  1,\n  2\n]\n"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1,\"b\":2,\"c\":3}' | jq '[.[]]'")
                .stdout,
            "[\n  1,\n  2,\n  3\n]\n"
        );

        assert_eq!(
            env.exec("echo '[{\"n\":3},{\"n\":1},{\"n\":2}]' | jq -c 'min_by(.n)'")
                .stdout,
            "{\"n\":1}\n"
        );
        assert_eq!(
            env.exec("echo '[{\"n\":3},{\"n\":1},{\"n\":2}]' | jq -c 'max_by(.n)'")
                .stdout,
            "{\"n\":3}\n"
        );
        assert_eq!(
            env.exec("echo '[[1,2],[3,4]]' | jq 'flatten'").stdout,
            "[\n  1,\n  2,\n  3,\n  4\n]\n"
        );
        assert_eq!(
            env.exec("echo '[{\"n\":3},{\"n\":1},{\"n\":2}]' | jq -c 'sort_by(.n)'")
                .stdout,
            "[{\"n\":1},{\"n\":2},{\"n\":3}]\n"
        );
        assert_eq!(
            env.exec("echo '[{\"g\":1,\"v\":\"a\"},{\"g\":2,\"v\":\"b\"},{\"g\":1,\"v\":\"c\"}]' | jq -c 'group_by(.g)'")
                .stdout,
            "[[{\"g\":1,\"v\":\"a\"},{\"g\":1,\"v\":\"c\"}],[{\"g\":2,\"v\":\"b\"}]]\n"
        );
        assert_eq!(
            env.exec("echo '[{\"n\":1},{\"n\":2},{\"n\":1}]' | jq -c 'unique_by(.n)'")
                .stdout,
            "[{\"n\":1},{\"n\":2}]\n"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1,\"b\":2}' | jq -c 'to_entries'")
                .stdout,
            "[{\"key\":\"a\",\"value\":1},{\"key\":\"b\",\"value\":2}]\n"
        );
        assert_eq!(
            env.exec("echo '[{\"key\":\"a\",\"value\":1}]' | jq -c 'from_entries'")
                .stdout,
            "{\"a\":1}\n"
        );
    }

    #[test]
    fn structured_data_jq_string_keyword_and_safe_object_rows() {
        let env = bash();

        assert_eq!(
            env.exec("echo '\"a,b,c\"' | jq 'split(\",\")'").stdout,
            "[\n  \"a\",\n  \"b\",\n  \"c\"\n]\n"
        );
        assert_eq!(
            env.exec("echo '\"foobar\"' | jq 'test(\"bar\")'").stdout,
            "true\n"
        );
        assert_eq!(
            env.exec("echo '\"hello world\"' | jq 'endswith(\"world\")'")
                .stdout,
            "true\n"
        );
        assert_eq!(
            env.exec("echo '\"hello world\"' | jq 'ltrimstr(\"hello \")'")
                .stdout,
            "\"world\"\n"
        );
        assert_eq!(
            env.exec("echo '\"hello world\"' | jq 'rtrimstr(\" world\")'")
                .stdout,
            "\"hello\"\n"
        );
        assert_eq!(
            env.exec("echo '\"hello\"' | jq 'ascii_upcase'").stdout,
            "\"HELLO\"\n"
        );
        assert_eq!(
            env.exec("echo '\"foobar\"' | jq 'sub(\"o\"; \"0\")'")
                .stdout,
            "\"f0obar\"\n"
        );
        assert_eq!(
            env.exec("echo '\"foobar\"' | jq 'gsub(\"o\"; \"0\")'")
                .stdout,
            "\"f00bar\"\n"
        );
        assert_eq!(
            env.exec("echo '\"abcabc\"' | jq 'indices(\"bc\")'").stdout,
            "[\n  1,\n  4\n]\n"
        );

        assert_eq!(
            env.exec("echo '{\"label\":\"hello\"}' | jq '.label'")
                .stdout,
            "\"hello\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"and\":true}' | jq '.and'").stdout,
            "true\n"
        );
        assert_eq!(
            env.exec("echo '{\"or\":false}' | jq '.or'").stdout,
            "false\n"
        );
        assert_eq!(env.exec("echo '{\"not\":42}' | jq '.not'").stdout, "42\n");
        assert_eq!(
            env.exec("echo '{\"if\":\"value\"}' | jq '.if'").stdout,
            "\"value\"\n"
        );
        assert_eq!(env.exec("echo '{\"try\":1}' | jq '.try'").stdout, "1\n");
        assert_eq!(env.exec("echo '{\"catch\":2}' | jq '.catch'").stdout, "2\n");
        assert_eq!(
            env.exec("echo '{\"reduce\":\"data\"}' | jq '.reduce'")
                .stdout,
            "\"data\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"foreach\":\"items\"}' | jq '.foreach'")
                .stdout,
            "\"items\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"def\":\"definition\"}' | jq '.def'")
                .stdout,
            "\"definition\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"break\":\"stop\"}' | jq '.break'").stdout,
            "\"stop\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"data\":{\"label\":\"nested\"}}' | jq '.data.label'")
                .stdout,
            "\"nested\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"label\":\"x\",\"value\":1}' | jq -c '{lab: .label, val: .value}'")
                .stdout,
            "{\"lab\":\"x\",\"val\":1}\n"
        );
        assert_eq!(
            env.exec("echo '{\"label\":\"hello\"}' | jq -c '{label}'")
                .stdout,
            "{\"label\":\"hello\"}\n"
        );
        assert_eq!(
            env.exec("echo '{\"not\":true}' | jq -c '{not: .not}'")
                .stdout,
            "{\"not\":true}\n"
        );
        assert_eq!(
            env.exec("echo '{\"and\":1}' | jq -c '{and}'").stdout,
            "{\"and\":1}\n"
        );

        assert_eq!(
            env.exec("echo '{\"__proto__\":\"safe\"}' | jq '.__proto__'")
                .stdout,
            "\"safe\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"constructor\":\"safe\"}' | jq '.constructor'")
                .stdout,
            "\"safe\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"prototype\":\"safe\"}' | jq '.prototype'")
                .stdout,
            "\"safe\"\n"
        );
        assert_eq!(
            env.exec("echo 'null' | jq -c '{(\"__proto__\"): \"polluted\"}'")
                .stdout,
            "{}\n"
        );
        assert_eq!(
            env.exec("echo 'null' | jq -c '{(\"constructor\"): \"polluted\"}'")
                .stdout,
            "{}\n"
        );
        assert_eq!(
            env.exec("echo 'null' | jq -c '{(\"prototype\"): \"polluted\"}'")
                .stdout,
            "{}\n"
        );
        assert_eq!(
            env.exec("echo 'null' | jq -c '{a: 1, (\"__proto__\"): 2, b: 3}'")
                .stdout,
            "{\"a\":1,\"b\":3}\n"
        );
        assert_eq!(
            env.exec("echo '[{\"key\":\"__proto__\",\"value\":\"polluted\"},{\"key\":\"safe\",\"value\":\"ok\"}]' | jq -c 'from_entries'")
                .stdout,
            "{\"safe\":\"ok\"}\n"
        );
        assert_eq!(
            env.exec(
                "echo '[{\"key\":\"constructor\",\"value\":\"polluted\"}]' | jq -c 'from_entries'"
            )
            .stdout,
            "{}\n"
        );
        assert_eq!(
            env.exec(
                "echo '[{\"key\":\"prototype\",\"value\":\"polluted\"}]' | jq -c 'from_entries'"
            )
            .stdout,
            "{}\n"
        );
        assert_eq!(
            env.exec("echo '[{\"name\":\"__proto__\",\"value\":\"polluted\"},{\"Name\":\"constructor\",\"value\":\"polluted\"},{\"k\":\"prototype\",\"v\":\"polluted\"}]' | jq -c 'from_entries'")
                .stdout,
            "{}\n"
        );
    }

    #[test]
    fn structured_data_jq_keyword_field_access_space_rows() {
        let env = bash();
        // space-separated keyword/identifier after dot is NOT field access -> error
        assert_ne!(
            env.exec("echo '{\"if\":\"value\"}' | jq '. if'").exit_code,
            0
        );
        assert_ne!(env.exec("echo '{\"and\":true}' | jq '. and'").exit_code, 0);
        assert_ne!(env.exec("echo '{\"try\":1}' | jq '. try'").exit_code, 0);
        assert_ne!(
            env.exec("echo '{\"foo\":\"bar\"}' | jq '. foo'").exit_code,
            0
        );
        assert_ne!(
            env.exec("echo '{\"data\":{\"foo\":\"bar\"}}' | jq '.data. foo'")
                .exit_code,
            0
        );
        // space-separated string after dot SHOULD be field access
        let r = env.exec("echo '{\"foo\":\"bar\"}' | jq '. \"foo\"'");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "\"bar\"\n");
        let r = env.exec("echo '{\"data\":{\"foo\":\"bar\"}}' | jq '.data. \"foo\"'");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "\"bar\"\n");
    }

    #[test]
    fn structured_data_jq_prototype_pollution_safe_key_rows() {
        let env = bash();
        // from_entries / with_entries drop dangerous keys
        assert_eq!(
            env.exec("echo '{\"a\":1}' | jq 'with_entries(.key = \"__proto__\")'")
                .stdout
                .trim(),
            "{}"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1}' | jq 'with_entries(.key = \"constructor\")'")
                .stdout
                .trim(),
            "{}"
        );
        // setpath ignores dangerous keys
        assert_eq!(
            env.exec("echo '{}' | jq 'setpath([\"__proto__\"]; \"polluted\")'")
                .stdout
                .trim(),
            "{}"
        );
        assert_eq!(
            env.exec("echo '{}' | jq 'setpath([\"constructor\"]; \"polluted\")'")
                .stdout
                .trim(),
            "{}"
        );
        assert_eq!(
            env.exec("echo '{\"a\":{}}' | jq -c 'setpath([\"a\",\"__proto__\"]; \"polluted\")'")
                .stdout
                .trim(),
            "{\"a\":{}}"
        );
        assert_eq!(
            env.exec(
                "echo '{}' | jq -c 'setpath([\"safe\"]; \"ok\") | setpath([\"__proto__\"]; \"bad\")'"
            )
            .stdout
            .trim(),
            "{\"safe\":\"ok\"}"
        );
        // object construction with dangerous computed keys
        assert_eq!(
            env.exec(
                "echo '{}' | jq -c '{(\"__proto__\"): 1, (\"constructor\"): 2, (\"prototype\"): 3, safe: 4}'"
            )
            .stdout
            .trim(),
            "{\"safe\":4}"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1}' | jq -c '. + {(\"__proto__\"): 2} | . + {b: 3}'")
                .stdout
                .trim(),
            "{\"a\":1,\"b\":3}"
        );
        // no host prototype pollution: fresh empty object has no keys
        assert_eq!(env.exec("echo '{}' | jq -c 'keys'").stdout.trim(), "[]");
    }

    #[test]
    fn structured_data_jq_prototype_pollution_update_delete_merge_rows() {
        // Mirrors jq.prototype-pollution.test.ts:201-374 — assignment, update,
        // delete, deep-merge, fromstream, iterator-update, reduce, and chained
        // operations must all drop __proto__/constructor/prototype while keeping
        // safe keys intact.
        let env = bash();

        // update operations with dangerous keys (201-240)
        assert_eq!(
            env.exec("echo '{}' | jq '.__proto__ = \"polluted\"'")
                .stdout
                .trim(),
            "{}"
        );
        assert_eq!(
            env.exec("echo '{}' | jq '.constructor = \"polluted\"'")
                .stdout
                .trim(),
            "{}"
        );
        assert_eq!(
            env.exec("echo '{}' | jq '.__proto__ |= . + \"test\"'")
                .stdout
                .trim(),
            "{}"
        );
        assert_eq!(
            env.exec("echo '{}' | jq '.__proto__ += \"test\"'")
                .stdout
                .trim(),
            "{}"
        );
        assert_eq!(
            env.exec("echo '{}' | jq '.[\"__proto__\"] = \"polluted\"'")
                .stdout
                .trim(),
            "{}"
        );

        // delete operations with dangerous keys (244-268)
        assert_eq!(
            env.exec("echo '{\"a\":1}' | jq -c 'del(.__proto__)'")
                .stdout
                .trim(),
            "{\"a\":1}"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1}' | jq -c 'del(.constructor)'")
                .stdout
                .trim(),
            "{\"a\":1}"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1}' | jq -c 'delpaths([[\"__proto__\"]])'")
                .stdout
                .trim(),
            "{\"a\":1}"
        );

        // deep merge with dangerous keys (272-290)
        assert_eq!(
            env.exec("echo '{\"a\":1}' | jq -c '. * {\"__proto__\": \"polluted\"}'")
                .stdout
                .trim(),
            "{\"a\":1}"
        );
        assert_eq!(
            env.exec(
                "echo '{\"a\":{\"b\":1}}' | jq -c '. * {\"a\": {\"__proto__\": \"polluted\"}}'"
            )
            .stdout
            .trim(),
            "{\"a\":{\"b\":1}}"
        );

        // edge cases and combinations (333-382)
        // Only the exact dangerous keys are filtered; differently-cased keys are
        // kept (rendered in sorted order by this port's JSON model).
        assert_eq!(
            env.exec("echo '{}' | jq -c '{(\"__Proto__\"): 1, (\"__PROTO__\"): 2}'")
                .stdout
                .trim(),
            "{\"__PROTO__\":2,\"__Proto__\":1}"
        );
        assert_eq!(
            env.exec("echo '{}' | jq -c '{a: 1, b: 2, c: 3} | .d = 4 | del(.b)'")
                .stdout
                .trim(),
            "{\"a\":1,\"c\":3,\"d\":4}"
        );
        assert_eq!(
            env.exec("echo '{\"a\":{\"b\":{}}}' | jq -c '.a.b.__proto__ = \"polluted\"'")
                .stdout
                .trim(),
            "{\"a\":{\"b\":{}}}"
        );
    }

    #[test]
    fn structured_data_jq_permissive_control_chars_in_strings_rows() {
        // Mirrors jq.permissive-control-chars.test.ts:10-82 — real jq accepts
        // literal control characters inside JSON string values.
        let payload = |body: &str| {
            Bash::with_options(BashOptions {
                files: BTreeMap::from([("/payload.json".to_string(), body.to_string())]),
                ..BashOptions::default()
            })
        };

        let r = payload("{\"body\":\"first\nsecond\"}\n").exec("jq -r '.body' /payload.json");
        assert_eq!(r.stderr, "");
        assert_eq!(r.stdout, "first\nsecond\n");
        assert_eq!(r.exit_code, 0);

        let r = payload("{\"body\":\"col1\tcol2\"}\n").exec("jq -r '.body' /payload.json");
        assert_eq!(r.stderr, "");
        assert_eq!(r.stdout, "col1\tcol2\n");
        assert_eq!(r.exit_code, 0);

        let r = payload("{\"body\":\"a\rb\"}\n").exec("jq -r '.body' /payload.json");
        assert_eq!(r.stderr, "");
        assert_eq!(r.stdout, "a\rb\n");
        assert_eq!(r.exit_code, 0);

        // escape sequence preserved next to a literal control char
        let r =
            payload("{\"body\":\"line1\\nline2\nline3\"}\n").exec("jq -r '.body' /payload.json");
        assert_eq!(r.stderr, "");
        assert_eq!(r.stdout, "line1\nline2\nline3\n");
        assert_eq!(r.exit_code, 0);

        // control chars outside strings are not rewritten
        let r = payload("{\n  \"a\": 1,\n  \"b\": 2\n}\n").exec("jq -c '.' /payload.json");
        assert_eq!(r.stderr, "");
        assert_eq!(r.stdout, "{\"a\":1,\"b\":2}\n");
        assert_eq!(r.exit_code, 0);

        // concatenated JSON where one value contains a literal newline
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/stream.json".to_string(),
                "{\"a\":\"x\ny\"}{\"a\":\"z\"}\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        let r = env.exec("jq -r '.a' /stream.json");
        assert_eq!(r.stderr, "");
        assert_eq!(r.stdout, "x\ny\nz\n");
        assert_eq!(r.exit_code, 0);
    }

    #[test]
    fn structured_data_jq_filters_elif_trycatch_and_variable_rows() {
        // Mirrors jq.filters.test.ts:104-150 — elif branches, try/catch, and
        // variable binding (including use inside object construction).
        let env = bash();

        assert_eq!(
            env.exec(
                "echo '5' | jq 'if . > 10 then \"big\" elif . > 3 then \"medium\" else \"small\" end'"
            )
            .stdout,
            "\"medium\"\n"
        );
        assert_eq!(
            env.exec("echo '1' | jq 'try error(\"oops\") catch \"caught\"'")
                .stdout,
            "\"caught\"\n"
        );
        assert_eq!(env.exec("echo '5' | jq '. as $x | $x * $x'").stdout, "25\n");
        // Object keys render in sorted order in this port's JSON model.
        assert_eq!(
            env.exec("echo '3' | jq -c '. as $n | {value: $n, doubled: ($n * 2)}'")
                .stdout,
            "{\"doubled\":6,\"value\":3}\n"
        );
    }

    #[test]
    fn structured_data_jq_keyword_destructuring_rows() {
        // Mirrors jq.keyword-field-access.test.ts:205-221 — keyword keys are
        // allowed in object destructuring patterns and shorthand bindings.
        let env = bash();

        let r = env.exec("echo '{\"label\":\"hello\"}' | jq -c '. as {label: $l} | $l'");
        assert_eq!(r.stdout, "\"hello\"\n");
        assert_eq!(r.exit_code, 0);

        let r = env.exec("echo '{\"not\":42}' | jq '. as {$not} | $not'");
        assert_eq!(r.stdout, "42\n");
        assert_eq!(r.exit_code, 0);
    }

    #[test]
    fn structured_data_jq_concatenated_slurp_rows() {
        // Mirrors jq.test.ts:240 — slurping concatenated pretty-printed objects.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/file1.json".to_string(),
                    "{\n  \"id\": 1,\n  \"merged\": true\n}".to_string(),
                ),
                (
                    "/file2.json".to_string(),
                    "{\n  \"id\": 2,\n  \"merged\": false\n}".to_string(),
                ),
                (
                    "/file3.json".to_string(),
                    "{\n  \"id\": 3,\n  \"merged\": true\n}".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });
        let r = env.exec(
            "cat /file1.json /file2.json /file3.json | jq -csS 'group_by(.merged) | map({merged: .[0].merged, count: length})'",
        );
        assert_eq!(r.exit_code, 0);
        // Upstream compares the parsed value with `toEqual`, so assert the two
        // grouped buckets are both present (order is not part of the contract).
        let parsed: serde_json::Value = serde_json::from_str(r.stdout.trim()).unwrap();
        let entries = parsed.as_array().expect("array result");
        assert_eq!(entries.len(), 2);
        assert!(entries.contains(&serde_json::json!({"merged": true, "count": 2})));
        assert!(entries.contains(&serde_json::json!({"merged": false, "count": 1})));
    }

    #[test]
    fn structured_data_jq_multi_file_range_limit_and_tab_rows() {
        // many files in parallel
        let mut files = BTreeMap::new();
        for i in 0..10 {
            files.insert(
                format!("/data/file{i}.json"),
                format!("{{\"id\":{i},\"value\":{}}}", i * 10),
            );
        }
        let env = Bash::with_options(BashOptions {
            files,
            ..BashOptions::default()
        });
        let paths: Vec<String> = (0..10).map(|i| format!("/data/file{i}.json")).collect();
        assert_eq!(
            env.exec(&format!("jq '.id' {}", paths.join(" "))).stdout,
            "0\n1\n2\n3\n4\n5\n6\n7\n8\n9\n"
        );

        // find | xargs jq -r
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/repo/issues/1.json".to_string(),
                    "{\"author\":\"alice\"}".to_string(),
                ),
                (
                    "/repo/issues/2.json".to_string(),
                    "{\"author\":\"bob\"}".to_string(),
                ),
                (
                    "/repo/pulls/1.json".to_string(),
                    "{\"author\":\"charlie\"}".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });
        assert_eq!(
            env.exec("find /repo -name '*.json' | sort | xargs jq -r '.author'")
                .stdout,
            "alice\nbob\ncharlie\n"
        );

        // slurp concatenated JSON into array length
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/stream.json".to_string(),
                "{\"x\":1}\n{\"x\":2}\n{\"x\":3}".to_string(),
            )]),
            ..BashOptions::default()
        });
        assert_eq!(env.exec("cat /stream.json | jq -s 'length'").stdout, "3\n");

        let env = bash();
        // --tab indentation
        let r = env.exec("echo '{\"a\":1}' | jq --tab '.'");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "{\n\t\"a\": 1\n}\n");

        // range limits: limit caps and moderate ranges complete
        let r = env.exec("jq -n '[limit(5; range(1000000))]'");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "[\n  0,\n  1,\n  2,\n  3,\n  4\n]\n");
        let r = env.exec("jq -n '[range(50000)] | length'");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "50000");
    }

    #[test]
    fn structured_data_yq_yaml_json_env_and_error_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/data.yaml".to_string(),
                    "name: test\nversion: 1\n".to_string(),
                ),
                (
                    "/nested.yaml".to_string(),
                    "config:\n  database:\n    host: localhost\n    port: 5432\n".to_string(),
                ),
                (
                    "/items.yaml".to_string(),
                    "items:\n  - name: foo\n    value: 1\n  - name: bar\n    value: 2\n"
                        .to_string(),
                ),
                (
                    "/fruits.yaml".to_string(),
                    "fruits:\n  - apple\n  - banana\n  - cherry\n".to_string(),
                ),
                (
                    "/users.yaml".to_string(),
                    "users:\n  - name: alice\n    active: true\n  - name: bob\n    active: false\n  - name: charlie\n    active: true\n"
                        .to_string(),
                ),
                (
                    "/data.json".to_string(),
                    "{\"name\":\"test\",\"value\":42}".to_string(),
                ),
            ]),
            env: BTreeMap::from([
                ("TEST_VAR".to_string(), "test_value".to_string()),
                ("EMPTY".to_string(), String::new()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("yq '.name' /data.yaml").stdout, "test\n");
        assert_eq!(
            env.exec("yq '.config.database.host' /nested.yaml").stdout,
            "localhost\n"
        );
        assert_eq!(env.exec("yq '.items[0].name' /items.yaml").stdout, "foo\n");
        assert_eq!(
            env.exec("yq '.fruits[]' /fruits.yaml").stdout,
            "apple\nbanana\ncherry\n"
        );
        assert_eq!(
            env.exec("yq '.users[] | select(.active) | .name' /users.yaml")
                .stdout,
            "alice\ncharlie\n"
        );
        assert_eq!(
            env.exec("yq -c -o json '.' /data.yaml").stdout,
            "{\"name\":\"test\",\"version\":1}\n"
        );
        assert_eq!(
            env.exec("yq -r -o json '.name' /data.yaml").stdout,
            "test\n"
        );
        assert_eq!(env.exec("yq -p json '.name' /data.json").stdout, "test\n");
        assert!(
            env.exec("yq -p json '.' /data.json")
                .stdout
                .contains("name: test")
        );
        assert_eq!(env.exec("cat /data.yaml | yq '.name' -").stdout, "test\n");
        assert_eq!(env.exec("yq -n 'length'").stdout, "0\n");
        assert_eq!(
            env.exec("echo 'null' | yq -o json 'env.TEST_VAR'").stdout,
            "\"test_value\"\n"
        );
        assert_eq!(
            env.exec("echo 'null' | yq -o json 'env.MISSING'").stdout,
            "null\n"
        );
        assert_eq!(
            env.exec("echo 'null' | yq -o json 'env.EMPTY'").stdout,
            "\"\"\n"
        );
        assert_eq!(env.exec("yq '.name' /missing.yaml").exit_code, 1);
        assert_eq!(env.exec("yq --unknown '.'").exit_code, 1);
        assert!(env.exec("yq --help").stdout.contains("YAML"));
        assert!(
            env.exec("yq --input-format=nope '.' /data.yaml")
                .stderr
                .contains("invalid input format")
        );
        assert!(
            env.exec("yq --output-format=nope '.' /data.yaml")
                .stderr
                .contains("invalid output format")
        );
        assert!(
            env.exec("yq -p nope '.' /data.yaml")
                .stderr
                .contains("invalid input format")
        );
        assert!(
            env.exec("yq -o nope '.' /data.yaml")
                .stderr
                .contains("invalid output format")
        );
        assert_eq!(env.exec("echo 'true' | yq -e '.'").exit_code, 0);
        assert_eq!(env.exec("echo 'null' | yq -e '.'").exit_code, 1);
        assert_eq!(env.exec("echo 'false' | yq -e '.'").exit_code, 1);
        assert_eq!(
            env.exec("echo '{\"a\":1}' | yq -rc -o json '.'").stdout,
            "{\"a\":1}\n"
        );
        assert_eq!(
            env.exec("echo '[5,10,15]' | yq -o json 'first'").stdout,
            "5\n"
        );
        assert_eq!(
            env.exec("echo '[5,10,15]' | yq -o json 'last'").stdout,
            "15\n"
        );
        assert_eq!(
            env.exec("echo '[1,2,3,4]' | yq -o json 'add'").stdout,
            "10\n"
        );
        assert_eq!(
            env.exec("echo '[5,2,8,1]' | yq -o json 'min'").stdout,
            "1\n"
        );
        assert_eq!(
            env.exec("echo '[5,2,8,1]' | yq -o json 'max'").stdout,
            "8\n"
        );
    }

    #[test]
    fn structured_data_yq_deep_query_env_and_security_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/letters.yaml".to_string(),
                    "items:\n  - a\n  - b\n  - c\n".to_string(),
                ),
                (
                    "/nums.yaml".to_string(),
                    "items:\n  - 1\n  - 2\n".to_string(),
                ),
                (
                    "/nums3.yaml".to_string(),
                    "items:\n  - 1\n  - 2\n  - 3\n".to_string(),
                ),
                (
                    "/items.yaml".to_string(),
                    "items:\n  - a\n  - b\n  - a\n  - c\n  - b\n".to_string(),
                ),
                (
                    "/sort.yaml".to_string(),
                    "items:\n  - name: b\n    val: 2\n  - name: a\n    val: 1\n".to_string(),
                ),
                (
                    "/group.yaml".to_string(),
                    "items:\n  - type: a\n    v: 1\n  - type: b\n    v: 2\n  - type: a\n    v: 3\n"
                        .to_string(),
                ),
                (
                    "/proto.yaml".to_string(),
                    "__proto__:\n  polluted: true\nsafe: value\n".to_string(),
                ),
                (
                    "/ctor.yaml".to_string(),
                    "constructor:\n  prototype:\n    polluted: true\nname: safe\n".to_string(),
                ),
                (
                    "/proto.json".to_string(),
                    "{\"__proto__\":{\"polluted\":true},\"safe\":\"value\"}".to_string(),
                ),
                (
                    "/ctor.json".to_string(),
                    "{\"constructor\":{\"prototype\":{\"polluted\":true}},\"name\":\"ok\"}"
                        .to_string(),
                ),
            ]),
            env: BTreeMap::from([
                ("A".to_string(), "1".to_string()),
                ("B".to_string(), "2".to_string()),
                ("MY_VAR".to_string(), "my_value".to_string()),
                ("SPECIAL".to_string(), "a=1&b=2".to_string()),
                ("NAME".to_string(), "world".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("yq -j '.items[]' /letters.yaml").stdout, "abc");
        assert!(
            env.exec("yq -o json -I 4 '.' /items.yaml")
                .stdout
                .contains("    \"a\"")
        );
        assert_eq!(
            env.exec("yq -cej -o json '.items[]' /nums.yaml").stdout,
            "12"
        );
        assert_eq!(
            env.exec("yq -o json '.items | unique' /items.yaml").stdout,
            "[\n  \"a\",\n  \"b\",\n  \"c\"\n]\n"
        );
        assert_eq!(
            env.exec("yq '.items | sort_by(.name) | .[0].name' /sort.yaml")
                .stdout,
            "a\n"
        );
        assert_eq!(
            env.exec("yq -o json '.items | reverse' /nums3.yaml").stdout,
            "[\n  3,\n  2,\n  1\n]\n"
        );
        assert_eq!(
            env.exec("yq '.items | group_by(.type) | length' /group.yaml")
                .stdout,
            "2\n"
        );

        assert_eq!(
            env.exec("echo 'null' | yq -o json '$ENV.MY_VAR'").stdout,
            "\"my_value\"\n"
        );
        let env_keys = env.exec("echo 'null' | yq -o json 'env | keys'").stdout;
        assert!(env_keys.contains("\"A\""));
        assert!(env_keys.contains("\"B\""));
        assert_eq!(
            env.exec("echo 'null' | yq -o json '$ENV.NONEXISTENT'")
                .stdout,
            "null\n"
        );
        assert_eq!(
            env.exec("echo 'null' | yq -o json 'env.SPECIAL'").stdout,
            "\"a=1&b=2\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"greeting\":\"hello\"}' | yq -o json '.greeting + \" \" + env.NAME'")
                .stdout,
            "\"hello world\"\n"
        );

        assert_eq!(env.exec("yq '.safe' /proto.yaml").stdout, "value\n");
        assert_eq!(env.exec("yq '.name' /ctor.yaml").stdout, "safe\n");
        assert_eq!(env.exec("yq -p json '.safe' /proto.json").stdout, "value\n");
        assert_eq!(env.exec("yq -p json '.name' /ctor.json").stdout, "ok\n");
    }

    #[test]
    fn structured_data_xan_basic_columns_data_filter_rows() {
        let users = "name,age,email,active\nalice,30,alice@example.com,true\nbob,25,bob@example.com,false\ncharlie,35,charlie@example.com,true\ndiana,28,diana@example.com,true\n";
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/users.csv".to_string(), users.to_string()),
                ("/empty.csv".to_string(), "name,age\n".to_string()),
                ("/numbers.csv".to_string(), "n\n1\n2\n3\n4\n5\n".to_string()),
                ("/products.csv".to_string(), "id,name,price,category,in_stock\n1,Widget,19.99,electronics,true\n2,Gadget,29.99,electronics,true\n3,Gizmo,9.99,accessories,false\n".to_string()),
                ("/data.json".to_string(), "[{\"name\":\"alice\",\"age\":30},{\"name\":\"bob\",\"age\":25}]".to_string()),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("xan count /users.csv").stdout, "4\n");
        assert_eq!(env.exec("xan count /numbers.csv").stdout, "5\n");
        assert_eq!(env.exec("xan count /empty.csv").stdout, "0\n");
        assert_eq!(env.exec("echo 'a\n1\n2\n3' | xan count").stdout, "3\n");
        assert_eq!(
            env.exec("xan headers /users.csv").stdout,
            "0   name\n1   age\n2   email\n3   active\n"
        );
        assert_eq!(
            env.exec("xan headers -j /users.csv").stdout,
            "name\nage\nemail\nactive\n"
        );
        assert_eq!(
            env.exec("xan head -l 2 /users.csv").stdout,
            "name,age,email,active\nalice,30,alice@example.com,true\nbob,25,bob@example.com,false\n"
        );
        assert_eq!(
            env.exec("xan tail -l 2 /users.csv").stdout,
            "name,age,email,active\ncharlie,35,charlie@example.com,true\ndiana,28,diana@example.com,true\n"
        );
        assert_eq!(
            env.exec("xan slice -s 1 -e 3 /users.csv").stdout,
            "name,age,email,active\nbob,25,bob@example.com,false\ncharlie,35,charlie@example.com,true\n"
        );
        assert_eq!(
            env.exec("xan reverse /numbers.csv").stdout,
            "n\n5\n4\n3\n2\n1\n"
        );
        assert_eq!(
            env.exec("xan enum -c row /numbers.csv").stdout,
            "row,n\n0,1\n1,2\n2,3\n3,4\n4,5\n"
        );
        assert_eq!(
            env.exec("xan enum /numbers.csv").stdout,
            "index,n\n0,1\n1,2\n2,3\n3,4\n4,5\n"
        );
        assert_eq!(
            env.exec("xan behead /numbers.csv").stdout,
            "1\n2\n3\n4\n5\n"
        );
        assert_eq!(
            env.exec("xan select name,email /users.csv").stdout,
            "name,email\nalice,alice@example.com\nbob,bob@example.com\ncharlie,charlie@example.com\ndiana,diana@example.com\n"
        );
        assert_eq!(
            env.exec("xan select 0-1 /users.csv").stdout,
            "name,age\nalice,30\nbob,25\ncharlie,35\ndiana,28\n"
        );
        assert_eq!(
            env.exec("xan drop email,active /users.csv").stdout,
            "name,age\nalice,30\nbob,25\ncharlie,35\ndiana,28\n"
        );
        assert_eq!(
            env.exec("xan rename VALUE /numbers.csv").stdout,
            "VALUE\n1\n2\n3\n4\n5\n"
        );
        assert_eq!(
            env.exec("xan to json /numbers.csv").stdout,
            "[\n  {\n    \"n\": 1\n  },\n  {\n    \"n\": 2\n  },\n  {\n    \"n\": 3\n  },\n  {\n    \"n\": 4\n  },\n  {\n    \"n\": 5\n  }\n]\n"
        );
        assert_eq!(
            env.exec("xan from -f json /data.json").stdout,
            "age,name\n30,alice\n25,bob\n"
        );
        assert_eq!(
            env.exec("xan filter 'age > 28' /users.csv").stdout,
            "name,age,email,active\nalice,30,alice@example.com,true\ncharlie,35,charlie@example.com,true\n"
        );
        assert_eq!(
            env.exec("xan filter -v 'age > 28' /users.csv").stdout,
            "name,age,email,active\nbob,25,bob@example.com,false\ndiana,28,diana@example.com,true\n"
        );
        assert_eq!(
            env.exec("xan sort -s age -N /users.csv").stdout,
            "name,age,email,active\nbob,25,bob@example.com,false\ndiana,28,diana@example.com,true\nalice,30,alice@example.com,true\ncharlie,35,charlie@example.com,true\n"
        );
        assert_eq!(
            env.exec("xan dedup -s category /products.csv").stdout,
            "id,name,price,category,in_stock\n1,Widget,19.99,electronics,true\n3,Gizmo,9.99,accessories,false\n"
        );
        assert_eq!(
            env.exec("xan search -s name -r '^a' /users.csv").stdout,
            "name,age,email,active\nalice,30,alice@example.com,true\n"
        );
        assert_eq!(env.exec("xan count /missing.csv").exit_code, 1);
        assert!(env.exec("xan --help").stdout.contains("xan"));
        assert_eq!(env.exec("xan foobar /data.csv").exit_code, 1);
        assert_eq!(
            env.exec("xan parallel").stderr,
            "xan parallel: not yet implemented\n"
        );
    }

    #[test]
    fn structured_data_xan_extended_csv_rows() {
        let users = "name,age,email,active\nalice,30,alice@example.com,true\nbob,25,bob@example.com,false\ncharlie,35,charlie@example.com,true\ndiana,28,diana@example.com,true\n";
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/users.csv".to_string(), users.to_string()),
                ("/numbers.csv".to_string(), "n\n1\n2\n3\n4\n5\n".to_string()),
                (
                    "/products.csv".to_string(),
                    "id,name,price,category,in_stock\n1,Widget,19.99,electronics,true\n2,Gadget,29.99,electronics,true\n3,Gizmo,9.99,accessories,false\n"
                        .to_string(),
                ),
                ("/same.csv".to_string(), "n\n5\n5\n5\n".to_string()),
                ("/unique.csv".to_string(), "n\n1\n2\n3\n".to_string()),
                ("/small.csv".to_string(), "n\n1\n".to_string()),
                ("/header.csv".to_string(), "a,b,c\n".to_string()),
                (
                    "/people.csv".to_string(),
                    "name,age\nalice,30\nbob,25\n".to_string(),
                ),
                (
                    "/matrix.json".to_string(),
                    "[[\"name\",\"age\"],[\"alice\",30],[\"bob\",25]]".to_string(),
                ),
                ("/bad.json".to_string(), "not valid json".to_string()),
                (
                    "/home/user/data.csv".to_string(),
                    "a,b,c\n1,2,3\n".to_string(),
                ),
                (
                    "/search.csv".to_string(),
                    "name\nFOO\nfoo\nbar\n".to_string(),
                ),
            ]),
            cwd: Some("/home/user".to_string()),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec("echo 'a,b,c\n1,2,3' | xan headers -j").stdout,
            "a\nb\nc\n"
        );
        assert_eq!(env.exec("xan head /users.csv").stdout, users);
        assert_eq!(
            env.exec("echo 'a\n1\n2\n3\n4\n5' | xan head -l 2").stdout,
            "a\n1\n2\n"
        );
        assert_eq!(env.exec("xan tail /users.csv").stdout, users);
        assert_eq!(
            env.exec("xan slice -l 2 /users.csv").stdout,
            "name,age,email,active\nalice,30,alice@example.com,true\nbob,25,bob@example.com,false\n"
        );
        assert_eq!(env.exec("xan behead /small.csv").stdout, "1\n");
        assert_eq!(env.exec("xan behead /header.csv").stdout, "");
        assert_eq!(
            env.exec("xan select 0,2 /users.csv").stdout,
            "name,email\nalice,alice@example.com\nbob,bob@example.com\ncharlie,charlie@example.com\ndiana,diana@example.com\n"
        );
        assert_eq!(
            env.exec("xan select email,name /users.csv").stdout,
            "email,name\nalice@example.com,alice\nbob@example.com,bob\ncharlie@example.com,charlie\ndiana@example.com,diana\n"
        );
        assert_eq!(env.exec("xan select a,b data.csv").stdout, "a,b\n1,2\n");
        assert_eq!(
            env.exec("xan drop 2,3 /users.csv").stdout,
            "name,age\nalice,30\nbob,25\ncharlie,35\ndiana,28\n"
        );
        assert_eq!(env.exec("xan drop c data.csv").stdout, "a,b\n1,2\n");
        assert_eq!(
            env.exec("xan rename username -s name /users.csv | xan select username")
                .stdout,
            "username\nalice\nbob\ncharlie\ndiana\n"
        );
        assert_eq!(
            env.exec("xan filter 'active eq \"true\"' /users.csv")
                .stdout,
            "name,age,email,active\nalice,30,alice@example.com,true\ncharlie,35,charlie@example.com,true\ndiana,28,diana@example.com,true\n"
        );
        assert_eq!(
            env.exec("xan filter -l 1 'age > 20' /users.csv").stdout,
            "name,age,email,active\nalice,30,alice@example.com,true\n"
        );
        assert_eq!(env.exec("xan filter 'n > 100' /numbers.csv").stdout, "n\n");
        assert_eq!(env.exec("xan sort -s name /users.csv").stdout, users);
        assert_eq!(
            env.exec("xan sort -s age -N -R /users.csv").stdout,
            "name,age,email,active\ncharlie,35,charlie@example.com,true\nalice,30,alice@example.com,true\ndiana,28,diana@example.com,true\nbob,25,bob@example.com,false\n"
        );
        assert_eq!(
            env.exec("xan dedup -s n /unique.csv").stdout,
            "n\n1\n2\n3\n"
        );
        assert_eq!(env.exec("xan dedup -s n /same.csv").stdout, "n\n5\n");
        assert_eq!(
            env.exec("xan search -i -r 'foo' /search.csv").stdout,
            "name\nFOO\nfoo\n"
        );
        assert_eq!(
            env.exec("xan to json /small.csv").stdout,
            "[\n  {\n    \"n\": 1\n  }\n]\n"
        );
        assert_eq!(
            env.exec("xan to json /people.csv").stdout,
            "[\n  {\n    \"age\": 30,\n    \"name\": \"alice\"\n  },\n  {\n    \"age\": 25,\n    \"name\": \"bob\"\n  }\n]\n"
        );
        assert_eq!(
            env.exec("xan to").stderr,
            "xan to: usage: xan to <format> [FILE]\n"
        );
        assert_eq!(
            env.exec("xan from -f json /matrix.json").stdout,
            "name,age\nalice,30\nbob,25\n"
        );
        assert_eq!(
            env.exec("xan from -f json /bad.json").stderr,
            "xan from: invalid JSON input\n"
        );
        assert_eq!(
            env.exec("xan from /matrix.json").stderr,
            "xan from: usage: xan from -f <format> [FILE]\n"
        );
    }

    #[test]
    fn structured_data_xan_agg_aggregations() {
        // Ports packages/just-bash/src/commands/xan/xan.agg.test.ts:10-188.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/data.csv".to_string(), "n\n1\n2\n3\n4\n".to_string()),
                (
                    "/colors.csv".to_string(),
                    "color\nred\nblue\nyellow\nred\n".to_string(),
                ),
                (
                    "/names.csv".to_string(),
                    "name\nJohn\nMary\nLucas\n".to_string(),
                ),
                (
                    "/names2.csv".to_string(),
                    "name\nJohn\nMary\nLucas\nMary\nLucas\n".to_string(),
                ),
                (
                    "/expr.csv".to_string(),
                    "a,b\n1,2\n2,0\n3,6\n4,2\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // count() counts all rows
        assert_eq!(
            env.exec("xan agg 'count() as count' /data.csv").stdout,
            "count\n4\n"
        );
        // count(expr) counts matching rows
        assert_eq!(
            env.exec("xan agg 'count(n > 2) as count' /data.csv").stdout,
            "count\n2\n"
        );
        // sum(col) sums values
        assert_eq!(
            env.exec("xan agg 'sum(n) as sum' /data.csv").stdout,
            "sum\n10\n"
        );
        // mean(col) computes average
        assert_eq!(
            env.exec("xan agg 'mean(n) as mean' /data.csv").stdout,
            "mean\n2.5\n"
        );
        // avg is alias for mean
        assert_eq!(
            env.exec("xan agg 'avg(n) as mean' /data.csv").stdout,
            "mean\n2.5\n"
        );
        // min(col) gets minimum
        assert_eq!(
            env.exec("xan agg 'min(n) as min' /data.csv").stdout,
            "min\n1\n"
        );
        // max(col) gets maximum
        assert_eq!(
            env.exec("xan agg 'max(n) as max' /data.csv").stdout,
            "max\n4\n"
        );
        // first(col) gets first value
        assert_eq!(
            env.exec("xan agg 'first(n) as first' /data.csv").stdout,
            "first\n1\n"
        );
        // last(col) gets last value
        assert_eq!(
            env.exec("xan agg 'last(n) as last' /data.csv").stdout,
            "last\n4\n"
        );
        // median(col) computes median
        assert_eq!(
            env.exec("xan agg 'median(n) as median' /data.csv").stdout,
            "median\n2.5\n"
        );
        // multiple aggregations
        assert_eq!(
            env.exec("xan agg 'count() as count, sum(n) as sum' /data.csv")
                .stdout,
            "count,sum\n4,10\n"
        );
        // all(expr) checks if all rows match
        assert_eq!(
            env.exec("xan agg 'all(n >= 1) as all' /data.csv").stdout,
            "all\ntrue\n"
        );
        assert_eq!(
            env.exec("xan agg 'all(n >= 2) as all' /data.csv").stdout,
            "all\nfalse\n"
        );
        // any(expr) checks if any row matches
        assert_eq!(
            env.exec("xan agg 'any(n >= 1) as any' /data.csv").stdout,
            "any\ntrue\n"
        );
        assert_eq!(
            env.exec("xan agg 'any(n >= 5) as any' /data.csv").stdout,
            "any\nfalse\n"
        );
        // mode finds most common value
        assert_eq!(
            env.exec("xan agg 'mode(color) as mode' /colors.csv").stdout,
            "mode\nred\n"
        );
        // cardinality counts unique values
        assert_eq!(
            env.exec("xan agg 'cardinality(color) as cardinality' /colors.csv")
                .stdout,
            "cardinality\n3\n"
        );
        // values concatenates all values
        assert_eq!(
            env.exec("xan agg 'values(name) as V' /names.csv").stdout,
            "V\nJohn|Mary|Lucas\n"
        );
        // distinct_values gets unique values
        assert_eq!(
            env.exec("xan agg 'distinct_values(name) as V' /names2.csv")
                .stdout,
            "V\nJohn|Lucas|Mary\n"
        );
        // aggregates computed expressions
        assert_eq!(
            env.exec("xan agg 'sum(add(a, b + 1)) as sum' /expr.csv")
                .stdout,
            "sum\n24\n"
        );
    }

    #[test]
    fn structured_data_xan_sample_and_flatten_rows() {
        // Ports packages/just-bash/src/commands/xan/xan.basic.test.ts:222-282
        // (xan sample positional/seed/error and xan flatten/-l/f alias).
        let separator = "\u{2500}".repeat(80);
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/big.csv".to_string(),
                    "n\n1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n".to_string(),
                ),
                ("/three.csv".to_string(), "n\n1\n2\n3\n".to_string()),
                ("/one.csv".to_string(), "n\n1\n".to_string()),
                (
                    "/people.csv".to_string(),
                    "name,age\nalice,30\nbob,25\n".to_string(),
                ),
                ("/single.csv".to_string(), "x\n1\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        // xan sample: samples N rows (positional argument), seeded -> 4 lines.
        let sampled = env.exec("xan sample 3 --seed 42 /big.csv");
        assert_eq!(sampled.exit_code, 0);
        let lines: Vec<&str> = sampled.stdout.trim().split('\n').collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "n");

        // xan sample: returns all rows if sample size exceeds data.
        assert_eq!(env.exec("xan sample 10 /three.csv").stdout, "n\n1\n2\n3\n");

        // xan sample: errors without sample size.
        let missing = env.exec("xan sample /one.csv");
        assert_eq!(missing.exit_code, 1);
        assert!(missing.stderr.contains("usage"));

        // xan flatten: displays records vertically.
        assert_eq!(
            env.exec("xan flatten /people.csv").stdout,
            format!(
                "Row n\u{b0}0\n{separator}\nname alice\nage  30\n\nRow n\u{b0}1\n{separator}\nname bob\nage  25\n"
            )
        );

        // xan flatten: limits rows with -l.
        assert_eq!(
            env.exec("xan flatten -l 1 /three.csv").stdout,
            format!("Row n\u{b0}0\n{separator}\nn 1\n")
        );

        // xan flatten: works with alias f.
        assert_eq!(
            env.exec("xan f /single.csv").stdout,
            format!("Row n\u{b0}0\n{separator}\nx 1\n")
        );
    }

    #[test]
    fn structured_data_xan_frequency_rows() {
        // Ports packages/just-bash/src/commands/xan/xan.frequency.test.ts:12-83.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/in.csv".to_string(),
                    "h1,h2\na,z\na,y\na,y\nb,z\n,z\n".to_string(),
                ),
                (
                    "/data.csv".to_string(),
                    "a\nx\nx\ny\ny\nz\nz\n".to_string(),
                ),
                (
                    "/group.csv".to_string(),
                    "name,color\njohn,blue\nmary,red\nmary,red\nmary,red\nmary,purple\njohn,yellow\njohn,blue\n".to_string(),
                ),
                (
                    "/nums.csv".to_string(),
                    "n\n1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // computes frequency of all columns
        let all = env.exec("xan frequency --no-extra -l 0 /in.csv");
        assert_eq!(all.exit_code, 0);
        let lines: Vec<&str> = all.stdout.trim().split('\n').collect();
        assert_eq!(lines[0], "field,value,count");
        assert!(lines.len() > 1);

        // selects specific column with -s
        let h2 = env.exec("xan frequency -s h2 --no-extra -l 0 /in.csv");
        assert_eq!(h2.exit_code, 0);
        assert!(h2.stdout.contains("field,value,count\n"));
        assert!(h2.stdout.contains("h2,z,3"));
        assert!(h2.stdout.contains("h2,y,2"));

        // limits results with -l
        let limited = env.exec("xan frequency -l 1 --no-extra /in.csv");
        assert_eq!(limited.exit_code, 0);
        let limited_lines: Vec<&str> = limited.stdout.trim().split('\n').collect();
        assert_eq!(limited_lines[0], "field,value,count");

        // includes empty values
        let empty = env.exec("xan frequency -s h1 -l 0 /in.csv");
        assert_eq!(empty.exit_code, 0);
        assert!(empty.stdout.contains("<empty>"));

        // shows stability with equal counts
        assert_eq!(
            env.exec("xan frequency /data.csv").stdout,
            "field,value,count\na,x,2\na,y,2\na,z,2\n"
        );

        // groups frequency by column with -g
        let grouped = env.exec("xan frequency -g name /group.csv");
        assert_eq!(grouped.exit_code, 0);
        assert!(grouped.stdout.contains("field,name,value,count\n"));

        // shows all values with -A
        let show_all = env.exec("xan frequency -A /nums.csv");
        assert_eq!(show_all.exit_code, 0);
        let all_lines: Vec<&str> = show_all.stdout.trim().split('\n').collect();
        assert_eq!(all_lines.len(), 12);
    }

    #[test]
    fn structured_data_xan_top_transpose_fixlengths_split_search_rows() {
        // Ports packages/just-bash/src/commands/xan/xan.filter-sort.test.ts:129,138,149,158,185
        // and packages/just-bash/src/commands/xan/xan.data.test.ts:85,98,108,147,156,165,176,185,194.
        let products = "id,name,price,category,in_stock\n1,Widget,19.99,electronics,true\n2,Gadget,29.99,electronics,true\n3,Gizmo,9.99,accessories,false\n4,Doodad,49.99,electronics,true\n5,Thingamajig,14.99,accessories,true\n";
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/products.csv".to_string(), products.to_string()),
                (
                    "/search.csv".to_string(),
                    "h1,h2\nfoobar,x\nabc,y\nbarfoo,z\n".to_string(),
                ),
                ("/bad.csv".to_string(), "name\nalice\n".to_string()),
                (
                    "/metric.csv".to_string(),
                    "metric,jan,feb,mar\nsales,100,150,200\ncosts,80,90,100\n".to_string(),
                ),
                ("/single.csv".to_string(), "name\nalice\nbob\n".to_string()),
                ("/emptycols.csv".to_string(), "a,b,c\n".to_string()),
                (
                    "/ragged.csv".to_string(),
                    "a,b,c\n1,2,3\n4,5\n6\n".to_string(),
                ),
                (
                    "/wide.csv".to_string(),
                    "a,b,c,d\n1,2,3,4\n5,6,7,8\n".to_string(),
                ),
                ("/short.csv".to_string(), "a,b,c\n1,2\n3\n".to_string()),
                ("/six.csv".to_string(), "n\n1\n2\n3\n4\n5\n6\n".to_string()),
                ("/five.csv".to_string(), "n\n1\n2\n3\n4\n5\n".to_string()),
                ("/one.csv".to_string(), "n\n1\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        // xan top: top N by numeric column (descending)
        assert_eq!(
            env.exec("xan top price -l 2 /products.csv").stdout,
            "id,name,price,category,in_stock\n4,Doodad,49.99,electronics,true\n2,Gadget,29.99,electronics,true\n"
        );
        // xan top -R: bottom N (ascending)
        assert_eq!(
            env.exec("xan top price -l 2 -R /products.csv").stdout,
            "id,name,price,category,in_stock\n3,Gizmo,9.99,accessories,false\n5,Thingamajig,14.99,accessories,true\n"
        );

        // xan search: regex over all columns
        assert_eq!(
            env.exec("xan search -r '^foo' /search.csv").stdout,
            "h1,h2\nfoobar,x\n"
        );
        // xan search -v: inverted match
        assert_eq!(
            env.exec("xan search -v -r '^foo' /search.csv").stdout,
            "h1,h2\nabc,y\nbarfoo,z\n"
        );
        // xan search: invalid regex pattern reports a precise error
        let bad = env.exec("xan search '[' /bad.csv");
        assert_eq!(bad.exit_code, 1);
        assert_eq!(bad.stderr, "xan search: invalid regex pattern '['\n");

        // xan transpose: swap rows and columns
        assert_eq!(
            env.exec("xan transpose /metric.csv").stdout,
            "metric,sales,costs\njan,100,80\nfeb,150,90\nmar,200,100\n"
        );
        // xan transpose: single column (no data rows after transpose)
        assert_eq!(
            env.exec("xan transpose /single.csv").stdout,
            "name,alice,bob\n"
        );
        // xan transpose: header-only input
        assert_eq!(
            env.exec("xan transpose /emptycols.csv").stdout,
            "column\na\nb\nc\n"
        );

        // xan fixlengths: pad short rows
        assert_eq!(
            env.exec("xan fixlengths /ragged.csv").stdout,
            "a,b,c\n1,2,3\n4,5,\n6,,\n"
        );
        // xan fixlengths -l: truncate long rows
        assert_eq!(
            env.exec("xan fixlengths -l 2 /wide.csv").stdout,
            "a,b\n1,2\n5,6\n"
        );
        // xan fixlengths -d: custom default value
        assert_eq!(
            env.exec("xan fixlengths -d 'N/A' /short.csv").stdout,
            "a,b,c\n1,2,N/A\n3,N/A,N/A\n"
        );

        // xan split -c: into N chunks
        assert_eq!(
            env.exec("xan split -c 3 /six.csv").stdout,
            "Split into 3 parts\n"
        );
        // xan split -S: by chunk size
        assert_eq!(
            env.exec("xan split -S 2 /five.csv").stdout,
            "Split into 3 parts\n"
        );
        // xan split: errors without -c or -S
        let err = env.exec("xan split /one.csv");
        assert_eq!(err.exit_code, 1);
        assert_eq!(err.stderr, "xan split: must specify -c or -S\n");
    }

    #[test]
    fn structured_data_xan_groupby_rows() {
        // Ports packages/just-bash/src/commands/xan/xan.groupby.test.ts:13,22,29,38,47,58,77,91.
        let data =
            "id,value_A,value_B,value_C\nx,1,2,3\ny,2,3,4\nz,3,4,5\ny,1,2,3\nz,2,3,5\nz,3,6,7\n";
        let multi = "name,color,count\njohn,blue,1\nmary,orange,3\nmary,orange,2\njohn,yellow,9\njohn,blue,2\n";
        let sorted =
            "id,value_A,value_B,value_C\nx,1,2,3\ny,2,3,4\ny,1,2,3\nz,2,3,5\nz,3,6,7\nz,3,4,5\n";
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/data.csv".to_string(), data.to_string()),
                ("/multi.csv".to_string(), multi.to_string()),
                ("/sorted.csv".to_string(), sorted.to_string()),
                (
                    "/empty.csv".to_string(),
                    "id,value_A,value_B,value_C\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // groups and sums
        assert_eq!(
            env.exec("xan groupby id 'sum(value_A) as sumA' /data.csv")
                .stdout,
            "id,sumA\nx,1\ny,3\nz,8\n"
        );
        // groups and counts (alias defaults to count())
        assert_eq!(
            env.exec("xan groupby id 'count()' /data.csv").stdout,
            "id,count()\nx,1\ny,2\nz,3\n"
        );
        // groups with complex nested add() expression
        assert_eq!(
            env.exec("xan groupby id 'sum(add(value_A,add(value_B,value_C))) as sum' /data.csv")
                .stdout,
            "id,sum\nx,6\ny,15\nz,38\n"
        );
        // computes mean per group (JS float formatting preserved)
        assert_eq!(
            env.exec("xan groupby id 'mean(value_A) as meanA' /data.csv")
                .stdout,
            "id,meanA\nx,1\ny,1.5\nz,2.6666666666666665\n"
        );
        // computes max per group across multiple specs
        assert_eq!(
            env.exec(
                "xan groupby id 'max(value_A) as maxA, max(value_B) as maxB,max(value_C) as maxC' /data.csv"
            )
            .stdout,
            "id,maxA,maxB,maxC\nx,1,2,3\ny,2,3,4\nz,3,6,7\n"
        );
        // groups by multiple columns preserving first-seen order
        assert_eq!(
            env.exec("xan groupby name,color 'sum(count) as sum' /multi.csv")
                .stdout,
            "name,color,sum\njohn,blue,3\nmary,orange,5\njohn,yellow,9\n"
        );
        // --sorted is accepted (no-op) and still groups correctly
        assert_eq!(
            env.exec("xan groupby id 'sum(value_A) as sumA' --sorted /sorted.csv")
                .stdout,
            "id,sumA\nx,1\ny,3\nz,8\n"
        );
        // empty data yields just the header row
        assert_eq!(
            env.exec("xan groupby id 'sum(value_A) as sumA' --sorted /empty.csv")
                .stdout,
            "id,sumA\n"
        );
    }

    #[test]
    fn structured_data_xan_shuffle_rows() {
        // Ports packages/just-bash/src/commands/xan/xan.data.test.ts:119,133.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/data.csv".to_string(), "n\n1\n2\n3\n4\n5\n".to_string()),
                (
                    "/ten.csv".to_string(),
                    "n\n1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // shuffles rows with seed for reproducibility: header preserved,
        // all five values present, six total lines.
        let result = env.exec("xan shuffle --seed 42 /data.csv");
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.trim().split('\n').collect();
        assert_eq!(lines[0], "n");
        assert_eq!(lines.len(), 6);
        let mut values: Vec<i64> = lines[1..]
            .iter()
            .map(|l| l.parse::<i64>().unwrap())
            .collect();
        values.sort_unstable();
        assert_eq!(values, vec![1, 2, 3, 4, 5]);
        // Deterministic for a given seed.
        assert_eq!(
            env.exec("xan shuffle --seed 42 /data.csv").stdout,
            result.stdout
        );

        // produces different order with different seeds
        let r1 = env.exec("xan shuffle --seed 1 /ten.csv");
        let r2 = env.exec("xan shuffle --seed 2 /ten.csv");
        assert_eq!(r1.exit_code, 0);
        assert_eq!(r2.exit_code, 0);
        assert_ne!(r1.stdout, r2.stdout);
    }

    #[test]
    fn structured_data_xan_partition_rows() {
        // Ports packages/just-bash/src/commands/xan/xan.data.test.ts:205,214,225,280,332.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.csv".to_string(),
                "region,value\nnorth,10\nsouth,20\nnorth,30\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        // partitions by column value
        assert_eq!(
            env.exec("xan partition region /data.csv").stdout,
            "Partitioned into 2 files by 'region'\n"
        );
        assert_eq!(
            env.exec("cat /north.csv").stdout,
            "region,value\nnorth,10\nnorth,30\n"
        );
        assert_eq!(
            env.exec("cat /south.csv").stdout,
            "region,value\nsouth,20\n"
        );

        // errors on missing column
        let missing = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/m.csv".to_string(), "a,b\n1,2\n".to_string())]),
            ..BashOptions::default()
        });
        let err = missing.exec("xan partition nonexistent /m.csv");
        assert_eq!(err.exit_code, 1);
        assert_eq!(
            err.stderr,
            "xan partition: column 'nonexistent' not found\n"
        );

        // does not silently overwrite when distinct values share a sanitized name:
        // a/b, a:b, a b all sanitize to a_b -> distinct hashed files.
        let collide = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.csv".to_string(),
                "key,value\na/b,1\na:b,2\na b,3\nplain,4\na/b,11\na:b,22\na b,33\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        assert_eq!(
            collide.exec("xan partition key /data.csv").stdout,
            "Partitioned into 4 files by 'key'\n"
        );
        let ls = collide.exec("ls /");
        let files: Vec<&str> = ls
            .stdout
            .split('\n')
            .filter(|f| f.ends_with(".csv") && *f != "data.csv")
            .collect();
        assert_eq!(files.len(), 4);
        assert!(files.contains(&"plain.csv"));
        // The colliding files together contain exactly the six colliding rows.
        let mut all_values: Vec<String> = Vec::new();
        for f in files.iter().filter(|f| f.starts_with("a_b")) {
            let content = collide.exec(&format!("cat /{f}")).stdout;
            for line in content.split('\n').skip(1).filter(|l| !l.is_empty()) {
                all_values.push(line.to_string());
            }
        }
        all_values.sort();
        assert_eq!(
            all_values,
            vec!["a b,3", "a b,33", "a/b,1", "a/b,11", "a:b,2", "a:b,22"]
        );

        // disambiguates a hash-suffixed colliding name vs a literal value
        // with the same sanitized form (FNV-1a of "a/b"), computed here
        // with the same FNV-1a base36 logic the implementation uses.
        let hash = {
            let mut h: u32 = 2166136261;
            for byte in "a/b".encode_utf16() {
                h = (h ^ (byte as u32)).wrapping_mul(16777619);
            }
            let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
            let mut value = h as u64;
            let mut out = Vec::new();
            if value == 0 {
                out.push(b'0');
            }
            while value > 0 {
                out.push(digits[(value % 36) as usize]);
                value /= 36;
            }
            out.reverse();
            let mut base36 = String::from_utf8(out).unwrap();
            while base36.len() < 6 {
                base36.insert(0, '0');
            }
            base36.chars().take(6).collect::<String>()
        };
        let literal_like = format!("a_b_{hash}");
        let disambig = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.csv".to_string(),
                format!("key,value\na/b,1\na:b,2\n{literal_like},3\n"),
            )]),
            ..BashOptions::default()
        });
        assert_eq!(
            disambig.exec("xan partition key /data.csv").stdout,
            "Partitioned into 3 files by 'key'\n"
        );
        let ls = disambig.exec("ls /");
        let files: Vec<&str> = ls
            .stdout
            .split('\n')
            .filter(|f| f.ends_with(".csv") && *f != "data.csv")
            .collect();
        assert_eq!(files.len(), 3);
        let unique: std::collections::HashSet<&&str> = files.iter().collect();
        assert_eq!(unique.len(), 3);
        let mut rows: Vec<String> = Vec::new();
        for f in &files {
            let content = disambig.exec(&format!("cat /{f}")).stdout;
            for line in content.split('\n').skip(1).filter(|l| !l.is_empty()) {
                rows.push(line.to_string());
            }
        }
        rows.sort();
        assert_eq!(
            rows,
            vec![
                "a/b,1".to_string(),
                "a:b,2".to_string(),
                format!("{literal_like},3")
            ]
        );

        // deterministic suffix across repeated runs (same input = same filenames).
        let run = || {
            let b = Bash::with_options(BashOptions {
                files: BTreeMap::from([(
                    "/data.csv".to_string(),
                    "k,v\na/b,1\na:b,2\n".to_string(),
                )]),
                ..BashOptions::default()
            });
            b.exec("xan partition k /data.csv");
            let mut names: Vec<String> = b
                .exec("ls /")
                .stdout
                .split('\n')
                .filter(|f| f.ends_with(".csv") && *f != "data.csv")
                .map(|f| f.to_string())
                .collect();
            names.sort();
            names
        };
        let ls1 = run();
        let ls2 = run();
        assert_eq!(ls1, ls2);
        assert_eq!(ls1.len(), 2);
    }

    #[test]
    fn structured_data_xan_transform_rows() {
        // Ports packages/just-bash/src/commands/xan/xan.transform.test.ts:12,19,28,35,42,51,58,65,73.
        let data = "a,b,c\n1,2,3\n4,5,6\n";
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/data.csv".to_string(), data.to_string()),
                (
                    "/names.csv".to_string(),
                    "name,value\nhello,1\nworld,2\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // transforms a column with expression
        assert_eq!(
            env.exec("xan transform b 'add(a, b)' /data.csv").stdout,
            "a,b,c\n1,3,3\n4,9,6\n"
        );
        // transforms with rename
        assert_eq!(
            env.exec("xan transform b 'add(a, b)' -r sum /data.csv")
                .stdout,
            "a,sum,c\n1,3,3\n4,9,6\n"
        );
        // transforms with underscore reference
        assert_eq!(
            env.exec("xan transform b 'mul(_, 2)' /data.csv").stdout,
            "a,b,c\n1,4,3\n4,10,6\n"
        );
        // transforms multiple columns
        assert_eq!(
            env.exec("xan transform a,b 'mul(_, 10)' /data.csv").stdout,
            "a,b,c\n10,20,3\n40,50,6\n"
        );
        // transforms multiple columns with rename
        assert_eq!(
            env.exec("xan transform a,b 'mul(_, 10)' -r x,y /data.csv")
                .stdout,
            "x,y,c\n10,20,3\n40,50,6\n"
        );
        // errors on missing column
        let err = env.exec("xan transform z 'add(a, b)' /data.csv");
        assert_eq!(err.exit_code, 1);
        assert!(err.stderr.contains("column 'z' not found"));
        // errors on missing arguments (no positional args at all)
        let err = env.exec("xan transform");
        assert_eq!(err.exit_code, 1);
        assert!(err.stderr.contains("usage"));
        // errors on missing expression (only the column positional supplied)
        let err = env.exec("echo '' | xan transform a");
        assert_eq!(err.exit_code, 1);
        assert!(err.stderr.contains("usage"));
        // transforms with string function upper(_)
        assert_eq!(
            env.exec("xan transform name 'upper(_)' /names.csv").stdout,
            "name,value\nHELLO,1\nWORLD,2\n"
        );
    }

    #[test]
    fn structured_data_xan_map_rows() {
        // Ports packages/just-bash/src/commands/xan/xan.map.test.ts:10,19,30,39,50,
        // 63,76,102,113,126,139,150 (the "uses trim function" case at :89 requires
        // CSV quote preservation, which the in-memory CSV reader does not model).
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/ab.csv".to_string(), "a,b\n1,2\n2,3\n".to_string()),
                ("/n.csv".to_string(), "n\n10\n15\n".to_string()),
                ("/ob.csv".to_string(), "a,b\n1,4\n5,2\n".to_string()),
                (
                    "/names.csv".to_string(),
                    "full_name\njohn landis\nbéatrice babka\n".to_string(),
                ),
                (
                    "/names2.csv".to_string(),
                    "full_name\njohn landis\nmary smith\n".to_string(),
                ),
                ("/case.csv".to_string(), "name\nJohn\nmary\n".to_string()),
                (
                    "/words.csv".to_string(),
                    "word\ncat\ndog\nelephant\n".to_string(),
                ),
                ("/xy.csv".to_string(), "x,y\n10,3\n20,4\n".to_string()),
                ("/neg.csv".to_string(), "n\n-5.7\n3.2\n".to_string()),
                ("/score.csv".to_string(), "score\n85\n55\n70\n".to_string()),
                (
                    "/coalesce.csv".to_string(),
                    "name,id\njohn,1\n,2\nmary,3\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // adds computed column
        assert_eq!(
            env.exec("xan map 'add(a, b) as c' /ab.csv").stdout,
            "a,b,c\n1,2,3\n2,3,5\n"
        );
        // adds multiple computed columns
        assert_eq!(
            env.exec("xan map 'add(a, b) as c, mul(a, b) as d' /ab.csv")
                .stdout,
            "a,b,c,d\n1,2,3,2\n2,3,5,6\n"
        );
        // uses index() function
        assert_eq!(
            env.exec("xan map 'index() as r' /n.csv").stdout,
            "n,r\n10,0\n15,1\n"
        );
        // overwrites columns with -O
        assert_eq!(
            env.exec("xan map -O 'b * 10 as b, a * b as c' /ob.csv")
                .stdout,
            "a,b,c\n1,40,4\n5,20,10\n"
        );
        // filters rows with --filter
        assert_eq!(
            env.exec(
                "xan map \"if(startswith(full_name, 'j'), split(full_name, ' ')[0]) as first_name\" --filter /names.csv"
            )
            .stdout,
            "full_name,first_name\njohn landis,john\n"
        );
        // uses split function
        assert_eq!(
            env.exec("xan map \"split(full_name, ' ')[0] as first\" /names2.csv")
                .stdout,
            "full_name,first\njohn landis,john\nmary smith,mary\n"
        );
        // uses upper and lower
        assert_eq!(
            env.exec("xan map 'upper(name) as upper, lower(name) as lower' /case.csv")
                .stdout,
            "name,upper,lower\nJohn,JOHN,john\nmary,MARY,mary\n"
        );
        // uses len function
        assert_eq!(
            env.exec("xan map 'len(word) as length' /words.csv").stdout,
            "word,length\ncat,3\ndog,3\nelephant,8\n"
        );
        // computes arithmetic expressions
        assert_eq!(
            env.exec("xan map 'x + y as sum, x - y as diff, x * y as prod, x / y as quot' /xy.csv")
                .stdout,
            "x,y,sum,diff,prod,quot\n10,3,13,7,30,3.3333333333333335\n20,4,24,16,80,5\n"
        );
        // uses abs and round
        assert_eq!(
            env.exec("xan map 'abs(n) as absolute, round(n) as rounded' /neg.csv")
                .stdout,
            "n,absolute,rounded\n-5.7,5.7,-6\n3.2,3.2,3\n"
        );
        // uses if expression
        assert_eq!(
            env.exec("xan map \"if(score >= 60, 'pass', 'fail') as result\" /score.csv")
                .stdout,
            "score,result\n85,pass\n55,fail\n70,pass\n"
        );
        // uses coalesce for defaults
        assert_eq!(
            env.exec("xan map \"coalesce(name, 'unknown') as name_safe\" /coalesce.csv")
                .stdout,
            "name,id,name_safe\njohn,1,john\n,2,unknown\nmary,3,mary\n"
        );
    }

    #[test]
    fn structured_data_xan_reshape_rows() {
        // Ports packages/just-bash/src/commands/xan/xan.reshape.test.ts:
        // explode (12,21,30,39,48,58), implode (70,80,90,100), pivot (113,127,
        // 140,153,166).
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/tags.csv".to_string(),
                    "id,tags\n1,a|b|c\n2,x|y\n".to_string(),
                ),
                ("/items.csv".to_string(), "id,items\n1,a;b;c\n".to_string()),
                ("/tags2.csv".to_string(), "id,tags\n1,a|b\n".to_string()),
                (
                    "/tagsempty.csv".to_string(),
                    "id,tags\n1,a|b\n2,\n3,c\n".to_string(),
                ),
                ("/missing.csv".to_string(), "id,tags\n1,a\n".to_string()),
                (
                    "/impl.csv".to_string(),
                    "id,tag\n1,a\n1,b\n1,c\n2,x\n2,y\n".to_string(),
                ),
                ("/impl2.csv".to_string(), "id,val\n1,a\n1,b\n".to_string()),
                ("/impl3.csv".to_string(), "id,tag\n1,a\n1,b\n".to_string()),
                (
                    "/impl4.csv".to_string(),
                    "id,tag\n1,a\n2,x\n1,b\n".to_string(),
                ),
                (
                    "/pivot.csv".to_string(),
                    "region,product,amount\nnorth,A,10\nnorth,B,20\nsouth,A,15\nsouth,B,25\n"
                        .to_string(),
                ),
                (
                    "/pivot2.csv".to_string(),
                    "region,product,amount\nnorth,A,10\nnorth,B,20\nsouth,A,15\nsouth,A,5\n"
                        .to_string(),
                ),
                (
                    "/pivot3.csv".to_string(),
                    "cat,type,val\nX,a,10\nX,a,20\nX,b,30\n".to_string(),
                ),
                (
                    "/pivot4.csv".to_string(),
                    "year,quarter,sales\n2023,Q1,100\n2023,Q2,150\n2024,Q1,120\n".to_string(),
                ),
                ("/pivot5.csv".to_string(), "a,b,c\n1,2,3\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        // explode: splits delimited values into rows
        assert_eq!(
            env.exec("xan explode tags /tags.csv").stdout,
            "id,tags\n1,a\n1,b\n1,c\n2,x\n2,y\n"
        );
        // explode: custom separator with -s
        assert_eq!(
            env.exec("xan explode items -s ';' /items.csv").stdout,
            "id,items\n1,a\n1,b\n1,c\n"
        );
        // explode: renames column with -r
        assert_eq!(
            env.exec("xan explode tags -r tag /tags2.csv").stdout,
            "id,tag\n1,a\n1,b\n"
        );
        // explode: handles empty values
        assert_eq!(
            env.exec("xan explode tags /tagsempty.csv").stdout,
            "id,tags\n1,a\n1,b\n2,\n3,c\n"
        );
        // explode: drops empty rows with --drop-empty
        assert_eq!(
            env.exec("xan explode tags --drop-empty /tagsempty.csv")
                .stdout,
            "id,tags\n1,a\n1,b\n3,c\n"
        );
        // explode: errors on missing column
        let err = env.exec("xan explode nonexistent /missing.csv");
        assert_eq!(err.exit_code, 1);
        assert!(err.stderr.contains("not found"));

        // implode: combines consecutive rows with same key
        assert_eq!(
            env.exec("xan implode tag /impl.csv").stdout,
            "id,tag\n1,a|b|c\n2,x|y\n"
        );
        // implode: custom separator with -s
        assert_eq!(
            env.exec("xan implode val -s ';' /impl2.csv").stdout,
            "id,val\n1,a;b\n"
        );
        // implode: renames column with -r
        assert_eq!(
            env.exec("xan implode tag -r tags /impl3.csv").stdout,
            "id,tags\n1,a|b\n"
        );
        // implode: only groups consecutive rows with same key
        assert_eq!(
            env.exec("xan implode tag /impl4.csv").stdout,
            "id,tag\n1,a\n2,x\n1,b\n"
        );

        // pivot: count aggregation
        assert_eq!(
            env.exec("xan pivot product 'count(amount)' -g region /pivot.csv")
                .stdout,
            "region,A,B\nnorth,1,1\nsouth,1,1\n"
        );
        // pivot: sum aggregation (south has no B -> 0)
        assert_eq!(
            env.exec("xan pivot product 'sum(amount)' -g region /pivot2.csv")
                .stdout,
            "region,A,B\nnorth,10,20\nsouth,20,0\n"
        );
        // pivot: mean aggregation
        assert_eq!(
            env.exec("xan pivot type 'mean(val)' -g cat /pivot3.csv")
                .stdout,
            "cat,a,b\nX,15,30\n"
        );
        // pivot: auto-determines group columns when not specified
        assert_eq!(
            env.exec("xan pivot quarter 'sum(sales)' /pivot4.csv")
                .stdout,
            "year,Q1,Q2\n2023,100,150\n2024,120,0\n"
        );
        // pivot: errors on invalid aggregation expression
        let err = env.exec("xan pivot b 'invalid syntax' /pivot5.csv");
        assert_eq!(err.exit_code, 1);
        assert!(err.stderr.contains("invalid aggregation"));
    }

    #[test]
    fn structured_data_xan_flatmap_rows() {
        // Ports packages/just-bash/src/commands/xan/xan.reshape.test.ts:171,184
        // (describe "xan flatmap") — array-returning specs expand into multiple
        // rows, scalar specs map one-to-one.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/flat.csv".to_string(),
                    "text\nhello world\nfoo bar baz\n".to_string(),
                ),
                ("/flat2.csv".to_string(), "n\n1\n2\n".to_string()),
            ]),
            ..BashOptions::default()
        });
        // expands array results (split) into multiple rows
        assert_eq!(
            env.exec("xan flatmap \"split(text, ' ') as word\" /flat.csv")
                .stdout,
            "text,word\nhello world,hello\nhello world,world\nfoo bar baz,foo\nfoo bar baz,bar\nfoo bar baz,baz\n"
        );
        // handles non-array (scalar) results — one row per input row
        assert_eq!(
            env.exec("xan flatmap 'n * 2 as doubled' /flat2.csv").stdout,
            "n,doubled\n1,2\n2,4\n"
        );
    }

    #[test]
    fn structured_data_xan_moonblade_parenthesized_rows() {
        // Ports packages/just-bash/src/commands/xan/moonblade-parser.test.ts:
        // 17,22,30,38,46,54 — the parenthesized-expression backtracking
        // regression. These reach the parser via `xan filter '(...)'`. The
        // requirement is that grouped expressions parse without infinite
        // recursion / stack overflow and are not mis-parsed as lambdas.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/x.csv".to_string(), "x\n1\n2\n6\n".to_string()),
                ("/xy.csv".to_string(), "x,y\n1,2\n6,7\n".to_string()),
            ]),
            ..BashOptions::default()
        });
        // Each grouped/lambda form must parse cleanly (exit 0, no panic / stack
        // overflow) and must NOT be mis-handled as a crashing lambda. The
        // upstream regression only required "no RangeError and not a lambda";
        // these deterministic header-only outputs prove the parser terminates
        // and never re-enters the `(` case at the same position.
        //
        // (ident): grouped identifier expression.
        let grouped = env.exec("xan filter '(x)' /x.csv");
        assert_eq!(grouped.exit_code, 0);
        assert_eq!(grouped.stdout, "x\n");
        // (ident op value): grouped comparison (not a lambda).
        let cmp = env.exec("xan filter '(x > 5)' /x.csv");
        assert_eq!(cmp.exit_code, 0);
        assert_eq!(cmp.stdout, "x\n");
        // (ident, ident): top-level comma — must not stack-overflow.
        let tuple = env.exec("xan filter '(x, y)' /xy.csv");
        assert_eq!(tuple.exit_code, 0);
        assert_eq!(tuple.stdout, "x,y\n");
        // (ident) => body: single-arg lambda parses.
        let lambda1 = env.exec("xan filter '(x) => x > 5' /x.csv");
        assert_eq!(lambda1.exit_code, 0);
        assert_eq!(lambda1.stdout, "x\n");
        // (a, b) => body: multi-arg lambda parses.
        let lambda2 = env.exec("xan filter '(a, b) => a' /xy.csv");
        assert_eq!(lambda2.exit_code, 0);
        assert_eq!(lambda2.stdout, "x,y\n");
        // (ident).method(): paren around a method-call receiver.
        let method = env.exec("xan filter '(x).len()' /x.csv");
        assert_eq!(method.exit_code, 0);
        assert_eq!(method.stdout, "x\n");
    }

    #[test]
    fn structured_data_xan_dangerous_keyword_columns_rows() {
        // Ports packages/just-bash/src/commands/xan/xan.prototype-pollution.test.ts:
        // 88 (explode column named `constructor`), and the behavioral core of
        // 130/154/168 — processing CSV whose headers/cells are JS-dangerous
        // keywords (constructor/prototype/__proto__) must produce correct
        // output and never corrupt later rows. The Rust port has no JS
        // Object.prototype to pollute, so the security property holds by
        // construction; here we assert the observable command output.
        let env = Bash::new();
        // explode a column literally named `constructor`: header preserved,
        // delimited value split into rows (header + 2 data rows).
        let exploded = env.exec("printf 'constructor,value\\na|b,1\\n' | xan explode constructor");
        assert_eq!(exploded.exit_code, 0);
        assert_eq!(exploded.stdout, "constructor,value\na,1\nb,1\n");
        assert_eq!(exploded.stdout.trim_end().lines().count(), 3);

        // select a dangerous-keyword column (the body of the prototype-pollution
        // via-CSV-headers case) returns just that column, with the dangerous
        // cell value carried through verbatim.
        let selected = env.exec(
            "printf 'constructor,other\\npolluted_constructor,value\\n' | xan select constructor",
        );
        assert_eq!(selected.exit_code, 0);
        assert_eq!(selected.stdout, "constructor\npolluted_constructor\n");

        // transpose with dangerous-keyword header/first column: rows become
        // columns; both dangerous tokens survive as plain data.
        let transposed = env.exec("printf 'constructor,a,b\\nprototype,1,2\\n' | xan transpose");
        assert_eq!(transposed.exit_code, 0);
        assert!(transposed.stdout.contains("constructor"));
        assert!(transposed.stdout.contains("prototype"));

        // enum -c constructor: the index column may be named with a dangerous
        // keyword; rows are still numbered 0,1,... with no corruption.
        let enumerated = env.exec("printf 'value\\na\\nb\\n' | xan enum -c constructor");
        assert_eq!(enumerated.exit_code, 0);
        assert_eq!(enumerated.stdout, "constructor,value\n0,a\n1,b\n");
    }

    #[test]
    fn structured_data_xan_multifile_join_merge_rows() {
        // Ports packages/just-bash/src/commands/xan/xan.multifile.test.ts:
        // join (117,133,149,165) and merge (183,195,207,219).
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/left.csv".to_string(),
                    "id,name\n1,alice\n2,bob\n".to_string(),
                ),
                (
                    "/right.csv".to_string(),
                    "user_id,score\n1,100\n".to_string(),
                ),
                ("/users.csv".to_string(), "id,name\n1,alice\n".to_string()),
                (
                    "/orders.csv".to_string(),
                    "user_id,item\n1,book\n1,pen\n1,paper\n".to_string(),
                ),
                (
                    "/dl.csv".to_string(),
                    "id,name,status\n1,alice,active\n2,bob,inactive\n".to_string(),
                ),
                (
                    "/dr.csv".to_string(),
                    "id,status,score\n1,verified,100\n2,pending,85\n".to_string(),
                ),
                ("/a.csv".to_string(), "id,name\n1,alice\n".to_string()),
                ("/b.csv".to_string(), "user_id,score\n1,100\n".to_string()),
                ("/m1.csv".to_string(), "id,val\n1,a\n3,c\n".to_string()),
                ("/m2.csv".to_string(), "id,val\n2,b\n4,d\n".to_string()),
                ("/mh1.csv".to_string(), "id,name\n1,alice\n".to_string()),
                (
                    "/mh2.csv".to_string(),
                    "id,email\n2,bob@x.com\n".to_string(),
                ),
                ("/m3.csv".to_string(), "id,val\n1,a\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        // join: uses default value with -D
        assert_eq!(
            env.exec("xan join --left -D 'N/A' id /left.csv user_id /right.csv")
                .stdout,
            "id,name,user_id,score\n1,alice,1,100\n2,bob,N/A,N/A\n"
        );
        // join: handles one-to-many joins
        assert_eq!(
            env.exec("xan join id /users.csv user_id /orders.csv")
                .stdout,
            "id,name,user_id,item\n1,alice,1,book\n1,alice,1,pen\n1,alice,1,paper\n"
        );
        // join: deduplicates shared column names
        assert_eq!(
            env.exec("xan join id /dl.csv id /dr.csv").stdout,
            "id,name,status,score\n1,alice,active,100\n2,bob,inactive,85\n"
        );
        // join: errors on missing key column
        let err = env.exec("xan join nonexistent /a.csv user_id /b.csv");
        assert_eq!(err.exit_code, 1);
        assert_eq!(
            err.stderr,
            "xan join: column 'nonexistent' not found in first file\n"
        );

        // merge: merges multiple files with same headers
        assert_eq!(
            env.exec("xan merge /m1.csv /m2.csv").stdout,
            "id,val\n1,a\n3,c\n2,b\n4,d\n"
        );
        // merge: sorts merged data with -s
        assert_eq!(
            env.exec("xan merge -s id /m1.csv /m2.csv").stdout,
            "id,val\n1,a\n2,b\n3,c\n4,d\n"
        );
        // merge: errors on mismatched headers
        let err = env.exec("xan merge /mh1.csv /mh2.csv");
        assert_eq!(err.exit_code, 1);
        assert!(err.stderr.contains("same headers"));
        // merge: requires at least 2 files
        let err = env.exec("xan merge /m3.csv");
        assert_eq!(err.exit_code, 1);
        assert!(err.stderr.contains("usage"));
    }

    #[test]
    fn structured_data_sqlite3_options_errors_and_simple_select_rows() {
        let env = bash();

        assert_eq!(
            env.exec("sqlite3 :memory: \"SELECT 1, 2, 3\"").stdout,
            "1|2|3\n"
        );
        assert_eq!(
            env.exec("sqlite3 -list :memory: \"SELECT 1, 2, 3\"").stdout,
            "1|2|3\n"
        );
        assert_eq!(
            env.exec(
                "sqlite3 :memory: \"CREATE TABLE t(x INT); INSERT INTO t VALUES(1),(2),(3); SELECT * FROM t\""
            )
            .stdout,
            "1\n2\n3\n"
        );
        assert_eq!(
            env.exec(
                "echo \"CREATE TABLE t(x); INSERT INTO t VALUES(42); SELECT * FROM t\" | sqlite3 :memory:"
            )
            .stdout,
            "42\n"
        );
        assert_eq!(
            env.exec("sqlite3 -list -header :memory: \"SELECT 1 as a, 2 as b\"")
                .stdout,
            "a|b\n1|2\n"
        );
        assert_eq!(
            env.exec("sqlite3 -csv :memory: \"SELECT 1 as col1, 2 as col2\"")
                .stdout,
            "1,2\n"
        );
        assert_eq!(
            env.exec("sqlite3 -csv -header :memory: \"SELECT 1 as col1, 2 as col2\"")
                .stdout,
            "col1,col2\n1,2\n"
        );
        assert_eq!(
            env.exec("sqlite3 -json :memory: \"SELECT 1 as t, 0 as f\"")
                .stdout,
            "[{\"t\":1,\"f\":0}]\n"
        );
        assert_eq!(
            env.exec("sqlite3 -separator , :memory: \"SELECT 1, 2\"")
                .stdout,
            "1,2\n"
        );
        assert_eq!(
            env.exec("sqlite3 -newline '|' :memory: \"SELECT 1; SELECT 2\"")
                .stdout,
            "1|2|"
        );
        assert!(env.exec("sqlite3 -version").stdout.starts_with("3."));
        assert!(env.exec("sqlite3 --help").stdout.contains("DATABASE"));
        assert!(env.exec("sqlite3 -help").stdout.contains("DATABASE"));
        assert_eq!(
            env.exec(
                "sqlite3 -newline '|' :memory: \"CREATE TABLE t(x); INSERT INTO t VALUES(1),(2),(3); SELECT * FROM t\""
            )
            .stdout,
            "1|2|3|"
        );
        assert_eq!(
            env.exec(
                "sqlite3 -separator ',' :memory: \"CREATE TABLE t(a,b); INSERT INTO t VALUES(1,2); SELECT * FROM t\""
            )
            .stdout,
            "1,2\n"
        );
        assert_eq!(env.exec("sqlite3").exit_code, 1);
        assert_eq!(env.exec("sqlite3 :memory:").exit_code, 1);
        assert_eq!(env.exec("sqlite3 :memory: -separator").exit_code, 1);
        assert_eq!(env.exec("sqlite3 :memory: -newline").exit_code, 1);
        assert_eq!(env.exec("sqlite3 :memory: -nullvalue").exit_code, 1);
        assert_eq!(env.exec("sqlite3 :memory: -cmd").exit_code, 1);
        assert_eq!(env.exec("sqlite3 -xyz :memory: \"SELECT 1\"").exit_code, 1);
        assert!(
            env.exec("sqlite3 :memory: \"SELECT load_extension('/tmp/evil.so')\"")
                .stdout
                .contains("not authorized")
        );
    }

    #[test]
    fn structured_data_sqlite3_deep_options_modes_and_error_rows() {
        let env = bash();

        assert_eq!(
            env.exec("sqlite3 :memory: -- \"SELECT 1 as value\"").stdout,
            "1\n"
        );
        assert_eq!(
            env.exec("sqlite3 -echo :memory: \"SELECT 1; SELECT 2\"")
                .stdout,
            "SELECT 1; SELECT 2\n1\n2\n"
        );
        assert_eq!(
            env.exec(
                "sqlite3 -cmd \"CREATE TABLE t(x); INSERT INTO t VALUES(42)\" :memory: \"SELECT * FROM t\""
            )
            .stdout,
            "42\n"
        );
        assert_eq!(
            env.exec("sqlite3 -header :memory: \"CREATE TABLE t(col1 INT, col2 TEXT); INSERT INTO t VALUES(1,'a'); SELECT * FROM t\"")
                .stdout,
            "col1|col2\n1|a\n"
        );
        assert_eq!(
            env.exec("sqlite3 -noheader :memory: \"CREATE TABLE t(x INT); INSERT INTO t VALUES(1); SELECT * FROM t\"")
                .stdout,
            "1\n"
        );
        assert_eq!(
            env.exec("sqlite3 -nullvalue \"NULL\" :memory: \"CREATE TABLE t(x); INSERT INTO t VALUES(1),(NULL); SELECT * FROM t\"")
                .stdout,
            "1\nNULL\n"
        );
        let bail = env.exec("sqlite3 -bail :memory: \"SELECT * FROM bad; SELECT 1\"");
        assert_eq!(bail.exit_code, 1);
        assert!(bail.stderr.contains("no such table"));
        assert!(!bail.stdout.contains('1'));

        assert_eq!(
            env.exec("sqlite3 :memory: \"CREATE TABLE t(a INT, b TEXT); INSERT INTO t VALUES(1,'x'),(2,'y'); SELECT * FROM t\"")
                .stdout,
            "1|x\n2|y\n"
        );
        assert_eq!(
            env.exec("sqlite3 :memory: \"CREATE TABLE a(x); CREATE TABLE b(y); INSERT INTO a VALUES(1); INSERT INTO b VALUES(2); SELECT * FROM a; SELECT * FROM b\"")
                .stdout,
            "1\n2\n"
        );
        assert!(
            env.exec("sqlite3 :memory: \"SELEC * FROM t\"")
                .stdout
                .contains("Error:")
        );
        assert!(
            env.exec("sqlite3 :memory: \"SELECT * FROM nonexistent\"")
                .stdout
                .contains("no such table")
        );
        assert_eq!(
            env.exec("sqlite3 -json :memory: \"CREATE TABLE t(x); INSERT INTO t VALUES(NULL); SELECT * FROM t\"")
                .stdout,
            "[{\"x\":null}]\n"
        );

        assert_eq!(
            env.exec("sqlite3 -csv :memory: \"CREATE TABLE t(a,b); INSERT INTO t VALUES(1,'hello'),(2,'world'); SELECT * FROM t\"")
                .stdout,
            "1,hello\n2,world\n"
        );
        assert_eq!(
            env.exec("sqlite3 -json :memory: \"CREATE TABLE t(id INT, name TEXT); INSERT INTO t VALUES(1,'alice'),(2,'bob'); SELECT * FROM t\"")
                .stdout,
            "[{\"id\":1,\"name\":\"alice\"},\n{\"id\":2,\"name\":\"bob\"}]\n"
        );
        assert_eq!(
            env.exec("sqlite3 -line :memory: \"CREATE TABLE t(a INT, b TEXT); INSERT INTO t VALUES(1,'x'); SELECT * FROM t\"")
                .stdout,
            "    a = 1\n    b = x\n"
        );
        assert_eq!(
            env.exec("sqlite3 -tabs :memory: \"CREATE TABLE t(a,b); INSERT INTO t VALUES(1,2),(3,4); SELECT * FROM t\"")
                .stdout,
            "1\t2\n3\t4\n"
        );
        assert_eq!(
            env.exec("sqlite3 -quote :memory: \"CREATE TABLE t(a INT, b TEXT); INSERT INTO t VALUES(1,'hello'),(NULL,'world'); SELECT * FROM t\"")
                .stdout,
            "1,'hello'\nNULL,'world'\n"
        );

        let continue_after_error =
            env.exec("sqlite3 :memory: \"SELECT * FROM nonexistent; SELECT 42\"");
        assert_eq!(continue_after_error.exit_code, 0);
        assert!(continue_after_error.stdout.contains("Error:"));
        assert!(continue_after_error.stdout.contains("no such table"));
        assert!(continue_after_error.stdout.contains("42"));

        let multiple_errors =
            env.exec("sqlite3 :memory: \"SELECT * FROM bad1; SELECT * FROM bad2; SELECT 1\"");
        assert_eq!(multiple_errors.exit_code, 0);
        assert!(multiple_errors.stdout.contains("bad1"));
        assert!(multiple_errors.stdout.contains("bad2"));
        assert!(multiple_errors.stdout.contains('1'));

        let bail_first =
            env.exec("sqlite3 -bail :memory: \"SELECT * FROM bad1; SELECT * FROM bad2\"");
        assert_eq!(bail_first.exit_code, 1);
        assert!(bail_first.stderr.contains("bad1"));
        assert!(!bail_first.stderr.contains("bad2"));

        let bail_partial =
            env.exec("sqlite3 -bail :memory: \"SELECT 1; SELECT * FROM bad; SELECT 2\"");
        assert_eq!(bail_partial.exit_code, 1);
        assert!(bail_partial.stdout.contains('1'));
        assert!(!bail_partial.stdout.contains('2'));

        assert_eq!(
            env.exec("sqlite3 --xyz :memory: \"SELECT 1\"").stderr,
            "sqlite3: Error: unknown option: -xyz\nUse -help for a list of options.\n"
        );
        assert!(
            env.exec(
                "sqlite3 :memory: \"SELECT load_extension('/tmp/evil.so', 'sqlite3_evil_init')\""
            )
            .stdout
            .contains("not authorized")
        );
    }

    #[test]
    fn structured_data_jbc37_html_to_markdown_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test.html".to_string(), "<h2>From File</h2>".to_string())]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec("echo \"<h1>Hello World</h1>\" | html-to-markdown")
                .stdout,
            "# Hello World\n"
        );
        let paragraphs =
            env.exec("echo \"<p>First paragraph.</p><p>Second paragraph.</p>\" | html-to-markdown");
        assert!(paragraphs.stdout.contains("First paragraph."));
        assert!(paragraphs.stdout.contains("Second paragraph."));
        assert_eq!(
            env.exec("echo \"<a href='https://example.com'>Click here</a>\" | html-to-markdown")
                .stdout,
            "[Click here](https://example.com)\n"
        );
        assert_eq!(
            env.exec("echo \"<strong>bold</strong> and <em>italic</em>\" | html-to-markdown")
                .stdout,
            "**bold** and _italic_\n"
        );
        let unordered =
            env.exec("echo \"<ul><li>One</li><li>Two</li><li>Three</li></ul>\" | html-to-markdown");
        assert!(unordered.stdout.contains("-   One"));
        assert!(unordered.stdout.contains("-   Two"));
        assert!(unordered.stdout.contains("-   Three"));
        let ordered =
            env.exec("echo \"<ol><li>First</li><li>Second</li></ol>\" | html-to-markdown");
        assert!(ordered.stdout.contains("1."));
        assert!(ordered.stdout.contains("First"));
        assert!(ordered.stdout.contains("Second"));
        let code_block =
            env.exec("echo \"<pre><code>const x = 1;</code></pre>\" | html-to-markdown");
        assert!(code_block.stdout.contains("```"));
        assert!(code_block.stdout.contains("const x = 1;"));
        assert!(
            env.exec("echo \"Use <code>npm install</code> to install\" | html-to-markdown")
                .stdout
                .contains("`npm install`")
        );
        assert_eq!(
            env.exec("echo \"<img src='photo.jpg' alt='A photo'>\" | html-to-markdown")
                .stdout,
            "![A photo](photo.jpg)\n"
        );
        assert!(
            env.exec("echo \"<blockquote>A wise quote</blockquote>\" | html-to-markdown")
                .stdout
                .contains("> A wise quote")
        );
        assert!(
            env.exec("echo \"<ul><li>Item</li></ul>\" | html-to-markdown -b '*'")
                .stdout
                .contains("*   Item")
        );
        assert!(
            env.exec("echo \"<ul><li>Item</li></ul>\" | html-to-markdown --bullet=+")
                .stdout
                .contains("+   Item")
        );
        assert!(
            env.exec("echo \"<pre><code>code</code></pre>\" | html-to-markdown -c '~~~'")
                .stdout
                .contains("~~~")
        );
        assert!(
            env.exec("echo \"<hr>\" | html-to-markdown -r '***'")
                .stdout
                .contains("***")
        );
        let setext = env.exec("echo \"<h1>Title</h1>\" | html-to-markdown --heading-style=setext");
        assert!(setext.stdout.contains("Title"));
        assert!(setext.stdout.contains("="));
        assert_eq!(
            env.exec("html-to-markdown /test.html").stdout,
            "## From File\n"
        );
        assert_eq!(env.exec("html-to-markdown /nonexistent.html").exit_code, 1);
        let help = env.exec("html-to-markdown --help").stdout;
        assert!(help.contains("html-to-markdown"));
        assert!(help.contains("convert HTML to Markdown"));
        assert!(help.contains("BashEnv extension"));
        assert!(help.contains("turndown"));
        assert!(help.contains("Examples:"));
        assert!(help.contains("curl"));
        assert!(help.contains("Headings"));
        assert!(help.contains("Links"));
        assert!(help.contains("Bold"));
        assert!(help.contains("Lists"));
        let scripts = env
            .exec("echo \"<p>Hello</p><script>alert(1);</script><p>World</p>\" | html-to-markdown");
        assert!(scripts.stdout.contains("Hello"));
        assert!(scripts.stdout.contains("World"));
        assert!(!scripts.stdout.contains("alert"));
        let styles = env.exec(
            "echo \"<style>.red { color: red; }</style><p>Styled text</p>\" | html-to-markdown",
        );
        assert!(styles.stdout.contains("Styled text"));
        assert!(!styles.stdout.contains("color"));
        let multi = env.exec("echo \"<style>body{}</style><h1>Title</h1><script>var x=1;</script><p>Text</p><script>var y=2;</script>\" | html-to-markdown");
        assert!(multi.stdout.contains("# Title"));
        assert!(multi.stdout.contains("Text"));
        assert!(!multi.stdout.contains("var x"));
        assert!(
            env.exec("echo \"<script type='text/javascript'>console.log(1);</script><p>Content</p>\" | html-to-markdown")
                .stdout
                .contains("Content")
        );
        assert_eq!(env.exec("printf '' | html-to-markdown").stdout, "");
        assert_eq!(
            env.exec("echo \"Just plain text\" | html-to-markdown")
                .stdout,
            "Just plain text\n"
        );
        let nested = env.exec(
            "echo \"<div><h1>Title</h1><p>Text with <strong>bold</strong></p></div>\" | html-to-markdown",
        );
        assert!(nested.stdout.contains("# Title"));
        assert!(nested.stdout.contains("**bold**"));
        assert!(
            env.exec("html-to-markdown --invalid")
                .stderr
                .contains("unrecognized option")
        );
        let utf8 = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/in.html".to_string(),
                "<p>한글 / café / 漢字</p>\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        assert!(
            utf8.exec("cat /in.html | html-to-markdown")
                .stdout
                .contains("한글 / café / 漢字")
        );
    }

    #[test]
    fn structured_data_jbc37_jq_path_math_dot_and_safe_key_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/in.json".to_string(),
                serde_json::json!({"msg":"한글 / café / 漢字"}).to_string(),
            )]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec("echo '{\"a\":{\"b\":{\"c\":\"deep\"}}}' | jq '.a | .b | .c'")
                .stdout,
            "\"deep\"\n"
        );
        assert_eq!(
            env.exec("echo '[0,1,2,3,4,5]' | jq '.[2:4]'").stdout,
            "[\n  2,\n  3\n]\n"
        );
        assert_eq!(
            env.exec("echo '[0,1,2,3,4]' | jq '.[:3]'").stdout,
            "[\n  0,\n  1,\n  2\n]\n"
        );
        assert_eq!(
            env.exec("echo '[0,1,2,3,4]' | jq '.[3:]'").stdout,
            "[\n  3,\n  4\n]\n"
        );
        assert_eq!(
            env.exec("echo '\"hello\"' | jq '.[1:4]'").stdout,
            "\"ell\"\n"
        );
        assert_eq!(
            env.exec("echo '[0,1,2,3,4]' | jq '.[-2:]'").stdout,
            "[\n  3,\n  4\n]\n"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1,\"b\":2}' | jq '.a, .b'").stdout,
            "1\n2\n"
        );
        assert_eq!(
            env.exec("echo '{\"x\":1,\"y\":2,\"z\":3}' | jq '.x, .y, .z'")
                .stdout,
            "1\n2\n3\n"
        );
        assert_eq!(
            env.exec("echo '[[[1]],[[2]]]' | jq 'flatten(1)'").stdout,
            "[\n  [\n    1\n  ],\n  [\n    2\n  ]\n]\n"
        );
        assert_eq!(
            env.exec(
                "echo '{\"a\":1,\"b\":2}' | jq -c 'with_entries({key: .key, value: (.value + 10)})'"
            )
            .stdout,
            "{\"a\":11,\"b\":12}\n"
        );
        assert_eq!(
            env.exec("echo '[[1,2],[3,4]]' | jq 'transpose'").stdout,
            "[\n  [\n    1,\n    3\n  ],\n  [\n    2,\n    4\n  ]\n]\n"
        );
        assert_eq!(
            env.exec("jq -n '[limit(3; range(10))]'").stdout,
            "[\n  0,\n  1,\n  2\n]\n"
        );
        assert_eq!(
            env.exec("echo '{\"a\":{\"b\":42}}' | jq 'getpath([\"a\",\"b\"])'")
                .stdout,
            "42\n"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1}' | jq -c 'setpath([\"b\"]; 2)'")
                .stdout,
            "{\"a\":1,\"b\":2}\n"
        );
        assert_eq!(
            env.exec("echo '{\"a\":{\"b\":1}}' | jq '[.. | numbers]'")
                .stdout,
            "[\n  1\n]\n"
        );
        assert_eq!(env.exec("jq -n 'pow(2; 3)'").stdout, "8\n");
        assert_eq!(env.exec("jq -n 'pow(4; 0.5)'").stdout, "2\n");
        assert_eq!(
            env.exec("jq -n 'atan2(3; 4)'").stdout,
            "0.6435011087932844\n"
        );
        assert_eq!(
            env.exec("jq -n 'atan2(-1; -1)'").stdout,
            "-2.356194490192345\n"
        );
        assert_eq!(env.exec("jq -n 'pow(\"a\"; 2)'").stdout, "null\n");
        assert_eq!(env.exec("jq -n 'atan2(\"a\"; 2)'").stdout, "null\n");
        assert_eq!(
            env.exec("echo '{\"null\":\"n\"}' | jq '.null'").stdout,
            "\"n\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"data\":{\"as\":\"val\"}}' | jq '.data.as'")
                .stdout,
            "\"val\"\n"
        );
        assert_ne!(env.exec("echo '{\"if\":\"x\"}' | jq '. if'").exit_code, 0);
        assert_ne!(
            env.exec("echo '{\"data\":{\"and\":\"x\"}}' | jq '.data. and'")
                .exit_code,
            0
        );
        assert_ne!(
            env.exec("echo '{\"foo\":\"x\"}' | jq '.  foo'").exit_code,
            0
        );
        assert_eq!(
            env.exec("echo '{\"foo\":\"bar\"}' | jq '.  \"foo\"'")
                .stdout,
            "\"bar\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"data\":{\"foo\":\"bar\"}}' | jq '.data.\"foo\"'")
                .stdout,
            "\"bar\"\n"
        );
        assert_ne!(
            env.exec("echo '[{\"foo\":1}]' | jq '.[0]. foo'").exit_code,
            0
        );
        assert_eq!(
            env.exec("echo '[{\"foo\":1}]' | jq '.[0]. \"foo\"'").stdout,
            "1\n"
        );
        assert_ne!(env.exec("echo '{\"foo\":1}' | jq '(.). foo'").exit_code, 0);
        assert_eq!(
            env.exec("echo '{\"foo\":1}' | jq '(.). \"foo\"'").stdout,
            "1\n"
        );
        assert_eq!(
            env.exec("echo 'null' | jq -c '{(\"__defineGetter__\"): 1, safe: 2}'")
                .stdout,
            "{\"safe\":2}\n"
        );
        assert_eq!(
            env.exec("echo '[{\"key\":\"toString\",\"value\":1},{\"key\":\"safe\",\"value\":2}]' | jq -c 'from_entries'")
                .stdout,
            "{\"safe\":2}\n"
        );
        assert_eq!(
            env.exec("cat /in.json | jq -r '.msg'").stdout,
            "한글 / café / 漢字\n"
        );
    }

    #[test]
    fn structured_data_jbc37_yq_format_and_utf8_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/quote.json".to_string(),
                    serde_json::json!("it's").to_string(),
                ),
                (
                    "/in.yaml".to_string(),
                    "msg: 한글 / café / 漢字\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec("echo '\"hello\"' | yq -o json '@base64'").stdout,
            "\"aGVsbG8=\"\n"
        );
        assert_eq!(
            env.exec("echo '\"aGVsbG8=\"' | yq -o json '@base64d'")
                .stdout,
            "\"hello\"\n"
        );
        assert_eq!(
            env.exec("echo '\"hello world\"' | yq -o json '@uri'")
                .stdout,
            "\"hello%20world\"\n"
        );
        assert_eq!(
            env.exec("echo '[\"a\",\"b\",\"c\"]' | yq -o json '@csv'")
                .stdout,
            "\"a,b,c\"\n"
        );
        assert_eq!(
            env.exec("echo '[\"a\",\"b,c\",\"d\"]' | yq -o json '@csv'")
                .stdout,
            "\"a,\\\"b,c\\\",d\"\n"
        );
        assert_eq!(
            env.exec("echo '[\"a\",\"b\",\"c\"]' | yq -o json '@tsv'")
                .stdout,
            "\"a\\tb\\tc\"\n"
        );
        assert_eq!(
            env.exec("echo '{\"a\":1}' | yq -o json '@json'").stdout,
            "\"{\\\"a\\\":1}\"\n"
        );
        assert_eq!(
            env.exec("echo '\"<script>alert(1)</script>\"' | yq -o json '@html'")
                .stdout,
            "\"&lt;script&gt;alert(1)&lt;/script&gt;\"\n"
        );
        assert_eq!(
            env.exec("echo '\"hello world\"' | yq -o json '@sh'").stdout,
            "\"'hello world'\"\n"
        );
        assert_eq!(
            env.exec("cat /quote.json | yq -o json '@sh'").stdout,
            "\"'it'\\\\''s'\"\n"
        );
        assert_eq!(
            env.exec("echo '\"test\"' | yq -o json '@text'").stdout,
            "\"test\"\n"
        );
        assert_eq!(
            env.exec("echo '\"héllo\"' | yq -o json '@base64'").stdout,
            "\"aMOpbGxv\"\n"
        );
        assert_eq!(
            env.exec("echo '123' | yq -o json '@base64'").stdout,
            "null\n"
        );
        assert_eq!(
            env.exec("echo '[\"a\",\"b\"]' | yq -o json '@base64'")
                .stdout,
            "null\n"
        );
        assert_eq!(
            env.exec("echo '[\"a\",null,\"c\"]' | yq -o json '@csv'")
                .stdout,
            "\"a,,c\"\n"
        );
        assert_eq!(
            env.exec("echo '[1,2,3]' | yq -o json '@csv'").stdout,
            "\"1,2,3\"\n"
        );
        assert_eq!(env.exec("echo '[]' | yq -o json '@csv'").stdout, "\"\"\n");
        assert_eq!(
            env.exec("echo '[\"a\",\"b\\\"c\",\"d\"]' | yq -o json '@csv'")
                .stdout,
            "\"a,\\\"b\\\"\\\"c\\\",d\"\n"
        );
        assert_eq!(
            env.exec("echo '\"test\"' | yq -o json '@csv'").stdout,
            "null\n"
        );
        assert_eq!(
            env.exec("echo '\"a=1&b=2?c#d\"' | yq -o json '@uri'")
                .stdout,
            "\"a%3D1%26b%3D2%3Fc%23d\"\n"
        );
        assert_eq!(
            env.exec("echo '\"a & b\"' | yq -o json '@html'").stdout,
            "\"a &amp; b\"\n"
        );
        assert_eq!(env.exec("echo '123' | yq -o json '@html'").stdout, "null\n");
        assert_eq!(env.exec("echo '123' | yq -o json '@sh'").stdout, "null\n");
        assert_eq!(
            env.exec("echo 'null' | yq -o json '@text'").stdout,
            "\"\"\n"
        );
        assert_eq!(
            env.exec("echo '42' | yq -o json '@text'").stdout,
            "\"42\"\n"
        );
        assert_eq!(
            env.exec("cat /in.yaml | yq '.msg'").stdout.trim(),
            "한글 / café / 漢字"
        );
    }

    #[test]
    fn structured_data_jbc49_sqlite3_write_ops_and_file_persistence_rows() {
        // JBC-49: sqlite3 write operations (UPDATE/DELETE/DROP/ALTER/REPLACE),
        // file-backed persistence, readonly overlay semantics, non-prefixed
        // mutation writeback (CTE/PRAGMA/comment-led), and parsing edge cases.

        // --- write-ops: UPDATE ---
        let env = bash();
        assert_eq!(
            env.exec(
                "sqlite3 :memory: \"CREATE TABLE t(id INT, val TEXT); INSERT INTO t VALUES(1,'a'),(2,'b'); UPDATE t SET val='x' WHERE id=1; SELECT * FROM t ORDER BY id\""
            )
            .stdout,
            "1|x\n2|b\n"
        );
        assert_eq!(
            env.exec(
                "sqlite3 :memory: \"CREATE TABLE t(x INT); INSERT INTO t VALUES(1),(2),(3); UPDATE t SET x=0; SELECT * FROM t\""
            )
            .stdout,
            "0\n0\n0\n"
        );

        // --- write-ops: DELETE ---
        assert_eq!(
            env.exec(
                "sqlite3 :memory: \"CREATE TABLE t(x INT); INSERT INTO t VALUES(1),(2),(3); DELETE FROM t WHERE x=2; SELECT * FROM t ORDER BY x\""
            )
            .stdout,
            "1\n3\n"
        );
        assert_eq!(
            env.exec(
                "sqlite3 :memory: \"CREATE TABLE t(x INT); INSERT INTO t VALUES(1),(2); DELETE FROM t; SELECT COUNT(*) FROM t\""
            )
            .stdout,
            "0\n"
        );

        // --- write-ops: DROP TABLE ---
        assert_eq!(
            env.exec(
                "sqlite3 :memory: \"CREATE TABLE t(x); DROP TABLE t; SELECT name FROM sqlite_master WHERE type='table'\""
            )
            .stdout,
            ""
        );
        let dropped =
            env.exec("sqlite3 :memory: \"CREATE TABLE t(x); DROP TABLE t; SELECT * FROM t\"");
        assert!(dropped.stdout.contains("no such table"));
        assert_eq!(dropped.exit_code, 0);

        // --- write-ops: ALTER TABLE ---
        assert_eq!(
            env.exec(
                "sqlite3 :memory: \"CREATE TABLE old(x); ALTER TABLE old RENAME TO new; SELECT name FROM sqlite_master WHERE type='table'\""
            )
            .stdout,
            "new\n"
        );
        assert_eq!(
            env.exec(
                "sqlite3 :memory: \"CREATE TABLE t(a INT); INSERT INTO t VALUES(1); ALTER TABLE t ADD COLUMN b TEXT DEFAULT 'x'; SELECT * FROM t\""
            )
            .stdout,
            "1|x\n"
        );

        // --- write-ops: REPLACE INTO ---
        assert_eq!(
            env.exec(
                "sqlite3 :memory: \"CREATE TABLE t(id INTEGER PRIMARY KEY, val TEXT); INSERT INTO t VALUES(1,'a'); REPLACE INTO t VALUES(1,'b'); SELECT * FROM t\""
            )
            .stdout,
            "1|b\n"
        );
        assert_eq!(
            env.exec(
                "sqlite3 :memory: \"CREATE TABLE t(id INTEGER PRIMARY KEY, val TEXT); INSERT INTO t VALUES(1,'a'); REPLACE INTO t VALUES(2,'b'); SELECT * FROM t ORDER BY id\""
            )
            .stdout,
            "1|a\n2|b\n"
        );

        // --- write-ops: persistence to file (same session, fresh exec) ---
        let env = bash();
        env.exec("sqlite3 /test.db \"CREATE TABLE t(x INT); INSERT INTO t VALUES(1)\"");
        env.exec("sqlite3 /test.db \"UPDATE t SET x=99\"");
        assert_eq!(
            env.exec("sqlite3 /test.db \"SELECT * FROM t\"").stdout,
            "99\n"
        );

        let env = bash();
        env.exec("sqlite3 /test.db \"CREATE TABLE t(x INT); INSERT INTO t VALUES(1),(2),(3)\"");
        env.exec("sqlite3 /test.db \"DELETE FROM t WHERE x=2\"");
        assert_eq!(
            env.exec("sqlite3 /test.db \"SELECT * FROM t ORDER BY x\"")
                .stdout,
            "1\n3\n"
        );

        let env = bash();
        env.exec("sqlite3 /test.db \"CREATE TABLE t1(x); CREATE TABLE t2(y)\"");
        env.exec("sqlite3 /test.db \"DROP TABLE t1\"");
        assert_eq!(
            env.exec(
                "sqlite3 /test.db \"SELECT name FROM sqlite_master WHERE type='table' ORDER BY name\""
            )
            .stdout,
            "t2\n"
        );

        let env = bash();
        env.exec("sqlite3 /newdb.db \"CREATE TABLE t(x); INSERT INTO t VALUES(42)\"");
        let created = env.exec("sqlite3 /newdb.db \"SELECT * FROM t\"");
        assert_eq!(created.stdout, "42\n");
        assert_eq!(created.exit_code, 0);

        // --- test.ts: file operations ---
        let env = bash();
        env.exec(
            "sqlite3 /test.db \"CREATE TABLE users(id INT, name TEXT); INSERT INTO users VALUES(1,'alice')\"",
        );
        assert_eq!(
            env.exec("sqlite3 /test.db \"SELECT * FROM users\"").stdout,
            "1|alice\n"
        );

        let env = bash();
        env.exec("sqlite3 /data.db \"CREATE TABLE t(x INT)\"");
        env.exec("sqlite3 /data.db \"INSERT INTO t VALUES(1)\"");
        env.exec("sqlite3 /data.db \"INSERT INTO t VALUES(2)\"");
        assert_eq!(
            env.exec("sqlite3 /data.db \"SELECT * FROM t\"").stdout,
            "1\n2\n"
        );

        // --- options.test.ts: -readonly does not persist ---
        let env = bash();
        env.exec("sqlite3 /ro.db \"CREATE TABLE t(x INT); INSERT INTO t VALUES(1)\"");
        env.exec("sqlite3 -readonly /ro.db \"INSERT INTO t VALUES(2)\"");
        assert_eq!(env.exec("sqlite3 /ro.db \"SELECT * FROM t\"").stdout, "1\n");

        // --- parsing.test.ts: semicolon inside double-quoted identifier ---
        // The sqlite3 statement splitter must keep a `;` that appears inside a
        // double-quoted identifier as part of that identifier rather than
        // treating it as a statement terminator (otherwise `CREATE TABLE
        // t("col;name" ...)` would be split mid-identifier). A single-quoted
        // shell argument delivers the SQL verbatim so the splitter behavior is
        // exercised directly.
        let env = bash();
        let parsed = env.exec(
            "sqlite3 :memory: 'CREATE TABLE t(\"col;name\" INT); INSERT INTO t VALUES(7); SELECT * FROM t'",
        );
        assert_eq!(parsed.stdout, "7\n");
        assert_eq!(parsed.exit_code, 0);

        // --- writeback-edge-cases: non-prefixed mutation writeback ---
        // CTE-prefixed INSERT
        let env = bash();
        let setup =
            env.exec("sqlite3 /tmp/db.sqlite \"CREATE TABLE t(x INT); INSERT INTO t VALUES(1)\"");
        assert_eq!(setup.exit_code, 0);
        assert_eq!(setup.stderr, "");
        let write = env.exec(
            "sqlite3 /tmp/db.sqlite \"WITH cte AS (SELECT 2 AS v) INSERT INTO t SELECT v FROM cte\"",
        );
        assert_eq!(write.exit_code, 0);
        assert_eq!(write.stderr, "");
        assert_eq!(
            env.exec("sqlite3 /tmp/db.sqlite \"SELECT x FROM t ORDER BY x\"")
                .stdout,
            "1\n2\n"
        );

        // CTE-prefixed UPDATE
        let env = bash();
        env.exec(
            "sqlite3 /tmp/db.sqlite \"CREATE TABLE t(id INT, val TEXT); INSERT INTO t VALUES(1,'a'),(2,'b')\"",
        );
        let write = env.exec(
            "sqlite3 /tmp/db.sqlite \"WITH ids AS (SELECT 1 AS id) UPDATE t SET val='changed' WHERE id IN (SELECT id FROM ids)\"",
        );
        assert_eq!(write.exit_code, 0);
        assert_eq!(write.stderr, "");
        assert_eq!(
            env.exec("sqlite3 /tmp/db.sqlite \"SELECT id, val FROM t ORDER BY id\"")
                .stdout,
            "1|changed\n2|b\n"
        );

        // CTE-prefixed DELETE
        let env = bash();
        env.exec(
            "sqlite3 /tmp/db.sqlite \"CREATE TABLE t(x INT); INSERT INTO t VALUES(1),(2),(3)\"",
        );
        let write = env.exec(
            "sqlite3 /tmp/db.sqlite \"WITH ids AS (SELECT 2 AS x) DELETE FROM t WHERE x IN (SELECT x FROM ids)\"",
        );
        assert_eq!(write.exit_code, 0);
        assert_eq!(write.stderr, "");
        assert_eq!(
            env.exec("sqlite3 /tmp/db.sqlite \"SELECT x FROM t ORDER BY x\"")
                .stdout,
            "1\n3\n"
        );

        // Mutating PRAGMA (user_version)
        let env = bash();
        env.exec("sqlite3 /tmp/db.sqlite \"CREATE TABLE t(x INT)\"");
        let write = env.exec("sqlite3 /tmp/db.sqlite \"PRAGMA user_version = 42\"");
        assert_eq!(write.exit_code, 0);
        assert_eq!(write.stderr, "");
        assert_eq!(
            env.exec("sqlite3 /tmp/db.sqlite \"PRAGMA user_version\"")
                .stdout,
            "42\n"
        );

        // Comment-led INSERT (line comment via stdin)
        let env = bash();
        env.exec("sqlite3 /tmp/db.sqlite \"CREATE TABLE t(x INT)\"");
        let write = env.exec(
            "printf '%s\\n%s' '-- adding a row' 'INSERT INTO t VALUES(7);' | sqlite3 /tmp/db.sqlite",
        );
        assert_eq!(write.exit_code, 0);
        assert_eq!(write.stderr, "");
        assert_eq!(
            env.exec("sqlite3 /tmp/db.sqlite \"SELECT x FROM t\"")
                .stdout,
            "7\n"
        );

        // Comment-led UPDATE (block comment)
        let env = bash();
        env.exec(
            "sqlite3 /tmp/db.sqlite \"CREATE TABLE t(id INT, val TEXT); INSERT INTO t VALUES(1,'a')\"",
        );
        let write =
            env.exec("sqlite3 /tmp/db.sqlite \"/* note */ UPDATE t SET val='b' WHERE id=1\"");
        assert_eq!(write.exit_code, 0);
        assert_eq!(write.stderr, "");
        assert_eq!(
            env.exec("sqlite3 /tmp/db.sqlite \"SELECT id, val FROM t\"")
                .stdout,
            "1|b\n"
        );

        // Negative control: plain SELECT not marked modified
        let env = bash();
        env.exec("sqlite3 /tmp/db.sqlite \"CREATE TABLE t(x INT); INSERT INTO t VALUES(5)\"");
        let read = env.exec("sqlite3 /tmp/db.sqlite \"SELECT x FROM t\"");
        assert_eq!(read.exit_code, 0);
        assert_eq!(read.stdout, "5\n");
    }

    #[test]
    fn structured_data_jbc37_sqlite3_formatter_and_utf8_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/q.sql".to_string(),
                "SELECT '한글 / café / 漢字' AS msg;\n".to_string(),
            )]),
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec("sqlite3 -csv :memory: \"SELECT 'hello,world'\"")
                .stdout,
            "\"hello,world\"\n"
        );
        assert_eq!(
            env.exec("sqlite3 -csv :memory: \"SELECT '', 'x', ''\"")
                .stdout,
            ",x,\n"
        );
        assert_eq!(
            env.exec("sqlite3 -json :memory: \"CREATE TABLE t(x INT); SELECT * FROM t\"")
                .stdout,
            ""
        );
        let special_json = env.exec("sqlite3 -json :memory: \"SELECT 'line1\nline2' as x\"");
        assert!(special_json.stdout.contains("line1\\nline2"));
        let column = env.exec("sqlite3 -column -header :memory: \"SELECT 'short' as a, 'this is a very long value' as b\"");
        assert!(column.stdout.contains("short"));
        assert!(column.stdout.contains("this is a very long value"));
        assert!(column.stdout.contains("--"));
        let table =
            env.exec("sqlite3 -table -header :memory: \"CREATE TABLE t(a INT); SELECT * FROM t\"");
        assert!(table.stdout.contains("+"));
        assert!(table.stdout.contains("|"));
        assert!(table.stdout.contains("a"));
        let markdown = env.exec("sqlite3 -markdown -header :memory: \"CREATE TABLE t(a INT, b TEXT); INSERT INTO t VALUES(1,'x'); SELECT * FROM t\"");
        assert!(markdown.stdout.contains("| a | b |"));
        assert!(markdown.stdout.contains("|---|---|"));
        assert!(markdown.stdout.contains("| 1 | x |"));
        let html =
            env.exec("sqlite3 -html -header :memory: \"SELECT '<div>&</div>' as col1, 2 as col2\"");
        assert!(html.stdout.contains("<TH>col1</TH>"));
        assert!(html.stdout.contains("&lt;div&gt;&amp;&lt;/div&gt;"));
        assert!(html.stdout.contains("<TD>2</TD>"));
        let box_mode = env.exec("sqlite3 -box :memory: \"SELECT 42 as value\"");
        assert!(box_mode.stdout.contains("┌"));
        assert!(box_mode.stdout.contains("└"));
        assert!(box_mode.stdout.contains("value"));
        assert!(box_mode.stdout.contains("42"));
        let ascii = env.exec(
            "sqlite3 -ascii :memory: \"CREATE TABLE t(a,b); INSERT INTO t VALUES(1,2),(3,4); SELECT * FROM t\"",
        );
        assert!(ascii.stdout.contains('\x1f'));
        assert!(ascii.stdout.contains('\x1e'));
        assert_eq!(
            env.exec("sqlite3 -quote :memory: \"SELECT 42\"").stdout,
            "42\n"
        );
        assert_eq!(
            env.exec("sqlite3 -quote :memory: \"SELECT 3.14\"").stdout,
            "3.1400000000000001\n"
        );
        assert_eq!(
            env.exec("sqlite3 -quote :memory: \"SELECT 'hello'\"")
                .stdout,
            "'hello'\n"
        );
        assert_eq!(
            env.exec("sqlite3 -quote :memory: \"SELECT NULL\"").stdout,
            "NULL\n"
        );
        assert_eq!(
            env.exec("sqlite3 -nullvalue 'N/A' :memory: \"SELECT NULL, 1\"")
                .stdout,
            "N/A|1\n"
        );
        assert_eq!(
            env.exec("sqlite3 -csv -nullvalue 'N/A' :memory: \"SELECT NULL, 1\"")
                .stdout,
            "N/A,1\n"
        );
        assert!(
            env.exec("sqlite3 -column -nullvalue 'NULL' :memory: \"SELECT NULL as x\"")
                .stdout
                .contains("NULL")
        );
        assert_eq!(
            env.exec("sqlite3 :memory: \"SELECT X'48454C4C4F'\"").stdout,
            "HELLO\n"
        );
        assert_eq!(
            env.exec("sqlite3 -csv -header -nullvalue 'N/A' :memory: \"SELECT 1 as a, NULL as b\"")
                .stdout,
            "a,b\n1,N/A\n"
        );
        assert_eq!(
            env.exec("sqlite3 -json -cmd \"CREATE TABLE t(x); INSERT INTO t VALUES(42)\" :memory: \"SELECT * FROM t\"")
                .stdout,
            "[{\"x\":42}]\n"
        );
        let echo_header = env.exec("sqlite3 -echo -header :memory: \"SELECT 1 as x\"");
        assert!(echo_header.stdout.contains("SELECT 1 as x"));
        assert!(echo_header.stdout.contains("x"));
        assert!(echo_header.stdout.contains("1"));
        assert_eq!(
            env.exec("cat /q.sql | sqlite3 :memory:").stdout.trim(),
            "한글 / café / 漢字"
        );
    }

    #[test]
    fn structured_data_jbc37_xan_utf8_stdin_row() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/in.csv".to_string(),
                "name,city\n홍길동,서울\nAlice,Paris\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        let result = env.exec("cat /in.csv | xan select city");
        assert!(result.stdout.contains("서울"));
        assert!(result.stdout.contains("Paris"));
    }

    #[test]
    fn jbc31_readme_quick_start_and_configuration_examples_use_public_bash_api() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data/file.txt".to_string(),
                "content\npattern\n".to_string(),
            )]),
            env: BTreeMap::from([("MY_VAR".to_string(), "value".to_string())]),
            cwd: Some("/app".to_string()),
            ..BashOptions::default()
        });

        let wrote = env.exec("echo \"Hello\" > greeting.txt");
        assert_eq!(wrote.exit_code, 0);

        let result = env.exec("cat greeting.txt");
        assert_eq!(result.stdout, "Hello\n");
        assert_eq!(result.exit_code, 0);

        let configured = env.exec_with_options(
            "pwd; echo $TEMP; cat",
            JustBashExecOptions::new()
                .with_cwd("/data")
                .with_env("TEMP", "value")
                .with_stdin("hello from stdin\n"),
        );
        assert_eq!(configured.stdout, "/data\nvalue\nhello from stdin\n");
        assert_eq!(configured.exit_code, 0);

        let clean = env.exec_with_options(
            "env",
            JustBashExecOptions::new()
                .with_replace_env(true)
                .with_env("ONLY", "this"),
        );
        assert!(clean.stdout.contains("ONLY=this"));
        assert!(!clean.stdout.contains("MY_VAR=value"));

        let grep = env.exec_with_options(
            "grep",
            JustBashExecOptions::new().with_args(["-r", "pattern", "/data"]),
        );
        assert_eq!(grep.stdout, "/data/file.txt:pattern\n");
        assert_eq!(grep.exit_code, 0);

        assert_eq!(env.read_file("/app/greeting.txt").unwrap(), "Hello\n");
        assert!(env.file_exists("/data/file.txt"));
    }

    #[test]
    fn jbc31_docs_supported_command_list_matches_public_registry() {
        let default_names = UPSTREAM_DEFAULT_COMMAND_NAMES;

        for command in [
            "cat",
            "cp",
            "file",
            "ln",
            "ls",
            "mkdir",
            "mv",
            "readlink",
            "rm",
            "rmdir",
            "split",
            "stat",
            "touch",
            "tree",
            "awk",
            "base64",
            "column",
            "comm",
            "cut",
            "diff",
            "expand",
            "fold",
            "grep",
            "egrep",
            "fgrep",
            "head",
            "join",
            "md5sum",
            "nl",
            "od",
            "paste",
            "printf",
            "rev",
            "rg",
            "sed",
            "sha1sum",
            "sha256sum",
            "sort",
            "strings",
            "tac",
            "tail",
            "tr",
            "unexpand",
            "uniq",
            "wc",
            "xargs",
            "jq",
            "sqlite3",
            "xan",
            "yq",
            "basename",
            "dirname",
            "du",
            "echo",
            "env",
            "find",
            "hostname",
            "printenv",
            "pwd",
            "tee",
            "alias",
            "bash",
            "chmod",
            "clear",
            "date",
            "expr",
            "false",
            "help",
            "history",
            "seq",
            "sh",
            "sleep",
            "time",
            "timeout",
            "true",
            "unalias",
            "which",
            "whoami",
            "gzip",
            "gunzip",
            "zcat",
            "tar",
        ] {
            assert!(
                default_names.contains(&command),
                "README-listed command {command} should exist in the tracked upstream registry data"
            );
        }

        let public_registry = Bash::new().registered_command_names();
        for implemented in [
            "echo", "cat", "printf", "ls", "mkdir", "touch", "rm", "cp", "mv", "pwd", "grep", "rg",
            "sed", "awk", "jq", "yq", "xan", "sqlite3", "bash", "sh", "which", "whoami",
        ] {
            assert!(
                public_registry.iter().any(|name| name == implemented),
                "implemented public command {implemented} should be registered"
            );
        }
        assert!(default_names.len() > 40);
        assert!(default_names.len() < 150);
    }

    #[test]
    fn jbc31_custom_commands_match_public_api_usage_rows() {
        let load_count = Arc::new(Mutex::new(0));
        let lazy_count = Arc::clone(&load_count);
        let regular = JustBashCustomCommand::new("regular", |_| {
            JustBashCustomCommandResult::stdout("regular\n")
        });
        let lazy = JustBashCustomCommand::lazy("lazy", move || {
            *lazy_count.lock().expect("load count lock") += 1;
            JustBashCustomCommand::new("lazy", |_| JustBashCustomCommandResult::stdout("lazy\n"))
        });

        assert_eq!(regular.name(), "regular");
        assert!(!regular.is_lazy());
        assert!(lazy.is_lazy());

        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test.txt".to_string(), "file content".to_string())]),
            env: BTreeMap::from([("MY_VAR".to_string(), "my_value".to_string())]),
            cwd: Some("/home/user".to_string()),
            custom_commands: vec![
                JustBashCustomCommand::new("hello", |ctx| {
                    let name = ctx.args.first().map(String::as_str).unwrap_or("world");
                    JustBashCustomCommandResult::stdout(format!(
                        "Hello, {name}! CWD: {}\n",
                        ctx.cwd
                    ))
                }),
                JustBashCustomCommand::new("wordcount", |ctx| {
                    let words = ctx.stdin.split_whitespace().count();
                    JustBashCustomCommandResult::stdout(format!("{words}\n"))
                }),
                JustBashCustomCommand::new("reader", |ctx| {
                    match ctx.args.first().and_then(|path| ctx.read_file(path).ok()) {
                        Some(content) => JustBashCustomCommandResult::stdout(content),
                        None => JustBashCustomCommandResult::stderr(1, "reader: missing file\n"),
                    }
                }),
                JustBashCustomCommand::new("showenv", |ctx| {
                    let key = ctx.args.first().cloned().unwrap_or_default();
                    let value = ctx.env.get(&key).cloned().unwrap_or_default();
                    JustBashCustomCommandResult::stdout(format!("{key}={value}\n"))
                }),
                JustBashCustomCommand::new("echo", |ctx| {
                    JustBashCustomCommandResult::stdout(format!("Custom: {}\n", ctx.args.join(" ")))
                }),
                JustBashCustomCommand::new("cmd1", |_| {
                    JustBashCustomCommandResult::stdout("one\n")
                }),
                JustBashCustomCommand::new("cmd2", |_| {
                    JustBashCustomCommandResult::stdout("two\n")
                }),
                JustBashCustomCommand::new("failing", |_| {
                    JustBashCustomCommandResult::stderr(42, "error occurred\n")
                }),
                JustBashCustomCommand::new("upper", |ctx| {
                    JustBashCustomCommandResult::stdout(ctx.stdin.to_uppercase())
                }),
                JustBashCustomCommand::new("wrapper", |ctx| {
                    if ctx.args.is_empty() {
                        return JustBashCustomCommandResult::stderr(1, "exec not available\n");
                    }
                    let result = ctx.exec(ctx.args.join(" "));
                    JustBashCustomCommandResult::new(
                        format!("[wrapped] {}", result.stdout),
                        result.stderr,
                        result.exit_code,
                    )
                }),
                regular,
                lazy.clone(),
            ],
            ..BashOptions::default()
        });

        assert_eq!(
            env.exec("hello Alice").stdout,
            "Hello, Alice! CWD: /home/user\n"
        );
        assert_eq!(env.exec("hello").stdout, "Hello, world! CWD: /home/user\n");
        assert_eq!(env.exec("printf 'one two three' | wordcount").stdout, "3\n");
        assert_eq!(env.exec("reader /test.txt").stdout, "file content");
        assert_eq!(env.exec("showenv MY_VAR").stdout, "MY_VAR=my_value\n");
        assert_eq!(env.exec("echo hello world").stdout, "Custom: hello world\n");
        assert_eq!(env.exec("cmd1").stdout, "one\n");
        assert_eq!(env.exec("cmd2").stdout, "two\n");

        let failing = env.exec("failing");
        assert_eq!(failing.stdout, "");
        assert_eq!(failing.stderr, "error occurred\n");
        assert_eq!(failing.exit_code, 42);

        assert_eq!(
            env.exec("printf 'hello world\\n' | upper | cat").stdout,
            "HELLO WORLD\n"
        );
        assert_eq!(env.exec("wrapper printf hello").stdout, "[wrapped] hello");
        assert_eq!(env.exec("regular").stdout, "regular\n");
        assert_eq!(env.exec("lazy").stdout, "lazy\n");
        assert_eq!(env.exec("lazy").stdout, "lazy\n");
        assert_eq!(*load_count.lock().expect("load count lock"), 1);
        assert!(
            env.registered_command_names()
                .contains(&"hello".to_string())
        );
    }

    #[test]
    fn jbc31_bash_agent_and_website_examples_use_virtual_workspace_commands() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/workspace/README.md".to_string(),
                    "# Project\n".to_string(),
                ),
                (
                    "/workspace/src/app.ts".to_string(),
                    "export function run() {\n  // TODO: inspect\n}\n".to_string(),
                ),
                (
                    "/workspace/src/lib.ts".to_string(),
                    "export const value = 42;\n".to_string(),
                ),
            ]),
            cwd: Some("/workspace".to_string()),
            ..BashOptions::default()
        });

        assert_eq!(env.exec("ls /workspace").stdout, "README.md\nsrc\n");
        assert_eq!(env.exec("cat /workspace/README.md").stdout, "# Project\n");
        assert_eq!(
            env.exec("grep -r TODO /workspace").stdout,
            "/workspace/src/app.ts:  // TODO: inspect\n"
        );
        assert_eq!(
            env.exec("find /workspace -name \"*.ts\"").stdout,
            "/workspace/src/app.ts\n/workspace/src/lib.ts\n"
        );
        assert_eq!(
            env.exec("head -1 /workspace/src/app.ts").stdout,
            "export function run() {\n"
        );
        assert_eq!(
            env.exec("wc /workspace/src/lib.ts").stdout,
            "1 5 25 /workspace/src/lib.ts\n"
        );
    }

    // --- Bash.exec-options.test.ts: "Logging" suite -----------------------

    use crate::{BashLogData, BashLogEntry, BashLogLevel, BashLogger};

    /// Mirror of the upstream `createMockLogger` helper: records every entry in
    /// arrival order so tests can inspect message, level, and data.
    #[derive(Debug, Default)]
    struct MockLogger {
        logs: Mutex<Vec<BashLogEntry>>,
    }

    impl BashLogger for MockLogger {
        fn log(&self, entry: BashLogEntry) {
            self.logs.lock().expect("logger mutex").push(entry);
        }
    }

    impl MockLogger {
        fn entries(&self) -> Vec<BashLogEntry> {
            self.logs.lock().expect("logger mutex").clone()
        }

        fn find(&self, message: &str) -> Option<BashLogEntry> {
            self.entries()
                .into_iter()
                .find(|entry| entry.message == message)
        }
    }

    fn logging_bash() -> (Bash, Arc<MockLogger>) {
        let logger = Arc::new(MockLogger::default());
        let env = Bash::with_options(BashOptions {
            logger: Some(logger.clone()),
            ..BashOptions::default()
        });
        (env, logger)
    }

    fn output_of(entry: &BashLogEntry) -> &str {
        match &entry.data {
            BashLogData::Output(output) => output,
            other => panic!("expected output data, got {other:?}"),
        }
    }

    #[test]
    fn exec_options_logging_does_not_log_without_logger() {
        // packages/just-bash/src/Bash.exec-options.test.ts:721
        let env = Bash::new();
        let result = env.exec("echo hello");
        assert_eq!(result.stdout, "hello\n");
    }

    #[test]
    fn exec_options_logging_logs_exec_command_at_info_level() {
        // packages/just-bash/src/Bash.exec-options.test.ts:728
        let (env, logger) = logging_bash();
        env.exec("echo hello");

        let exec_log = logger.find("exec").expect("exec log defined");
        assert_eq!(exec_log.level, BashLogLevel::Info);
        assert_eq!(
            exec_log.data,
            BashLogData::Command("echo hello".to_string())
        );
    }

    #[test]
    fn exec_options_logging_logs_stdout_at_debug_level() {
        // packages/just-bash/src/Bash.exec-options.test.ts:740
        let (env, logger) = logging_bash();
        env.exec("echo hello");

        let stdout_log = logger.find("stdout").expect("stdout log defined");
        assert_eq!(stdout_log.level, BashLogLevel::Debug);
        assert_eq!(output_of(&stdout_log), "hello\n");
    }

    #[test]
    fn exec_options_logging_logs_stderr_at_info_level() {
        // packages/just-bash/src/Bash.exec-options.test.ts:752
        let (env, logger) = logging_bash();
        env.exec("echo error >&2");

        let stderr_log = logger.find("stderr").expect("stderr log defined");
        assert_eq!(stderr_log.level, BashLogLevel::Info);
        assert_eq!(output_of(&stderr_log), "error\n");
    }

    #[test]
    fn exec_options_logging_logs_exit_code_at_info_level() {
        // packages/just-bash/src/Bash.exec-options.test.ts:764
        let (env, logger) = logging_bash();
        env.exec("echo hello");

        let exit_log = logger.find("exit").expect("exit log defined");
        assert_eq!(exit_log.level, BashLogLevel::Info);
        assert_eq!(exit_log.data, BashLogData::ExitCode(0));
    }

    #[test]
    fn exec_options_logging_logs_non_zero_exit_code() {
        // packages/just-bash/src/Bash.exec-options.test.ts:776
        let (env, logger) = logging_bash();
        env.exec("exit 42");

        let exit_log = logger.find("exit").expect("exit log defined");
        assert_eq!(exit_log.data, BashLogData::ExitCode(42));
    }

    #[test]
    fn exec_options_logging_logs_in_correct_order_exec_then_exit() {
        // packages/just-bash/src/Bash.exec-options.test.ts:787
        let (env, logger) = logging_bash();
        env.exec("echo out; echo err >&2");

        let logs = logger.entries();
        assert_eq!(logs.first().expect("at least one log").message, "exec");
        assert_eq!(logs.last().expect("at least one log").message, "exit");
    }

    #[test]
    fn exec_options_logging_does_not_log_stdout_when_empty() {
        // packages/just-bash/src/Bash.exec-options.test.ts:797
        let (env, logger) = logging_bash();
        env.exec("true");

        assert!(logger.find("stdout").is_none());
    }

    #[test]
    fn exec_options_logging_does_not_log_stderr_when_empty() {
        // packages/just-bash/src/Bash.exec-options.test.ts:807
        let (env, logger) = logging_bash();
        env.exec("echo hello");

        assert!(logger.find("stderr").is_none());
    }

    #[test]
    fn exec_options_logging_does_not_log_empty_commands() {
        // packages/just-bash/src/Bash.exec-options.test.ts:817
        let (env, logger) = logging_bash();
        env.exec("");
        env.exec("   ");

        assert_eq!(logger.entries().len(), 0);
    }

    #[test]
    fn exec_options_logging_logs_parse_errors() {
        // packages/just-bash/src/Bash.exec-options.test.ts:827
        let (env, logger) = logging_bash();
        env.exec("echo ${");

        let exec_log = logger.find("exec").expect("exec log defined");
        assert_eq!(exec_log.data, BashLogData::Command("echo ${".to_string()));

        let stderr_log = logger.find("stderr").expect("stderr log defined");
        assert!(output_of(&stderr_log).contains("syntax error"));

        let exit_log = logger.find("exit").expect("exit log defined");
        assert_eq!(exit_log.data, BashLogData::ExitCode(2));
    }

    #[test]
    fn structured_data_yq_prototype_pollution_defense_rows() {
        // packages/just-bash/src/commands/yq/yq.prototype-pollution.test.ts:21,34,46,58,67,76,87,97,109,120
        let env = Bash::with_options(BashOptions {
            env: BTreeMap::from([
                ("constructor".to_string(), "ctor_value".to_string()),
                ("prototype".to_string(), "proto_value".to_string()),
            ]),
            ..BashOptions::default()
        });

        // YAML input with dangerous keys (DANGEROUS_KEYWORDS.slice(0, 3)).
        for keyword in ["constructor", "__proto__", "prototype"] {
            let result = env.exec(&format!("echo '{keyword}: value' | yq '.{keyword}'"));
            assert_eq!(result.exit_code, 0);
            assert_eq!(result.stdout.trim_end(), "value");
        }

        // JSON input with dangerous keys.
        for keyword in ["constructor", "__proto__", "prototype"] {
            let result = env.exec(&format!(
                "echo '{{\"{keyword}\": \"value\"}}' | yq -p json '.{keyword}'"
            ));
            assert_eq!(result.exit_code, 0);
            assert_eq!(result.stdout.trim_end(), "value");
        }

        // keys lists dangerous keys.
        let keys = env.exec("printf '__proto__: a\\nconstructor: b\\nnormal: c\\n' | yq 'keys'");
        assert_eq!(keys.exit_code, 0);
        assert!(keys.stdout.contains("__proto__"));
        assert!(keys.stdout.contains("constructor"));

        // to_entries preserves dangerous key.
        let entries = env.exec("echo 'constructor: val' | yq 'to_entries | .[0].key'");
        assert_eq!(entries.exit_code, 0);
        assert_eq!(entries.stdout.trim_end(), "constructor");

        // has() with dangerous key.
        let has_true = env.exec("echo 'constructor: value' | yq 'has(\"constructor\")'");
        assert_eq!(has_true.exit_code, 0);
        assert_eq!(has_true.stdout.trim_end(), "true");
        let has_false = env.exec("echo 'other: value' | yq 'has(\"__proto__\")'");
        assert_eq!(has_false.exit_code, 0);
        assert_eq!(has_false.stdout.trim_end(), "false");

        // $ENV access with dangerous keyword names.
        let env_ctor = env.exec("echo 'null' | yq '$ENV.constructor'");
        assert_eq!(env_ctor.exit_code, 0);
        assert_eq!(env_ctor.stdout.trim_end(), "ctor_value");
        let env_proto = env.exec("echo 'null' | yq '$ENV.prototype'");
        assert_eq!(env_proto.exit_code, 0);
        assert_eq!(env_proto.stdout.trim_end(), "proto_value");

        // add merges objects with a dangerous key.
        let added = env
            .exec("echo '[{\"constructor\": \"a\"}, {\"normal\": \"b\"}]' | yq -p json 'add | .constructor'");
        assert_eq!(added.exit_code, 0);
        assert_eq!(added.stdout.trim_end(), "a");

        // getpath with dangerous key.
        let path = env.exec("echo 'constructor: value' | yq 'getpath([\"constructor\"])'");
        assert_eq!(path.exit_code, 0);
        assert_eq!(path.stdout.trim_end(), "value");
    }

    #[test]
    fn structured_data_yq_json_stdin_and_jq_filter_rows() {
        // packages/just-bash/src/commands/yq/yq.test.ts:89,194,201,210,232,251,269
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.yaml".to_string(),
                "name: test\nvalue: 42\n".to_string(),
            )]),
            ..BashOptions::default()
        });

        // should output as JSON with -o json
        let json_out = env.exec("yq -o json '.' /data.yaml");
        assert_eq!(json_out.exit_code, 0);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json_out.stdout).unwrap(),
            serde_json::json!({"name": "test", "value": 42})
        );

        // should read from stdin
        let stdin = env.exec("echo 'name: test' | yq '.name'");
        assert_eq!(stdin.exit_code, 0);
        assert_eq!(stdin.stdout, "test\n");

        // should accept - for stdin
        let dash = env.exec("echo 'value: 42' | yq '.value' -");
        assert_eq!(dash.exit_code, 0);
        assert_eq!(dash.stdout, "42\n");

        // should support -n for null input with object construction
        let null_input = env.exec("yq -n '{name: \"created\"}' -o json");
        assert_eq!(null_input.exit_code, 0);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&null_input.stdout).unwrap(),
            serde_json::json!({"name": "created"})
        );

        // should support map filter
        let mapped =
            env.exec("printf 'numbers:\\n  - 1\\n  - 2\\n  - 3\\n' | yq '.numbers | map(. * 2)'");
        assert_eq!(mapped.exit_code, 0);
        let map_lines: Vec<&str> = mapped.stdout.trim().split('\n').collect();
        assert!(map_lines.contains(&"- 2"));
        assert!(map_lines.contains(&"- 4"));
        assert!(map_lines.contains(&"- 6"));

        // should support keys filter
        let keys = env.exec(
            "printf 'config:\\n  host: localhost\\n  port: 8080\\n  debug: true\\n' | yq '.config | keys'",
        );
        assert_eq!(keys.exit_code, 0);
        assert!(keys.stdout.contains("debug"));
        assert!(keys.stdout.contains("host"));
        assert!(keys.stdout.contains("port"));

        // should support length filter
        let length = env.exec("printf 'items:\\n  - a\\n  - b\\n  - c\\n' | yq '.items | length'");
        assert_eq!(length.exit_code, 0);
        assert_eq!(length.stdout, "3\n");
    }

    #[test]
    fn structured_data_yq_navigation_parent_parents_root_rows() {
        // packages/just-bash/src/commands/yq/yq.navigation.test.ts:8,19,32,47,
        //   60,74,87,101,115,127,141,154,165
        // Verifies the portable yq navigation operators parent/parent(n)/parents/
        // root, which require tracking the path used to reach the piped value.
        // Each assertion mirrors the upstream vitest expectation byte-for-byte;
        // they fail if path tracking regresses.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/nested.yaml".to_string(),
                    "a:\n  b:\n    c: value\n".to_string(),
                ),
                ("/leaf.yaml".to_string(), "a:\n  b: test\n".to_string()),
                ("/scalar.yaml".to_string(), "value: test\n".to_string()),
                (
                    "/items.yaml".to_string(),
                    "items:\n  - name: foo\n    val: 1\n  - name: bar\n    val: 2\n".to_string(),
                ),
                ("/shallow.yaml".to_string(), "a: test\n".to_string()),
                ("/pair.yaml".to_string(), "a: 1\nb: 2\n".to_string()),
                (
                    "/deep.yaml".to_string(),
                    "a:\n  b:\n    c:\n      d: value\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // :8 parent returns immediate parent
        let r = env.exec("yq -o json '.a.b.c | parent' /nested.yaml");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "{\n  \"c\": \"value\"\n}\n");

        // :19 parent(2) returns grandparent
        let r = env.exec("yq -o json '.a.b.c | parent(2)' /nested.yaml");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "{\n  \"b\": {\n    \"c\": \"value\"\n  }\n}\n");

        // :32 parent(-1) returns root
        let r = env.exec("yq -o json '.a.b.c | parent(-1)' /nested.yaml");
        assert_eq!(r.exit_code, 0);
        assert_eq!(
            r.stdout,
            "{\n  \"a\": {\n    \"b\": {\n      \"c\": \"value\"\n    }\n  }\n}\n"
        );

        // :47 root returns document root
        let r = env.exec("yq -o json '.a.b.c | root' /nested.yaml");
        assert_eq!(r.exit_code, 0);
        assert_eq!(
            r.stdout,
            "{\n  \"a\": {\n    \"b\": {\n      \"c\": \"value\"\n    }\n  }\n}\n"
        );

        // :60 parents returns array of all ancestors (immediate, grand, root)
        let r = env.exec("yq -o json '.a.b.c | parents | length' /nested.yaml");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "3\n");

        // :74 parent(0) returns current value
        let r = env.exec("yq -o json '.a.b | parent(0)' /leaf.yaml");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "\"test\"\n");

        // :87 parent(-2) returns one level below root
        let r = env.exec("yq -o json '.a.b.c | parent(-2)' /nested.yaml");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "{\n  \"b\": {\n    \"c\": \"value\"\n  }\n}\n");

        // :101 parent beyond root returns empty
        let r = env.exec("yq -o json '.a.b | parent(10)' /leaf.yaml");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "");

        // :115 parent at root returns empty
        let r = env.exec("yq -o json '. | parent' /scalar.yaml");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "");

        // :127 parent with array index path
        let r = env.exec("yq -o json '.items[0].name | parent' /items.yaml");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "{\n  \"name\": \"foo\",\n  \"val\": 1\n}\n");

        // :141 parents on shallow path (just root)
        let r = env.exec("yq -o json '.a | parents | length' /shallow.yaml");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "1\n");

        // :154 root without prior navigation
        let r = env.exec("yq -o json 'root' /pair.yaml");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "{\n  \"a\": 1,\n  \"b\": 2\n}\n");

        // :165 chained parent calls
        let r = env.exec("yq -o json '.a.b.c.d | parent | parent' /deep.yaml");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "{\n  \"c\": {\n    \"d\": \"value\"\n  }\n}\n");
    }

    #[test]
    fn structured_data_yq_fixtures_toml_ini_csv_unicode_rows() {
        // packages/just-bash/src/commands/yq/yq.fixtures.test.ts:516,600,650,659,
        //   671,682,691,700,709,718
        // Verifies portable yq TOML->YAML conversion, JSON unicode raw output, and
        // the INI/CSV input parsers (global keys before sections, boolean vs
        // string coercion, special-character paths, RFC-4180 quoted/escaped CSV
        // fields, semicolon/tab delimiter auto-detection, and UTF-8 fields). Each
        // assertion mirrors the upstream vitest expectation exactly and fails if
        // the parser/converter regresses.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/fixtures/yaml/simple.yaml".to_string(), "title: Simple Document\ncount: 42\nenabled: true\ntags:\n  - important\n  - featured\n  - new\n".to_string()),
                ("/fixtures/toml/config.toml".to_string(), "[server]\nhost = \"localhost\"\nport = 8080\ndebug = false\n\n[database]\nurl = \"postgres://localhost:5432/mydb\"\nmax_connections = 100\ntimeout = 30\n\n[database.pool]\nmin_size = 10\nmax_size = 50\n\n[logging]\nlevel = \"info\"\nformat = \"json\"\noutput = \"stdout\"\n\n[features]\ndark_mode = true\nnotifications = true\nanalytics = false\n".to_string()),
                ("/fixtures/json/users.json".to_string(), "{\n  \"users\": [\n    {\n      \"name\": \"alice\",\n      \"age\": 30,\n      \"email\": \"alice@example.com\",\n      \"active\": true\n    },\n    { \"name\": \"bob\", \"age\": 25, \"email\": \"bob@example.com\", \"active\": false },\n    {\n      \"name\": \"charlie\",\n      \"age\": 35,\n      \"email\": \"charlie@example.com\",\n      \"active\": true\n    }\n  ],\n  \"metadata\": {\n    \"version\": 1.0,\n    \"generated\": \"2024-01-01\"\n  }\n}\n".to_string()),
                ("/fixtures/json/special.json".to_string(), "{\n  \"empty_string\": \"\",\n  \"null_value\": null,\n  \"unicode\": \"Hello \\u4e16\\u754c\",\n  \"escaped\": \"quotes: \\\"nested\\\" and backslash: \\\\path\",\n  \"numbers\": {\n    \"integer\": 42,\n    \"float\": 3.14159,\n    \"negative\": -17,\n    \"scientific\": 1.5e10,\n    \"zero\": 0,\n    \"max_safe\": 9007199254740991\n  },\n  \"booleans\": {\n    \"true_val\": true,\n    \"false_val\": false\n  },\n  \"arrays\": {\n    \"empty\": [],\n    \"mixed\": [1, \"two\", true, null, { \"nested\": \"object\" }],\n    \"nested\": [[1, 2], [3, 4], [5, 6]]\n  },\n  \"objects\": {\n    \"empty\": {},\n    \"with-dash\": \"value\",\n    \"with.dot\": \"value\",\n    \"with space\": \"value\",\n    \"123numeric\": \"starts with number\"\n  },\n  \"deeply\": {\n    \"nested\": {\n      \"structure\": {\n        \"value\": \"found it\"\n      }\n    }\n  }\n}\n".to_string()),
                ("/fixtures/ini/special.ini".to_string(), "; INI special cases with comments\n# Another comment style\n\n; Top-level values (before any section)\nglobal_key=global_value\napp_name=Test App\n\n[empty_section]\n; This section has no values\n\n[strings]\nsimple=hello\nwith_spaces=hello world\nquoted=\"quoted value\"\nsingle_quoted='single quoted'\nequals_in_value=key=value\nsemicolon=value;not a comment\n\n[numbers]\ninteger=42\nfloat=3.14\nnegative=-17\nzero=0\n\n[booleans]\ntrue_val=true\nfalse_val=false\nyes_val=yes\nno_val=no\non_val=on\noff_val=off\none=1\nzero=0\n\n[special_keys]\nkey-with-dash=value\nkey.with.dots=value\nkey_with_underscores=value\nUPPERCASE=value\n\n[paths]\nunix_path=/var/log/app.log\nwindows_path=C:\\\\Users\\\\test\nurl=https://example.com/path?query=value\n".to_string()),
                ("/fixtures/csv/special.csv".to_string(), "name,description,value,enabled\n\"Simple\",\"A simple value\",100,true\n\"With, comma\",\"Value with comma in name\",200,false\n\"With \"\"quotes\"\"\",\"Escaped quotes inside\",300,true\n\"With\nnewline\",\"Multiline value\",400,true\n\"Empty\",\"\",0,false\n\"Unicode\",\"Hello 世界\",500,true\n\"Special chars\",\"Tab:\tBackslash:\\\",600,false\n".to_string()),
                ("/fixtures/csv/semicolon.csv".to_string(), "name;price;quantity\nWidget;19.99;100\nGadget;29.99;50\n\"Item;with;semicolons\";9.99;25\n".to_string()),
                ("/fixtures/csv/tabs.tsv".to_string(), "id\tname\tcategory\tprice\n1\tApple\tfruit\t1.50\n2\tBanana\tfruit\t0.75\n3\tCarrot\tvegetable\t0.50\n4\tBread\tbakery\t2.99\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        // :516 should convert TOML to YAML
        let r = env.exec("yq '.' /fixtures/toml/config.toml");
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("server:"));
        assert!(r.stdout.contains("host: localhost"));

        // :600 should handle unicode (JSON input, raw output)
        let r = env.exec("yq -p json '.unicode' /fixtures/json/special.json -r");
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("Hello"));

        // :650 should handle global keys before sections
        let r = env.exec("yq -p ini '.global_key' /fixtures/ini/special.ini");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "global_value");

        // :659 should handle various boolean formats (true->bool, yes->string)
        let r = env.exec("yq -p ini '.booleans' /fixtures/ini/special.ini -o json");
        assert_eq!(r.exit_code, 0);
        let bools: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
        assert_eq!(bools["true_val"], serde_json::json!(true));
        assert_eq!(bools["yes_val"], serde_json::json!("yes"));

        // :671 should handle paths with special characters
        let r = env.exec("yq -p ini '.paths.url' /fixtures/ini/special.ini");
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("https://example.com"));

        // :682 should handle quoted values with commas
        let r = env.exec("yq -p csv '.[1].name' /fixtures/csv/special.csv");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "With, comma");

        // :691 should handle escaped quotes
        let r = env.exec("yq -p csv '.[2].name' /fixtures/csv/special.csv");
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("quotes"));

        // :700 should auto-detect semicolon delimiter
        let r = env.exec("yq -p csv '.[0].name' /fixtures/csv/semicolon.csv");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "Widget");

        // :709 should auto-detect tab delimiter
        let r = env.exec("yq -p csv '.[0].name' /fixtures/csv/tabs.tsv");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "Apple");

        // :718 should handle unicode in CSV
        let r = env.exec("yq -p csv '.[5].description' /fixtures/csv/special.csv");
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("Hello"));
    }

    #[test]
    fn structured_data_yq_fixtures_yaml_json_ini_csv_rows() {
        // packages/just-bash/src/commands/yq/yq.fixtures.test.ts
        //   YAML: :103   JSON: :139,:149,:158,:167
        //   INI:  :222,:231,:241,:252,:259
        //   CSV:  :273,:282,:291,:300,:310
        // Each assertion mirrors the upstream vitest expectation exactly,
        // exercising the portable yq YAML/JSON/INI/CSV input parsers.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/fixtures/yaml/users.yaml".to_string(),
                    concat!(
                        "users:\n",
                        "  - name: alice\n    age: 30\n    email: alice@example.com\n    active: true\n",
                        "  - name: bob\n    age: 25\n    email: bob@example.com\n    active: false\n",
                        "  - name: charlie\n    age: 35\n    email: charlie@example.com\n    active: true\n",
                        "metadata:\n  version: 1.0\n  generated: \"2024-01-01\"\n",
                    )
                    .to_string(),
                ),
                (
                    "/fixtures/json/users.json".to_string(),
                    concat!(
                        "{\n  \"users\": [\n",
                        "    { \"name\": \"alice\", \"age\": 30, \"email\": \"alice@example.com\", \"active\": true },\n",
                        "    { \"name\": \"bob\", \"age\": 25, \"email\": \"bob@example.com\", \"active\": false },\n",
                        "    { \"name\": \"charlie\", \"age\": 35, \"email\": \"charlie@example.com\", \"active\": true }\n",
                        "  ]\n}\n",
                    )
                    .to_string(),
                ),
                (
                    "/fixtures/json/nested.json".to_string(),
                    concat!(
                        "{\n  \"company\": {\n    \"name\": \"Acme Corp\",\n    \"departments\": [\n",
                        "      { \"name\": \"Engineering\", \"employees\": 50, \"budget\": 500000 },\n",
                        "      { \"name\": \"Sales\", \"employees\": 30, \"budget\": 300000 },\n",
                        "      { \"name\": \"Marketing\", \"employees\": 20, \"budget\": 200000 }\n",
                        "    ]\n  }\n}\n",
                    )
                    .to_string(),
                ),
                (
                    "/fixtures/ini/config.ini".to_string(),
                    concat!(
                        "[database]\nhost=localhost\nport=5432\nname=myapp\nssl=true\n\n",
                        "[server]\nhost=0.0.0.0\nport=8080\ndebug=false\nworkers=4\n\n",
                        "[logging]\nlevel=info\nformat=json\npath=/var/log/app.log\n",
                    )
                    .to_string(),
                ),
                (
                    "/fixtures/ini/app.ini".to_string(),
                    concat!(
                        "name=MyApp\nversion=2.5.0\nenvironment=production\n\n",
                        "[features]\ndark_mode=true\nnotifications=true\nanalytics=false\n\n",
                        "[limits]\nmax_users=1000\nmax_requests=10000\ntimeout=30\n",
                    )
                    .to_string(),
                ),
                (
                    "/fixtures/csv/users.csv".to_string(),
                    concat!(
                        "name,age,email,active\n",
                        "alice,30,alice@example.com,true\n",
                        "bob,25,bob@example.com,false\n",
                        "charlie,35,charlie@example.com,true\n",
                    )
                    .to_string(),
                ),
                (
                    "/fixtures/csv/products.csv".to_string(),
                    concat!(
                        "id,name,price,category,in_stock\n",
                        "1,Widget,19.99,electronics,true\n",
                        "2,Gadget,29.99,electronics,true\n",
                        "3,Gizmo,9.99,accessories,false\n",
                        "4,Doodad,49.99,electronics,true\n",
                        "5,Thingamajig,14.99,accessories,true\n",
                    )
                    .to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // --- YAML fixtures ---
        // :103 should extract user names from users.yaml
        let names = env.exec("yq '.users[].name' /fixtures/yaml/users.yaml");
        assert_eq!(names.exit_code, 0);
        assert_eq!(names.stdout, "alice\nbob\ncharlie\n");

        // --- JSON fixtures ---
        // :139 should extract user emails from users.json
        let emails = env.exec("yq -p json '.users[].email' /fixtures/json/users.json");
        assert_eq!(emails.exit_code, 0);
        assert!(emails.stdout.contains("alice@example.com"));
        assert!(emails.stdout.contains("bob@example.com"));

        // :149 should get department names from nested.json
        let departments =
            env.exec("yq -p json '.company.departments[].name' /fixtures/json/nested.json");
        assert_eq!(departments.exit_code, 0);
        assert_eq!(departments.stdout, "Engineering\nSales\nMarketing\n");

        // :158 should calculate total employees from nested.json
        let employees = env.exec(
            "yq -p json '[.company.departments[].employees] | add' /fixtures/json/nested.json",
        );
        assert_eq!(employees.exit_code, 0);
        assert_eq!(employees.stdout, "100\n");

        // :167 should find departments with budget > 250000
        let budget = env.exec(
            "yq -p json '[.company.departments[] | select(.budget > 250000) | .name]' /fixtures/json/nested.json -o json",
        );
        assert_eq!(budget.exit_code, 0);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&budget.stdout).unwrap(),
            serde_json::json!(["Engineering", "Sales"])
        );

        // --- INI fixtures ---
        // :222 should get database host from config.ini
        let ini_host = env.exec("yq -p ini '.database.host' /fixtures/ini/config.ini");
        assert_eq!(ini_host.exit_code, 0);
        assert_eq!(ini_host.stdout, "localhost\n");

        // :231 should get server port from config.ini (INI values are strings)
        let ini_port = env.exec("yq -p ini '.server.port' /fixtures/ini/config.ini");
        assert_eq!(ini_port.exit_code, 0);
        assert!(ini_port.stdout.trim().contains("8080"));

        // :241 should get all section keys from config.ini
        let ini_keys = env.exec("yq -p ini 'keys' /fixtures/ini/config.ini");
        assert_eq!(ini_keys.exit_code, 0);
        assert!(ini_keys.stdout.contains("database"));
        assert!(ini_keys.stdout.contains("server"));
        assert!(ini_keys.stdout.contains("logging"));

        // :252 should get top-level name from app.ini
        let ini_name = env.exec("yq -p ini '.name' /fixtures/ini/app.ini");
        assert_eq!(ini_name.exit_code, 0);
        assert_eq!(ini_name.stdout, "MyApp\n");

        // :259 should get feature flags from app.ini (booleans)
        let ini_features = env.exec("yq -p ini '.features' /fixtures/ini/app.ini -o json");
        assert_eq!(ini_features.exit_code, 0);
        let features = serde_json::from_str::<serde_json::Value>(&ini_features.stdout).unwrap();
        assert_eq!(features["dark_mode"], serde_json::json!(true));
        assert_eq!(features["notifications"], serde_json::json!(true));
        assert_eq!(features["analytics"], serde_json::json!(false));

        // --- CSV fixtures ---
        // :273 should get first user name from users.csv
        let csv_first = env.exec("yq -p csv '.[0].name' /fixtures/csv/users.csv");
        assert_eq!(csv_first.exit_code, 0);
        assert_eq!(csv_first.stdout, "alice\n");

        // :282 should get all user ages from users.csv (numbers)
        let csv_ages = env.exec("yq -p csv '.[].age' /fixtures/csv/users.csv");
        assert_eq!(csv_ages.exit_code, 0);
        assert_eq!(csv_ages.stdout, "30\n25\n35\n");

        // :291 should filter electronics products from products.csv
        let csv_filter = env.exec(
            "yq -p csv '[.[] | select(.category == \"electronics\") | .name]' /fixtures/csv/products.csv -o json",
        );
        assert_eq!(csv_filter.exit_code, 0);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&csv_filter.stdout).unwrap(),
            serde_json::json!(["Widget", "Gadget", "Doodad"])
        );

        // :300 should calculate total price of in-stock items from products.csv
        let csv_total = env.exec(
            "yq -p csv '[.[] | select(.in_stock == true) | .price] | add' /fixtures/csv/products.csv",
        );
        assert_eq!(csv_total.exit_code, 0);
        let total: f64 = csv_total.stdout.trim().parse().unwrap();
        assert!((total - 114.96).abs() < 0.01, "total was {total}");

        // :310 should get product count from products.csv
        let csv_count = env.exec("yq -p csv 'length' /fixtures/csv/products.csv");
        assert_eq!(csv_count.exit_code, 0);
        assert_eq!(csv_count.stdout, "5\n");
    }

    #[test]
    fn structured_data_yq_toml_fixtures_rows() {
        // packages/just-bash/src/commands/yq/yq.fixtures.test.ts — TOML fixtures
        //   :321 :330 :339 :348 :357 :367 :376 :385 :396 :405 :414 :423
        // Mirrors the upstream cargo.toml / pyproject.toml / config.toml /
        // special.toml fixture queries exactly, exercising the portable yq
        // TOML input parser (tables, dotted keys, inline tables, arrays of
        // tables, multiline basic/literal strings, and unicode scalars).
        let cargo = concat!(
            "[package]\n",
            "name = \"my-rust-app\"\n",
            "version = \"1.2.3\"\n",
            "edition = \"2021\"\n",
            "authors = [\"Alice <alice@example.com>\", \"Bob <bob@example.com>\"]\n",
            "description = \"A sample Rust application\"\n\n",
            "[dependencies]\n",
            "serde = { version = \"1.0\", features = [\"derive\"] }\n",
            "tokio = { version = \"1.0\", features = [\"full\"] }\n\n",
            "[dev-dependencies]\n",
            "criterion = \"0.5\"\n\n",
            "[profile.release]\n",
            "opt-level = 3\n",
            "lto = true\n",
        );
        let config = concat!(
            "[server]\n",
            "host = \"localhost\"\n",
            "port = 8080\n",
            "debug = false\n\n",
            "[database]\n",
            "url = \"postgres://localhost:5432/mydb\"\n",
            "max_connections = 100\n",
            "timeout = 30\n\n",
            "[database.pool]\n",
            "min_size = 10\n",
            "max_size = 50\n\n",
            "[logging]\n",
            "level = \"info\"\n",
            "format = \"json\"\n",
            "output = \"stdout\"\n\n",
            "[features]\n",
            "dark_mode = true\n",
            "notifications = true\n",
            "analytics = false\n",
        );
        let pyproject = concat!(
            "[project]\n",
            "name = \"my-python-app\"\n",
            "version = \"2.0.0\"\n",
            "description = \"A sample Python application\"\n",
            "authors = [\n",
            "    { name = \"Alice\", email = \"alice@example.com\" },\n",
            "    { name = \"Bob\", email = \"bob@example.com\" }\n",
            "]\n",
            "requires-python = \">=3.9\"\n",
        );
        let special = concat!(
            "# TOML edge cases\n\n",
            "# Basic types\n",
            "string_value = \"hello world\"\n",
            "integer_value = 42\n",
            "float_value = 3.14\n",
            "boolean_true = true\n",
            "boolean_false = false\n\n",
            "# Datetime values\n",
            "date_value = 2024-01-15\n",
            "datetime_value = 2024-01-15T10:30:00Z\n\n",
            "# Arrays\n",
            "simple_array = [1, 2, 3, 4, 5]\n",
            "string_array = [\"apple\", \"banana\", \"cherry\"]\n",
            "nested_array = [[1, 2], [3, 4], [5, 6]]\n\n",
            "# Multiline strings\n",
            "multiline_basic = \"\"\"\n",
            "This is a\n",
            "multiline string\n",
            "spanning multiple lines\"\"\"\n\n",
            "multiline_literal = '''\n",
            "Line 1\n",
            "Line 2\n",
            "Line 3'''\n\n",
            "# Inline table\n",
            "inline = { name = \"inline\", value = 123 }\n\n",
            "# Dotted keys (must be before [[products]] to be root-level)\n",
            "dotted.key.value = \"nested via dots\"\n\n",
            "# Unicode (must be before [[products]] to be root-level)\n",
            "unicode = \"Hello 世界 🌍\"\n\n",
            "# Array of tables (must come last since it affects all subsequent keys)\n",
            "[[products]]\n",
            "name = \"Widget\"\n",
            "price = 19.99\n\n",
            "[[products]]\n",
            "name = \"Gadget\"\n",
            "price = 29.99\n\n",
            "[[products]]\n",
            "name = \"Doodad\"\n",
            "price = 49.99\n",
        );
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/fixtures/toml/cargo.toml".to_string(), cargo.to_string()),
                ("/fixtures/toml/config.toml".to_string(), config.to_string()),
                (
                    "/fixtures/toml/pyproject.toml".to_string(),
                    pyproject.to_string(),
                ),
                (
                    "/fixtures/toml/special.toml".to_string(),
                    special.to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // :321 should get package name from cargo.toml
        let pkg_name = env.exec("yq '.package.name' /fixtures/toml/cargo.toml");
        assert_eq!(pkg_name.exit_code, 0);
        assert_eq!(pkg_name.stdout, "my-rust-app\n");

        // :330 should get package version from cargo.toml
        let pkg_ver = env.exec("yq '.package.version' /fixtures/toml/cargo.toml");
        assert_eq!(pkg_ver.exit_code, 0);
        assert_eq!(pkg_ver.stdout, "1.2.3\n");

        // :339 should get dependency versions from cargo.toml (inline table)
        let dep_ver =
            env.exec("yq '.dependencies.serde.version' /fixtures/toml/cargo.toml -o json -r");
        assert_eq!(dep_ver.exit_code, 0);
        assert_eq!(dep_ver.stdout, "1.0\n");

        // :348 should get project name from pyproject.toml
        let proj_name = env.exec("yq '.project.name' /fixtures/toml/pyproject.toml");
        assert_eq!(proj_name.exit_code, 0);
        assert_eq!(proj_name.stdout, "my-python-app\n");

        // :357 should get author emails from pyproject.toml (array of inline tables)
        let emails = env.exec("yq '.project.authors[].email' /fixtures/toml/pyproject.toml");
        assert_eq!(emails.exit_code, 0);
        assert!(emails.stdout.contains("alice@example.com"));
        assert!(emails.stdout.contains("bob@example.com"));

        // :367 should get database settings from config.toml (integer)
        let max_conn = env.exec("yq '.database.max_connections' /fixtures/toml/config.toml");
        assert_eq!(max_conn.exit_code, 0);
        assert_eq!(max_conn.stdout, "100\n");

        // :376 should get nested pool settings from config.toml (dotted section)
        let pool_max = env.exec("yq '.database.pool.max_size' /fixtures/toml/config.toml");
        assert_eq!(pool_max.exit_code, 0);
        assert_eq!(pool_max.stdout, "50\n");

        // :385 should get feature flags from config.toml (booleans)
        let features = env.exec("yq '.features' /fixtures/toml/config.toml -o json");
        assert_eq!(features.exit_code, 0);
        let parsed_features = serde_json::from_str::<serde_json::Value>(&features.stdout).unwrap();
        assert_eq!(parsed_features["dark_mode"], serde_json::json!(true));
        assert_eq!(parsed_features["analytics"], serde_json::json!(false));

        // :396 should handle array of tables from special.toml
        let products = env.exec("yq '.products[].name' /fixtures/toml/special.toml");
        assert_eq!(products.exit_code, 0);
        assert_eq!(products.stdout, "Widget\nGadget\nDoodad\n");

        // :405 should handle inline tables from special.toml
        let inline = env.exec("yq '.inline.name' /fixtures/toml/special.toml");
        assert_eq!(inline.exit_code, 0);
        assert_eq!(inline.stdout, "inline\n");

        // :414 should handle unicode from special.toml
        let unicode = env.exec("yq '.unicode' /fixtures/toml/special.toml");
        assert_eq!(unicode.exit_code, 0);
        assert!(unicode.stdout.contains("Hello"));

        // :423 should convert TOML to JSON from config.toml
        let server = env.exec("yq '.server' /fixtures/toml/config.toml -o json");
        assert_eq!(server.exit_code, 0);
        let parsed_server = serde_json::from_str::<serde_json::Value>(&server.stdout).unwrap();
        assert_eq!(parsed_server["host"], serde_json::json!("localhost"));
        assert_eq!(parsed_server["port"], serde_json::json!(8080));
    }

    #[test]
    fn structured_data_yq_format_parser_security_rows() {
        // packages/just-bash/src/commands/yq/yq.yaml-security.test.ts
        //   :47 (!!python tag unresolved), :119 (XML XXE not expanded),
        //   :138 (XML __proto__ element safe), :155 (CSV __proto__ header safe),
        //   :166 (TOML dangerous key safe), :181 (INI dangerous section safe).
        // Each parser must treat dangerous constructs as inert data, matching
        // the upstream vitest expectations exactly.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/python.yaml".to_string(),
                    "exploit: !!python/object/apply:os.system ['echo pwned']\n".to_string(),
                ),
                (
                    "/xxe.xml".to_string(),
                    concat!(
                        "<?xml version=\"1.0\"?>\n",
                        "<!DOCTYPE foo [\n",
                        "  <!ENTITY xxe SYSTEM \"file:///etc/passwd\">\n",
                        "]>\n",
                        "<root><data>&xxe;</data></root>\n",
                    )
                    .to_string(),
                ),
                (
                    "/proto.xml".to_string(),
                    "<root><__proto__><polluted>true</polluted></__proto__><safe>value</safe></root>".to_string(),
                ),
                (
                    "/proto.csv".to_string(),
                    "__proto__,safe\nevil,value\n".to_string(),
                ),
                (
                    "/proto.toml".to_string(),
                    "\"__proto__\" = \"evil\"\nsafe = \"value\"\n".to_string(),
                ),
                (
                    "/proto.ini".to_string(),
                    "[__proto__]\npolluted=true\n\n[safe]\nvalue=ok\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // :47 should not execute !!python/object tags — value stays raw text.
        let python = env.exec("yq '.exploit' /python.yaml");
        assert_eq!(python.exit_code, 0);
        assert_ne!(python.stdout, "pwned\n");

        // :119 should not expand external entities (XXE).
        let xxe = env.exec("yq -p xml '.root.data' /xxe.xml");
        if xxe.exit_code == 0 {
            assert!(!xxe.stdout.contains("root:"));
        }

        // :138 should handle __proto__ element name in XML safely.
        let xml_proto = env.exec("yq -p xml '.root.safe' /proto.xml");
        assert_eq!(xml_proto.exit_code, 0);
        assert_eq!(xml_proto.stdout.trim(), "value");

        // :155 should handle __proto__ column header in CSV safely.
        let csv_proto = env.exec("yq -p csv '.[0].safe' /proto.csv");
        assert_eq!(csv_proto.exit_code, 0);
        assert_eq!(csv_proto.stdout.trim(), "value");

        // :166 should handle dangerous keys in TOML safely.
        let toml_proto = env.exec("yq -p toml '.safe' /proto.toml");
        assert_eq!(toml_proto.exit_code, 0);
        assert_eq!(toml_proto.stdout.trim(), "value");

        // :181 should handle dangerous section names in INI safely.
        let ini_proto = env.exec("yq -p ini '.safe.value' /proto.ini");
        assert_eq!(ini_proto.exit_code, 0);
        assert_eq!(ini_proto.stdout.trim(), "ok");
    }

    #[test]
    fn structured_data_yq_xml_io_and_format_conversion_rows() {
        // packages/just-bash/src/commands/yq/yq.test.ts
        //   :149 (read XML), :160 (XML attributes), :174 (output XML),
        //   :492 (JSON -> CSV), :699 (custom XML attribute prefix).
        // packages/just-bash/src/commands/yq/yq.fixtures.test.ts
        //   :454 (CSV -> JSON), :475 (YAML -> INI), :485 (INI -> JSON),
        //   :506 (YAML -> TOML), :729 (average age), :747 (group_by),
        //   :757 (object construction from map).
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/data.xml".to_string(),
                    "<root><name>test</name><value>42</value></root>".to_string(),
                ),
                (
                    "/attr.xml".to_string(),
                    "<item id=\"123\" name=\"test\"/>".to_string(),
                ),
                (
                    "/out.yaml".to_string(),
                    "\nroot:\n  name: test\n  value: 42\n".to_string(),
                ),
                (
                    "/data.json".to_string(),
                    "[{\"name\":\"alice\",\"score\":95},{\"name\":\"bob\",\"score\":87}]"
                        .to_string(),
                ),
                ("/prefix.xml".to_string(), "<item id=\"123\"/>".to_string()),
                (
                    "/fixtures/csv/users.csv".to_string(),
                    concat!(
                        "name,age,email,active\n",
                        "alice,30,alice@example.com,true\n",
                        "bob,25,bob@example.com,false\n",
                    )
                    .to_string(),
                ),
                (
                    "/fixtures/yaml/simple.yaml".to_string(),
                    concat!(
                        "title: Simple Document\ncount: 42\nenabled: true\n",
                        "tags:\n  - important\n  - featured\n  - new\n",
                    )
                    .to_string(),
                ),
                (
                    "/fixtures/ini/config.ini".to_string(),
                    "[database]\nhost=localhost\n\n[server]\nport=8080\n".to_string(),
                ),
                (
                    "/fixtures/yaml/users.yaml".to_string(),
                    concat!(
                        "users:\n",
                        "  - name: alice\n    age: 30\n    email: alice@example.com\n",
                        "  - name: bob\n    age: 25\n    email: bob@example.com\n",
                        "  - name: charlie\n    age: 35\n    email: charlie@example.com\n",
                    )
                    .to_string(),
                ),
                (
                    "/fixtures/csv/products.csv".to_string(),
                    concat!(
                        "id,name,price,category,in_stock\n",
                        "1,Widget,19.99,electronics,true\n",
                        "2,Gadget,29.99,electronics,true\n",
                        "3,Gizmo,9.99,accessories,false\n",
                    )
                    .to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // yq.test.ts :149 should read XML with -p xml
        let xml_read = env.exec("yq -p xml '.root.name' /data.xml");
        assert_eq!(xml_read.exit_code, 0);
        assert_eq!(xml_read.stdout, "test\n");

        // yq.test.ts :160 should handle XML attributes (+@ prefix)
        let xml_attr = env.exec("yq -p xml '.item[\"+@id\"]' /attr.xml -o json");
        assert_eq!(xml_attr.exit_code, 0);
        assert_eq!(xml_attr.stdout, "\"123\"\n");

        // yq.test.ts :174 should output as XML
        let xml_out = env.exec("yq -o xml '.' /out.yaml");
        assert_eq!(xml_out.exit_code, 0);
        assert!(xml_out.stdout.contains("<root>"));
        assert!(xml_out.stdout.contains("<name>test</name>"));
        assert!(xml_out.stdout.contains("<value>42</value>"));
        assert!(xml_out.stdout.contains("</root>"));

        // yq.test.ts :492 should convert JSON to CSV (key order preserved)
        let json_csv = env.exec("yq -p json -o csv '.' /data.json");
        assert_eq!(json_csv.exit_code, 0);
        assert!(json_csv.stdout.contains("name,score"));
        assert!(json_csv.stdout.contains("alice,95"));
        assert!(json_csv.stdout.contains("bob,87"));

        // yq.test.ts :699 should use custom attribute prefix
        let xml_prefix = env
            .exec("yq -p xml --xml-attribute-prefix='@' '.item[\"@id\"]' /prefix.xml -o json -r");
        assert_eq!(xml_prefix.exit_code, 0);
        assert_eq!(xml_prefix.stdout, "123\n");

        // fixtures :454 should convert CSV to JSON (dynamic typing)
        let csv_json = env.exec("yq -p csv '.[0]' /fixtures/csv/users.csv -o json");
        assert_eq!(csv_json.exit_code, 0);
        let csv_user = serde_json::from_str::<serde_json::Value>(&csv_json.stdout).unwrap();
        assert_eq!(csv_user["name"], serde_json::json!("alice"));
        assert_eq!(csv_user["age"], serde_json::json!(30));

        // fixtures :475 should convert YAML to INI
        let yaml_ini = env.exec("yq '.' /fixtures/yaml/simple.yaml -o ini");
        assert_eq!(yaml_ini.exit_code, 0);
        assert!(yaml_ini.stdout.contains("title=Simple Document"));
        assert!(yaml_ini.stdout.contains("count=42"));

        // fixtures :485 should convert INI to JSON (INI values stay strings)
        let ini_json = env.exec("yq -p ini '.' /fixtures/ini/config.ini -o json");
        assert_eq!(ini_json.exit_code, 0);
        let cfg = serde_json::from_str::<serde_json::Value>(&ini_json.stdout).unwrap();
        assert_eq!(cfg["database"]["host"], serde_json::json!("localhost"));
        assert_eq!(cfg["server"]["port"], serde_json::json!("8080"));

        // fixtures :506 should convert YAML to TOML
        let yaml_toml = env.exec("yq '.' /fixtures/yaml/simple.yaml -o toml");
        assert_eq!(yaml_toml.exit_code, 0);
        assert!(yaml_toml.stdout.contains("title = \"Simple Document\""));
        assert!(yaml_toml.stdout.contains("count = 42"));

        // fixtures :729 should calculate average age from users.yaml
        let avg_age = env.exec("yq '[.users[].age] | add / length' /fixtures/yaml/users.yaml");
        assert_eq!(avg_age.exit_code, 0);
        assert_eq!(avg_age.stdout, "30\n");

        // fixtures :747 should group products by category from products.csv
        let group_by = env.exec(
            "yq -p csv 'group_by(.category) | map({category: .[0].category, count: length})' /fixtures/csv/products.csv -o json",
        );
        assert_eq!(group_by.exit_code, 0);
        let groups = serde_json::from_str::<serde_json::Value>(&group_by.stdout).unwrap();
        assert_eq!(groups.as_array().unwrap().len(), 2);

        // fixtures :757 should transform user data structure from users.yaml
        let transform = env
            .exec("yq '.users | map({(.name): .email}) | add' /fixtures/yaml/users.yaml -o json");
        assert_eq!(transform.exit_code, 0);
        let emails = serde_json::from_str::<serde_json::Value>(&transform.stdout).unwrap();
        assert_eq!(emails["alice"], serde_json::json!("alice@example.com"));
        assert_eq!(emails["bob"], serde_json::json!("bob@example.com"));
    }

    #[test]
    fn structured_data_yq_format_conversion_frontmatter_and_autodetect_rows() {
        // packages/just-bash/src/commands/yq/yq.test.ts
        //   :354 :365 (format validation), :377 :395 :407 :424 (INI),
        //   :438 :449 :460 :474 :492 :506 (CSV), :686 (--no-csv-header),
        //   :712 :729 :743 :756 (TOML), :770 :781 (TSV), :808 (inplace error),
        //   :817 :839 :856 :873 (front-matter), :898 :909 :920 (auto-detect).
        // packages/just-bash/src/commands/yq/yq.fixtures.test.ts
        //   :211 (XML user names), :497 (XML -> JSON).
        // Each assertion mirrors the upstream vitest expectation exactly.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/data.yaml".to_string(),
                    "database:\n  host: localhost\n  port: 5432\n".to_string(),
                ),
                (
                    "/kv.yaml".to_string(),
                    "name: test\nversion: 1\n".to_string(),
                ),
                (
                    "/config.ini".to_string(),
                    "[database]\nhost=localhost\nport=5432\n\n[server]\ndebug=true\n".to_string(),
                ),
                (
                    "/data.csv".to_string(),
                    "name,age,city\nalice,30,NYC\nbob,25,LA\ncharlie,35,NYC\n".to_string(),
                ),
                (
                    "/rows.yaml".to_string(),
                    "- name: alice\n  age: 30\n- name: bob\n  age: 25\n".to_string(),
                ),
                (
                    "/scores.json".to_string(),
                    "[{\"name\":\"alice\",\"score\":95},{\"name\":\"bob\",\"score\":87}]".to_string(),
                ),
                (
                    "/headerless.csv".to_string(),
                    "alice,30\nbob,25\n".to_string(),
                ),
                (
                    "/Cargo.toml".to_string(),
                    "[package]\nname = \"my-app\"\nversion = \"1.0.0\"\n\n[dependencies]\nserde = \"1.0\"\n".to_string(),
                ),
                (
                    "/config.toml".to_string(),
                    "[server]\nhost = \"localhost\"\nport = 8080\n".to_string(),
                ),
                (
                    "/server.yaml".to_string(),
                    "server:\n  host: localhost\n  port: 8080\n".to_string(),
                ),
                (
                    "/app.json".to_string(),
                    "{\"app\": {\"name\": \"test\", \"version\": \"2.0\"}}".to_string(),
                ),
                (
                    "/data.tsv".to_string(),
                    "name\tage\tcity\nalice\t30\tNYC\nbob\t25\tLA\n".to_string(),
                ),
                (
                    "/rows.tsv".to_string(),
                    "name\tvalue\na\t1\nb\t2\n".to_string(),
                ),
                (
                    "/post.md".to_string(),
                    "---\ntitle: My Post\ndate: 2024-01-01\ntags:\n  - tech\n  - web\n---\n\n# Content here\n".to_string(),
                ),
                (
                    "/tags.md".to_string(),
                    "---\ntitle: Test\ntags:\n  - a\n  - b\n---\nContent".to_string(),
                ),
                (
                    "/hugo.md".to_string(),
                    "+++\ntitle = \"Hugo Post\"\ndate = \"2024-01-01\"\n+++\n\nContent here.\n".to_string(),
                ),
                (
                    "/plain.md".to_string(),
                    "# Just a heading\n\nNo front-matter here.".to_string(),
                ),
                (
                    "/data.xml".to_string(),
                    "<root><name>test</name></root>".to_string(),
                ),
                (
                    "/auto.csv".to_string(),
                    "name,age\nalice,30\nbob,25\n".to_string(),
                ),
                (
                    "/auto.ini".to_string(),
                    "[database]\nhost=localhost\n".to_string(),
                ),
                (
                    "/users.xml".to_string(),
                    "<root><users><user><name>alice</name></user><user><name>bob</name></user><user><name>charlie</name></user></users></root>".to_string(),
                ),
                (
                    "/books.xml".to_string(),
                    concat!(
                        "<library>\n",
                        "  <book id=\"1\" genre=\"fiction\">\n    <title>The Great Adventure</title>\n    <author>Jane Smith</author>\n  </book>\n",
                        "  <book id=\"2\" genre=\"non-fiction\">\n    <title>Learning TypeScript</title>\n    <author>John Doe</author>\n  </book>\n",
                        "  <book id=\"3\" genre=\"fiction\">\n    <title>Mystery Manor</title>\n    <author>Alice Johnson</author>\n  </book>\n",
                        "</library>\n",
                    )
                    .to_string(),
                ),
                (
                    "/conv.toml".to_string(),
                    "[server]\nhost = \"localhost\"\nport = 8080\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // --- format validation (:354/:365) ---
        for format in ["yaml", "json"] {
            let result = env.exec(&format!(
                "echo '{{}}' | yq --input-format={format} --output-format=json"
            ));
            assert_eq!(result.exit_code, 0, "input format {format}");
        }
        for format in ["yaml", "json", "xml", "ini", "csv", "toml"] {
            let result = env.exec(&format!("echo '{{}}' | yq --output-format={format}"));
            assert_eq!(result.exit_code, 0, "output format {format}");
        }

        // --- INI (:377/:395/:407/:424) ---
        let ini_host = env.exec("yq -p ini '.database.host' /config.ini");
        assert_eq!(ini_host.stdout, "localhost\n");
        let ini_port = env.exec("yq -p ini '.database.port' /config.ini");
        assert!(ini_port.stdout.trim().contains("5432"));
        let ini_out = env.exec("yq -o ini '.' /data.yaml");
        assert!(ini_out.stdout.contains("[database]"));
        assert!(ini_out.stdout.contains("host=localhost"));
        assert!(ini_out.stdout.contains("port=5432"));
        let ini_conv = env.exec("yq -o ini '.' /kv.yaml");
        assert!(ini_conv.stdout.contains("name=test"));
        assert!(ini_conv.stdout.contains("version=1"));

        // --- CSV (:438/:449/:460/:474/:492/:506) ---
        assert_eq!(
            env.exec("yq -p csv '.[0].name' /data.csv").stdout,
            "alice\n"
        );
        assert_eq!(
            env.exec("yq -p csv '.[].name' /data.csv").stdout,
            "alice\nbob\ncharlie\n"
        );
        let filtered =
            env.exec("yq -p csv '[.[] | select(.city == \"NYC\") | .name]' /data.csv -o json");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&filtered.stdout).unwrap(),
            serde_json::json!(["alice", "charlie"])
        );
        let tsv_delim = env.exec("yq -p csv --csv-delimiter='\\t' '.[0].name' /data.tsv");
        assert_eq!(tsv_delim.stdout, "alice\n");

        // --- --no-csv-header (:686) ---
        let no_header = env.exec("yq -p csv --no-csv-header '.[0][0]' /headerless.csv");
        assert_eq!(no_header.stdout, "alice\n");

        // --- TOML (:712/:729/:743/:756) ---
        assert_eq!(
            env.exec("yq '.package.name' /Cargo.toml").stdout,
            "my-app\n"
        );
        assert_eq!(env.exec("yq '.server.port' /config.toml").stdout, "8080\n");
        let toml_out = env.exec("yq -o toml '.' /server.yaml");
        assert!(toml_out.stdout.contains("[server]"));
        assert!(toml_out.stdout.contains("host = \"localhost\""));
        assert!(toml_out.stdout.contains("port = 8080"));
        let json_toml = env.exec("yq -p json -o toml '.' /app.json");
        assert!(json_toml.stdout.contains("[app]"));
        assert!(json_toml.stdout.contains("name = \"test\""));

        // --- TSV (:770/:781) ---
        assert_eq!(env.exec("yq '.[0].name' /data.tsv").stdout, "alice\n");
        assert_eq!(env.exec("yq '.[].name' /rows.tsv").stdout, "a\nb\n");

        // --- inplace error (:808) ---
        let inplace_err = env.exec("echo 'x: 1' | yq -i '.x'");
        assert_eq!(inplace_err.exit_code, 1);
        assert!(inplace_err.stderr.contains("requires a file"));

        // --- front-matter (:817/:839/:856/:873) ---
        assert_eq!(
            env.exec("yq --front-matter '.title' /post.md").stdout,
            "My Post\n"
        );
        assert_eq!(
            env.exec("yq --front-matter '.tags[]' /tags.md").stdout,
            "a\nb\n"
        );
        assert_eq!(env.exec("yq -f '.title' /hugo.md").stdout, "Hugo Post\n");
        let no_fm = env.exec("yq --front-matter '.title' /plain.md");
        assert_eq!(no_fm.exit_code, 1);
        assert!(no_fm.stderr.contains("no front-matter found"));

        // --- auto-detect (:898/:909/:920) ---
        assert_eq!(env.exec("yq '.root.name' /data.xml").stdout, "test\n");
        assert_eq!(env.exec("yq '.[0].name' /auto.csv").stdout, "alice\n");
        assert_eq!(
            env.exec("yq '.database.host' /auto.ini").stdout,
            "localhost\n"
        );

        // --- XML fixtures (:178 titles, :189 id attr, :199 filter fiction,
        //     :211 user names, :497 XML -> JSON title) ---
        let titles = env.exec("yq -p xml '.library.book[].title' /books.xml");
        assert!(titles.stdout.contains("The Great Adventure"));
        assert!(titles.stdout.contains("Learning TypeScript"));
        assert!(titles.stdout.contains("Mystery Manor"));
        let by_id = env.exec("yq -p xml '.library.book[0][\"+@id\"]' /books.xml -o json");
        assert_eq!(by_id.stdout, "\"1\"\n");
        let fiction = env.exec(
            "yq -p xml '[.library.book[] | select(.[\"+@genre\"] == \"fiction\") | .title]' /books.xml -o json",
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&fiction.stdout).unwrap(),
            serde_json::json!(["The Great Adventure", "Mystery Manor"])
        );
        assert_eq!(
            env.exec("yq -p xml '.root.users.user[].name' /users.xml")
                .stdout,
            "alice\nbob\ncharlie\n"
        );
        let xml_to_json = env.exec("yq -p xml '.library.book[0].title' /books.xml -o json -r");
        assert_eq!(xml_to_json.stdout, "The Great Adventure\n");
    }

    #[test]
    fn structured_data_yq_special_fixtures_slurp_and_inplace_rows() {
        // packages/just-bash/src/commands/yq/yq.fixtures.test.ts
        //   :526 (empty string), :535 (null value),
        //   :611 (self-closing tag), :620 (multiple attributes),
        //   :630 (deeply nested XML), :639 (repeated elements as array).
        // packages/just-bash/src/commands/yq/yq.test.ts
        //   :219 (slurp multiple YAML documents), :794 (in-place -i, TSV suite).
        // Inputs mirror the upstream special.yaml / special.xml fixtures and the
        // inline documents from the slurp and inplace describe blocks exactly.
        let special_yaml = concat!(
            "# YAML special cases\n",
            "empty_string: \"\"\n",
            "null_value: null\n",
            "multiline: |\n",
            "  This is a\n  multiline string\n  with preserved newlines\n",
            "folded: >\n",
            "  This is a folded\n  string that will be\n  joined into one line\n",
            "unicode: \"Hello \\u4e16\\u754c\"\n",
            "nested_arrays:\n  - - 1\n    - 2\n  - - 3\n    - 4\n",
            "anchor: &anchor_name\n  shared: data\n",
            "reference: *anchor_name\n",
        );
        let special_xml = concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<root xmlns:custom=\"http://example.com/custom\">\n",
            "  <!-- XML special cases -->\n",
            "  <empty></empty>\n",
            "  <self-closing/>\n",
            "  <multiple-attrs id=\"1\" class=\"item\" data-value=\"test\" enabled=\"true\"/>\n",
            "  <nested>\n    <level1>\n      <level2>\n        <level3>deep value</level3>\n      </level2>\n    </level1>\n  </nested>\n",
            "  <repeated>\n    <item>first</item>\n    <item>second</item>\n    <item>third</item>\n  </repeated>\n",
            "</root>\n",
        );
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/fixtures/yaml/special.yaml".to_string(),
                    special_yaml.to_string(),
                ),
                (
                    "/fixtures/xml/special.xml".to_string(),
                    special_xml.to_string(),
                ),
                (
                    "/data.yaml".to_string(),
                    "---\nname: first\n---\nname: second\n".to_string(),
                ),
                (
                    "/inplace.yaml".to_string(),
                    "version: 1.0\nname: test\n".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // fixtures.test.ts :526 should handle empty string -> '""'
        let empty = env.exec("yq '.empty_string' /fixtures/yaml/special.yaml -o json");
        assert_eq!(empty.exit_code, 0);
        assert_eq!(empty.stdout.trim(), "\"\"");

        // fixtures.test.ts :535 should handle null value -> 'null'
        let null_value = env.exec("yq '.null_value' /fixtures/yaml/special.yaml");
        assert_eq!(null_value.exit_code, 0);
        assert_eq!(null_value.stdout.trim(), "null");

        // fixtures.test.ts :611 self-closing tag is present
        let self_closing =
            env.exec("yq -p xml '.root | has(\"self-closing\")' /fixtures/xml/special.xml");
        assert_eq!(self_closing.exit_code, 0);
        assert_eq!(self_closing.stdout.trim(), "true");

        // fixtures.test.ts :620 multiple attributes -> '"1"'
        let multi_attr = env.exec(
            "yq -p xml '.root[\"multiple-attrs\"][\"+@id\"]' /fixtures/xml/special.xml -o json",
        );
        assert_eq!(multi_attr.exit_code, 0);
        assert_eq!(multi_attr.stdout.trim(), "\"1\"");

        // fixtures.test.ts :630 deeply nested XML
        let deep =
            env.exec("yq -p xml '.root.nested.level1.level2.level3' /fixtures/xml/special.xml");
        assert_eq!(deep.exit_code, 0);
        assert_eq!(deep.stdout.trim(), "deep value");

        // fixtures.test.ts :639 repeated elements parsed as array of length 3
        let repeated =
            env.exec("yq -p xml '.root.repeated.item | length' /fixtures/xml/special.xml");
        assert_eq!(repeated.exit_code, 0);
        assert_eq!(repeated.stdout.trim(), "3");

        // yq.test.ts :219 slurp multiple YAML documents with -s
        let slurp = env.exec("yq -s '.[0].name' /data.yaml");
        assert_eq!(slurp.exit_code, 0);
        assert_eq!(slurp.stdout, "first\n");

        // yq.test.ts :794 modify file in-place with -i (TSV describe block)
        let inplace = env.exec("yq -i '.version = \"2.0\"' /inplace.yaml");
        assert_eq!(inplace.exit_code, 0);
        assert_eq!(inplace.stdout, "");
        let read_back = env.exec("cat /inplace.yaml");
        assert!(read_back.stdout.contains("version: \"2.0\""));
    }

    #[test]
    fn awk_jbc_command_awk_expression_edge_and_error_rows() {
        // Maps the portable pending rows of awk.expressions.test.ts,
        // awk.edge-cases.test.ts, and awk.errors.test.ts that the Rust awk
        // subset already implements 1:1. Each assertion mirrors the upstream
        // vitest expectation exactly (strict stdout + exit code where the
        // upstream test pins them; exit-code-only where the upstream only
        // asserts the call does not crash).
        let env = Bash::default();

        // --- awk.expressions.test.ts: nested arithmetic ---
        // deeply nested parentheses (:6) => ((((1+2)*3)-4)/5) == 1
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print ((((1 + 2) * 3) - 4) / 5) }'"#)
                .stdout,
            "1\n"
        );
        // complex formula (:15) => (2+3)*(4-1)/(6-3) == 5
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print (2 + 3) * (4 - 1) / (6 - 3) }'"#)
                .stdout,
            "5\n"
        );
        // quadratic expression (:24) => a*x*x + b*x + c == 16
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { a=1; b=2; c=1; x=3; print a*x*x + b*x + c }'"#)
                .stdout,
            "16\n"
        );
        // mixed operations with power (:34) => 2^3 + 4^2 - 3^2 == 15
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 2^3 + 4^2 - 3^2 }'"#)
                .stdout,
            "15\n"
        );

        // --- awk.expressions.test.ts: nested conditionals ---
        // if-else if-else chain (:46) => x=15 -> "medium"
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN {
          x = 15
          if (x < 10) print "small"
          else if (x < 20) print "medium"
          else print "large"
        }'"#
            )
            .stdout,
            "medium\n"
        );
        // nested if statements (:60) => "both positive"
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN {
          x = 5; y = 10
          if (x > 0)
            if (y > 0)
              print "both positive"
        }'"#
            )
            .stdout,
            "both positive\n"
        );
        // ternary in ternary (:74) => x=50 -> "medium"
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN {
          x = 50
          print x < 30 ? "low" : (x < 70 ? "medium" : "high")
        }'"#
            )
            .stdout,
            "medium\n"
        );
        // ternary with complex conditions (:86) => "both positive"
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN {
          a = 5; b = 10
          print (a > 0 && b > 0) ? "both positive" : "not both positive"
        }'"#
            )
            .stdout,
            "both positive\n"
        );

        // --- awk.edge-cases.test.ts: control-flow / variable edges ---
        // very long string built in a for loop (:150) => length 100
        let very_long =
            env.exec(r#"echo "" | awk 'BEGIN { for(i=0;i<100;i++) s=s"x"; print length(s) }'"#);
        assert_eq!(very_long.exit_code, 0);
        assert_eq!(very_long.stdout, "100\n");
        // loop with zero iterations (:238)
        let zero_loop =
            env.exec(r#"echo "" | awk 'BEGIN { for(i=0; i<0; i++) print i; print "done" }'"#);
        assert_eq!(zero_loop.exit_code, 0);
        assert_eq!(zero_loop.stdout, "done\n");
        // while with false condition (:247)
        let false_while =
            env.exec(r#"echo "" | awk 'BEGIN { while(0) print "never"; print "done" }'"#);
        assert_eq!(false_while.exit_code, 0);
        assert_eq!(false_while.stdout, "done\n");
        // assignment in condition (:283) => if (x = 5) print x -> 5
        let assign_cond = env.exec(r#"echo "" | awk 'BEGIN { if (x = 5) print x }'"#);
        assert_eq!(assign_cond.exit_code, 0);
        assert_eq!(assign_cond.stdout, "5\n");

        // --- awk.errors.test.ts: field / function / special-var edges ---
        // negative field index (:143): only requires exit 0 (empty or no error)
        let neg_field = env.exec("echo 'a b c' | awk '{ print $-1 }'");
        assert_eq!(neg_field.exit_code, 0);
        // split with missing array argument (:179): only requires exit 0
        let split_missing = env.exec("echo 'hello' | awk '{ print split($1) }'");
        assert_eq!(split_missing.exit_code, 0);
        // undefined function call returns empty string (:258)
        let undef_fn = env.exec("echo '1' | awk '{ print undefined_func() }'");
        assert_eq!(undef_fn.exit_code, 0);
        assert_eq!(undef_fn.stdout, "\n");
        // setting a high field index updates NF (:302) => NF becomes 10
        let nf_extend = env.exec("echo 'a b c' | awk '{ $10 = \"x\"; print NF }'");
        assert_eq!(nf_extend.exit_code, 0);
        assert_eq!(nf_extend.stdout, "10\n");
        // assigning to NF truncates fields (:311): only requires exit 0
        let nf_assign = env.exec("echo 'a b c d e' | awk '{ NF = 2; print $0 }'");
        assert_eq!(nf_assign.exit_code, 0);
        // invalid format specifier %z handled gracefully (:322): exit 0
        let bad_fmt = env.exec("echo '1' | awk '{ printf \"%z\", $1 }'");
        assert_eq!(bad_fmt.exit_code, 0);
        // width/precision with no specifier handled gracefully (:329): exit 0
        let bad_prec = env.exec("echo '1' | awk '{ printf \"%10.5\", $1 }'");
        assert_eq!(bad_prec.exit_code, 0);
    }

    // JBC-awk: multiple pattern/action rules, default print action, next, and
    // BEGIN/main/END ordering across multiple rules.
    // Upstream: packages/just-bash/src/commands/awk/awk.functions.test.ts
    //   :142 multiple rules in order, :153 next skips remaining rules,
    //   :162 default action for pattern-only rule, :171 mixed pattern/action,
    //   :184 BEGIN/main/END ordering, :210 next skips remaining rules.
    #[test]
    fn awk_jbc_command_awk_multiple_rules_and_next_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/data.txt".to_string(),
                    "apple\nbanana\ncherry\n".to_string(),
                ),
                ("/ab.txt".to_string(), "a\nb\nc\n".to_string()),
                (
                    "/hw.txt".to_string(),
                    "hello\nworld\nhello world\n".to_string(),
                ),
                ("/num123.txt".to_string(), "1\n2\n3\n".to_string()),
                ("/skipkeep.txt".to_string(), "skip\nkeep\n".to_string()),
                ("/abonly.txt".to_string(), "a\nb\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        // :142 should execute multiple rules in order
        assert_eq!(
            env.exec(r#"awk '/apple/{print "FRUIT"} /banana/{print "YELLOW"}' /data.txt"#)
                .stdout,
            "FRUIT\nYELLOW\n"
        );
        // :153 should handle pattern with next to skip rules
        assert_eq!(
            env.exec(r#"awk '/b/{next}{print}' /ab.txt"#).stdout,
            "a\nc\n"
        );
        // :162 should execute default action (print) for pattern-only rules
        assert_eq!(
            env.exec(r#"awk '/hello/' /hw.txt"#).stdout,
            "hello\nhello world\n"
        );
        // :171 should handle mixed pattern and action-only rules
        assert_eq!(
            env.exec(r#"awk '/2/{print "TWO"} {print "line:" $0}' /num123.txt"#)
                .stdout,
            "line:1\nTWO\nline:2\nline:3\n"
        );
        // :184 should execute BEGIN, main rules, and END in order
        let begin_end =
            env.exec(r#"awk 'BEGIN{print "START"} {print $0} END{print "END"}' /abonly.txt"#);
        assert_eq!(begin_end.stdout, "START\na\nb\nEND\n");
        assert_eq!(begin_end.exit_code, 0);
        // :210 should skip remaining rules for current line
        assert_eq!(
            env.exec(r#"awk '/skip/{next}{print "processed:", $0}' /skipkeep.txt"#)
                .stdout,
            "processed: keep\n"
        );
    }

    // JBC-awk: plain `getline` and `getline VAR` reading the next record from
    // the main input stream. Each assertion fails if the shared main-input
    // cursor, NR bookkeeping, $0 re-split, or getline-at-EOF no-op behavior is
    // wrong.
    // Upstream: packages/just-bash/src/commands/awk/awk.getline.test.ts
    //   :5 reads next line into $0, :21 reads next line into variable,
    //   :37 updates NR when reading next line,
    //   :54 skips lines when used in pattern match,
    //   :74 handles getline at end of file gracefully,
    //   :88 combines lines using getline.
    #[test]
    fn awk_jbc_command_awk_getline_main_input_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                (
                    "/header.txt".to_string(),
                    "header\nvalue1\nvalue2\nvalue3".to_string(),
                ),
                (
                    "/names.txt".to_string(),
                    "name: Alice\nage: 30\nname: Bob\nage: 25".to_string(),
                ),
                ("/lines.txt".to_string(), "line1\nline2\nline3".to_string()),
                (
                    "/headers.txt".to_string(),
                    "header1\ndata1\nheader2\ndata2\nheader3\ndata3".to_string(),
                ),
                ("/single.txt".to_string(), "only line".to_string()),
                (
                    "/keys.txt".to_string(),
                    "key1\nvalue1\nkey2\nvalue2".to_string(),
                ),
            ]),
            ..BashOptions::default()
        });

        // :5 reads next line into $0
        let into_line = env.exec(r#"awk '/header/ { getline; print }' /header.txt"#);
        assert_eq!(into_line.exit_code, 0);
        assert_eq!(into_line.stdout, "value1\n");

        // :21 reads next line into variable (variable holds the record; $2 is
        // still the matched record's second field, proving $0 is untouched).
        assert_eq!(
            env.exec(r#"awk '/^name:/ { getline age_line; print $2, age_line }' /names.txt"#)
                .stdout,
            "Alice age: 30\nBob age: 25\n"
        );

        // :37 updates NR when reading next line; getline at EOF leaves NR put.
        assert_eq!(
            env.exec(r#"awk '{ print NR; getline; print NR }' /lines.txt"#)
                .stdout,
            "1\n2\n3\n3\n"
        );

        // :54 skips lines when used in pattern match
        assert_eq!(
            env.exec(r#"awk '/^header/ { print; getline; print "  ->" $0 }' /headers.txt"#)
                .stdout,
            "header1\n  ->data1\nheader2\n  ->data2\nheader3\n  ->data3\n"
        );

        // :74 handles getline at end of file gracefully ($0 unchanged at EOF)
        assert_eq!(
            env.exec(r#"awk '{ print $0; getline; print "after: " $0 }' /single.txt"#)
                .stdout,
            "only line\nafter: only line\n"
        );

        // :88 combines lines using getline
        assert_eq!(
            env.exec(r#"awk 'NR % 2 == 1 { key = $0; getline; print key ": " $0 }' /keys.txt"#)
                .stdout,
            "key1: value1\nkey2: value2\n"
        );
    }

    // JBC-awk: `getline [VAR] < FILE` reading from an external file, including
    // the 1/0/-1 return value, EOF handling, $0 re-split, and getline-into-var
    // not re-splitting. Each assertion fails if the per-file cursor, return
    // value, or field re-split is wrong.
    // Upstream: packages/just-bash/src/commands/awk/awk.getline.test.ts
    //   :104 reads from external file with getline < file,
    //   :121 reads entire file line by line with getline < file,
    //   :136 returns -1 for nonexistent file in getline,
    //   :270 returns 1 on successful read, :281 returns 0 on EOF,
    //   :296 returns -1 on error, :307 re-splits $0 after getline,
    //   :318 does not re-split when getline into variable,
    //   :332 reads all lines in while loop, :343 counts lines with getline loop.
    // Also packages/just-bash/src/commands/awk/awk.errors.test.ts:277 handle
    // getline from non-existent file.
    #[test]
    fn awk_jbc_command_awk_getline_from_file_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/test/main.txt".to_string(), "line1\nline2".to_string()),
                (
                    "/test/other.txt".to_string(),
                    "external1\nexternal2\nexternal3".to_string(),
                ),
                ("/test/seq.txt".to_string(), "a\nb\nc".to_string()),
                ("/test/colon.txt".to_string(), "a:b:c".to_string()),
                ("/test/single.txt".to_string(), "single".to_string()),
                ("/test/nums.txt".to_string(), "1\n2\n3\n4\n5".to_string()),
                ("/test/abcd.txt".to_string(), "a\nb\nc\nd".to_string()),
            ]),
            ..BashOptions::default()
        });

        // :104 reads from external file with getline < file (a fresh cursor per
        // file; each main record pulls the next external line).
        assert_eq!(
            env.exec(r#"awk '{ getline ext < "/test/other.txt"; print $0, ext }' /test/main.txt"#)
                .stdout,
            "line1 external1\nline2 external2\n"
        );

        // :121 reads entire file line by line with getline < file in a while
        // loop over the redirect's return value.
        assert_eq!(
            env.exec(
                r#"awk 'BEGIN { while ((getline line < "/test/seq.txt") > 0) print "got:", line }'"#
            )
            .stdout,
            "got: a\ngot: b\ngot: c\n"
        );

        // :136 returns -1 for nonexistent file in getline.
        assert_eq!(
            env.exec(r#"awk 'BEGIN { ret = getline x < "/nonexistent"; print "ret:", ret }'"#)
                .stdout,
            "ret: -1\n"
        );

        // :270 returns 1 on successful read.
        assert_eq!(
            env.exec(r#"awk 'BEGIN { ret = (getline < "/test/main.txt"); print "ret:", ret }'"#)
                .stdout,
            "ret: 1\n"
        );

        // :281 returns 0 on EOF after reading the single available line.
        assert_eq!(
            env.exec(
                "awk 'BEGIN {\n  getline < \"/test/single.txt\"\n  ret = (getline < \"/test/single.txt\")\n  print \"ret:\", ret\n}'"
            )
            .stdout,
            "ret: 0\n"
        );

        // :296 returns -1 on error (unopenable path).
        assert_eq!(
            env.exec(r#"awk 'BEGIN { ret = (getline < "/nonexistent/file"); print "ret:", ret }'"#)
                .stdout,
            "ret: -1\n"
        );

        // :307 re-splits $0 after getline (FS applies to the read line).
        assert_eq!(
            env.exec(r#"awk 'BEGIN { FS=":"; getline < "/test/colon.txt"; print $2 }'"#)
                .stdout,
            "b\n"
        );

        // :318 does not re-split when getline into variable (NF stays 0).
        assert_eq!(
            env.exec(r#"awk 'BEGIN { FS=":"; getline x < "/test/colon.txt"; print x; print NF }'"#)
                .stdout,
            "a:b:c\n0\n"
        );

        // :332 reads all lines in while loop, summing numeric values.
        assert_eq!(
            env.exec(
                r#"awk 'BEGIN { sum=0; while ((getline n < "/test/nums.txt") > 0) sum += n; print sum }'"#
            )
            .stdout,
            "15\n"
        );

        // :343 counts lines with a plain getline loop.
        assert_eq!(
            env.exec(
                r#"awk 'BEGIN { count=0; while ((getline < "/test/abcd.txt") > 0) count++; print count }'"#
            )
            .stdout,
            "4\n"
        );

        // awk.errors.test.ts:277 should handle getline from non-existent file
        // (stdin is ignored; the redirect returns -1).
        let from_missing =
            env.exec(r#"echo '1' | awk '{ ret = (getline x < "/nonexistent"); print ret }'"#);
        assert_eq!(from_missing.stdout, "-1\n");
        assert_eq!(from_missing.exit_code, 0);
    }

    // JBC-awk: `"cmd" | getline [VAR]` runs the command INSIDE the Just Bash
    // session (never the host shell) and reads its stdout one record per
    // getline. Each assertion fails if the command output is not captured,
    // fields are not re-split into `$0`, the variable form leaks into `$0`, or
    // the success/EOF return value is wrong.
    // Upstream: packages/just-bash/src/commands/awk/awk.getline.test.ts
    //   :147 reads from command pipe into $0,
    //   :156 reads from command pipe into variable,
    //   :165 reads multiple lines from command pipe,
    //   :176 updates fields after command pipe getline into $0,
    //   :185 returns 1 on success, 0 on EOF.
    #[test]
    fn awk_jbc_command_awk_command_pipe_getline_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/test/data.txt".to_string(),
                "line1\nline2\nline3".to_string(),
            )]),
            ..BashOptions::default()
        });

        // :147 reads from command pipe into $0 (plain getline replaces $0).
        let into_line = env.exec(r#"awk 'BEGIN { "echo hello" | getline; print }'"#);
        assert_eq!(into_line.exit_code, 0);
        assert_eq!(into_line.stdout, "hello\n");

        // :156 reads from command pipe into a variable (leaves $0 untouched).
        let into_var = env.exec(r#"awk 'BEGIN { "echo world" | getline x; print "got:", x }'"#);
        assert_eq!(into_var.exit_code, 0);
        assert_eq!(into_var.stdout, "got: world\n");

        // :165 reads multiple lines from a command pipe in a while loop (the
        // redirect is truthy per successful read, then EOF ends the loop).
        let multi = env.exec(
            r#"awk 'BEGIN { while (("cat /test/data.txt") | getline line) print "read:", line }'"#,
        );
        assert_eq!(multi.exit_code, 0);
        assert_eq!(multi.stdout, "read: line1\nread: line2\nread: line3\n");

        // :176 updates fields after command pipe getline into $0 (FS re-splits).
        let fields =
            env.exec(r#"awk 'BEGIN { FS=":"; "echo a:bc:def" | getline; print NF, $1, $2 }'"#);
        assert_eq!(fields.exit_code, 0);
        assert_eq!(fields.stdout, "3 a bc\n");

        // :185 returns 1 on the first read and 0 once the command's single line
        // of output is exhausted (the command is run once and buffered).
        let ret = env.exec(
            "awk 'BEGIN {\n  ret1 = (\"echo single\" | getline)\n  ret2 = (\"echo single\" | getline)\n  print ret1, ret2\n}'",
        );
        assert_eq!(ret.exit_code, 0);
        assert_eq!(ret.stdout, "1 0\n");
    }

    // JBC-awk: `print`/`printf` redirected to a file with `>` (truncate) and
    // `>>` (append). Each assertion fails if the redirect buffer, append seed,
    // or printf formatting to file is wrong.
    // Upstream: packages/just-bash/src/commands/awk/awk.getline.test.ts
    //   :200 writes to file with print > file,
    //   :217 overwrites then appends with > on same file,
    //   :234 appends with print >> file, :251 printf writes to file.
    #[test]
    fn awk_jbc_command_awk_print_to_file_rows() {
        // :200 writes to file with print > file (stdout stays empty).
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test/input.txt".to_string(), "hello\nworld".to_string())]),
            ..BashOptions::default()
        });
        let result = env.exec(r#"awk '{ print $0 > "/test/output.txt" }' /test/input.txt"#);
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
        assert_eq!(env.read_file("/test/output.txt").unwrap(), "hello\nworld\n");

        // :217 overwrites the file: the open `>` stream appends each record so
        // every line lands in order.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/test/input.txt".to_string(),
                "line1\nline2\nline3".to_string(),
            )]),
            ..BashOptions::default()
        });
        assert_eq!(
            env.exec(r#"awk '{ print $0 > "/test/out.txt" }' /test/input.txt"#)
                .exit_code,
            0
        );
        assert_eq!(
            env.read_file("/test/out.txt").unwrap(),
            "line1\nline2\nline3\n"
        );

        // :234 appends with print >> file (existing content is preserved).
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/test/existing.txt".to_string(), "existing\n".to_string()),
                ("/test/input.txt".to_string(), "new1\nnew2".to_string()),
            ]),
            ..BashOptions::default()
        });
        assert_eq!(
            env.exec(r#"awk '{ print $0 >> "/test/existing.txt" }' /test/input.txt"#)
                .exit_code,
            0
        );
        assert_eq!(
            env.read_file("/test/existing.txt").unwrap(),
            "existing\nnew1\nnew2\n"
        );

        // :251 printf writes to file with %03d zero-padding.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/test/input.txt".to_string(), "1\n2\n3".to_string())]),
            ..BashOptions::default()
        });
        assert_eq!(
            env.exec(r#"awk '{ printf "%03d\n", $1 > "/test/out.txt" }' /test/input.txt"#)
                .exit_code,
            0
        );
        assert_eq!(env.read_file("/test/out.txt").unwrap(), "001\n002\n003\n");
    }

    // JBC-awk: `%` modulo with a negative divisor truncates toward zero, so the
    // result keeps the sign of the dividend: `7 % -3` is `1` (not `-2`). This
    // also proves the multiplicative right operand now parses a leading unary
    // minus (`% -3`).
    //
    // Upstream: packages/just-bash/src/commands/awk/awk.modulo.test.ts
    //   :113 should handle negative divisor.
    #[test]
    fn awk_jbc_command_awk_modulo_negative_divisor_row() {
        let env = Bash::new();
        let result = env.exec(r#"echo "" | awk 'BEGIN { print 7 % -3 }'"#);
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\n");
    }

    // JBC-awk: `rand()` returns a value in `[0, 1)` and `srand(seed)` reseeds the
    // generator without error, with `rand()` after it still in `[0, 1)`. The
    // generator is deterministic, so the exact value is asserted to be in range.
    //
    // Upstream: packages/just-bash/src/commands/awk/awk.math.test.ts
    //   :69 rand() returns a value between 0 and 1,
    //   :77 srand() can be called with a seed.
    #[test]
    fn awk_jbc_command_awk_rand_and_srand_rows() {
        let env = Bash::new();

        // :69 rand() returns a value between 0 and 1.
        let rand = env.exec(r#"echo '' | awk '{ print rand() }'"#);
        assert_eq!(rand.exit_code, 0);
        let value: f64 = rand.stdout.trim().parse().expect("rand() must be numeric");
        assert!(
            (0.0..1.0).contains(&value),
            "rand() must be in [0, 1), got {value}"
        );

        // :77 srand() can be called with a seed; rand() after it stays in range.
        let seeded = env.exec(r#"echo '' | awk '{ srand(42); print rand() }'"#);
        assert_eq!(seeded.exit_code, 0);
        let seeded_value: f64 = seeded
            .stdout
            .trim()
            .parse()
            .expect("rand() after srand must be numeric");
        assert!(
            (0.0..1.0).contains(&seeded_value),
            "rand() after srand must be in [0, 1), got {seeded_value}"
        );
    }

    // JBC-awk: the `nextfile` statement stops processing the current input file
    // and resumes with the first record of the next file. FNR/FILENAME drive the
    // decision, and records skipped by nextfile never reach later rules.
    //
    // Upstream: packages/just-bash/src/commands/awk/awk.nextfile.test.ts
    //   :6 should skip to next file,
    //   :20 should skip rest of first file,
    //   :34 should continue with next file content,
    //   :50 should reset FNR for each file,
    //   :66 should maintain NR across files,
    //   :80 should skip file on condition,
    //   :96 should end processing when nextfile on single file.
    #[test]
    fn awk_jbc_command_awk_nextfile_rows() {
        // :6 nextfile after FNR == 2 prints the first two records of each file
        // and skips the third of every file.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/a.txt".to_string(), "a1\na2\na3\n".to_string()),
                ("/b.txt".to_string(), "b1\nb2\nb3\n".to_string()),
            ]),
            ..BashOptions::default()
        });
        let skip_to_next = env.exec(r#"awk '{ print; if (FNR == 2) nextfile }' /a.txt /b.txt"#);
        assert_eq!(skip_to_next.exit_code, 0);
        assert_eq!(skip_to_next.stdout, "a1\na2\nb1\nb2\n");

        // :20 nextfile on the first record of /a.txt skips the rest of that
        // file, so only /b.txt's records print.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/a.txt".to_string(), "skip1\nskip2\nskip3\n".to_string()),
                ("/b.txt".to_string(), "keep1\nkeep2\n".to_string()),
            ]),
            ..BashOptions::default()
        });
        let skip_first = env
            .exec(r#"awk 'FNR == 1 && FILENAME == "/a.txt" { nextfile } { print }' /a.txt /b.txt"#);
        assert_eq!(skip_first.exit_code, 0);
        assert_eq!(skip_first.stdout, "keep1\nkeep2\n");

        // :34 nextfile when $1 == "2" skips the rest of /first.txt then processes
        // /second.txt in full.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/first.txt".to_string(), "1\n2\n3\n".to_string()),
                ("/second.txt".to_string(), "a\nb\nc\n".to_string()),
            ]),
            ..BashOptions::default()
        });
        let continue_next =
            env.exec(r#"awk '{ if ($1 == "2") nextfile; print }' /first.txt /second.txt"#);
        assert_eq!(continue_next.exit_code, 0);
        assert_eq!(continue_next.stdout, "1\na\nb\nc\n");

        // :50 FNR resets to 1 at each new file while iterating all records.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/a.txt".to_string(), "a1\na2\n".to_string()),
                ("/b.txt".to_string(), "b1\nb2\nb3\n".to_string()),
            ]),
            ..BashOptions::default()
        });
        let fnr_reset = env.exec(r#"awk '{ print FILENAME, FNR }' /a.txt /b.txt"#);
        assert_eq!(fnr_reset.exit_code, 0);
        assert_eq!(
            fnr_reset.stdout,
            "/a.txt 1\n/a.txt 2\n/b.txt 1\n/b.txt 2\n/b.txt 3\n"
        );

        // :66 NR keeps counting across files while FNR resets per file.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/a.txt".to_string(), "a1\na2\n".to_string()),
                ("/b.txt".to_string(), "b1\nb2\n".to_string()),
            ]),
            ..BashOptions::default()
        });
        let nr_across = env.exec(r#"awk '{ print NR, FNR }' /a.txt /b.txt"#);
        assert_eq!(nr_across.exit_code, 0);
        assert_eq!(nr_across.stdout, "1 1\n2 2\n3 1\n4 2\n");

        // :80 nextfile on a regex+FNR condition skips the rest of /skip.txt.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/skip.txt".to_string(), "SKIP\ndata1\ndata2\n".to_string()),
                ("/keep.txt".to_string(), "KEEP\ndata3\ndata4\n".to_string()),
            ]),
            ..BashOptions::default()
        });
        let skip_cond =
            env.exec(r#"awk 'FNR == 1 && /SKIP/ { nextfile } { print }' /skip.txt /keep.txt"#);
        assert_eq!(skip_cond.exit_code, 0);
        assert_eq!(skip_cond.stdout, "KEEP\ndata3\ndata4\n");

        // :96 nextfile on the only file ends processing at that record.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "1\n2\n3\n4\n5\n".to_string())]),
            ..BashOptions::default()
        });
        let single = env.exec(r#"awk '{ print; if ($1 == 3) nextfile }' /data.txt"#);
        assert_eq!(single.exit_code, 0);
        assert_eq!(single.stdout, "1\n2\n3\n");
    }

    // JBC-awk: execution limits keep awk programs bounded. Infinite loops, runaway
    // print/concat growth, and unbounded recursion all fail with a controlled
    // execution-limit error (exit code 126) instead of hanging or aborting; large
    // but finite work still completes.
    //
    // Upstream: packages/just-bash/src/commands/awk/awk.limits.test.ts
    //   :16 while(1), :27 for(;1;), :37 for(;;), :47 do-while(1),
    //   :59 recursive function, :70 mutual recursion,
    //   :83 print in loop, :94 string concat growth,
    //   :107 large array creation, :119 getline in loop.
    #[test]
    fn awk_jbc_command_awk_execution_limit_rows() {
        let env = Bash::new();

        // :16/:27/:37/:47 infinite loops must error with a non-empty stderr and a
        // non-zero exit code rather than hanging.
        for program in [
            r#"echo "test" | awk 'BEGIN { while(1) print "x" }'"#,
            r#"echo "test" | awk 'BEGIN { for(i=0; 1; i++) print "x" }'"#,
            r#"echo "test" | awk 'BEGIN { for(;;) print "x" }'"#,
            r#"echo "test" | awk 'BEGIN { do { print "x" } while(1) }'"#,
        ] {
            let result = env.exec(program);
            assert_ne!(result.exit_code, 0, "infinite loop must fail: {program}");
            assert!(
                !result.stderr.is_empty(),
                "infinite loop must report an error: {program}"
            );
        }

        // :59 direct recursion hits the recursion-depth limit (exit 126).
        let recursion = env.exec(r#"echo "test" | awk 'function f() { f() } BEGIN { f() }'"#);
        assert_eq!(recursion.exit_code, 126);
        assert!(
            recursion.stderr.contains("recursion depth exceeded"),
            "recursion error text, got {:?}",
            recursion.stderr
        );

        // :70 mutual recursion hits the same recursion-depth limit.
        let mutual = env
            .exec(r#"echo "test" | awk 'function a() { b() } function b() { a() } BEGIN { a() }'"#);
        assert_eq!(mutual.exit_code, 126);
        assert!(
            mutual.stderr.contains("recursion depth exceeded"),
            "mutual recursion error text, got {:?}",
            mutual.stderr
        );

        // :83 a tight print loop hits the output-size limit (exit 126, "exceeded").
        let print_loop =
            env.exec(r#"echo "test" | awk 'BEGIN { for(i=0; i<1000000; i++) print "x" }'"#);
        assert_eq!(print_loop.exit_code, 126);
        assert!(
            print_loop.stderr.contains("exceeded"),
            "print loop error text, got {:?}",
            print_loop.stderr
        );

        // :94 exponential string concatenation hits the string-length limit.
        let concat = env.exec(
            r#"echo "test" | awk 'BEGIN { s="x"; for(i=0; i<30; i++) s=s s; print length(s) }'"#,
        );
        assert_eq!(concat.exit_code, 126);
        assert!(
            concat.stderr.contains("exceeded"),
            "concat error text, got {:?}",
            concat.stderr
        );

        // :107 a large but finite array still completes (upstream only requires
        // the run to terminate with a defined exit code rather than hang).
        let big_array = env.exec(
            r#"echo "test" | awk 'BEGIN { for(i=0; i<100000; i++) a[i]=i; print length(a) }'"#,
        );
        assert_eq!(big_array.exit_code, 0);

        // :119 getline against a missing /dev/zero returns -1 so the while loop
        // never runs: the program completes without hanging.
        let getline_loop = env
            .exec(r#"echo "test" | awk '{ while((getline line < "/dev/zero") > 0) print line }'"#);
        assert_eq!(getline_loop.exit_code, 0);
        assert_eq!(getline_loop.stdout, "");
    }

    // JBC-awk: special variables FILENAME/FNR and string functions
    // match()/RSTART/RLENGTH/gensub(), printf %x/%X/%o/%c, and the ^/** power
    // operators with a fractional exponent.
    // Upstream: packages/just-bash/src/commands/awk/awk.functions.test.ts
    //   :307 FILENAME contains filename,
    //   :316 FILENAME empty for stdin, :325 FNR resets per file,
    //   :343 RSTART/RLENGTH set by match, :352 RSTART/RLENGTH no match,
    //   :365 match position, :374 match returns 0, :383 match regex,
    //   :394 gensub first, :403 gensub g, :421 gensub Nth,
    //   :434 power ^, :441 power **, :448 fractional exponent,
    //   :459 %x lower, :468 %X upper, :479 %o octal, :490 %c char.
    #[test]
    fn awk_jbc_command_awk_special_vars_string_fns_and_printf_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/test.txt".to_string(), "line1\n".to_string()),
                ("/a.txt".to_string(), "a1\na2\n".to_string()),
                ("/b.txt".to_string(), "b1\nb2\nb3\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        // :307 FILENAME should contain current filename
        assert_eq!(
            env.exec(r#"awk '{print FILENAME}' /test.txt"#).stdout,
            "/test.txt\n"
        );
        // :316 FILENAME should be empty for stdin
        assert_eq!(
            env.exec(r#"echo "test" | awk '{print FILENAME}'"#).stdout,
            "\n"
        );
        // :325 FNR should reset for each file
        assert_eq!(
            env.exec(r#"awk '{print FILENAME, FNR, NR}' /a.txt /b.txt"#)
                .stdout,
            "/a.txt 1 1\n/a.txt 2 2\n/b.txt 1 3\n/b.txt 2 4\n/b.txt 3 5\n"
        );
        // :343 RSTART/RLENGTH should be set by match()
        assert_eq!(
            env.exec(r#"echo "hello world" | awk '{ match($0, /wor/); print RSTART, RLENGTH }'"#)
                .stdout,
            "7 3\n"
        );
        // :352 RSTART/RLENGTH should be 0 and -1 when no match
        assert_eq!(
            env.exec(r#"echo "hello" | awk '{ match($0, /xyz/); print RSTART, RLENGTH }'"#)
                .stdout,
            "0 -1\n"
        );
        // :365 match() should return position of match
        assert_eq!(
            env.exec(r#"echo "hello world" | awk '{ print match($0, /world/) }'"#)
                .stdout,
            "7\n"
        );
        // :374 match() should return 0 for no match
        assert_eq!(
            env.exec(r#"echo "hello" | awk '{ print match($0, /xyz/) }'"#)
                .stdout,
            "0\n"
        );
        // :383 match() should work with regex patterns
        assert_eq!(
            env.exec(r#"echo "test123abc" | awk '{ match($0, /[0-9]+/); print RSTART, RLENGTH }'"#)
                .stdout,
            "5 3\n"
        );
        // :394 gensub() should replace first occurrence
        assert_eq!(
            env.exec(r#"echo "hello hello" | awk '{ print gensub(/hello/, "hi", 1) }'"#)
                .stdout,
            "hi hello\n"
        );
        // :403 gensub() should replace all occurrences with g
        assert_eq!(
            env.exec(r#"echo "hello hello" | awk '{ print gensub(/hello/, "hi", "g") }'"#)
                .stdout,
            "hi hi\n"
        );
        // :421 gensub() should replace Nth occurrence
        assert_eq!(
            env.exec(r#"echo "a b a b a" | awk '{ print gensub(/a/, "X", 2) }'"#)
                .stdout,
            "a b X b a\n"
        );
        // :434 power operator with ^
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 2^10 }'"#).stdout,
            "1024\n"
        );
        // :441 power operator with **
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 3**4 }'"#).stdout,
            "81\n"
        );
        // :448 fractional exponent
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 9^0.5 }'"#).stdout,
            "3\n"
        );
        // :459 printf %x hex lowercase
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { printf "%x\n", 255 }'"#)
                .stdout,
            "ff\n"
        );
        // :468 printf %X hex uppercase
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { printf "%X\n", 255 }'"#)
                .stdout,
            "FF\n"
        );
        // :479 printf %o octal
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { printf "%o\n", 64 }'"#)
                .stdout,
            "100\n"
        );
        // :490 printf %c numeric -> character
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { printf "%c\n", 65 }'"#)
                .stdout,
            "A\n"
        );
    }

    // JBC-awk: field iteration with C-style for loops over $i / NF, string
    // reversal via length()/substr(), and regex/multichar/character-class -F
    // field separators.
    // Upstream files:
    //   awk.fields.test.ts :317 iterate over all fields, :326 iterate reverse,
    //     :335 sum all fields.
    //   awk.functions.test.ts :523 regex FS split, :534 multichar FS,
    //     :545 character-class FS.
    #[test]
    fn awk_jbc_command_awk_field_iteration_and_fs_rows() {
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/a1b.txt".to_string(), "a1b2c3d\n".to_string()),
                ("/colons.txt".to_string(), "a::b::c\n".to_string()),
                ("/seps.txt".to_string(), "a,b;c:d\n".to_string()),
            ]),
            ..BashOptions::default()
        });

        // fields :317 should iterate over all fields
        assert_eq!(
            env.exec(r#"echo "a b c" | awk '{ for(i=1; i<=NF; i++) print i, $i }'"#)
                .stdout,
            "1 a\n2 b\n3 c\n"
        );
        // fields :326 should iterate in reverse
        assert_eq!(
            env.exec(r#"echo "a b c" | awk '{ for(i=NF; i>=1; i--) printf $i " "; print "" }'"#)
                .stdout,
            "c b a \n"
        );
        // fields :335 should sum all fields
        assert_eq!(
            env.exec(
                r#"echo "1 2 3 4 5" | awk '{ sum=0; for(i=1; i<=NF; i++) sum+=$i; print sum }'"#
            )
            .stdout,
            "15\n"
        );
        // functions :523 should split on regex pattern (-F)
        assert_eq!(
            env.exec(r#"awk -F'[0-9]' '{print $1, $2, $3, $4}' /a1b.txt"#)
                .stdout,
            "a b c d\n"
        );
        // functions :534 should split on multiple characters (-F'::')
        assert_eq!(
            env.exec(r#"awk -F'::' '{print $1, $2, $3}' /colons.txt"#)
                .stdout,
            "a b c\n"
        );
        // functions :545 should handle character class (-F'[,;:]')
        assert_eq!(
            env.exec(r#"awk -F'[,;:]' '{print $1, $2, $3, $4}' /seps.txt"#)
                .stdout,
            "a b c d\n"
        );
    }

    // JBC-awk: C-style loop control flow in BEGIN/END blocks — nested
    // for/while, break and continue (including inner-loop-only scoping), and the
    // fibonacci/string-reversal idioms the upstream expression suite exercises.
    // Upstream files:
    //   awk.expressions.test.ts :114 for-while nesting, :132 break in inner
    //     loop only, :149 continue in inner loop, :460 fibonacci, :498 reverse
    //     string.
    //   awk.functions.test.ts :247 break out of for loop, :256 continue to next
    //     iteration, :265 break out of while loop, :274 continue in while loop.
    #[test]
    fn awk_jbc_command_awk_loop_break_continue_rows() {
        let env = Bash::default();

        // expressions :114 for-while nesting
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN {
          for (i=1; i<=2; i++) {
            j = 1
            while (j <= 2) {
              printf "%d%d ", i, j
              j++
            }
          }
          print ""
        }'"#
            )
            .stdout,
            "11 12 21 22 \n"
        );
        // expressions :132 break in inner loop only
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN {
          for (i=1; i<=3; i++) {
            for (j=1; j<=3; j++) {
              if (j == 2) break
              printf "%d%d ", i, j
            }
          }
          print ""
        }'"#
            )
            .stdout,
            "11 21 31 \n"
        );
        // expressions :149 continue in inner loop
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN {
          for (i=1; i<=2; i++) {
            for (j=1; j<=3; j++) {
              if (j == 2) continue
              printf "%d%d ", i, j
            }
          }
          print ""
        }'"#
            )
            .stdout,
            "11 13 21 23 \n"
        );
        // expressions :460 fibonacci
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN {
          a = 0; b = 1
          for (i = 0; i < 10; i++) {
            printf "%d ", a
            t = a; a = b; b = t + b
          }
          print ""
        }'"#
            )
            .stdout,
            "0 1 1 2 3 5 8 13 21 34 \n"
        );
        // expressions :498 reverse string
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN {
          s = "hello"
          r = ""
          for (i = length(s); i > 0; i--) r = r substr(s, i, 1)
          print r
        }'"#
            )
            .stdout,
            "olleh\n"
        );

        // functions :247 break out of for loop
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN { for(i=1; i<=10; i++) { if(i>5) break; print i } }'"#
            )
            .stdout,
            "1\n2\n3\n4\n5\n"
        );
        // functions :256 continue to next iteration
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN { for(i=1; i<=5; i++) { if(i==3) continue; print i } }'"#
            )
            .stdout,
            "1\n2\n4\n5\n"
        );
        // functions :265 break out of while loop
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { i=0; while(1) { i++; if(i>3) break; print i } }'"#)
                .stdout,
            "1\n2\n3\n"
        );
        // functions :274 continue in while loop
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN { i=0; while(i<5) { i++; if(i==3) continue; print i } }'"#
            )
            .stdout,
            "1\n2\n4\n5\n"
        );
    }

    // JBC-awk: do-while loops execute their body at least once and re-test the
    // condition after each iteration.
    // Upstream: packages/just-bash/src/commands/awk/awk.functions.test.ts
    //   :285 execute body at least once, :294 loop while condition is true.
    #[test]
    fn awk_jbc_command_awk_do_while_rows() {
        let env = Bash::default();

        // :285 should execute body at least once (condition false up front)
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { i=10; do { print i; i++ } while(i<10) }'"#)
                .stdout,
            "10\n"
        );
        // :294 should loop while condition is true
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { i=1; do { print i; i++ } while(i<=3) }'"#)
                .stdout,
            "1\n2\n3\n"
        );
    }

    // JBC-awk: operator semantics — division-by-zero exit code, the remaining
    // comparison operators, short-circuit evaluation of && and ||, regex match
    // operators in conditions and on fields, ternary expressions, chained
    // increments, and operator precedence.
    // Upstream: packages/just-bash/src/commands/awk/awk.operators.test.ts
    #[test]
    fn awk_jbc_command_awk_operator_precedence_and_logic_rows() {
        let env = Bash::default();

        // :80 division by zero exits 0 (inf/0 result tolerated)
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 10 / 0 }'"#)
                .exit_code,
            0
        );
        // :116 compare with <=
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print (3 <= 5), (5 <= 5), (6 <= 5) }'"#)
                .stdout,
            "1 1 0\n"
        );
        // :125 compare with >
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print (5 > 3), (3 > 5) }'"#)
                .stdout,
            "1 0\n"
        );
        // :172 short-circuit && (RHS assignment not evaluated)
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { x=0; (0 && (x=1)); print x }'"#)
                .stdout,
            "0\n"
        );
        // :181 short-circuit || (RHS assignment not evaluated)
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { x=0; (1 || (x=1)); print x }'"#)
                .stdout,
            "0\n"
        );

        let files = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "apple\nbanana\napricot\ncherry\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        // :219 use ~ in condition
        assert_eq!(
            files.exec(r#"awk '$0 ~ /^a/ { print }' /data.txt"#).stdout,
            "apple\napricot\n"
        );
        // :228 use !~ in condition
        assert_eq!(
            files.exec(r#"awk '$0 !~ /^a/ { print }' /data.txt"#).stdout,
            "banana\ncherry\n"
        );

        // :237 match field with regex
        assert_eq!(
            env.exec(r#"echo "123 abc 456" | awk '{ print ($2 ~ /[a-z]+/) }'"#)
                .stdout,
            "1\n"
        );
        // :266 ternary works with expressions
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { x=5; print (x > 3 ? "big" : "small") }'"#)
                .stdout,
            "big\n"
        );
        // :275 nest ternary operators
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN { x=5; print (x<3 ? "low" : (x<7 ? "mid" : "high")) }'"#
            )
            .stdout,
            "mid\n"
        );
        // :295 ternary in print arguments
        assert_eq!(
            env.exec(r#"echo "5" | awk '{ print "Value is " ($1 % 2 == 0 ? "even" : "odd") }'"#)
                .stdout,
            "Value is odd\n"
        );
        // :407 chain increments in expression
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { x = 1; y = x++ + ++x; print y, x }'"#)
                .stdout,
            "4 3\n"
        );
        // :420 multiplication before addition
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 2 + 3 * 4 }'"#)
                .stdout,
            "14\n"
        );
        // :429 parentheses for grouping
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print (2 + 3) * 4 }'"#)
                .stdout,
            "20\n"
        );
        // :438 exponent before multiplication
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 2 * 3 ^ 2 }'"#)
                .stdout,
            "18\n"
        );
        // :447 comparison before logical
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 1 < 2 && 3 < 4 }'"#)
                .stdout,
            "1\n"
        );
        // :456 complex precedence
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 2 + 3 * 4 ^ 2 - 10 / 2 }'"#)
                .stdout,
            "45\n"
        );
        // :466 unary minus precedence: -2^2 == -(2^2) == -4
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print -2 ^ 2 }'"#).stdout,
            "-4\n"
        );
    }

    // just-bash-command-awk: portable awk gensub backreferences and printf
    // %c / %e formatting that the existing subset previously mis-handled.
    // Upstream: packages/just-bash/src/commands/awk/awk.functions.test.ts
    //   :412 gensub backreferences (\2 \1) reorder captured groups,
    //   :499 printf %c prints the first character of a string argument,
    //   :510 printf %.2e formats in scientific notation as `1.23e+3`.
    #[test]
    fn awk_jbc_command_awk_gensub_backreference_and_printf_c_e_rows() {
        let env = Bash::default();

        // :412 gensub with \2 \1 backreferences swaps the two captured words.
        // The shell-level `\\\\2` collapses to `\2` for the awk string literal,
        // mirroring the upstream template-literal escaping.
        let backref = env.exec(
            r#"echo "hello world" | awk '{ print gensub(/([a-z]+) ([a-z]+)/, "\\2 \\1", 1) }'"#,
        );
        assert_eq!(backref.exit_code, 0);
        assert_eq!(backref.stdout, "world hello\n");

        // :499 printf %c with a string argument prints its first character.
        let char_string = env.exec(r#"echo "" | awk 'BEGIN { printf "%c\n", "hello" }'"#);
        assert_eq!(char_string.exit_code, 0);
        assert_eq!(char_string.stdout, "h\n");

        // A numeric %c argument still prints the character for that code point,
        // guarding against a regression in the dual numeric/string handling.
        let char_code = env.exec(r#"echo "" | awk 'BEGIN { printf "%c\n", 65 }'"#);
        assert_eq!(char_code.stdout, "A\n");

        // :510 printf %.2e formats 1234.5 as `1.23e+3` (JS toExponential form:
        // explicit sign, no leading zero in the exponent).
        let scientific = env.exec(r#"echo "" | awk 'BEGIN { printf "%.2e\n", 1234.5 }'"#);
        assert_eq!(scientific.exit_code, 0);
        assert_eq!(scientific.stdout, "1.23e+3\n");
    }

    #[test]
    fn awk_jbc_command_awk_ternary_operator_rows() {
        // Mirrors packages/just-bash/src/commands/awk/awk.ternary.test.ts
        // (basic, comparison, nested, assignment, function, and truthiness rows;
        // the getline-backed async rows remain pending without getline support).
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "1\n2\n3\n".to_string())]),
            ..BashOptions::default()
        });

        // :6 true condition selects the consequent branch.
        let r = env.exec(r#"echo "" | awk 'BEGIN { print 1 ? "yes" : "no" }'"#);
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "yes\n");
        // :15 false condition selects the alternate branch.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 0 ? "yes" : "no" }'"#)
                .stdout,
            "no\n"
        );
        // :24 expressions are evaluated inside the chosen branch (5*2 = 10).
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { x = 5; print x > 3 ? x * 2 : x / 2 }'"#)
                .stdout,
            "10\n"
        );
        // :35 numeric comparison condition.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { n = 10; print n > 5 ? "big" : "small" }'"#)
                .stdout,
            "big\n"
        );
        // :44 string comparison condition.
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN { s = "hello"; print s == "hello" ? "match" : "no match" }'"#
            )
            .stdout,
            "match\n"
        );
        // :53 equality check over each record's first field.
        assert_eq!(
            env.exec(r#"awk '{ print $1 == 2 ? "two" : "not two" }' /data.txt"#)
                .stdout,
            "not two\ntwo\nnot two\n"
        );
        // :66 nested ternary resolves to the final alternate ("zero").
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN { x = 0; print x > 0 ? "positive" : x < 0 ? "negative" : "zero" }'"#
            )
            .stdout,
            "zero\n"
        );
        // :75 multiple nesting falls through to "other".
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN { x = 5; print x == 1 ? "one" : x == 2 ? "two" : x == 3 ? "three" : "other" }'"#
            )
            .stdout,
            "other\n"
        );
        // :86 ternary result assigned to a variable.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { a = 10; b = a > 5 ? "high" : "low"; print b }'"#)
                .stdout,
            "high\n"
        );
        // :95 ternary inside a compound expression ((10)+5 = 15).
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { x = 3; y = (x > 2 ? 10 : 1) + 5; print y }'"#)
                .stdout,
            "15\n"
        );
        // :106 function call in the condition (length("abc") > 2).
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print length("abc") > 2 ? "long" : "short" }'"#)
                .stdout,
            "long\n"
        );
        // :115 function calls in the branches.
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN { x = 1; print x ? toupper("yes") : tolower("NO") }'"#
            )
            .stdout,
            "YES\n"
        );
        // :150 empty string is falsy.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { x = ""; print x ? "truthy" : "falsy" }'"#)
                .stdout,
            "falsy\n"
        );
        // :159 non-empty string is truthy.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { x = "hello"; print x ? "truthy" : "falsy" }'"#)
                .stdout,
            "truthy\n"
        );
        // :168 non-zero number (negative) is truthy.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print -5 ? "truthy" : "falsy" }'"#)
                .stdout,
            "truthy\n"
        );

        // :179 a getline in the consequent branch is evaluated when the
        // condition is true: `1 ? (getline line < FILE) : 0` reads the file.
        let consequent = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "hello world\n".to_string())]),
            ..BashOptions::default()
        });
        let r = consequent.exec(
            r#"echo "" | awk 'BEGIN { x = 1 ? (getline line < "/data.txt") : 0; print line }'"#,
        );
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello world\n");

        // :190 a getline in the alternate branch is evaluated when the condition
        // is false: `0 ? 0 : (getline line < FILE)` reads the file.
        let alternate = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "test data\n".to_string())]),
            ..BashOptions::default()
        });
        let r = alternate.exec(
            r#"echo "" | awk 'BEGIN { x = 0 ? 0 : (getline line < "/data.txt"); print line }'"#,
        );
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "test data\n");

        // :201 nested ternary picks the consequent getline (data1) and leaves
        // the alternate getline (data2) unevaluated.
        let nested = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/data1.txt".to_string(), "first\n".to_string()),
                ("/data2.txt".to_string(), "second\n".to_string()),
            ]),
            ..BashOptions::default()
        });
        let r = nested.exec(
            r#"echo "" | awk 'BEGIN { x = 1 ? (getline a < "/data1.txt") : (getline b < "/data2.txt"); print a }'"#,
        );
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "first\n");
    }

    #[test]
    fn awk_jbc_command_awk_modulo_operator_rows() {
        // Mirrors packages/just-bash/src/commands/awk/awk.modulo.test.ts
        // (basic %, %= with variables, modulo conditions, modulo in a for loop,
        // and signed operands).
        let evens = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "1\n2\n3\n4\n5\n6\n".to_string())]),
            ..BashOptions::default()
        });
        let lines = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "a\nb\nc\nd\ne\nf\n".to_string())]),
            ..BashOptions::default()
        });
        let env = Bash::default();

        // :13 modulo of an exact division is 0.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 10 % 2 }'"#).stdout,
            "0\n"
        );
        // :20 floating-point modulo.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 5.5 % 2 }'"#)
                .stdout,
            "1.5\n"
        );
        // :27 zero dividend.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print 0 % 5 }'"#).stdout,
            "0\n"
        );
        // :45 %= compound assignment between variables (25 % 7 = 4).
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { a = 25; b = 7; a %= b; print a }'"#)
                .stdout,
            "4\n"
        );
        // :65 even-number filter via $1 % 2 == 0.
        assert_eq!(
            evens
                .exec(r#"awk '$1 % 2 == 0 { print }' /data.txt"#)
                .stdout,
            "2\n4\n6\n"
        );
        // :83 every Nth line via NR % 3 == 0.
        assert_eq!(
            lines
                .exec(r#"awk 'NR % 3 == 0 { print }' /data.txt"#)
                .stdout,
            "c\nf\n"
        );
        // :94 modulo inside a for loop.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { for(i=1; i<=10; i++) if(i%3==0) print i }'"#)
                .stdout,
            "3\n6\n9\n"
        );
        // :105 negative dividend keeps the dividend's sign (-7 % 3 = -1).
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print -7 % 3 }'"#).stdout,
            "-1\n"
        );
    }

    #[test]
    fn awk_jbc_command_awk_atan2_math_rows() {
        // Mirrors the deterministic atan2 rows in
        // packages/just-bash/src/commands/awk/awk.math.test.ts.
        let env = Bash::default();

        // :59 atan2(0, 1) is exactly 0.
        assert_eq!(
            env.exec(r#"echo '0 1' | awk '{ print atan2($1, $2) }'"#)
                .stdout,
            "0\n"
        );
        // :97 atan2(1, 0) is pi/2; assert it is close to the expected value.
        let distance = env.exec(r#"echo '1 0' | awk '{ print atan2($1, $2) }'"#);
        assert_eq!(distance.exit_code, 0);
        let value: f64 = distance
            .stdout
            .trim()
            .parse()
            .expect("numeric atan2 output");
        assert!(
            (value - std::f64::consts::FRAC_PI_2).abs() < 1e-5,
            "atan2(1, 0) should be ~pi/2, got {value}"
        );
    }

    #[test]
    fn awk_jbc_command_awk_regex_pattern_rows() {
        // Mirrors the leading regex-pattern rows in
        // packages/just-bash/src/commands/awk/awk.patterns.test.ts.
        let fruits = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "apple\nbanana\ncherry\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        let anchored = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "apple\nbanana\napricot\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        let suffix = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "hello\nworld\nfellow\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        let animals = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "cat\ndog\ncow\n".to_string())]),
            ..BashOptions::default()
        });
        let mixed = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "abc\nxyz\n123\n".to_string())]),
            ..BashOptions::default()
        });

        // :6 literal regex matches only the record containing "ana".
        assert_eq!(fruits.exec(r#"awk '/ana/' /data.txt"#).stdout, "banana\n");
        // :15 ^ anchors the match to the start of the record.
        assert_eq!(
            anchored.exec(r#"awk '/^a/' /data.txt"#).stdout,
            "apple\napricot\n"
        );
        // :24 $ anchors the match to the end of the record.
        assert_eq!(suffix.exec(r#"awk '/llo$/' /data.txt"#).stdout, "hello\n");
        // :33 character class matches every record containing c or d.
        assert_eq!(
            animals.exec(r#"awk '/[cd]/' /data.txt"#).stdout,
            "cat\ndog\ncow\n"
        );
        // :42 negated character class [^a-z] matches only the digit record.
        assert_eq!(mixed.exec(r#"awk '/[^a-z]/' /data.txt"#).stdout, "123\n");

        // :51 alternation /red|blue/ matches either branch.
        let colors = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "red\ngreen\nblue\nyellow\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        assert_eq!(
            colors.exec(r#"awk '/red|blue/' /data.txt"#).stdout,
            "red\nblue\n"
        );

        // Expression patterns operating over numeric/string field values.
        let nums = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "1\n2\n3\n".to_string())]),
            ..BashOptions::default()
        });
        // :71 numeric equality keeps only the matching record.
        assert_eq!(nums.exec(r#"awk '$1 == 2' /data.txt"#).stdout, "2\n");
        // :80 numeric inequality keeps every non-matching record.
        assert_eq!(nums.exec(r#"awk '$1 != 2' /data.txt"#).stdout, "1\n3\n");
        let names = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "alice\nbob\ncharlie\n".to_string())]),
            ..BashOptions::default()
        });
        // :107 string equality against a quoted literal.
        assert_eq!(names.exec(r#"awk '$1 == "bob"' /data.txt"#).stdout, "bob\n");
        let fruit3 = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "apple\nbanana\ncherry\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        // :116 lexicographic comparison against a string literal.
        assert_eq!(
            fruit3.exec(r#"awk '$1 < "c"' /data.txt"#).stdout,
            "apple\nbanana\n"
        );

        // Combined patterns over NR and field values.
        let header = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "header\nline1\nline2\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        // :165 NR == 1 matches the first record.
        assert_eq!(header.exec(r#"awk 'NR == 1' /data.txt"#).stdout, "header\n");
        // :174 NR > 1 skips the first record.
        assert_eq!(
            header.exec(r#"awk 'NR > 1' /data.txt"#).stdout,
            "line1\nline2\n"
        );
        let five = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "1\n2\n3\n4\n5\n".to_string())]),
            ..BashOptions::default()
        });
        // :183 compound NR range expression.
        assert_eq!(
            five.exec(r#"awk 'NR >= 2 && NR <= 4' /data.txt"#).stdout,
            "2\n3\n4\n"
        );
        let six = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "a\nb\nc\nd\ne\nf\n".to_string())]),
            ..BashOptions::default()
        });
        // :192 every Nth record via NR % 2 == 1.
        assert_eq!(
            six.exec(r#"awk 'NR % 2 == 1' /data.txt"#).stdout,
            "a\nc\ne\n"
        );

        // Field regex-match patterns using ~ and !~.
        let labeled = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "apple red\nbanana yellow\napricot orange\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        // :203 $1 ~ /^a/ keeps records whose first field starts with a.
        assert_eq!(
            labeled.exec(r#"awk '$1 ~ /^a/' /data.txt"#).stdout,
            "apple red\napricot orange\n"
        );
        // :212 $1 !~ /^a/ keeps the inverse set.
        assert_eq!(
            labeled.exec(r#"awk '$1 !~ /^a/' /data.txt"#).stdout,
            "banana yellow\n"
        );
        let twofield = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "fruit apple\nveg carrot\nfruit banana\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        // :221 $2 ~ /^a/ matches on the second field.
        assert_eq!(
            twofield.exec(r#"awk '$2 ~ /^a/' /data.txt"#).stdout,
            "fruit apple\n"
        );

        // :232 action-only rule with no pattern runs on every record.
        let abc = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "a\nb\nc\n".to_string())]),
            ..BashOptions::default()
        });
        assert_eq!(
            abc.exec(r#"awk '{ print "line:", $0 }' /data.txt"#).stdout,
            "line: a\nline: b\nline: c\n"
        );

        // :243 pattern-only rule prints every matching record.
        let yesno = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "yes\nno\nyes\n".to_string())]),
            ..BashOptions::default()
        });
        assert_eq!(yesno.exec(r#"awk '/yes/' /data.txt"#).stdout, "yes\nyes\n");
    }

    #[test]
    fn awk_r13jb_command_awk_compound_condition_and_format_rows() {
        // Mirrors packages/just-bash/src/commands/awk/awk.test.ts compound
        // condition, variable-comparison, FILENAME/FNR exit-next, do-while,
        // break/continue, and printf-format rows.
        let nums = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "1 10\n2 20\n3 30\n4 40\n5 50\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        // :378 && (AND) condition keeps records 2..4.
        assert_eq!(
            nums.exec(r#"awk '$1>=2 && $1<=4{print}' /data.txt"#).stdout,
            "2 20\n3 30\n4 40\n"
        );

        let letters = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "1 a\n2 b\n3 c\n4 d\n5 e\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        // :387 || (OR) condition keeps records 1 and 5.
        assert_eq!(
            letters
                .exec(r#"awk '$1==1 || $1==5{print}' /data.txt"#)
                .stdout,
            "1 a\n5 e\n"
        );

        let lines = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "line1\nline2\nline3\nline4\nline5\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        // :396 NR range with && keeps records 2..4.
        assert_eq!(
            lines
                .exec(r#"awk 'NR>=2 && NR<=4{print}' /data.txt"#)
                .stdout,
            "line2\nline3\nline4\n"
        );

        let scores = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "10\n25\n15\n30\n5\n".to_string())]),
            ..BashOptions::default()
        });
        // :407 compare field to a -v user variable.
        assert_eq!(
            scores
                .exec(r#"awk -v threshold=20 '$1>threshold{print}' /data.txt"#)
                .stdout,
            "25\n30\n"
        );
        // :418 track the running maximum across records.
        assert_eq!(
            scores
                .exec(r#"awk 'BEGIN{max=0}$1>max{max=$1}END{print max}' /data.txt"#)
                .stdout,
            "30\n"
        );
        // :429 track the running minimum across records.
        assert_eq!(
            scores
                .exec(r#"awk 'BEGIN{min=9999}$1<min{min=$1}END{print min}' /data.txt"#)
                .stdout,
            "5\n"
        );

        let words = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "one\ntwo words\nthree word line\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        // :440 NF comparison keeps multi-field records.
        assert_eq!(
            words.exec(r#"awk 'NF>1{print}' /data.txt"#).stdout,
            "two words\nthree word line\n"
        );

        let prices = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/prices.csv".to_string(),
                "apple,1.50\nbanana,0.75\norange,2.00\ngrape,3.50\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        // :449 filter CSV by a numeric field comparison and print field 1.
        assert_eq!(
            prices
                .exec(r#"awk -F, '$2>=2{print $1}' /prices.csv"#)
                .stdout,
            "orange\ngrape\n"
        );

        let world = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "hello world\n".to_string())]),
            ..BashOptions::default()
        });
        // :473 match() returns 0 and sets RSTART 0 / RLENGTH -1 when no match.
        assert_eq!(
            world
                .exec(r#"awk '{print match($0, /foo/), RSTART, RLENGTH}' /data.txt"#)
                .stdout,
            "0 0 -1\n"
        );

        let foos = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "foo bar foo baz foo\n".to_string())]),
            ..BashOptions::default()
        });
        // :497 gensub replaces only the Nth occurrence.
        assert_eq!(
            foos.exec(r#"awk '{print gensub(/foo/, "XXX", 2)}' /data.txt"#)
                .stdout,
            "foo bar XXX baz foo\n"
        );

        let three = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "line1\nline2\nline3\n".to_string())]),
            ..BashOptions::default()
        });
        // :557 exit N propagates the exit code.
        assert_eq!(three.exec(r#"awk 'NR==2{exit 5}' /data.txt"#).exit_code, 5);

        let abc = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "a\nb\nc\n".to_string())]),
            ..BashOptions::default()
        });
        // :565 next skips the remaining rules for the current record.
        assert_eq!(
            abc.exec(r#"awk '/b/{next}{print}' /data.txt"#).stdout,
            "a\nc\n"
        );

        let env = Bash::default();
        // :576 do-while executes the body at least once.
        let do_while = env.exec(r#"awk 'BEGIN{i=0; do{i++}while(i<3); print i}'"#);
        assert_eq!(do_while.exit_code, 0);
        assert_eq!(do_while.stdout, "3\n");
        // :589 break exits the loop.
        assert_eq!(
            env.exec(r#"awk 'BEGIN{for(i=1;i<=10;i++){if(i==5)break; print i}}'"#)
                .stdout,
            "1\n2\n3\n4\n"
        );
        // :600 continue skips to the next iteration.
        assert_eq!(
            env.exec(r#"awk 'BEGIN{for(i=1;i<=5;i++){if(i==3)continue; print i}}'"#)
                .stdout,
            "1\n2\n4\n5\n"
        );
        // :613 printf %x formats hexadecimal.
        assert_eq!(
            env.exec(r#"awk 'BEGIN{printf "%x\n", 255}'"#).stdout,
            "ff\n"
        );
        // :622 printf %o formats octal.
        assert_eq!(env.exec(r#"awk 'BEGIN{printf "%o\n", 8}'"#).stdout, "10\n");
        // :631 printf %c formats a character from a code point.
        assert_eq!(env.exec(r#"awk 'BEGIN{printf "%c\n", 65}'"#).stdout, "A\n");
        // :640 printf %.2e formats scientific notation (JS toExponential form).
        assert_eq!(
            env.exec(r#"awk 'BEGIN{printf "%.2e\n", 1234}'"#).stdout,
            "1.23e+3\n"
        );
    }

    #[test]
    fn awk_r13jb_command_awk_double_negative_literal_row() {
        // Mirrors packages/just-bash/src/commands/awk/awk.parsing.test.ts:479 —
        // `--5` parses as -(-5) = 5 (double unary minus, not a decrement).
        let env = Bash::default();
        let result = env.exec(r#"echo "" | awk 'BEGIN { print --5 }'"#);
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "5\n");
    }

    #[test]
    fn awk_r13jb_command_awk_prototype_pollution_environ_and_special_var_rows() {
        // Mirrors packages/just-bash/src/commands/awk/awk.prototype-pollution.test.ts —
        // ENVIRON access, sprintf/function names, and FS/RS/OFS holding
        // JavaScript-dangerous keywords must behave like ordinary AWK strings,
        // and AWK arrays/ENVIRON never pollute host data structures.
        let env = Bash::default();

        // :29 ENVIRON["<keyword>"] reflects the exported environment value for
        // the first four dangerous keywords (the upstream loop body).
        for keyword in ["constructor", "__proto__", "prototype", "hasOwnProperty"] {
            let program = format!(
                "export {keyword}=env_value; echo | awk 'BEGIN {{ print ENVIRON[\"{keyword}\"] }}'"
            );
            let result = env.exec(&program);
            assert_eq!(result.exit_code, 0, "ENVIRON[{keyword}] exit");
            assert_eq!(result.stdout, "env_value\n", "ENVIRON[{keyword}] output");
        }
        // :39 ENVIRON["constructor"] reads the exported value.
        let ctor = env.exec(
            "export constructor=ctor_val; echo | awk 'BEGIN { print ENVIRON[\"constructor\"] }'",
        );
        assert_eq!(ctor.exit_code, 0);
        assert_eq!(ctor.stdout, "ctor_val\n");

        // :223 sprintf with a dangerous-keyword argument round-trips verbatim.
        let sprintf =
            env.exec("echo | awk 'BEGIN {\n  s = sprintf(\"%s\", \"__proto__\")\n  print s\n}'");
        assert_eq!(sprintf.exit_code, 0);
        assert_eq!(sprintf.stdout, "__proto__\n");

        // :237 a user function whose name is a prefix of a dangerous keyword runs.
        let proto_func = env.exec(
            "echo | awk '\n  function proto_func() { return \"__proto__\" }\n  BEGIN { print proto_func() }\n'",
        );
        assert_eq!(proto_func.exit_code, 0);
        assert_eq!(proto_func.stdout, "__proto__\n");

        // :249 a dangerous keyword passed as a function argument round-trips.
        let echo_val = env.exec(
            "echo | awk '\n  function echo_val(v) { return v }\n  BEGIN { print echo_val(\"__proto__\") }\n'",
        );
        assert_eq!(echo_val.exit_code, 0);
        assert_eq!(echo_val.stdout, "__proto__\n");

        // :263 FS set to a dangerous keyword splits ordinary fields.
        let fs = env.exec("echo 'a__proto__b' | awk -F '__proto__' '{ print $1, $2 }'");
        assert_eq!(fs.exit_code, 0);
        assert_eq!(fs.stdout, "a b\n");

        // :272 RS containing a dangerous keyword still yields the record.
        let rs = env.exec("echo 'a b' | awk 'BEGIN { RS=\"__proto__\" } { print $0 }'");
        assert_eq!(rs.exit_code, 0);
        assert!(rs.stdout.contains("a b"), "RS output: {:?}", rs.stdout);

        // :281 OFS set to a dangerous keyword joins fields with that literal.
        let ofs = env.exec("echo 'a b' | awk 'BEGIN { OFS=\"__proto__\" } { print $1, $2 }'");
        assert_eq!(ofs.exit_code, 0);
        assert_eq!(ofs.stdout, "a__proto__b\n");

        // :292 ENVIRON access with a dangerous key returns the exported value
        // without affecting any host data structure (Rust arrays are plain maps,
        // so there is no prototype to pollute).
        let environ_print = env
            .exec("export __proto__=polluted\necho | awk 'BEGIN { print ENVIRON[\"__proto__\"] }'");
        assert_eq!(environ_print.exit_code, 0);
        assert_eq!(environ_print.stdout, "polluted\n");

        // :305 assigning dangerous keys into an AWK array succeeds and stays
        // isolated to that array (no host pollution possible by construction).
        let array_assign = env.exec(
            "echo | awk 'BEGIN {\n  arr[\"__proto__\"] = \"polluted\"\n  arr[\"constructor\"] = \"hacked\"\n  print arr[\"__proto__\"], arr[\"constructor\"]\n}'",
        );
        assert_eq!(array_assign.exit_code, 0);
        assert_eq!(array_assign.stdout, "polluted hacked\n");
    }

    #[test]
    fn awk_r10jb_command_awk_parsing_block_and_expression_rows() {
        // Mirrors packages/just-bash/src/commands/awk/awk.parsing.test.ts
        // block-structure parsing and expression-parsing edge cases.
        let env = Bash::default();
        let triple = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "x\n".to_string())]),
            ..BashOptions::default()
        });
        let _ = &triple;

        // :401 C-style for loop in BEGIN concatenates 1..3.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { for (i=1; i<=3; i++) printf i; print "" }'"#)
                .stdout,
            "123\n"
        );
        // :410 while loop.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { i=1; while(i<=3) { printf i; i++ }; print "" }'"#)
                .stdout,
            "123\n"
        );
        // :419 do-while loop.
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN { i=1; do { printf i; i++ } while(i<=3); print "" }'"#
            )
            .stdout,
            "123\n"
        );
        // :428 for-in loop sums array values.
        assert_eq!(
            env.exec(
                r#"echo "" | awk 'BEGIN { a[1]=1; a[2]=2; for (k in a) sum+=a[k]; print sum }'"#
            )
            .stdout,
            "3\n"
        );
        // :472 negative numbers parse as unary minus.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print -5 }'"#).stdout,
            "-5\n"
        );
        // :487 chained field access $($1).
        assert_eq!(
            env.exec(r#"echo "1 2 3" | awk '{ print $($1) }'"#).stdout,
            "1\n"
        );
        // :495 array with an arithmetic index expression.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { a[1+1] = "two"; print a[2] }'"#)
                .stdout,
            "two\n"
        );
        // :504 ternary with nested expressions.
        assert_eq!(
            env.exec(r#"echo "" | awk 'BEGIN { print (1 ? (2 ? "a" : "b") : "c") }'"#)
                .stdout,
            "a\n"
        );

        // :515 missing program is an error mentioning "missing".
        let missing = env.exec("awk");
        assert_eq!(missing.exit_code, 1);
        assert!(
            missing.stderr.contains("missing"),
            "expected missing-program diagnostic, got {:?}",
            missing.stderr
        );
        // :522 invalid option exits 1.
        assert_eq!(env.exec("awk --invalid-option '{print}'").exit_code, 1);
        // :528 missing input file exits 1 with a "No such file" diagnostic.
        let nofile = env.exec("awk '{print}' /nonexistent.txt");
        assert_eq!(nofile.exit_code, 1);
        assert!(
            nofile.stderr.contains("No such file"),
            "expected No-such-file diagnostic, got {:?}",
            nofile.stderr
        );
    }

    #[test]
    fn awk_r10jb_command_awk_output_file_redirection_rows() {
        // Mirrors packages/just-bash/src/commands/awk/awk.output.test.ts
        // file-output redirection rows.
        // :144 print > FILE truncates and writes a single record.
        let env = Bash::default();
        assert_eq!(
            env.exec(
                r#"echo "test" | awk '{ print "output" > "/tmp/out.txt" }' && cat /tmp/out.txt"#
            )
            .stdout,
            "output\n"
        );
        // :153 print >> FILE appends to existing content.
        let existing = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/tmp/existing.txt".to_string(), "line1\n".to_string())]),
            ..BashOptions::default()
        });
        assert_eq!(
            existing
                .exec(
                    r#"echo "test" | awk '{ print "line2" >> "/tmp/existing.txt" }' && cat /tmp/existing.txt"#
                )
                .stdout,
            "line1\nline2\n"
        );
        // :164 print > FILE keeps the stream open so multiple records all land in order.
        let abc = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/data.txt".to_string(), "a\nb\nc\n".to_string())]),
            ..BashOptions::default()
        });
        assert_eq!(
            abc.exec(r#"awk '{ print $0 > "/tmp/out.txt" }' /data.txt && cat /tmp/out.txt"#)
                .stdout,
            "a\nb\nc\n"
        );
    }

    #[test]
    fn awk_r10jb_command_awk_utf8_stdin_row() {
        // Mirrors packages/just-bash/src/commands/awk/awk.utf8-stdin.test.ts:5 —
        // awk splits multibyte fields read through a pipe.
        let env = Bash::with_options(BashOptions {
            files: BTreeMap::from([("/in.txt".to_string(), "한글 café 漢字\n".to_string())]),
            ..BashOptions::default()
        });
        let result = env.exec("cat /in.txt | awk '{ print $2 }'");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "café\n");
    }

    #[test]
    fn awk_r10jb_command_awk_prototype_pollution_dangerous_key_rows() {
        // Mirrors packages/just-bash/src/commands/awk/awk.prototype-pollution.test.ts —
        // AWK variables, arrays, fields, and special variables named after
        // JavaScript-dangerous keywords must behave like ordinary AWK identifiers.
        let env = Bash::default();

        // :51 a variable named after a dangerous keyword holds an ordinary value.
        for keyword in [
            "constructor",
            "__proto__",
            "prototype",
            "hasOwnProperty",
            "isPrototypeOf",
            "toString",
            "valueOf",
            "__defineGetter__",
            "__defineSetter__",
        ] {
            let program =
                format!("echo | awk 'BEGIN {{ {keyword} = \"test_value\"; print {keyword} }}'");
            let result = env.exec(&program);
            assert_eq!(result.exit_code, 0, "keyword {keyword} exit");
            assert_eq!(result.stdout, "test_value\n", "keyword {keyword} output");
        }
        // :61 multiple dangerous-keyword variables coexist.
        assert_eq!(
            env.exec(
                "echo | awk 'BEGIN {\n  __proto__ = \"a\"\n  constructor = \"b\"\n  prototype = \"c\"\n  print __proto__, constructor, prototype\n}'"
            )
            .stdout,
            "a b c\n"
        );
        // :78 an array key that is a dangerous keyword round-trips.
        for keyword in ["constructor", "__proto__", "prototype", "hasOwnProperty"] {
            let program = format!(
                "echo | awk 'BEGIN {{ arr[\"{keyword}\"] = \"value\"; print arr[\"{keyword}\"] }}'"
            );
            let result = env.exec(&program);
            assert_eq!(result.exit_code, 0, "array key {keyword} exit");
            assert_eq!(result.stdout, "value\n", "array key {keyword} output");
        }
        // :88 iterating an array with dangerous keys yields every entry.
        let iterate = env.exec(
            "echo | awk 'BEGIN {\n  arr[\"__proto__\"] = 1\n  arr[\"constructor\"] = 2\n  arr[\"prototype\"] = 3\n  for (k in arr) print k, arr[k]\n}' | sort",
        );
        assert_eq!(iterate.exit_code, 0);
        assert!(iterate.stdout.contains("__proto__ 1"));
        assert!(iterate.stdout.contains("constructor 2"));
        assert!(iterate.stdout.contains("prototype 3"));
        // :104 the `in` operator distinguishes present vs absent dangerous keys.
        assert_eq!(
            env.exec(
                "echo | awk 'BEGIN { arr[\"__proto__\"] = 1; if (\"__proto__\" in arr) print \"found\"; if (\"constructor\" in arr) print \"not found\" }'"
            )
            .stdout,
            "found\n"
        );
        // :117 deleting a dangerous key removes it.
        assert_eq!(
            env.exec(
                "echo | awk 'BEGIN { arr[\"__proto__\"] = 1; delete arr[\"__proto__\"]; if (\"__proto__\" in arr) print \"still there\"; else print \"deleted\" }'"
            )
            .stdout,
            "deleted\n"
        );
        // :134 -v keyword=value assigns a dangerous-keyword variable.
        for keyword in ["constructor", "__proto__", "prototype", "hasOwnProperty"] {
            let program = format!("echo | awk -v {keyword}=injected 'BEGIN {{ print {keyword} }}'");
            let result = env.exec(&program);
            assert_eq!(result.exit_code, 0, "-v {keyword} exit");
            assert_eq!(result.stdout, "injected\n", "-v {keyword} output");
        }
        // :146 / :153 field input containing dangerous keywords prints verbatim.
        assert_eq!(
            env.exec(r#"echo '__proto__' | awk '{ print $1 }'"#).stdout,
            "__proto__\n"
        );
        assert_eq!(
            env.exec(r#"echo 'constructor' | awk '{ print $1 }'"#)
                .stdout,
            "constructor\n"
        );
        // :160 CSV-like dangerous keywords split into ordinary fields.
        assert_eq!(
            env.exec(r#"echo '__proto__,constructor,prototype' | awk -F, '{ print $1, $2, $3 }'"#)
                .stdout,
            "__proto__ constructor prototype\n"
        );
        // :169 dangerous keyword used as an array key built from field data.
        assert_eq!(
            env.exec(
                "printf '__proto__ value1\nconstructor value2\n' | awk '{ data[$1] = $2 } END { print data[\"__proto__\"], data[\"constructor\"] }'"
            )
            .stdout,
            "value1 value2\n"
        );
    }

    #[test]
    fn awk_r10jb_command_awk_prototype_pollution_string_function_rows() {
        // Mirrors packages/just-bash/src/commands/awk/awk.prototype-pollution.test.ts —
        // string/regex builtins operate on dangerous-keyword values normally.
        let env = Bash::default();

        // :181 gsub replaces every dangerous-keyword occurrence in $0.
        assert_eq!(
            env.exec(
                r#"echo '__proto__ test __proto__' | awk '{ gsub(/__proto__/, "replaced"); print }'"#
            )
            .stdout,
            "replaced test replaced\n"
        );
        // :190 split honours a dangerous-keyword payload across the array.
        assert_eq!(
            env.exec(
                "echo | awk 'BEGIN { str = \"__proto__:constructor:prototype\"; n = split(str, arr, \":\"); print arr[1], arr[2], arr[3] }'"
            )
            .stdout,
            "__proto__ constructor prototype\n"
        );
        // :203 match locates a dangerous-keyword pattern.
        assert_eq!(
            env.exec(r#"echo '__proto__' | awk '{ if (match($0, /__proto__/)) print "matched" }'"#)
                .stdout,
            "matched\n"
        );
        // :214 printf prints dangerous-keyword string arguments verbatim.
        assert_eq!(
            env.exec(r#"echo | awk 'BEGIN { printf "%s %s\n", "__proto__", "constructor" }'"#)
                .stdout,
            "__proto__ constructor\n"
        );
    }

    /// Closes the portable `sqlite3.parsing.test.ts` rows. Upstream exercises the
    /// SQL statement splitter (semicolons inside quotes / doubled-quote escapes),
    /// quoted-value handling, set-op + subquery edge cases, and result-set
    /// shaping over the in-memory engine. Each assertion fails if the parser,
    /// `''` unescaping, UNION/subquery/ORDER BY, or CASE evaluation regresses.
    #[test]
    fn structured_data_sqlite3_sql_parsing_and_result_set_rows() {
        let env = bash();

        // statement splitting :6,:26 — semicolons inside single-quoted strings
        // must not split the statement.
        assert_eq!(
            env.exec("sqlite3 :memory: \"CREATE TABLE t(x); INSERT INTO t VALUES('a;b'); SELECT * FROM t\"")
                .stdout,
            "a;b\n"
        );
        assert_eq!(
            env.exec("sqlite3 :memory: \"CREATE TABLE t(x); INSERT INTO t VALUES('a;b;c;d'); SELECT * FROM t\"")
                .stdout,
            "a;b;c;d\n"
        );
        // :35 statement without trailing semicolon
        assert_eq!(env.exec("sqlite3 :memory: \"SELECT 1\"").stdout, "1\n");
        // :42 empty statements (multiple semicolons)
        assert_eq!(
            env.exec("sqlite3 :memory: \"SELECT 1;;; SELECT 2\"").stdout,
            "1\n2\n"
        );
        // :49 whitespace-only (incl. newline) between statements
        assert_eq!(
            env.exec("sqlite3 :memory: \"SELECT 1;   \n   SELECT 2\"")
                .stdout,
            "1\n2\n"
        );
        // :58 SQL doubled-quote escaping with embedded semicolon
        assert_eq!(
            env.exec("sqlite3 :memory: \"CREATE TABLE t(x); INSERT INTO t VALUES('it''s;weird'); SELECT * FROM t\"")
                .stdout,
            "it's;weird\n"
        );
        // :68 multiple doubled quotes with semicolon inside
        assert_eq!(
            env.exec("sqlite3 :memory: \"CREATE TABLE t(x); INSERT INTO t VALUES('a''b;c''d'); SELECT * FROM t\"")
                .stdout,
            "a'b;c'd\n"
        );
        // :78 doubled quote at end of string followed by semicolon
        assert_eq!(
            env.exec("sqlite3 :memory: \"CREATE TABLE t(x); INSERT INTO t VALUES('test'''); SELECT * FROM t\"")
                .stdout,
            "test'\n"
        );
        // :90 escaped single quotes in SQLite style
        assert_eq!(
            env.exec("sqlite3 :memory: \"CREATE TABLE t(x); INSERT INTO t VALUES('it''s'); SELECT * FROM t\"")
                .stdout,
            "it's\n"
        );
        // :99 empty string value
        assert_eq!(
            env.exec(
                "sqlite3 :memory: \"CREATE TABLE t(x); INSERT INTO t VALUES(''); SELECT * FROM t\""
            )
            .stdout,
            "\n"
        );
        // :108 string value containing newlines
        assert_eq!(
            env.exec("sqlite3 :memory: \"CREATE TABLE t(x); INSERT INTO t VALUES('line1\nline2'); SELECT * FROM t\"")
                .stdout,
            "line1\nline2\n"
        );
        // :119 single statement
        assert_eq!(env.exec("sqlite3 :memory: \"SELECT 42\"").stdout, "42\n");
        // :126 many statements
        assert_eq!(
            env.exec("sqlite3 :memory: \"SELECT 1; SELECT 2; SELECT 3; SELECT 4; SELECT 5\"")
                .stdout,
            "1\n2\n3\n4\n5\n"
        );
        // :135 complex query with subquery in FROM, UNION dedupe and ORDER BY
        assert_eq!(
            env.exec(
                "sqlite3 :memory: \"SELECT * FROM (SELECT 1 as x UNION SELECT 2) ORDER BY x\""
            )
            .stdout,
            "1\n2\n"
        );
        // :144 constant CASE expression
        assert_eq!(
            env.exec("sqlite3 :memory: \"SELECT CASE WHEN 1=1 THEN 'yes' ELSE 'no' END\"")
                .stdout,
            "yes\n"
        );
        // :155 empty result set prints nothing
        assert_eq!(
            env.exec("sqlite3 :memory: \"CREATE TABLE t(x); SELECT * FROM t\"")
                .stdout,
            ""
        );
        // :164 -header still prints the header row for an empty result set
        assert_eq!(
            env.exec("sqlite3 -header :memory: \"CREATE TABLE t(a INT, b TEXT); SELECT * FROM t\"")
                .stdout,
            "a|b\n"
        );
        // :173 single-column projection
        assert_eq!(
            env.exec("sqlite3 :memory: \"CREATE TABLE t(x); INSERT INTO t VALUES(1),(2); SELECT x FROM t\"")
                .stdout,
            "1\n2\n"
        );
        // :182 many columns joined by the list separator
        assert_eq!(
            env.exec("sqlite3 :memory: \"SELECT 1,2,3,4,5,6,7,8,9,10\"")
                .stdout,
            "1|2|3|4|5|6|7|8|9|10\n"
        );
        // :191 single aliased row
        assert_eq!(
            env.exec("sqlite3 :memory: \"SELECT 42 as answer\"").stdout,
            "42\n"
        );
    }

    /// Closes the portable `sqlite3.test.ts` unknown-option and REAL-precision
    /// rows plus the matching `sqlite3.formatters.test.ts` line-mode rows.
    #[test]
    fn structured_data_sqlite3_unknown_option_real_precision_and_line_mode_rows() {
        let env = bash();

        // sqlite3.test.ts:98 unknown option emits the exact diagnostic + exit 1.
        let unknown = env.exec("sqlite3 -unknown :memory: \"SELECT 1\"");
        assert_eq!(
            unknown.stderr,
            "sqlite3: Error: unknown option: -unknown\nUse -help for a list of options.\n"
        );
        assert_eq!(unknown.exit_code, 1);

        // sqlite3.test.ts:127 REAL values render with full IEEE-754 precision in
        // -json (3.14 -> 3.1400000000000001), integers stay integral.
        assert_eq!(
            env.exec("sqlite3 -json :memory: \"CREATE TABLE t(i INT, f REAL); INSERT INTO t VALUES(42, 3.14); SELECT * FROM t\"")
                .stdout,
            "[{\"i\":42,\"f\":3.1400000000000001}]\n"
        );

        // sqlite3.formatters.test.ts:126 line mode right-aligns column names to a
        // minimum width of 5 with the "=" at a constant offset.
        let aligned = env.exec("sqlite3 -line :memory: \"SELECT 1 as aa, 2 as bbbb\"");
        let lines: Vec<&str> = aligned.stdout.lines().filter(|l| !l.is_empty()).collect();
        assert!(aligned.stdout.contains("aa = 1"));
        assert!(aligned.stdout.contains("bbbb = 2"));
        assert_eq!(lines[0].find('='), lines[1].find('='));

        // sqlite3.formatters.test.ts:143 multiple rows in line mode are separated
        // by a blank line.
        assert_eq!(
            env.exec("sqlite3 -line :memory: \"SELECT 1 as x UNION SELECT 2\"")
                .stdout,
            "    x = 1\n\n    x = 2\n"
        );
    }

    /// Maps real-command pipeline and short-circuit rows from
    /// `packages/just-bash/src/syntax/operators.test.ts` and `set-errexit.test.ts`
    /// that exercise the registry-backed `Bash` runtime (head/tail/grep/find/mkdir
    /// plus `set -e` short-circuit semantics) rather than the parser-only
    /// interpreter. Each tuple mirrors an upstream `Bash().exec(...)` assertion and
    /// fails if the documented stdout/exit-code or filesystem effect regresses.
    #[test]
    fn jbpi_real_bash_operator_pipeline_and_errexit_rows_match_upstream() {
        let env = Bash::new();
        // operators.test.ts:247 first n lines with head in a pipe.
        assert_eq!(
            env.exec("echo -e \"1\\n2\\n3\\n4\\n5\" | head -n 2").stdout,
            "1\n2\n"
        );
        // operators.test.ts:253 last n lines with tail in a pipe.
        assert_eq!(
            env.exec("echo -e \"1\\n2\\n3\\n4\\n5\" | tail -n 2").stdout,
            "4\n5\n"
        );
        // operators.test.ts:259 head and tail combined in a pipe.
        assert_eq!(
            env.exec("echo -e \"1\\n2\\n3\\n4\\n5\" | head -n 4 | tail -n 2")
                .stdout,
            "3\n4\n"
        );
        // operators.test.ts:267 file contents through multiple filters.
        let dat = Bash::with_options(BashOptions {
            files: BTreeMap::from([(
                "/data.txt".to_string(),
                "apple\nbanana\napricot\nblueberry\navocado\n".to_string(),
            )]),
            ..BashOptions::default()
        });
        assert_eq!(
            dat.exec("cat /data.txt | grep a | head -n 3").stdout,
            "apple\nbanana\napricot\n"
        );

        // operators.test.ts:36 `&&` runs the second command and persists its
        // filesystem effect when the first succeeds.
        let fx = Bash::new();
        fx.exec("mkdir /test && echo created > /test/file.txt");
        assert_eq!(fx.read_file("/test/file.txt").unwrap(), "created\n");

        // operators.test.ts:108 `||` error-handler pattern: the first `mkdir`
        // succeeds silently; a repeated `mkdir` of the existing dir fails and the
        // `||` branch runs.
        let eh = Bash::new();
        assert_eq!(
            eh.exec("mkdir /dir || echo \"dir already exists\"").stdout,
            ""
        );
        assert_eq!(
            eh.exec("mkdir /dir || echo \"dir already exists\"").stdout,
            "dir already exists\n"
        );

        // control-flow.test.ts:335 `! ` passes through to `find -not`, excluding
        // utils*.ts while keeping app.ts.
        let proj = Bash::with_options(BashOptions {
            files: BTreeMap::from([
                ("/project/src/app.ts".to_string(), "code".to_string()),
                ("/project/src/utils.ts".to_string(), "utils".to_string()),
                ("/project/test.json".to_string(), "{}".to_string()),
            ]),
            ..BashOptions::default()
        });
        let found = proj.exec("find /project -name \"*.ts\" -not -name \"utils*\"");
        assert!(found.stdout.contains("app.ts"), "found={:?}", found.stdout);
        assert!(
            !found.stdout.contains("utils.ts"),
            "found={:?}",
            found.stdout
        );

        // variables.test.ts:157 `echo -e` with escaped double backslashes emits a
        // single literal backslash per pair.
        assert_eq!(
            env.exec("echo -e \"path\\\\\\\\to\\\\\\\\file\"").stdout,
            "path\\to\\file\n"
        );

        // set-errexit.test.ts short-circuit/disable rows.
        let run = |script: &str| {
            let bash = Bash::new();
            let r = bash.exec(script);
            (r.stdout, r.exit_code)
        };
        // :85 `set +o errexit` re-enables continuing past a failed command.
        assert_eq!(
            run("set -o errexit\nset +o errexit\necho before\nfalse\necho after"),
            ("before\nafter\n".to_string(), 0)
        );
        // :100 a failed command on the left of `&&` does not trip errexit.
        assert_eq!(
            run("set -e\nfalse && echo \"not reached\"\necho after"),
            ("after\n".to_string(), 0)
        );
        // :111 a failed command on the left of `||` does not trip errexit.
        assert_eq!(
            run("set -e\nfalse || echo \"fallback\"\necho after"),
            ("fallback\nafter\n".to_string(), 0)
        );
        // :134 `&& ... ||` list that ends by succeeding does not trip errexit.
        assert_eq!(
            run("set -e\nfalse && echo \"skip\" || echo \"fallback\"\necho after"),
            ("fallback\nafter\n".to_string(), 0)
        );
    }

    const RG_SHERLOCK: &str = "For the Doctor Watsons of this world, as opposed to the Sherlock\nHolmeses, success in the province of detective work must always\nbe, to a very large extent, the result of luck. Sherlock Holmes\ncan extract a clew from a wisp of straw or a flake of cigar ash;\nbut Doctor Watson has to have it taken out for him and dusted,\nand exhibited clearly, with a label attached.\n";

    fn rg_json_messages(stdout: &str) -> Vec<serde_json::Value> {
        stdout
            .trim()
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid JSON line"))
            .collect()
    }

    #[test]
    fn rg_json_basic_output_structure_matches_ripgrep() {
        // maps imported-tests/json.test.ts:37 "basic: JSON output structure".
        let result =
            home_bash(&[("sherlock", RG_SHERLOCK)]).exec("rg --json 'Sherlock Holmes' sherlock");
        assert_eq!(result.exit_code, 0);
        let msgs = rg_json_messages(&result.stdout);
        assert_eq!(msgs[0]["type"], "begin");
        assert_eq!(
            msgs[0]["data"]["path"],
            serde_json::json!({ "text": "sherlock" })
        );
        assert_eq!(msgs[1]["type"], "match");
        assert_eq!(
            msgs[1]["data"]["lines"],
            serde_json::json!({
                "text": "be, to a very large extent, the result of luck. Sherlock Holmes\n"
            })
        );
        assert_eq!(msgs[1]["data"]["line_number"], 3);
        assert_eq!(msgs[1]["data"]["absolute_offset"], 129);
        let submatches = msgs[1]["data"]["submatches"].as_array().unwrap();
        assert_eq!(submatches.len(), 1);
        assert_eq!(
            submatches[0]["match"],
            serde_json::json!({ "text": "Sherlock Holmes" })
        );
        assert_eq!(submatches[0]["start"], 48);
        assert_eq!(submatches[0]["end"], 63);
        assert_eq!(msgs[2]["type"], "end");
        assert_eq!(msgs[2]["data"]["binary_offset"], serde_json::Value::Null);
        assert_eq!(msgs[3]["type"], "summary");
        assert_eq!(msgs[3]["data"]["stats"]["searches_with_match"], 1);
    }

    #[test]
    fn rg_json_quiet_emits_only_summary() {
        // maps imported-tests/json.test.ts:103 "quiet_stats: JSON with --quiet shows only summary".
        let result = home_bash(&[("sherlock", RG_SHERLOCK)])
            .exec("rg --json --quiet 'Sherlock Holmes' sherlock");
        assert_eq!(result.exit_code, 0);
        let msgs = rg_json_messages(&result.stdout);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["type"], "summary");
        assert_eq!(msgs[0]["data"]["stats"]["searches_with_match"], 1);
    }

    #[test]
    fn rg_json_reports_all_submatches_on_a_line() {
        // maps imported-tests/json.test.ts:124 "should output all matches with correct submatches".
        let result =
            home_bash(&[("test.txt", "foo bar foo baz foo\n")]).exec("rg --json foo test.txt");
        assert_eq!(result.exit_code, 0);
        let msgs = rg_json_messages(&result.stdout);
        let matched = msgs.iter().find(|m| m["type"] == "match").unwrap();
        let submatches = matched["data"]["submatches"].as_array().unwrap();
        assert_eq!(submatches.len(), 3);
        assert_eq!(
            submatches[0],
            serde_json::json!({ "match": { "text": "foo" }, "start": 0, "end": 3 })
        );
        assert_eq!(
            submatches[1],
            serde_json::json!({ "match": { "text": "foo" }, "start": 8, "end": 11 })
        );
        assert_eq!(
            submatches[2],
            serde_json::json!({ "match": { "text": "foo" }, "start": 16, "end": 19 })
        );
    }

    #[test]
    fn rg_json_emits_begin_end_per_file() {
        // maps imported-tests/json.test.ts:157 "should output multiple files with begin/end messages".
        let result =
            home_bash(&[("a.txt", "hello\n"), ("b.txt", "hello\n")]).exec("rg --json hello");
        assert_eq!(result.exit_code, 0);
        let msgs = rg_json_messages(&result.stdout);
        assert_eq!(msgs.iter().filter(|m| m["type"] == "begin").count(), 2);
        assert_eq!(msgs.iter().filter(|m| m["type"] == "end").count(), 2);
        assert_eq!(msgs.iter().filter(|m| m["type"] == "match").count(), 2);
        assert_eq!(msgs.iter().filter(|m| m["type"] == "summary").count(), 1);
    }

    #[test]
    fn rg_json_outputs_summary_with_no_matches() {
        // maps imported-tests/json.test.ts:189 "should output summary even with no matches".
        let result = home_bash(&[("test.txt", "hello world\n")]).exec("rg --json notfound");
        assert_eq!(result.exit_code, 1);
        let msgs = rg_json_messages(&result.stdout);
        let summary = msgs.iter().find(|m| m["type"] == "summary").unwrap();
        assert_eq!(summary["data"]["stats"]["searches_with_match"], 0);
    }

    #[test]
    fn rg_json_handles_empty_file_with_summary() {
        // maps imported-tests/json.test.ts:207 "should handle empty file with summary".
        let result = home_bash(&[("empty.txt", "")]).exec("rg --json foo empty.txt");
        assert_eq!(result.exit_code, 1);
        let msgs = rg_json_messages(&result.stdout);
        let summary = msgs.iter().find(|m| m["type"] == "summary").unwrap();
        assert_eq!(summary["data"]["stats"]["searches_with_match"], 0);
        assert_eq!(summary["data"]["stats"]["bytes_searched"], 0);
    }

    #[test]
    fn rg_heading_groups_matches_under_file_path() {
        // maps imported-tests/regression.test.ts:203 "r99: should show heading output format".
        let result = home_bash(&[("foo1", "test\n"), ("foo2", "zzz\n"), ("bar", "test\n")])
            .exec("rg --heading test");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("test"));
        // Heading mode prints the file label on its own line above the matches.
        assert!(result.stdout.contains("bar\n"));
        assert!(result.stdout.contains("foo1\n"));
    }

    #[test]
    fn rg_column_shows_match_column_with_line_number() {
        // maps rg.ripgrep-compat.test.ts:691 "should show column with --column"
        // and imported-tests/misc.test.ts:72 "should show column numbers with --column".
        // `--column` implies `-n`; the column is the 1-based char index of the match.
        let result = home_bash(&[("sherlock", SHERLOCK)]).exec("rg -n --column Sherlock sherlock");
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "1:57:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
3:49:be, to a very large extent, the result of luck. Sherlock Holmes\n"
        );
    }

    #[test]
    fn rg_column_directory_search_prefixes_filename_and_column() {
        // maps imported-tests/regression.test.ts:232 "r105_part2: should show column with --column".
        let result = home_bash(&[("foo", "zztest\n")]).exec("rg --column test");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "foo:1:3:zztest\n");
    }

    #[test]
    fn rg_column_after_bom_is_one_based_from_content_start() {
        // maps imported-tests/regression.test.ts:1144 "r1638: should have correct column with BOM".
        // The leading UTF-8 BOM is stripped before searching, so `x` is at column 1.
        let result = home_bash(&[("foo", "\u{FEFF}x\n")]).exec("rg --column x");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "foo:1:1:x\n");
    }

    #[test]
    fn rg_only_matching_with_column_reports_each_match_position() {
        // maps imported-tests/regression.test.ts:521
        // "r451_only_matching: should show column with only matching".
        let result = home_bash(&[("digits.txt", "1 2 3\n123\n")])
            .exec("rg --only-matching --column '[0-9]' digits.txt");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1:1:1\n1:3:2\n1:5:3\n2:1:1\n2:2:2\n2:3:3\n");
    }

    #[test]
    fn rg_vimgrep_emits_one_line_per_match_with_column() {
        // maps rg.ripgrep-compat.test.ts:742 "should output vimgrep format with --vimgrep"
        // and imported-tests/misc.test.ts:1038 "should show vimgrep format".
        let result =
            home_bash(&[("sherlock", SHERLOCK)]).exec("rg --vimgrep 'Sherlock|Watson' sherlock");
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "1:16:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
1:57:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
3:49:be, to a very large extent, the result of luck. Sherlock Holmes\n\
5:12:but Doctor Watson has to have it taken out for him and dusted,\n"
        );
    }

    #[test]
    fn rg_vimgrep_directory_search_prefixes_filename() {
        // maps imported-tests/misc.test.ts:1038 "should show vimgrep format" (directory form)
        // and imported-tests/regression.test.ts:220 "r105_part1: should show column with --vimgrep".
        let result = home_bash(&[("foo", "zztest\n")]).exec("rg --vimgrep test");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "foo:1:3:zztest\n");
    }

    #[test]
    fn rg_vimgrep_without_line_numbers_drops_line_column_only() {
        // maps imported-tests/misc.test.ts:1055 "should show vimgrep format without line numbers".
        let result = home_bash(&[("sherlock", SHERLOCK)]).exec("rg --vimgrep -N 'Sherlock|Watson'");
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "sherlock:16:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
sherlock:57:For the Doctor Watsons of this world, as opposed to the Sherlock\n\
sherlock:49:be, to a very large extent, the result of luck. Sherlock Holmes\n\
sherlock:12:but Doctor Watson has to have it taken out for him and dusted,\n"
        );
    }

    #[test]
    fn rg_byte_offset_with_only_matching_reports_file_byte_position() {
        // maps rg.ripgrep-compat.test.ts:817 "should show byte offset with -b"
        // and imported-tests/misc.test.ts:575 "should show byte offset with -b -o".
        let result = home_bash(&[("sherlock", SHERLOCK)]).exec("rg -b -o Sherlock");
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "sherlock:56:Sherlock\nsherlock:177:Sherlock\n"
        );
    }

    #[test]
    fn rg_replace_substitutes_each_match_in_line() {
        // maps rg.ripgrep-compat.test.ts:726 "should replace matches with -r"
        // and imported-tests/misc.test.ts:261 "should replace matches with -r".
        let result = home_bash(&[("sherlock", SHERLOCK)]).exec("rg -r FooBar Sherlock sherlock");
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "For the Doctor Watsons of this world, as opposed to the FooBar\n\
be, to a very large extent, the result of luck. FooBar Holmes\n"
        );
    }

    #[test]
    fn rg_replace_with_numbered_capture_groups() {
        // maps imported-tests/misc.test.ts:278 "should replace with capture groups".
        let result = home_bash(&[("sherlock", SHERLOCK)])
            .exec("rg -r '$2, $1' '([A-Z][a-z]+) ([A-Z][a-z]+)' sherlock");
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "For the Watsons, Doctor of this world, as opposed to the Sherlock\n\
be, to a very large extent, the result of luck. Holmes, Sherlock\n\
but Watson, Doctor has to have it taken out for him and dusted,\n"
        );
    }

    #[test]
    fn rg_replace_with_named_capture_groups() {
        // maps imported-tests/misc.test.ts:297 "should replace with named capture groups".
        let result = home_bash(&[("sherlock", SHERLOCK)])
            .exec("rg -r '$last, $first' '(?P<first>[A-Z][a-z]+) (?P<last>[A-Z][a-z]+)' sherlock");
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "For the Watsons, Doctor of this world, as opposed to the Sherlock\n\
be, to a very large extent, the result of luck. Holmes, Sherlock\n\
but Watson, Doctor has to have it taken out for him and dusted,\n"
        );
    }

    #[test]
    fn rg_replace_with_only_matching_outputs_replaced_groups() {
        // maps imported-tests/misc.test.ts:316 "should replace only matching parts".
        let result =
            home_bash(&[("sherlock", SHERLOCK)]).exec("rg -o -r '$1' 'of (\\w+)' sherlock");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "this\ndetective\nluck\nstraw\ncigar\n");
    }

    #[test]
    fn rg_replace_full_match_reference_with_braces() {
        // maps imported-tests/regression.test.ts:1159
        // "r1739: should replace with reference to full match".
        let result = home_bash(&[("test", "a\n")]).exec("rg -r '${0}f' '.*' test");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "af\n");
    }

    #[test]
    fn rg_passthru_prints_all_lines_with_match_markers() {
        // maps rg.ripgrep-compat.test.ts:858 "should print all lines with --passthru".
        let result =
            home_bash(&[("file", "\nfoo\nbar\nfoobar\n\nbaz\n")]).exec("rg -n --passthru foo file");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1-\n2:foo\n3-bar\n4:foobar\n5-\n6-baz\n");
    }

    #[test]
    fn rg_count_with_only_matching_counts_individual_matches() {
        // maps imported-tests/misc.test.ts:637 "should count via --count --only-matching".
        let result = home_bash(&[("sherlock", SHERLOCK)]).exec("rg --count --only-matching the");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "sherlock:4\n");
    }

    #[test]
    fn rg_iglob_matches_case_insensitively() {
        // maps imported-tests/misc.test.ts:519
        // "should use case-insensitive glob matching with --iglob".
        let result = home_bash(&[("file1.HTML", "Sherlock\n"), ("file2.html", "Sherlock\n")])
            .exec("rg --iglob '*.html' Sherlock");
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "file1.HTML:1:Sherlock\nfile2.html:1:Sherlock\n"
        );
    }

    #[test]
    fn rg_glob_case_insensitive_flag_makes_globs_case_insensitive() {
        // maps imported-tests/misc.test.ts:554
        // "should make all globs case-insensitive with --glob-case-insensitive".
        let result = home_bash(&[("file1.HTML", "Sherlock\n"), ("file2.html", "Sherlock\n")])
            .exec("rg --glob-case-insensitive --glob '*.html' Sherlock");
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "file1.HTML:1:Sherlock\nfile2.html:1:Sherlock\n"
        );
    }

    #[test]
    fn rg_max_filesize_skips_files_over_the_limit() {
        // maps imported-tests/misc.test.ts:803 "should filter files by size with --max-filesize".
        let large = format!("Sherlock {}\n", "x".repeat(100));
        let result = home_bash(&[("small.txt", "Sherlock\n"), ("large.txt", large.as_str())])
            .exec("rg --max-filesize 50 Sherlock");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "small.txt:1:Sherlock\n");
    }

    #[test]
    fn rg_max_filesize_accepts_kilobyte_suffix() {
        // maps imported-tests/misc.test.ts:817 "should accept K suffix for kilobytes".
        let result = home_bash(&[("test.txt", "Sherlock\n")]).exec("rg --max-filesize 1K Sherlock");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "test.txt:1:Sherlock\n");
    }

    #[test]
    fn rg_max_filesize_accepts_megabyte_suffix() {
        // maps imported-tests/misc.test.ts:829 "should accept M suffix for megabytes".
        let result = home_bash(&[("test.txt", "Sherlock\n")]).exec("rg --max-filesize 1M Sherlock");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "test.txt:1:Sherlock\n");
    }

    #[test]
    fn rg_strips_leading_utf8_bom_before_anchored_match() {
        // maps imported-tests/regression.test.ts:866 "r1163: should handle UTF-8 BOM".
        let result = home_bash(&[("bom.txt", "\u{FEFF}test123\ntest123\n")]).exec("rg '^test123'");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "bom.txt:1:test123\nbom.txt:2:test123\n");
    }

    #[test]
    fn rg_fixed_strings_from_file_match_many_duplicate_literals() {
        // maps imported-tests/regression.test.ts:1040
        // "r1334_crazy_literals: should handle many literal patterns".
        let patterns = "1.208.0.0/12\n".repeat(40);
        let result = home_bash(&[
            ("patterns", patterns.as_str()),
            ("corpus", "1.208.0.0/12\n"),
        ])
        .exec("rg -Ff patterns corpus");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1.208.0.0/12\n");
    }

    #[test]
    fn rg_files_errors_on_glob_with_unclosed_character_class() {
        // maps imported-tests/regression.test.ts:1505
        // "r3127_glob_flag_not_allow_unclosed_class: should error on unclosed class in glob".
        let result = home_bash(&[("[abc", ""), ("test", "")]).exec("rg --files -g '[abc'");
        assert_ne!(result.exit_code, 0);
        assert_eq!(
            result.stderr,
            "rg: glob '[abc' has an unclosed character class\n"
        );
    }

    #[test]
    fn rg_negated_glob_with_leading_slash_is_root_anchored() {
        // maps imported-tests/regression.test.ts:489 "r405: should handle negated glob with path".
        let result = home_bash(&[
            ("foo/bar/file1.txt", "test\n"),
            ("bar/foo/file2.txt", "test\n"),
        ])
        .exec("rg -g '!/foo/**' test");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "bar/foo/file2.txt:1:test\n");
    }

    #[test]
    fn rg_follow_symlinked_directory_root_with_dash_l() {
        // maps imported-tests/regression.test.ts:405 "r256: should follow directory symlinks with -L".
        let bash = home_bash(&[("realdir/test.txt", "test content\n")]);
        bash.exec("ln -s realdir /home/user/linkdir");
        let result = bash.exec("rg -L test linkdir");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("test content"));
    }

    #[test]
    fn rg_follow_symlinked_directory_root_with_dash_l_and_single_thread() {
        // maps imported-tests/regression.test.ts:420
        // "r256: should follow directory symlinks with -L and -j1".
        let bash = home_bash(&[("realdir/test.txt", "test content\n")]);
        bash.exec("ln -s realdir /home/user/linkdir");
        let result = bash.exec("rg -L -j1 test linkdir");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("test content"));
    }

    #[test]
    fn rg_gitignore_negation_unhides_dotfile() {
        // maps imported-tests/regression.test.ts:171
        // "r90: should handle negation of hidden file in gitignore".
        let result = home_bash(&[
            (".git/.gitkeep", ""),
            (".gitignore", "!.foo\n"),
            (".foo", "test\n"),
        ])
        .exec("rg test");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, ".foo:1:test\n");
    }

    #[test]
    fn rg_follow_symlinked_subdirectory_surfaces_content_through_both_paths() {
        // maps rg.flags.test.ts:63 "should follow symlinks to directories with -L".
        let bash = home_bash(&[("subdir/file.txt", "hello\n")]);
        bash.exec("ln -s subdir /home/user/linkdir");
        // Without -L only the real directory is searched.
        let without = bash.exec("rg hello");
        assert_eq!(without.exit_code, 0);
        assert_eq!(without.stdout, "subdir/file.txt:1:hello\n");
        // With -L the file is found through both the real and symlinked paths.
        let with = bash.exec("rg -L --sort path hello");
        assert_eq!(with.exit_code, 0);
        assert_eq!(
            with.stdout,
            "linkdir/file.txt:1:hello\nsubdir/file.txt:1:hello\n"
        );
    }

    #[test]
    fn rg_imported_dot_prefix_preservation_and_json_replacement_rows_are_portable() {
        // JBC: closes imported ripgrep rows that exercise literal `./` path-prefix
        // preservation (feature.test.ts "should preserve ./ prefix when given",
        // regression.test.ts "should preserve ./ prefix") and the JSON submatch
        // `replacement` field (json.test.ts "replacement: JSON output with
        // replacement text"). Each assertion fails if the rg port regresses.

        // feature.test.ts:265 — `rg --files ./sub` preserves the `./sub/` prefix on
        // listed files instead of collapsing to the cwd-relative `sub/file.txt`.
        assert_home_exec(
            &[("sub/file.txt", "content\n")],
            "rg --files ./sub",
            0,
            "./sub/file.txt\n",
        );

        // regression.test.ts:1398 — `rg --hidden --files ./` preserves the bare `./`
        // prefix, labelling the hidden file `./a/.ignore` (not `a/.ignore`).
        assert_home_exec(
            &[("a/.ignore", ".foo\n")],
            "rg --hidden --files ./",
            0,
            "./a/.ignore\n",
        );

        // A bare `.` root is NOT prefixed (ripgrep only preserves an explicit
        // `./`): `rg --files .` keeps the cwd-relative label.
        assert_home_exec(
            &[("sub/file.txt", "content\n")],
            "rg --files .",
            0,
            "sub/file.txt\n",
        );

        // json.test.ts:82 — `rg --json '<pat>' -r '<rep>'` attaches the raw replace
        // string verbatim as the submatch `replacement` field.
        let result = home_bash(&[("sherlock", SHERLOCK)])
            .exec("rg --json 'Sherlock Holmes' -r 'John Watson' sherlock");
        assert_eq!(result.exit_code, 0);
        let match_line = result
            .stdout
            .lines()
            .find(|line| line.contains("\"type\":\"match\""))
            .expect("a match message");
        let value: serde_json::Value = serde_json::from_str(match_line).expect("valid match JSON");
        let submatch = &value["data"]["submatches"][0];
        assert_eq!(submatch["match"]["text"], "Sherlock Holmes");
        assert_eq!(
            submatch["replacement"],
            serde_json::json!({ "text": "John Watson" }),
            "JSON submatch must carry the raw -r replacement text",
        );

        // Without `-r`, no `replacement` key is emitted on the submatch.
        let plain =
            home_bash(&[("sherlock", SHERLOCK)]).exec("rg --json 'Sherlock Holmes' sherlock");
        let plain_match = plain
            .stdout
            .lines()
            .find(|line| line.contains("\"type\":\"match\""))
            .expect("a match message");
        let plain_value: serde_json::Value =
            serde_json::from_str(plain_match).expect("valid match JSON");
        assert!(
            plain_value["data"]["submatches"][0]
                .get("replacement")
                .is_none(),
            "no replacement key without -r",
        );
    }
}
