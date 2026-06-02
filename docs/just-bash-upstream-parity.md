# Just Bash Upstream Parity

Upstream source refreshed with:

```bash
npx opensrc fetch https://github.com/vercel-labs/just-bash
```

Local mirror inspected at `/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/just-bash/main`.

## Command Behavior Slices

JB-05 owns the portable command registry and earlier high-risk built-ins/coreutils/text/search command smoke coverage implemented in `crates/just-bash/src/commands.rs`, `crates/just-bash/src/runtime.rs`, and additive command hooks in `crates/just-bash/src/exec.rs`.

JBC-07 extends that command slice with exact upstream row closures for portable text/search/structured-data commands. JBC-09 narrows the largest remaining `command:awk` gap with exact portable AWK rows for print, fields, separators, BEGIN/END, simple patterns, stdin/files, and common diagnostics. JBC-10 extends the `command:rg` slice with exact row closures for portable ripgrep-compatible virtual filesystem search, filters, output modes, context, max-count, no-filename, `--files`, and stdin behavior. JBC-11 promotes exact-pass generated conformance-corpus comparison rows to verified rows without hiding unrelated command-family failures. JBC-12 maps portable syntax and transform rows to named parser/AST tests. JBC-13 maps portable read-write, overlay, and mountable filesystem rows to deterministic virtual filesystem tests. JBC-14 maps portable sandbox/security rows and JS-only worker/runtime exceptions. JBC-15 closes additional interpreter core, builtin dispatch, expansion, substitution, arithmetic, array, alias/function, loop, pipefail/status, and diagnostic rows. JBC-16 adds deterministic structured/data command coverage for bounded `jq`, `yq`, `xan`, and `sqlite3` behavior that runs fully against the Rust virtual filesystem. JBC-17 closes executor-package and public example behavior through the shared session/executor API, NAPI-backed JavaScript smoke coverage, and explicit no-host-shell semantics. JBC-18 closes focused CLI/package rows for argument planning, help/version output, command invocation shape, JSON result shape, CJS package entry behavior, and JS-only distribution exceptions. JBC-19 closes targeted shell quoting, heredoc, pipeline-stderr, serializer, and transform plugin rows. JBC-20 closes portable core runtime/session rows for facade cwd/env APIs, per-exec scoped environment/cwd restoration, timeout/cancellation behavior, and selected cd/env/parse-error comparison rows while re-verifying already-mapped pipeline diagnostics/status rows. JBC-21 closes basename/dirname comparison-corpus rows through crate-backed small POSIX command implementations. JBC-22 closes `find` plus opt-in fake-transport `curl` rows. JBC-23 closes deeper text command rows. JBC-24 closes deeper structured-data and query-engine safe-object rows. These slices do not claim upstream `fs/**`, remaining filesystem rows beyond the JBC-13 mappings, remaining parser/syntax/transform rows beyond the JBC-12/JBC-19 mappings, remaining security rows beyond the JBC-14 mappings, remaining interpreter rows beyond the JBC-15/JBC-19/JBC-20 mappings, full AWK/JQ/YQ languages, full ripgrep compatibility, full CSV/SQL engines, search-engine internals, binary tests, UTF-8 byte-level tests, PCRE/stats/replace/vimgrep/multiline/passthru behavior, host OverlayFS write policy/root existence, CLI errexit runtime behavior, bundled binary runtime execution rows, interactive readline shell semantics, remaining mock-clock/logger/PIPESTATUS behavior, broad `test`/`tee` comparison suites, remaining small POSIX command rows beyond the generated basename/dirname corpus subset, TeePlugin exec/file-capture rows, or JS-only command runtimes. Rows are closed only when `docs/open-agents/just-bash-parity.md` names a Rust test, NAPI JS proof, generated corpus proof, or explicit exception below.

