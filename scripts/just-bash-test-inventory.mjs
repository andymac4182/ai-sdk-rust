#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..'
);

const defaultUpstreamRoot =
  '/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/just-bash/main';
const upstreamRoot = process.env.JUST_BASH_UPSTREAM_PATH ?? defaultUpstreamRoot;
const outputPath = path.join(
  repositoryRoot,
  'docs/open-agents/just-bash-parity.md'
);
const rustRunnerFixturePath = path.join(
  repositoryRoot,
  'crates/just-bash/tests/fixtures/just-bash-conformance.json'
);

const upstreamRepo = 'vercel-labs/just-bash';
const upstreamHead = 'd64009aef6bc1556e7c84b22ed455863275ea953';
const inventoryDate = '2026-06-02';

const expectedManifestCount = 8;
const expectedTsFileCount = 908;
const expectedTestFileCount = 485;

const skipDirectories = new Set([
  '.ai-sdk-rust-conformance',
  '.git',
  'dist',
  'node_modules',
  'vendor',
]);
const codeFilePattern = /\.(?:[cm]?ts|tsx)$/;
const testFilePattern = /\.(?:test|spec)(?:-d)?\.(?:[cm]?ts|tsx)$/;
const validStatuses = new Set([
  'portable-pending',
  'portable-verified',
  'js-only-documented',
  'type-system-impossible',
]);
const strictPassStatuses = new Set([
  'portable-verified',
  'js-only-documented',
  'type-system-impossible',
]);

const jb03CaseGroups = [
  {
    file: 'packages/just-bash/src/fs/real-fs-utils.test.ts',
    lines: [25, 29, 33, 37, 41, 45, 49, 53, 57, 61, 65, 69, 76, 82],
    owner: 'crates/just-bash::path',
    rustTest: 'upstream_real_fs_utils_normalize_path_cases',
    notes: 'JB-03 verifies portable normalizePath behavior without host filesystem access.',
  },
  {
    file: 'packages/just-bash/src/fs/real-fs-utils.test.ts',
    lines: [91, 95, 99, 103, 110, 114, 118, 122, 130, 136],
    owner: 'crates/just-bash::path',
    rustTest: 'upstream_real_fs_utils_is_path_within_root_cases',
    notes: 'JB-03 verifies boundary-safe path containment as a pure helper.',
  },
  {
    file: 'packages/just-bash/src/fs/real-fs-utils.test.ts',
    lines: [288, 294, 298, 302, 306, 310],
    owner: 'crates/just-bash::path',
    rustTest: 'upstream_real_fs_utils_validate_path_cases',
    notes: 'JB-03 verifies null-byte path rejection and sanitized virtual errors.',
  },
  {
    file: 'packages/just-bash/src/fs/interface.contract.test.ts',
    lines: [5],
    owner: 'crates/just-bash::fs',
    rustTest: 'upstream_interface_contract_reads_writes_appends_stats_lists_and_removes_files',
    notes: 'JB-03 verifies the portable in-memory filesystem interface contract.',
  },
  {
    file: 'packages/just-bash/src/fs/interface.contract.test.ts',
    lines: [21],
    owner: 'crates/just-bash::fs',
    rustTest: 'upstream_interface_contract_copies_and_moves_without_content_changes',
    notes: 'JB-03 verifies copy and move preserve file contents in the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/fs/interface.contract.test.ts',
    lines: [34],
    owner: 'crates/just-bash::path',
    rustTest: 'upstream_interface_contract_rejects_null_byte_paths',
    notes: 'JB-03 verifies null-byte rejection for read and mutating virtual paths.',
  },
  {
    file: 'packages/just-bash/src/fs/interface.contract.test.ts',
    lines: [45],
    owner: 'crates/just-bash::path',
    rustTest: 'upstream_interface_contract_clamps_traversal_above_root',
    notes: 'JB-03 verifies traversal normalization clamps above the virtual root.',
  },
  {
    file: 'packages/just-bash/src/fs/interface.contract.test.ts',
    lines: [54],
    owner: 'crates/just-bash::path',
    rustTest: 'upstream_interface_contract_resolves_relative_paths_consistently',
    notes: 'JB-03 verifies relative path, dirname, and join behavior.',
  },
  {
    file: 'packages/just-bash/src/fs/interface.contract.test.ts',
    lines: [62],
    owner: 'crates/just-bash::fs',
    rustTest: 'upstream_interface_contract_symlinks_keep_absolute_targets_virtual',
    notes: 'JB-03 verifies virtual symlink creation and absolute target preservation.',
  },
  {
    file: 'packages/just-bash/src/fs/in-memory-fs/in-memory-fs.test.ts',
    lines: [
      6, 16, 26, 35, 45, 57, 66, 76, 85, 95, 104, 114, 125, 138, 149, 162,
      177, 187, 204, 215,
    ],
    owner: 'crates/just-bash::encoding',
    rustTest: 'upstream_in_memory_binary_and_encoding_cases',
    notes: 'JB-03 verifies binary buffers, encodings, append, large files, and symlinked reads.',
  },
  {
    file: 'packages/just-bash/src/fs/in-memory-fs/in-memory-fs.test.ts',
    lines: [229, 252, 266, 281, 289, 299],
    owner: 'crates/just-bash::fs',
    rustTest: 'upstream_in_memory_readdir_with_file_types_cases',
    notes: 'JB-03 verifies deterministic directory entries and file type metadata.',
  },
  {
    file: 'packages/just-bash/src/fs/in-memory-fs/in-memory-fs.security.test.ts',
    lines: [14, 20, 28, 35, 43, 51, 60, 70, 80, 91, 98, 110, 120],
    owner: 'crates/just-bash::fs',
    rustTest: 'upstream_in_memory_symlink_path_resolution_cases',
    notes: 'JB-03 verifies in-memory symlink resolution, loops, chained links, and clamped targets.',
  },
  {
    file: 'packages/just-bash/src/fs/in-memory-fs/in-memory-fs.security.test.ts',
    lines: [133, 141, 149, 157, 165, 176, 187, 284, 294, 305, 312],
    owner: 'crates/just-bash::fs',
    rustTest: 'upstream_in_memory_lstat_stat_realpath_symlink_cases',
    notes: 'JB-03 verifies readlink, lstat/stat, and realpath symlink semantics.',
  },
  {
    file: 'packages/just-bash/src/fs/in-memory-fs/in-memory-fs.security.test.ts',
    lines: [
      201, 217, 236, 250, 259, 269, 274, 353, 368, 383, 401, 414, 423, 446,
      463, 481, 493, 503, 510,
    ],
    owner: 'crates/just-bash::fs',
    rustTest:
      'upstream_in_memory_write_append_rm_cp_link_symlink_policy_cases; upstream_real_fs_utils_normalize_path_cases; upstream_real_fs_utils_validate_path_cases; upstream_in_memory_binary_and_encoding_cases; upstream_error_sanitization_cases',
    notes: 'JB-03 verifies in-memory write, append, rm, cp, link, path edge, encoding, and error safety cases.',
  },
  {
    file: 'packages/just-bash/src/fs/sanitize-error.test.ts',
    lines: [5],
    owner: 'crates/just-bash::sanitize',
    rustTest: 'upstream_error_sanitization_cases',
    notes: 'JB-03 verifies host path and file URL scrubbing.',
  },
  {
    file: 'packages/just-bash/src/fs/real-fs-utils.sanitize.test.ts',
    lines: [5, 13, 20, 27, 33, 39, 46, 55, 61, 67, 71, 76, 82, 88, 94, 100, 106],
    owner: 'crates/just-bash::sanitize',
    rustTest: 'upstream_error_sanitization_cases',
    notes: 'JB-03 verifies sanitizer behavior without depending on real filesystem access.',
  },
  {
    file: 'packages/just-bash/src/encoding.fs-fallback.test.ts',
    lines: [36, 54],
    owner: 'crates/just-bash::file_reader',
    rustTest: 'upstream_file_reader_fallback_and_concat_cases',
    notes: 'JB-03 verifies byte-preserving file-reader fallback and ByteString round trips.',
  },
];

const jb03CaseOverrides = new Map();
for (const group of jb03CaseGroups) {
  for (const line of group.lines) {
    jb03CaseOverrides.set(`${group.file}:${line}`, {
      status: 'portable-verified',
      owner: group.owner,
      rustTest: group.rustTest,
      notes: group.notes,
    });
  }
}

const jb06NetworkSourceFiles = [
  'packages/just-bash/src/network/allow-list.ts',
  'packages/just-bash/src/network/allow-list/shared.ts',
  'packages/just-bash/src/network/dns-pin.ts',
  'packages/just-bash/src/network/fetch.ts',
  'packages/just-bash/src/network/index.ts',
  'packages/just-bash/src/network/types.ts',
];

const jb06NetworkTestFiles = [
  'packages/just-bash/src/network/allow-list/bypass.test.ts',
  'packages/just-bash/src/network/allow-list/dns-rebinding-integration.test.ts',
  'packages/just-bash/src/network/allow-list/dns-rebinding.test.ts',
  'packages/just-bash/src/network/allow-list/e2e.test.ts',
  'packages/just-bash/src/network/allow-list/firewall.test.ts',
  'packages/just-bash/src/network/allow-list/mock.test.ts',
  'packages/just-bash/src/network/allow-list/pen-test-pocs.test.ts',
  'packages/just-bash/src/network/allow-list/unit.test.ts',
  'packages/just-bash/src/network/dns-pin-fetch.test.ts',
  'packages/just-bash/src/network/dns-pin.test.ts',
];

