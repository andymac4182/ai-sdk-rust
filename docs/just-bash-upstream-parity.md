# Just Bash Upstream Parity

Upstream source refreshed with:

```bash
npx opensrc fetch https://github.com/vercel-labs/just-bash
```

Local mirror inspected at `/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/just-bash/main`.

## Command Behavior Slices

JB-05 owns the portable command registry and earlier high-risk built-ins/coreutils/text/search command smoke coverage implemented in `crates/just-bash/src/commands.rs`, `crates/just-bash/src/runtime.rs`, and additive command hooks in `crates/just-bash/src/exec.rs`.

JBC-07 extends that command slice with exact upstream row closures for portable text/search/structured-data commands. This ledger does not claim upstream `fs/**`, parser/syntax, security rows, full AWK/JQ languages, full ripgrep compatibility, binary tests, UTF-8 byte-level tests, or JS-only command runtimes. Rows are closed only when `docs/open-agents/just-bash-parity.md` names a Rust test below.

Mapped Rust tests:

| Upstream file/case | Rust test | Status | Notes |
| --- | --- | --- | --- |
| `packages/just-bash/src/Bash.commands.test.ts:6` registers all commands by default | `registry_upstream_bash_commands_registers_all_supported_by_default` | portable-mapped | Verifies default registry exposes supported high-risk commands (`echo`, `ls`, `grep`) without host process execution. |
| `packages/just-bash/src/Bash.commands.test.ts:20` only registers specified commands | `registry_upstream_bash_commands_only_registers_specified_commands` | portable-mapped | Restricted registry permits `echo`/`cat` and rejects `ls` with exit 127. |
| `packages/just-bash/src/Bash.commands.test.ts:39` grep unavailable when filtered | `registry_upstream_bash_commands_grep_not_available_when_not_registered` | portable-mapped | Restricted registry rejects `grep` in a pipeline with exit 127. |
| `packages/just-bash/src/Bash.commands.test.ts:49` default command names | `registry_upstream_get_command_names_returns_upstream_defaults` | portable-mapped | Mirrors upstream registry names as data without claiming unsupported implementations. |
| `packages/just-bash/src/Bash.commands.test.ts:60` empty commands array means no commands | `registry_upstream_bash_commands_empty_commands_array_means_no_commands` | portable-mapped | Empty registry rejects `echo` with command-not-found. |
| `packages/just-bash/src/Bash.commands.test.ts:70` subset file commands | `registry_upstream_bash_commands_can_use_subset_of_file_commands` | portable-mapped | Restricted registry permits `ls`/`cat`/`mkdir` and rejects unregistered file commands. |
| `packages/just-bash/src/commands/bash/bash.test.ts` all 25 cases | `bash_upstream_command_executes_dash_c_and_positional_args`; `bash_upstream_command_executes_script_files_and_shebangs`; `bash_upstream_command_forwards_stdin_to_nested_dash_c` | portable-mapped | Uses Rust interpreter recursion, virtual files, and forwarded pipeline stdin; does not call host `/bin/bash`. |
| `packages/just-bash/src/commands/echo/echo.test.ts` all 16 cases | `echo_upstream_command_handles_newline_and_escape_flags` | portable-mapped | Covers empty/simple/multiple args, quotes, `-n`, `-e`, `-E`, combined flags, and dash-leading text. |
| `packages/just-bash/src/commands/printf/printf.test.ts` 14 selected formatter/escape cases | `printf_upstream_command_formats_core_specifiers_and_escapes` | portable-mapped | Leaves backslash/unicode/width/error/`-v` printf rows pending in the JB-01 ledger. |
| `packages/just-bash/src/commands/env/env.test.ts` all 13 cases | `pwd_cd_export_env_and_printenv_are_virtual_and_isolated` | portable-mapped | Virtual env/printenv, `-i`, `-u`, help, and host isolation. |
| `packages/just-bash/src/commands/pwd/pwd.test.ts` all 7 cases | `pwd_cd_export_env_and_printenv_are_virtual_and_isolated` | portable-mapped | Default `/home/user`, explicit cwd, `cd`, `cd ..`, and ignored args. |
| `packages/just-bash/src/commands/touch/touch.test.ts` all 8 cases | `coreutils_upstream_file_commands_use_virtual_fs` | portable-mapped | Create, preserve content, relative paths, spaces, hidden files, and missing operand. |
| `packages/just-bash/src/commands/{cat,ls,mkdir,rm,cp,mv}` smoke cases | `coreutils_upstream_file_commands_use_virtual_fs` | portable-smoke-only | Supportive command smoke coverage only; JB-01 rows remain pending. |
| `packages/just-bash/src/commands/bash/bash.test.ts:156` redirection file operation script | `redirection_upstream_cases_write_append_and_read_virtual_files` | portable-mapped | Covers `>`, `>>`, and `<` with virtual files. |
| `packages/just-bash/src/commands/{find,grep,rg,sed,awk}` basic behavior | `find_grep_rg_sed_and_awk_cover_high_risk_text_search_bucket` | portable-smoke-only | Basic search/text behavior only; JB-01 rows remain pending until case-exact tests land. |
| `packages/just-bash/src/commands/jq/jq.basic.test.ts` simple field lookup | `structured_data_jq_minimal_port_reads_virtual_input` | portable-smoke-only | Minimal structured-data smoke for virtual input; structured-data rows remain pending. |
| 20 exact rows in `packages/just-bash/src/commands/grep/grep.basic.test.ts`; 7 exact rows in `packages/just-bash/src/commands/rg/rg.basic.test.ts`; 10 exact rows in `packages/just-bash/src/commands/sed/sed.test.ts`; 8 exact rows in `packages/just-bash/src/commands/awk/awk.test.ts` | `text_search_grep_rg_sed_and_awk_close_upstream_rows` | portable-mapped | JBC-07 covers portable grep flags/stdin/files/recursive search, rg current-dir basics, sed substitution/addresses, and simple awk print-field behavior. |
| 11 exact rows in `head.test.ts`; 16 in `tail.test.ts`; 12 in `wc.test.ts`; 10 in `sort.test.ts`; 11 in `uniq.test.ts`; 11 in `cut.test.ts`; 13 in `tr.test.ts` | `text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows` | portable-mapped | JBC-07 covers the highest-agent-value text pipeline commands over the virtual filesystem and stdin. |
| 14 exact rows in `packages/just-bash/src/commands/jq/jq.basic.test.ts` | `structured_data_jq_basic_rows_access_and_iteration` | portable-mapped | JBC-07 covers jq identity pretty-printing, object/array access, missing/null handling, iteration, and simple pipes over JSON stdin. |