Mapped Rust/NAPI proofs:

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
| 200 additional exact rows across `packages/just-bash/src/commands/rg/{rg.basic,rg.filtering,rg.output,rg.max-count,rg.no-filename,rg.ripgrep-compat,rg.utf8-stdin}.test.ts` and `packages/just-bash/src/commands/rg/imported-tests/feature.test.ts` | `rg_upstream_basic_rows_are_portable`; `rg_upstream_filtering_rows_are_portable`; `rg_upstream_output_mode_rows_are_portable`; `rg_upstream_max_count_rows_are_portable`; `rg_upstream_no_filename_rows_are_portable`; `rg_upstream_ripgrep_compat_rows_are_portable`; `rg_upstream_files_and_imported_feature_rows_are_portable` | portable-mapped | JBC-10 verifies portable rg recursive virtual search, path filters/globs, case/fixed/regex/word/line flags, line numbers, context, hidden/gitignore handling, binary skipping, stdin UTF-8, `--files`, max-depth, max-count, count/list/quiet/no-filename modes, and pattern-file/null-separator behavior without host `rg`. |
| 11 exact rows in `head.test.ts`; 16 in `tail.test.ts`; 12 in `wc.test.ts`; 10 in `sort.test.ts`; 11 in `uniq.test.ts`; 11 in `cut.test.ts`; 13 in `tr.test.ts` | `text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows` | portable-mapped | JBC-07 covers the highest-agent-value text pipeline commands over the virtual filesystem and stdin. |
| 14 exact rows in `packages/just-bash/src/commands/jq/jq.basic.test.ts` | `structured_data_jq_basic_rows_access_and_iteration` | portable-mapped | JBC-07 covers jq identity pretty-printing, object/array access, missing/null handling, iteration, and simple pipes over JSON stdin. |
| 55 additional exact `command:awk` rows across `awk.test.ts`, `awk.output.test.ts`, `awk.fields.test.ts`, and `awk.edge-cases.test.ts`; 20 exact AWK real-Bash comparison rows | `awk_upstream_core_rows_cover_blocks_patterns_printf_stdin_and_errors`; `awk_upstream_field_separator_output_and_filename_rows` | portable-mapped | JBC-09 covers portable AWK escapes, `-v`, BEGIN/END, regex and NR patterns, printf, stdin/errors, string concatenation, `FILENAME`/`FNR`, `$NF`, FS/OFS/ORS basics, regex/tab separators, and empty input behavior. Arithmetic, arrays, functions, field mutation, and control flow remain pending. |
| 179 exact-pass generated comparison corpus rows | generated corpus-case proofs consumed by `crates/just-bash/tests/conformance_corpus.rs` | portable-mapped | JBC-11 promotes only comparison fixture rows where the Rust backend exactly matches normalized stdout, stderr, and exit status. Mismatching rows remain pending in `docs/open-agents/just-bash-parity.md`. |
| 52 additional exact rows across `packages/just-bash/src/commands/jq/{jq.test.ts,jq.operators.test.ts,jq.functions.test.ts,jq.filters.test.ts,jq.strings.test.ts}` | `structured_data_jq_flags_files_functions_and_operators_close_rows` | portable-mapped | JBC-16 covers jq flags, multi-file/stdin inputs, JSON streams, selected operators/functions/string helpers, and fail-closed error/help paths. |
| 28 exact rows across `packages/just-bash/src/commands/yq/{yq.test.ts,yq.env.test.ts}` | `structured_data_yq_yaml_json_env_and_error_rows` | portable-mapped | JBC-16 covers simple YAML/JSON navigation, JSON/raw/compact output, scoped environment lookup, option validation, exit status, and simple jq-compatible functions. |
| 27 exact rows across `packages/just-bash/src/commands/xan/{xan.basic.test.ts,xan.columns.test.ts,xan.data.test.ts,xan.filter-sort.test.ts}` | `structured_data_xan_basic_columns_data_filter_rows` | portable-mapped | JBC-16 covers CSV row counts, headers, head/tail/slice/reverse/enum/behead, select/drop/rename, JSON conversion, filters, sort, dedup, search, and diagnostics. |
| 21 exact rows across `packages/just-bash/src/commands/sqlite3/{sqlite3.test.ts,sqlite3.formatters.test.ts,sqlite3.options.test.ts,sqlite3.errors.test.ts}` | `structured_data_sqlite3_options_errors_and_simple_select_rows` | portable-mapped | JBC-16 covers help/version, simple in-memory CREATE/INSERT/SELECT, stdin SQL, list/csv/json output, separator/newline options, argument errors, unknown options, and `load_extension` blocking. |
| `packages/just-bash/src/cli/just-bash.test.ts` 4 help/version rows | `jbc18_cli_help_and_version_flags_match_upstream_output`; `jbc18_napi_cli_planner_matches_upstream_help_version_and_argument_rows` | portable-mapped | JBC-18 verifies `-h`/`--help` and `-v`/`--version` output through the Rust CLI planner and NAPI planner harness. |
| `packages/just-bash/src/cli/just-bash.test.ts` 10 invocation/JSON/source rows | `jbc18_cli_invocation_shape_executes_inline_stdin_script_file_and_json_rows` | portable-mapped | JBC-18 verifies inline scripts, echo, pipes, `ls`, readable virtual files, stdin source, script-file source, JSON stdout/stderr/exitCode shape, and `/home/user/project` mount-path routing against the Rust in-memory backend. |
| `packages/just-bash/src/cli/just-bash.test.ts` 4 cwd normalization rows | `jbc18_cli_argument_parser_routes_sources_root_and_cwd`; `jbc18_napi_cli_planner_matches_upstream_help_version_and_argument_rows` | portable-mapped | JBC-18 verifies default mount cwd, `--cwd` override, and upstream-style virtual cwd normalization without host filesystem access. |
| `packages/just-bash/src/cli/just-bash.test.ts` 2 diagnostics rows | `jbc18_cli_argument_parser_handles_combined_flags_and_errors`; `jbc18_napi_cli_planner_matches_upstream_help_version_and_argument_rows` | portable-mapped | JBC-18 verifies unknown-option and missing `-c` diagnostics. Combined `-ec` parsing is tested as source coverage, but the runtime `errexit` rows stay pending. |
| `packages/just-bash/src/cli/just-bash.bundle.test.ts:193` CJS package entry row | `jbc18_napi_cjs_entrypoint_requires_and_executes_basic_commands` | portable-mapped | JBC-18 verifies the Rust-backed CommonJS package entry requires successfully and executes commands through the NAPI adapter. |
| `packages/just-bash/src/cli/just-bash.bundle.test.ts` 13 dist/bin, lazy-load, worker-layout, optional WASM, and ESM dynamic-require rows | `js-only:upstream-esbuild-binary-lazy-load-worker-and-dynamic-require-distribution` | js-only-documented | JBC-18 documents exact JavaScript package distribution rows that do not map to Rust runtime parity; Rust/NAPI CJS and ESM package entries are verified separately. |
| `examples/cjs-consumer/index.ts` and `packages/just-bash/src/cli/{exec,shell}.ts` source rows | `jbc18_napi_cjs_entrypoint_requires_and_executes_basic_commands`; `jbc18_napi_esm_entrypoint_imports_and_executes_basic_commands`; `js-only:node-dev-exec-and-readline-shell-wrapper` | portable-mapped / js-only-documented | JBC-18 maps the CJS consumer example to the NAPI package-entry tests and classifies Node dev/interactive CLI wrappers as JS-only tooling. |
| Additional exact text rows across `echo.binary`, `printf`, `grep.advanced`, `sed`, `head`, `tail`, `wc`, `sort`, `cut`, `tr`, and `uniq` suites | `echo_upstream_command_handles_newline_and_escape_flags`; `printf_upstream_command_formats_core_specifiers_and_escapes`; `text_search_grep_rg_sed_and_awk_close_upstream_rows`; `text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows` | portable-mapped | JBC-23 closes deeper text-stream and text-pipeline cases over the Rust virtual filesystem without host command fallback. |
| 92 additional exact rows across `packages/just-bash/src/commands/jq/{jq.functions.test.ts,jq.operators.test.ts,jq.filters.test.ts,jq.construction.test.ts,jq.strings.test.ts,jq.keyword-field-access.test.ts,jq.prototype-pollution.test.ts}` | `structured_data_jq_deep_query_construction_and_operator_rows`; `structured_data_jq_string_keyword_and_safe_object_rows` | portable-mapped | JBC-24 covers additional jq functions, operators, filters, construction, keyword field access, string helpers, and dangerous-key handling over deterministic JSON stdin. |
| 16 additional exact rows across `packages/just-bash/src/commands/yq/{yq.test.ts,yq.env.test.ts,yq.yaml-security.test.ts}` | `structured_data_yq_deep_query_env_and_security_rows` | portable-mapped | JBC-24 covers yq join/indent/combined-option behavior, additional jq-compatible functions, scoped env lookup, and YAML/JSON dangerous-key handling. |
| 26 additional exact rows across `packages/just-bash/src/commands/xan/{xan.basic.test.ts,xan.columns.test.ts,xan.data.test.ts,xan.filter-sort.test.ts}` | `structured_data_xan_extended_csv_rows` | portable-mapped | JBC-24 covers additional xan headers/head/tail/slice/behead, select/drop/rename, filter/sort/dedup/search, and JSON conversion diagnostics over in-memory CSV/JSON fixtures. |
| 23 additional exact rows across `packages/just-bash/src/commands/sqlite3/{sqlite3.test.ts,sqlite3.output-modes.test.ts,sqlite3.options.test.ts,sqlite3.errors.test.ts}` | `structured_data_sqlite3_deep_options_modes_and_error_rows` | portable-mapped | JBC-24 covers additional sqlite3 options, output modes, SQL error flow, NULL JSON, and `load_extension` blocking over in-memory databases. |
| 10 exact rows in `packages/just-bash/src/commands/query-engine/safe-object.test.ts` | `structured_data_query_engine_safe_key_rows` | portable-mapped | JBC-24 covers safe-key classification, ignored unsafe inserts, and filtered `from_entries` construction through Rust structured-data helpers. |

