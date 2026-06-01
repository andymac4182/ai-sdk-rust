//! Upstream-style `Bash` facade backed by the landed in-process session.

use std::collections::BTreeMap;

use crate::{
    JustBashExecOptions, JustBashExecResult, JustBashResult, JustBashSession,
    JustBashSessionOptions,
};

/// Construction options for the upstream-style [`Bash`] facade.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BashOptions {
    /// Optional list of portable command names to register.
    pub commands: Option<Vec<String>>,
    /// Initial virtual files.
    pub files: BTreeMap<String, String>,
    /// Base environment.
    pub env: BTreeMap<String, String>,
    /// Base working directory.
    pub cwd: Option<String>,
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
        session_options.files = options.files;
        session_options.env = options.env;
        session_options.cwd = options.cwd;
        session_options.commands = options.commands;
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
        self.session.read_file(path)
    }

    /// Writes a UTF-8 virtual file.
    pub fn write_file(&self, path: &str, content: &str) -> JustBashResult<()> {
        self.session.write_file(path, content)
    }

    /// Returns true when a virtual path exists.
    pub fn file_exists(&self, path: &str) -> bool {
        self.session.file_exists(path)
    }

    /// Returns sorted registered command names.
    pub fn registered_command_names(&self) -> Vec<String> {
        self.session.registered_command_names()
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

    use crate::UPSTREAM_DEFAULT_COMMAND_NAMES;

    fn bash() -> Bash {
        Bash::new()
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
        assert!(env.exec("rg beta /repo").stdout.contains("repo/a.txt:beta"));
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

        assert_eq!(env.exec("jq .name /data.json").stdout, "Ada\n");
    }
}