## JBC-06 Core Command Conformance Slice

This slice closes exact portable upstream rows for high-volume core command behavior in the generated JB-01 ledger. Rows not listed here remain `portable-pending`, including `ls` long-format/executable/symlink classification, root default-directory assertions, binary/UTF-8 side files, and generated source rows.

Mapped Rust tests:

| Upstream file/case | Rust test | Status | Notes |
| --- | --- | --- | --- |
| `packages/just-bash/src/commands/cat/cat.test.ts` all 17 cases | `cat_upstream_command_covers_files_numbering_stdin_and_errors` | portable-verified | Verifies file reads, concatenation, `-n`, stdin, `-`, relative paths, and missing-file continuation with the virtual filesystem. |
| `packages/just-bash/src/commands/ls/ls.test.ts` 17 selected portable cases | `ls_upstream_command_covers_hidden_multi_path_recursive_and_classify_cases` | portable-verified | Verifies hidden-file flags, multiple paths, recursion, single files, empty directories, directory classification, and reverse sorting. |
| `packages/just-bash/src/commands/mkdir/mkdir.test.ts` 9 portable cases | `mkdir_rm_upstream_command_flags_and_errors_are_virtual` | portable-verified | Verifies `-p`/`--parents`, nonrecursive parent errors, existing paths, missing operands, relative paths, and multiple nested paths. |
| `packages/just-bash/src/commands/rm/rm.test.ts` 14 portable cases | `mkdir_rm_upstream_command_flags_and_errors_are_virtual` | portable-verified | Verifies file removal, `-f`, recursive flags, combined flags, missing operands, empty directories, relative paths, and missing-file diagnostics. |
| `packages/just-bash/src/commands/cp/cp.test.ts` all 14 cases | `cp_mv_upstream_command_directory_targets_flags_and_errors_are_virtual` | portable-verified | Verifies file copies, overwrites, directory targets, multi-source copies, recursive directory copies, relative paths, and common diagnostics. |
| `packages/just-bash/src/commands/mv/mv.test.ts` all 21 cases | `cp_mv_upstream_command_directory_targets_flags_and_errors_are_virtual` | portable-verified | Verifies file and directory moves, directory targets, multi-source moves, relative paths, force/no-clobber/verbose flags, help, and diagnostics. |

## Pending JB Follow-Up Counts

The upstream command package currently has 4,899 command-domain cases in the
JB-01 generated inventory. JB-05 maps 83 command cases plus 6 core registry
cases, JBC-06 maps 92 additional exact core-command cases, and JBC-07 maps 143
additional text/search/structured-data rows to named Rust tests. These slices
do not claim full command parity. After regeneration the full Just Bash ledger
is `931` verified / `8,889` pending / `116` JS-only.

The counts below are the exact current upstream command-family case counts from
`docs/open-agents/just-bash-parity.md` after the JB-05, JBC-06, and JBC-07 row
mappings. Seed smoke coverage in these slices does not close rows outside the
named verified cases.

| Family | Exact upstream command cases | Verified exact rows | Pending |
| --- | ---: | ---: | ---: |
| Core filesystem command rows closed by JBC-06 (`cat`, `ls`, `mkdir`, `rm`, `cp`, `mv`) | 92 | 92 | 0 |
| `grep` basic/advanced/perl/exclude/binary/UTF-8 suite | 213 | 20 | 193 |
| Full `awk` command language suite | 643 | 8 | 635 |
| Full `sed` command/address/error suite | 231 | 10 | 221 |
| Full `rg` ripgrep compatibility suite, including imported tests | 590 | 7 | 583 |
| `head` command suite | 17 | 11 | 6 |
| `tail` command suite | 19 | 16 | 3 |
| `wc` command suite | 20 | 12 | 8 |
| `sort` command suite | 57 | 10 | 47 |
| `uniq` command suite | 15 | 11 | 4 |
| `cut` command suite | 16 | 11 | 5 |
| `tr` command suite | 27 | 13 | 14 |
| `jq` command suite | 254 | 14 | 240 |
| Archive/compression (`gzip`, `gunzip`, `zcat`, `tar`) | 210 | 0 | 210 |
| Remaining structured data (`yq`, `xan`, `sqlite3`) | 567 | 0 | 567 |
| Remaining command utilities outside the groups above | 1,722 | 0 | 1,722 |

Do not mark these rows `verified` until each portable upstream case maps to a named Rust test or a documented non-portable exception.
