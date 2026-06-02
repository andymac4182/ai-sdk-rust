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
      'JBC-28 verifies python3/python remain unavailable by default in the Rust and Open Agents Just Bash backends and fail closed without invoking host Python. The python-enabled row remains pending.',
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
      'JBC-25 verifies portable AWK array element creation, numeric and expression indices, missing elements, overwrite, concatenated keys, counting, grouped sums, SUBSEP-style two-dimensional keys, and compound assignment; in/delete/for-in iteration and split-array rows remain pending.',
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
    lines: jbc33RegexJsOnlyLines,
    status: 'js-only-documented',
    owner: 'crates/just-bash::regex::js-wrapper-exception',
    rustTest: 'js-only:user-regex-regexp-wrapper-callback-and-lastindex-api',
    notes:
      'JBC-33 classifies JavaScript RegExp wrapper identity, callback replacement, lastIndex state, native RegExp access, factory instance checks, and TypeScript RegexLike interface rows as JS-only API behavior; portable regex semantics are verified separately.',
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
    ...jbc31SourceGroups,
    ...jbc17SourceGroups,
    ...jbc18SourceGroups,
    ...jbc19SourceGroups,
    ...jbc26SourceGroups,
    ...jbc33SourceGroups,
  ].find((entry) => groupMatchesFile(entry, relativePath));
  return group ? rowOverrideFromGroup(group) : undefined;
}

function caseOverrideFor(testCase) {
  const group = [
    ...jbc28CaseGroups,
    ...jb06CaseGroups,
    ...jbc07CaseGroups,
    ...jbc09CaseGroups,
    ...jbc10CaseGroups,
    ...jbc12CaseGroups,
    ...jbc13CaseGroups,
    ...jbc26CaseGroups,
    ...jbc15CaseGroups,
    ...jbc16CaseGroups,
    ...jbc17CaseGroups,
    ...jbc18CaseGroups,
    ...jbc31CaseGroups,
    ...jbc19CaseGroups,
    ...jbc20CaseGroups,
    ...jbc22CaseGroups,
    ...jbc23CaseGroups,
    ...jbc24CaseGroups,
    ...jbc25CaseGroups,
    ...jbc27CaseGroups,
    ...jbc30AgentExampleCaseGroups,
    ...jbc33CaseGroups,
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