## JBC-12 Syntax and Transform Slice

JBC-12 closes portable parser/shell/AST/transform rows without claiming command implementation parity beyond what the mapped tests exercise. The Rust coverage lives in `crates/just-bash/src/shell.rs` and maps only rows named by `docs/open-agents/just-bash-parity.md`; advanced parameter operations and remaining parser cases stay `portable-pending`. JBC-19 extends this surface for heredocs, Tee AST rewriting, plugin chaining, quoting, and pipeline stderr rows in the section below.

Mapped Rust tests:

| Upstream file/case | Rust test | Status | Notes |
| --- | --- | --- | --- |
| All 16 rows in `packages/just-bash/src/syntax/case-statement.test.ts` | `jbc12_syntax_case_statement_matches_upstream_patterns` | portable-verified | Verifies exact/wildcard/glob/character-class/multi-pattern matching, variable and command-substitution case words, optional opening parens, no-match behavior, and first-match execution. |
| All 27 rows in `packages/just-bash/src/syntax/command-substitution.test.ts` | `jbc12_syntax_command_substitution_and_arithmetic_rows_match_upstream` | portable-verified | Verifies command substitution, newline collapse, assignment capture, input redirection, pipelines inside substitutions, and arithmetic operators/variables. |
| All 8 rows in `packages/just-bash/src/transform/plugins/command-collector.test.ts` plus 8 portable collector traversal rows in `transform.test.ts` | `jbc12_transform_command_collector_walks_upstream_ast_shapes` | portable-verified | Verifies sorted unique command collection across pipelines, conditionals, loops, case statements, functions, and command substitutions without mutating execution behavior. |
| 63 core round-trip rows in `packages/just-bash/src/transform/serialize.test.ts` | `jbc12_transform_serialize_round_trips_core_ast_rows` | portable-verified | Verifies Rust AST parse/serialize/parse equivalence for simple commands, pipelines, lists, redirections, word parts, selected parameter operations, and supported compound commands. |