const jb06LimitTestFiles = [
  'packages/just-bash/src/security-limits.test.ts',
  'packages/just-bash/src/security/limits/dos-limits.test.ts',
  'packages/just-bash/src/security/limits/memory-exhaustion.test.ts',
  'packages/just-bash/src/security/limits/output-size-limits.test.ts',
  'packages/just-bash/src/security/limits/pipeline-limits.test.ts',
  'packages/just-bash/src/security/limits/security-hardening.test.ts',
];

const jb06RuntimeOnlySourceFiles = [
  'packages/just-bash/src/commands/js-exec/js-exec-worker.ts',
  'packages/just-bash/src/commands/python3/worker.ts',
  'packages/just-bash/src/commands/sqlite3/worker.ts',
  'packages/just-bash/src/security/wasm-callback.ts',
  'packages/just-bash/src/security/worker-defense-in-depth.ts',
];

const jb06RuntimeOnlyTestFiles = [
  'packages/just-bash/src/browser.bundle.test.ts',
  'packages/just-bash/src/commands/js-exec/js-exec.module-resolution-security.test.ts',
  'packages/just-bash/src/commands/js-exec/js-exec.security.test.ts',
  'packages/just-bash/src/commands/python3/python3.queue-desync.runtime.test.ts',
  'packages/just-bash/src/commands/python3/python3.queue-timeout-exploit.test.ts',
  'packages/just-bash/src/commands/python3/python3.worker-protocol-abuse.test.ts',
  'packages/just-bash/src/commands/sqlite3/sqlite3.worker-protocol-abuse.test.ts',
  'packages/just-bash/src/commands/sqlite3/sqlite3.worker-resolution.test.ts',
  'packages/just-bash/src/security/wasm-callback.test.ts',
  'packages/just-bash/src/security/worker-defense-in-depth.test.ts',
];

const jb06SourceGroups = [
  {
    files: jb06NetworkSourceFiles,
    status: 'portable-verified',
    owner: 'crates/just-bash::security::network',
    notes:
      'JB-06 verifies portable allow-list, DNS-pinning request planning, fake transport, redirect, timeout, method, and response-limit seams without live network.',
  },
  {
    files: jb06RuntimeOnlySourceFiles,
    status: 'js-only-documented',
    owner: 'crates/just-bash::security::runtime-classification',
    notes:
      'JB-06 classifies this optional JS/worker/WASM runtime source as not part of the portable Rust backend; portable network, limits, redaction, and diagnostics are mapped separately.',
  },
];

const jb06CaseGroups = [
  {
    files: jb06NetworkTestFiles,
    status: 'portable-verified',
    owner: 'crates/just-bash::security::network',
    rustTest:
      'just_bash_network_allow_list_matches_origins_paths_and_bypass_cases; just_bash_network_validates_allow_list_and_defaults_to_no_network; just_bash_network_plans_allowed_request_with_fake_dns_and_header_firewall; just_bash_network_blocks_private_hosts_and_dns_rebinding_before_transport; just_bash_network_fails_closed_for_dns_errors_but_allows_enotfound_to_fetch; just_bash_network_revalidates_redirects_and_repins_each_hop; just_bash_network_blocks_disallowed_redirect_without_second_transport_call; just_bash_network_enforces_method_timeout_and_response_limits; just_bash_network_fake_transport_records_only_planned_requests',
    notes:
      'JB-06 maps the portable network policy surface with fake DNS and fake transport only; no live fetch or host networking is used.',
  },
  {
    files: jb06LimitTestFiles,
    status: 'portable-verified',
    owner: 'crates/just-bash::security::limits',
    rustTest:
      'just_bash_security_resource_limits_match_upstream_limit_diagnostics; just_bash_security_timeout_and_abort_share_cancellation_contract; just_bash_security_allow_deny_policy_blocks_denied_and_unknown_commands',
    notes:
      'JB-06 verifies deterministic resource, timeout, cancellation, command-limit, and diagnostic seams without executing a host shell.',
  },
  {
    file: 'packages/just-bash/src/sandbox/Sandbox.security.test.ts',
    lines: [6, 22, 36, 51, 64, 76, 93],
    status: 'portable-verified',
    owner: 'crates/just-bash::security',
    rustTest:
      'just_bash_security_path_validation_blocks_escape_and_nul_cases; just_bash_security_allow_deny_policy_blocks_denied_and_unknown_commands; just_bash_security_timeout_and_abort_share_cancellation_contract',
    notes:
      'JB-06 verifies shell-injection-safe path/argv policy seams and consistent timeout/abort diagnostics without spawning commands.',
  },
  {
    file: 'packages/just-bash/src/security/sandbox/information-disclosure.test.ts',
    lines: [19, 54, 108, 116, 124, 132, 140, 574],
    status: 'portable-verified',
    owner: 'crates/just-bash::security::redaction',
    rustTest:
      'just_bash_security_redacts_sandbox_paths_and_sensitive_env_values; just_bash_security_path_validation_blocks_escape_and_nul_cases',
    notes:
      'JB-06 verifies the portable path and sensitive-environment redaction seam; command-specific virtualization rows stay with their command/runtime owners.',
  },
  {
    file: 'packages/just-bash/src/commands/cat/cat.test.ts',
    lines: [5, 15, 24, 36, 49, 58, 67, 77, 89, 96, 106, 115, 125, 133, 140, 150, 160],
    status: 'portable-verified',
    owner: 'crates/just-bash::commands::cat',
    rustTest: 'cat_upstream_command_covers_files_numbering_stdin_and_errors',
    notes:
      'JBC-06 verifies portable cat file reads, concatenation, -n numbering, stdin, dash placeholder, relative paths, and missing-file behavior in the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/ls/ls.test.ts',
    lines: [5, 29, 41, 53, 102, 114, 129, 137, 146, 159, 172, 184, 196, 213, 329, 342, 355],
    status: 'portable-verified',
    owner: 'crates/just-bash::commands::ls',
    rustTest: 'ls_upstream_command_covers_hidden_multi_path_recursive_and_classify_cases',
    notes:
      'JBC-06 verifies portable ls directory listing, hidden-file flags, multiple paths, recursion, single files, empty directories, classify directories, and reverse sorting.',
  },
  {
    file: 'packages/just-bash/src/commands/mkdir/mkdir.test.ts',
    lines: [24, 34, 41, 48, 58, 68, 77, 85, 95],
    status: 'portable-verified',
    owner: 'crates/just-bash::commands::mkdir',
    rustTest: 'mkdir_rm_upstream_command_flags_and_errors_are_virtual',
    notes:
      'JBC-06 verifies portable mkdir parent flags, nonrecursive parent errors, existing-path handling, missing operands, relative paths, and multiple nested paths.',
  },
  {
    file: 'packages/just-bash/src/commands/rm/rm.test.ts',
    lines: [5, 31, 41, 49, 58, 70, 79, 92, 102, 111, 119, 127, 135, 143],
    status: 'portable-verified',
    owner: 'crates/just-bash::commands::rm',
    rustTest: 'mkdir_rm_upstream_command_flags_and_errors_are_virtual',
    notes:
      'JBC-06 verifies portable rm file removal, force and recursive flags, combined flags, missing operands, empty directories, relative paths, and missing-file diagnostics.',
  },
  {
    file: 'packages/just-bash/src/commands/cp/cp.test.ts',
    lines: [5, 15, 24, 36, 48, 61, 73, 82, 92, 101, 113, 122, 131, 140],
    status: 'portable-verified',
    owner: 'crates/just-bash::commands::cp',
    rustTest: 'cp_mv_upstream_command_directory_targets_flags_and_errors_are_virtual',
    notes:
      'JBC-06 verifies portable cp file copies, overwrites, directory targets, multi-source copies, recursive directory copies, relative paths, and common diagnostics.',
  },
  {
    file: 'packages/just-bash/src/commands/mv/mv.test.ts',
    lines: [5, 15, 24, 33, 45, 58, 70, 81, 93, 105, 114, 123, 133, 146, 160, 178, 188, 197, 209, 223, 233],
    status: 'portable-verified',
    owner: 'crates/just-bash::commands::mv',
    rustTest: 'cp_mv_upstream_command_directory_targets_flags_and_errors_are_virtual',
    notes:
      'JBC-06 verifies portable mv file and directory moves, directory targets, multi-source moves, relative paths, force/no-clobber/verbose flags, help, and diagnostics.',
  },
  {
    files: jb06RuntimeOnlyTestFiles,
    status: 'js-only-documented',
    owner: 'crates/just-bash::security::runtime-classification',
    rustTest: 'just_bash_optional_runtime_security_cases_are_classified_nonportable',
    notes:
      'JB-06 classifies browser bundle, QuickJS, Node worker, Python WASM, SQLite WASM, and WASM callback bridge behavior as not portable to this Rust backend; portable security behavior is mapped in separate JB-06 rows.',
  },
];

