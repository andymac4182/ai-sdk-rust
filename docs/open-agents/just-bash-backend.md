# Open Agents Just Bash Backend

This adapter note tracks the Open Agents bridge from the `bash` tool sandbox
trait to the shared `crates/just-bash` in-process backend. The default
`just-bash` backend is virtual-only: it does not require Vercel sandbox
credentials and does not spawn host `/bin/bash`, `sh`, package managers,
language runtimes, or arbitrary host processes.

## Source Snapshot

| Field | Value |
| --- | --- |
| Just Bash upstream | `vercel-labs/just-bash` |
| Just Bash fetch command | `npx opensrc fetch https://github.com/vercel-labs/just-bash` |
| Open Agents upstream | `vercel-labs/open-agents` |
| Open Agents fetch command | `npx opensrc fetch https://github.com/vercel-labs/open-agents` |
| Local Just Bash mirror | `/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/just-bash/main` |
| Local Open Agents mirror | `/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/open-agents/main` |
| Rust owner | `crates/open-agents-sandbox/src/just_bash.rs` |
| Backend owner | `crates/just-bash` |
| Status | crate-backed Open Agents adapter |

## Covered Subset

The adapter runs in process with a virtual filesystem and exposes `/workspace`
as the Open Agents working directory. The Open Agents smoke tests cover the
default command path, filesystem persistence, per-exec env/cwd reset, failure
mapping, and no-host-shell behavior.

Open Agents fixture coverage currently exercises:

- `echo`
- `pwd`
- `cat`
- `printf`
- `mkdir`
- `ls`
- `touch`
- `cd`
- `export` and simple `NAME=value` assignment
- `true`
- `false`
- simple `;`, `&&`, `$VAR`/`${VAR}` expansion, pipes, and `>`/`>>` redirection

Unsupported commands return exit code `127` with `command not found`. Detached
execution and snapshots remain unsupported by the Open Agents Just Bash
adapter. Broader command/runtime parity belongs to `crates/just-bash` and is
tracked row-by-row in `docs/open-agents/just-bash-parity.md`.

## Config Surface

`OPEN_AGENTS_SANDBOX=just-bash` is the default safe backend for local and
fixture Open Agents runs. Operators can still explicitly select previous
external backends:

| Backend | Env | Behavior |
| --- | --- | --- |
| Just Bash | `OPEN_AGENTS_SANDBOX=just-bash` | In-process virtual filesystem, no host process execution. |
| Local | `OPEN_AGENTS_SANDBOX=local` | Explicit host local sandbox; may execute host `/bin/bash` inside `OPEN_AGENTS_SANDBOX_ROOT`. |
| Vercel | `OPEN_AGENTS_SANDBOX=vercel` | Explicit live Vercel Sandbox backend. |

`--check-config` prints `sandbox=just-bash` and
`sandbox_working_directory=/workspace` for the default route.

## Ledger Rows

The generated Just Bash inventory in `docs/open-agents/just-bash-parity.md`
is the source of truth for upstream Just Bash row status. Rows are only closed
when they cite named Rust tests. Open Agents-specific routing and service rows
are tracked in `docs/open-agents/upstream-parity.md`.

| Upstream behavior | Rust test | Status | Notes |
| --- | --- | --- | --- |
| Just Bash `Bash` uses a virtual filesystem and restores per-exec cwd/env | `just_bash_resets_env_and_cwd_between_exec_calls` | covered-adapter | Proves Open Agents starts each tool call from the configured virtual cwd/env while retaining virtual FS mutations. |
| Just Bash in-memory filesystem persists files across executions | `just_bash_persists_virtual_files_across_exec_calls` | covered-adapter | Covers Open Agents sandbox persistence across tool calls through the shared crate. |
| Open Agents bash tool can run simple commands through a sandbox without Vercel credentials | `slack_app_mention_routes_bash_tool_call_through_just_bash_without_vercel_credentials` | covered-adapter | Proves Slack-started Open Agents fixture routing uses the safe backend. |
| Open Agents bash approval policy still gates risky commands | `open_agent_risky_commands_need_approval` | covered | Existing approval logic remains unchanged. |
| Unsupported Just Bash commands must not fall back to host processes | `just_bash_maps_failures_to_shell_shaped_exec_results` | covered-adapter | Unsupported commands return `127` from the crate-backed runtime. |
| External sandbox backends stay explicit options | `from_reader_accepts_explicit_local_sandbox_for_host_process_backend`; `from_reader_accepts_vercel_sandbox_selection_without_credentials` | covered | Local and Vercel remain selectable, but no longer the default safe backend. |
