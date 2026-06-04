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

const jbc27SandboxSourceFiles = [
  'packages/just-bash/src/sandbox/Command.ts',
  'packages/just-bash/src/sandbox/Sandbox.ts',
  'packages/just-bash/src/sandbox/index.ts',
];

const jbc27SecurityPolicySourceFiles = [
  'packages/just-bash/src/security/index.ts',
  'packages/just-bash/src/security/security-violation-logger.ts',
  'packages/just-bash/src/security/types.ts',
];

const jbc27FuzzingSourceFiles = [
  'packages/just-bash/src/security/fuzzing/config.ts',
  'packages/just-bash/src/security/fuzzing/corpus/known-attacks.ts',
  'packages/just-bash/src/security/fuzzing/coverage/coverage-tracker.ts',
  'packages/just-bash/src/security/fuzzing/coverage/feature-coverage.ts',
  'packages/just-bash/src/security/fuzzing/coverage/index.ts',
  'packages/just-bash/src/security/fuzzing/coverage/known-features.ts',
  'packages/just-bash/src/security/fuzzing/generators/coverage-boost-generator.ts',
  'packages/just-bash/src/security/fuzzing/generators/flag-driven-generator.ts',
  'packages/just-bash/src/security/fuzzing/generators/grammar-generator.ts',
  'packages/just-bash/src/security/fuzzing/generators/index.ts',
  'packages/just-bash/src/security/fuzzing/generators/malformed-generator.ts',
  'packages/just-bash/src/security/fuzzing/index.ts',
  'packages/just-bash/src/security/fuzzing/oracles/assertions.ts',
  'packages/just-bash/src/security/fuzzing/oracles/dos-oracle.ts',
  'packages/just-bash/src/security/fuzzing/oracles/sandbox-oracle.ts',
  'packages/just-bash/src/security/fuzzing/runners/fuzz-runner.ts',
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

const jbc28RuntimeOnlySourceFiles = [
  'packages/just-bash/src/commands/js-exec/fetch-polyfill.ts',
  'packages/just-bash/src/commands/js-exec/module-shims.ts',
  'packages/just-bash/src/commands/js-exec/path-polyfill.ts',
  'packages/just-bash/src/commands/worker-bridge/bridge-handler.ts',
  'packages/just-bash/src/commands/worker-bridge/protocol.ts',
  'packages/just-bash/src/commands/worker-bridge/sync-backend.ts',
];

const jbc28JsRuntimeOnlyTestFiles = [
  'packages/just-bash/src/commands/js-exec/js-exec.exec.test.ts',
  'packages/just-bash/src/commands/js-exec/js-exec.node-compat.test.ts',
  'packages/just-bash/src/commands/python3/fs-bridge-handler.output-limit.test.ts',
  'packages/just-bash/src/commands/worker-bridge/bridge-handler.test.ts',
  'packages/just-bash/src/security/attacks/js-exec-exploit-regression.test.ts',
  'packages/just-bash/src/security/attacks/js-exec-host-runtime-breakout-probes.test.ts',
  'packages/just-bash/src/security/attacks/js-exec-recursion-guard-bypass.test.ts',
];

const jbc14JsDefenseSourceFiles = [
  'packages/just-bash/src/security/blocked-globals.ts',
  'packages/just-bash/src/security/defense-context.ts',
  'packages/just-bash/src/security/defense-in-depth-box.ts',
  'packages/just-bash/src/security/trusted-globals.ts',
];

const jbc14SandboxWorkerOnlyTestFiles = [
  'packages/just-bash/src/security/sandbox/python-sqlite-information-disclosure.test.ts',
  'packages/just-bash/src/security/sandbox/worker-protocol-runtime-desync.test.ts',
];

const jbc14SandboxEscapePortableLines = [
  21, 28, 33, 40, 50, 60, 66, 73, 96, 102, 116, 140, 267, 276, 286, 378,
];

const jbc14InformationDisclosurePortableLines = [
  28, 46, 65, 73, 81, 89, 97, 150, 158, 166, 174, 201, 209, 264, 272, 280,
  287, 439, 501, 508, 517, 556, 564,
];

const jb06SourceGroups = [
  {
    files: jbc27SandboxSourceFiles,
    status: 'portable-verified',
    owner: 'crates/just-bash::security::sandbox-facade',
    notes:
      'JBC-27 maps the portable Sandbox facade contract to JustBashSession virtual cwd/env/files, command metadata, scoped env restoration, and timeout side-effect tests.',
  },
  {
    files: jbc27SecurityPolicySourceFiles,
    status: 'portable-verified',
    owner: 'crates/just-bash::security::policy',
    notes:
      'JBC-27 maps the security policy/type/logger surface to deterministic Rust diagnostics, redaction, command policy, and bounded violation log tests.',
  },
  {
    files: jbc27FuzzingSourceFiles,
    status: 'portable-verified',
    owner: 'crates/just-bash::security::fuzzing',
    notes:
      'JBC-27 maps the portable fuzz corpus and oracle surface to deterministic Rust malformed-input, known-attack, numeric-edge, and no-host-leak probes.',
  },
  {
    files: jbc14JsDefenseSourceFiles,
    status: 'js-only-documented',
    owner: 'crates/just-bash::security::runtime-classification',
    notes:
      'JBC-14 classifies Node global monkey-patching, AsyncLocalStorage defense context, and pre-captured JS globals as JavaScript host-runtime defenses; Rust maps the portable policy surfaces separately.',
  },
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
  {
    files: jbc28RuntimeOnlySourceFiles,
    status: 'js-only-documented',
    owner: 'crates/just-bash::security::runtime-classification',
    notes:
      'JBC-28 classifies JavaScript polyfill, Node compatibility, and SharedArrayBuffer worker bridge source as optional JavaScript host-runtime behavior; Rust keeps the optional runtime absent and verifies no host fallback separately.',
  },
];

const jb06CaseGroups = [
  {
    file: 'packages/just-bash/src/security/sandbox/command-security.test.ts',
    lines: [103],
    status: 'portable-verified',
    owner: 'crates/just-bash::security::sandbox',
    rustTest: 'just_bash_security_sandbox_command_rows_are_virtual_and_registry_bound',
    notes:
      'JBC-14 verifies the PATH-hijack row by showing /bin-prefixed commands still resolve through the Rust registry and fail closed when not registered; the remaining command-family rows stay pending with command owners.',
  },
  {
    file: 'packages/just-bash/src/security/sandbox/sandbox-escape.test.ts',
    lines: jbc14SandboxEscapePortableLines,
    status: 'portable-verified',
    owner: 'crates/just-bash::security::sandbox',
    rustTest:
      'just_bash_security_sandbox_command_rows_are_virtual_and_registry_bound; just_bash_security_sandbox_dynamic_rows_fail_closed_or_stay_in_process; just_bash_security_sandbox_information_disclosure_rows_do_not_expose_host_state; just_bash_security_fuzzing_attack_oracles_are_ported_to_rust',
    notes:
      'JBC-14 verifies these exact portable sandbox rows with Rust executions that use the in-memory filesystem, isolated env/session state, registry-bound command resolution, and fail-closed attack probes. Parser, process-info, device, and broader command-family rows not exercised here remain pending.',
  },
  {
    file: 'packages/just-bash/src/security/sandbox/information-disclosure.test.ts',
    lines: jbc14InformationDisclosurePortableLines,
    status: 'portable-verified',
    owner: 'crates/just-bash::security::sandbox',
    rustTest: 'just_bash_security_sandbox_information_disclosure_rows_do_not_expose_host_state',
    notes:
      'JBC-14 verifies these exact non-disclosure rows against Rust output and errors, including host path, env, process, network-tool, history, hostname, and source-fail-closed checks. Date, tar/stat metadata, JS object-shape, and other unexercised rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/security/sandbox/error-forwarding-runtime-leak-probe.test.ts',
    lines: [43, 63],
    status: 'js-only-documented',
    owner: 'crates/just-bash::security::runtime-classification',
    rustTest: 'just_bash_optional_runtime_security_cases_are_classified_nonportable',
    notes:
      'JBC-14 classifies Python and SQLite worker error-forwarding leak probes as JavaScript/WASM worker runtime behavior; the Rust backend has no Python/SQLite worker bridge, and portable redaction is verified separately.',
  },
  {
    files: jbc14SandboxWorkerOnlyTestFiles,
    status: 'js-only-documented',
    owner: 'crates/just-bash::security::runtime-classification',
    rustTest: 'just_bash_optional_runtime_security_cases_are_classified_nonportable',
    notes:
      'JBC-14 classifies Python/SQLite information-disclosure and worker protocol desync probes as JavaScript/WASM worker runtime behavior; Rust has no worker bridge to desynchronize, while portable redaction and sandbox policy are verified separately.',
  },
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
      'just_bash_security_resource_limits_match_upstream_limit_diagnostics; just_bash_security_timeout_and_abort_share_cancellation_contract; just_bash_security_allow_deny_policy_blocks_denied_and_unknown_commands; just_bash_security_jbc27_sandbox_facade_rows_use_virtual_session_contract',
    notes:
      'JB-06/JBC-27 verifies deterministic resource, timeout, cancellation, command-limit, timeout side-effect, and diagnostic seams without executing a host shell.',
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

const jbc27AttackPortableTestFiles = [
  'packages/just-bash/src/security/attacks/awk-getline-piping-security.test.ts',
  'packages/just-bash/src/security/attacks/filename-attacks.test.ts',
  'packages/just-bash/src/security/attacks/find-exec-quoting-injection.test.ts',
  'packages/just-bash/src/security/attacks/fuzz-discovered-attacks.test.ts',
  'packages/just-bash/src/security/attacks/injection-attacks.test.ts',
  'packages/just-bash/src/security/attacks/nested-exec-command-injection.test.ts',
  'packages/just-bash/src/security/attacks/numeric-edge-cases.test.ts',
  'packages/just-bash/src/security/attacks/query-engine-defense-violation-probes.test.ts',
  'packages/just-bash/src/security/attacks/query-engine-js-rce-format-variants.test.ts',
  'packages/just-bash/src/security/attacks/query-engine-js-rce-variants.test.ts',
  'packages/just-bash/src/security/attacks/tar-hostile-codecs.test.ts',
  'packages/just-bash/src/security/attacks/timeout-post-timeout-side-effect.test.ts',
  'packages/just-bash/src/security/attacks/timeout-signal-propagation-gaps.test.ts',
  'packages/just-bash/src/security/attacks/timeout-stdin-forwarding.test.ts',
  'packages/just-bash/src/security/attacks/yq-js-tag-function-probe.test.ts',
];

const jbc27JsRuntimeAttackTestFiles = [
  'packages/just-bash/src/security/attacks/defense-context-invariant.test.ts',
  'packages/just-bash/src/security/attacks/defense-dynamic-import-builtin.test.ts',
  'packages/just-bash/src/security/attacks/defense-in-depth-bypass-hypotheses.test.ts',
  'packages/just-bash/src/security/attacks/defense-in-depth-combined-chain.test.ts',
  'packages/just-bash/src/security/attacks/defense-in-depth-independence.test.ts',
  'packages/just-bash/src/security/attacks/defense-in-depth-timing-confusion.test.ts',
  'packages/just-bash/src/security/attacks/js-exec-exploit-regression.test.ts',
  'packages/just-bash/src/security/attacks/js-exec-host-runtime-breakout-probes.test.ts',
  'packages/just-bash/src/security/attacks/js-exec-recursion-guard-bypass.test.ts',
  'packages/just-bash/src/security/attacks/proxy-trap-completeness.test.ts',
  'packages/just-bash/src/security/defense-in-depth-box-concurrent.test.ts',
  'packages/just-bash/src/security/defense-in-depth-box.test.ts',
  'packages/just-bash/src/security/defense-in-depth-exploit-regression.test.ts',
  'packages/just-bash/src/security/defense-in-depth-hardening.test.ts',
  'packages/just-bash/src/security/symbol-locking.test.ts',
];

const jbc27PrototypePollutionTestFiles = [
  'packages/just-bash/src/security/prototype-pollution/prototype-pollution-awk.test.ts',
  'packages/just-bash/src/security/prototype-pollution/prototype-pollution-bash-extended.test.ts',
  'packages/just-bash/src/security/prototype-pollution/prototype-pollution-comprehensive.test.ts',
  'packages/just-bash/src/security/prototype-pollution/prototype-pollution-edge-cases.test.ts',
  'packages/just-bash/src/security/prototype-pollution/prototype-pollution-sed.test.ts',
  'packages/just-bash/src/security/prototype-pollution/prototype-pollution-syntax-features.test.ts',
];

const jbc27FuzzingTestFiles = [
  'packages/just-bash/src/security/fuzzing/__tests__/fuzz-coverage.test.ts',
  'packages/just-bash/src/security/fuzzing/__tests__/fuzz-dos.test.ts',
  'packages/just-bash/src/security/fuzzing/__tests__/fuzz-malformed.test.ts',
  'packages/just-bash/src/security/fuzzing/__tests__/fuzz-sandbox.test.ts',
  'packages/just-bash/src/security/fuzzing/generators/grammar-generator.test.ts',
];

const jbc27SandboxPortableTestFiles = [
  'packages/just-bash/src/sandbox/Sandbox.test.ts',
  'packages/just-bash/src/security/sandbox/command-security.test.ts',
  'packages/just-bash/src/security/sandbox/dynamic-execution.test.ts',
  'packages/just-bash/src/security/sandbox/information-disclosure.test.ts',
  'packages/just-bash/src/security/sandbox/sandbox-escape.test.ts',
];

const jbc27CaseGroups = [
  {
    files: jbc27AttackPortableTestFiles,
    status: 'portable-verified',
    owner: 'crates/just-bash::security::attack-corpus',
    rustTest:
      'just_bash_security_jbc27_attack_corpus_paths_and_injection_rows_are_virtualized; just_bash_security_jbc27_fuzz_oracle_malformed_inputs_never_leak_host_state; just_bash_security_jbc27_prototype_pollution_keywords_remain_plain_data; just_bash_security_jbc27_sandbox_facade_rows_use_virtual_session_contract',
    notes:
      'JBC-27 maps these portable attack rows to Rust virtual filesystem, command-injection, malformed-input, numeric-edge, timeout side-effect, structured-query, and no-host-leak probes.',
  },
  {
    file: 'packages/just-bash/src/security/attacks/code-exec-exploit-regression.test.ts',
    lines: [18, 36, 53],
    status: 'portable-verified',
    owner: 'crates/just-bash::security::attack-corpus',
    rustTest:
      'just_bash_security_jbc27_attack_corpus_paths_and_injection_rows_are_virtualized; just_bash_security_jbc27_prototype_pollution_keywords_remain_plain_data',
    notes:
      'JBC-27 maps the portable AWK, SQLite, and structured-query code-exec regression rows to fail-closed Rust command execution and structured-data prototype-key probes.',
  },
  {
    file: 'packages/just-bash/src/security/attacks/code-exec-exploit-regression.test.ts',
    lines: [73],
    status: 'js-only-documented',
    owner: 'crates/just-bash::security::runtime-classification',
    rustTest: 'just_bash_optional_runtime_security_cases_are_classified_nonportable',
    notes:
      'JBC-27 classifies the Python worker escape row as JavaScript/WASM worker hardening; the Rust backend has no Python worker bridge.',
  },
  {
    files: jbc27JsRuntimeAttackTestFiles,
    status: 'js-only-documented',
    owner: 'crates/just-bash::security::runtime-classification',
    rustTest: 'just_bash_optional_runtime_security_cases_are_classified_nonportable',
    notes:
      'JBC-27 classifies these Node/JavaScript defense-in-depth, AsyncLocalStorage, dynamic import, process/global monkey-patch, symbol-locking, and js-exec worker rows as host-runtime hardening outside the portable Rust backend.',
  },
  {
    files: jbc27PrototypePollutionTestFiles,
    status: 'portable-verified',
    owner: 'crates/just-bash::security::prototype-pollution',
    rustTest: 'just_bash_security_jbc27_prototype_pollution_keywords_remain_plain_data',
    notes:
      'JBC-27 verifies prototype-like keys remain plain Rust map/data keys across env, export, structured query, and AWK variable seams; no JavaScript prototype object exists in the Rust backend.',
  },
  {
    files: jbc27FuzzingTestFiles,
    status: 'portable-verified',
    owner: 'crates/just-bash::security::fuzzing',
    rustTest:
      'just_bash_security_jbc27_fuzz_oracle_malformed_inputs_never_leak_host_state; just_bash_security_jbc27_attack_corpus_paths_and_injection_rows_are_virtualized; just_bash_security_jbc27_prototype_pollution_keywords_remain_plain_data',
    notes:
      'JBC-27 maps fuzz generator, oracle, malformed-input, DOS, sandbox, and known-attack rows to deterministic Rust probes that assert no panic, no host leakage, and bounded output.',
  },
  {
    files: jbc27SandboxPortableTestFiles,
    status: 'portable-verified',
    owner: 'crates/just-bash::security::sandbox',
    rustTest:
      'just_bash_security_jbc27_sandbox_facade_rows_use_virtual_session_contract; just_bash_security_jbc27_attack_corpus_paths_and_injection_rows_are_virtualized; just_bash_security_sandbox_command_rows_are_virtual_and_registry_bound; just_bash_security_sandbox_dynamic_rows_fail_closed_or_stay_in_process; just_bash_security_sandbox_information_disclosure_rows_do_not_expose_host_state',
    notes:
      'JBC-27 maps remaining portable sandbox rows to Rust session facade, registry-bound commands, dynamic execution fail-closed behavior, virtual files, scoped env, timeout side-effect, and non-disclosure probes.',
  },
  {
    file: 'packages/just-bash/src/security/sandbox/error-forwarding-runtime-leak-probe.test.ts',
    lines: [23],
    status: 'portable-verified',
    owner: 'crates/just-bash::security::sandbox',
    rustTest: 'just_bash_security_sandbox_information_disclosure_rows_do_not_expose_host_state',
    notes:
      'JBC-27 maps the portable hard-link error leak probe to Rust no-host-marker information-disclosure checks; Python/SQLite worker leak rows stay JS-only.',
  },
  {
    file: 'packages/just-bash/src/security/security-violation-logger.test.ts',
    status: 'portable-verified',
    owner: 'crates/just-bash::security::policy',
    rustTest: 'just_bash_security_jbc27_policy_and_violation_rows_are_deterministic',
    notes:
      'JBC-27 verifies deterministic violation recording, reverse chronological listing, grouping, retention caps, and clearing with SecurityViolationLog.',
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
  {
    file: 'packages/just-bash/src/commands/awk/awk.errors.test.ts',
    lines: [
      6, 14, 20, 29, 38, 47, 57, 64, 71, 80, 89, 96, 103, 110, 119, 129,
      136, 150, 158, 170, 186, 195, 205, 216, 223, 230, 237, 246, 252, 270,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc09_error_handling_and_type_coercion_rows',
    notes:
      'JBC-09 verifies portable awk error handling: integer/float division by zero (inf), modulo by zero (nan), fail-closed invalid-regex diagnostics for match/gsub/sub, unset scalar/array coercion, string-to-number arithmetic, mixed-type and numeric-string comparison, $0/out-of-bounds/non-integer/extended field access, substr with one argument, sprintf with no/extra/missing format args, sqrt/log/exp math edges (nan/-inf/inf), unmatched-brace and unmatched-paren syntax errors, and the missing-input-file error.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.edge-cases.test.ts',
    lines: [202, 213, 220, 229, 258, 265, 274, 294, 303, 312],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc09_edge_case_control_array_and_special_var_rows',
    notes:
      'JBC-09 verifies portable awk edge cases: case-sensitive regex non-match, empty action block, nested if without else, multiple semicolons, uninitialized variable as number/string, NF reassignment, empty associative-array iteration, numeric-string vs numeric subscript collapse, and delete on an absent key. C-style for/while loops in BEGIN/END remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.errors.test.ts',
    lines: [288, 295],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc09_edge_case_control_array_and_special_var_rows',
    notes:
      'JBC-09 verifies portable awk special-variable edges: NF is 0 for an empty record and NR is 0 inside BEGIN before any record is read.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.expressions.test.ts',
    lines: [6, 15, 24, 34, 46, 60, 74, 86],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_expression_edge_and_error_rows',
    notes:
      'just-bash-command-awk verifies portable awk complex expressions: deeply nested parentheses, compound formulae, quadratic/power arithmetic, if/else-if/else chains, nested bodyless if statements, and nested/complex ternary conditions.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.edge-cases.test.ts',
    lines: [150, 238, 247, 283],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_expression_edge_and_error_rows',
    notes:
      'just-bash-command-awk verifies portable awk control-flow/variable edges: building a long string in a C-style for loop, a for loop with zero iterations, a while with a false condition, and assignment used as an if condition.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.errors.test.ts',
    lines: [143, 179, 258, 302, 311, 322, 329],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_expression_edge_and_error_rows',
    notes:
      'just-bash-command-awk verifies portable awk error/edge handling: negative field index, split() with the array argument omitted, an undefined function call returning empty, NF growing when a high field is set, assigning to NF, and graceful printf handling of an unknown specifier and width/precision with no conversion.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.functions.test.ts',
    lines: [142, 153, 162, 171, 184, 210],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_multiple_rules_and_next_rows',
    notes:
      'just-bash-command-awk verifies portable awk multiple pattern/action rules executing in order, next skipping the remaining rules for a record, the default print action for a pattern-only rule, mixed pattern and action-only rules, and BEGIN/main/END ordering.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.functions.test.ts',
    lines: [
      307, 316, 325, 343, 352, 365, 374, 383, 394, 403, 421, 434, 441, 448, 459,
      468, 479, 490,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_special_vars_string_fns_and_printf_rows',
    notes:
      'just-bash-command-awk verifies portable awk FILENAME (file and empty-for-stdin), FNR resetting per file, match() setting RSTART/RLENGTH and returning position/0, gensub() first/global/Nth replacement, the ^ and ** power operators with a fractional exponent, and printf %x/%X/%o/%c formatting.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.functions.test.ts',
    lines: [523, 534, 545],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_field_iteration_and_fs_rows',
    notes:
      'just-bash-command-awk verifies portable awk -F regex field separators: a bracket character class on digits, a multi-character literal separator, and a punctuation character class.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.fields.test.ts',
    lines: [317, 326, 335],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_field_iteration_and_fs_rows',
    notes:
      'just-bash-command-awk verifies portable awk field iteration with C-style for loops: iterating $i forward over NF, iterating in reverse, and summing all fields.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.expressions.test.ts',
    lines: [114, 132, 149, 460, 498],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_loop_break_continue_rows',
    notes:
      'just-bash-command-awk verifies portable awk C-style loop control: nested for/while, break and continue scoped to the innermost loop, and the fibonacci and string-reversal loop idioms.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.functions.test.ts',
    lines: [247, 256, 265, 274],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_loop_break_continue_rows',
    notes:
      'just-bash-command-awk verifies portable awk break/continue in for and while loops within BEGIN blocks.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.functions.test.ts',
    lines: [285, 294],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_do_while_rows',
    notes:
      'just-bash-command-awk verifies portable awk do-while loops execute the body at least once and re-test the condition after each iteration.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.operators.test.ts',
    lines: [80, 116, 125, 172, 181, 219, 228, 237, 266, 275, 295, 407, 420, 429, 438, 447, 456, 466],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_operator_precedence_and_logic_rows',
    notes:
      'just-bash-command-awk verifies portable awk operator semantics: division-by-zero exit code, the <= and > comparisons, short-circuit && and ||, ~/!~ regex match in conditions and on fields, ternary with expressions/nesting/in print arguments, chained increments, and operator precedence including POSIX unary-minus vs exponent binding (-2^2 == -4).',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.functions.test.ts',
    lines: [412, 499, 510],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_gensub_backreference_and_printf_c_e_rows',
    notes:
      'just-bash-command-awk verifies portable awk gensub() backreferences (\\2 \\1 reorder captured groups), printf %c printing the first character of a string argument, and printf %.2e scientific notation matching JS toExponential (1.23e+3).',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.ternary.test.ts',
    lines: [6, 15, 24, 35, 44, 53, 66, 75, 86, 95, 106, 115, 150, 159, 168],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_ternary_operator_rows',
    notes:
      'just-bash-command-awk verifies portable awk ternary ?: semantics: true/false branch selection, expressions evaluated in the chosen branch, numeric/string/equality comparison conditions, single- and multi-level nested ternary, assignment of the result, ternary inside a compound expression, function calls in the condition and branches, and truthiness of empty strings, non-empty strings, and non-zero numbers. The getline-backed async rows remain pending without getline support.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.modulo.test.ts',
    lines: [13, 20, 27, 45, 65, 83, 94, 105],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_modulo_operator_rows',
    notes:
      'just-bash-command-awk verifies portable awk modulo: exact-division zero result, floating-point modulo, zero dividend, the %= compound assignment between variables, even-number and every-Nth-record filters via $1 % 2 and NR % 3, modulo inside a for loop, and a negative dividend keeping the dividend sign. The negative-divisor row (7 % -3) remains pending unary-minus operand parsing.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.math.test.ts',
    lines: [59, 97],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_atan2_math_rows',
    notes:
      'just-bash-command-awk verifies portable awk atan2(): atan2(0, 1) is exactly 0 and atan2(1, 0) is pi/2. The non-deterministic rand()/srand() rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.patterns.test.ts',
    lines: [
      6, 15, 24, 33, 42, 51, 71, 80, 107, 116, 165, 174, 183, 192, 203, 212,
      221, 232, 243,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_regex_pattern_rows',
    notes:
      'just-bash-command-awk verifies portable awk regex patterns: literal /ana/, ^/$ anchors, [cd] and negated [^a-z] character classes, alternation /red|blue/, expression patterns ($1 ==/!= numeric, string equality, lexicographic comparison), combined NR patterns (NR == 1, NR > 1, range, every Nth via NR % 2), field regex-match patterns ($1 ~/!~ /^a/, $2 ~ /^a/), action-only rule on every record, and pattern-only rule printing matches.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.getline.test.ts',
    lines: [5, 21, 37, 54, 74, 88],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_command_awk_getline_main_input_rows',
    notes:
      'just-bash-command-awk verifies plain `getline` and `getline VAR` reading the next record from the main input stream: getline into $0 re-splits fields, getline into a variable leaves $0/fields intact, NR advances on each successful read and getline-at-EOF is a no-op, getline inside a pattern-matched action skips the consumed record from the main loop, and combining adjacent records with getline. Redirected forms (getline < file, cmd | getline) and getline used as a return-valued expression remain pending.',
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
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/feature.test.ts',
    lines: [
      20, 35, 49, 62, 77, 89, 102, 396, 407, 418, 443, 860, 874, 890,
      904, 917, 931, 945, 974, 990, 1006, 1024,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_feature_filters_stats_pcre_and_ignore_rows_are_portable',
    notes:
      'JBC-10 verifies imported rg feature rows for --no-filename, -o only-matching, -S smart case, -l/-c/--files-without-match, exit codes, -A/-C context max precedence, --stats output (matches/files/bytes), PCRE2 rejection, -f- stdin patterns, --ignore-file, and --no-ignore-vcs over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/misc.test.ts',
    lines: [21],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_feature_filters_stats_pcre_and_ignore_rows_are_portable',
    notes:
      'JBC-10 verifies the imported rg single_file row searches an explicit file without a filename prefix.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/binary.test.ts',
    lines: [18, 32, 44, 71, 86, 101, 117],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_binary_detection_rows_are_portable',
    notes:
      'JBC-10 verifies imported rg binary-detection rows: NUL-containing files are skipped in directory and explicit-file search, in -c counts, in -l file lists, and in mixed-content directories.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/binary.test.ts',
    lines: [56, 134, 145, 156, 167, 180, 201, 214, 227, 240, 255, 270],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_binary_edge_case_and_flag_rows_are_portable',
    notes:
      'JBC-10 verifies imported rg binary edge cases: NUL after the 8KB sample window is still searched, all-NUL/leading/trailing/multiple-NUL files are skipped, common binary signatures (PNG/PDF/ZIP) are skipped, and binary skipping holds under -i, -v, -w, -C, -m, and subdirectory search.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/misc.test.ts',
    lines: [
      127, 144, 161, 178, 212, 231, 246, 331, 364, 486, 503, 538, 590,
      605, 620, 652, 670, 685, 701, 718, 735, 752, 769, 786, 1150, 1181,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_misc_search_modes_counts_and_context_rows_are_portable',
    notes:
      'JBC-10 verifies imported rg misc rows: -v/-n inverted, -i case-insensitive, -w word, -x whole-line, -F literal, -q quiet, -t/-T file-type filter, -g/--glob filters, --count/--count-matches/--include-zero counts, --files-with-matches/--files-without-match, -A/-B/-C context with line numbers, --files listing, and --sort path over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/misc.test.ts',
    lines: [89, 107, 348, 380, 398, 415, 434, 891, 906, 946, 965, 1004],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_misc_type_heading_ignore_and_symlink_rows_are_portable',
    notes:
      'JBC-10 verifies imported rg misc rows: -H forces the filename prefix on a single explicit file, --heading prints the file label heading then unprefixed lines, -t all / -T all match (or negate) any known file type, --type-clear empties a type so it matches nothing, --type-add registers a glob-based or include-composed type, the generic .ignore and ripgrep-specific .rgignore files exclude matching paths, file symlinks are skipped during traversal by default and followed with -L, and -uu (unrestricted2) includes hidden dotfiles over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/rg.no-filename.test.ts',
    lines: [222, 234, 246, 258, 276, 289, 304, 316, 328, 340, 353, 367, 379, 393],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_upstream_no_filename_context_filter_and_regex_rows_are_portable',
    notes:
      'JBC-10 verifies rg -I (no-filename) with -A/-B/-C context (keeping line numbers and context separators, no per-file separator across files), -t/-g filters, --hidden, combined short flags (-Iin/-In), regex alternation and character classes, and -I -N -o piping output over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/regression.test.ts',
    lines: [22, 39, 73, 88, 104, 120, 155, 188, 324, 376, 390, 438, 453, 509, 538, 550],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_regression_gitignore_regex_and_flag_rows_are_portable',
    notes:
      'JBC-10 verifies imported ripgrep regression rows: rooted/unanchored/nested/trailing-slash/double-star/dot-star gitignore patterns, --files with a path argument, IP-style repeated-group and cyrillic case-folding regex, smart-case bracket sensitivity, -e dash patterns, -q quiet exit, --only-matching, and --quiet --files glob exit codes over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/regression.test.ts',
    lines: [285, 468, 716, 730, 742, 1071, 1313, 1385],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest:
      'rg_imported_regression_negation_symlink_anchored_and_exit_code_rows_are_portable',
    notes:
      'JBC-10 verifies additional imported ripgrep regression rows: -L follows file symlinks and skips broken symlinks while searching targets, complex --files glob exclusion plus inclusion, anchored .ignore patterns (/parent/*.txt, trailing-slash /testdir/sub/sub2/) and files-with-matches honouring .ignore from a cwd subdirectory, --no-ignore-dot disabling .ignore/.rgignore filtering, and --hidden --files listing dotfiles while honouring .ignore over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/misc.test.ts',
    lines: [1135],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest:
      'rg_imported_regression_negation_symlink_anchored_and_exit_code_rows_are_portable',
    notes:
      'JBC-10 verifies the imported rg misc -a (text) row searches binary (NUL-containing) file content literally and prints the matching lines over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/gitignore.test.ts',
    lines: [63],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_gitignore_anchoring_and_exit_code_rows_are_portable',
    notes:
      'JBC-10 verifies a non-rooted gitignore pattern that contains a slash (`doc/frotz`) is rooted at the gitignore base: it hides `doc/frotz` but not `a/doc/frotz`.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/regression.test.ts',
    lines: [137, 355, 659, 677, 702, 757, 1174, 1447, 1465],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_gitignore_anchoring_and_exit_code_rows_are_portable',
    notes:
      'JBC-10 verifies imported ripgrep regression rows for base-anchored gitignore semantics (slash-containing or rooted patterns match a path or any descendant, never a free-floating substring deeper in the tree: `/*`+`!/dir` negation, `.a/b`, `/a/b`, `/a/*/b`, `rust/target`, `foobar/debug`), interspersed `-g` glob flags after the positional pattern, an empty `-f` pattern file exiting 1 (no matches) rather than 2 (no pattern given), and `--files-without-match` exit codes plus `-q` quiet suppression over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/feature.test.ts',
    lines: [965, 966, 967, 968, 969, 970, 1047],
    status: 'js-only-documented',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_unsupported_upstream_skip_features_are_rejected_or_classified',
    notes:
      'JBC-10 classifies the upstream `it.skip` rg feature rows that ripgrep/just-bash leave unimplemented: alternate text encodings via -E/--encoding (Shift-JIS, UTF-16 auto/explicit, EUC-JP, unknown, replacement) and -M/--max-columns truncation. The Rust rg port rejects these flags as unrecognized options, matching the upstream skip contract; the Rust test asserts the rejection so it fails if support is ever silently added.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/json.test.ts',
    lines: [186],
    status: 'js-only-documented',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_unsupported_upstream_skip_features_are_rejected_or_classified',
    notes:
      'JBC-10 classifies the upstream `it.skip` JSON row r1412_look_behind_match_missing, which requires PCRE2 look-behind. The Rust rg port rejects PCRE2/look-around regex rather than matching, which the named Rust test asserts.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/misc.test.ts',
    lines: [666, 1071, 1074, 1075, 1097, 1098, 1099, 1100, 1101, 1102, 1103, 1104, 1200, 1201],
    status: 'js-only-documented',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_unsupported_upstream_skip_features_are_rejected_or_classified',
    notes:
      'JBC-10 classifies the upstream `it.skip` misc rg rows for unimplemented features: --no-include-zero count override, --no-column vimgrep output, --pre/--pre-glob file preprocessing, non-gzip compression (bzip2/xz/lz4/lzma/brotli/zstd/uncompress/invalid-gzip via -z/--search-zip), and --sort/--sortr accessed-time ordering that requires real system timestamps. The Rust rg port rejects these flags as unrecognized options, asserted by the named Rust test so it fails if support is ever added.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/regression.test.ts',
    lines: [1091, 1092, 1095, 1140, 1190, 1378, 1381, 1519],
    status: 'js-only-documented',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_unsupported_upstream_skip_features_are_rejected_or_classified',
    notes:
      'JBC-10 classifies the upstream `it.skip` regression rg rows that require unimplemented features: PCRE2 look-ahead/look-behind (r1401_x2, r1412, r1573, r3139), --crlf (r1765), --no-unicode (r2574), and --null-data (r2658). The Rust rg port rejects PCRE2/look-around regex and these flags as unrecognized options, asserted by the named Rust test.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/rg-parser-threads.test.ts',
    lines: [30, 35, 40, 45, 50, 57],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_parser_threads_compat_flag_never_clobbers_max_depth',
    notes:
      'JBC-10 verifies the `-j`/`--threads` ripgrep compatibility flag is a value-consuming no-op that never clobbers max-depth: the Rust port preserves the default depth and any explicit --max-depth set before or after -j, accepts the long --threads form (including --threads=N), and handles the combined short form -Iij <n>. The named Rust test exercises deep virtual-filesystem traversal so it fails if -j ever mutates depth.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/binary.test.ts',
    lines: [292, 293, 294, 295, 296, 297, 298, 299],
    status: 'js-only-documented',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_unsupported_upstream_skip_features_are_rejected_or_classified',
    notes:
      'JBC-10 classifies the upstream `it.skip` rg binary mmap rows (memory map not supported). The virtual-filesystem Rust port has no memory-mapped search path and rejects --mmap/--no-mmap as unrecognized options, asserted by the named Rust test so it fails if mmap support is ever added.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/misc.test.ts',
    lines: [1124, 1125, 1128, 1131],
    status: 'js-only-documented',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_unsupported_upstream_skip_features_are_rejected_or_classified',
    notes:
      'JBC-10 classifies the upstream `it.skip` misc binary rows: binary_convert_mmap, binary_search_mmap, binary_quit_mmap (mmap not relevant) and binary_quit (binary-quit behavior not implemented). The Rust port rejects --mmap/--no-mmap and --binary as unrecognized options, asserted by the named Rust test.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/regression.test.ts',
    lines: [504, 505, 639, 880, 988, 1220, 1239, 1293, 1309],
    status: 'js-only-documented',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_unsupported_upstream_skip_features_are_rejected_or_classified',
    notes:
      'JBC-10 classifies the upstream `it.skip` regression rg rows for unimplemented features: color output (r428 color context path / unrecognized style, r599 color with empty matches via --color/--colors), case-insensitive ignore-file matching (r1164 via --ignore-file-case-insensitive), and multiline/vimgrep/passthru/replacement rows (--multiline/-U, --vimgrep, --passthru, --replace/-r). The Rust port rejects each of these flags as unrecognized options, asserted by the named Rust test so it fails if support is ever added.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/misc.test.ts',
    lines: [38, 55, 844, 858, 875, 921, 986],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_misc_filename_ignore_and_hidden_rows_are_portable',
    notes:
      'JBC-10 verifies imported rg misc rows: directory search with filename prefix, -n line numbers, hidden-file exclusion by default and inclusion with --hidden, .gitignore exclusion, and --no-ignore / -u unrestricted overrides over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/regression.test.ts',
    lines: [56, 303, 340, 564, 579, 625, 643, 772, 945, 1056, 1102, 1117],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_regression_regex_word_and_flag_rows_are_portable',
    notes:
      'JBC-10 verifies imported ripgrep regression rows for regex and flag behavior: gitignore negation after double-star, complex parse/include regex (-N), smart-case word boundaries, word-boundary with spaces, -w -o alternation, -e leading-hyphen patterns, -c ignoring -C context, capture groups, repeated-zero patterns, -A -m context-with-max-count, semicolon/comma regex, and multi-space field matching over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/regression.test.ts',
    lines: [689, 787, 803, 815, 830, 839, 884, 900, 967, 1413, 1490],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_regression_gitignore_files_and_exit_code_rows_are_portable',
    notes:
      'JBC-10 verifies imported ripgrep regression rows for gitignore, file-listing, and exit codes: build-directory negation with -l, a**b non-match, --files-with-matches / --files-without-match listings, invalid-flag and match/no-match/quiet exit codes, ** and **/**/* gitignore non-matches, pattern files without trailing newline, **/bar/* -l non-match, and unclosed character class allowed in gitignore over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/regression.test.ts',
    lines: [247, 594, 606, 916, 929, 1003, 1364, 1429],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_regression_misc_pattern_file_context_and_filter_rows_are_portable',
    notes:
      'JBC-10 verifies imported ripgrep regression rows: full-path gitignore pattern hiding foo/sherlock while keeping foo/watson (r105), repeated -i staying case-insensitive (r553_switch), a later -C 0 overriding an earlier -C 1 to drop the context separator (r553_flag), -F and -x with a pattern file (r1176 literal/line-regex), a bounded-repetition DNA sequence regex (r1319), multiple -e patterns with --only-matching listing each match (r2236), and --stats reporting the bytes-searched summary line (r2770) over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/imported-tests/misc.test.ts',
    lines: [1004, 1167],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_imported_misc_type_list_and_unrestricted_rows_are_portable',
    notes:
      'JBC-10 verifies imported rg misc rows: -uu (unrestricted2) including hidden dotfiles in the search results, and --type-list listing the known file types (rust, py) over the virtual filesystem.',
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

const jbc19SourceGroups = [
  {
    files: [
      'packages/just-bash/src/transform/pipeline.ts',
      'packages/just-bash/src/transform/plugins/tee-plugin.ts',
      'packages/just-bash/src/transform/types.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::transform',
    notes:
      'JBC-19 verifies portable transform pipeline ordering, metadata merging, TeePlugin AST rewriting, target filtering, sanitized timestamp paths, and plugin failure propagation without JS host APIs.',
  },
  {
    files: ['packages/just-bash/src/helpers/shell-quote.ts'],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell-quote',
    notes:
      'JBC-19 verifies shellJoinArgs single-quote escaping and literal argv preservation through the Rust parser/interpreter.',
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
  {
    file: 'packages/just-bash/src/syntax/break-continue.test.ts',
    lines: [6, 19, 34, 49, 64, 72, 85, 98, 113, 129, 139, 153],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbc13_syntax_break_continue_matches_upstream_behavior',
    notes:
      'JBC-13 verifies portable break/continue across for/while/until loops, multi-level break/continue n, no-op outside loops, invalid-arg exit 128, case-in-loop, and subshell containment.',
  },
  {
    file: 'packages/just-bash/src/syntax/set-pipefail.test.ts',
    lines: [6, 17, 28, 39, 52, 64, 78, 91, 105],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbc13_syntax_set_pipefail_matches_upstream_behavior',
    notes:
      'JBC-13 verifies portable set -o pipefail status propagation (first/middle/rightmost stage), +o pipefail disable, errexit interaction, and single-command pipelines.',
  },
  {
    file: 'packages/just-bash/src/syntax/loops.test.ts',
    lines: [79, 117],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbc13_syntax_loop_guard_conditions_match_upstream',
    notes:
      'JBC-13 verifies portable while/until guard conditions that skip the body when initially unsatisfied.',
  },
  {
    file: 'packages/just-bash/src/syntax/control-flow.test.ts',
    lines: [230, 277],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbc13_syntax_local_keyword_scopes_match_upstream',
    notes:
      'JBC-13 verifies portable local-variable scoping: shadowing an outer variable and keeping reassignments within the same function scope.',
  },
  {
    file: 'packages/just-bash/src/syntax/control-flow.test.ts',
    lines: [287, 293, 299, 306, 329],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbc13_syntax_negation_operator_matches_upstream',
    notes:
      'JBC-13 verifies portable ! negation of pipeline status, && / || chaining, and use inside an if condition.',
  },
];

const jbc26SourceGroups = [
  {
    files: [
      'packages/just-bash/src/fs/encoding.ts',
      'packages/just-bash/src/fs/path-utils.ts',
      'packages/just-bash/src/fs/sanitize-error.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::path-encoding-sanitize',
    notes:
      'JBC-26 verifies portable virtual path normalization, encoding conversion, large base64 rendering, and host-path error sanitization in Rust without host filesystem access.',
  },
];

const jbc26CaseGroups = [
  {
    file: 'packages/just-bash/src/comparison-tests/file-operations.comparison.test.ts',
    lines: [21, 34, 46, 67, 78, 93, 104, 123, 135, 147, 160, 186, 198, 212, 224, 253, 265, 276, 299],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::file-ops',
    rustTest: 'jbc26_file_operation_comparison_rows_are_virtual_and_stateful',
    notes:
      'JBC-26 verifies exact mkdir/rm/cp/mv/touch/pwd comparison rows against the stateful Rust virtual filesystem command layer.',
  },
  {
    file: 'packages/just-bash/src/fs/cross-fs-no-symlinks.test.ts',
    lines: [
      95, 106, 127, 133, 142, 146, 157, 161, 172, 176, 182, 192, 198, 209,
      215, 226, 239, 244, 249, 254, 259, 302, 317, 342, 357, 374, 386, 403,
      409, 423, 429, 443, 456, 473, 483, 526, 532, 537, 580, 586, 592, 597,
      607, 613, 617, 627, 637, 643, 652, 656, 662, 671, 679, 687, 704, 711,
      728, 736, 747, 756, 799, 807, 812, 836, 846, 856, 868, 877, 883, 890,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::symlink-policy',
    rustTest:
      'jbc26_symlink_policy_mount_routing_and_overlay_precedence_rows_are_virtual; jbc26_virtual_fs_path_security_encoding_and_error_shape_rows_are_sanitized',
    notes:
      'JBC-26 verifies default-deny symlink creation/traversal/mutation, lstat/readlink visibility, no-host path traversal behavior, null-byte rejection, Unicode/special-name handling, and normal file access through Rust virtual filesystems.',
  },
  {
    file: 'packages/just-bash/src/fs/cross-fs-security.test.ts',
    lines: [
      77, 83, 89, 95, 101, 107, 113, 117, 123, 129, 138, 144, 150, 157,
      166, 171, 177, 188, 194, 199, 205, 217, 226, 243, 262, 282, 308, 313,
      318, 323, 333, 345, 360, 364, 373, 387, 421, 427, 431, 454, 462, 470,
      478, 486, 499, 510, 529, 551, 560, 577, 591, 642, 658, 745, 751, 761,
      776, 794, 801, 813, 840, 867, 881, 899, 909, 922, 931, 946, 951, 956,
      966, 972, 983, 1001, 1007, 1018, 1039, 1051, 1075, 1094, 1113, 1135,
      1169, 1181, 1195, 1220, 1242, 1267, 1305, 1393, 1407, 1418, 1438, 1454,
      1467, 1484, 1493, 1516, 1540, 1559, 1574, 1603, 1622, 1651, 1679, 1709,
      1725, 1745, 1779,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::path-security',
    rustTest:
      'jbc26_virtual_fs_path_security_encoding_and_error_shape_rows_are_sanitized; jbc26_symlink_policy_mount_routing_and_overlay_precedence_rows_are_virtual',
    notes:
      'JBC-26 verifies portable null-byte, traversal clamp, special path, symlink-loop, no-host-mutation, error-shape, read-only, overlay, and virtual mount safety rows. Real host permission and OS-symlink rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/fs/read-write-fs/read-write-fs.security.test.ts',
    lines: [
      52, 58, 65, 71, 79, 84, 89, 95, 105, 113, 120, 132, 140, 154, 158,
      169, 173, 191, 195, 203, 207, 211, 220, 232, 247, 263, 277, 290, 309,
      313, 317, 324, 330, 338, 344, 351, 356, 361, 366, 373, 388, 401, 420,
      424, 430, 438, 445, 449, 455, 554, 573, 616, 654, 659, 936, 941, 953,
      963, 972, 984, 990, 997, 1005, 1009, 1034, 1174, 1191, 1201, 1211,
      1220, 1230, 1239, 1254, 1265, 1277, 1288, 1301, 1314, 1368, 1413, 1459,
      1477,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::read-write-security',
    rustTest:
      'jbc26_virtual_fs_path_security_encoding_and_error_shape_rows_are_sanitized; jbc26_symlink_policy_mount_routing_and_overlay_precedence_rows_are_virtual',
    notes:
      'JBC-26 maps portable ReadWriteFs path, symlink, traversal, encoding, and sanitized-error semantics to deterministic Rust virtual filesystem tests. Rows requiring real OS symlink or permission setup stay pending.',
  },
  {
    file: 'packages/just-bash/src/fs/read-write-fs/read-write-fs.test.ts',
    lines: [398],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::read-write',
    rustTest: 'upstream_read_write_fs_virtual_backend_reads_writes_stats_and_mutates_paths',
    notes:
      'JBC-26 keeps the remaining existing-path EEXIST row mapped to the read/write virtual backend mutation test.',
  },
  {
    file: 'packages/just-bash/src/fs/mountable-fs/mountable-fs.security.test.ts',
    lines: [20, 33, 44, 54, 71, 88, 105, 122, 137, 358, 366, 376, 384, 398, 409, 419, 432, 441],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::mountable-security',
    rustTest: 'jbc26_symlink_policy_mount_routing_and_overlay_precedence_rows_are_virtual',
    notes:
      'JBC-26 verifies mount-local symlink resolution, loops, broken links, traversal normalization, cross-mount isolation, busy mount mutations, cross-device links, readlink, and realpath over virtual mounts. Real-FS mounted backend rows stay pending.',
  },
  {
    file: 'packages/just-bash/src/fs/overlay-fs/overlay-fs.security.test.ts',
    lines: [
      53, 57, 63, 69, 75, 79, 83, 89, 95, 99, 103, 109, 115, 123, 127, 131,
      135, 142, 147, 152, 158, 180, 186, 193, 202, 208, 218, 226, 235, 249,
      257, 267, 277, 285, 293, 314, 318, 322, 326, 330, 337, 343, 349, 356,
      362, 371, 377, 383, 391, 397, 404, 410, 415, 419, 426, 435, 444, 448,
      455, 459, 463, 467, 477, 484, 490, 494, 499, 504, 508, 516, 535, 546,
      553, 561, 568, 581, 593, 608, 615, 620, 625, 744, 753, 762, 774, 791,
      930, 949, 1140, 1155, 1166, 1183, 1326, 1339, 1350, 1365, 1562, 1568,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::overlay-security',
    rustTest:
      'jbc26_virtual_fs_path_security_encoding_and_error_shape_rows_are_sanitized; jbc26_symlink_policy_mount_routing_and_overlay_precedence_rows_are_virtual',
    notes:
      'JBC-26 verifies overlay path normalization, no-host traversal, special path handling, hard-link copy semantics, read-only/write/delete precedence, upper symlink overwrite/append behavior, base64 large reads, and /dev/null as a virtual file. Real OS symlink and BashEnv-overlay e2e rows stay pending.',
  },
  {
    file: 'packages/just-bash/src/fs/overlay-fs/overlay-fs.test.ts',
    lines: [773, 795, 816, 833, 851, 868, 884, 896],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::overlay',
    rustTest:
      'upstream_overlay_fs_virtual_backend_mount_copy_on_write_deletion_and_read_only_cases; jbc26_symlink_policy_mount_routing_and_overlay_precedence_rows_are_virtual',
    notes:
      'JBC-26 verifies overlay merged directory entries, type metadata, deletion hiding, case-sensitive sorting, memory symlink entries, and non-existent directory errors through Rust overlay tests.',
  },
];

const jbc15CaseGroups = [
  {
    file: 'packages/just-bash/src/interpreter/arithmetic.test.ts',
    lines: [
      6, 13, 20, 27, 34, 41, 48, 55, 62, 69, 76, 83, 99, 106, 113, 120,
      127, 134, 143, 150, 171, 180, 187, 194, 356, 363, 370, 393, 400, 407,
      473, 480,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::arithmetic',
    rustTest: 'upstream_arithmetic_binary_comparison_logical_unary_and_variable_rows',
    notes:
      'JBC-15 verifies portable arithmetic expansion and command rows for binary, comparison, logical, unary, variable, grouping, precedence, and zero/nonzero status behavior.',
  },
  {
    file: 'packages/just-bash/src/interpreter/arithmetic.test.ts',
    lines: [90, 157, 164, 231, 238, 245],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::arithmetic',
    rustTest: 'upstream_arithmetic_comma_short_circuit_ternary_rows',
    notes:
      'JBC-15 verifies portable arithmetic comma sequencing, short-circuit && / || that suppresses right-hand side assignment side effects, and true/false/nested ternary evaluation.',
  },
  {
    file: 'packages/just-bash/src/interpreter/arithmetic.test.ts',
    lines: [201, 208, 215, 222],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::arithmetic',
    rustTest: 'upstream_arithmetic_increment_decrement_rows',
    notes:
      'JBC-15 verifies portable arithmetic pre/post increment and decrement returning the correct expression value and mutating the underlying variable.',
  },
  {
    file: 'packages/just-bash/src/interpreter/arithmetic.test.ts',
    lines: [254, 261, 268, 275, 282, 289, 296, 303, 310, 317, 324],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::arithmetic',
    rustTest: 'upstream_arithmetic_assignment_operator_rows',
    notes:
      'JBC-15 verifies portable arithmetic = and compound += -= *= /= %= <<= >>= &= |= ^= assignment operators returning and persisting the assigned value.',
  },
  {
    file: 'packages/just-bash/src/interpreter/arithmetic.test.ts',
    lines: [377, 384, 443, 450, 457, 464],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::arithmetic',
    rustTest: 'upstream_arithmetic_variable_resolution_and_base_rows',
    notes:
      'JBC-15 verifies portable recursive variable-name resolution, re-evaluation of expressions stored in variables, and octal / hex / base#number / hex-with-letters literal parsing.',
  },
  {
    file: 'packages/just-bash/src/interpreter/arithmetic.test.ts',
    lines: [416, 423, 432],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::arithmetic',
    rustTest: 'upstream_arithmetic_array_element_rows',
    notes:
      'JBC-15 verifies portable arithmetic array-element access, assignment, and post-increment over indexed arrays.',
  },
  {
    file: 'packages/just-bash/src/interpreter/control-flow.test.ts',
    lines: [6, 17, 29, 90, 101, 126, 138, 163, 375, 388, 401, 414, 498],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::control-flow',
    rustTest: 'upstream_control_flow_if_for_case_rows',
    notes:
      'JBC-15 verifies portable if/else, for-list expansion, empty for lists, loop-variable persistence, brace-expanded for lists, literal/glob/multi/default case matches, and case-in-for behavior.',
  },
  {
    file: 'packages/just-bash/src/interpreter/builtins/export.test.ts',
    lines: [6, 12, 18, 26, 34, 50, 57, 63, 69, 78, 85, 94, 102],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::builtins::export',
    rustTest: 'upstream_export_builtin_assignment_listing_and_same_exec_rows',
    notes:
      'JBC-15 verifies portable export assignment, multi-assignment, equals-in-value, empty and existing-name exports, listing and -p output, alias exclusion, -n value preservation, same-exec use, and subshell visibility.',
  },
  {
    file: 'packages/just-bash/src/interpreter/builtins/unset.test.ts',
    lines: [6, 16, 25, 37, 48, 60, 72, 85, 101, 112],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::builtins::unset',
    rustTest: 'upstream_unset_builtin_variable_function_and_scope_rows',
    notes:
      'JBC-15 verifies portable unset variable, multi-variable, missing-variable, -v, -f function, missing-function, function-scope, local-scope, status, and non-crashing simple unset behavior.',
  },
  {
    file: 'packages/just-bash/src/interpreter/builtins/set.test.ts',
    lines: [411, 422, 433, 455, 465],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::builtins::set',
    rustTest: 'upstream_set_pipefail_rows',
    notes:
      'JBC-15 verifies portable pipefail success, first-command failure, middle-command failure, default last-command pipeline status, and +o pipefail disablement.',
  },
];

const r10jbInterpreterCoreCaseGroups = [
  {
    file: 'packages/just-bash/src/interpreter/arithmetic.test.ts',
    lines: [333, 340, 347],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::arithmetic',
    rustTest: 'r10jb_interpreter_arithmetic_error_rows_match_upstream',
    notes:
      'R10JB verifies portable arithmetic error reporting: division by zero, modulo by zero, and negative exponent each abort the expansion with a diagnostic on stderr and exit status 1.',
  },
  {
    file: 'packages/just-bash/src/interpreter/control-flow.test.ts',
    lines: [113, 150, 174, 187, 198, 209, 221, 233, 428, 442],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::control-flow',
    rustTest:
      'r10jb_interpreter_control_flow_loops_and_case_modifier_rows_match_upstream',
    notes:
      'R10JB verifies portable IFS field-splitting in for-in, positional-parameter iteration for `for i; do`, the invalid-identifier runtime error, the five C-style `for (( init; cond; update ))` rows, and the `;&` fall-through and `;;&` continue-matching case terminators.',
  },
  {
    file: 'packages/just-bash/src/interpreter/prototype-pollution.test.ts',
    lines: [
      135, 228, 349, 362, 375, 388, 451, 459, 466, 655, 665, 678, 688, 868,
      892, 903, 916,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::prototype-pollution',
    rustTest:
      'r10jb_interpreter_prototype_pollution_identifier_rows_match_upstream',
    notes:
      'R10JB verifies JavaScript prototype keywords (constructor, __proto__, prototype, hasOwnProperty, ...) are treated as ordinary bash identifiers as array values/elements, parameter expansions, comparisons, function/alias/local names, while-loop conditions, subshells, and command substitutions.',
  },
];

const jbc16CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/jq/jq.test.ts',
    lines: [
      6, 15, 26, 33, 42, 51, 60, 69, 80, 107, 121, 134, 146, 158, 170,
      182, 224, 261, 274, 300, 309, 318, 325, 334, 343, 350, 357, 366, 384,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_flags_files_functions_and_operators_close_rows',
    notes:
      'JBC-16 verifies portable jq flags, file/stdin inputs, JSON streams, error/help handling, exit status, compact/raw output, and deterministic in-memory file behavior.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.operators.test.ts',
    lines: [6, 12, 18, 36, 60, 110, 116],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_flags_files_functions_and_operators_close_rows',
    notes:
      'JBC-16 verifies portable jq arithmetic, string concatenation, equality, not, and alternative operators over JSON stdin.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.functions.test.ts',
    lines: [22, 45, 61, 68, 83, 92, 99, 108, 124, 131],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_flags_files_functions_and_operators_close_rows',
    notes:
      'JBC-16 verifies portable jq length, type, first, last, reverse, sort, unique, add, min, and max functions.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.filters.test.ts',
    lines: [6, 14],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_flags_files_functions_and_operators_close_rows',
    notes:
      'JBC-16 verifies portable jq select and map filters for simple in-memory JSON arrays.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.strings.test.ts',
    lines: [12, 30, 66, 96],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_flags_files_functions_and_operators_close_rows',
    notes:
      'JBC-16 verifies portable jq join, startswith, ascii_downcase, and index string helpers.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.keyword-field-access.test.ts',
    lines: [115, 121, 127, 135, 141, 151, 158],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_keyword_field_access_space_rows',
    notes:
      'JBC-16 verifies that a space-separated keyword/identifier after a dot is rejected as not-a-field-access (nonzero exit) while a space-separated string after a dot stays field access, including chained forms.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.prototype-pollution.test.ts',
    lines: [141, 150, 161, 170, 179, 189, 323, 344, 386],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_prototype_pollution_safe_key_rows',
    notes:
      'JBC-16 verifies that with_entries renaming, setpath (including nested and mixed-safe paths), and object construction with computed dangerous keys drop __proto__/constructor/prototype while preserving safe keys and leaving keys empty.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.test.ts',
    lines: [93, 209, 287, 375],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_multi_file_range_limit_and_tab_rows',
    notes:
      'JBC-16 verifies parallel multi-file input, find|xargs piping, slurp length over concatenated NDJSON, and --tab indentation.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.limits.test.ts',
    lines: [90, 124],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_multi_file_range_limit_and_tab_rows',
    notes:
      'JBC-16 verifies that limit(n; range(...)) caps output and that moderate ranges complete deterministically.',
  },
  {
    file: 'packages/just-bash/src/commands/yq/yq.test.ts',
    lines: [
      6, 17, 33, 50, 66, 100, 111, 124, 135, 287, 305, 314, 324, 331,
      340, 347, 534, 542, 550, 571, 591, 600, 609, 618, 627,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_yq_yaml_json_env_and_error_rows',
    notes:
      'JBC-16 verifies portable yq YAML/JSON navigation, JSON rendering, raw/compact output, env/error paths, exit status, and simple jq-compatible functions.',
  },
  {
    file: 'packages/just-bash/src/commands/yq/yq.env.test.ts',
    lines: [8, 38, 60],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_yq_yaml_json_env_and_error_rows',
    notes:
      'JBC-16 verifies portable yq env object field lookup, missing env nulls, and empty env strings inside the scoped Rust session.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.basic.test.ts',
    lines: [27, 34, 41, 48, 57, 64, 89, 116, 127, 147, 156, 163, 172, 179, 186, 193, 202],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_basic_columns_data_filter_rows',
    notes:
      'JBC-16 verifies portable xan count, headers, head/tail, slice, reverse, enum, behead, help/error, and unimplemented-command diagnostics over in-memory CSV.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.columns.test.ts',
    lines: [27, 54, 75, 105],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_basic_columns_data_filter_rows',
    notes:
      'JBC-16 verifies portable xan select/drop/rename column transforms over in-memory CSV.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.data.test.ts',
    lines: [42],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_basic_columns_data_filter_rows',
    notes:
      'JBC-16 verifies portable xan from-json conversion to CSV with deterministic header ordering.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.filter-sort.test.ts',
    lines: [27, 47, 83, 103, 167],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_basic_columns_data_filter_rows',
    notes:
      'JBC-16 verifies portable xan numeric filters, inverted filters, numeric sorting, dedup, and column-scoped regex search.',
  },
  {
    file: 'packages/just-bash/src/commands/sqlite3/sqlite3.test.ts',
    lines: [6, 24, 32, 73, 84],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_sqlite3_options_errors_and_simple_select_rows',
    notes:
      'JBC-16 verifies portable sqlite3 help, missing-argument handling, stdin SQL, and simple in-memory CREATE/INSERT/SELECT behavior without WASM workers.',
  },
  {
    file: 'packages/just-bash/src/commands/sqlite3/sqlite3.formatters.test.ts',
    lines: [6, 13, 20, 42, 82],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_sqlite3_options_errors_and_simple_select_rows',
    notes:
      'JBC-16 verifies portable sqlite3 list/csv/json formatting for simple SELECT result sets.',
  },
  {
    file: 'packages/just-bash/src/commands/sqlite3/sqlite3.options.test.ts',
    lines: [6, 24, 79],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_sqlite3_options_errors_and_simple_select_rows',
    notes:
      'JBC-16 verifies portable sqlite3 version, custom row separator, and custom column separator options.',
  },
  {
    file: 'packages/just-bash/src/commands/sqlite3/sqlite3.errors.test.ts',
    lines: [6, 15, 24, 33, 42, 49, 104, 125],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_sqlite3_options_errors_and_simple_select_rows',
    notes:
      'JBC-16 verifies portable sqlite3 missing option argument errors, required-argument errors, unknown options, and load_extension blocking.',
  },
];

const jbc28CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/python3/python3.optin.test.ts',
    lines: [5, 24],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::optional-runtimes',
    rustTest:
      'just_bash_optional_js_python_commands_fail_closed_without_host_runtime; open_agents_just_bash_blocks_js_python_host_runtime_without_fallback',
    notes:
      'JBC-28 verifies python3/python remain unavailable by default in the Rust and Open Agents Just Bash backends and fail closed without invoking host Python. JBC-39 maps the explicit fake-backend opt-in row separately.',
  },
  {
    file: 'packages/just-bash/src/commands/js-exec/js-exec.security.test.ts',
    lines: [33, 51],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::optional-runtimes',
    rustTest:
      'just_bash_optional_js_python_commands_fail_closed_without_host_runtime; open_agents_just_bash_blocks_js_python_host_runtime_without_fallback',
    notes:
      'JBC-28 verifies js-exec/node remain unavailable in the Rust and Open Agents Just Bash backends and fail closed without invoking a host JavaScript runtime.',
  },
  {
    file: 'packages/just-bash/src/security/attacks/js-exec-host-runtime-breakout-probes.test.ts',
    lines: [76, 103],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::optional-runtimes',
    rustTest:
      'just_bash_optional_js_python_commands_fail_closed_without_host_runtime; open_agents_just_bash_blocks_js_python_host_runtime_without_fallback',
    notes:
      'JBC-28 verifies node, path-qualified node/python, and env-wrapped host runtime probes fail closed in Rust/Open Agents Just Bash without writing host markers or spawning a host shell.',
  },
  {
    files: jbc28JsRuntimeOnlyTestFiles,
    status: 'js-only-documented',
    owner: 'crates/just-bash::security::runtime-classification',
    rustTest: 'just_bash_runtime_bridge_surfaces_are_classified_nonportable',
    notes:
      'JBC-28 classifies QuickJS child_process, Node compatibility shims, Python/worker bridge output limits, and JS worker attack-regression rows as optional JavaScript host-runtime behavior. Rust keeps the optional runtime absent; portable host-fallback probes are mapped separately.',
  },
];

const jbc39RuntimeSourceFiles = [
  'packages/just-bash/src/commands/js-exec/js-exec.ts',
  'packages/just-bash/src/commands/python3/python3.ts',
];

const jbc39PackageRuntimeSourceFiles = [
  'packages/just-bash/vitest.config.ts',
  'packages/just-bash/vitest.wasm.config.ts',
];

const jbc39JsExecRuntimeTestFiles = [
  'packages/just-bash/src/commands/js-exec/js-exec.esm.test.ts',
  'packages/just-bash/src/commands/js-exec/js-exec.fs.test.ts',
  'packages/just-bash/src/commands/js-exec/js-exec.http.test.ts',
  'packages/just-bash/src/commands/js-exec/js-exec.invoke-tool.test.ts',
  'packages/just-bash/src/commands/js-exec/js-exec.test.ts',
  'packages/just-bash/src/commands/js-exec/js-exec.ts-strip.test.ts',
  'packages/just-bash/src/commands/js-exec/js-exec.utf8-stdin.test.ts',
];

const jbc39PythonRuntimeTestFiles = [
  'packages/just-bash/src/commands/python3/python3.advanced.test.ts',
  'packages/just-bash/src/commands/python3/python3.env.test.ts',
  'packages/just-bash/src/commands/python3/python3.files.test.ts',
  'packages/just-bash/src/commands/python3/python3.http.test.ts',
  'packages/just-bash/src/commands/python3/python3.oop.test.ts',
  'packages/just-bash/src/commands/python3/python3.security.test.ts',
  'packages/just-bash/src/commands/python3/python3.stdlib.test.ts',
  'packages/just-bash/src/commands/python3/python3.test.ts',
  'packages/just-bash/src/commands/python3/python3.utf8-stdin.test.ts',
];

const jbc39SourceGroups = [
  {
    files: jbc39RuntimeSourceFiles,
    status: 'js-only-documented',
    owner: 'crates/just-bash::runtime::optional-language-runtimes',
    notes:
      'JBC-39 classifies the real QuickJS/CPython worker-backed language command implementations as JavaScript package host-runtime behavior. Rust exposes only an explicit fake/runtime-provider boundary and keeps host fallback disabled.',
  },
  {
    files: jbc39PackageRuntimeSourceFiles,
    status: 'js-only-documented',
    owner: 'packages/just-bash::vitest-package-runtime',
    notes:
      'JBC-39 classifies package Vitest and WASM worker isolation configuration as JavaScript package-runtime tooling, not portable Rust runtime behavior.',
  },
];

const jbc39CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/js-exec/js-exec.test.ts',
    lines: [225],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::optional-language-runtimes',
    rustTest:
      'just_bash_optional_language_commands_fail_closed_until_backend_is_explicit; open_agents_just_bash_blocks_js_python_host_runtime_without_fallback',
    notes:
      'JBC-39 verifies js-exec/node remain unavailable unless an explicit Rust language backend is injected and Open Agents still does not fall back to host JavaScript runtimes.',
  },
  {
    file: 'packages/just-bash/src/commands/python3/python3.optin.test.ts',
    lines: [13],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::optional-language-runtimes',
    rustTest: 'just_bash_optional_language_commands_use_only_explicit_fake_backend',
    notes:
      'JBC-39 verifies the portable opt-in boundary with an injected fake Python backend that registers python3/python and handles --version without invoking host Python. Real CPython/WASM execution rows remain JS-only runtime behavior.',
  },
  {
    file: 'packages/just-bash/src/Bash.commands.test.ts',
    lines: [86, 106],
    status: 'js-only-documented',
    owner: 'crates/just-bash::runtime::custom-command-host-boundary',
    rustTest: 'just_bash_host_runtime_custom_command_defense_rows_are_classified_nonportable',
    notes:
      'JBC-39 classifies trusted/untrusted JavaScript custom-command defense-in-depth rows as Node global monkey-patching behavior. Rust custom commands are trusted in-process callbacks verified separately and do not expose an untrusted JS global runtime.',
  },
  {
    files: jbc39JsExecRuntimeTestFiles,
    status: 'js-only-documented',
    owner: 'crates/just-bash::runtime::optional-language-runtimes',
    rustTest: 'just_bash_optional_language_commands_use_only_explicit_fake_backend',
    notes:
      'JBC-39 classifies enabled js-exec execution, QuickJS ESM/TypeScript parsing, filesystem/HTTP/tool bridges, process shims, output limits, bootstrap code, and UTF-8 guest execution as JavaScript host-runtime behavior. Rust verifies only explicit backend injection and no host fallback.',
  },
  {
    files: jbc39PythonRuntimeTestFiles,
    status: 'js-only-documented',
    owner: 'crates/just-bash::runtime::optional-language-runtimes',
    rustTest: 'just_bash_optional_language_commands_use_only_explicit_fake_backend',
    notes:
      'JBC-39 classifies enabled python3/python execution, CPython/Emscripten standard library behavior, filesystem/HTTP/env bridges, security controls, queueing, and UTF-8 guest execution as JavaScript package WASM-runtime behavior. Rust verifies only explicit backend injection and no host fallback.',
  },
];

const jbc30AgentExampleTestFiles = [
  'packages/just-bash/src/agent-examples/bug-investigation.test.ts',
  'packages/just-bash/src/agent-examples/code-review.test.ts',
  'packages/just-bash/src/agent-examples/codebase-exploration.test.ts',
  'packages/just-bash/src/agent-examples/config-analysis.test.ts',
  'packages/just-bash/src/agent-examples/debugging-workflow.test.ts',
  'packages/just-bash/src/agent-examples/dependency-analysis.test.ts',
  'packages/just-bash/src/agent-examples/feature-implementation.test.ts',
  'packages/just-bash/src/agent-examples/log-analysis.test.ts',
  'packages/just-bash/src/agent-examples/multi-file-migration.test.ts',
  'packages/just-bash/src/agent-examples/python-scripting.test.ts',
  'packages/just-bash/src/agent-examples/refactoring-workflow.test.ts',
  'packages/just-bash/src/agent-examples/security-audit.test.ts',
  'packages/just-bash/src/agent-examples/text-processing-workflows.test.ts',
];

const jbc30AgentExampleCaseGroups = [
  {
    file: 'packages/just-bash/src/agent-examples/codebase-exploration.test.ts',
    lines: [232, 240, 247, 255, 266, 273, 281, 291, 301, 308, 320],
    status: 'js-only-documented',
    owner: 'crates/just-bash::agent-examples::runtime-classification',
    rustTest:
      'agent_examples_host_metadata_rows_fail_closed_without_host_runtime; slack_app_mention_runs_agent_example_just_bash_workflow_through_service_adapter',
    notes:
      'JBC-30 classifies human-readable ls/du size rows as upstream JS object-file/host-metadata examples. Rust Just Bash does not register du or fall back to a host shell; the service adapter proof keeps those rows closed as documented exclusions.',
  },
  {
    file: 'packages/just-bash/src/agent-examples/security-audit.test.ts',
    lines: [309, 320, 332, 341, 348, 357, 365, 376, 388, 399, 409, 423],
    status: 'js-only-documented',
    owner: 'crates/just-bash::agent-examples::runtime-classification',
    rustTest:
      'agent_examples_host_metadata_rows_fail_closed_without_host_runtime; slack_app_mention_runs_agent_example_just_bash_workflow_through_service_adapter',
    notes:
      'JBC-30 classifies host/object-file permission audit rows as nonportable host metadata examples. Rust Just Bash keeps find -perm inside virtual file metadata and the Open Agents service proof blocks host shell fallback.',
  },
  {
    file: 'packages/just-bash/src/agent-examples/python-scripting.test.ts',
    lines: [46, 60, 70, 112, 123, 134, 175, 188, 224, 236, 274, 286, 356, 368, 379, 430, 442, 473],
    status: 'js-only-documented',
    owner: 'crates/just-bash::agent-examples::runtime-classification',
    rustTest:
      'agent_examples_python_scripting_rows_fail_closed_without_host_runtime; slack_app_mention_runs_agent_example_just_bash_workflow_through_service_adapter',
    notes:
      'JBC-30 classifies python:true rows as optional upstream JS/Python runtime examples. Rust Just Bash has no python3 backend for Open Agents and fails closed without host /bin/bash fallback.',
  },
  {
    files: jbc30AgentExampleTestFiles,
    status: 'portable-verified',
    owner: 'crates/just-bash::agent-examples',
    rustTest:
      'agent_examples_portable_file_search_text_and_state_workflows_use_virtual_backend; slack_app_mention_runs_agent_example_just_bash_workflow_through_service_adapter',
    notes:
      'JBC-30 maps portable agent-example rows to a Rust virtual-filesystem/search/text/state workflow corpus plus an Open Agents Slack service adapter proof for crate-backed multi-command execution with no host shell fallback.',
  },
];

const jbc31BrowserWebsiteSourceFiles = [
  'examples/website/app/api/fs/route.ts',
  'examples/website/app/components/Terminal.tsx',
  'examples/website/app/components/TerminalData.tsx',
  'examples/website/app/components/lite-terminal/LiteTerminal.ts',
  'examples/website/app/components/lite-terminal/ansi-parser.ts',
  'examples/website/app/components/lite-terminal/index.ts',
  'examples/website/app/components/lite-terminal/input-handler.ts',
  'examples/website/app/components/lite-terminal/types.ts',
  'examples/website/app/components/terminal-content.ts',
  'examples/website/app/components/terminal-parts/agent-command.ts',
  'examples/website/app/components/terminal-parts/commands.ts',
  'examples/website/app/components/terminal-parts/constants.ts',
  'examples/website/app/components/terminal-parts/index.ts',
  'examples/website/app/components/terminal-parts/input-handler.ts',
  'examples/website/app/components/terminal-parts/markdown.ts',
  'examples/website/app/components/terminal-parts/welcome.ts',
  'examples/website/app/layout.tsx',
  'examples/website/app/md/[[...path]]/route.ts',
  'examples/website/app/opengraph-image.tsx',
  'examples/website/app/page.tsx',
  'examples/website/next.config.ts',
];

const jbc31ExecutorJsExampleSourceFiles = [
  'examples/executor-tools/inline-tools.ts',
  'examples/executor-tools/main.ts',
  'examples/executor-tools/multi-api-agent.ts',
  'examples/executor-tools/multi-turn-discovery.ts',
];

const jbc31SourceGroups = [
  {
    files: ['examples/cjs-consumer/index.ts'],
    status: 'portable-verified',
    owner: 'crates/just-bash-napi::public-examples',
    notes:
      'JBC-31 verifies the CJS consumer shape through the NAPI smoke harness plus Rust README-style Bash constructor and exec tests.',
  },
  {
    files: ['examples/custom-command/commands.ts', 'examples/custom-command/main.ts'],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::custom-commands',
    notes:
      'JBC-31 maps the local custom-command public API behavior to Rust custom commands with args, stdin, env, virtual file reads, built-in override, lazy loading, pipelines, and subcommand exec. The live AI summarize branch remains an external-provider example and is not counted as Rust behavior.',
  },
  {
    files: ['examples/bash-agent/agent.ts', 'examples/website/app/api/agent/route.ts'],
    status: 'portable-verified',
    owner: 'crates/just-bash::examples::virtual-workspace',
    notes:
      'JBC-31 verifies the public Just Bash sandbox usage from these examples with deterministic virtual workspace commands (`ls`, `cat`, `grep -r`, `find`, `head`, and `wc`). AI streaming, bash-tool wiring, and Next.js response mechanics remain JavaScript host integration outside the Rust runtime row.',
  },
  {
    files: ['examples/bash-agent/main.ts', 'examples/bash-agent/shell.ts'],
    status: 'js-only-documented',
    owner: 'examples/bash-agent::interactive-cli',
    notes:
      'JBC-31 classifies the interactive readline shell, terminal coloring, process argv handling, and live AI agent loop as JavaScript CLI example code rather than portable Rust Just Bash behavior.',
  },
  {
    files: jbc31ExecutorJsExampleSourceFiles,
    status: 'js-only-documented',
    owner: 'examples/executor-tools::js-exec-live-tools',
    notes:
      'JBC-31 classifies these executor examples as JavaScript/QuickJS `js-exec`, SDK discovery, browser fetch, and live public API demonstrations. The Rust backend has a separate deterministic executor-tool seam; these source rows are not portable Rust runtime behavior.',
  },
  {
    files: jbc31BrowserWebsiteSourceFiles,
    status: 'js-only-documented',
    owner: 'examples/website::browser-ui',
    notes:
      'JBC-31 classifies the website UI, static data routes, markdown/image routes, terminal rendering, and Next.js config as browser/server presentation code. Public Just Bash sandbox behavior is mapped separately through the website agent route source row.',
  },
];

const jbc31CaseGroups = [
  {
    file: 'packages/just-bash/src/custom-commands.test.ts',
    lines: [15, 26, 43, 54, 64, 104, 118, 132, 148, 165, 179, 202, 223, 238, 252, 272],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::custom-commands',
    rustTest: 'jbc31_custom_commands_match_public_api_usage_rows',
    notes:
      'JBC-31 verifies the portable custom-command API surface with Rust eager and lazy command handlers, args/context data, stdin, virtual file reads, environment access, built-in override, multiple commands, non-zero status, pipelines, subcommand exec, and mixed eager/lazy registration. TypeScript structural helper rows map to the Rust `name()` and `is_lazy()` public methods rather than TS object-shape checks.',
  },
  {
    file: 'packages/just-bash/src/readme.test.ts',
    lines: [317, 370, 383],
    status: 'portable-verified',
    owner: 'crates/just-bash::docs::command-registry',
    rustTest: 'jbc31_docs_supported_command_list_matches_public_registry',
    notes:
      'JBC-31 verifies the README command-list rows against the tracked upstream registry data and the Rust public command registry without claiming unsupported command implementations.',
  },
  {
    file: 'packages/just-bash/src/readme.test.ts',
    lines: [432, 438, 444, 450, 472, 478],
    status: 'js-only-documented',
    owner: 'packages/just-bash::documentation-harness',
    rustTest: 'js-only-documented',
    notes:
      'JBC-31 classifies these rows as documentation-harness checks for markdown block presence, TypeScript compilation, and package docs example execution. Portable README/API behavior is mapped separately to named Rust tests; this JS docs harness itself is not Rust runtime behavior.',
  },
];

const jbc17ExecutorJsOnlySourceFiles = [
  'packages/just-bash-executor/src/create-executor.ts',
  'packages/just-bash-executor/src/executor-discovery-plugin.ts',
  'packages/just-bash-executor/src/executor-init.ts',
  'packages/just-bash-executor/src/index.ts',
  'packages/just-bash-executor/src/parse-tool-args.ts',
  'packages/just-bash-executor/src/types.ts',
  'packages/just-bash-executor/vitest.config.ts',
];

const jbc17JsOnlyExampleSourceFiles = [
  'examples/bash-agent/agent.ts',
  'examples/bash-agent/main.ts',
  'examples/bash-agent/shell.ts',
  'examples/custom-command/commands.ts',
  'examples/custom-command/main.ts',
  'examples/executor-tools/inline-tools.ts',
  'examples/executor-tools/main.ts',
  'examples/executor-tools/multi-api-agent.ts',
  'examples/website/app/api/agent/route.ts',
  'examples/website/app/api/fs/route.ts',
  'examples/website/app/components/Terminal.tsx',
  'examples/website/app/components/TerminalData.tsx',
  'examples/website/app/components/lite-terminal/LiteTerminal.ts',
  'examples/website/app/components/lite-terminal/ansi-parser.ts',
  'examples/website/app/components/lite-terminal/index.ts',
  'examples/website/app/components/lite-terminal/input-handler.ts',
  'examples/website/app/components/lite-terminal/types.ts',
  'examples/website/app/components/terminal-content.ts',
  'examples/website/app/components/terminal-parts/agent-command.ts',
  'examples/website/app/components/terminal-parts/commands.ts',
  'examples/website/app/components/terminal-parts/constants.ts',
  'examples/website/app/components/terminal-parts/index.ts',
  'examples/website/app/components/terminal-parts/input-handler.ts',
  'examples/website/app/components/terminal-parts/markdown.ts',
  'examples/website/app/components/terminal-parts/welcome.ts',
  'examples/website/app/layout.tsx',
  'examples/website/app/md/[[...path]]/route.ts',
  'examples/website/app/opengraph-image.tsx',
  'examples/website/app/page.tsx',
  'examples/website/next.config.ts',
];

const jbc17SourceGroups = [
  {
    file: 'packages/just-bash-executor/src/tool-command.ts',
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::executor',
    notes:
      'JBC-17 verifies the portable executor CLI bridge with Rust tests for camel/kebab names, JSON/flag/stdin argument parsing, help, errors, aliases, namespaces, jq pipelines, and hidden command exposure.',
  },
  {
    files: jbc17ExecutorJsOnlySourceFiles,
    status: 'js-only-documented',
    owner: 'js-only:just-bash-executor-sdk-runtime',
    notes:
      'JBC-17 classifies these @executor-js SDK loader, plugin, package-export, and TypeScript config sources as JavaScript package-runtime behavior; the portable Rust executor CLI/session surface is mapped separately.',
  },
  {
    file: 'examples/cjs-consumer/index.ts',
    status: 'portable-verified',
    owner: 'crates/just-bash-napi::smoke',
    notes:
      'JBC-17 verifies the portable consumer shape with the NAPI CommonJS smoke: Bash alias construction, exec result stdout/exitCode, fixture seeding, cwd, and virtual file persistence.',
  },
  {
    file: 'examples/executor-tools/multi-turn-discovery.ts',
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::executor',
    notes:
      'JBC-17 verifies the portable custom-source example behavior with an in-process countries executor, argument filtering, jq composition, and virtual filesystem writes without network or host shell fallback.',
  },
  {
    files: jbc17JsOnlyExampleSourceFiles,
    status: 'js-only-documented',
    owner: 'js-only:just-bash-examples',
    notes:
      'JBC-17 classifies the remaining example source files as JavaScript/browser/Next.js/AI-provider/network/custom-TypeScript-command scaffolding; portable Bash/session/executor behavior is mapped by the named Rust and NAPI tests.',
  },
];

const jbc17CaseGroups = [
  {
    file: 'packages/just-bash-executor/src/tool-command.test.ts',
    lines: [
      20, 26, 31, 36, 44, 49, 54, 59, 64, 69, 75, 81, 86, 91, 96, 101,
      106, 111, 116, 121, 127, 132, 137, 142, 147, 152, 157,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::executor',
    rustTest: 'just_bash_executor_cli_helpers_match_upstream_tool_command_rows',
    notes:
      'JBC-17 verifies executor CLI helper behavior for camel/kebab names, key/value and flag parsing, JSON coercion, stdin JSON precedence, malformed JSON diagnostics, boolean flags, and command-dispatch help handling.',
  },
  {
    file: 'packages/just-bash-executor/src/tool-command.test.ts',
    lines: [201, 208, 215, 222, 229, 236, 243, 250, 261, 268, 279, 287, 300, 318, 334, 352],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::executor',
    rustTest: 'just_bash_executor_tool_command_parses_flags_json_stdin_and_errors',
    notes:
      'JBC-17 verifies namespace command invocation, jq pipelines, help output, unknown subcommands, tool failures, multiple namespaces, hidden exposure, and camelCase aliases through the Rust executor bridge.',
  },
  {
    file: 'packages/just-bash-executor/src/executor-examples.test.ts',
    lines: [150, 162, 175, 188, 211],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::executor',
    rustTest: 'just_bash_executor_custom_source_example_rows_use_virtual_session_state',
    notes:
      'JBC-17 maps the portable custom-source example rows to a seeded in-memory countries executor that proves calls, list filtering, chained detail lookup, jq composition, and virtual filesystem persistence with no network or host shell.',
  },
  {
    file: 'packages/just-bash-executor/src/docs.test.ts',
    lines: [76, 83, 88, 93, 113, 118, 128, 136, 143, 148],
    status: 'js-only-documented',
    owner: 'js-only:just-bash-executor-docs',
    rustTest: 'js-only-documented',
    notes:
      'JBC-17 classifies README/SKILL markdown and TypeScript snippet syntax validation as documentation/package QA for the JavaScript executor package, not a portable Rust runtime behavior.',
  },
  {
    file: 'packages/just-bash-executor/src/executor-examples.test.ts',
    lines: [236, 259, 287, 339, 381, 424, 512, 548],
    status: 'js-only-documented',
    owner: 'js-only:just-bash-executor-sdk-runtime',
    rustTest: 'js-only-documented',
    notes:
      'JBC-17 classifies approval callbacks, executor-js discovery plugins, GraphQL/OpenAPI loader behavior, and Node fetch/package integration as JavaScript SDK runtime behavior; portable custom-source execution is verified separately.',
  },
  {
    file: 'packages/just-bash-executor/src/node-esm-smoke.test.ts',
    lines: [63, 88, 129, 170],
    status: 'js-only-documented',
    owner: 'js-only:just-bash-executor-node-esm',
    rustTest: 'js-only-documented',
    notes:
      'JBC-17 classifies plain Node ESM plugin loading and @just-bash/executor package-resolution smoke tests as JavaScript packaging behavior; Rust/NAPI consumer behavior is covered by the NAPI smoke.',
  },
];

const jbc18SourceGroups = [
  {
    files: ['packages/just-bash/src/cli/just-bash.ts'],
    status: 'portable-verified',
    owner: 'crates/just-bash::cli',
    notes:
      'JBC-18 verifies portable just-bash CLI argument planning, help/version output, virtual mount cwd selection, stdin/script-file source routing, JSON result shape, and NAPI planner exposure. Host OverlayFS read-only/existence behavior remains pending at exact test rows.',
  },
  {
    files: [
      'packages/just-bash/src/cli/exec.ts',
      'packages/just-bash/src/cli/shell.ts',
    ],
    status: 'js-only-documented',
    owner: 'crates/just-bash::cli::js-dev-tools',
    notes:
      'JBC-18 classifies the upstream Node dev:exec helper and readline interactive shell wrapper as JavaScript CLI tooling; portable Bash execution and CLI planning are mapped separately.',
  },
  {
    files: ['examples/cjs-consumer/index.ts'],
    status: 'portable-verified',
    owner: 'crates/just-bash-napi::package-entry',
    notes:
      'JBC-18 verifies the Rust-backed NAPI CommonJS and ESM package entries with named JS smoke tests that import/require the package and execute commands.',
  },
];

const jbc18CliBundleJsOnlyLines = [
  28, 34, 70, 81, 87, 103, 114, 124, 141, 163, 168, 170, 211,
];

const jbc18CaseGroups = [
  {
    file: 'packages/just-bash/src/cli/just-bash.test.ts',
    lines: [50, 57, 63, 69],
    status: 'portable-verified',
    owner: 'crates/just-bash::cli',
    rustTest:
      'jbc18_cli_help_and_version_flags_match_upstream_output; jbc18_napi_cli_planner_matches_upstream_help_version_and_argument_rows',
    notes:
      'JBC-18 verifies upstream -h/--help and -v/--version output strings through the Rust CLI planner and NAPI planner harness.',
  },
  {
    file: 'packages/just-bash/src/cli/just-bash.test.ts',
    lines: [77, 84, 90, 102, 137, 203, 218, 277, 286, 295],
    status: 'portable-verified',
    owner: 'crates/just-bash::cli',
    rustTest: 'jbc18_cli_invocation_shape_executes_inline_stdin_script_file_and_json_rows',
    notes:
      'JBC-18 verifies portable CLI invocation shape over the Rust in-memory backend: inline scripts, echo, pipes, ls, read-only read behavior, stdin source, script-file source, JSON stdout/stderr/exitCode formatting, and /home/user/project mount-path routing. Host OverlayFS read-only and root-existence rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/cli/just-bash.test.ts',
    lines: [307, 312, 324, 338],
    status: 'portable-verified',
    owner: 'crates/just-bash::cli',
    rustTest:
      'jbc18_cli_argument_parser_routes_sources_root_and_cwd; jbc18_napi_cli_planner_matches_upstream_help_version_and_argument_rows',
    notes:
      'JBC-18 verifies default mount cwd, explicit --cwd override, and upstream-style virtual cwd normalization without host filesystem access.',
  },
  {
    file: 'packages/just-bash/src/cli/just-bash.test.ts',
    lines: [352, 358],
    status: 'portable-verified',
    owner: 'crates/just-bash::cli',
    rustTest:
      'jbc18_cli_argument_parser_handles_combined_flags_and_errors; jbc18_napi_cli_planner_matches_upstream_help_version_and_argument_rows',
    notes:
      'JBC-18 verifies unknown-option and missing -c diagnostics through the Rust CLI parser and NAPI planner harness.',
  },
  {
    file: 'packages/just-bash/src/cli/just-bash.bundle.test.ts',
    lines: [193],
    status: 'portable-verified',
    owner: 'crates/just-bash-napi::package-entry',
    rustTest: 'jbc18_napi_cjs_entrypoint_requires_and_executes_basic_commands',
    notes:
      'JBC-18 verifies the Rust-backed CommonJS package entry requires successfully and executes basic commands through the NAPI adapter.',
  },
  {
    file: 'packages/just-bash/src/cli/just-bash.bundle.test.ts',
    lines: jbc18CliBundleJsOnlyLines,
    status: 'js-only-documented',
    owner: 'crates/just-bash::cli::js-distribution-exception',
    rustTest: 'js-only:upstream-esbuild-binary-lazy-load-worker-and-dynamic-require-distribution',
    notes:
      'JBC-18 classifies these exact upstream dist/bin, esbuild lazy-load, worker chunk layout, optional WASM runtime, and ESM dynamic-require rows as JavaScript package distribution behavior. Rust/NAPI package entry behavior is verified separately; portable command runtime rows remain mapped by command-owner tests.',
  },
];

const jbc19CaseGroups = [
  {
    file: 'packages/just-bash/src/helpers/shell-quote.test.ts',
    lines: [6, 10, 14, 18, 22, 26, 30, 39, 43, 52, 59, 66, 73, 80],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell-quote',
    rustTest: 'jbc19_shell_join_args_quotes_and_preserves_literal_arguments',
    notes:
      'JBC-19 verifies shellJoinArgs quoting, metacharacter neutralization, empty strings, whitespace, quotes, newlines, tabs, and interpreter literal-preservation rows.',
  },
  {
    file: 'packages/just-bash/src/syntax/here-document.test.ts',
    lines: [5, 14, 25, 34, 43, 52, 63, 74, 82, 91, 101, 110, 119, 129, 139, 152, 162, 220, 232],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::heredoc',
    rustTest: 'jbc19_heredoc_rows_parse_serialize_and_execute_with_expansion',
    notes:
      'JBC-19 verifies heredoc stdin delivery, multiline/empty bodies, quoted delimiters, variable and command substitution, grep/wc/pipeline consumers, delimiter variants, whitespace preservation, and no brace expansion.',
  },
  {
    file: 'packages/just-bash/src/comparison-tests/here-document.comparison.test.ts',
    lines: [20, 31, 45, 58, 69],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::heredoc',
    rustTest: 'jbc19_heredoc_rows_parse_serialize_and_execute_with_expansion',
    notes:
      'JBC-19 maps the portable real-Bash comparison heredoc rows to deterministic Rust heredoc execution and serializer-equivalence checks.',
  },
  {
    file: 'packages/just-bash/src/interpreter/pipeline-execution.test.ts',
    lines: [5, 11, 16, 22, 27, 35, 47, 57],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::pipeline',
    rustTest: 'jbc19_pipeline_stderr_rows_keep_regular_and_pipe_stderr_separate',
    notes:
      'JBC-19 verifies regular pipes keep stderr on the parent stream, |& pipes stdout and stderr together, multi-stage stderr stays ordered, and final command status is preserved.',
  },
  {
    file: 'packages/just-bash/src/transform/serialize.test.ts',
    lines: [215, 217, 218, 219, 221, 223, 225, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 239, 240, 242, 244],
    status: 'portable-verified',
    owner: 'crates/just-bash::transform::serialize',
    rustTest: 'jbc19_transform_serialize_quoting_edge_rows_round_trip',
    notes:
      'JBC-19 verifies Rust parse/serialize/parse stability for upstream string escaping edge rows covering adjacent quotes, empty quotes, escaped literals, glob/meta characters, substitutions, and arithmetic in quotes.',
  },
  {
    file: 'packages/just-bash/src/transform/serialize.test.ts',
    lines: [271, 272, 273, 274, 275, 278, 280, 282, 284, 286, 290, 291, 293, 295, 297, 303, 304, 305, 306, 307, 308, 313, 315, 316, 373, 375, 377, 379, 397],
    status: 'portable-verified',
    owner: 'crates/just-bash::transform::serialize',
    rustTest: 'jbc19_transform_serialize_quoting_edge_rows_round_trip',
    notes:
      'JBC-19 verifies execution-equivalent Rust serializer behavior for portable quoting rows with escaped dollars/backticks/backslashes, quoted literals, variable and command substitutions, escaped spaces/globs/hashes, and literal quoted globs.',
  },
  {
    file: 'packages/just-bash/src/transform/serialize.test.ts',
    lines: [331, 333, 335, 337, 339, 341, 343, 344, 345, 347, 349, 387, 388, 398, 403, 404, 405, 406, 408, 409, 411, 413, 415, 416],
    status: 'portable-verified',
    owner: 'crates/just-bash::transform::serialize',
    rustTest: 'jbc19_heredoc_rows_parse_serialize_and_execute_with_expansion',
    notes:
      'JBC-19 verifies heredoc and here-string parse/serialize/execution equivalence for variable, quoted, command-substitution, special-character, empty-line, tab-stripping, escaped-dollar, and command-consumer rows.',
  },
  {
    file: 'packages/just-bash/src/transform/transform.test.ts',
    lines: [13, 22, 31, 42, 57, 80, 91, 100],
    status: 'portable-verified',
    owner: 'crates/just-bash::transform::tee',
    rustTest: 'jbc19_transform_tee_plugin_metadata_and_script_rows',
    notes:
      'JBC-19 verifies no-plugin identity plus TeePlugin AST rewriting, target filtering, metadata, global counters, dynamic command names, and sanitized timestamp paths.',
  },
  {
    file: 'packages/just-bash/src/transform/transform.test.ts',
    lines: [170, 180, 197, 209, 217, 223, 241],
    status: 'portable-verified',
    owner: 'crates/just-bash::transform::pipeline',
    rustTest: 'jbc19_transform_plugin_ordering_and_metadata_rows',
    notes:
      'JBC-19 verifies transform plugin ordering, collector visibility after Tee rewrites, single/no-plugin pipeline behavior, metadata merging, rewrite plugins, and exception propagation.',
  },
  {
    file: 'packages/just-bash/src/transform/plugins/tee-plugin.test.ts',
    lines: [9, 23, 48, 69, 97, 130, 161, 175, 187, 206, 222, 242, 262, 284],
    status: 'portable-verified',
    owner: 'crates/just-bash::transform::tee',
    rustTest: 'jbc19_tee_plugin_exec_describe_rows',
    notes:
      'JBC-19 verifies the TeePlugin exec describe-block AST-rewrite contract: single commands and compound/&&/|| chains are left unwrapped, each targeted pipeline stage records commandName/command/stdoutFile metadata, nested output dirs and persistent counters produce unique sanitized paths, targetCommandPattern filtering selects the right stages, and PIPESTATUS save/restore preserves pipeline exit semantics.',
  },
];

const jbc20CaseGroups = [
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [
      87, 95, 199, 206, 215, 229, 244, 273, 288, 298, 311, 323, 335, 352,
      368, 421, 448,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::session-scope',
    rustTest: 'jbc20_exec_scope_restores_env_cwd_after_errors_and_concurrent_runs',
    notes:
      'JBC-20 verifies per-exec env/cwd scoping, restoration after command/tokenization errors, concurrent env isolation, command-set variable non-leakage, and portable sleep duration parsing without host shell fallback.',
  },
  {
    file: 'packages/just-bash/src/Bash.general.test.ts',
    lines: [506, 514, 521, 530, 540, 545, 583, 588, 597, 603, 609, 615, 624],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::bash-facade',
    rustTest: 'jbc20_bash_general_default_layout_and_api_rows_match_upstream',
    notes:
      'JBC-20 verifies upstream-style Bash facade file APIs, getCwd/getEnv, virtual /bin command stubs, /tmp/default HOME layout, and no default /home/user layout when files or cwd are explicitly supplied.',
  },
  {
    file: 'packages/just-bash/src/comparison-tests/cd.comparison.test.ts',
    lines: [21, 36, 54, 74, 95, 109, 127, 139],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::comparison-core',
    rustTest: 'jbc20_cd_env_and_status_comparison_rows_match_core_runtime',
    notes:
      'JBC-20 verifies portable cd/pwd comparison rows for relative traversal, cd -, cd errors, and dot path handling inside the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/comparison-tests/env.comparison.test.ts',
    lines: [22, 50, 62, 77],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::comparison-core',
    rustTest: 'jbc20_cd_env_and_status_comparison_rows_match_core_runtime',
    notes:
      'JBC-20 verifies portable env and printenv comparison rows for KEY=value output, specific variables, missing variable status, and multiple values.',
  },
  {
    file: 'packages/just-bash/src/comparison-tests/parse-errors.comparison.test.ts',
    lines: [58, 66, 76, 86, 177, 185, 193, 203, 211, 219, 227, 236, 243, 250, 260, 269],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::comparison-core',
    rustTest: 'jbc20_cd_env_and_status_comparison_rows_match_core_runtime',
    notes:
      'JBC-20 verifies portable comparison rows for unknown-command diagnostics, missing-file statuses, exit/true/false status, &&/||/semicolon behavior, quoting, and empty/whitespace commands.',
  },
  {
    file: 'packages/just-bash/src/commands/timeout/timeout.test.ts',
    lines: [7, 17, 29, 41, 50, 59, 68, 79, 87, 95, 103, 113, 123, 133, 145, 165, 177, 194],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::timeout',
    rustTest: 'jbc20_timeout_command_rows_use_cooperative_in_process_cancellation',
    notes:
      'JBC-20 verifies portable timeout command duration parsing, ignored foreground/kill/signal options, operand diagnostics, cooperative 124 cancellation, no post-timeout output or file side effects, and help output.',
  },
  {
    file: 'packages/just-bash/src/interpreter/pipeline-execution.test.ts',
    lines: [5, 11, 16, 22, 27, 35, 57],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::pipeline',
    rustTest: 'jbc20_pipeline_stderr_exit_status_and_metadata_rows_are_stable',
    notes:
      'JBC-20 verifies portable pipeline stderr propagation from first/middle/last commands, stdout/stderr separation, multiple error collection, and last-command exit status.',
  },
];

const jbc22CaseGroups = [
  {
    files: [
      'packages/just-bash/src/commands/find/find.basic.test.ts',
      'packages/just-bash/src/commands/find/find.depth.test.ts',
      'packages/just-bash/src/commands/find/find.operators.test.ts',
      'packages/just-bash/src/commands/find/find.patterns.test.ts',
      'packages/just-bash/src/commands/find/find.perm.test.ts',
      'packages/just-bash/src/commands/find/find.predicates.test.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::commands::find',
    rustTest:
      'jbc22_find_command_closes_basic_pattern_depth_operator_action_rows; jbc22_find_command_closes_printf_delete_exec_and_metadata_rows',
    notes:
      'JBC-22 verifies portable find traversal, multiple roots, relative display paths, name/path/regex/type/empty/size/permission predicates, depth bounds, boolean operators, errors, and deterministic virtual-filesystem metadata.',
  },
  {
    files: [
      'packages/just-bash/src/commands/find/find.actions.test.ts',
      'packages/just-bash/src/commands/find/find.exec.test.ts',
      'packages/just-bash/src/commands/find/find.exec-command-name-quoting.test.ts',
      'packages/just-bash/src/commands/find/find.printf.test.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::commands::find',
    rustTest: 'jbc22_find_command_closes_printf_delete_exec_and_metadata_rows',
    notes:
      'JBC-22 verifies portable find -print0, -printf metadata/directives, -delete, and -exec {} / {}+ behavior inside the Rust virtual filesystem without host shell fallback.',
  },
  {
    file: 'packages/just-bash/src/commands/find/find.perf.test.ts',
    status: 'portable-verified',
    owner: 'crates/just-bash::commands::find',
    rustTest:
      'jbc22_find_command_closes_basic_pattern_depth_operator_action_rows; jbc22_find_command_closes_printf_delete_exec_and_metadata_rows',
    notes:
      'JBC-22 maps the portable find optimization corpus to deterministic Rust traversal, pruning, depth, filtering, metadata, and action behavior over in-memory fixtures; JS trace timing is not required for command-output parity.',
  },
  {
    files: [
      'packages/just-bash/src/commands/curl/tests/allowlist.test.ts',
      'packages/just-bash/src/commands/curl/tests/availability.test.ts',
      'packages/just-bash/src/commands/curl/tests/errors.test.ts',
      'packages/just-bash/src/commands/curl/tests/methods.test.ts',
      'packages/just-bash/src/commands/curl/tests/timeout.test.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::commands::curl',
    rustTest:
      'jbc22_curl_command_uses_opt_in_fake_transport_and_resource_policy; jbc22_network_resource_seam_records_planned_fake_requests_without_live_io',
    notes:
      'JBC-22 verifies curl remains absent by default, is registered only with explicit network policy, allows/denies URLs and methods through the deterministic network planner, records fake requests, and never performs live network I/O.',
  },
  {
    files: [
      'packages/just-bash/src/commands/curl/curl.prototype-pollution.test.ts',
      'packages/just-bash/src/commands/curl/tests/auth.test.ts',
      'packages/just-bash/src/commands/curl/tests/binary.test.ts',
      'packages/just-bash/src/commands/curl/tests/cookies.test.ts',
      'packages/just-bash/src/commands/curl/tests/data-at-file.test.ts',
      'packages/just-bash/src/commands/curl/tests/form.test.ts',
      'packages/just-bash/src/commands/curl/tests/options.test.ts',
      'packages/just-bash/src/commands/curl/tests/parse.test.ts',
      'packages/just-bash/src/commands/curl/tests/upload.test.ts',
      'packages/just-bash/src/commands/curl/tests/verbose.test.ts',
      'packages/just-bash/src/commands/curl/tests/writeout.test.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::commands::curl',
    rustTest:
      'jbc22_curl_command_closes_parse_body_output_cookie_and_error_rows; jbc22_curl_command_uses_opt_in_fake_transport_and_resource_policy',
    notes:
      'JBC-22 verifies portable curl option parsing, headers, auth, cookies, data-at-file/urlencode/form/upload bodies, binary output files, include/verbose/write-out formatting, and fail/silent errors against seeded fake responses.',
  },
];

const jbc23CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/echo/echo.binary.test.ts',
    lines: [6, 20, 29, 42, 54, 66],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-stream',
    rustTest: 'echo_upstream_command_handles_newline_and_escape_flags',
    notes:
      'JBC-23 verifies portable echo -e binary hex/octal escapes, null bytes, virtual-file redirects, and cat round trips without host echo.',
  },
  {
    file: 'packages/just-bash/src/commands/printf/printf.test.ts',
    lines: [
      6, 13, 20, 28, 35, 42, 49, 58, 64, 70, 77, 83, 89, 95, 101, 107,
      113, 121, 127, 133, 139, 147, 155, 162, 173,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-stream',
    rustTest: 'printf_upstream_command_formats_core_specifiers_and_escapes',
    notes:
      'JBC-23 verifies portable printf specifiers, escapes, width/precision, usage errors, missing args, invalid numeric warnings, and help output.',
  },
  {
    file: 'packages/just-bash/src/commands/printf/escapes.test.ts',
    lines: [
      6, 10, 14, 18, 22, 26, 30, 34, 40, 44, 48, 54, 58, 62, 71, 75, 79,
      85, 89, 93, 97, 103, 107, 111, 115, 121, 127, 136, 140, 144, 148,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-stream',
    rustTest: 'printf_upstream_command_formats_core_specifiers_and_escapes',
    notes:
      'JBC-23 verifies printf escape processing plus the user-visible width and precision behavior backed by the upstream escape helpers.',
  },
  {
    file: 'packages/just-bash/src/commands/printf/printf.binary.test.ts',
    lines: [6, 20, 29, 42, 54, 66],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-stream',
    rustTest: 'printf_upstream_command_formats_core_specifiers_and_escapes',
    notes:
      'JBC-23 verifies portable printf binary hex/octal escapes, null bytes, virtual-file redirects, and cat round trips; base64 remains pending.',
  },
  {
    file: 'packages/just-bash/src/commands/grep/grep.advanced.test.ts',
    lines: [
      7, 27, 37, 50, 59, 68, 80, 89, 111, 118, 125, 132, 139, 146, 158,
      171, 180, 189, 198, 207, 218, 227, 236, 256, 268, 280, 292,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-search',
    rustTest: 'text_search_grep_rg_sed_and_awk_close_upstream_rows',
    notes:
      'JBC-23 verifies portable grep pipe chains, only-matching, context, max-count, whole-line matching, no-filename, and recursive no-filename rows.',
  },
  {
    file: 'packages/just-bash/src/commands/sed/sed.test.ts',
    lines: [118, 126, 135, 143, 156, 164, 172, 180, 198, 208, 218, 232, 243, 254],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-search',
    rustTest: 'text_search_grep_rg_sed_and_awk_close_upstream_rows',
    notes:
      'JBC-23 verifies portable sed empty replacements, delete/substitute ranges, case-insensitive substitutions, multiple -e scripts, and ampersand replacement escaping.',
  },
  {
    file: 'packages/just-bash/src/commands/head/head.test.ts',
    lines: [99],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes: 'JBC-23 verifies the remaining portable head -n 1 first-line row.',
  },
  {
    file: 'packages/just-bash/src/commands/tail/tail.test.ts',
    lines: [91],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes: 'JBC-23 verifies the remaining portable tail -n 1 last-line row.',
  },
  {
    file: 'packages/just-bash/src/commands/wc/wc.test.ts',
    lines: [48, 77, 116],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes:
      'JBC-23 verifies portable wc combined -lw, empty-file counts, and newline-terminated line counts.',
  },
  {
    file: 'packages/just-bash/src/commands/sort/sort.test.ts',
    lines: [99, 108, 120, 129, 138, 149, 158],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes:
      'JBC-23 verifies portable sort combined numeric/reverse flags, ignore-case variants, case-folded unique count, field sort, and help output.',
  },
  {
    file: 'packages/just-bash/src/commands/cut/cut.utf8-stdin.test.ts',
    lines: [5, 13],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes: 'JBC-23 verifies cut UTF-8 stdin codepoint slicing and delimiter field preservation.',
  },
  {
    file: 'packages/just-bash/src/commands/tr/tr.utf8-stdin.test.ts',
    lines: [5, 12],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes: 'JBC-23 verifies tr UTF-8 stdin translation and multibyte pass-through.',
  },
  {
    file: 'packages/just-bash/src/commands/tr/tr.complement.test.ts',
    lines: [6, 14, 23, 29, 35, 43, 50, 61, 70],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes:
      'JBC-23 verifies tr complement delete/translate/squeeze behavior plus POSIX alnum/alpha classes.',
  },
  {
    file: 'packages/just-bash/src/commands/uniq/uniq.utf8-stdin.test.ts',
    lines: [5, 12],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes:
      'JBC-23 verifies uniq UTF-8 stdin preservation and Unicode-aware case folding without corrupting surviving output.',
  },
  {
    file: 'packages/just-bash/src/commands/uniq/uniq.binary.test.ts',
    lines: [5, 24],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_pipeline_head_tail_wc_sort_uniq_cut_tr_close_upstream_rows',
    notes:
      'JBC-23 verifies uniq binary-style line dedupe and UTF-8 leading-byte preservation under -i.',
  },
];

const jbc34CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/grep/grep.basic.test.ts',
    lines: [
      119, 147, 156, 165, 174, 183, 194, 258, 278, 286, 294, 302, 310,
      318, 326, 335, 343, 351, 359, 370, 379, 388, 399, 407, 415, 423,
      431, 442, 450, 458, 466, 480, 488, 498, 506, 514, 522, 530, 538,
      546, 554, 564, 662, 688, 704, 722, 732,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-search',
    rustTest: 'text_search_jbc34_grep_basic_regex_and_utf8_rows',
    notes:
      'JBC-34 verifies additional portable grep file-list, recursive, word, extended-regex, -e, directory diagnostic, regex, POSIX-class, combined-flag, Unicode, filename, and supported BRE-compatible rows without claiming unsupported BRE word-boundary, grouping, interval, include/exclude, PCRE, or binary behavior.',
  },
  {
    file: 'packages/just-bash/src/commands/grep/grep.advanced.test.ts',
    lines: [15, 245],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-search',
    rustTest: 'text_search_jbc34_grep_basic_regex_and_utf8_rows',
    notes:
      'JBC-34 verifies the portable grep pipeline filter row and case-insensitive max-count row; glob include and BRE alternation rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/grep/grep.utf8-stdin.test.ts',
    lines: [5],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-search',
    rustTest: 'text_search_jbc34_grep_basic_regex_and_utf8_rows',
    notes: 'JBC-34 verifies grep preserves multibyte stdin matches through the Rust pipe path.',
  },
  {
    file: 'packages/just-bash/src/commands/grep/grep.exclude.test.ts',
    lines: [6, 21, 36, 51, 68, 86, 104, 119, 131, 143, 160],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-search',
    rustTest: 'text_search_jbc_grep_exclude_files_without_match_and_bracket_rows',
    notes:
      'JBC-34 verifies grep --exclude (single/multiple globs, non-recursive explicit paths), --exclude-dir (single/multiple plus combined with --exclude), and -L/--files-without-match (explicit list, exit-code 0/1, recursive -rL, and the long-form flag) over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/grep/grep.basic.test.ts',
    lines: [636, 644],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-search',
    rustTest: 'text_search_jbc_grep_exclude_files_without_match_and_bracket_rows',
    notes:
      'JBC-34 verifies POSIX bracket edge cases where a leading ] is a literal class member: a[][]b matches ] or [, and a[^]b]c negates ] and b.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/rg.patterns.test.ts',
    lines: [
      5, 18, 31, 44, 57, 70, 85, 98, 113, 126, 139, 154, 167, 180, 193,
      206, 219, 234, 249, 262, 276,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'text_search_jbc34_rg_patterns_gitignore_and_edge_rows',
    notes:
      'JBC-34 verifies rg word/line/fixed/invert patterns, multiple -e/--regexp rows, smart case, regex anchors, alternation, quantifiers, classes, and combined -w/-c/-l/-v rows.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/rg.edge-cases.test.ts',
    lines: [
      5, 18, 31, 44, 57, 74, 87, 100, 113, 126, 142, 155, 168, 183, 196,
      209, 224, 239, 255, 266, 277, 289, 303, 316, 329, 342, 359, 378,
      393, 413, 427,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'text_search_jbc34_rg_patterns_gitignore_and_edge_rows',
    notes:
      'JBC-34 verifies rg empty/whitespace, fixed-special, boundary, Unicode, ordering, exit-code, word-boundary, inverted-context, gitignore comment/blank, and glob edge rows over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/gitignore.test.ts',
    lines: [6, 14, 21, 30, 37, 47, 56, 72, 80, 90, 96, 105, 113, 121],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'text_search_jbc34_rg_patterns_gitignore_and_edge_rows',
    notes:
      'JBC-34 verifies rg gitignore extension, exact name, directory, negation, directory-only, rooted, double-star, comment, blank, question-mark, character-class, and negated-class rows; rooted slash-containing patterns remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/rg.flags.test.ts',
    lines: [87, 105],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'text_search_jbc34_rg_patterns_gitignore_and_edge_rows',
    notes:
      'JBC-34 verifies portable rg -u no-ignore and -uu hidden-file search rows; symlink-follow and binary-as-text rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/rg/rg.flags.test.ts',
    lines: [11, 23, 35, 49, 122, 135, 149],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::rg',
    rustTest: 'rg_flags_symlink_unrestricted_and_text_rows_are_portable',
    notes:
      'JBC-10 verifies portable rg -L/--follow acceptance, default symlink skipping, -L file-symlink following, -u/--no-ignore and -uu/--no-ignore --hidden equivalence, and -a binary-as-text search over the virtual filesystem.',
  },
  {
    file: 'packages/just-bash/src/commands/sed/sed.regex.test.ts',
    lines: [6, 15, 24, 33, 43, 52, 61, 70, 80, 90, 100],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-search',
    rustTest: 'text_search_jbc34_sed_regex_and_utf8_rows',
    notes:
      'JBC-34 verifies sed POSIX-class substitution rows backed by Rust regex; BRE/ERE grammar, backreference, newline/tab replacement, and quantifier edge rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/sed/sed.utf8-stdin.test.ts',
    lines: [5],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-search',
    rustTest: 'text_search_jbc34_sed_regex_and_utf8_rows',
    notes: 'JBC-34 verifies sed matches and replaces multibyte text from a piped virtual file.',
  },
  {
    file: 'packages/just-bash/src/commands/sort/sort.utf8-stdin.test.ts',
    lines: [5, 12],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_stream_jbc34_utf8_pipeline_rows_use_implemented_commands',
    notes: 'JBC-34 verifies sort and sort -f preserve UTF-8 text through stdin without byte corruption.',
  },
  {
    file: 'packages/just-bash/src/commands/head/head.utf8-stdin.test.ts',
    lines: [5, 12],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_stream_jbc34_utf8_pipeline_rows_use_implemented_commands',
    notes: 'JBC-34 verifies head and tail preserve multibyte stdin lines; tac remains pending.',
  },
  {
    file: 'packages/just-bash/src/commands/wc/wc.utf8-stdin.test.ts',
    lines: [6],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_stream_jbc34_utf8_pipeline_rows_use_implemented_commands',
    notes: 'JBC-34 verifies wc -c counts UTF-8 bytes and wc -m counts codepoints from stdin.',
  },
  {
    file: 'packages/just-bash/src/commands/utf8-across-commands.test.ts',
    lines: [202, 208, 216, 224, 241, 249, 257],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_stream_jbc34_utf8_pipeline_rows_use_implemented_commands',
    notes:
      'JBC-34 verifies UTF-8 preservation through implemented grep, sed, tr, sort, uniq, cut, head, tail, and wc pipeline combinations; tee, split, AWK redirect, and unimplemented command rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/utf8-bytestring.test.ts',
    lines: [61, 71, 104, 116, 190],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::text-pipeline',
    rustTest: 'text_stream_jbc34_utf8_pipeline_rows_use_implemented_commands',
    notes:
      'JBC-34 verifies selected UTF-8 byte/codepoint rows for wc, cut, tr, uniq, and uniq-to-wc paths; rev, base64, split, tee, expand/unexpand, and sed newline-sensitive byte rows remain pending.',
  },
];

const jbc24CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/jq/jq.functions.test.ts',
    lines: [6, 13, 29, 36, 52, 75, 115, 138, 146, 156, 171, 179, 189, 199, 209, 237, 243],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_deep_query_construction_and_operator_rows',
    notes:
      'JBC-24 verifies additional portable jq keys/values, scalar/object metadata, range streams, string add, min_by/max_by, flatten, sort/group/unique_by, and entries conversion over deterministic JSON stdin.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.operators.test.ts',
    lines: [24, 30, 44, 50, 66, 72, 78, 84, 90, 98, 104, 124, 134, 140, 146, 152, 158, 166, 172],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_deep_query_construction_and_operator_rows',
    notes:
      'JBC-24 verifies additional portable jq arithmetic, comparisons, logic, defaulting, math, and type conversion operators over deterministic JSON stdin.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.filters.test.ts',
    lines: [20, 28, 38, 44, 50, 58, 64, 74, 80, 88, 96, 114, 120],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_deep_query_construction_and_operator_rows',
    notes:
      'JBC-24 verifies additional portable jq select chains, object/array has and contains, any/all, simple if/else, and optional field access.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.construction.test.ts',
    lines: [6, 14, 22, 30, 40, 46],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_deep_query_construction_and_operator_rows',
    notes:
      'JBC-24 verifies portable jq object and array construction rows with static keys, shorthand keys, dynamic keys, piped values, and object value iteration.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.strings.test.ts',
    lines: [6, 22, 38, 48, 56, 72, 80, 86, 102],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_string_keyword_and_safe_object_rows',
    notes:
      'JBC-24 verifies additional portable jq split, regex test, suffix/prefix trimming, uppercase, substitution, global substitution, and indices string helpers.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.keyword-field-access.test.ts',
    lines: [6, 15, 22, 29, 36, 43, 50, 57, 66, 75, 84, 95, 104, 169, 178, 187, 196],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_string_keyword_and_safe_object_rows',
    notes:
      'JBC-24 verifies portable jq keyword-like field access and keyword object-key construction rows without opening parser/destructuring gaps.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.prototype-pollution.test.ts',
    lines: [15, 26, 36, 46, 57, 66, 75, 88, 100, 109, 118],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jq_string_keyword_and_safe_object_rows',
    notes:
      'JBC-24 verifies portable jq handling for dangerous object keys by allowing safe direct lookup while filtering unsafe constructed/from_entries keys.',
  },
  {
    file: 'packages/just-bash/src/commands/query-engine/safe-object.test.ts',
    lines: [17, 21, 25, 29, 38, 117, 129, 163, 173, 184],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_query_engine_safe_key_rows',
    notes:
      'JBC-24 verifies portable query-engine safe-key classification, ignored unsafe inserts, and filtered from_entries construction through the Rust structured-data helpers.',
  },
  {
    file: 'packages/just-bash/src/commands/yq/yq.test.ts',
    lines: [521, 560, 580, 636, 645, 659, 670],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_yq_deep_query_env_and_security_rows',
    notes:
      'JBC-24 verifies additional portable yq join output, custom JSON indentation, combined short options, and jq-compatible unique/sort_by/reverse/group_by rows.',
  },
  {
    file: 'packages/just-bash/src/commands/yq/yq.env.test.ts',
    lines: [17, 26, 49, 69, 78],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_yq_deep_query_env_and_security_rows',
    notes:
      'JBC-24 verifies additional portable yq $ENV/env object lookup, env key listing, missing values, special characters, and expression composition.',
  },
  {
    file: 'packages/just-bash/src/commands/yq/yq.yaml-security.test.ts',
    lines: [60, 75, 93, 105],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_yq_deep_query_env_and_security_rows',
    notes:
      'JBC-24 verifies portable yq YAML/JSON dangerous-key handling for deterministic in-memory fixtures without claiming non-YAML/JSON format support.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.basic.test.ts',
    lines: [71, 80, 98, 107, 136, 211],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_extended_csv_rows',
    notes:
      'JBC-24 verifies additional portable xan headers stdin, default head/tail, stdin head, slice length, and header-only behead rows over in-memory CSV.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.columns.test.ts',
    lines: [36, 45, 63, 84, 93, 112],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_extended_csv_rows',
    notes:
      'JBC-24 verifies additional portable xan select/drop/rename rows including requested column order and relative virtual file paths.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.filter-sort.test.ts',
    lines: [36, 56, 65, 74, 92, 113, 120, 176],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_extended_csv_rows',
    notes:
      'JBC-24 verifies additional portable xan filter, limit, header-only, string/numeric reverse sort, dedup, and case-insensitive search rows.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.data.test.ts',
    lines: [10, 22, 31, 54, 63, 72],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_extended_csv_rows',
    notes:
      'JBC-24 verifies additional portable xan to/from JSON rows, pretty JSON output, and fail-closed data-format diagnostics.',
  },
  {
    file: 'packages/just-bash/src/commands/sqlite3/sqlite3.options.test.ts',
    lines: [15, 35, 48, 59, 68, 90, 114],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_sqlite3_deep_options_modes_and_error_rows',
    notes:
      'JBC-24 verifies additional portable sqlite3 option rows for end-of-options, echo, -cmd, header/noheader, nullvalue, and bail behavior.',
  },
  {
    file: 'packages/just-bash/src/commands/sqlite3/sqlite3.output-modes.test.ts',
    lines: [6, 24, 35, 78, 104],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_sqlite3_deep_options_modes_and_error_rows',
    notes:
      'JBC-24 verifies additional portable sqlite3 csv/json/line/tabs/quote output rows without claiming table, markdown, box, html, or escaping completeness.',
  },
  {
    file: 'packages/just-bash/src/commands/sqlite3/sqlite3.errors.test.ts',
    lines: [58, 69, 82, 92, 113, 135],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_sqlite3_deep_options_modes_and_error_rows',
    notes:
      'JBC-24 verifies additional portable sqlite3 non-bail/bail SQL error flow, double-dash option normalization, and load_extension entry-point blocking rows.',
  },
  {
    file: 'packages/just-bash/src/commands/sqlite3/sqlite3.test.ts',
    lines: [15, 40, 91, 107, 118],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_sqlite3_deep_options_modes_and_error_rows',
    notes:
      'JBC-24 verifies additional portable sqlite3 multi-column, multi-statement, syntax/missing-table diagnostics, and NULL JSON rows over in-memory databases.',
  },
];

const jbc25CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/awk/awk.test.ts',
    lines: [
      264, 275, 286, 297, 308, 319, 332, 343, 354, 365, 462, 486, 510,
      519,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest:
      'awk_jbc25_scalar_expression_operator_and_ternary_rows; awk_jbc25_builtin_math_string_match_and_substitution_rows',
    notes:
      'JBC-25 verifies portable AWK scalar arithmetic, compound assignment, prefix/postfix increments, match state, gensub, and power operators; full control flow, user functions, getline, output redirection, and format variants remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.operators.test.ts',
    lines: [
      6, 13, 20, 27, 34, 41, 48, 55, 62, 71, 89, 98, 107, 134, 145,
      154, 163, 192, 201, 210, 248, 257, 284, 306, 315, 324, 333, 342,
      351, 360, 371, 380, 389, 398,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc25_scalar_expression_operator_and_ternary_rows',
    notes:
      'JBC-25 verifies portable AWK scalar arithmetic, comparison, logical, regex-match, ternary, assignment, compound assignment, and prefix/postfix increment rows; short-circuit assignment side effects and unary/exponent precedence edge cases remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.patterns.test.ts',
    lines: [60, 89, 98, 127, 136, 145, 154],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc25_pattern_and_range_rows',
    notes:
      'JBC-25 verifies portable AWK regex quantifier patterns plus scalar greater-than, less-than, AND, OR, NOT, and combined regex/expression patterns.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.range.test.ts',
    lines: [5, 21, 40, 58, 71, 85, 101],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc25_pattern_and_range_rows',
    notes:
      'JBC-25 verifies portable AWK range patterns, repeated ranges, action ranges, single-line ranges, ranges through EOF, regex start/end ranges, and numbered-content ranges.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.math.test.ts',
    lines: [6, 12, 18, 24, 30, 38, 44, 50, 89],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc25_builtin_math_string_match_and_substitution_rows',
    notes:
      'JBC-25 verifies portable AWK int, sqrt, exp, log, sin, cos, atan2, and combined sqrt arithmetic rows; random seeding and additional atan2 variants remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.strings.test.ts',
    lines: [
      6, 15, 24, 33, 44, 53, 62, 71, 91, 100, 127, 138, 156, 176, 205,
      241, 253, 262, 280, 300, 309, 318, 354,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc25_builtin_math_string_match_and_substitution_rows',
    notes:
      'JBC-25 verifies portable AWK length, substr, index, case conversion, sub/gsub including matched-text replacement and field targets, and bounded sprintf rows; wider formatting and remaining substitution variants stay pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.strings.test.ts',
    lines: [
      80, 109, 118, 147, 165, 185, 194, 214, 223, 232, 271, 289, 327,
      336, 345, 365, 374, 383, 390, 401, 410, 419, 428, 439, 446, 455,
      464,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest:
      'awk_jbc25_string_builtin_concat_compare_and_coercion_rows',
    notes:
      'JBC-25 verifies additional portable AWK string rows: substr middle slice, index single-char and first-occurrence, tolower/toupper across cases and symbols, sub match/no-match/explicit-target, gsub no-match and digit replacement, sprintf width/left-justify/zero-pad, variable and literal concatenation, numeric-literal concatenation, accumulating concat, string ==/!=/</> comparison, leading-numeric and non-numeric +0 coercion, and number-to-string via empty concatenation.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.modulo.test.ts',
    lines: [6, 36, 54, 74],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest:
      'awk_jbc25_scalar_expression_operator_and_ternary_rows; awk_jbc25_array_and_computed_field_rows',
    notes:
      'JBC-25 verifies portable AWK modulo expressions, scalar modulo assignment, array-element modulo assignment, and odd-number filters; loop modulo and additional numeric edge variants remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.ternary.test.ts',
    lines: [126, 137],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc25_scalar_expression_operator_and_ternary_rows',
    notes:
      'JBC-25 verifies portable AWK ternary field conditions and field branches; nested, function-heavy, truthiness, and getline ternaries remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.arrays.test.ts',
    lines: [6, 15, 24, 33, 42, 51, 196, 218, 231, 306],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc25_array_and_computed_field_rows',
    notes:
      'JBC-25 verifies portable AWK array element creation, numeric and expression indices, missing elements, overwrite, concatenated keys, counting, grouped sums, SUBSEP-style two-dimensional keys, and compound assignment.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.arrays.test.ts',
    lines: [62, 71, 80, 89, 100, 109, 118, 129, 138],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc25_in_delete_and_for_in_array_rows',
    notes:
      'JBC-25 verifies portable AWK `in` membership (existing/missing/numeric keys, non-creating tests), `delete` of single elements, missing elements and whole arrays, and for-in key iteration with value indirection.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.arrays.test.ts',
    lines: [147, 158, 167, 176, 185, 207, 240, 253, 264, 275, 288],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc25_split_subsep_and_array_field_rows',
    notes:
      'JBC-25 verifies portable AWK empty for-in, split() into arrays (explicit and whitespace separators, count and clear-before-fill), unique-value counting, SUBSEP matrix storage and parenthesised `(i,j) in a` membership, field-keyed accumulation, line storage by field, and array pre-increment.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.fields.test.ts',
    lines: [38, 52, 281],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc25_array_and_computed_field_rows',
    notes:
      'JBC-25 verifies portable AWK computed field access and field arithmetic; field mutation, iteration, and rebuild edge rows remain pending.',
  },
];

const jbc36OverlayE2eJsOnlyLines = [
  42, 48, 54, 60, 67, 76, 86, 94, 108, 116, 126, 138, 148, 158, 166,
  179, 187, 203, 213, 223, 233, 246, 265, 271, 276, 282, 287, 292, 298,
  304, 310, 317, 324, 337, 343, 349, 356, 374, 381, 387, 396, 406, 421,
  439, 461, 474, 485, 490, 498, 506, 519, 528, 537, 547, 552, 557, 563,
  572, 595,
];

const jbc36OverlaySecurityJsOnlyLines = [
  165, 640, 647, 653, 659, 666, 673, 679, 685, 692, 700, 708, 729, 803,
  818, 829, 845, 855, 871, 883, 898, 905, 911, 917, 923, 969, 987, 1002,
  1020, 1037, 1051, 1065, 1077, 1089, 1101, 1113, 1126, 1149, 1219, 1235,
  1245, 1256, 1262, 1267, 1272, 1278, 1283, 1298, 1308, 1377, 1385, 1395,
  1405, 1415, 1425, 1434, 1444, 1455, 1490, 1510, 1527, 1544,
];

const jbc36ReadWriteSecurityJsOnlyLines = [
  462, 477, 491, 502, 517, 530, 541, 594, 638, 647, 666, 683, 699, 715,
  733, 750, 767, 784, 804, 842, 873, 904, 917, 1015, 1046, 1058, 1073,
  1085, 1114, 1136, 1329, 1350, 1430,
];

const jbc36MountableSecurityJsOnlyLines = [
  175, 195, 215, 248, 266, 284, 297, 315, 336,
];

const jbc36InMemoryLazyJsOnlyLines = [
  315, 324, 337, 347, 360, 370, 380, 390, 398, 414, 422, 433, 442, 452,
];

const jbc36CaseGroups = [
  {
    file: 'packages/just-bash/src/Bash.general.test.ts',
    lines: [
      14, 22, 28, 34, 48, 54, 71, 89, 96, 105, 113, 122, 130, 138, 146,
      152, 160, 173, 179, 188, 229, 237, 246, 254, 264, 270, 277, 283,
      302, 308, 314, 320, 327, 335, 346, 365, 371, 377, 385, 394, 406,
      416, 426, 438, 447, 454, 463, 474, 480, 486, 498, 556, 563, 569,
      575,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::jbc36-core-runtime',
    rustTest: 'jbc36_core_runtime_shell_session_rows_are_virtual_and_stateful',
    notes:
      'JBC-36 verifies these core Bash facade rows with the in-process virtual session: pipes, redirection, env/default expansion, export/unset scope, &&/||/; chaining, exit status, cd/cwd handling, empty input, and whitespace. Escaped-quote parser semantics stay pending for the parser/interpreter slice.',
  },
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [257, 391, 516, 554, 611, 639],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::jbc36-session-isolation',
    rustTest: 'jbc36_exec_errexit_concurrent_env_cwd_and_file_rows_are_isolated',
    notes:
      'JBC-36 verifies concurrent virtual execs share the base session while preserving per-exec env/cwd isolation, file writes, sleep-delayed interleaving, and shell-option isolation without host shell fallback or JavaScript mock-clock injection.',
  },
  {
    file: 'packages/just-bash/src/encoding-pipeline.test.ts',
    lines: [23, 29, 35, 43, 64, 89, 193],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::jbc36-encoding-pipeline',
    rustTest: 'jbc36_utf8_redirection_and_text_stdin_rows_are_byte_safe',
    notes:
      'JBC-36 verifies text-to-byte wc counts, byte-producer file counts, UTF-8 redirection, cat redirect byte preservation, and text stdin byte counts through the virtual Rust pipeline. Tee, heredoc, function/subshell, byte-stdin-kind, and package encoding-helper rows remain pending or JS-only as exact rows say.',
  },
  {
    file: 'packages/just-bash/src/encoding-pipeline.test.ts',
    lines: [55, 71, 108, 136, 142, 165],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::jbc45-encoding-pipeline',
    rustTest: 'jbc45_encoding_pipeline_byte_text_contract_rows_are_byte_safe',
    notes:
      'JBC-45 verifies additional byte/text pipeline contract rows through the Rust virtual session: rev|base64 of reversed UTF-8 bytes, utf8 redirect of unmarked text stdout past the sampling window, tee byte-identical capture, bash -c piped-stdin forwarding, function-call piped stdin, and text-emitting custom-command UTF-8 byte counts. The sed-byte-count, binary cat|cat|cat round-trip, heredoc/here-string, group/subshell stdin, byte-emitting custom command, and byte-stdin-kind rows remain pending where the byte-tagged pipeline/compound-command support is not yet ported.',
  },
  {
    file: 'packages/just-bash/src/encoding-pipeline.test.ts',
    lines: [213],
    status: 'js-only-documented',
    owner: 'js-only:just-bash-encoding-helper-package-exports',
    rustTest: 'js-only:upstream-typescript-encoding-helper-exports',
    notes:
      'JBC-36 classifies the upstream package-entry encoding helper export row as TypeScript package API surface. Portable byte/text pipeline behavior is mapped to Rust execution rows separately.',
  },
  {
    file: 'packages/just-bash/src/cli/shell.test.ts',
    lines: [
      11, 21, 32, 45, 56, 67, 77, 89, 98, 110, 122, 132, 144, 156, 163,
      171, 180, 193, 199,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::jbc36-shell-session',
    rustTest: 'jbc36_core_runtime_shell_session_rows_are_virtual_and_stateful',
    notes:
      'JBC-36 maps the portable shell-session CLI rows to the Rust virtual session proof for cd/pwd, command chaining, export/unset scope, initial env availability, and exit status. The upstream interactive readline wrapper remains JS-only source tooling.',
  },
  {
    file: 'packages/just-bash/src/cli/just-bash.test.ts',
    lines: [233, 245, 257, 269],
    status: 'portable-verified',
    owner: 'crates/just-bash::cli::jbc36-errexit',
    rustTest: 'jbc36_cli_errexit_executes_runtime_stop_and_json_rows',
    notes:
      'JBC-36 verifies -e, --errexit, combined -ec, non-errexit continuation, and JSON stdout/stderr/exitCode shape through the Rust CLI planner plus in-memory execution backend.',
  },
  {
    file: 'packages/just-bash/src/cli/just-bash.test.ts',
    lines: [113, 124, 130, 146, 158, 170, 184, 194, 364],
    status: 'js-only-documented',
    owner: 'js-only:just-bash-cli-host-overlay-root',
    rustTest: 'js-only:upstream-host-backed-cli-overlay-root-and-allow-write',
    notes:
      'JBC-36 classifies these upstream CLI rows as host OverlayFS/root validation behavior for the TypeScript binary. The Rust CLI planner and execution tests keep CLI behavior virtual and host-agnostic.',
  },
  {
    file: 'packages/just-bash/src/cli/just-bash.bundle.test.ts',
    lines: [40, 46, 52, 62, 93],
    status: 'portable-verified',
    owner: 'crates/just-bash-napi::jbc36-cli-bundle',
    rustTest: 'jbc36_napi_cli_bundle_virtual_execution_and_errexit_rows',
    notes:
      'JBC-36 verifies the NAPI package/bundle execution surface with virtual cwd/files, echo, pipes, redirection, JSON-shaped exec result fields, and errexit behavior.',
  },
  {
    file: 'packages/just-bash/src/fs/read-write-fs/read-write-fs.piping.test.ts',
    lines: [36, 60, 78, 96, 119, 143],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::jbc36-read-write-piping',
    rustTest: 'jbc36_read_write_piping_large_virtual_data_rows',
    notes:
      'JBC-36 verifies the portable read/write piping behavior with virtual files: large and direct wc counts, small/medium sort-uniq pipelines, grep counting over large text, and byte-buffer persistence. Host ReadWriteFs adapter mechanics remain JS-only where exact rows require real roots or OS symlinks.',
  },
  {
    file: 'packages/just-bash/src/fs/mountable-fs/mountable-fs.test.ts',
    lines: [48],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::jbc36-mountable',
    rustTest: 'jbc36_mountable_construction_time_mount_rows_are_virtual',
    notes:
      'JBC-36 verifies construction-style virtual base plus mounted filesystem routing, mounted writes, directory listing, and invalid root mount rejection without host filesystem access.',
  },
  {
    file: 'packages/just-bash/src/fs/real-fs-utils.test.ts',
    lines: [162, 166, 173, 180, 189, 193, 202, 240],
    status: 'portable-verified',
    owner: 'crates/just-bash::path::jbc36-root-boundaries',
    rustTest: 'jbc36_real_fs_utils_virtual_root_boundary_rows',
    notes:
      'JBC-36 verifies pure path/root-boundary containment rows in Rust, including exact root, children, non-existent virtual children, sibling-prefix attacks, filesystem root rejection, and Windows-style boundary checks.',
  },
  {
    file: 'packages/just-bash/src/fs/real-fs-utils.test.ts',
    lines: [213, 229, 249, 258, 264, 277],
    status: 'js-only-documented',
    owner: 'js-only:just-bash-real-fs-host-validation',
    rustTest: 'js-only:upstream-realpath-host-directory-and-os-symlink-validation',
    notes:
      'JBC-36 classifies these rows as host realpath, host directory existence, and OS symlink validation behavior in the TypeScript filesystem adapter. Pure path containment rows are mapped to Rust tests separately.',
  },
  {
    file: 'packages/just-bash/src/fs/overlay-fs/overlay-fs.test.ts',
    lines: [20, 29, 35, 92, 685, 711, 725, 742, 755],
    status: 'js-only-documented',
    owner: 'js-only:just-bash-overlay-host-root-and-bashenv',
    rustTest: 'js-only:upstream-host-backed-overlayfs-root-and-bashenv-adapter',
    notes:
      'JBC-36 classifies these OverlayFS constructor and BashEnv rows as TypeScript host-root adapter behavior. Portable overlay precedence and virtual command execution are covered by Rust filesystem/runtime proofs.',
  },
  {
    file: 'packages/just-bash/src/fs/read-write-fs/read-write-fs.test.ts',
    lines: [19, 24, 33],
    status: 'js-only-documented',
    owner: 'js-only:just-bash-read-write-host-root',
    rustTest: 'js-only:upstream-host-backed-readwritefs-root-validation',
    notes:
      'JBC-36 classifies these ReadWriteFs constructor rows as host root directory validation for the TypeScript adapter; Rust execution remains virtual by default.',
  },
  {
    file: 'packages/just-bash/src/fs/overlay-fs/overlay-fs.e2e.test.ts',
    lines: jbc36OverlayE2eJsOnlyLines,
    status: 'js-only-documented',
    owner: 'js-only:just-bash-overlay-e2e-host-adapter',
    rustTest: 'js-only:upstream-host-backed-overlayfs-e2e-real-file-fixtures',
    notes:
      'JBC-36 classifies the upstream OverlayFS E2E suite as host-backed real-file adapter coverage. The portable command, redirection, virtual file, and session behaviors exercised by those workflows are mapped to Rust command/runtime tests in their owned rows.',
  },
  {
    file: 'packages/just-bash/src/fs/overlay-fs/overlay-fs.security.test.ts',
    lines: jbc36OverlaySecurityJsOnlyLines,
    status: 'js-only-documented',
    owner: 'js-only:just-bash-overlay-os-symlink-security',
    rustTest: 'js-only:upstream-host-backed-overlayfs-os-symlink-and-realpath-security',
    notes:
      'JBC-36 classifies these OverlayFS security rows as host realpath, OS symlink, permission, and path-leak probes for the TypeScript adapter. Rust virtual symlink and no-host-leak policy rows are verified separately.',
  },
  {
    file: 'packages/just-bash/src/fs/read-write-fs/read-write-fs.security.test.ts',
    lines: jbc36ReadWriteSecurityJsOnlyLines,
    status: 'js-only-documented',
    owner: 'js-only:just-bash-read-write-os-symlink-security',
    rustTest: 'js-only:upstream-host-backed-readwritefs-os-symlink-and-realpath-security',
    notes:
      'JBC-36 classifies these ReadWriteFs security rows as host root, OS symlink, realpath, permission, and sanitized host-error probes. Rust virtual traversal and sanitized-error behavior stays mapped to virtual filesystem tests.',
  },
  {
    file: 'packages/just-bash/src/fs/mountable-fs/mountable-fs.security.test.ts',
    lines: jbc36MountableSecurityJsOnlyLines,
    status: 'js-only-documented',
    owner: 'js-only:just-bash-mountable-host-adapter-security',
    rustTest: 'js-only:upstream-mounted-host-readwrite-overlayfs-security',
    notes:
      'JBC-36 classifies these mountable security rows as mounted host ReadWriteFs/OverlayFs adapter behavior involving real OS symlinks and real roots. Portable virtual mount routing is mapped to Rust MountableFileSystem tests.',
  },
  {
    file: 'packages/just-bash/src/fs/cross-fs-security.test.ts',
    lines: [603, 631, 676, 690, 711, 1331, 1503, 1767],
    status: 'js-only-documented',
    owner: 'js-only:just-bash-cross-fs-host-adapter-security',
    rustTest: 'js-only:upstream-cross-fs-real-root-and-os-symlink-security',
    notes:
      'JBC-36 classifies these cross-FS rows as host real-root and OS symlink adapter security scenarios. Virtual mount boundaries and symlink policy are covered by Rust filesystem tests separately.',
  },
  {
    file: 'packages/just-bash/src/fs/in-memory-fs/in-memory-fs.test.ts',
    lines: jbc36InMemoryLazyJsOnlyLines,
    status: 'js-only-documented',
    owner: 'js-only:just-bash-in-memory-lazy-provider-callbacks',
    rustTest: 'js-only:upstream-javascript-lazy-file-provider-callbacks',
    notes:
      'JBC-36 classifies lazy file provider rows as JavaScript callback/promise behavior in the TypeScript InMemoryFs adapter. Rust VirtualFileSystem uses eager virtual file content, with byte persistence mapped separately.',
  },
];

const jbc33SourceGroups = [
  {
    files: [
      'packages/just-bash/src/ast/types.ts',
      'packages/just-bash/src/parser/arithmetic-parser.ts',
      'packages/just-bash/src/parser/arithmetic-primaries.ts',
      'packages/just-bash/src/parser/command-parser.ts',
      'packages/just-bash/src/parser/compound-parser.ts',
      'packages/just-bash/src/parser/conditional-parser.ts',
      'packages/just-bash/src/parser/expansion-parser.ts',
      'packages/just-bash/src/parser/lexer.ts',
      'packages/just-bash/src/parser/parser-substitution.ts',
      'packages/just-bash/src/parser/parser.ts',
      'packages/just-bash/src/parser/types.ts',
      'packages/just-bash/src/parser/word-parser.ts',
      'packages/just-bash/src/interpreter/arithmetic.ts',
      'packages/just-bash/src/interpreter/assignment-expansion.ts',
      'packages/just-bash/src/interpreter/conditionals.ts',
      'packages/just-bash/src/interpreter/control-flow.ts',
      'packages/just-bash/src/interpreter/expansion.ts',
      'packages/just-bash/src/interpreter/functions.ts',
      'packages/just-bash/src/interpreter/interpreter.ts',
      'packages/just-bash/src/interpreter/simple-command-assignments.ts',
      'packages/just-bash/src/interpreter/subshell-group.ts',
      'packages/just-bash/src/interpreter/types.ts',
      'packages/just-bash/src/interpreter/expansion/arith-text-expansion.ts',
      'packages/just-bash/src/interpreter/expansion/brace-range.ts',
      'packages/just-bash/src/interpreter/expansion/command-substitution.ts',
      'packages/just-bash/src/interpreter/expansion/parameter-ops.ts',
      'packages/just-bash/src/interpreter/expansion/quoting.ts',
      'packages/just-bash/src/interpreter/expansion/tilde.ts',
      'packages/just-bash/src/interpreter/expansion/unquoted-expansion.ts',
      'packages/just-bash/src/interpreter/expansion/variable.ts',
      'packages/just-bash/src/interpreter/expansion/word-split.ts',
      'packages/just-bash/src/interpreter/helpers/condition.ts',
      'packages/just-bash/src/interpreter/helpers/errors.ts',
      'packages/just-bash/src/interpreter/helpers/ifs.ts',
      'packages/just-bash/src/interpreter/helpers/loop.ts',
      'packages/just-bash/src/interpreter/helpers/quoting.ts',
      'packages/just-bash/src/interpreter/helpers/result.ts',
      'packages/just-bash/src/interpreter/helpers/statements.ts',
      'packages/just-bash/src/interpreter/helpers/string-compare.ts',
      'packages/just-bash/src/interpreter/helpers/string-tests.ts',
      'packages/just-bash/src/interpreter/helpers/variable-tests.ts',
      'packages/just-bash/src/interpreter/helpers/word-matching.ts',
      'packages/just-bash/src/interpreter/helpers/word-parts.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    notes:
      'JBC-33 verifies the Rust parser/interpreter AST, variable, operator, loop, control-flow, function, local-scope, command-substitution, arithmetic, quoting, and prototype-key data rows covered by the named shell tests.',
  },
  {
    files: [
      'packages/just-bash/src/regex/index.ts',
      'packages/just-bash/src/regex/user-regex.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::regex',
    notes:
      'JBC-33 verifies portable user-regex matching, captures, replacement, split, search, Unicode, dotAll, anchors, and group behavior with Rust regex tests; JavaScript RegExp wrapper rows are documented separately.',
  },
];

const jbc33RegexJsOnlyLines = [
  46, 69, 97, 121, 127, 135, 191, 208, 217, 228, 239, 252, 265, 278, 284,
  312, 330, 337, 346, 353, 367, 418, 439,
];

const jbc33CaseGroups = [
  {
    file: 'packages/just-bash/src/syntax/variables.test.ts',
    lines: [
      6, 12, 18, 24, 30, 36, 42, 48, 54, 60, 69, 75, 81, 89, 95, 101,
      107, 113, 119, 125, 139, 145, 151, 165, 173, 179, 185, 193, 200,
      208, 215, 221, 227, 233, 241, 247, 253, 260, 267, 273, 279,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbc33_syntax_variables_operators_and_loop_rows_match_upstream',
    notes:
      'JBC-33 verifies variable/default expansion, quoting, echo escape rows except the byte-level backslash row, exit/unknown/whitespace behavior, and simple assignment/export rows through the Rust shell interpreter.',
  },
  {
    file: 'packages/just-bash/src/syntax/operators.test.ts',
    lines: [
      6, 13, 20, 27, 52, 59, 68, 75, 82, 118, 129, 135, 141, 154, 161,
      169, 177, 213, 223, 229, 235, 281, 287, 297, 303, 311, 319, 325,
      333, 347, 353,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbc33_syntax_variables_operators_and_loop_rows_match_upstream',
    notes:
      'JBC-33 verifies portable &&, ||, semicolon, pipe, grep-pipeline, and redirection syntax rows through the Rust shell. Rows that require command-family head/tail/wc/mkdir/rm behavior stay pending with command owners.',
  },
  {
    file: 'packages/just-bash/src/syntax/loops.test.ts',
    lines: [7, 14, 20, 26, 33, 41, 54, 163],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbc33_syntax_variables_operators_and_loop_rows_match_upstream',
    notes:
      'JBC-33 verifies portable for-loop parsing/execution, file operands, exit status, and nested for loops through the Rust shell. While/until/protection rows remain pending until separately proven.',
  },
  {
    file: 'packages/just-bash/src/syntax/control-flow.test.ts',
    lines: [
      6, 13, 20, 29, 39, 47, 55, 65, 80, 87, 95, 103, 112, 120, 132,
      138, 144, 150, 156, 162, 170, 178, 185, 214, 246, 253,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbc33_syntax_control_flow_functions_and_local_rows_match_upstream',
    notes:
      'JBC-33 verifies portable if/elif/nesting, function definition/calls/args/status, builtin override, and selected local variable rows through the Rust shell. File-count, negation, and deeper local-scope rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/interpreter/prototype-pollution.test.ts',
    lines: [
      36, 43, 50, 57, 64, 71, 80, 87, 94, 101, 112, 119, 126, 155, 162,
      178, 187, 196, 203, 212, 219, 279, 288, 299, 314, 321, 328, 335,
      545, 558, 571, 578, 614, 776, 907, 916,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbc33_interpreter_prototype_keywords_remain_plain_shell_data',
    notes:
      'JBC-33 verifies prototype-like keywords remain plain Rust shell data across echo, variable assignment, unset, functions, command substitution, arithmetic, loops, case, literal punctuation, heredocs, brace expansion, export, and subshell rows.',
  },
  {
    file: 'packages/just-bash/src/transform/transform.test.ts',
    lines: [254, 263, 279, 286],
    status: 'portable-verified',
    owner: 'crates/just-bash::transform::pipeline',
    rustTest: 'jbc33_transform_exec_metadata_rows_are_portable',
    notes:
      'JBC-33 verifies transform exec integration via Rust transform pipeline metadata, rewrite, no-plugin, and metadata-merge proof cases.',
  },
  {
    file: 'packages/just-bash/src/regex/user-regex.test.ts',
    lines: [
      11, 17, 25, 31, 36, 41, 55, 64, 79, 86, 92, 106, 111, 116, 143,
      148, 153, 158, 165, 170, 177, 185, 247, 260, 273, 292, 299, 306,
      321, 389, 424, 430, 451, 456, 461, 469, 474, 481, 486, 493, 498,
      505, 511, 520,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::regex',
    rustTest: 'jbc33_user_regex_portable_match_search_split_and_replace_rows',
    notes:
      'JBC-33 verifies portable regex construction, matching, capture, replacement, split, search, iteration, Unicode, dotAll, anchors, named groups, nested groups, and non-capturing groups with Rust regex.',
  },
  {
    file: 'packages/just-bash/src/regex/user-regex.test.ts',
    lines: [
      33, 38, 43, 49, 200, 223, 249, 255, 393, 404, 412, 453, 463, 464,
      471, 483, 488, 495, 500,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::regex',
    rustTest:
      'jbc33_user_regex_portable_membership_zero_length_multiline_and_unicode_rows',
    notes:
      'JBC-33 verifies portable user-regex membership testing, zero-length word-boundary global matching, multiline anchoring, cached-matcher reuse, escaped specials, anchors, the empty pattern, Unicode literal/escape matching, and the dotAll flag with Rust regex; JavaScript lastIndex state and native RegExp wrapper rows stay documented separately.',
  },
  {
    file: 'packages/just-bash/src/regex/user-regex.test.ts',
    lines: jbc33RegexJsOnlyLines,
    status: 'js-only-documented',
    owner: 'crates/just-bash::regex::js-wrapper-exception',
    rustTest: 'js-only:user-regex-regexp-wrapper-callback-and-lastindex-api',
    notes:
      'JBC-33 classifies JavaScript RegExp wrapper identity, callback replacement, lastIndex state, native RegExp access, factory instance checks, and TypeScript RegexLike interface rows as JS-only API behavior; portable regex semantics are verified separately.',
  },
];

const jbr1CaseGroups = [
  {
    file: 'packages/just-bash/src/syntax/parser-edge-cases.test.ts',
    lines: [
      6, 12, 18, 24, 36, 42, 50, 56, 62, 74, 80, 88, 94, 100, 106, 112,
      118, 124, 130, 139, 145, 151, 157, 163, 208, 214, 220, 226, 232, 238,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbr1_syntax_parser_edge_cases_match_upstream',
    notes:
      'JBR-1 verifies portable parser edge cases — nested/empty/adjacent quoting, escape sequences, ${VAR:-default} expansion, $? and undefined-variable expansion, multi-space/tab/leading/trailing whitespace normalization, and operator parsing without spaces — through the Rust shell interpreter.',
  },
];

const jbR3SyntaxCaseGroups = [
  {
    file: 'packages/just-bash/src/syntax/subshell-args.test.ts',
    lines: [80, 88, 96, 105, 113, 122, 130, 137, 144, 151, 158],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jb_subshell_args_operator_precedence_rows_match_upstream',
    notes:
      'JBR-3 verifies portable operator-precedence rows — `!` binding tighter than `&&`/`||`, `!` negating whole pipelines, `&&`/`||` left-associativity, `;` lowest precedence, and stacked `!` toggling exit status — through the Rust shell. The positional-argument (`bash -c`, `sh -c`, script-file) and `xargs` rows require subshell-arg and command-family behavior and stay pending with command owners.',
  },
  {
    file: 'packages/just-bash/src/syntax/variables.test.ts',
    lines: [131],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jb_syntax_variables_quoted_newline_row_matches_upstream',
    notes:
      'JBR-3 verifies a literal newline inside double quotes is preserved verbatim through the Rust shell quoting pipeline. The byte-level backslash escape row (L157) stays pending as a documented `echo -e` collapse divergence.',
  },
  {
    file: 'packages/just-bash/src/syntax/loops.test.ts',
    lines: [69, 86, 97, 171, 190],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jb_syntax_loops_while_guard_rows_match_upstream',
    notes:
      'JBR-3 verifies portable `while grep -q` guard loops that flip a virtual-filesystem file and last-command exit-status propagation through the Rust shell. Infinite-loop protection (L135/L145/L153), `until grep -q` re-evaluation timing rows, and loop-variable cleanup (L60) stay pending; the no-op `while false`/`until true` rows (L79/L117) are verified separately by JBC-13.',
  },
  {
    file: 'packages/just-bash/src/syntax/operators.test.ts',
    lines: [43, 91, 102, 147, 185, 191, 197, 205, 339],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jb_syntax_operators_logical_and_redirection_rows_match_upstream',
    notes:
      'JBR-3 verifies portable operator rows — `&&` short-circuit protecting the filesystem, all-failing `||` chains, `;` exit-status propagation, mixed `&&`/`||`/`;` precedence chains, and cross-`exec` `>>` appends — through the Rust shell. Rows needing head/tail/wc command families stay pending with command owners; L154/L275 are verified separately by JBC-33.',
  },
];

const jbR2CaseGroups = [
  {
    file: 'packages/just-bash/src/interpreter/control-flow.test.ts',
    lines: [42, 60, 73, 247, 260, 272, 291, 304, 318, 331, 343, 358],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'r2_interpreter_control_flow_rows_match_upstream',
    notes:
      'JBR-2 verifies portable interpreter control-flow rows — elif chains, &&-conditioned and nested if, while/until execution and skip, nested while, break/continue, and break/continue N — through the Rust shell. IFS-splitting, positional-parameter, invalid-identifier-error, and C-style for (( )) rows stay pending.',
  },
  {
    file: 'packages/just-bash/src/syntax/parser-edge-cases.test.ts',
    lines: [30, 68, 186, 193, 247, 254, 260, 266, 272, 281, 288, 295, 303, 311, 318, 324],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'r2_syntax_parser_edge_cases_and_redirection_rows_match_upstream',
    notes:
      'JBR-2 verifies additional portable parser-edge-cases rows — adjacent quoting, escaped space, && / || precedence and semicolons, pipes with semicolons, assignment-vs-argument, empty/whitespace input, bare/double semicolon syntax errors, long argument, unicode, double-quoted newline, and 2>/dev/null / 2>&1 redirection — through the Rust shell.',
  },
  {
    file: 'packages/just-bash/src/syntax/operators.test.ts',
    lines: [275],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'r2_syntax_parser_edge_cases_and_redirection_rows_match_upstream',
    notes:
      'JBR-2 verifies that the operators.test.ts `||`-is-not-a-pipe row falls back to the right-hand command when the left command fails, through the Rust shell.',
  },
  {
    file: 'packages/just-bash/src/interpreter/arithmetic.test.ts',
    lines: [487],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'r2_interpreter_arithmetic_command_assignment_row_matches_upstream',
    notes:
      'JBR-2 verifies the portable arithmetic-command assignment row `(( x = 5 + 3 )); echo $x` through the Rust shell. Division/modulo-by-zero and negative-exponent error rows stay pending until arithmetic error reporting lands.',
  },
];

const jbpiParserInterpreterCaseGroups = [
  {
    file: 'packages/just-bash/src/syntax/control-flow.test.ts',
    lines: [73, 222, 238, 261, 269, 313, 321],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest:
      'jbpi_syntax_control_flow_function_local_and_negation_rows_match_upstream',
    notes:
      'JB-PI verifies portable syntax control-flow rows through the Rust shell: an if body yields its last command exit code; `local` inside a function does not leak / is restored / can be declared without a value / is independent in nested calls; and `!` negates a failing grep to success and a succeeding grep to failure.',
  },
  {
    file: 'packages/just-bash/src/interpreter/control-flow.test.ts',
    lines: [456, 471, 484, 513],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest:
      'jbpi_interpreter_control_flow_nested_and_quoted_case_rows_match_upstream',
    notes:
      'JB-PI verifies portable interpreter control-flow nested-structure rows through the Rust shell: a single-quoted case pattern matches a literal `*`, if-inside-for, for-inside-if, and while-inside-case.',
  },
  {
    file: 'packages/just-bash/src/syntax/loops.test.ts',
    lines: [184, 200],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest:
      'jbpi_syntax_loops_for_without_semicolon_and_malformed_rows_match_upstream',
    notes:
      'JB-PI verifies portable loops rows through the Rust shell: a `for` loop without a semicolon before `do` still iterates, and a `for` header missing `in` is a syntax error (exit 2).',
  },
  {
    file: 'packages/just-bash/src/syntax/parser-edge-cases.test.ts',
    lines: [171, 178],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest:
      'jbpi_syntax_parser_edge_cases_redirection_without_space_rows_match_upstream',
    notes:
      'JB-PI verifies portable parser-edge-cases redirection rows through the Rust shell virtual filesystem: `>` and `>>` without a space around the operator truncate and append respectively.',
  },
  {
    file: 'packages/just-bash/src/syntax/here-document.test.ts',
    lines: [173, 188],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest:
      'jbpi_syntax_here_document_whitespace_preservation_rows_match_upstream',
    notes:
      'JB-PI verifies portable here-document whitespace-preservation rows through the Rust shell: an indented heredoc body keeps its own indentation, and a quoted-delimiter heredoc preserves a leading-space ASCII-art triangle verbatim.',
  },
  {
    file: 'packages/just-bash/src/interpreter/helpers/xtrace.test.ts',
    lines: [253],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbpi_interpreter_xtrace_command_substitution_row_matches_upstream',
    notes:
      'JB-PI verifies the portable xtrace row through the Rust shell: under `set -x` the command run inside a command substitution is traced to stderr while its output still flows to stdout.',
  },
  {
    file: 'packages/just-bash/src/interpreter/assoc-array.test.ts',
    lines: [143],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest:
      'jbpi_interpreter_assoc_array_indexed_numeric_indices_row_matches_upstream',
    notes:
      'JB-PI verifies the portable indexed-array row through the Rust shell: `declare -a arr` with numeric-index element assignments reads back the correct values.',
  },
  {
    file: 'packages/just-bash/src/interpreter/builtins/set.test.ts',
    lines: [44, 55, 66, 87, 100, 120, 130, 170, 180, 191, 203],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbpi_interpreter_set_nounset_non_error_rows_match_upstream',
    notes:
      'JB-PI verifies the portable `set -u` (nounset) non-error rows through the Rust shell: a set variable and an empty-string value read without error, `+u` / `+o nounset` disable it, the `$?` / `$#` / `$@` special vars never trip it, `${var:-}` / `${var:=}` / `${var:+}` parameter expansion is allowed, and `set -eu` with a set variable runs cleanly. The unbound-variable error rows stay pending until nounset error reporting lands.',
  },
  {
    file: 'packages/just-bash/src/syntax/parse-errors.test.ts',
    lines: [
      6, 13, 19, 29, 35, 43, 50, 57, 75, 82, 89, 99, 106, 115, 122, 131,
      139, 156, 165, 188, 197, 204, 210, 216,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbpi_syntax_parse_errors_match_upstream',
    notes:
      'JB-PI verifies portable parse-error rows through the Rust parser/interpreter: unclosed/missing-keyword if/for/while/until syntax errors (exit 2 with "syntax error"), the elif-condition selection, else/fi without if, a digit-starting function name accepted, unclosed function body, graceful handling of unclosed quotes / missing redirect target / empty pipe and &&/|| operands, the unknown-command 127 row, and the local-outside-function exit-1 row. The runtime invalid-identifier row (L64) and filesystem-backed redirect/path/cat rows (L147, L172, L179) stay pending.',
  },
  {
    file: 'packages/just-bash/src/syntax/set-errexit.test.ts',
    lines: [6, 18, 29, 43, 56, 73],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbpi_syntax_set_errexit_match_upstream',
    notes:
      'JB-PI verifies portable set -e (errexit) rows through the Rust parser/interpreter: `set -e` exits on the first failure, execution continues without it, success does not exit, `set +e` disables and `set -e` re-enables, and `set -o errexit` enables it. The same test also exercises the &&/||, if/elif-condition, while/until-condition and -body, negated-command, and preserve-exit-code exemptions; the unimplemented combined-flag (`-ee`/`-ze`/`-ez`) and `set` help/list/invalid-option rows stay pending.',
  },
  {
    file: 'packages/just-bash/src/syntax/composition.test.ts',
    lines: [
      6, 27, 53, 65, 75, 84, 92, 114, 125, 136, 148, 162, 178, 222, 244,
      258, 270, 282, 294, 305, 339, 353, 367, 381, 455, 479, 490, 497,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'r10jb_syntax_composition_operator_and_loop_rows_match_upstream',
    notes:
      'R10JB verifies portable syntax-feature composition rows through the Rust shell: command substitution in if/case words and for lists, here documents inside if/case/loop blocks and with command/arithmetic expansion, pipes inside if/case branches, here-doc-through-pipe counting, case-in-function and case-in-loop, command substitution and arithmetic in functions, nested command substitution, the failed-command-in-substitution / empty-here-doc / no-matching-case edge rows. Rows requiring mkdir/uniq/head/tail command families or `[[ ]]` arithmetic comparison stay pending with their command owners.',
  },
  {
    file: 'packages/just-bash/src/syntax/operators.test.ts',
    lines: [241],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'r10jb_syntax_composition_operator_and_loop_rows_match_upstream',
    notes:
      'R10JB verifies the portable `echo -e | wc -l` line-count pipe row through the Rust shell. The head/tail/mkdir mixed-operator rows stay pending with their command owners.',
  },
  {
    file: 'packages/just-bash/src/syntax/loops.test.ts',
    lines: [124],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'r10jb_syntax_composition_operator_and_loop_rows_match_upstream',
    notes:
      'R10JB verifies the portable until-loop row that executes once when its condition is initially false (a grep -q over a virtual file) through the Rust shell.',
  },
];

const jbc35CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/awk/awk.fields.test.ts',
    lines: [
      45, 61, 68, 77, 86, 95, 127, 136, 156, 165, 183, 290, 297, 306,
      346, 355, 362, 371, 382, 391, 400,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc35_field_rebuild_printf_and_edge_rows',
    notes:
      'JBC-35 verifies portable AWK variable field indexes, field assignment, NF/OFS record rebuilds, computed field mutation, whitespace and delimiter field edges, and field predicate rows; loops, delete/split, getline, redirection, parser, and error rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.output.test.ts',
    lines: [115, 124, 133],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc35_field_rebuild_printf_and_edge_rows',
    notes:
      'JBC-35 verifies portable AWK printf length modifiers plus positive and negative dynamic width formatting; output redirection rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.edge-cases.test.ts',
    lines: [323, 330, 339, 350, 359, 368, 377, 386, 399, 408, 417],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc35_field_rebuild_printf_and_edge_rows',
    notes:
      'JBC-35 verifies portable AWK print/printf, multiple BEGIN/END blocks, BEGIN-scoped variables, END NR, and string-number coercion edge rows; loop and array-iteration edge rows remain pending.',
  },
];

const jbc42CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/awk/awk.parsing.test.ts',
    lines: [
      6, 13, 22, 33, 50, 60, 71, 81, 91, 101, 112, 125, 143, 152, 161,
      170, 179, 186, 193, 200, 207, 216, 225, 234, 243, 252, 261, 272, 282,
      294, 303, 313, 322, 331, 342, 349, 358, 367, 374, 383, 392,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc42_parser_comment_numeric_and_if_rows',
    notes:
      'JBC-42 verifies portable AWK whitespace and newline continuation parsing, string escapes, scientific and leading-decimal numeric literals, line comments, simple if/else actions, regex-literal parsing (special chars, brackets, anchors, quantifiers, regex-vs-division disambiguation), two-character/compound-assignment/increment-decrement/logical/regex operator parsing, and block-structure parsing (empty blocks, multiple rules, BEGIN+END, pattern-without-action, action-without-pattern); loops, for-in, getline, output redirection, and parser error edges remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.parsing.test.ts',
    lines: [439, 448, 457],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc42_user_function_and_conditional_flow_rows',
    notes:
      'JBC-42 verifies portable AWK function definition parsing for no-parameter, parameterized, and multiple user-defined functions; loops, for-in, getline, output redirection, and parser error edges remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.functions.test.ts',
    lines: [6, 15, 24, 35, 45, 55, 65, 76, 85, 96, 107, 118, 129],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc42_user_function_and_conditional_flow_rows',
    notes:
      'JBC-42 verifies portable AWK user-defined functions with scalar returns, parameter-local restoration, extra local parameters, recursion, nested calls, string returns, BEGIN calls, and multiple definitions; loops, array passing, getline, output redirection, and broader parser errors remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.functions.test.ts',
    lines: [199, 223, 234],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc42_user_function_and_conditional_flow_rows',
    notes:
      'JBC-42 verifies portable AWK conditional next and exit flow rows without claiming loop break/continue, do-while, getline, or output redirection support.',
  },
];

const jbcAwkEdgeCaseGroups = [
  {
    file: 'packages/just-bash/src/commands/awk/awk.edge-cases.test.ts',
    lines: [44, 53, 64, 71, 82, 91, 98, 105, 112, 122, 132, 141, 159, 168, 175, 184, 193],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_edge_special_chars_numeric_string_and_regex_rows',
    notes:
      'JBC-AWK-EDGE verifies portable AWK handling of literal quotes/backslashes/brackets/dollar/ampersand field data, very large and very small numeric magnitudes formatted with JS-compatible scientific notation, negative zero, floating-point precision, integer overflow, empty-string comparison, length() of empty and space-only records, and anchored/escaped/empty regex matches. The for-loop string-builder row (line 150) remains pending because C-style for is not yet implemented.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.arrays.test.ts',
    lines: [297],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_edge_special_chars_numeric_string_and_regex_rows',
    notes:
      'JBC-AWK-EDGE verifies portable AWK post-increment of an array element yields the pre-increment value while updating the stored entry.',
  },
  {
    file: 'packages/just-bash/src/commands/awk/awk.binary.test.ts',
    lines: [5],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::awk',
    rustTest: 'awk_jbc_edge_special_chars_numeric_string_and_regex_rows',
    notes:
      'JBC-AWK-EDGE verifies portable AWK arithmetic over a file whose bytes are processed as text records ("1 2\\n3 4\\n" -> 3, 7).',
  },
];

const jbc37CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/html-to-markdown/html-to-markdown.test.ts',
    lines: [
      6, 16, 26, 35, 44, 55, 66, 76, 85, 94, 105, 114, 123, 132, 139,
      151, 160, 171, 180, 189, 198, 210, 222, 233, 246, 258, 265, 274,
      284,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::html-to-markdown',
    rustTest: 'structured_data_jbc37_html_to_markdown_rows',
    notes:
      'JBC-37 implements deterministic in-memory html-to-markdown conversion for headings, paragraphs, links, emphasis, lists, code, images, blockquotes, options, file input, help text, script/style stripping, empty/plain/nested HTML, and unknown-option diagnostics.',
  },
  {
    file: 'packages/just-bash/src/commands/html-to-markdown/html-to-markdown.utf8-stdin.test.ts',
    lines: [5],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::html-to-markdown',
    rustTest: 'structured_data_jbc37_html_to_markdown_rows',
    notes:
      'JBC-37 verifies html-to-markdown preserves multibyte UTF-8 text through the Rust virtual pipe path.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.basic.test.ts',
    lines: [119, 130, 136, 142, 148, 154, 162, 168],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jbc37_jq_path_math_dot_and_safe_key_rows',
    notes:
      'JBC-37 verifies portable jq multi-pipe chains, array/string slicing, negative slice bounds, and comma-output streams over deterministic JSON stdin.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.functions.test.ts',
    lines: [163, 217, 227, 251, 259, 267, 277, 287, 294, 301, 308, 316, 322],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jbc37_jq_path_math_dot_and_safe_key_rows',
    notes:
      'JBC-37 verifies jq flatten depth, with_entries, transpose, limit, getpath, setpath, recursive number selection, pow, atan2, and nonnumeric null results.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.dot-adjacency.test.ts',
    lines: [6, 25, 40, 62, 77, 85, 92, 99, 110, 116, 125, 131],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jbc37_jq_path_math_dot_and_safe_key_rows',
    notes:
      'JBC-37 verifies jq adjacent keyword/string field access and fail-closed whitespace-sensitive dot selector errors.',
  },
  {
    file: 'packages/just-bash/src/commands/jq/jq.utf8-stdin.test.ts',
    lines: [8],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jbc37_jq_path_math_dot_and_safe_key_rows',
    notes:
      'JBC-37 verifies jq preserves multibyte UTF-8 string values through the Rust virtual pipe path.',
  },
  {
    file: 'packages/just-bash/src/commands/query-engine/safe-object.test.ts',
    lines: [48, 54, 67, 199],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jbc37_jq_path_math_dot_and_safe_key_rows',
    notes:
      'JBC-37 verifies the Rust query helpers filter the upstream extended dangerous-key set during constructed object/from_entries insertion while preserving safe keys.',
  },
  {
    file: 'packages/just-bash/src/commands/query-engine/safe-object.sanitize-parsed-data.test.ts',
    lines: [5, 30, 37, 47],
    status: 'js-only-documented',
    owner: 'docs/just-bash::query-engine-js-object-sanitizer',
    rustTest: 'js-only-documented',
    notes:
      'JBC-37 documents these rows as JavaScript object-graph sanitizer behavior for null prototypes, Date instances, cycles, and shared references; Rust serde_json inputs do not expose JS prototypes or cyclic object identity.',
  },
  {
    file: 'packages/just-bash/src/commands/yq/yq.format-strings.test.ts',
    lines: [
      8, 15, 24, 33, 42, 53, 62, 69, 78, 85, 92, 101, 109, 116, 125,
      134, 141, 148, 160, 167, 176, 183, 190, 197, 204,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jbc37_yq_format_and_utf8_rows',
    notes:
      'JBC-37 verifies yq format-string operators @base64, @base64d, @uri, @csv, @tsv, @json, @html, @sh, and @text including unicode, null, numeric, quote, and non-string/array edge cases.',
  },
  {
    file: 'packages/just-bash/src/commands/yq/yq.utf8-stdin.test.ts',
    lines: [5],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jbc37_yq_format_and_utf8_rows',
    notes:
      'JBC-37 verifies yq preserves multibyte UTF-8 YAML values through the Rust virtual pipe path.',
  },
  {
    file: 'packages/just-bash/src/commands/sqlite3/sqlite3.output-modes.test.ts',
    lines: [15, 44, 55, 65, 89, 115, 127, 138],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::sqlite3',
    rustTest: 'structured_data_jbc37_sqlite3_formatter_and_utf8_rows',
    notes:
      'JBC-37 verifies deterministic sqlite3 output modes for escaped CSV, column, table, markdown, box, html, HTML escaping, and ASCII separators over the Rust in-memory SQL engine.',
  },
  {
    file: 'packages/just-bash/src/commands/sqlite3/sqlite3.formatters.test.ts',
    lines: [
      31, 51, 62, 72, 94, 103, 114, 155, 165, 177, 190, 202, 213, 220,
      228, 237, 246, 255, 264, 275, 287, 296, 305,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::sqlite3',
    rustTest: 'structured_data_jbc37_sqlite3_formatter_and_utf8_rows',
    notes:
      'JBC-37 verifies sqlite3 formatter edge rows for CSV quoting, empty JSON results, JSON escaped characters, HTML escaping/header cells, column/table/box rendering, quote/nullvalue modes, BLOB text decoding, and combined options.',
  },
  {
    file: 'packages/just-bash/src/commands/sqlite3/sqlite3.utf8-stdin.test.ts',
    lines: [5],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::sqlite3',
    rustTest: 'structured_data_jbc37_sqlite3_formatter_and_utf8_rows',
    notes:
      'JBC-37 verifies sqlite3 preserves multibyte UTF-8 SQL string literals through the Rust virtual pipe path.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.utf8-stdin.test.ts',
    lines: [5],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::xan',
    rustTest: 'structured_data_jbc37_xan_utf8_stdin_row',
    notes:
      'JBC-37 verifies xan preserves multibyte UTF-8 CSV fields through the Rust virtual pipe path.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.agg.test.ts',
    lines: [
      10, 19, 28, 37, 46, 55, 64, 73, 82, 91, 100, 111, 124, 137, 146, 157,
      166, 179,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_agg_aggregations',
    notes:
      'JBC verifies portable xan agg count/count(expr)/sum/mean/avg/min/max/first/last/median/multiple/all/any/mode/cardinality/values/distinct_values and computed-expression aggregation over in-memory CSV.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.basic.test.ts',
    lines: [222, 233, 242, 255, 266, 275],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_sample_and_flatten_rows',
    notes:
      'JBC verifies portable xan sample positional/seeded/error-without-size and xan flatten vertical record display, -l limit, and f alias over in-memory CSV.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.frequency.test.ts',
    lines: [12, 22, 34, 43, 50, 61, 75],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_frequency_rows',
    notes:
      'JBC verifies portable xan frequency all-columns, -s column select, -l limit, empty-value <empty> display, equal-count stability ordering, -g groupby header, and -A show-all (limit 0) over in-memory CSV.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.filter-sort.test.ts',
    lines: [129, 138, 149, 158, 185],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_top_transpose_fixlengths_split_search_rows',
    notes:
      'JBC verifies portable xan top numeric-descending and -R bottom-N selection, plus xan search regex/-v inverted matching and the precise invalid-regex-pattern error over in-memory CSV.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.data.test.ts',
    lines: [85, 98, 108, 147, 156, 165, 176, 185, 194],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_top_transpose_fixlengths_split_search_rows',
    notes:
      'JBC verifies portable xan transpose (multi-row, single-column, header-only), xan fixlengths padding/truncation/custom-default, and xan split -c/-S part counting plus the missing-mode error over in-memory CSV with virtual-file output.',
  },
  {
    file: 'packages/just-bash/src/commands/yq/yq.prototype-pollution.test.ts',
    lines: [21, 34, 46, 58, 67, 76, 87, 97, 109, 120],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_yq_prototype_pollution_defense_rows',
    notes:
      'JBC-yq verifies the Rust yq engine reads dangerous YAML/JSON keys (constructor, __proto__, prototype) as ordinary data, lists them via keys/to_entries, answers has() correctly, resolves $ENV entries named after the keywords, merges objects with add, and reads them through getpath without prototype pollution.',
  },
  {
    file: 'packages/just-bash/src/commands/yq/yq.prototype-pollution.test.ts',
    lines: [131, 145],
    status: 'js-only-documented',
    owner: 'docs/just-bash::yq-host-prototype-isolation',
    rustTest: 'js-only-documented',
    notes:
      'JBC-yq documents these rows as JavaScript host assertions: they probe whether the Node Object.prototype was mutated after yq parsed dangerous YAML/JSON keys. Rust serde_json has no shared global prototype object, so the host-pollution observation is not expressible; the portable parsing behavior is covered by structured_data_yq_prototype_pollution_defense_rows.',
  },
  {
    file: 'packages/just-bash/src/commands/yq/yq.test.ts',
    lines: [89, 194, 201, 210, 232, 251, 269],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_yq_json_stdin_and_jq_filter_rows',
    notes:
      'JBC-yq verifies yq JSON output with -o json, reading from stdin (implicit and via -), -n null-input object construction, and the jq-style map/keys/length filters over in-memory YAML.',
  },
];

const jbcXanGroupbyTransformCaseGroups = [
  {
    file: 'packages/just-bash/src/commands/xan/xan.groupby.test.ts',
    lines: [13, 22, 29, 38, 47, 58, 77, 91],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_groupby_rows',
    notes:
      'JBC verifies portable xan groupby sum/count()/nested-add/mean/multi-max aggregations, multi-column grouping with first-seen order, the --sorted no-op flag, and header-only empty data over in-memory CSV.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.data.test.ts',
    lines: [119, 133],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_shuffle_rows',
    notes:
      'JBC verifies portable xan shuffle reproducibility with --seed (Fisher-Yates over the shared seeded LCG) and that distinct seeds produce distinct orderings over in-memory CSV.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.data.test.ts',
    lines: [205, 214, 225, 280, 332],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_partition_rows',
    notes:
      'JBC verifies portable xan partition by column value, the missing-column error, and the collision-safe filename allocator (FNV-1a hash suffix plus counter disambiguation) that prevents distinct values sanitizing to the same name from silently overwriting one another, with deterministic filenames across runs.',
  },
  {
    file: 'packages/just-bash/src/commands/xan/xan.transform.test.ts',
    lines: [12, 19, 28, 35, 42, 51, 58, 65, 73],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_xan_transform_rows',
    notes:
      'JBC verifies portable xan transform in-place column rewriting with add/mul/upper expressions, the `_` current-column variable, single and comma-separated multi-column targets, -r rename (single and multi), and the missing-column/missing-argument/missing-expression usage errors over in-memory CSV.',
  },
];

const jbcYqFixturesCaseGroups = [
  {
    file: 'packages/just-bash/src/commands/yq/yq.fixtures.test.ts',
    lines: [103, 139, 149, 158, 167, 222, 231, 241, 252, 259, 273, 282, 291, 300, 310],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_yq_fixtures_yaml_json_ini_csv_rows',
    notes:
      'JBC-yq verifies portable yq fixture queries over YAML and JSON input (postfix `[]` projection, select/add aggregation), plus the Rust INI and CSV input parsers (section nesting, true/false coercion, papaparse-style dynamic typing) exercised by the fixtures suite.',
  },
  {
    file: 'packages/just-bash/src/commands/yq/yq.test.ts',
    lines: [
      354, 365, 377, 395, 407, 424, 438, 449, 460, 506, 686, 712, 729,
      743, 756, 770, 781, 808, 817, 839, 856, 873, 898, 909, 920,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest:
      'structured_data_yq_format_conversion_frontmatter_and_autodetect_rows',
    notes:
      'JBC-yq verifies portable yq format validation, INI/CSV/TOML/TSV input and INI/CSV/TOML output conversion, --no-csv-header and --csv-delimiter handling, in-place error reporting, YAML/TOML front-matter extraction, and extension-based format auto-detection.',
  },
  {
    file: 'packages/just-bash/src/commands/yq/yq.fixtures.test.ts',
    lines: [178, 189, 199, 211, 497],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest:
      'structured_data_yq_format_conversion_frontmatter_and_autodetect_rows',
    notes:
      'JBC-yq verifies the portable yq XML input parser over fixture documents: element/array projection, attribute extraction with the +@ prefix, attribute-based select filtering, and XML-to-JSON scalar conversion.',
  },
];

const jbc44CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/query-engine/safe-object.test.ts',
    lines: [
      74, 80, 91, 101, 109, 142, 148, 192, 211, 241, 255, 260, 265, 279,
      287, 291, 295, 299, 305, 331,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::structured-data',
    rustTest: 'structured_data_jbc44_query_engine_safe_object_rows',
    notes:
      'JBC-44 verifies portable query-engine safe-object get/set/delete/assign/copy/has-own semantics over Rust serde_json maps, including dangerous-key filtering, missing keys, array rejection, same-target assignment, and chained updates.',
  },
  {
    file: 'packages/just-bash/src/commands/query-engine/safe-object.test.ts',
    lines: [96, 134, 155, 218, 224, 232, 273],
    status: 'js-only-documented',
    owner: 'docs/just-bash::query-engine-js-object-sanitizer',
    rustTest: 'js-only-documented',
    notes:
      'JBC-44 documents these rows as JavaScript object prototype or reference-identity guard behavior; Rust serde_json maps have no JS prototype chain or nested object identity to validate.',
  },
];

const jbc38CaseGroups = [
  {
    files: [
      'packages/just-bash/src/commands/basename/basename.test.ts',
      'packages/just-bash/src/commands/dirname/dirname.test.ts',
      'packages/just-bash/src/commands/which/which.test.ts',
      'packages/just-bash/src/commands/chmod/chmod.test.ts',
      'packages/just-bash/src/commands/readlink/readlink.test.ts',
      'packages/just-bash/src/commands/ln/ln.test.ts',
      'packages/just-bash/src/commands/stat/stat.test.ts',
      'packages/just-bash/src/commands/test/test.test.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::small-posix-path',
    rustTest: 'jbc38_small_posix_path_commands_match_upstream_rows',
    notes:
      'JBC-38 verifies portable which, basename, dirname, ln/readlink, chmod/stat, and test/[ rows against the virtual filesystem and registry-bound command lookup.',
  },
  {
    file: 'packages/just-bash/src/commands/base64/base64.test.ts',
    lines: [6, 14, 22, 32, 43, 53, 67, 75, 83, 93, 100, 111, 121, 128, 137, 147],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::small-posix-stream',
    rustTest: 'jbc38_small_posix_stream_date_and_inspection_commands_match_upstream_rows',
    notes:
      'JBC-38 verifies portable base64 stdin/file encode/decode, wrapping, dash stdin, help, and option/error behavior; binary byte fixture rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/file/file.test.ts',
    lines: [100, 108, 116, 128, 138, 152, 160, 168, 176, 186, 196, 204, 212, 222, 235, 244, 253],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::small-posix-inspection',
    rustTest: 'jbc38_small_posix_stream_date_and_inspection_commands_match_upstream_rows',
    notes:
      'JBC-38 verifies portable file text, extension, directory, brief/MIME, multiple-file, missing-file, usage, and help rows; binary magic-byte rows remain pending.',
  },
  {
    files: [
      'packages/just-bash/src/commands/base64/base64.utf8-stdin.test.ts',
      'packages/just-bash/src/commands/gzip/gzip.test.ts',
      'packages/just-bash/src/commands/rev/rev.test.ts',
      'packages/just-bash/src/commands/rev/rev.utf8-stdin.test.ts',
      'packages/just-bash/src/commands/seq/seq.test.ts',
      'packages/just-bash/src/commands/date/date.test.ts',
      'packages/just-bash/src/commands/diff/diff.test.ts',
      'packages/just-bash/src/commands/diff/diff.utf8-stdin.test.ts',
      'packages/just-bash/src/commands/strings/strings.test.ts',
      'packages/just-bash/src/commands/strings/strings.utf8-stdin.test.ts',
      'packages/just-bash/src/commands/tee/tee.test.ts',
      'packages/just-bash/src/commands/tee/tee.utf8-stdin.test.ts',
      'packages/just-bash/src/commands/du/du.test.ts',
      'packages/just-bash/src/commands/tree/tree.test.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::small-posix-stream',
    rustTest: 'jbc38_small_posix_stream_date_and_inspection_commands_match_upstream_rows',
    notes:
      'JBC-38 verifies portable gzip/gunzip/zcat, rev, seq, date, diff, strings, tee, du, and tree rows over virtual files/stdin with deterministic stdout, stderr, and exit codes; gzip uses the in-process virtual codec rather than host binary fallback.',
  },
  {
    files: [
      'packages/just-bash/src/commands/comm/comm.test.ts',
      'packages/just-bash/src/commands/comm/comm.utf8-stdin.test.ts',
      'packages/just-bash/src/commands/column/column.test.ts',
      'packages/just-bash/src/commands/join/join.test.ts',
      'packages/just-bash/src/commands/paste/paste.test.ts',
      'packages/just-bash/src/commands/paste/paste.utf8-stdin.test.ts',
      'packages/just-bash/src/commands/expand/expand.test.ts',
      'packages/just-bash/src/commands/expand/expand.utf8-stdin.test.ts',
      'packages/just-bash/src/commands/expand/unexpand.test.ts',
      'packages/just-bash/src/commands/expand/unexpand.utf8-stdin.test.ts',
      'packages/just-bash/src/commands/fold/fold.test.ts',
      'packages/just-bash/src/commands/fold/fold.utf8-stdin.test.ts',
      'packages/just-bash/src/commands/nl/nl.test.ts',
      'packages/just-bash/src/commands/nl/nl.utf8-stdin.test.ts',
      'packages/just-bash/src/commands/split/split.test.ts',
      'packages/just-bash/src/commands/xargs/xargs.test.ts',
      'packages/just-bash/src/commands/xargs/xargs.utf8-stdin.test.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::small-posix-table',
    rustTest: 'jbc38_small_posix_table_and_xargs_commands_match_upstream_rows',
    notes:
      'JBC-38 verifies portable comm, column, join, paste, expand/unexpand, fold, nl, split, and xargs rows over virtual files/stdin; quoted host-script command-name rows remain pending.',
  },
];

const jbc41CaseGroups = [
  {
    file: 'packages/just-bash/src/interpreter/helpers/xtrace.test.ts',
    lines: [
      6, 17, 28, 45, 55, 66, 78, 88, 102, 115, 126, 138, 150, 164, 178,
      194, 213, 224, 239, 268, 283,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::parser-interpreter',
    rustTest: 'jbc41_interpreter_xtrace_set_x_ps4_and_execution_rows',
    notes:
      'JBC-41 verifies portable set -x/+x tracing, PS4 literal and variable prefixes, argument quoting, assignments, loops, branches, subshell restoration, pipelines, and function call/body traces. Command-substitution stderr tracing remains pending.',
  },
];

const jbc43CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/grep/grep.basic.test.ts',
    lines: [574, 583, 591, 599, 610, 618, 626, 654, 670, 678, 696, 714],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::grep',
    rustTest: 'text_search_jbc43_grep_bre_include_and_real_world_rows',
    notes:
      'JBC-43 verifies portable plain-grep BRE intervals, BRE grouping anchors, literal mid-pattern caret/star rules, POSIX word-boundary classes, and POSIX class intervals over virtual files.',
  },
  {
    file: 'packages/just-bash/src/commands/grep/grep.advanced.test.ts',
    lines: [306, 319, 333, 390, 399, 408, 420, 431, 444, 455, 466, 479, 494],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::grep',
    rustTest: 'text_search_jbc43_grep_bre_include_and_real_world_rows',
    notes:
      'JBC-43 verifies portable grep --include recursive filtering, BRE alternation, and real-world virtual file search rows without host grep fallback.',
  },
];

const jbc45CaseGroups = [
  {
    file: 'packages/just-bash/src/Bash.general.test.ts',
    lines: [492],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::jbc45-core-leftovers',
    rustTest: 'jbc45_core_exec_escaped_quotes_and_concurrency_rows',
    notes:
      'JBC-45 verifies the escaped double-quote core exec row through the Rust shell tokenizer without host shell fallback.',
  },
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [481, 584, 661],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::jbc45-core-leftovers',
    rustTest: 'jbc45_core_exec_escaped_quotes_and_concurrency_rows',
    notes:
      'JBC-45 verifies high-concurrency env isolation, concurrent function-scope non-leakage, and race-condition state isolation through cloned Rust virtual sessions. Logger-option rows remain pending because Rust has no logger option surface.',
  },
  {
    file: 'packages/just-bash/src/fs/real-fs-utils.test.ts',
    lines: [331, 339, 351, 359, 367, 380, 398],
    status: 'portable-verified',
    owner: 'crates/just-bash::path::jbc45-symlink-target-sanitizer',
    rustTest: 'jbc45_real_fs_symlink_target_sanitizer_cases',
    notes:
      'JBC-45 verifies portable symlink-target presentation for relative targets, absolute in-root targets, basename-only outside-root targets, non-existent in-root targets, and root-prefix boundary attacks without host realpath access.',
  },
  {
    file: 'packages/just-bash/src/fs/in-memory-fs/in-memory-fs.security.test.ts',
    lines: [322, 337, 432],
    status: 'portable-verified',
    owner: 'crates/just-bash::fs::jbc45-in-memory-leftovers',
    rustTest: 'jbc45_in_memory_symlink_concurrency_and_device_path_rows',
    notes:
      'JBC-45 verifies concurrent virtual symlink reads, concurrent circular-symlink fail-closed behavior, and that virtual test -c does not mistake /fake/dev/null for a character device.',
  },
];

const jbc46SourceGroups = [
  {
    files: [
      'packages/just-bash/src/commands/md5sum/checksum.ts',
      'packages/just-bash/src/commands/md5sum/md5sum.ts',
      'packages/just-bash/src/commands/md5sum/sha1sum.ts',
      'packages/just-bash/src/commands/md5sum/sha256sum.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::checksums',
    notes:
      'JBC-46 verifies md5sum, sha1sum, and sha256sum hashing, file/stdin text input, file-backed binary bytes, check mode, Unicode files, help, and missing-file diagnostics over the Rust virtual filesystem. Binary stdin corruption rows remain pending.',
  },
  {
    files: ['packages/just-bash/src/commands/tar/tar.ts'],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::virtual-archive',
    notes:
      'JBC-46 verifies deterministic Just Bash virtual tar create/list/extract, gzip/bzip2 virtual wrappers, -C, strip, stdout extract, xz/zstd fail-closed gates, and high-byte virtual file round trips. Host-compatible tar byte parsing remains pending.',
  },
  {
    files: [
      'packages/just-bash/src/commands/help/help.ts',
      'packages/just-bash/src/commands/od/od.ts',
      'packages/just-bash/src/commands/sleep/sleep.ts',
      'packages/just-bash/src/commands/tac/tac.ts',
      'packages/just-bash/src/commands/touch/touch.ts',
      'packages/just-bash/src/commands/true/true.ts',
      'packages/just-bash/src/commands/pwd/pwd.ts',
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::small-command-leftovers',
    notes:
      'JBC-46 verifies the remaining portable help, od, tac, touch, true/false, pwd, and sleep parse/error/help rows through in-process Rust command execution. Sleep callback-duration injection rows remain pending.',
  },
];

const jbc46CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/md5sum/md5sum.test.ts',
    lines: [6, 14, 22, 33, 49, 60, 74, 83, 94, 102, 112, 128, 142, 158, 168, 179],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::checksums',
    rustTest: 'jbc46_checksum_commands_hash_check_and_binary_rows_are_virtual',
    notes:
      'JBC-46 verifies md5sum, sha1sum, and sha256sum text/file hashing, check mode, help, missing-file handling, Unicode files, and high-byte/null virtual file bytes without host digest binaries.',
  },
  {
    file: 'packages/just-bash/src/commands/md5sum/checksum.binary.test.ts',
    lines: [6, 20, 63, 88, 105, 132, 175],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::checksums',
    rustTest: 'jbc46_checksum_commands_hash_check_and_binary_rows_are_virtual',
    notes:
      'JBC-46 verifies file-backed binary checksum rows for md5sum, sha1sum, and sha256sum plus Unicode and binary check mode. Binary stdin and mutation/same-content rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/tar/tar.test.ts',
    lines: [
      6, 16, 25, 32, 39, 91, 143, 161, 173, 186, 224, 240, 269, 307, 322,
      391, 406, 422, 510, 557, 601, 989,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::virtual-archive',
    rustTest: 'jbc46_virtual_tar_archive_round_trips_list_extract_and_codecs',
    notes:
      'JBC-46 verifies these tar rows against a deterministic Rust virtual archive format created and consumed in-process, including help/errors, directory create/list/extract, exclude, strip, gzip virtual wrapper, stdout extract, high-byte file round trip, and xz fail-closed behavior. Host-compatible archive parsing and untested append/update/pattern rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/tar/tar.binary.test.ts',
    lines: [6],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::virtual-archive',
    rustTest: 'jbc46_virtual_tar_archive_round_trips_list_extract_and_codecs',
    notes:
      'JBC-46 verifies high-byte virtual file content survives a Rust-created tar archive round trip. Binary stdin archive rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/help/help.test.ts',
    lines: [6, 13, 23, 31, 38, 47, 55, 65],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::small-command-leftovers',
    rustTest: 'jbc46_small_command_leftovers_od_tac_help_touch_pwd_true_sleep_rows',
    notes:
      'JBC-46 verifies help listing, topic lookup, unknown topic errors, short synopsis output, and -- terminator handling in the Rust built-in registry.',
  },
  {
    file: 'packages/just-bash/src/commands/od/od.test.ts',
    lines: [5, 13, 29, 37, 46],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::small-command-leftovers',
    rustTest: 'jbc46_small_command_leftovers_od_tac_help_touch_pwd_true_sleep_rows',
    notes:
      'JBC-46 verifies portable od stdin/file octal output, -c character mode, -An address suppression, and missing-file diagnostics. Additional binary dump rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/tac/tac.test.ts',
    lines: [5, 12, 19, 26, 34],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::small-command-leftovers',
    rustTest: 'jbc46_small_command_leftovers_od_tac_help_touch_pwd_true_sleep_rows',
    notes:
      'JBC-46 verifies tac stdin reversal, single-line and empty inputs, file input, and missing-file diagnostics. Relative path rows remain pending.',
  },
  {
    file: 'packages/just-bash/src/commands/touch/touch.test.ts',
    lines: [5, 13, 21, 30, 39, 46, 56, 63],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::small-command-leftovers',
    rustTest: 'jbc46_small_command_leftovers_od_tac_help_touch_pwd_true_sleep_rows',
    notes:
      'JBC-46 verifies touch creates one or many virtual files, preserves existing content, creates nested/relative/space/hidden paths, and errors on missing operands.',
  },
  {
    file: 'packages/just-bash/src/commands/pwd/pwd.test.ts',
    lines: [5, 12, 19, 25, 34, 46, 55],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::small-command-leftovers',
    rustTest: 'jbc46_small_command_leftovers_od_tac_help_touch_pwd_true_sleep_rows',
    notes:
      'JBC-46 verifies pwd default/root/current directory output, same-exec cd changes, parent traversal, repeated cd changes, and ignored arguments.',
  },
  {
    file: 'packages/just-bash/src/commands/true/true.test.ts',
    lines: [5, 13, 19, 27, 35, 41],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::small-command-leftovers',
    rustTest: 'jbc46_small_command_leftovers_od_tac_help_touch_pwd_true_sleep_rows',
    notes:
      'JBC-46 verifies true and false exit status, ignored arguments, and conditional execution without host shell fallback.',
  },
  {
    file: 'packages/just-bash/src/commands/sleep/sleep.test.ts',
    lines: [115, 130, 138, 146, 156, 167],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::small-command-leftovers',
    rustTest: 'jbc46_small_command_leftovers_od_tac_help_touch_pwd_true_sleep_rows',
    notes:
      'JBC-46 verifies sleep mixed-suffix parsing, missing/invalid operand errors, help, and short-duration in-process completion. Upstream callback duration-capture rows remain pending.',
  },
];

const jbc47CaseGroups = [
  {
    file: 'packages/just-bash/src/commands/sed/sed.errors.test.ts',
    lines: [37, 44, 59, 66, 75, 82, 89, 106, 113, 122, 129, 170, 177, 186, 193],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::sed',
    rustTest:
      'sed_jbc47_argument_errors_transliteration_blocks_and_script_files',
    notes:
      'JBC-47 verifies portable sed error/diagnostic rows: missing -f script file (No such file or directory), lenient unterminated substitution / unknown command / unknown flag / line-0 address, missing context address (,3p), unterminated address regex (/foo d -> command expected), undefined branch labels (b/t), unknown short/long options, mismatched and unterminated y transliteration sets, and addressed single-command { } blocks. Multi-command blocks, a/i/c text, and step-address negative/zero rows remain pending the cycle engine.',
  },
  {
    file: 'packages/just-bash/src/commands/sed/sed.advanced.test.ts',
    lines: [35, 45, 122, 133, 144, 157],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::sed',
    rustTest:
      'sed_jbc47_argument_errors_transliteration_blocks_and_script_files',
    notes:
      'JBC-47 verifies portable sed y transliteration (lowercase->uppercase and character rotation) and -f script-file handling: reading a multi-command script file, ignoring # comments, combining -f with -e, and the missing-script-file diagnostic (couldn\'t open file). N/=/branch-with-labels advanced rows remain pending the cycle engine.',
  },
];

const jbSedTestTsCaseGroups = [
  {
    file: 'packages/just-bash/src/commands/sed/sed.test.ts',
    lines: [
      267, 281, 294, 307, 320, 339, 354, 364, 378, 388, 398, 410, 426, 435,
      444, 455, 464, 473, 484, 493, 609, 618, 627, 638, 647, 658, 670, 682,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::sed',
    rustTest: 'sed_test_ts_inplace_holdspace_text_and_cycle_rows',
    notes:
      'command-sed verifies the sed.test.ts cycle-engine rows over the virtual session: -i/--in-place editing (single/global/delete/match-delete/multi-file), the h/H/g/G/x hold space, the a/i/c text commands, relative-offset (+N) addresses with and without { } blocks, grouped { } commands, and the P/D first-line commands. Each assertion mirrors the upstream expectation verbatim.',
  },
  {
    file: 'packages/just-bash/src/commands/sed/sed.commands.test.ts',
    lines: [216, 249],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::sed',
    rustTest: 'sed_test_ts_inplace_holdspace_text_and_cycle_rows',
    notes:
      'command-sed verifies multiple -e scripts run in sequence and relative-offset (/alpha/,+2) address matching through the sed cycle engine.',
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
  const group = [
    ...jb06SourceGroups,
    ...jbc12SourceGroups,
    ...jbc13SourceGroups,
    ...jbc39SourceGroups,
    ...jbc31SourceGroups,
    ...jbc17SourceGroups,
    ...jbc18SourceGroups,
    ...jbc19SourceGroups,
    ...jbc26SourceGroups,
    ...jbc33SourceGroups,
    ...jbc46SourceGroups,
  ].find((entry) => groupMatchesFile(entry, relativePath));
  return group ? rowOverrideFromGroup(group) : undefined;
}

const jbAliasCaseGroups = [
  {
    file: 'packages/just-bash/src/commands/alias/alias.test.ts',
    lines: [6, 13, 20, 27, 34, 40, 47],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::alias',
    rustTest:
      'just_bash_alias_lists_no_aliases_initially; just_bash_alias_sets_and_lists_within_same_exec; just_bash_alias_shows_specific_alias_within_same_exec; just_bash_alias_errors_when_alias_not_found; just_bash_alias_sets_multiple_within_same_exec_in_definition_order; just_bash_alias_shows_help_with_help_flag; just_bash_alias_does_not_persist_across_exec_calls',
    notes:
      'JB alias builtin: portable definition-ordered alias listing, single-alias display, not-found errors, multi-definition order, --help, and fresh-shell non-persistence across exec calls over the virtual session.',
  },
  {
    file: 'packages/just-bash/src/commands/alias/alias.test.ts',
    lines: [62, 69, 76, 85],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::unalias',
    rustTest:
      'just_bash_unalias_removes_an_alias_within_same_exec; just_bash_unalias_errors_when_unaliasing_nonexistent_alias; just_bash_unalias_removes_all_aliases_with_a_flag; just_bash_unalias_shows_help_with_help_flag',
    notes:
      'JB unalias builtin: portable single-alias removal, not-found error, -a remove-all, and --help over the virtual session.',
  },
];

const jbInterpreterBuiltinsCaseGroups = [
  {
    file: 'packages/just-bash/src/interpreter/builtins/cd.test.ts',
    lines: [6, 16, 25, 35, 48, 60, 71, 82, 91, 98],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::builtins::cd',
    rustTest:
      'builtins_cd_changes_to_specified_directory; builtins_cd_changes_to_home_without_argument; builtins_cd_updates_pwd_environment_variable; builtins_cd_updates_oldpwd_environment_variable; builtins_cd_handles_cd_dash; builtins_cd_handles_dotdot; builtins_cd_handles_absolute_path; builtins_cd_does_not_change_real_process_cwd; builtins_cd_errors_on_nonexistent_directory; builtins_cd_errors_when_cd_to_a_file',
    notes:
      'JB cd builtin: portable virtual directory change, $HOME default, PWD/OLDPWD updates, cd -, .., absolute path, virtual isolation of the real cwd, and No-such-file / Not-a-directory errors over the virtual session.',
  },
  {
    file: 'packages/just-bash/src/interpreter/builtins/exit.test.ts',
    lines: [6, 12, 18, 24, 37, 43, 49, 57, 121],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::builtins::exit',
    rustTest:
      'builtins_exit_with_code_0_by_default; builtins_exit_with_specified_code; builtins_exit_with_code_1; builtins_exit_stops_execution_after_exit; builtins_exit_wraps_code_256_to_0; builtins_exit_wraps_code_257_to_1; builtins_exit_handles_negative_codes; builtins_exit_from_function; builtins_exit_errors_on_non_numeric_argument',
    notes:
      'JB exit builtin: portable default 0, explicit code, stop-after-exit, modulo-256 wrapping (256->0, 257->1, -1->255), exit from a function, and numeric-argument-required error with code 2.',
  },
  {
    file: 'packages/just-bash/src/interpreter/builtins/export.test.ts',
    lines: [108],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::builtins::export',
    rustTest: 'builtins_export_works_with_conditional',
    notes:
      'JB export builtin: portable export of a value then a [ ... ] && conditional use over the virtual session.',
  },
  {
    file: 'packages/just-bash/src/interpreter/builtins/export.test.ts',
    lines: [40],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::builtins::export',
    rustTest:
      'just_bash_bash_general_export_does_not_persist_functions_reset_and_filesystem_persists',
    notes:
      'JB export builtin: a variable exported in one exec call is not visible to a later exec on the same session, matching the upstream fresh-shell-per-exec contract.',
  },
  {
    file: 'packages/just-bash/src/interpreter/builtins/export.test.ts',
    lines: [116],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::builtins::export',
    rustTest: 'builtins_export_initial_env_vars_available_in_every_exec',
    notes:
      'JB export builtin: env vars supplied via the Bash constructor options remain available in every subsequent exec call on the same session.',
  },
  {
    file: 'packages/just-bash/src/interpreter/builtins/set.test.ts',
    lines: [228, 240, 251, 263, 276, 288, 303, 314],
    status: 'portable-verified',
    owner: 'crates/just-bash::exec::builtins::set',
    rustTest:
      'builtins_set_e_exits_immediately_when_command_fails; builtins_set_e_continues_execution_without_set_e; builtins_set_e_does_not_exit_if_command_succeeds; builtins_set_e_disabled_with_set_plus_e; builtins_set_e_enabled_with_set_o_errexit; builtins_set_e_disabled_with_set_plus_o_errexit; builtins_set_e_does_not_exit_on_failed_command_in_and_short_circuit; builtins_set_e_does_not_exit_on_failed_command_in_or_short_circuit',
    notes:
      'JB set builtin errexit (set -e / set -o errexit and their +e / +o disablement): a failing simple command aborts the script with exit code 1, a succeeding command does not, +e/+o re-disables errexit, and AND-OR list members that fail on the left of && or || do not trigger errexit.',
  },
];

const jbR5ParserInterpreterCaseGroups = [
  {
    file: 'packages/just-bash/src/interpreter/builtins/break.test.ts',
    lines: [6, 19, 34, 51, 66, 78, 93, 101, 112, 123, 134, 151, 165, 178],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::builtins::break',
    rustTest: 'r5_interpreter_builtin_break_matches_upstream',
    notes:
      'R5 verifies the portable `break` builtin 1:1 with break.test.ts: exiting for/while/until loops early, `break n` multi-level and single-level breaks, level exceeding loop depth, silent no-op outside a loop, fatal numeric-argument-required errors (non-numeric/zero/negative) with exit 128, fatal too-many-arguments error with exit 1, and break inside case/if/function nested in a loop.',
  },
  {
    file: 'packages/just-bash/src/interpreter/builtins/continue.test.ts',
    lines: [6, 19, 34, 51, 67, 78, 93, 101, 112, 123, 134, 151, 165, 178],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::builtins::continue',
    rustTest: 'r5_interpreter_builtin_continue_matches_upstream',
    notes:
      'R5 verifies the portable `continue` builtin 1:1 with continue.test.ts: skipping to the next iteration of for/while/until loops, `continue n` multi-level and single-level continues, level exceeding loop depth, silent no-op outside a loop, fatal numeric-argument-required errors (non-numeric/zero/negative) with exit 1, fatal too-many-arguments error with exit 1, and continue inside case/if/function nested in a loop. The two C-style `for (( ))` rows (L197, L208) stay pending until C-style for loops are implemented.',
  },
  {
    file: 'packages/just-bash/src/interpreter/builtins/eval.test.ts',
    lines: [6, 13, 19, 26, 35, 44, 54, 66, 81, 89, 95, 101, 112, 122, 131, 142, 148],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::builtins::eval',
    rustTest: 'jbpi_interpreter_builtin_eval_matches_upstream',
    notes:
      'R10JB verifies the portable `eval` builtin 1:1 with eval.test.ts through the Rust parser/interpreter: simple/multi-word commands, empty/no-argument no-ops, variable expansion before execution (including dynamic names and dynamic assignment), command construction over expanded word lists, command substitution, exit-code propagation of the executed/last command, the parse-error row (exit 1 with "Parse error"), current-environment execution, function visibility and persistent definition, and single/double quote handling. The piped row (L75) stays pending because `tr` is not provided by the parser/interpreter command seam.',
  },
  {
    file: 'packages/just-bash/src/interpreter/builtins/return.test.ts',
    lines: [6, 21, 33, 46, 60, 72, 84, 98, 107],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::builtins::return',
    rustTest: 'jbpi_interpreter_builtin_return_matches_upstream',
    notes:
      'R10JB verifies the portable `return` builtin 1:1 with return.test.ts through the Rust parser/interpreter: default/explicit/last-command/zero exit codes, modulo-256 wrapping (256->0, 257->1, -1->255), the not-in-a-function error (exit 1) and non-numeric-argument error (exit 2), innermost-only return in nested functions, propagation through control flow inside the function body, and stdout preservation before return.',
  },
  {
    file: 'packages/just-bash/src/interpreter/builtins/exit.test.ts',
    lines: [72, 85, 101, 110],
    status: 'portable-verified',
    owner: 'crates/just-bash::shell::builtins::exit',
    rustTest: 'jbpi_interpreter_builtin_exit_context_and_last_status_rows',
    notes:
      'R10JB verifies the previously-pending exit.test.ts rows through the Rust parser/interpreter: exit from inside a for loop and from inside an if block both stop the script with the requested code, and no-argument `exit` resolves to the last command status (1 after `false`, 0 after `true`).',
  },
];

const jbExecOptionsLoggingCaseGroups = [
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [721],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::logging',
    rustTest: 'exec_options_logging_does_not_log_without_logger',
    notes:
      'JB exec logging: a session without a logger executes normally and emits no log records.',
  },
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [728],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::logging',
    rustTest: 'exec_options_logging_logs_exec_command_at_info_level',
    notes:
      'JB exec logging: the exec command line is logged at info level with the command in the data payload.',
  },
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [740],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::logging',
    rustTest: 'exec_options_logging_logs_stdout_at_debug_level',
    notes:
      'JB exec logging: captured stdout is logged at debug level with the output in the data payload.',
  },
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [752],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::logging',
    rustTest: 'exec_options_logging_logs_stderr_at_info_level',
    notes:
      'JB exec logging: captured stderr is logged at info level with the output in the data payload.',
  },
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [764],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::logging',
    rustTest: 'exec_options_logging_logs_exit_code_at_info_level',
    notes:
      'JB exec logging: the exit code is logged at info level with the exit code in the data payload.',
  },
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [776],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::logging',
    rustTest: 'exec_options_logging_logs_non_zero_exit_code',
    notes:
      'JB exec logging: a non-zero exit code is propagated into the exit log data payload.',
  },
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [787],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::logging',
    rustTest: 'exec_options_logging_logs_in_correct_order_exec_then_exit',
    notes:
      'JB exec logging: records are emitted in order with exec first and exit last.',
  },
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [797],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::logging',
    rustTest: 'exec_options_logging_does_not_log_stdout_when_empty',
    notes:
      'JB exec logging: empty stdout produces no stdout record.',
  },
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [807],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::logging',
    rustTest: 'exec_options_logging_does_not_log_stderr_when_empty',
    notes:
      'JB exec logging: empty stderr produces no stderr record.',
  },
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [817],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::logging',
    rustTest: 'exec_options_logging_does_not_log_empty_commands',
    notes:
      'JB exec logging: empty or whitespace-only commands are a no-op and emit no records.',
  },
  {
    file: 'packages/just-bash/src/Bash.exec-options.test.ts',
    lines: [827],
    status: 'portable-verified',
    owner: 'crates/just-bash::runtime::logging',
    rustTest: 'exec_options_logging_logs_parse_errors',
    notes:
      'JB exec logging: a parse error (unterminated ${) still logs exec, a syntax-error stderr, and exit code 2.',
  },
];

const justBashCoreSerializeCaseGroups = [
  {
    file: 'packages/just-bash/src/transform/serialize.test.ts',
    lines: [
      53, 54, 103, 105, 106, 109, 110, 111, 112, 113, 114, 115, 116, 117,
      119, 121, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134,
      176, 177,
    ],
    status: 'portable-verified',
    owner: 'crates/just-bash::transform::serialize',
    rustTest: 'just_bash_core_serialize_round_trips_param_op_and_compound_rows',
    notes:
      'just-bash-core verifies Rust AST parse/serialize/parse equivalence for the timed-pipeline, parameter-operation (error-if-unset, substring, prefix/suffix removal, pattern replacement, case modification, indirection, prefix listing, transform @Q), and subshell/group-with-redirection serialize rows.',
  },
];

function caseOverrideFor(testCase) {
  const group = [
    ...jbSedTestTsCaseGroups,
    ...jbAliasCaseGroups,
    ...jbInterpreterBuiltinsCaseGroups,
    ...jbR5ParserInterpreterCaseGroups,
    ...jbExecOptionsLoggingCaseGroups,
    ...jbc39CaseGroups,
    ...jbc28CaseGroups,
    ...jb06CaseGroups,
    ...jbc07CaseGroups,
    ...jbc09CaseGroups,
    ...jbc10CaseGroups,
    ...jbc12CaseGroups,
    ...jbc13CaseGroups,
    ...jbc26CaseGroups,
    ...jbc15CaseGroups,
    ...r10jbInterpreterCoreCaseGroups,
    ...jbc16CaseGroups,
    ...jbc17CaseGroups,
    ...jbc18CaseGroups,
    ...jbc31CaseGroups,
    ...jbc19CaseGroups,
    ...jbc20CaseGroups,
    ...jbc22CaseGroups,
    ...jbc23CaseGroups,
    ...jbc34CaseGroups,
    ...jbc36CaseGroups,
    ...jbc24CaseGroups,
    ...jbc25CaseGroups,
    ...jbc35CaseGroups,
    ...jbc42CaseGroups,
    ...jbcAwkEdgeCaseGroups,
    ...jbc38CaseGroups,
    ...jbc41CaseGroups,
    ...jbc43CaseGroups,
    ...jbc45CaseGroups,
    ...jbc46CaseGroups,
    ...jbc47CaseGroups,
    ...jbc27CaseGroups,
    ...jbc30AgentExampleCaseGroups,
    ...jbc33CaseGroups,
    ...jbr1CaseGroups,
    ...jbR3SyntaxCaseGroups,
    ...jbR2CaseGroups,
    ...jbpiParserInterpreterCaseGroups,
    ...jbc37CaseGroups,
    ...jbc44CaseGroups,
    ...jbcXanGroupbyTransformCaseGroups,
    ...jbcYqFixturesCaseGroups,
    ...justBashCoreSerializeCaseGroups,
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
  for (const proofName of discoverNapiJsProofs()) {
    names.add(proofName);
  }
  for (const proofName of readRustRunnerProofs().proofNames) {
    names.add(proofName);
  }
  return names;
}

function discoverNapiJsProofs() {
  const roots = [path.join(repositoryRoot, 'crates/just-bash-napi/test')].filter((entry) =>
    fs.existsSync(entry)
  );
  const names = new Set();
  for (const root of roots) {
    for (const file of walk(root).filter((entry) => /\.(?:cjs|mjs|js)$/.test(entry))) {
      const source = fs.readFileSync(file, 'utf8');
      for (const match of source.matchAll(
        /\b(?:async\s+)?function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/g
      )) {
        names.add(match[1]);
      }
    }
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
    '- `scripts/master-parity-gate.sh --check` runs this ledger in non-strict mode now; set `JUST_BASH_STRICT_GATE=1` only after the JBC-32/JBC-40 closeout wave closes every portable row.',
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