## JBC-19 Shell Syntax, Heredoc, and Transform Closure

JBC-19 closes exact upstream rows for shell-safe argument quoting, heredoc parsing/serialization/execution, here-strings, pipeline stderr propagation, and transform plugin ordering/metadata. It keeps TeePlugin exec/file-capture rows and unproven advanced shell grammar rows pending unless the generated ledger names a Rust test.

Mapped Rust tests:

| Upstream file/case | Rust test | Status | Notes |
| --- | --- | --- | --- |
| All 14 rows in `packages/just-bash/src/helpers/shell-quote.test.ts` | `jbc19_shell_join_args_quotes_and_preserves_literal_arguments` | portable-verified | Verifies single-quote escaping, metacharacter neutralization, empty arguments, whitespace, quotes, newlines/tabs, and literal interpreter round trips. |
| 19 exact rows in `packages/just-bash/src/syntax/here-document.test.ts` plus all 5 heredoc comparison rows | `jbc19_heredoc_rows_parse_serialize_and_execute_with_expansion` | portable-verified | Verifies heredoc stdin delivery, quoted delimiters, variables, command substitution, grep/wc/pipeline consumers, delimiter variants, whitespace preservation, and serializer equivalence. |
| 8 exact rows in `packages/just-bash/src/interpreter/pipeline-execution.test.ts` | `jbc19_pipeline_stderr_rows_keep_regular_and_pipe_stderr_separate` | portable-verified | Verifies regular pipes keep stderr on the parent stream, `|&` pipes stdout and stderr together, multi-stage stderr cases, and final-command status. |
| 76 exact rows in `packages/just-bash/src/transform/serialize.test.ts` | `jbc19_transform_serialize_quoting_edge_rows_round_trip`; `jbc19_heredoc_rows_parse_serialize_and_execute_with_expansion` | portable-verified | Verifies quoting round trips, execution-equivalent serializer rows for portable quoting, heredocs, and here-strings. |
| 15 exact rows in `packages/just-bash/src/transform/transform.test.ts` plus transform source rows for `pipeline.ts`, `tee-plugin.ts`, and `types.ts` | `jbc19_transform_tee_plugin_metadata_and_script_rows`; `jbc19_transform_plugin_ordering_and_metadata_rows` | portable-verified | Verifies no-plugin identity, Tee AST rewriting, target filtering, metadata, global counters, dynamic command names, sanitized timestamps, plugin ordering, metadata merging, rewrites, and exception propagation. |

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

