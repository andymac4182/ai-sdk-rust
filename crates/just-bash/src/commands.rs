//! Portable Just Bash command registry.
//!
//! This layer mirrors the upstream lazy command registry as data while only
//! marking commands implemented by the Rust backend as dispatchable.

use std::collections::BTreeMap;

use crate::security::CommandSecurityPolicy;

/// Upstream file that defines the JavaScript lazy command registry.
pub const UPSTREAM_COMMAND_REGISTRY: &str = "packages/just-bash/src/commands/registry.ts";

/// Upstream default command names from `commands/registry.ts`.
pub const UPSTREAM_DEFAULT_COMMAND_NAMES: &[&str] = &[
    "echo",
    "cat",
    "printf",
    "ls",
    "mkdir",
    "rmdir",
    "touch",
    "rm",
    "cp",
    "mv",
    "ln",
    "chmod",
    "pwd",
    "readlink",
    "head",
    "tail",
    "wc",
    "stat",
    "grep",
    "fgrep",
    "egrep",
    "rg",
    "sed",
    "awk",
    "sort",
    "uniq",
    "comm",
    "cut",
    "paste",
    "tr",
    "rev",
    "nl",
    "fold",
    "expand",
    "unexpand",
    "strings",
    "split",
    "column",
    "join",
    "tee",
    "find",
    "basename",
    "dirname",
    "tree",
    "du",
    "env",
    "printenv",
    "alias",
    "unalias",
    "history",
    "xargs",
    "true",
    "false",
    "clear",
    "bash",
    "sh",
    "jq",
    "base64",
    "diff",
    "date",
    "sleep",
    "timeout",
    "seq",
    "expr",
    "md5sum",
    "sha1sum",
    "sha256sum",
    "file",
    "html-to-markdown",
    "help",
    "which",
    "tac",
    "hostname",
    "od",
    "gzip",
    "gunzip",
    "zcat",
    "tar",
    "yq",
    "xan",
    "sqlite3",
    "time",
    "whoami",
];

const PORTABLE_BUILTINS: &[(&str, Builtin)] = &[
    ("true", Builtin::True),
    ("false", Builtin::False),
    ("exit", Builtin::Exit),
    ("echo", Builtin::Echo),
    ("printf", Builtin::Printf),
    ("pwd", Builtin::Pwd),
    ("cd", Builtin::Cd),
    ("env", Builtin::Env),
    ("printenv", Builtin::Printenv),
    ("export", Builtin::Export),
    ("unset", Builtin::Unset),
    ("read", Builtin::Read),
    ("cat", Builtin::Cat),
    ("ls", Builtin::Ls),
    ("mkdir", Builtin::Mkdir),
    ("rm", Builtin::Rm),
    ("cp", Builtin::Cp),
    ("mv", Builtin::Mv),
    ("touch", Builtin::Touch),
    ("find", Builtin::Find),
    ("grep", Builtin::Grep),
    ("fgrep", Builtin::Fgrep),
    ("egrep", Builtin::Egrep),
    ("rg", Builtin::Rg),
    ("sed", Builtin::Sed),
    ("awk", Builtin::Awk),
    ("head", Builtin::Head),
    ("tail", Builtin::Tail),
    ("wc", Builtin::Wc),
    ("sort", Builtin::Sort),
    ("uniq", Builtin::Uniq),
    ("cut", Builtin::Cut),
    ("tr", Builtin::Tr),
    ("bash", Builtin::Bash),
    ("sh", Builtin::Sh),
    ("jq", Builtin::Jq),
    ("yq", Builtin::Yq),
    ("xan", Builtin::Xan),
    ("sqlite3", Builtin::Sqlite3),
    ("sleep", Builtin::Sleep),
    ("which", Builtin::Which),
    ("whoami", Builtin::Whoami),
];

/// Built-ins implemented by the portable Rust backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Builtin {
    True,
    False,
    Exit,
    Echo,
    Printf,
    Pwd,
    Cd,
    Env,
    Printenv,
    Export,
    Unset,
    Read,
    Cat,
    Ls,
    Mkdir,
    Rm,
    Cp,
    Mv,
    Touch,
    Find,
    Grep,
    Fgrep,
    Egrep,
    Rg,
    Sed,
    Awk,
    Head,
    Tail,
    Wc,
    Sort,
    Uniq,
    Cut,
    Tr,
    Bash,
    Sh,
    Jq,
    Yq,
    Xan,
    Sqlite3,
    Sleep,
    Which,
    Whoami,
}

/// Registry of command names available to a session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandRegistry {
    commands: BTreeMap<String, Builtin>,
    security_policy: CommandSecurityPolicy,
}

impl CommandRegistry {
    /// Creates the default portable registry.
    pub fn default_portable() -> Self {
        let commands = PORTABLE_BUILTINS
            .iter()
            .map(|(name, builtin)| ((*name).to_string(), *builtin))
            .collect::<BTreeMap<_, _>>();
        let security_policy = CommandSecurityPolicy::allow_only(commands.keys().cloned());
        Self {
            commands,
            security_policy,
        }
    }

    /// Creates a registry restricted to the requested command names.
    pub fn filtered(names: &[String]) -> Self {
        let commands = names
            .iter()
            .filter_map(|name| builtin_for(name).map(|builtin| (normalize_name(name), builtin)))
            .collect::<BTreeMap<_, _>>();
        let security_policy = CommandSecurityPolicy::allow_only(commands.keys().cloned());
        Self {
            commands,
            security_policy,
        }
    }

    /// Returns the built-in backing a command name.
    pub fn get(&self, name: &str) -> Option<Builtin> {
        let name = normalize_name(name);
        self.security_policy
            .check_command(&format!("/bin/{name}"))
            .ok()?;
        self.commands.get(&name).copied()
    }

    /// Returns true when the command is available.
    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Returns sorted registered command names.
    pub fn names(&self) -> Vec<String> {
        self.commands.keys().cloned().collect()
    }

    /// Returns the security policy applied by this registry.
    pub const fn security_policy(&self) -> &CommandSecurityPolicy {
        &self.security_policy
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::default_portable()
    }
}

fn builtin_for(name: &str) -> Option<Builtin> {
    let name = normalize_name(name);
    PORTABLE_BUILTINS
        .iter()
        .find_map(|(candidate, builtin)| (*candidate == name).then_some(*builtin))
}

fn normalize_name(name: &str) -> String {
    name.rsplit('/')
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_lowercase()
}