const jbc07CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/grep/grep.basic.test.ts',
    lines: [
      5, 15, 25, 34, 43, 52, 61, 70, 79, 88, 97, 106, 133, 203, 210, 218,
      228, 237, 249, 267,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-search',
    rustTest: 'text_search_grep_rg_sed_and_awk_close_upstream_rows',
    notes:
      'JBC-07 verifies portable grep regex/fixed search basics, flags, stdin, file labels, recursive virtual search, counts, and error cases without host grep.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/rg.basic.test.ts',
    lines: [5, 18, 32, 46, 72, 115, 143],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-search',
    rustTest: 'text_search_grep_rg_sed_and_awk_close_upstream_rows',
    notes:
      'JBC-07 verifies portable ripgrep-style current-directory search, relative paths, line numbers, no-match exit, no-line-number output, and case controls over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/sed/sed.test.ts',
    lines: [34, 42, 50, 58, 66, 74, 82, 90, 100, 108],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-search',
    rustTest: 'text_search_grep_rg_sed_and_awk_close_upstream_rows',
    notes:
      'JBC-07 verifies portable sed substitution, print/delete addresses, stdin, delimiters, regex replacement, and missing-file errors.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.test.ts',
    lines: [45, 54, 63, 72, 83, 92, 116, 125],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-search',
    rustTest: 'text_search_grep_rg_sed_and_awk_close_upstream_rows',
    notes:
      'JBC-07 verifies portable awk field printing, missing fields, custom field separators, NR, and NF for simple print programs.',
  },
  {
    file: 'packages/just-bash/src/commands/head/head.test.ts',
    lines: [5, 17, 25, 33, 41, 49, 60, 69, 75, 83, 91],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes:
      'JBC-07 verifies portable head line selection, attached -n, multi-file headers, stdin, empty input, missing files, and no-trailing-newline output.',
  },
  {
    file: 'packages/just-bash/src/commands/tail/tail.test.ts',
    lines: [5, 17, 25, 33, 41, 49, 60, 69, 75, 83, 99, 108, 117, 125, 133, 142],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes:
      'JBC-07 verifies portable tail line selection, attached -n, multi-file headers, stdin, +N syntax, empty input, missing files, and head/tail pipelines.',
  },
  {
    file: 'packages/just-bash/src/commands/wc/wc.test.ts',
    lines: [5, 16, 24, 32, 40, 58, 71, 85, 92, 100, 108, 124],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes:
      'JBC-07 verifies portable wc line/word/byte/char counts, long flags, stdin, multiple-file totals, spacing, and missing-file errors.',
  },
  {
    file: 'packages/just-bash/src/commands/sort/sort.test.ts',
    lines: [17, 25, 33, 41, 49, 57, 65, 73, 81, 91],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes:
      'JBC-07 verifies portable sort alphabetic/numeric/reverse/unique/key/stdin/case-sensitive behavior, empty input, and missing-file errors.',
  },
  {
    file: 'packages/just-bash/src/commands/uniq/uniq.test.ts',
    lines: [16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 98],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes:
      'JBC-07 verifies portable uniq adjacent grouping, counts, duplicate/unique filters, stdin, sort pipelines, empty input, and missing-file errors.',
  },
  {
    file: 'packages/just-bash/src/commands/cut/cut.test.ts',
    lines: [17, 25, 33, 41, 49, 57, 65, 73, 81, 91, 101],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes:
      'JBC-07 verifies portable cut field and character lists, delimiters, ranges, stdin, open ranges, missing-file errors, and missing selector diagnostics.',
  },
  {
    file: 'packages/just-bash/src/commands/tr/tr.test.ts',
    lines: [11, 19, 27, 35, 43, 51, 59, 67, 75, 83, 91, 99, 107],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes:
      'JBC-07 verifies portable tr translation, deletion, squeezing, ranges, short SET2 handling, newline deletion, and operand diagnostics.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.basic.test.ts',
    lines: [6, 14, 23, 30, 39, 46, 53, 62, 69, 76, 85, 92, 99, 110],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_basic_rows_access_and_iteration',
    notes:
      'JBC-07 verifies portable jq identity pretty-printing, object and array access, missing/null handling, iteration, and simple pipe filters over JSON stdin.',
  },
];