## JBC-14 Security and Sandbox Slice

JBC-14 closes exact upstream security rows only where the Rust backend proves the same portable security behavior with named tests. It maps 40 `security:sandbox` rows to Rust tests for registry-bound command resolution, virtual filesystem isolation, environment/session non-disclosure, source/eval fail-closed behavior, output/command limits, and host marker redaction. It classifies 8 Python/SQLite worker-protocol rows as `js-only-documented`, plus 4 JavaScript host-runtime defense source files (`blocked-globals.ts`, `defense-context.ts`, `defense-in-depth-box.ts`, `trusted-globals.ts`) as JS-only source rows.

Rows involving parser/interpreter behavior beyond the JBC-15 mappings, broad command-family behavior, process-info/device semantics not exercised by JBC-14, and ReadWriteFs host-adapter behavior remain pending unless `docs/open-agents/just-bash-parity.md` names an exact Rust test or a narrow JS-only exception.

Mapped Rust tests:

| Upstream file/case | Rust test | Status | Notes |
| --- | --- | --- | --- |
| `packages/just-bash/src/security/sandbox/command-security.test.ts:103` | `just_bash_security_sandbox_command_rows_are_virtual_and_registry_bound` | portable-verified | Verifies PATH hijacking does not bypass the Rust command registry. |
| 16 exact rows in `packages/just-bash/src/security/sandbox/sandbox-escape.test.ts` | `just_bash_security_sandbox_command_rows_are_virtual_and_registry_bound`; `just_bash_security_sandbox_dynamic_rows_fail_closed_or_stay_in_process`; `just_bash_security_sandbox_information_disclosure_rows_do_not_expose_host_state`; `just_bash_security_fuzzing_attack_oracles_are_ported_to_rust` | portable-verified | Verifies virtual file writes, host file/process denial, environment isolation, registry-only execution, command-count/output caps, and per-exec state isolation. |
| 23 exact rows in `packages/just-bash/src/security/sandbox/information-disclosure.test.ts` | `just_bash_security_sandbox_information_disclosure_rows_do_not_expose_host_state` | portable-verified | Verifies host path/env/process/network/history/source diagnostics do not expose host or secret markers. |
| Python/SQLite worker rows in `security/sandbox/{error-forwarding-runtime-leak-probe,python-sqlite-information-disclosure,worker-protocol-runtime-desync}.test.ts` | `just_bash_optional_runtime_security_cases_are_classified_nonportable` | js-only-documented | Rust has no Python/SQLite JS/WASM worker protocol to desynchronize; portable redaction and sandbox policy are mapped separately. |

