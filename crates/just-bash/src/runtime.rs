//! Upstream-style `Bash` facade backed by the landed in-process session.

use std::collections::BTreeMap;

use crate::{
    JustBashExecOptions, JustBashExecResult, JustBashResult, JustBashSession,
    JustBashSessionOptions, path::resolve_path,
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
        session_options.create_default_layout = create_default_layout;
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

    use crate::UPSTREAM_DEFAULT_COMMAND_NAMES;

    fn bash() -> Bash {
        Bash::new()
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
                ("/a.txt".to_string(), "found here\nmatch\n".to_string()),
                ("/b.txt".to_string(), "nothing\nmatch\n".to_string()),
                ("/c.txt".to_string(), "also found\n".to_string()),
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
}