const jbc09CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/awk/awk.test.ts',
    lines: [
      5, 12, 19, 28, 35, 103, 136, 147, 158, 169, 178, 187, 198, 207, 220,
      229, 236, 243, 253, 530, 539, 651,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest:
      'awk_upstream_core_rows_cover_blocks_patterns_printf_stdin_and_errors; awk_upstream_field_separator_output_and_filename_rows',
    notes:
      'JBC-09 verifies portable awk escapes, -v variables, BEGIN/END blocks, regex and NR patterns, printf, stdin, common errors, string concatenation, FILENAME/FNR, and regex field separators; full AWK arithmetic, arrays, functions, and control flow remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.output.test.ts',
    lines: [6, 15, 24, 33, 44, 53, 64, 77, 86, 95, 106],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest:
      'awk_upstream_core_rows_cover_blocks_patterns_printf_stdin_and_errors; awk_upstream_field_separator_output_and_filename_rows',
    notes:
      'JBC-09 verifies portable awk OFS/ORS print output, print without arguments, printf without implicit newline, explicit printf newline, and parenthesized printf for simple formats.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.fields.test.ts',
    lines: [
      6, 13, 22, 29, 106, 113, 120, 147, 174, 196, 205, 214, 227, 236,
      243, 252, 261, 270,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_upstream_field_separator_output_and_filename_rows',
    notes:
      'JBC-09 verifies portable awk field access, missing fields, $NF, NF basics, OFS/ORS print separators, whitespace/-F/BEGIN FS separators, tab/regex FS, and empty single-character FS fields.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.edge-cases.test.ts',
    lines: [6, 15, 26, 33],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_upstream_core_rows_cover_blocks_patterns_printf_stdin_and_errors',
    notes:
      'JBC-09 verifies portable awk empty-file, empty-stdin, BEGIN/END-on-empty-input, and blank-record NR/NF behavior.',
  },
  {
    file: 'packages/just-bash/src/comparison-tests/awk.comparison.test.ts',
    lines: [
      21, 28, 35, 42, 52, 59, 66, 75, 82, 91, 102, 113, 126, 133, 140,
      149, 160, 196, 201, 212,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest:
      'awk_upstream_core_rows_cover_blocks_patterns_printf_stdin_and_errors; awk_upstream_field_separator_output_and_filename_rows',
    notes:
      'JBC-09 maps the portable real-Bash comparison rows for field access, -F separators, NR/NF, BEGIN/END, simple pattern filtering, printf, stdin, and string concatenation to deterministic Rust tests.',
  },
];

const jbc10CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/rg/rg.basic.test.ts',
    lines: [59, 85, 100, 128, 156, 171, 184, 199, 215, 234, 242, 250],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_upstream_basic_rows_are_portable',
    notes:
      'JBC-10 verifies the remaining portable rg.basic rows for recursive virtual search, smart-case variants, binary skipping, max-depth, and diagnostics.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/rg.filtering.test.ts',
    lines: [5, 20, 34, 46, 60, 74, 91, 105, 121, 136, 151, 166, 181, 197, 212],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_upstream_filtering_rows_are_portable',
    notes:
      'JBC-10 verifies portable rg type, glob, hidden-file, and gitignore filtering over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/rg.output.test.ts',
    lines: [5, 18, 32, 46, 60, 73, 90, 103, 116, 131, 144, 157, 172, 189, 202, 215, 230, 245],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_upstream_output_mode_rows_are_portable',
    notes:
      'JBC-10 verifies portable rg count, file-list, only-matching, context, quiet, and help output modes.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/rg.max-count.test.ts',
    lines: [
      10, 22, 34, 48, 60, 72, 84, 98, 112, 127, 144, 157, 171, 185, 198,
      210, 222, 234, 248, 262, 275, 288, 301, 321, 333, 345, 357, 370,
      385, 397,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_upstream_max_count_rows_are_portable',
    notes:
      'JBC-10 verifies portable rg -m/--max-count behavior with files, flags, context, globs, types, and regexes.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/rg.no-filename.test.ts',
    lines: [10, 22, 34, 46, 61, 75, 88, 105, 118, 131, 143, 155, 167, 179, 191, 207],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_upstream_no_filename_rows_are_portable',
    notes:
      'JBC-10 verifies portable rg -I/--no-filename behavior for directory, file, count, match, invert, word, max-count, and file-list modes.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/rg.ripgrep-compat.test.ts',
    lines: [
      20, 35, 49, 63, 79, 94, 110, 125, 138, 151, 166, 181, 196, 214,
      228, 240, 254, 268, 283, 297, 310, 326, 340, 352, 367, 381, 395,
      411, 422, 438, 450, 467, 482, 494, 509, 523, 536, 550, 561, 572,
      585, 600, 614, 628, 643, 656, 672, 707, 759, 773, 787, 802, 831,
      873, 893, 907, 923,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_upstream_ripgrep_compat_rows_are_portable',
    notes:
      'JBC-10 verifies portable ripgrep-compat rg rows for search modes, filters, context, gitignore, pattern files, counts, headings, null separators, sorting, and no-filename output.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/feature.test.ts',
    lines: [
      114, 126, 138, 152, 166, 180, 194, 207, 221, 235, 250, 279, 292,
      305, 318, 330, 346, 357, 370, 382, 431, 458, 472, 486, 500, 516,
      529, 544, 558, 571, 584, 597, 610, 623, 638, 651, 666, 678, 689,
      703, 717, 731, 743, 757, 769, 783, 795, 807, 821, 833, 845,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_upstream_files_and_imported_feature_rows_are_portable',
    notes:
      'JBC-10 verifies portable imported rg feature rows for --files, max-depth, filters, gitignore, hidden files, word/line/fixed/invert matching, quiet, line numbers, context, and combined flags.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/rg.utf8-stdin.test.ts',
    lines: [5],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_upstream_files_and_imported_feature_rows_are_portable',
    notes:
      'JBC-10 verifies rg preserves UTF-8 stdin matches and emits the upstream stdin source label.',
  },
];

const jbc12SourceGroups = [
  {
    files: [
      'packages/just-bash/src/transform/plugins/command-collector.ts',
      'packages/just-bash/src/transform/serialize.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::transform',
    notes:
      'JBC-12 verifies portable Rust AST command collection and serializer round-trips over the exact mapped upstream transform rows.',
  },
];

const jbc12CaseGroups = [
  {
    file: 'packages/just-bash/src/syntax/case-statement.test.ts',
    lines: [5, 17, 29, 42, 55, 68, 81, 93, 102, 114, 127, 139, 154, 166, 178, 191],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbc12_syntax_case_statement_matches_upstream_patterns',
    notes:
      'JBC-12 verifies portable case parsing/execution for exact, wildcard, glob, multi-pattern, variable, command-substitution, optional-paren, no-match, and first-match rows.',
  },
  {
    file: 'packages/just-bash/src/syntax/command-substitution.test.ts',
    lines: [
      5, 12, 18, 26, 32, 38, 46, 55, 63, 69, 77, 83, 89, 95, 101, 107, 113,
      119, 125, 131, 137, 145, 153, 160, 166, 172, 178,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbc12_syntax_command_substitution_and_arithmetic_rows_match_upstream',
    notes:
      'JBC-12 verifies portable command substitution, unquoted newline handling, assignment capture, input redirection, pipelines, and arithmetic expansion/operator rows.',
  },
  {
    file: 'packages/just-bash/src/transform/plugins/command-collector.test.ts',
    lines: [6, 14, 24, 32, 40, 50, 64, 71],
    status: 'portable-verified',
    owner: 'crates/just-bash::transform::command-collector',
    rustTest: 'jbc12_transform_command_collector_walks_upstream_ast_shapes',
    notes:
      'JBC-12 verifies sorted unique Rust AST command collection across pipelines, if/else, loops, case, functions, and command substitutions without mutating execution behavior.',
  },
  {
    file: 'packages/just-bash/src/transform/transform.test.ts',
    lines: [112, 119, 126, 133, 140, 147, 154, 161],
    status: 'portable-verified',
    owner: 'crates/just-bash::transform::command-collector',
    rustTest: 'jbc12_transform_command_collector_walks_upstream_ast_shapes',
    notes:
      'JBC-12 maps the portable CommandCollectorPlugin AST traversal rows; plugin chaining, tee rewriting, and JS metadata API rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/transform/serialize.test.ts',
    lines: [
      39, 40, 41, 42, 43, 44, 45, 49, 50, 51, 52, 58, 59, 60, 61, 62, 66,
      67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 81, 82, 83, 84, 85, 86, 87,
      88, 89, 90, 91, 92, 93, 94, 98, 99, 100, 101, 102, 107, 108, 135,
      136, 137, 138, 142, 143, 144, 149, 150, 154, 155, 157, 163, 164, 166,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::transform::serialize',
    rustTest: 'jbc12_transform_serialize_round_trips_core_ast_rows',
    notes:
      'JBC-12 verifies Rust AST parse/serialize/parse equivalence for the mapped simple-command, pipeline, list, redirection, word-part, parameter, and compound-command rows.',
  },
];

const jbc13SourceGroups = [
  {
    files: [
      'packages/just-bash/src/fs/read-write-fs/index.ts',
      'packages/just-bash/src/fs/read-write-fs/read-write-fs.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::read-write',
    notes:
      'JBC-13 verifies read/write backend semantics with a deterministic virtual Rust adapter; host-root constructor and host filesystem security rows remain fail-closed as test rows.',
  },
  {
    files: [
      'packages/just-bash/src/fs/overlay-fs/index.ts',
      'packages/just-bash/src/fs/overlay-fs/overlay-fs.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::overlay',
    notes:
      'JBC-13 verifies overlay mount, lower/upper precedence, copy-on-write mutation, tombstones, symlinks, and read-only errors without host filesystem access.',
  },
  {
    files: [
      'packages/just-bash/src/fs/mountable-fs/index.ts',
      'packages/just-bash/src/fs/mountable-fs/mountable-fs.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::mountable',
    notes:
      'JBC-13 verifies mount routing, virtual mount parents, cross-mount copy/move/link behavior, chmod, symlinks, and path normalization over virtual Rust filesystems.',
  },
];

const jbc13CaseGroups = [
  {
    file: 'packages/just-bash/src/fs/read-write-fs/read-write-fs.test.ts',
    lines: [
      43, 51, 60, 69, 74, 82, 97, 107, 118, 130, 141, 153, 160, 167, 174,
      184, 193, 200, 219, 229, 237, 243, 252, 264, 269, 277, 286, 296, 301,
      310, 321, 333, 340, 352, 367, 377, 384, 409, 422, 429, 441, 454, 459,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::read-write',
    rustTest: 'upstream_read_write_fs_virtual_backend_reads_writes_stats_and_mutates_paths',
    notes:
      'JBC-13 verifies read/write backend file, directory, stat, list, remove, copy, move, chmod, symlink, hard-link, and readlink behavior in the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/fs/read-write-fs/read-write-fs.test.ts',
    lines: [468, 475, 485, 499, 508, 519, 540, 554, 572, 580, 589],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::read-write',
    rustTest: 'upstream_read_write_fs_virtual_backend_encoding_readdir_and_path_inventory_cases',
    notes:
      'JBC-13 verifies read/write backend resolvePath, getAllPaths, base64/hex encoding, sorted typed readdir entries, symlink typing, error cases, and readdir name parity.',
  },
  {
    file: 'packages/just-bash/src/fs/overlay-fs/overlay-fs.test.ts',
    lines: [
      45, 50, 59, 67, 74, 84, 103, 115, 128, 144, 160, 177, 195, 210, 230,
      247, 263, 278, 295, 310, 328, 340, 353, 369, 383, 395, 409, 426, 440,
      457, 472, 488, 499, 510, 522, 533, 546, 560, 573, 587, 601, 615, 629,
      642, 656, 673,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::overlay',
    rustTest: 'upstream_overlay_fs_virtual_backend_mount_copy_on_write_deletion_and_read_only_cases',
    notes:
      'JBC-13 verifies overlay mount points, lower-layer reads, upper-layer writes, deletion tombstones, merged listings, symlinks, copy/move/link/chmod, and read-only mutator errors without host filesystem access.',
  },
  {
    file: 'packages/just-bash/src/fs/mountable-fs/mountable-fs.test.ts',
    lines: [
      7, 18, 29, 37, 57, 67, 74, 86, 98, 109, 127, 136, 144, 156, 170, 179,
      193, 206, 217, 226, 236, 247, 255, 267, 279, 291, 304, 321, 337, 350,
      363, 373, 386, 398, 409, 419, 433, 444, 457, 476, 494, 505, 519, 528,
      537, 546, 556, 570, 583, 594, 607, 613, 619, 627, 637, 646, 655,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::mountable',
    rustTest: 'upstream_mountable_fs_routes_mounts_cross_mount_ops_and_virtual_dirs',
    notes:
      'JBC-13 verifies mount/unmount validation, route dispatch, virtual mount parents, busy mount errors, cross-mount copy/move/link, symlinks, modes, path resolution, and edge-case normalization.',
  },
];

function groupMatchesFile(group, file) {
  if (group.file && group.file !== file) {
    return false;
  }
  if (group.files && !group.files.includes(file)) {
    return false;
  }
  return true;
}

function rowOverrideFromGroup(group) {
  const { file: _file, files: _files, lines: _lines, ...override } = group;
  return override;
}

function sourceOverrideFor(relativePath) {
  const group = [...jb06SourceGroups, ...jbc12SourceGroups, ...jbc13SourceGroups].find((entry) =>
    groupMatchesFile(entry, relativePath)
  );
  return group ? rowOverrideFromGroup(group) : undefined;
}

function caseOverrideFor(testCase) {
  const group = [
    ...jb06CaseGroups,
    ...jbc07CaseGroups,
    ...jbc09CaseGroups,
    ...jbc10CaseGroups,
    ...jbc12CaseGroups,
    ...jbc13CaseGroups,
  ].find(
    (entry) =>
      groupMatchesFile(entry, testCase.file) &&
      (!entry.lines || entry.lines.includes(testCase.line))
  );
  return group ? rowOverrideFromGroup(group) : undefined;
}

function usage() {
  console.log(`Usage: node scripts/just-bash-test-inventory.mjs [options]

Options:
  --check      Verify docs/open-agents/just-bash-parity.md is current.
  --strict     Fail when any portable case is pending or maps to a missing Rust test.
  --dry-run    Print current inventory counts as JSON.
  --help       Show this help text.

Environment:
  JUST_BASH_UPSTREAM_PATH  Override the OpenSrc mirror path.`);
}

function fail(message) {
  console.error(`just-bash inventory: ${message}`);
  process.exit(1);
}

function parseArgs(argv) {
  const options = { check: false, strict: false, dryRun: false };
  for (const arg of argv) {
    if (arg === '--help') {
      usage();
      process.exit(0);
    }
    if (arg === '--check') {
      options.check = true;
      continue;
    }
    if (arg === '--strict') {
      options.strict = true;
      continue;
    }
    if (arg === '--dry-run') {
      options.dryRun = true;
      continue;
    }
    fail(`unknown option: ${arg}`);
  }
  return options;
}

function walk(directory) {
  const entries = fs.readdirSync(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    if (skipDirectories.has(entry.name)) {
      continue;
    }
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...walk(entryPath));
    } else if (entry.isFile()) {
      files.push(entryPath);
    }
  }
  return files;
}

function upstreamRelative(filePath) {
  return path.relative(upstreamRoot, filePath).replaceAll(path.sep, '/');
}

function repositoryRelative(filePath) {
  return path.relative(repositoryRoot, filePath).replaceAll(path.sep, '/');
}

function escapeCell(value) {
  return String(value ?? '')
    .replaceAll('\\', '\\\\')
    .replaceAll('|', '\\|')
    .replaceAll('\n', '<br>');
}

function renderTable(headers, rows) {
  const lines = [
    `| ${headers.map(escapeCell).join(' | ')} |`,
    `| ${headers.map(() => '---').join(' | ')} |`,
  ];
  for (const row of rows) {
    lines.push(`| ${row.map(escapeCell).join(' | ')} |`);
  }
  return lines.join('\n');
}

function isEscaped(text, index) {
  let backslashes = 0;
  for (let cursor = index - 1; cursor >= 0 && text[cursor] === '\\'; cursor -= 1) {
    backslashes += 1;
  }
  return backslashes % 2 === 1;
}

function unescapeCell(cell) {
  return cell
    .trim()
    .replaceAll('<br>', '\n')
    .replace(/\\([\\|])/g, '$1');
}

function splitMarkdownRow(line) {
  const trimmed = line.trim();
  if (!trimmed.startsWith('|') || !trimmed.endsWith('|')) {
    return undefined;
  }
  const cells = [];
  let current = '';
  for (let index = 1; index < trimmed.length - 1; index += 1) {
    const char = trimmed[index];
    if (char === '|' && !isEscaped(trimmed, index)) {
      cells.push(unescapeCell(current));
      current = '';
    } else {
      current += char;
    }
  }
  cells.push(unescapeCell(current));
  return cells;
}

function isSeparatorRow(cells) {
  return cells.every((cell) => /^:?-{3,}:?$/.test(cell.trim()));
}

function parseTable(markdown, heading) {
  const lines = markdown.split(/\r?\n/);
  const headingIndex = lines.findIndex((line) => line.trim() === heading);
  if (headingIndex === -1) {
    return { headers: [], rows: [] };
  }

  let tableIndex = headingIndex + 1;
  while (tableIndex < lines.length && !lines[tableIndex].trim().startsWith('|')) {
    tableIndex += 1;
  }

  const headers = splitMarkdownRow(lines[tableIndex] ?? '');
  const separator = splitMarkdownRow(lines[tableIndex + 1] ?? '');
  if (!headers || !separator || !isSeparatorRow(separator)) {
    return { headers: [], rows: [] };
  }

  const rows = [];
  for (let rowIndex = tableIndex + 2; rowIndex < lines.length; rowIndex += 1) {
    const cells = splitMarkdownRow(lines[rowIndex]);
    if (!cells) {
      break;
    }
    if (cells.length !== headers.length) {
      continue;
    }
    rows.push(Object.fromEntries(headers.map((header, index) => [header, cells[index]])));
  }
  return { headers, rows };
}

function readExistingLedger() {
  if (!fs.existsSync(outputPath)) {
    return {
      sourceRows: new Map(),
      caseRows: new Map(),
    };
  }

  const markdown = fs.readFileSync(outputPath, 'utf8');
  const sourceRows = new Map();
  for (const row of parseTable(markdown, '## Source File Inventory').rows) {
    if (row['Upstream source file']) {
      sourceRows.set(row['Upstream source file'], row);
    }
  }

  const caseRows = new Map();
  for (const row of parseTable(markdown, '## Test Case Inventory').rows) {
    const key = testCaseKey({
      file: row['Upstream test file'],
      line: Number.parseInt(row.Line, 10),
      declaration: row.Declaration,
      name: row.Case,
    });
    if (key) {
      caseRows.set(key, row);
    }
  }

  return { sourceRows, caseRows };
}

let cachedRustRunnerProofs;

function readRustRunnerProofs() {
  if (cachedRustRunnerProofs) {
    return cachedRustRunnerProofs;
  }
  const byLedgerKey = new Map();
  const proofNames = new Set();

  if (fs.existsSync(rustRunnerFixturePath)) {
    const fixture = JSON.parse(fs.readFileSync(rustRunnerFixturePath, 'utf8'));
    for (const testCase of fixture.cases ?? []) {
      if (testCase.kind !== 'comparison-fixture' || testCase.status !== 'portable-verified') {
        continue;
      }
      const ledgerKey = testCase.parity?.ledgerKey;
      const rustTestName = testCase.rustTestName;
      if (!ledgerKey || !rustTestName) {
        continue;
      }
      byLedgerKey.set(ledgerKey, {
        status: 'portable-verified',
        owner: testCase.parity?.owner ?? 'crates/just-bash::conformance_corpus',
        rustTest: rustTestName,
        notes:
          testCase.parity?.notes ??
          'JBC-11 Rust corpus runner exact match for the generated comparison fixture stdout, stderr, and exit code.',
      });
      proofNames.add(rustTestName);
    }
  }

  cachedRustRunnerProofs = { byLedgerKey, proofNames };
  return cachedRustRunnerProofs;
}

function packageIdFor(relativePath) {
  if (relativePath === 'package.json') {
    return 'root';
  }
  if (relativePath.startsWith('packages/')) {
    return relativePath.split('/').slice(0, 2).join('/');
  }
  if (relativePath.startsWith('examples/')) {
    return relativePath.split('/').slice(0, 2).join('/');
  }
  if (relativePath.startsWith('.changeset/')) {
    return '.changeset';
  }
  if (relativePath.startsWith('.github/')) {
    return '.github';
  }
  return relativePath.split('/')[0] || 'root';
}

function packageName(manifestRelativePath) {
  const manifest = JSON.parse(
    fs.readFileSync(path.join(upstreamRoot, manifestRelativePath), 'utf8')
  );
  return manifest.name ?? '(unnamed)';
}

function domainForPath(relativePath) {
  if (relativePath.startsWith('packages/just-bash-executor/')) {
    return 'executor';
  }
  if (relativePath.startsWith('examples/')) {
    return `example:${relativePath.split('/')[1] ?? 'root'}`;
  }
  if (!relativePath.startsWith('packages/just-bash/src/')) {
    return packageIdFor(relativePath);
  }

  const parts = relativePath.split('/');
  const area = parts[3] ?? 'core';
  const areaEntry = parts[4] ?? '';
  if (!areaEntry && codeFilePattern.test(area)) {
    return 'core';
  }
  if (area === 'commands') {
    return codeFilePattern.test(areaEntry)
      ? 'command:shared'
      : `command:${areaEntry || 'registry'}`;
  }
  if (area === 'spec-tests') {
    return `spec:${areaEntry || 'suite'}`;
  }
  if (area === 'comparison-tests') {
    return 'comparison-tests';
  }
  if (area === 'agent-examples') {
    return 'agent-examples';
  }
  if (area === 'security') {
    return `security:${parts[4] ?? 'core'}`;
  }
  if (area === 'fs') {
    return codeFilePattern.test(areaEntry)
      ? 'fs:core'
      : `fs:${areaEntry || 'core'}`;
  }
  if (area === 'interpreter') {
    return codeFilePattern.test(areaEntry)
      ? 'interpreter:core'
      : `interpreter:${areaEntry || 'core'}`;
  }
  if (area === 'cli' || area === 'shell') {
    return area;
  }
  return area;
}

function ownerForPath(relativePath) {
  const domain = domainForPath(relativePath);
  if (domain.startsWith('command:')) {
    return `pending:just-bash-${domain.replace(':', '-')}`;
  }
  if (domain.startsWith('example:')) {
    return 'pending:just-bash-examples';
  }
  if (domain.startsWith('fs:')) {
    return 'pending:just-bash-fs';
  }
  if (domain.startsWith('security:')) {
    return 'pending:just-bash-security';
  }
  if (domain.startsWith('interpreter:') || domain === 'parser' || domain === 'syntax' || domain === 'ast') {
    return 'pending:just-bash-parser-interpreter';
  }
  if (domain === 'executor') {
    return 'pending:just-bash-executor';
  }
  if (domain === 'cli' || domain === 'shell') {
    return 'pending:just-bash-cli';
  }
  if (domain === 'network') {
    return 'pending:just-bash-network';
  }
  if (domain === 'sandbox') {
    return 'pending:just-bash-sandbox';
  }
  if (domain === 'agent-examples') {
    return 'pending:just-bash-agent-examples';
  }
  if (domain === 'comparison-tests' || domain.startsWith('spec:')) {
    return 'pending:just-bash-spec-comparison';
  }
  return 'pending:just-bash-core';
}

function isIdentifierStart(char) {
  return /[A-Za-z_$]/.test(char ?? '');
}

function isIdentifierPart(char) {
  return /[A-Za-z0-9_$]/.test(char ?? '');
}

function skipWhitespace(source, index) {
  let cursor = index;
  while (/\s/.test(source[cursor] ?? '')) {
    cursor += 1;
  }
  return cursor;
}

function lineStarts(source) {
  const starts = [0];
  for (let index = 0; index < source.length; index += 1) {
    if (source[index] === '\n') {
      starts.push(index + 1);
    }
  }
  return starts;
}

function lineNumberAt(starts, index) {
  let low = 0;
  let high = starts.length - 1;
  while (low <= high) {
    const middle = Math.floor((low + high) / 2);
    if (starts[middle] <= index) {
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return high + 1;
}

function findMatching(source, openIndex, openChar, closeChar) {
  let depth = 0;
  let state = 'code';
  for (let index = openIndex; index < source.length; index += 1) {
    const char = source[index];
    const next = source[index + 1];

    if (state === 'line-comment') {
      if (char === '\n') {
        state = 'code';
      }
      continue;
    }
    if (state === 'block-comment') {
      if (char === '*' && next === '/') {
        index += 1;
        state = 'code';
      }
      continue;
    }
    if (state === 'single') {
      if (char === '\\') {
        index += 1;
      } else if (char === "'") {
        state = 'code';
      }
      continue;
    }
    if (state === 'double') {
      if (char === '\\') {
        index += 1;
      } else if (char === '"') {
        state = 'code';
      }
      continue;
    }
    if (state === 'template') {
      if (char === '\\') {
        index += 1;
      } else if (char === '`') {
        state = 'code';
      }
      continue;
    }

    if (char === '/' && next === '/') {
      index += 1;
      state = 'line-comment';
      continue;
    }
    if (char === '/' && next === '*') {
      index += 1;
      state = 'block-comment';
      continue;
    }
    if (char === "'") {
      state = 'single';
      continue;
    }
    if (char === '"') {
      state = 'double';
      continue;
    }
    if (char === '`') {
      state = 'template';
      continue;
    }

    if (char === openChar) {
      depth += 1;
    } else if (char === closeChar) {
      depth -= 1;
      if (depth === 0) {
        return index;
      }
    }
  }
  return undefined;
}

function parseStringLiteral(source, index) {
  const quote = source[index];
  if (quote !== '"' && quote !== "'" && quote !== '`') {
    return undefined;
  }

  let value = '';
  for (let cursor = index + 1; cursor < source.length; cursor += 1) {
    const char = source[cursor];
    if (char === '\\') {
      value += source[cursor + 1] ?? '';
      cursor += 1;
      continue;
    }
    if (char === quote) {
      return {
        value: value.trim().replace(/\s+/g, ' '),
        end: cursor,
      };
    }
    value += char;
  }
  return undefined;
}

function firstArgumentText(source, openIndex, closeIndex) {
  const start = skipWhitespace(source, openIndex + 1);
  const literal = parseStringLiteral(source, start);
  if (literal) {
    return literal.value || '<empty>';
  }

  let cursor = start;
  let state = 'code';
  while (cursor < closeIndex) {
    const char = source[cursor];
    const next = source[cursor + 1];
    if (state === 'line-comment') {
      if (char === '\n') state = 'code';
      cursor += 1;
      continue;
    }
    if (state === 'block-comment') {
      if (char === '*' && next === '/') {
        cursor += 2;
        state = 'code';
      } else {
        cursor += 1;
      }
      continue;
    }
    if (state === 'single' || state === 'double' || state === 'template') {
      const end = state === 'single' ? "'" : state === 'double' ? '"' : '`';
      if (char === '\\') {
        cursor += 2;
      } else if (char === end) {
        cursor += 1;
        state = 'code';
      } else {
        cursor += 1;
      }
      continue;
    }
    if (char === '/' && next === '/') {
      cursor += 2;
      state = 'line-comment';
      continue;
    }
    if (char === '/' && next === '*') {
      cursor += 2;
      state = 'block-comment';
      continue;
    }
    if (char === "'") {
      cursor += 1;
      state = 'single';
      continue;
    }
    if (char === '"') {
      cursor += 1;
      state = 'double';
      continue;
    }
    if (char === '`') {
      cursor += 1;
      state = 'template';
      continue;
    }
    if (char === ',') {
      break;
    }
    cursor += 1;
  }

  const expression = source.slice(start, cursor).trim().replace(/\s+/g, ' ');
  return expression ? `<dynamic:${expression.slice(0, 80)}>` : '<unknown>';
}

function findBlockRange(source, startIndex) {
  const open = source.indexOf('{', startIndex);
  if (open === -1) {
    return undefined;
  }
  const close = findMatching(source, open, '{', '}');
  if (close === undefined) {
    return undefined;
  }
  return { start: open, end: close };
}

function parseTestLikeCall(source, idStart, idEnd, name) {
  let cursor = skipWhitespace(source, idEnd);
  const declaration = [name];
  while (source[cursor] === '.') {
    cursor += 1;
    cursor = skipWhitespace(source, cursor);
    if (!isIdentifierStart(source[cursor])) {
      return undefined;
    }
    const propStart = cursor;
    cursor += 1;
    while (isIdentifierPart(source[cursor])) {
      cursor += 1;
    }
    declaration.push(source.slice(propStart, cursor));
    cursor = skipWhitespace(source, cursor);
  }

  if (source[cursor] !== '(') {
    return undefined;
  }

  const firstOpen = cursor;
  const firstClose = findMatching(source, firstOpen, '(', ')');
  if (firstClose === undefined) {
    return undefined;
  }

  let titleOpen = firstOpen;
  let titleClose = firstClose;
  const afterFirst = skipWhitespace(source, firstClose + 1);
  if (source[afterFirst] === '(') {
    const secondClose = findMatching(source, afterFirst, '(', ')');
    if (secondClose === undefined) {
      return undefined;
    }
    titleOpen = afterFirst;
    titleClose = secondClose;
  }

  return {
    declaration: declaration.join('.'),
    name: firstArgumentText(source, titleOpen, titleClose),
    start: idStart,
    end: titleClose,
    block: name === 'describe' ? findBlockRange(source, titleClose + 1) : undefined,
  };
}

function scanIdentifiers(source, callback) {
  let state = 'code';
  for (let index = 0; index < source.length; index += 1) {
    const char = source[index];
    const next = source[index + 1];

    if (state === 'line-comment') {
      if (char === '\n') state = 'code';
      continue;
    }
    if (state === 'block-comment') {
      if (char === '*' && next === '/') {
        index += 1;
        state = 'code';
      }
      continue;
    }
    if (state === 'single') {
      if (char === '\\') index += 1;
      else if (char === "'") state = 'code';
      continue;
    }
    if (state === 'double') {
      if (char === '\\') index += 1;
      else if (char === '"') state = 'code';
      continue;
    }
    if (state === 'template') {
      if (char === '\\') index += 1;
      else if (char === '`') state = 'code';
      continue;
    }

    if (char === '/' && next === '/') {
      index += 1;
      state = 'line-comment';
      continue;
    }
    if (char === '/' && next === '*') {
      index += 1;
      state = 'block-comment';
      continue;
    }
    if (char === "'") {
      state = 'single';
      continue;
    }
    if (char === '"') {
      state = 'double';
      continue;
    }
    if (char === '`') {
      state = 'template';
      continue;
    }

    if (!isIdentifierStart(char)) {
      continue;
    }
    const start = index;
    index += 1;
    while (isIdentifierPart(source[index])) {
      index += 1;
    }
    const identifier = source.slice(start, index);
    callback(identifier, start, index);
    index -= 1;
  }
}

function testCaseKey(row) {
  if (!row.file || !row.line || !row.declaration || !row.name) {
    return undefined;
  }
  return `${row.file}:${row.line}:${row.declaration}:${row.name}`;
}

function extractTestCases(relativePath) {
  const absolutePath = path.join(upstreamRoot, relativePath);
  const source = fs.readFileSync(absolutePath, 'utf8');
  const starts = lineStarts(source);
  const describes = [];
  const cases = [];

  scanIdentifiers(source, (identifier, start, end) => {
    if (identifier !== 'describe' && identifier !== 'it' && identifier !== 'test') {
      return;
    }
    const call = parseTestLikeCall(source, start, end, identifier);
    if (!call) {
      return;
    }
    if (identifier === 'describe') {
      if (call.block) {
        describes.push(call);
      }
      return;
    }
    cases.push({
      file: relativePath,
      line: lineNumberAt(starts, start),
      declaration: call.declaration,
      name: call.name,
      start,
    });
  });

  describes.sort((left, right) => left.start - right.start);
  for (const row of cases) {
    row.suite = describes
      .filter((suite) => suite.block && suite.block.start < row.start && row.start < suite.block.end)
      .map((suite) => suite.name)
      .join(' > ') || '(top-level)';
  }
  return cases;
}

function splitRustTestNames(value) {
  return String(value ?? '')
    .split(';')
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function isPendingMarker(value) {
  return (
    !value ||
    value === 'n/a' ||
    value === 'missing' ||
    value.startsWith('pending:')
  );
}

function discoverRustTests() {
  const roots = ['crates', 'src']
    .map((entry) => path.join(repositoryRoot, entry))
    .filter((entry) => fs.existsSync(entry));
  const names = new Set();

  for (const root of roots) {
    for (const file of walk(root).filter((entry) => entry.endsWith('.rs'))) {
      const lines = fs.readFileSync(file, 'utf8').split(/\r?\n/);
      let pendingTestAttribute = false;
      for (const line of lines) {
        if (/^\s*#\[(?:tokio::)?test(?:\([^]]*\))?\]/.test(line)) {
          pendingTestAttribute = true;
          continue;
        }
        if (pendingTestAttribute && /^\s*#\[/.test(line)) {
          continue;
        }
        const match =
          /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/.exec(
            line
          );
        if (pendingTestAttribute && match) {
          names.add(match[1]);
        }
        pendingTestAttribute = false;
      }
    }
  }
  for (const proofName of readRustRunnerProofs().proofNames) {
    names.add(proofName);
  }
  return names;
}

function findFixtureRoots(allFiles) {
  const fixtureNames = new Set(['fixtures', 'fixture', '__fixtures__', 'testdata', 'examples']);
  const roots = new Map();

  for (const file of allFiles) {
    const parts = file.split('/');
    for (let index = 0; index < parts.length - 1; index += 1) {
      if (!fixtureNames.has(parts[index])) {
        continue;
      }
      if (parts[index] === 'examples' && index !== 0 && !file.includes('/commands/js-exec/examples/')) {
        continue;
      }
      const root = parts.slice(0, index + 1).join('/');
      const current = roots.get(root) ?? {
        root,
        packageId: packageIdFor(root),
        domain: domainForPath(root),
        files: 0,
      };
      current.files += 1;
      roots.set(root, current);
      break;
    }
  }

  return [...roots.values()].sort((left, right) => left.root.localeCompare(right.root));
}

function defaultSourceRow(relativePath) {
  return {
    packageId: packageIdFor(relativePath),
    domain: domainForPath(relativePath),
    file: relativePath,
    status: 'portable-pending',
    owner: ownerForPath(relativePath),
    notes:
      'Inventory-only source row; classify only with a named Rust test or an explicit documented exception.',
  };
}

function sourceRowWithOverride(relativePath, existingRows) {
  const row = defaultSourceRow(relativePath);
  const existing = existingRows.get(relativePath);
  const merged = existing
    ? {
        ...row,
        status: existing.Status || row.status,
        owner: existing['Rust owner crate/module or exception'] || row.owner,
        notes: existing.Notes || row.notes,
      }
    : row;
  const jb06Override = sourceOverrideFor(relativePath);
  return jb06Override ? { ...merged, ...jb06Override } : merged;
}

function defaultCaseRow(testCase) {
  return {
    packageId: packageIdFor(testCase.file),
    domain: domainForPath(testCase.file),
    file: testCase.file,
    line: testCase.line,
    suite: testCase.suite,
    name: testCase.name,
    declaration: testCase.declaration,
    status: 'portable-pending',
    owner: ownerForPath(testCase.file),
    rustTest: ownerForPath(testCase.file),
    notes:
      'Inventory-only pending row; do not execute host shell commands as a Just Bash fallback.',
  };
}

function caseRowWithOverride(testCase, existingRows) {
  const row = defaultCaseRow(testCase);
  const existing = existingRows.get(testCaseKey(testCase));
  const merged = existing
    ? {
        ...row,
        status: existing.Status || row.status,
        owner: existing['Rust owner crate/module'] || row.owner,
        rustTest: existing['Rust test name or exception'] || row.rustTest,
        notes: existing.Notes || row.notes,
      }
    : row;
  const rustRunnerProof = readRustRunnerProofs().byLedgerKey.get(testCaseKey(testCase));
  if (rustRunnerProof) {
    return { ...merged, ...rustRunnerProof };
  }
  const jb03Override = jb03CaseOverrides.get(`${testCase.file}:${testCase.line}`);
  if (jb03Override) {
    return { ...merged, ...jb03Override };
  }
  const jb06Override = caseOverrideFor(testCase);
  return jb06Override ? { ...merged, ...jb06Override } : merged;
}

function countBy(rows, keyFn, seedFn) {
  const counts = new Map();
  for (const row of rows) {
    const key = keyFn(row);
    const current = counts.get(key) ?? seedFn(row);
    current.total += 1;
    current[row.status] = (current[row.status] ?? 0) + 1;
    counts.set(key, current);
  }
  return [...counts.values()].sort((left, right) =>
    `${left.packageId}:${left.domain}`.localeCompare(`${right.packageId}:${right.domain}`)
  );
}

function summarizeTestFiles(caseRows) {
  const files = new Map();
  for (const row of caseRows) {
    const key = row.file;
    const current = files.get(key) ?? {
      packageId: row.packageId,
      domain: row.domain,
      file: row.file,
      cases: 0,
      'portable-pending': 0,
      'portable-verified': 0,
      'js-only-documented': 0,
      'type-system-impossible': 0,
      owners: new Set(),
    };
    current.cases += 1;
    current[row.status] += 1;
    current.owners.add(row.owner);
    files.set(key, current);
  }
  return [...files.values()].sort((left, right) => left.file.localeCompare(right.file));
}

function strictGaps(caseRows, rustTests) {
  const gaps = [];
  for (const row of caseRows) {
    if (row.status === 'portable-pending') {
      gaps.push({
        ...row,
        reason: 'portable-pending',
      });
      continue;
    }
    if (row.status === 'portable-verified') {
      const names = splitRustTestNames(row.rustTest);
      if (names.length === 0 || names.some(isPendingMarker)) {
        gaps.push({ ...row, reason: 'missing-named-rust-test' });
        continue;
      }
      for (const name of names) {
        if (!rustTests.has(name)) {
          gaps.push({ ...row, reason: `rust-test-not-found:${name}` });
        }
      }
    }
  }
  return gaps;
}

function validateRows(sourceRows, caseRows) {
  const errors = [];
  for (const row of sourceRows) {
    if (!validStatuses.has(row.status)) {
      errors.push(`${row.file}: invalid source status "${row.status}"`);
    }
    if (!row.owner) {
      errors.push(`${row.file}: missing source owner or exception`);
    }
    if (
      (row.status === 'js-only-documented' || row.status === 'type-system-impossible') &&
      !row.notes
    ) {
      errors.push(`${row.file}: exception source row must carry notes`);
    }
  }
  for (const row of caseRows) {
    if (!validStatuses.has(row.status)) {
      errors.push(`${row.file}:${row.line}: invalid case status "${row.status}"`);
    }
    if (!row.owner) {
      errors.push(`${row.file}:${row.line}: missing case owner`);
    }
    if (strictPassStatuses.has(row.status) && !row.rustTest) {
      errors.push(`${row.file}:${row.line}: closed case row must name a Rust test or exception`);
    }
    if (
      (row.status === 'js-only-documented' || row.status === 'type-system-impossible') &&
      !row.notes
    ) {
      errors.push(`${row.file}:${row.line}: exception case row must carry notes`);
    }
  }
  return errors;
}

function buildInventory() {
  if (!fs.existsSync(upstreamRoot)) {
    fail(
      `upstream path not found: ${upstreamRoot}; run npx opensrc fetch https://github.com/vercel-labs/just-bash`
    );
  }

  const existing = readExistingLedger();
  const allFiles = walk(upstreamRoot).map(upstreamRelative).sort();
  const manifests = allFiles.filter((file) => file.endsWith('package.json'));
  const tsFiles = allFiles.filter((file) => codeFilePattern.test(file));
  const testFiles = tsFiles.filter((file) => testFilePattern.test(file));
  const testFileSet = new Set(testFiles);
  const sourceFiles = tsFiles.filter((file) => !testFileSet.has(file));

  const sourceRows = sourceFiles.map((file) => sourceRowWithOverride(file, existing.sourceRows));
  const caseRows = testFiles.flatMap((file) =>
    extractTestCases(file).map((testCase) => caseRowWithOverride(testCase, existing.caseRows))
  );

  return {
    allFiles,
    manifests,
    tsFiles,
    testFiles,
    sourceFiles,
    sourceRows,
    caseRows,
    fixtureRoots: findFixtureRoots(allFiles),
  };
}

function renderMarkdown(inventory, rustTests, gaps) {
  const sourceSummary = countBy(
    inventory.sourceRows,
    (row) => `${row.packageId}|${row.domain}`,
    (row) => ({
      packageId: row.packageId,
      domain: row.domain,
      total: 0,
      'portable-pending': 0,
      'portable-verified': 0,
      'js-only-documented': 0,
      'type-system-impossible': 0,
    })
  );
  const caseSummary = countBy(
    inventory.caseRows,
    (row) => `${row.packageId}|${row.domain}`,
    (row) => ({
      packageId: row.packageId,
      domain: row.domain,
      total: 0,
      'portable-pending': 0,
      'portable-verified': 0,
      'js-only-documented': 0,
      'type-system-impossible': 0,
    })
  );
  const testFileSummary = summarizeTestFiles(inventory.caseRows);

  const packageIds = new Set([
    ...inventory.manifests.map(packageIdFor),
    ...inventory.sourceRows.map((row) => row.packageId),
    ...inventory.caseRows.map((row) => row.packageId),
  ]);
  const manifestByPackage = new Map(
    inventory.manifests.map((manifest) => [packageIdFor(manifest), manifest])
  );
  const packageInventory = [...packageIds].sort().map((packageId) => {
    const manifest = manifestByPackage.get(packageId);
    const sourceCount = inventory.sourceRows.filter((row) => row.packageId === packageId).length;
    const testFileCount = inventory.testFiles.filter((file) => packageIdFor(file) === packageId).length;
    const caseCount = inventory.caseRows.filter((row) => row.packageId === packageId).length;
    return [
      packageId,
      manifest ?? '(none)',
      manifest ? packageName(manifest) : '(none)',
      sourceCount,
      testFileCount,
      caseCount,
      ownerForPath(`${packageId}/`),
    ];
  });

  const pendingCases = inventory.caseRows.filter((row) => row.status === 'portable-pending').length;
  const verifiedCases = inventory.caseRows.filter((row) => row.status === 'portable-verified').length;
  const jsOnlyCases = inventory.caseRows.filter((row) => row.status === 'js-only-documented').length;
  const typeSystemCases = inventory.caseRows.filter((row) => row.status === 'type-system-impossible').length;
  const gapSummary = countBy(
    gaps,
    (row) => `${row.owner}|${row.reason}`,
    (row) => ({
      packageId: row.owner,
      domain: row.reason,
      total: 0,
      'portable-pending': 0,
      'portable-verified': 0,
      'js-only-documented': 0,
      'type-system-impossible': 0,
    })
  );

  const lines = [
    '# Just Bash Upstream Parity',
    '',
    'This ledger is generated from the refreshed upstream Just Bash mirror and is the JB-01 inventory gate for future Rust implementation buckets. It is also the JBC conformance ledger used by the JS dual-engine harness and Rust corpus runner plan in `docs/open-agents/just-bash-conformance.md`.',
    '',
    'Rows are intentionally fail-closed: no row is verified until a sibling implementation bucket maps it to a named Rust test, a named Rust corpus case, or an explicit documented exception.',
    '',
    '## Source Snapshot',
    '',
    renderTable(
      ['Field', 'Value'],
      [
        ['Upstream repo', upstreamRepo],
        ['Inventory command', 'npx opensrc fetch https://github.com/vercel-labs/just-bash'],
        ['Local source path', upstreamRoot],
        ['Remote HEAD verification', 'git ls-remote https://github.com/vercel-labs/just-bash HEAD refs/heads/main'],
        ['Upstream commit', upstreamHead],
        ['Inventory date', inventoryDate],
        ['Package manifests', inventory.manifests.length],
        ['TS/TSX files outside dist/vendor/node_modules', inventory.tsFiles.length],
        ['Non-test TS/TSX source files', inventory.sourceFiles.length],
        ['Test files', inventory.testFiles.length],
        ['Test cases', inventory.caseRows.length],
        ['Portable pending cases', pendingCases],
        ['Portable verified cases', verifiedCases],
        ['JS-only documented cases', jsOnlyCases],
        ['Type-system impossible cases', typeSystemCases],
        ['Strict gate gaps', gaps.length],
        ['Inventory check command', 'node scripts/just-bash-test-inventory.mjs --check'],
        ['Strict gate command', 'node scripts/just-bash-test-inventory.mjs --strict'],
        ['Conformance plan', 'docs/open-agents/just-bash-conformance.md'],
      ]
    ),
    '',
    '## Status Rules',
    '',
    '- `portable-pending`: Rust ownership is identified, but no named Rust test closes the upstream row yet. This is not parity.',
    '- `portable-verified`: the row names one or more existing Rust `#[test]` / `#[tokio::test]` functions or generated conformance corpus-case proof names in `Rust test name or exception`.',
    '- `js-only-documented`: the row is explicitly excluded because it only verifies JavaScript packaging, browser bundling, Vitest harness behavior, or other non-Rust-runtime behavior. The notes column must explain why.',
    '- `type-system-impossible`: the row only verifies TypeScript type-system behavior that cannot become a Rust runtime test. The notes column must explain why.',
    '',
    'Do not classify missing behavior as nonportable. Until a sibling thread proves an exception, keep the row `portable-pending`. JB-01 does not execute host shell commands as a Just Bash fallback; this bucket only inventories upstream behavior.',
    '',
    '## Gate Rules',
    '',
    '- The refreshed upstream count is expected to be exactly 8 package manifests, 908 TS/TSX files outside `dist`, `vendor`, and `node_modules`, and 485 test files.',
    '- `--check` is the non-blocking inventory gate. It fails for upstream drift, stale generated markdown, invalid statuses, missing owners, or undocumented exceptions.',
    '- `--strict` is the implementation gate. It additionally fails when any `portable-pending` test case remains or when a `portable-verified` row names a Rust test or generated corpus-case proof that does not exist in the workspace.',
    '- Extra Rust tests are additive. They do not close an upstream row unless the row names the Rust test.',
    '- `scripts/master-parity-gate.sh --check` runs this ledger in non-strict mode now; set `JUST_BASH_STRICT_GATE=1` only after JBC-08 closes every portable row.',
    '',
    '## Conformance Harness Contract',
    '',
    '- The JS dual-engine harness must execute the same upstream TypeScript Just Bash test case against the upstream TypeScript engine and the Rust-backed JS/NAPI engine, then record normalized expectations for the Rust corpus runner.',
    '- The Rust corpus runner must expose every portable upstream case as a named Rust test or corpus case before the row can become `portable-verified`.',
    '- Harness, corpus, and hand-written tests must all write back to this generated ledger by exact upstream file, line, declaration, and case title. A broad smoke test is not enough to close an exact upstream row.',
    '- Rows that cannot run in Rust must stay `portable-pending` until they are proven and documented as `js-only-documented` or `type-system-impossible`.',
    '',
    '## Package Inventory',
    '',
    renderTable(
      ['Package id', 'Manifest', 'Package name', 'Non-test TS/TSX files', 'Test files', 'Test cases', 'Default Rust owner'],
      packageInventory
    ),
    '',
    '## Fixture Coverage',
    '',
    renderTable(
      ['Fixture root', 'Package', 'Domain', 'Files'],
      inventory.fixtureRoots.map((row) => [row.root, row.packageId, row.domain, row.files])
    ),
    '',
    '## Source Summary',
    '',
    renderTable(
      ['Package', 'Domain', 'Source files', 'Portable pending', 'Portable verified', 'JS-only documented', 'Type-system impossible'],
      sourceSummary.map((row) => [
        row.packageId,
        row.domain,
        row.total,
        row['portable-pending'],
        row['portable-verified'],
        row['js-only-documented'],
        row['type-system-impossible'],
      ])
    ),
    '',
    '## Test Case Summary',
    '',
    renderTable(
      ['Package', 'Domain', 'Cases', 'Portable pending', 'Portable verified', 'JS-only documented', 'Type-system impossible'],
      caseSummary.map((row) => [
        row.packageId,
        row.domain,
        row.total,
        row['portable-pending'],
        row['portable-verified'],
        row['js-only-documented'],
        row['type-system-impossible'],
      ])
    ),
    '',
    '## Strict Gap Summary',
    '',
    renderTable(
      ['Rust owner crate/module', 'Gap reason', 'Rows'],
      gapSummary.map((row) => [row.packageId, row.domain, row.total])
    ),
    '',
    '## Test File Inventory',
    '',
    renderTable(
      ['Package', 'Domain', 'Upstream test file', 'Cases', 'Portable pending', 'Portable verified', 'JS-only documented', 'Type-system impossible', 'Rust owner summary'],
      testFileSummary.map((row) => [
        row.packageId,
        row.domain,
        row.file,
        row.cases,
        row['portable-pending'],
        row['portable-verified'],
        row['js-only-documented'],
        row['type-system-impossible'],
        [...row.owners].sort().join('; '),
      ])
    ),
    '',
    '## Source File Inventory',
    '',
    renderTable(
      ['Package', 'Domain', 'Upstream source file', 'Status', 'Rust owner crate/module or exception', 'Notes'],
      inventory.sourceRows.map((row) => [
        row.packageId,
        row.domain,
        row.file,
        row.status,
        row.owner,
        row.notes,
      ])
    ),
    '',
    '## Test Case Inventory',
    '',
    renderTable(
      ['Package', 'Domain', 'Upstream test file', 'Line', 'Suite', 'Case', 'Declaration', 'Status', 'Rust owner crate/module', 'Rust test name or exception', 'Notes'],
      inventory.caseRows.map((row) => [
        row.packageId,
        row.domain,
        row.file,
        row.line,
        row.suite,
        row.name,
        row.declaration,
        row.status,
        row.owner,
        row.rustTest,
        row.notes,
      ])
    ),
  ];

  return `${lines.join('\n')}\n`;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const inventory = buildInventory();
  const rustTests = discoverRustTests();
  const gaps = strictGaps(inventory.caseRows, rustTests);
  const validationErrors = validateRows(inventory.sourceRows, inventory.caseRows);

  if (inventory.manifests.length !== expectedManifestCount) {
    validationErrors.push(
      `expected ${expectedManifestCount} package manifests, found ${inventory.manifests.length}`
    );
  }
  if (inventory.tsFiles.length !== expectedTsFileCount) {
    validationErrors.push(
      `expected ${expectedTsFileCount} TS/TSX files, found ${inventory.tsFiles.length}`
    );
  }
  if (inventory.testFiles.length !== expectedTestFileCount) {
    validationErrors.push(
      `expected ${expectedTestFileCount} test files, found ${inventory.testFiles.length}`
    );
  }

  if (options.dryRun) {
    console.log(
      JSON.stringify(
        {
          upstreamRoot,
          upstreamHead,
          manifests: inventory.manifests.length,
          tsFiles: inventory.tsFiles.length,
          sourceFiles: inventory.sourceFiles.length,
          testFiles: inventory.testFiles.length,
          testCases: inventory.caseRows.length,
          strictGaps: gaps.length,
          validationErrors,
        },
        null,
        2
      )
    );
    return;
  }

  const markdown = renderMarkdown(inventory, rustTests, gaps);

  if (options.check || options.strict) {
    const current = fs.existsSync(outputPath) ? fs.readFileSync(outputPath, 'utf8') : '';
    if (current !== markdown) {
      validationErrors.push(
        `${repositoryRelative(outputPath)} is stale; run node scripts/just-bash-test-inventory.mjs`
      );
    }
  } else {
    fs.mkdirSync(path.dirname(outputPath), { recursive: true });
    fs.writeFileSync(outputPath, markdown);
  }

  if (validationErrors.length > 0) {
    console.error('Just Bash inventory validation failed:');
    for (const error of validationErrors) {
      console.error(`  - ${error}`);
    }
    process.exit(1);
  }

  const summary =
    `${inventory.manifests.length} manifests; ${inventory.tsFiles.length} TS/TSX files; ` +
    `${inventory.sourceFiles.length} non-test source files; ${inventory.testFiles.length} test files; ` +
    `${inventory.caseRows.length} test cases; ${gaps.length} strict gap(s)`;

  if (options.strict && gaps.length > 0) {
    console.error(`Just Bash strict inventory gate failed: ${summary}`);
    const sample = gaps.slice(0, 20);
    for (const gap of sample) {
      console.error(
        `  - ${gap.owner}: ${gap.file}:${gap.line} ${gap.declaration}(${JSON.stringify(gap.name)}) [${gap.reason}]`
      );
    }
    if (gaps.length > sample.length) {
      console.error(`  - ... ${gaps.length - sample.length} more strict gap(s)`);
    }
    process.exit(1);
  }

  if (options.check || options.strict) {
    console.log(`Just Bash inventory check passed: ${summary}`);
    return;
  }

  console.log(`Wrote ${repositoryRelative(outputPath)}: ${summary}`);
}

main();