## JBC-20 Core Runtime And Session Slice

JBC-20 closes exact portable upstream rows for core runtime/session behavior in `crates/just-bash/src/exec.rs`, `crates/just-bash/src/runtime.rs`, and `crates/just-bash/src/commands.rs`. It adds upstream-style `Bash` facade cwd/env/file API behavior, virtual `/bin` command stubs, scoped per-exec env/cwd restoration after errors, real concurrent isolation tests, sleep duration parsing, an in-process `timeout` builtin, and stable result metadata. Rows that depend on upstream mock-clock injection, logger callbacks, PIPESTATUS/`|&`, shell loops, full `test`, and `tee` comparison behavior remain pending.

Mapped Rust tests:

| Upstream file/case | Rust test | Status | Notes |
| --- | --- | --- | --- |
| 17 exact rows in `packages/just-bash/src/Bash.exec-options.test.ts` | `jbc20_exec_scope_restores_env_cwd_after_errors_and_concurrent_runs` | portable-verified | Verifies multiple per-exec env vars, special-character values, restoration after command/tokenization errors, concurrent env isolation, command-set variable non-leakage, and portable sleep suffix/multiple-duration parsing. |
| 13 exact rows in `packages/just-bash/src/Bash.general.test.ts` | `jbc20_bash_general_default_layout_and_api_rows_match_upstream` | portable-verified | Verifies facade `readFile`/`writeFile` relative paths, `getCwd`, `getEnv`, default `/home/user`, `/tmp`, `/bin` command stubs, `/bin/echo`, default HOME, and no default `/home/user` layout when files or cwd are supplied. |
| All 18 rows in `packages/just-bash/src/commands/timeout/timeout.test.ts` | `jbc20_timeout_command_rows_use_cooperative_in_process_cancellation` | portable-verified | Verifies duration parsing, ignored options, operand diagnostics, help output, cooperative exit 124, and no stdout or virtual file side effects after timeout. |
| 7 rows in `packages/just-bash/src/interpreter/pipeline-execution.test.ts` already mapped by JBC-19 | `jbc20_pipeline_stderr_exit_status_and_metadata_rows_are_stable` | supporting-proof | Re-verifies stderr propagation from first/middle/last pipeline commands, stdout/stderr separation, multiple error collection, and last-command exit status without changing ledger ownership. |
| 8 cd comparison rows, 4 env/printenv comparison rows, and 16 parse/status/quoting comparison rows | `jbc20_cd_env_and_status_comparison_rows_match_core_runtime` | portable-verified | Verifies portable cd traversal/errors, env and printenv output/status, unknown-command diagnostics, missing-file statuses, exit/true/false, `&&`/`||`/semicolon, quoting, and empty/whitespace commands. |

## JBC-21 Small POSIX Basename/Dirname Slice

JBC-21 closes the portable basename/dirname comparison fixture rows through crate-backed `basename` and `dirname` builtins plus the Rust conformance corpus runner. Upstream standalone command-package rows for basename/dirname and the broader small POSIX command family remain pending unless the generated ledger names a Rust test or generated corpus proof.

Mapped Rust tests:

| Upstream file/case | Rust test | Status | Notes |
| --- | --- | --- | --- |
| All 15 rows in `packages/just-bash/src/comparison-tests/basename-dirname.comparison.test.ts` | generated `just_bash_runs_shared_conformance_corpus::comparison_basename_dirname_*` cases plus `basename_dirname_upstream_command_rows_are_portable` | portable-verified | Verifies basename suffix handling, multiple arguments, path-with-slash behavior, dirname root/current-directory behavior, and no host command fallback. |

## Pending JB Follow-Up Counts

