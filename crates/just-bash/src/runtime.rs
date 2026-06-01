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