The upstream command package currently has 4,899 command-domain cases in the
JB-01 generated inventory. JB-05 maps 83 command cases plus 6 core registry
cases, JBC-06 maps 92 additional exact core-command cases, JBC-07 maps 143
additional text/search/structured-data rows, JBC-09 maps 55 additional
`command:awk` rows plus 20 AWK comparison rows to named Rust tests, JBC-10 maps
200 additional exact rg rows to named Rust tests, and JBC-11 maps 179 exact-pass
generated comparison corpus rows. JBC-12 maps 43 syntax rows plus 79 transform
rows to named Rust tests. JBC-13 maps 157 filesystem rows to named Rust tests.
JBC-14 maps 40 exact portable security sandbox rows and 8 JS-only worker rows.
JBC-15 maps 73 interpreter core, builtin, expansion, substitution, arithmetic,
array, alias/function, loop, status, and diagnostic rows to named Rust tests.
JBC-16 closes 128 additional exact structured/data command rows on top of that
JBC-15 tracker baseline. JBC-17 then closes executor-package and public example
rows, JBC-18 closes 21 focused CLI/package case rows while documenting 13
JS-only package distribution rows, JBC-19 maps 137 exact shell quoting, heredoc,
pipeline-stderr, serializer, and transform plugin rows to named Rust tests, and
JBC-20 closes 76 net-new core runtime/session and comparison rows while
supporting 7 pipeline rows already owned by JBC-19. JBC-21 closes 15
basename/dirname comparison-corpus rows. JBC-22 closes 195 exact `find` rows and
202 exact `curl` rows with deterministic in-memory filesystem behavior plus
opt-in fake network/resource seams. JBC-23 closes deeper text command rows, and
JBC-24 closes 167 additional exact structured-data/query-engine rows. After
regeneration the combined Just Bash ledger is `2,857` verified / `6,920`
pending / `159` JS-only, with `6,931` strict gate gaps. These slices do not
claim full command, filesystem, syntax, transform, interpreter, security,
executor, examples, host-backed CLI, or source-only command-module parity until
every portable row is named in the generated ledger.

The counts below are the exact current upstream command-family case counts from
`docs/open-agents/just-bash-parity.md` after the JB-05, JBC-06, JBC-07, JBC-09,
JBC-10, JBC-11, JBC-13, JBC-16, JBC-22, JBC-23, and JBC-24 command-family row mappings. Seed smoke
coverage in these slices does not close rows outside the named verified cases.
JBC-12 syntax/transform closure, JBC-14 security closure, JBC-15 interpreter
closure, JBC-17 executor/example closure, JBC-18 CLI/package closure, JBC-19
shell/transform closure, JBC-20 core runtime/session closure, and JBC-21
basename/dirname comparison closure are tracked above and in the generated
per-domain tables.

| Family | Exact upstream command cases | Verified exact rows | Pending |
| --- | ---: | ---: | ---: |
| Core filesystem command rows closed by JBC-06 (`cat`, `ls`, `mkdir`, `rm`, `cp`, `mv`) | 92 | 92 | 0 |
| `grep` basic/advanced/perl/exclude/binary/UTF-8 suite | 213 | 47 | 166 |
| Full `awk` command language suite | 643 | 63 | 580 |
| Full `sed` command/address/error suite | 231 | 24 | 207 |
| Full `rg` ripgrep compatibility suite, including imported tests | 590 | 207 | 383 |
| `head` command suite | 17 | 12 | 5 |
| `tail` command suite | 19 | 17 | 2 |
| `wc` command suite | 20 | 15 | 5 |
| `sort` command suite | 57 | 17 | 40 |
| `uniq` command suite | 15 | 15 | 0 |
| `cut` command suite | 16 | 13 | 3 |
| `tr` command suite | 27 | 24 | 3 |
| `jq` command suite | 254 | 158 | 96 |
| `curl` command suite | 202 | 202 | 0 |
| `find` command suite | 195 | 195 | 0 |
| Archive/compression (`gzip`, `gunzip`, `zcat`, `tar`) | 210 | 0 | 210 |
| `yq` command suite | 215 | 44 | 171 |
| `xan` command suite | 201 | 53 | 148 |
| `sqlite3` command suite | 151 | 44 | 96 |
| `query-engine` safe-object/query helper suite | 45 | 10 | 35 |
| `search-engine` internal matcher/prefilter suite | 53 | 0 | 53 |
| Remaining command utilities outside the groups above | 1,402 | 155 | 1,212 |

`sqlite3` also has 11 JS-only worker/runtime rows documented as excluded in the generated ledger.

Do not mark these rows `verified` until each portable upstream case maps to a named Rust test or a documented non-portable exception.
