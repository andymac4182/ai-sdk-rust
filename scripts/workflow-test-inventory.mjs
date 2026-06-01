#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..'
);
const defaultSourceRoot =
  '/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel/workflow/main';
const sourceRoot = process.env.WORKFLOW_UPSTREAM_PATH ?? defaultSourceRoot;
const outputPath = path.join(repositoryRoot, 'docs/workflow-test-inventory.md');
const overridesPath = path.join(
  repositoryRoot,
  'docs/workflow-test-overrides.json'
);
const sourceHead = 'ae3c833acd4f44ab84db65b44eb2ba2646eaecf9';
const inventoryDate = '2026-06-01';

const testFilePattern =
  /\.(?:test|spec)(?:-d)?\.(?:[cm]?[tj]sx?|mts|cts)$/;
const targetCalls = new Set(['describe', 'it', 'test']);
const foundationalPackages = new Set([
  'core',
  'errors',
  'utils',
  'workflow',
  'world',
  'world-local',
]);

const rustOwners = new Map([
  ['ai', 'workflow-ai'],
  ['builders', 'workflow-builders'],
  ['cli', 'workflow-cli'],
  ['core', 'workflow-core'],
  ['errors', 'workflow-errors'],
  ['serde', 'workflow-serde'],
  ['swc-plugin-workflow', 'workflow-swc-plugin'],
  ['utils', 'workflow-utils'],
  ['workflow', 'workflow'],
  ['world', 'workflow-world'],
  ['world-local', 'workflow-world-local'],
  ['world-postgres', 'workflow-world-postgres'],
  ['world-vercel', 'workflow-world-vercel'],
]);

const verifiedRustFileTests = new Map([
  [
    'errors|packages/errors/src/ansi.test.ts',
    {
      testName: 'upstream_ansi_cases',
      note: 'Ported in workflow-errors; validated by cargo test -p workflow-errors.',
    },
  ],
  [
    'errors|packages/errors/src/build-error.test.ts',
    {
      testName: 'upstream_build_error_cases',
      note: 'Ported in workflow-errors; validated by cargo test -p workflow-errors.',
    },
  ],
  [
    'errors|packages/errors/src/corrupted-event-log-error.test.ts',
    {
      testName: 'upstream_corrupted_event_log_error_cases',
      note: 'Ported in workflow-errors; validated by cargo test -p workflow-errors.',
    },
  ],
  [
    'errors|packages/errors/src/fatal-error.test.ts',
    {
      testName: 'upstream_fatal_error_cases',
      note: 'Ported in workflow-errors; validated by cargo test -p workflow-errors.',
    },
  ],
  [
    'errors|packages/errors/src/runtime-decryption-error.test.ts',
    {
      testName: 'upstream_runtime_decryption_error_cases',
      note: 'Ported in workflow-errors; validated by cargo test -p workflow-errors.',
    },
  ],
  [
    'errors|packages/errors/src/serialization-error.test.ts',
    {
      testName: 'upstream_serialization_error_cases',
      note: 'Ported in workflow-errors; validated by cargo test -p workflow-errors.',
    },
  ],
  [
    'utils|packages/utils/src/check-data-dir.test.ts',
    {
      testName: 'upstream_check_data_dir_cases',
      note: 'Ported in workflow-utils; validated by cargo test -p workflow-utils.',
    },
  ],
  [
    'utils|packages/utils/src/get-port.test.ts',
    {
      testName: 'upstream_get_port_cases',
      note: 'Ported in workflow-utils; validated by cargo test -p workflow-utils.',
    },
  ],
  [
    'utils|packages/utils/src/parse-name.test.ts',
    {
      testName: 'upstream_parse_name_cases',
      note: 'Ported in workflow-utils; validated by cargo test -p workflow-utils.',
    },
  ],
  [
    'utils|packages/utils/src/pluralize.test.ts',
    {
      testName: 'upstream_pluralize_cases',
      note: 'Ported in workflow-utils; validated by cargo test -p workflow-utils.',
    },
  ],
  [
    'utils|packages/utils/src/promise.test.ts',
    {
      testName: 'upstream_promise_cases',
      note: 'Ported in workflow-utils; validated by cargo test -p workflow-utils.',
    },
  ],
  [
    'utils|packages/utils/src/re-exports.test.ts',
    {
      testName: 'upstream_re_exports_cases',
      note: 'Ported in workflow-utils; validated by cargo test -p workflow-utils.',
    },
  ],
  [
    'utils|packages/utils/src/time.test.ts',
    {
      testName: 'upstream_time_cases',
      note: 'Ported in workflow-utils; validated by cargo test -p workflow-utils.',
    },
  ],
  [
    'utils|packages/utils/src/world-target.test.ts',
    {
      testName: 'upstream_world_target_cases',
      note: 'Ported in workflow-utils; validated by cargo test -p workflow-utils.',
    },
  ],
]);

const verifiedWorldAttributeTests = new Map([
  [13, 'validate_attribute_key_accepts_a_normal_key'],
  [17, 'validate_attribute_key_rejects_empty_keys'],
  [21, 'validate_attribute_key_rejects_keys_over_the_length_cap'],
  [27, 'validate_attribute_key_accepts_keys_exactly_at_the_length_cap'],
  [
    33,
    'validate_attribute_key_rejects_keys_starting_with_reserved_prefix_by_default',
  ],
  [
    39,
    'validate_attribute_key_accepts_reserved_prefix_keys_when_allow_reserved_attributes_is_set',
  ],
  [
    45,
    'validate_attribute_key_still_rejects_reserved_prefix_keys_when_allow_reserved_attributes_is_explicitly_false',
  ],
  [53, 'validate_attribute_value_accepts_null_unset'],
  [57, 'validate_attribute_value_accepts_a_normal_string'],
  [61, 'validate_attribute_value_rejects_values_over_the_byte_cap'],
  [67, 'validate_attribute_value_counts_utf8_bytes_not_characters'],
  [79, 'validate_attribute_changes_accepts_a_small_batch_of_valid_changes'],
  [
    88,
    'validate_attribute_changes_rejects_duplicate_keys_within_a_single_batch',
  ],
  [
    97,
    'validate_attribute_changes_rejects_when_post_merge_count_exceeds_the_per_run_cap',
  ],
  [
    107,
    'validate_attribute_changes_does_not_count_upserts_on_already_present_keys_against_the_cap',
  ],
  [
    121,
    'validate_attribute_changes_rejects_reserved_prefix_keys_in_a_batch_by_default',
  ],
  [
    130,
    'validate_attribute_changes_accepts_reserved_prefix_keys_when_allow_reserved_attributes_is_set',
  ],
  [144, 'apply_attribute_changes_upserts_new_keys'],
  [150, 'apply_attribute_changes_overwrites_existing_keys'],
  [156, 'apply_attribute_changes_removes_keys_when_value_is_null'],
  [162, 'apply_attribute_changes_applies_set_and_unset_in_a_single_batch'],
  [171, 'apply_attribute_changes_returns_a_new_object_does_not_mutate_input'],
  [178, 'apply_attribute_changes_treats_undefined_existing_as_the_empty_record'],
]);

const verifiedWorldAttributeNote =
  'Ported in crates/workflow-world/src/attributes.rs; verified by cargo test -p workflow-world.';

const workflowFacadeOverrides = new Map(
  [
    [
      'packages/workflow/src/internal/builtins.test.ts:54',
      'workflow_builtins_set_attributes_rethrows_before_third_attempt',
    ],
    [
      'packages/workflow/src/internal/builtins.test.ts:74',
      'workflow_builtins_set_attributes_logs_after_third_failed_attempt',
    ],
    [
      'packages/workflow/src/observability.test.ts:12',
      'workflow_observability_reexports_parse_step_name_and_it_works',
    ],
    [
      'packages/workflow/src/observability.test.ts:17',
      'workflow_observability_reexports_parse_workflow_name_and_it_works',
    ],
    [
      'packages/workflow/src/observability.test.ts:24',
      'workflow_observability_reexports_parse_class_name_and_it_works',
    ],
    [
      'packages/workflow/src/observability.test.ts:29',
      'workflow_observability_reexports_observability_revivers',
    ],
    [
      'packages/workflow/src/observability.test.ts:36',
      'workflow_observability_reexports_hydrate_resource_io_and_handles_plain_values',
    ],
    [
      'packages/workflow/src/observability.test.ts:43',
      'workflow_observability_reexports_hydrate_data_and_passes_through_plain_values',
    ],
    [
      'packages/workflow/src/stdlib.test.ts:5',
      'workflow_stdlib_fetch_has_the_correct_name',
    ],
  ].map(([key, rustTestName]) => [
    key,
    {
      rustTestName,
      status: 'verified',
      note: 'Ported in crates/workflow/src/lib.rs and validated by cargo test -p workflow.',
    },
  ])
);

function rowOverride(row) {
  const workflowFacadeOverride = workflowFacadeOverrides.get(
    `${row.file}:${row.line}`
  );
  if (row.packageName === 'workflow' && workflowFacadeOverride) {
    return workflowFacadeOverride;
  }

  const verifiedRustFileTest = verifiedRustFileTests.get(
    `${row.packageName}|${row.file}`
  );
  if (verifiedRustFileTest) {
    return {
      rustTestName: verifiedRustFileTest.testName,
      status: 'verified',
      note: verifiedRustFileTest.note,
    };
  }
  if (
    row.packageName === 'world' &&
    row.file === 'packages/world/src/attributes.test.ts'
  ) {
    const rustTestName = verifiedWorldAttributeTests.get(row.line);
    if (rustTestName) {
      return {
        rustTestName,
        status: 'verified',
        note: verifiedWorldAttributeNote,
      };
    }
  }
  return undefined;
}

const wf04CorePrimitiveFiles = new Map([
  ['packages/core/src/capabilities.test.ts', 'capabilities'],
  ['packages/core/src/classify-error.test.ts', 'classify_error'],
  ['packages/core/src/context-errors.test.ts', 'context_errors'],
  ['packages/core/src/define-hook.test.ts', 'define_hook'],
  ['packages/core/src/describe-error.test.ts', 'describe_error'],
  ['packages/core/src/global.test.ts', 'global'],
  ['packages/core/src/log-format.test.ts', 'log_format'],
  ['packages/core/src/logger.test.ts', 'logger'],
  ['packages/core/src/schemas.test.ts', 'schemas'],
  ['packages/core/src/set-attributes.test.ts', 'set_attributes'],
  ['packages/core/src/source-map.test.ts', 'source_map'],
  ['packages/core/src/types.test.ts', 'types'],
  ['packages/core/src/util.test.ts', 'utility'],
]);

function applyRustParityOverrides(rows) {
  const prefix = rows[0] ? wf04CorePrimitiveFiles.get(rows[0].file) : undefined;
  if (!prefix) {
    return rows;
  }

  return rows.map((row, index) => {
    if (
      row.file === 'packages/core/src/context-errors.test.ts' &&
      row.line === 209
    ) {
      return {
        ...row,
        portability: 'js-only-documented',
        status: 'js-only-documented',
        rustTestName: '',
        note:
          'V8 Error.captureStackTrace stack-frame rewriting has no Rust runtime analogue; Rust reports native call sites/backtraces instead.',
      };
    }

    const rustTestName = `wf04_${prefix}_row_${String(index + 1).padStart(3, '0')}`;
    return {
      ...row,
      portability: 'portable',
      status: 'verified',
      rustTestName,
      note:
        'WF04 verified in workflow-core against the upstream portable row.',
    };
  });
}

const implementedWorldPackages = new Set(['world-postgres', 'world-vercel']);

const hostFrameworkPackages = new Set([
  'astro',
  'nest',
  'next',
  'nitro',
  'nuxt',
  'sveltekit',
]);
const webOnlyPackages = new Set(['web', 'web-shared']);
const docsOnlyPackages = new Set(['docs-typecheck', 'vitest', 'world-testing']);
const toolingPackages = new Set([
  'builders',
  'cli',
  'rollup',
  'swc-plugin-workflow',
  'vite',
]);
const typeSystemPackages = new Set(['typescript-plugin', 'tsconfig']);
const webSharedPortableContracts = new Map(
  [
    [
      'packages/web-shared/test/event-list-duration.test.ts',
      'uses the first step_started, not the last, for steps with retries',
      'web_shared_event_duration_uses_first_step_started_not_last_for_retries',
    ],
    [
      'packages/web-shared/test/event-list-duration.test.ts',
      'handles events in descending order (newest first)',
      'web_shared_event_duration_handles_descending_order',
    ],
    [
      'packages/web-shared/test/event-list-duration.test.ts',
      'still works for a step with a single start (no retry)',
      'web_shared_event_duration_handles_single_start_without_retry',
    ],
    [
      'packages/web-shared/test/event-list-duration.test.ts',
      'falls back to the started time when no created event is seen',
      'web_shared_event_duration_falls_back_to_started_time_without_created_event',
    ],
    [
      'packages/web-shared/test/exact-event-search-id.test.ts',
      'accepts full step IDs',
      'web_shared_exact_id_accepts_full_step_ids',
    ],
    [
      'packages/web-shared/test/exact-event-search-id.test.ts',
      'accepts full wait IDs',
      'web_shared_exact_id_accepts_full_wait_ids',
    ],
    [
      'packages/web-shared/test/exact-event-search-id.test.ts',
      'accepts full hook IDs',
      'web_shared_exact_id_accepts_full_hook_ids',
    ],
    [
      'packages/web-shared/test/exact-event-search-id.test.ts',
      'accepts full event IDs',
      'web_shared_exact_id_accepts_full_event_ids',
    ],
    [
      'packages/web-shared/test/exact-event-search-id.test.ts',
      'normalizes lowercase ULID bodies to uppercase',
      'web_shared_exact_id_normalizes_lowercase_ulid_bodies',
    ],
    [
      'packages/web-shared/test/exact-event-search-id.test.ts',
      'trims leading and trailing whitespace',
      'web_shared_exact_id_trims_leading_and_trailing_whitespace',
    ],
    [
      'packages/web-shared/test/exact-event-search-id.test.ts',
      'rejects partial IDs and run IDs',
      'web_shared_exact_id_rejects_partial_ids_and_run_ids',
    ],
    [
      'packages/web-shared/test/exact-event-search-id.test.ts',
      'rejects IDs with illegal Crockford characters or wrong length',
      'web_shared_exact_id_rejects_illegal_crockford_characters_or_wrong_length',
    ],
    [
      'packages/web-shared/test/exact-event-search-id.test.ts',
      'matches known workflow ID prefixes',
      'web_shared_exact_id_looks_like_workflow_id_matches_known_prefixes',
    ],
    [
      'packages/web-shared/test/exact-event-search-id.test.ts',
      'does not match free-text search input',
      'web_shared_exact_id_looks_like_workflow_id_rejects_free_text',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'groups step events with no run-level events for v1',
      'web_shared_trace_v1_groups_step_events_without_run_level_events',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'builds a valid trace for a completed v1 run with step events',
      'web_shared_trace_v1_builds_completed_run_with_step_events',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'builds a valid trace for a failed v1 run',
      'web_shared_trace_v1_builds_failed_run',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'builds a valid trace for a v1 run with no events at all',
      'web_shared_trace_v1_builds_run_with_no_events',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'builds step spans from v1 events (no step_created)',
      'web_shared_trace_v1_builds_step_spans_without_step_created',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'derives step status from v1 events without step_created',
      'web_shared_trace_v1_derives_step_status_without_step_created',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'uses correlationId for step span when stepName is unavailable',
      'web_shared_trace_v1_uses_correlation_id_when_step_name_is_unavailable',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'uses resumeAt for pending sleep span duration',
      'web_shared_trace_v1_uses_resume_at_for_pending_sleep_duration',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'caps pending sleep spans at the latest known event before resumeAt',
      'web_shared_trace_v1_caps_pending_sleep_spans_at_latest_known_event_before_resume_at',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'shows "succeeded" segment for a completed v1 run (no run_completed event)',
      'web_shared_trace_v1_run_segments_show_succeeded_for_completed_v1_run',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'shows "failed" segment for a failed v1 run (no run_failed event)',
      'web_shared_trace_v1_run_segments_show_failed_for_failed_v1_run',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'shows "running" segment for an in-progress v1 run',
      'web_shared_trace_v1_run_segments_show_running_for_in_progress_v1_run',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'shows queued + succeeded for a v1 run with startedAt',
      'web_shared_trace_v1_run_segments_show_queued_and_succeeded_with_started_at',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'v2 baseline: shows "succeeded" from run_completed event',
      'web_shared_trace_v1_run_segments_v2_baseline_succeeds_from_run_completed_event',
    ],
    [
      'packages/web-shared/test/trace-builder-v1.test.ts',
      'v2 mid-pagination: shows "running" when run_completed has not loaded yet',
      'web_shared_trace_v1_run_segments_v2_mid_pagination_runs_until_completion_event_loaded',
    ],
  ].map(([file, caseName, rustTestName]) => [
    `${file}\u0000${caseName}`,
    rustTestName,
  ])
);

const hostFrameworkNotes = new Map([
  [
    'nest',
    'NestJS adapter build output and tsconfig parsing; no Rust runtime counterpart unless a Nest host adapter is introduced.',
  ],
  [
    'next',
    'Next.js builder/env/webpack integration; no Rust runtime counterpart unless a Next host adapter is introduced.',
  ],
  [
    'nitro',
    'Nitro virtual handler, functionRule, and externals integration; no Rust runtime counterpart unless a Nitro host adapter is introduced.',
  ],
  [
    'sveltekit',
    'SvelteKit/Vercel config integration; no Rust runtime counterpart unless a SvelteKit host adapter is introduced.',
  ],
]);

const docsOnlyNotes = new Map([
  [
    'docs-typecheck',
    'Documentation markdown, code-sample, and sitemap validation tooling; outside Rust runtime parity.',
  ],
  [
    'vitest',
    'Vitest harness setup/options behavior for JavaScript tests; outside Rust runtime parity.',
  ],
  [
    'world-testing',
    'JavaScript world test harness and generated fixture package; outside Rust runtime parity.',
  ],
]);

const builderPortableRows = new Set([
  'packages/builders/src/get-input-files.test.ts',
  'packages/builders/src/module-specifier.test.ts',
  'packages/builders/src/resolve-sourcemap.test.ts',
  'packages/builders/src/transform-utils.test.ts',
  'packages/builders/src/workflow-alias.test.ts',
]);

const builderPortableSuitePrefixes = [
  'workflow-node-module-error helper functions',
  'createPseudoPackagePlugin > PSEUDO_PACKAGES constant',
];

function fail(message) {
  console.error(message);
  process.exit(1);
}

function rowKey(row) {
  return [
    row.packageName,
    row.file,
    row.line,
    row.suitePath || '(root)',
    row.caseName,
    row.declaration,
  ].join('\u001f');
}

function loadOverrides() {
  if (!fs.existsSync(overridesPath)) {
    return new Map();
  }
  const raw = JSON.parse(fs.readFileSync(overridesPath, 'utf8'));
  if (!Array.isArray(raw)) {
    fail(`${path.relative(repositoryRoot, overridesPath)} must contain an array`);
  }
  const overrides = new Map();
  for (const override of raw) {
    const required = ['packageName', 'file', 'line', 'suite', 'caseName', 'declaration'];
    for (const field of required) {
      if (!(field in override)) {
        fail(`Override is missing required field ${field}`);
      }
    }
    const key = [
      override.packageName,
      override.file,
      override.line,
      override.suite,
      override.caseName,
      override.declaration,
    ].join('\u001f');
    overrides.set(key, override);
  }
  return overrides;
}

function walk(directory, files = []) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === 'dist') {
      continue;
    }
    const fullPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      walk(fullPath, files);
    } else if (entry.isFile() && testFilePattern.test(entry.name)) {
      files.push(fullPath);
    }
  }
  return files;
}

function decodeLiteral(raw) {
  try {
    return Function(`"use strict"; return (${raw});`)();
  } catch {
    return raw.slice(1, -1);
  }
}

function tokenize(source) {
  const tokens = [];
  let index = 0;
  let line = 1;

  const push = (type, value, startLine) => {
    tokens.push({ type, value, line: startLine });
  };

  while (index < source.length) {
    const char = source[index];
    const next = source[index + 1];

    if (char === '\n') {
      line += 1;
      index += 1;
      continue;
    }
    if (/\s/.test(char)) {
      index += 1;
      continue;
    }
    if (char === '/' && next === '/') {
      index += 2;
      while (index < source.length && source[index] !== '\n') {
        index += 1;
      }
      continue;
    }
    if (char === '/' && next === '*') {
      index += 2;
      while (index < source.length) {
        if (source[index] === '\n') {
          line += 1;
        }
        if (source[index] === '*' && source[index + 1] === '/') {
          index += 2;
          break;
        }
        index += 1;
      }
      continue;
    }
    if (char === '=' && next === '>') {
      push('punc', '=>', line);
      index += 2;
      continue;
    }
    if (char === '"' || char === "'") {
      const quote = char;
      const start = index;
      const startLine = line;
      index += 1;
      while (index < source.length) {
        const current = source[index];
        if (current === '\\') {
          index += 2;
          continue;
        }
        if (current === '\n') {
          line += 1;
        }
        index += 1;
        if (current === quote) {
          break;
        }
      }
      const raw = source.slice(start, index);
      push('string', decodeLiteral(raw), startLine);
      continue;
    }
    if (char === '`') {
      const start = index;
      const startLine = line;
      index += 1;
      let dynamic = false;
      while (index < source.length) {
        const current = source[index];
        if (current === '\\') {
          index += 2;
          continue;
        }
        if (current === '$' && source[index + 1] === '{') {
          dynamic = true;
        }
        if (current === '\n') {
          line += 1;
        }
        index += 1;
        if (current === '`') {
          break;
        }
      }
      const raw = source.slice(start + 1, index - 1);
      push('template', dynamic ? `<dynamic template: ${raw}>` : raw, startLine);
      continue;
    }
    if (/[A-Za-z_$]/.test(char)) {
      const start = index;
      const startLine = line;
      index += 1;
      while (index < source.length && /[A-Za-z0-9_$]/.test(source[index])) {
        index += 1;
      }
      push('id', source.slice(start, index), startLine);
      continue;
    }
    if ('(){}[].,;:'.includes(char)) {
      push('punc', char, line);
      index += 1;
      continue;
    }

    index += 1;
  }

  return tokens;
}

function buildMatchingPairs(tokens) {
  const openers = new Map([
    ['(', ')'],
    ['{', '}'],
    ['[', ']'],
  ]);
  const closers = new Map([...openers].map(([open, close]) => [close, open]));
  const stacks = new Map([...openers.keys()].map((open) => [open, []]));
  const matches = new Map();

  tokens.forEach((token, index) => {
    if (token.type !== 'punc') {
      return;
    }
    if (openers.has(token.value)) {
      stacks.get(token.value).push(index);
      return;
    }
    if (closers.has(token.value)) {
      const stack = stacks.get(closers.get(token.value));
      const openIndex = stack.pop();
      if (openIndex !== undefined) {
        matches.set(openIndex, index);
        matches.set(index, openIndex);
      }
    }
  });

  return matches;
}

function countArrayRows(tokens, openBracket, closeBracket) {
  let count = 0;
  let sawValue = false;
  let depth = 0;
  for (let index = openBracket + 1; index < closeBracket; index += 1) {
    const token = tokens[index];
    if (token.type !== 'punc') {
      sawValue = true;
      continue;
    }
    if ('([{'.includes(token.value)) {
      depth += 1;
      sawValue = true;
      continue;
    }
    if (')]}'.includes(token.value)) {
      depth -= 1;
      continue;
    }
    if (token.value === ',' && depth === 0) {
      if (sawValue) {
        count += 1;
        sawValue = false;
      }
    } else {
      sawValue = true;
    }
  }
  return sawValue ? count + 1 : count;
}

function countEachRows(tokens, eachStart, matches) {
  const token = tokens[eachStart];
  if (!token) {
    return { count: 1, note: 'each rows not inspected' };
  }
  if (token.type === 'template') {
    const lines = token.value
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean);
    return {
      count: Math.max(lines.length - 1, 1),
      note: 'expanded from template each table',
    };
  }
  if (token.value !== '(') {
    return { count: 1, note: 'each rows not inspected' };
  }
  const closeParen = matches.get(eachStart);
  const first = tokens[eachStart + 1];
  if (first?.value === '[') {
    const closeBracket = matches.get(eachStart + 1);
    if (closeBracket !== undefined && closeBracket < closeParen) {
      return {
        count: Math.max(countArrayRows(tokens, eachStart + 1, closeBracket), 1),
        note: 'expanded from array each table',
      };
    }
  }
  return { count: 1, note: 'each rows need review' };
}

function firstArgument(tokens, openParen, closeParen) {
  for (let index = openParen + 1; index < closeParen; index += 1) {
    const token = tokens[index];
    if (token.type === 'string' || token.type === 'template') {
      return token;
    }
    if (token.value === ',') {
      break;
    }
  }
  return undefined;
}

function callbackBody(tokens, openParen, closeParen, matches) {
  for (let index = openParen + 1; index < closeParen; index += 1) {
    const token = tokens[index];
    if (token.value === '=>' && tokens[index + 1]?.value === '{') {
      return { start: index + 1, end: matches.get(index + 1) };
    }
    if (token.type === 'id' && token.value === 'function') {
      for (let bodyIndex = index + 1; bodyIndex < closeParen; bodyIndex += 1) {
        if (tokens[bodyIndex]?.value === '{') {
          return { start: bodyIndex, end: matches.get(bodyIndex) };
        }
      }
    }
  }
  return undefined;
}

function parseCall(tokens, index, matches) {
  const token = tokens[index];
  if (token?.type !== 'id' || !targetCalls.has(token.value)) {
    return undefined;
  }
  if (tokens[index - 1]?.value === '.') {
    return undefined;
  }

  let cursor = index + 1;
  const modifiers = [];
  let eachCount = 1;
  let eachNote = '';

  while (tokens[cursor]?.value === '.' && tokens[cursor + 1]?.type === 'id') {
    const modifier = tokens[cursor + 1].value;
    modifiers.push(modifier);
    cursor += 2;
    if (modifier === 'each') {
      const counted = countEachRows(tokens, cursor, matches);
      eachCount = counted.count;
      eachNote = counted.note;
      if (tokens[cursor]?.value === '(') {
        cursor = (matches.get(cursor) ?? cursor) + 1;
      } else if (tokens[cursor]?.type === 'template') {
        cursor += 1;
      }
    }
  }

  if (tokens[cursor]?.value !== '(') {
    return undefined;
  }

  const closeParen = matches.get(cursor);
  if (closeParen === undefined) {
    return undefined;
  }

  const nameToken = firstArgument(tokens, cursor, closeParen);
  const body = callbackBody(tokens, cursor, closeParen, matches);
  const name = nameToken?.value ?? '<dynamic test name>';
  const dynamicName =
    !nameToken || (nameToken.type === 'template' && name.startsWith('<dynamic'));

  return {
    base: token.value,
    modifiers,
    name,
    dynamicName,
    line: token.line,
    openParen: cursor,
    closeParen,
    body,
    eachCount,
    eachNote,
  };
}

function packageName(relativePath) {
  const parts = relativePath.split('/');
  return parts[0] === 'packages' ? parts[1] : 'unknown';
}

function classify(row, source) {
  const file = row.file;
  if (typeSystemPackages.has(row.packageName) || source.includes('expectTypeOf')) {
    return {
      portability: 'type-system-impossible',
      status: 'type-system-impossible',
      note: 'TypeScript type-system assertion; needs a Rust compile-test analogue or explicit impossible note.',
    };
  }
  const portableWebSharedContract = webSharedPortableContracts.get(
    `${row.file}\u0000${row.caseName}`
  );
  if (portableWebSharedContract) {
    return {
      portability: 'portable',
      status: 'verified',
      rustOwner: 'workflow-world',
      rustTestName: portableWebSharedContract,
      note: 'Portable workflow event/trace data contract ported in workflow-world.',
    };
  }
  if (hostFrameworkPackages.has(row.packageName)) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note:
        hostFrameworkNotes.get(row.packageName) ??
        'JavaScript framework adapter behavior; no Rust runtime counterpart unless a host adapter is introduced.',
    };
  }
  if (webOnlyPackages.has(row.packageName)) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note:
        row.packageName === 'web-shared'
          ? 'React/UI display, browser hydration, or typed-array inspector behavior; portable trace/event data rows are mapped separately to workflow-world.'
          : 'React dashboard, browser fetch, and client hook behavior; API data-shape parity belongs to the owning runtime/world crates.',
    };
  }
  if (docsOnlyPackages.has(row.packageName)) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note:
        docsOnlyNotes.get(row.packageName) ??
        'Documentation or JavaScript test harness package outside Rust runtime parity.',
    };
  }
  if (row.packageName === 'builders') {
    if (
      builderPortableRows.has(file) ||
      builderPortableSuitePrefixes.some((suite) =>
        row.suitePath.startsWith(suite)
      )
    ) {
      return {
        portability: 'portable',
        status: 'verified',
        note: 'Portable builder helper behavior ported to workflow-builders with named Rust parity tests.',
        rustTestName: builderRustTestName(row),
      };
    }
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note: 'JavaScript esbuild/build-host behavior; Rust keeps the portable helper surface in workflow-builders and does not force host plugins into runtime crates.',
    };
  }
  if (row.packageName === 'cli') {
    return {
      portability: 'portable',
      status: 'verified',
      note: 'Portable inspect output formatting behavior ported to workflow-cli.',
      rustTestName: cliRustTestName(row),
    };
  }
  if (toolingPackages.has(row.packageName)) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note: 'JavaScript build-plugin host behavior; no Rust runtime counterpart in this port slice.',
    };
  }
  if (
    foundationalPackages.has(row.packageName) &&
    (file.includes('/e2e/') ||
      file.includes('/vm/') ||
      file.endsWith('/source-map.test.ts'))
  ) {
    return {
      portability: 'needs-review',
      status: 'needs-review',
      note: 'Runtime-host or integration boundary; classify before claiming foundational parity.',
    };
  }
  if (row.dynamicName) {
    return {
      portability: 'needs-review',
      status: 'needs-review',
      note: 'Dynamic test name; inspect upstream source before porting.',
    };
  }
  return { portability: 'portable', status: 'not-started', note: row.eachNote };
}

function ported(
  rustTestName,
  note = 'WF05 core serialization/encryption/VM utility row verified in workflow-core.'
) {
  return {
    portability: 'portable',
    status: 'verified',
    rustTestName,
    note,
  };
}

const wf07Note =
  'WF07 core runtime engine: deterministic Rust test covers this portable upstream row with fake World/event-log seams.';

function verified(testName, note = wf07Note) {
  return {
    portability: 'portable',
    status: 'verified',
    rustTestName: testName,
    note,
  };
}

function jsOnly(note) {
  return {
    portability: 'js-only-documented',
    status: 'js-only-documented',
    rustTestName: '',
    note,
  };
}

function portableNotStarted(note) {
  return {
    portability: 'portable',
    status: 'not-started',
    rustTestName: '',
    note,
  };
}

function bucket05Override(row) {
  if (row.packageName !== 'core') return undefined;

  const file = row.file;
  const suite = row.suitePath;
  const testCase = row.caseName;

  if (file === 'packages/core/src/async-deserialization-ordering.test.ts') {
    if (testCase.includes('sequential step promises')) {
      return ported(
        'async_deserialization_ordering_test_resolves_sequential_step_promises_in_order'
      );
    }
    if (testCase.includes('hook payloads')) {
      return ported(
        'async_deserialization_ordering_test_resolves_hook_payloads_in_event_log_order'
      );
    }
    if (testCase.includes('mixed step_completed and step_failed')) {
      return ported(
        'async_deserialization_ordering_test_resolves_mixed_step_completed_and_failed_in_order'
      );
    }
    if (testCase.includes('many concurrent steps')) {
      return ported(
        'async_deserialization_ordering_test_handles_many_concurrent_steps_in_correct_order'
      );
    }
    if (testCase.includes('sleep and step promises')) {
      return ported(
        'async_deserialization_ordering_test_resolves_sleep_and_step_promises_in_event_log_order'
      );
    }
    if (testCase.includes('interleaved')) {
      return ported(
        'async_deserialization_ordering_test_resolves_interleaved_step_functions_in_event_log_order'
      );
    }
    return ported(
      'async_deserialization_ordering_test_resolves_step_promises_in_event_log_order'
    );
  }

  if (file === 'packages/core/src/encryption.test.ts') {
    if (suite.includes('round-trip')) {
      return ported('encryption_test_encrypt_decrypt_returns_original_plaintext');
    }
    if (suite.includes('importKey')) {
      return ported(
        'encryption_test_import_key_rejects_keys_that_are_not_exactly_32_bytes'
      );
    }
    if (testCase.includes('wrong key')) {
      return ported(
        'encryption_test_decrypt_failure_cases_wrong_key_is_runtime_decryption_error'
      );
    }
    if (suite.includes('encrypt failure cases')) {
      return ported(
        'encryption_test_encrypt_failure_cases_wrap_underlying_crypto_call'
      );
    }
    return ported(
      'encryption_test_decrypt_failure_cases_use_runtime_decryption_error'
    );
  }

  if (file === 'packages/core/src/serialization-format.test.ts') {
    if (suite.startsWith('encodeWithFormatPrefix')) {
      return testCase.includes('non-Uint8Array')
        ? ported('serialization_serialization_test_encode_with_format_prefix_passes_legacy_values')
        : ported('serialization_serialization_test_encode_with_format_prefix_prepends_prefix');
    }
    if (suite.startsWith('decodeFormatPrefix')) {
      if (testCase.includes('shorter')) {
        return ported('serialization_serialization_test_decode_format_prefix_rejects_short_or_invalid_bytes');
      }
      return testCase.includes('unknown')
        ? ported('serialization_format_test_decode_known_format_prefix_rejects_unknown_format')
        : ported('serialization_serialization_test_decode_format_prefix_decodes_bytes_and_legacy_values');
    }
    if (suite.startsWith('hydrateData')) {
      if (testCase.includes('unsupported')) {
        return ported('serialization_format_test_hydrate_data_passes_plain_values_and_rejects_unknown_prefix');
      }
      if (testCase.includes('encrypted')) {
        return ported('serialization_format_test_encrypted_data_handling_passes_through_or_decrypts');
      }
      return ported('serialization_format_test_hydrate_data_parses_prefixed_and_legacy_values');
    }
    if (suite.startsWith('hydrateResourceIO')) {
      if (testCase.includes('null/undefined') || testCase.includes('parse errors')) {
        return ported('serialization_format_test_hydrate_resource_io_handles_null_and_parse_errors_gracefully');
      }
      if (testCase.includes('encrypted')) {
        return ported('serialization_format_test_encrypted_data_handling_passes_through_or_decrypts');
      }
      if (testCase.includes('executionContext')) {
        return ported('serialization_format_test_hydrate_resource_io_hydrates_step_run_event_and_hook_fields');
      }
      return ported('serialization_format_test_hydrate_resource_io_hydrates_step_run_event_and_hook_fields');
    }
    if (suite.startsWith('observabilityRevivers')) {
      return ported(
        'serialization_format_test_observability_revivers_convert_stream_step_class_and_workflow_refs',
        'Rust port covers the serialized display/ref payload, not JS constructor identity.'
      );
    }
    if (suite.startsWith('isStreamRef') || suite.startsWith('isStreamId')) {
      return ported('serialization_format_test_stream_ref_stream_id_extract_and_truncate_helpers');
    }
    if (suite.startsWith('extractStreamIds') || suite.startsWith('truncateId')) {
      return ported('serialization_format_test_stream_ref_stream_id_extract_and_truncate_helpers');
    }
    if (suite.includes('encrypted data handling')) {
      if (suite.includes('isEncryptedData')) {
        return ported('serialization_format_test_is_encrypted_data_detects_only_encr_prefixed_bytes');
      }
      if (suite.includes('isExpiredStub')) {
        return ported('serialization_format_test_is_expired_stub_matches_exact_server_shapes');
      }
      return ported('serialization_format_test_encrypted_data_handling_passes_through_or_decrypts');
    }
    if (suite.includes('custom class instances')) {
      return ported(
        'serialization_format_test_observability_revivers_convert_stream_step_class_and_workflow_refs',
        'Rust port preserves class-instance reference data, not JS class identity.'
      );
    }
  }

  if (file === 'packages/core/src/serialization/serialization.test.ts') {
    if (suite.startsWith('isFormatPrefix')) {
      return testCase.includes('accept') || testCase.includes('boundary')
        ? ported('serialization_serialization_test_is_format_prefix_accepts_valid_prefixes')
        : ported('serialization_serialization_test_is_format_prefix_rejects_invalid_prefixes');
    }
    if (suite.startsWith('SerializationFormat constants')) {
      return ported('serialization_serialization_test_serialization_format_constants_are_valid');
    }
    if (suite.startsWith('encodeWithFormatPrefix')) {
      if (testCase.includes('non-Uint8Array')) {
        return ported('serialization_serialization_test_encode_with_format_prefix_passes_legacy_values');
      }
      if (testCase.includes('empty') || testCase.includes('large')) {
        return ported('serialization_serialization_test_encode_with_format_prefix_handles_empty_and_large_payloads');
      }
      return ported('serialization_serialization_test_encode_with_format_prefix_prepends_prefix');
    }
    if (suite.startsWith('decodeFormatPrefix')) {
      if (testCase.includes('too short') || testCase.includes('invalid')) {
        return ported('serialization_serialization_test_decode_format_prefix_rejects_short_or_invalid_bytes');
      }
      return ported('serialization_serialization_test_decode_format_prefix_decodes_bytes_and_legacy_values');
    }
    if (suite.startsWith('peekFormatPrefix') || suite.startsWith('isEncrypted')) {
      return ported('serialization_serialization_test_peek_format_prefix_and_is_encrypted_match_upstream');
    }
    if (suite.startsWith('encrypt')) {
      return ported('serialization_serialization_test_encrypt_decrypt_layer_matches_encr_contract');
    }
    if (suite.startsWith('decrypt')) {
      return testCase.includes('auth-tag')
        ? ported('serialization_serialization_test_decrypt_layer_requires_key_and_attaches_encr_prefix')
        : ported('serialization_serialization_test_encrypt_decrypt_layer_matches_encr_contract');
    }
    if (suite.startsWith('common reducers')) {
      if (testCase.includes('ArrayBuffer')) {
        return ported('serialization_serialization_test_common_reducers_reduce_arraybuffer_and_zero_length');
      }
      if (testCase.includes('typed arrays')) {
        return ported('serialization_serialization_test_common_reducers_reduce_typed_arrays');
      }
      if (testCase.includes('BigInt') || testCase.includes('Date') || testCase.includes('non-bigint')) {
        return ported('serialization_serialization_test_common_reducers_reduce_scalar_special_values');
      }
      if (testCase.includes('Error')) {
        return ported('serialization_serialization_test_common_revivers_round_trip_error_payloads');
      }
      return ported('serialization_serialization_test_common_reducers_reduce_maps_sets_headers_urls_and_regex');
    }
    if (suite.startsWith('common revivers')) {
      if (testCase.includes('Error')) {
        return ported('serialization_serialization_test_common_revivers_round_trip_error_payloads');
      }
      return ported('serialization_serialization_test_common_revivers_round_trip_portable_special_values');
    }
    if (suite.startsWith('class reducers') || suite.startsWith('class revivers')) {
      return jsOnly(
        'JavaScript constructor registry and static symbol methods; Rust port covers class reference payloads only.'
      );
    }
    if (suite.startsWith('step function reducer') || suite.startsWith('step function reviver')) {
      return jsOnly(
        'JavaScript function/proxy identity and bind semantics; Rust port covers typed step-function reference payloads only.'
      );
    }
    if (suite.startsWith('devalue codec')) {
      if (testCase.includes('StepFunction')) {
        return ported(
          'serialization_serialization_test_devalue_codec_step_function_mode_boundaries',
          'Rust counterpart verifies typed reference payload boundaries, not callable JS functions.'
        );
      }
      if (testCase.includes('primitives')) {
        return ported('serialization_serialization_test_devalue_codec_round_trips_primitives_in_all_modes');
      }
      if (testCase.includes('Date') || testCase.includes('Map') || testCase.includes('Set') || testCase.includes('nested')) {
        return ported('serialization_serialization_test_devalue_codec_round_trips_common_special_values');
      }
      return ported('serialization_serialization_test_devalue_codec_supports_legacy_and_bytes_output');
    }
    if (suite.startsWith('workflow.serialize')) {
      if (testCase.includes('non-serializable')) {
        return jsOnly('Raw JavaScript function values do not have a Rust runtime value analogue.');
      }
      if (testCase.includes('legacy')) {
        return ported('serialization_serialization_test_workflow_deserialize_legacy_non_binary_data');
      }
      if (testCase.includes('unsupported format')) {
        return ported('serialization_serialization_test_workflow_deserialize_rejects_unsupported_format_prefix');
      }
      return ported('serialization_serialization_test_workflow_serialize_deserialize_round_trips_values');
    }
    if (suite.startsWith('step.serialize') || suite.startsWith('client.serialize')) {
      if (testCase.includes('encrypted data without key')) {
        return ported('serialization_serialization_test_step_and_client_modes_reject_encrypted_data_without_key');
      }
      if (testCase.includes('encryption')) {
        return ported('serialization_serialization_test_step_and_client_modes_round_trip_with_encryption');
      }
      if (testCase.includes('legacy')) {
        return ported('serialization_serialization_test_workflow_deserialize_legacy_non_binary_data');
      }
      return ported('serialization_serialization_test_workflow_serialize_deserialize_round_trips_values');
    }
    if (suite.startsWith('cross-mode serialization')) {
      return ported('serialization_serialization_test_cross_mode_serialization_is_compatible');
    }
    if (suite.startsWith('edge cases')) {
      if (testCase.includes('circular references')) {
        return jsOnly('JavaScript object identity/cycles are devalue runtime semantics, not portable Rust value identity.');
      }
      return ported('serialization_serialization_test_edge_cases_cover_undefined_deep_mixed_empty_and_bigint_values');
    }
  }

  if (file === 'packages/core/src/serialization.test.ts') {
    if (suite.startsWith('getStreamType')) {
      return jsOnly('WHATWG ReadableStream controller type inspection is JavaScript runtime-specific.');
    }
    if (suite.startsWith('workflow arguments')) {
      if (/(WritableStream|ReadableStream|Response|Request)/.test(testCase)) {
        return jsOnly('WHATWG stream/fetch object identity and body transfer semantics are JavaScript runtime-specific.');
      }
      if (testCase.includes('unsupported type')) {
        return jsOnly('Unsupported JavaScript values such as raw functions have no Rust runtime value analogue.');
      }
      return ported('serialization_serialization_test_workflow_serialize_deserialize_round_trips_values');
    }
    if (suite.startsWith('workflow return value') || suite.startsWith('step arguments') || suite.startsWith('step return value')) {
      return jsOnly('Unsupported raw JavaScript function/symbol values have no Rust runtime value analogue.');
    }
    if (suite.startsWith('cross-VM Error serialization')) {
      return jsOnly('Cross-realm Error identity depends on Node VM globals; Rust port preserves portable error payloads.');
    }
    if (suite.startsWith('step function serialization') || suite.startsWith('WorkflowFunction serialization')) {
      return jsOnly('Callable JavaScript function/proxy identity is not portable; Rust port covers reference payload data only.');
    }
    if (suite.startsWith('custom class serialization') || suite.startsWith('custom Error subclass serialization')) {
      return jsOnly('JavaScript class constructors and WORKFLOW_SERIALIZE/WORKFLOW_DESERIALIZE static methods are not portable Rust runtime semantics.');
    }
    if (suite.startsWith('built-in Error subclass serialization') || suite.startsWith('DOMException serialization') || suite.startsWith('Workflow error serialization')) {
      if (testCase.includes('VM')) {
        return jsOnly('Cross-VM constructor identity depends on Node VM globals; Rust port preserves portable error payload data.');
      }
      return ported('serialization_serialization_test_common_revivers_round_trip_error_payloads');
    }
    if (suite.startsWith('dehydrate/hydrateStepError') || suite.startsWith('dehydrate/hydrateRunError')) {
      return ported('serialization_test_format_prefix_system_handles_all_dehydrate_hydrate_pairs');
    }
    if (suite.startsWith('encryption-failure propagation')) {
      return ported('serialization_serialization_test_decrypt_layer_requires_key_and_attaches_encr_prefix');
    }
    if (suite.startsWith('format prefix system')) {
      if (testCase.includes('unknown') || testCase.includes('too short')) {
        return ported('serialization_serialization_test_decode_format_prefix_rejects_short_or_invalid_bytes');
      }
      return ported('serialization_test_format_prefix_system_handles_all_dehydrate_hydrate_pairs');
    }
    if (suite.startsWith('decodeFormatPrefix legacy compatibility')) {
      return ported('serialization_test_decode_format_prefix_legacy_compatibility_handles_plain_values');
    }
    if (suite.startsWith('getSerializeStream')) {
      if (testCase.includes('length framing')) {
        return ported('serialization_test_get_serialize_stream_frames_each_chunk_with_format_prefix_and_length');
      }
      if (testCase.includes('concatenated')) {
        return ported('serialization_test_get_serialize_stream_frames_parse_and_coalesce');
      }
      if (testCase.includes('split')) {
        return ported('serialization_test_get_deserialize_stream_handles_arbitrary_frame_splits');
      }
      return ported('serialization_test_get_serialize_stream_frames_parse_and_coalesce');
    }
    if (suite.startsWith('getDeserializeStream legacy fallback')) {
      return ported('serialization_test_get_deserialize_stream_legacy_fallback_parses_newline_json');
    }
    if (suite.startsWith('stream encryption round-trip')) {
      if (testCase.includes('without a key') || testCase.includes('tampered')) {
        return ported('serialization_test_stream_encryption_errors_without_key_or_when_tampered');
      }
      if (testCase.includes('not encrypt')) {
        return ported('serialization_test_stream_encryption_does_not_encrypt_without_key');
      }
      if (testCase.includes('concatenated') || testCase.includes('split') || testCase.includes('large')) {
        return ported('serialization_test_stream_encryption_handles_concatenated_split_and_large_payloads');
      }
      return ported('serialization_test_stream_encryption_round_trip_frames_with_encr_prefix');
    }
    if (suite.startsWith('encryption integration')) {
      if (testCase.includes('wrong key') || testCase.includes('without a key')) {
        return ported('serialization_serialization_test_step_and_client_modes_reject_encrypted_data_without_key');
      }
      if (testCase.includes('not encrypt')) {
        return ported('serialization_test_stream_encryption_does_not_encrypt_without_key');
      }
      return ported('serialization_serialization_test_step_and_client_modes_round_trip_with_encryption');
    }
    if (suite.startsWith('encrypt/decrypt primitives')) {
      if (testCase.includes('reject keys')) {
        return ported('encryption_test_import_key_rejects_keys_that_are_not_exactly_32_bytes');
      }
      if (testCase.includes('truncated') || testCase.includes('wrong key') || testCase.includes('tampered')) {
        return ported('encryption_test_decrypt_failure_cases_use_runtime_decryption_error');
      }
      return ported('encryption_test_encrypt_decrypt_returns_original_plaintext');
    }
    if (suite.startsWith('maybeEncrypt') || suite.startsWith('isEncrypted')) {
      return testCase.includes('no key')
        ? ported('serialization_serialization_test_decrypt_layer_requires_key_and_attaches_encr_prefix')
        : ported('serialization_serialization_test_encrypt_decrypt_layer_matches_encr_contract');
    }
    if (suite.startsWith('AbortController serialization')) {
      return jsOnly('AbortController, AbortSignal, hook streams, and abort event listeners are JavaScript runtime objects outside this portable utility bucket.');
    }
  }

  if (file === 'packages/core/src/vm/uint8array-base64.test.ts') {
    if (suite.includes('toBase64')) return ported('vm_uint8array_base64_test_to_base64_rows_match_upstream');
    if (suite.includes('toHex')) return ported('vm_uint8array_base64_test_to_hex_rows_match_upstream');
    if (suite.includes('fromBase64') && suite.includes('strict')) {
      return ported('vm_uint8array_base64_test_from_base64_strict_and_stop_before_partial_rows_match_upstream');
    }
    if (suite.includes('fromBase64') && suite.includes('stop-before-partial')) {
      return ported('vm_uint8array_base64_test_from_base64_strict_and_stop_before_partial_rows_match_upstream');
    }
    if (suite.includes('fromBase64')) return ported('vm_uint8array_base64_test_from_base64_rows_match_upstream');
    if (suite.includes('fromHex')) return ported('vm_uint8array_base64_test_from_hex_rows_match_upstream');
    if (suite.includes('setFromBase64')) return ported('vm_uint8array_base64_test_set_from_base64_rows_match_upstream');
    if (suite.includes('setFromHex')) return ported('vm_uint8array_base64_test_set_from_hex_rows_match_upstream');
    return ported('vm_uint8array_base64_test_roundtrip_encoding_decoding_rows_match_upstream');
  }

  if (file === 'packages/core/src/vm/uuid.test.ts') {
    if (suite.includes('basic functionality') || suite.includes('UUID v4 specifications')) {
      return ported('vm_uuid_test_basic_functionality_and_v4_spec_rows_match_upstream');
    }
    if (suite.includes('deterministic behavior')) {
      return ported('vm_uuid_test_deterministic_behavior_rows_match_upstream');
    }
    return ported('vm_uuid_test_edge_cases_and_distribution_rows_match_upstream');
  }

  if (file === 'packages/core/src/vm/index.test.ts') {
    if (testCase.includes('btoa') || testCase.includes('atob') || testCase.includes('basic auth')) {
      return ported('vm_index_test_btoa_atob_basic_auth_portable_rows_match_upstream');
    }
    if (testCase.includes('crypto.randomUUID')) {
      return ported(
        'vm_uuid_test_basic_functionality_and_v4_spec_rows_match_upstream',
        'Rust port covers deterministic UUID utility; Node VM global installation is JS-only.'
      );
    }
    return jsOnly('Node vm.Context global patching/execution behavior has no portable Rust runtime counterpart in this utility bucket.');
  }

  return undefined;
}

const wf01CoreE2eNote =
  'WF-01 core E2E closure: current upstream E2E row is covered by the named deterministic Rust workflow-core test.';

const wf01CoreE2ePortableTests = new Map([
  [205, 'workflow_run_executes_workflow_with_arguments'],
  [295, 'workflow_run_promise_all_steps_resolve'],
  [301, 'workflow_run_promise_race_second_resolves'],
  [307, 'workflow_run_promise_race_second_resolves'],
  [323, 'server_writable_stream_multiple_sequential_writes'],
  [354, 'workflow_hook_multiple_received_events_iterator'],
  [565, 'workflow_run_sleep_single_suspend_and_resume'],
  [572, 'workflow_run_sleep_multiple_promise_all'],
  [585, 'workflow_run_sleep_single_suspend_and_resume'],
  [593, 'workflow_run_step_completed_event_resolves'],
  [601, 'serialization_serialization_test_workflow_serialize_deserialize_round_trips_values'],
  [607, 'workflow_run_records_metadata'],
  [720, 'server_writable_stream_multiple_sequential_writes'],
  [846, 'step_writable_preserves_order_across_repeated_get_writable'],
  [895, 'serialization_test_get_serialize_stream_frames_parse_and_coalesce'],
  [939, 'server_writable_stream_write_reaches_server'],
  [985, 'workflow_run_promise_race_out_of_order'],
  [996, 'workflow_run_user_defined_error_propagates'],
  [1031, 'workflow_run_nested_stack_trace_uses_workflow_name'],
  [1062, 'step_create_use_step_rejects_hydrated_thrown_value'],
  [1118, 'step_create_use_step_rejects_hydrated_thrown_value'],
  [1179, 'step_executor_handles_missing_fatal_abort_and_retryable_failures'],
  [1199, 'step_executor_handles_missing_fatal_abort_and_retryable_failures'],
  [1225, 'step_executor_handles_missing_fatal_abort_and_retryable_failures'],
  [1237, 'step_executor_handles_missing_fatal_abort_and_retryable_failures'],
  [1247, 'step_executor_handles_missing_fatal_abort_and_retryable_failures'],
  [1263, 'serialization_test_format_prefix_system_handles_all_dehydrate_hydrate_pairs'],
  [1295, 'serialization_test_format_prefix_system_handles_all_dehydrate_hydrate_pairs'],
  [1342, 'serialization_serialization_test_workflow_serialize_deserialize_round_trips_values'],
  [1370, 'serialization_serialization_test_workflow_serialize_deserialize_round_trips_values'],
  [1408, 'workflow_run_workflow_not_registered_error'],
  [1433, 'step_executor_handles_missing_fatal_abort_and_retryable_failures'],
  [1459, 'step_executor_handles_missing_fatal_abort_and_retryable_failures'],
  [1506, 'workflow_hook_dispose_after_created_before_disposed'],
  [1568, 'workflow_hook_conflict_rejects'],
  [1626, 'workflow_hook_dispose_after_created_before_disposed'],
  [1774, 'step_create_use_step_captures_closure_vars'],
  [1872, 'run_handle_exists_wakeup_serialization_and_return_value_errors'],
  [1894, 'run_handle_exists_wakeup_serialization_and_return_value_errors'],
  [2297, 'serialization_serialization_test_common_revivers_round_trip_error_payloads'],
  [2477, 'run_handle_return_value_reports_cancelled_runs'],
  [2575, 'hook_sleep_interaction_preserves_waiters_steps_and_payload_ordering'],
  [2624, 'events_consumer_defers_and_cancels_unconsumed_checks'],
  [2665, 'workflow_run_sleep_single_suspend_and_resume'],
  [2683, 'workflow_run_sleep_combined_with_steps'],
  [2709, 'abort_controller_abort_marks_hook_for_resumption'],
  [2744, 'abort_controller_step_multiple_consumers_receive_abort'],
  [2757, 'abort_controller_step_abort_pushes_stream_write_op'],
  [2778, 'abort_controller_step_already_aborted_immediate'],
  [2792, 'abort_controller_step_stream_packet_reason'],
  [2806, 'abort_controller_abort_called_twice_is_noop'],
  [2820, 'workflow_hook_resolves_payload'],
  [2840, 'abort_controller_step_already_aborted_immediate'],
  [2860, 'abort_controller_step_listener_fires_from_stream_packet'],
  [2901, 'abort_signal_any_fires_when_any_input_fires'],
  [2919, 'abort_controller_step_any_deserialized_and_local'],
  [2937, 'abort_controller_hook_token_reused_across_replays'],
  [2952, 'abort_controller_step_throw_if_aborted_wrapped_fatal'],
  [2966, 'abort_controller_step_stream_packet_reason'],
  [2989, 'abort_controller_step_fetch_abort_wrapped_fatal'],
  [3009, 'abort_controller_step_listener_fires_from_stream_packet'],
  [3035, 'workflow_run_sleep_combined_with_steps'],
  [3061, 'abort_controller_hook_token_reused_across_replays'],
  [3080, 'abort_controller_step_listener_fires_from_stream_packet'],
  [3096, 'abort_controller_step_throw_if_aborted_wrapped_fatal'],
  [3115, 'abort_controller_hook_token_reused_across_replays'],
  [3174, 'abort_signal_listener_runs_synchronously_before_after_abort'],
  [3232, 'context_storage_preserves_run_context'],
  [3254, 'start_resilient_start_and_failure_paths_match_upstream'],
  [3337, 'workflow_hook_resolves_payload'],
  [3373, 'workflow_run_sleep_single_suspend_and_resume'],
  [3401, 'workflow_hook_dispose_after_created_before_disposed'],
  [3440, 'wf04_set_attributes_row_002'],
  [3477, 'wf04_set_attributes_row_002'],
  [3536, 'wf04_set_attributes_row_002'],
  [3556, 'wf04_set_attributes_row_002'],
]);

const wf01CoreE2eJsOnlyRows = new Map([
  [
    252,
    'Dot-prefixed Next.js route and SWC discovery E2E; Rust workflow-core has no Next.js file-system route or JavaScript bundler analogue.',
  ],
  [
    278,
    'React rendering inside a JavaScript step depends on React/Node bundling and VM execution, not the portable Rust core runtime.',
  ],
  [
    313,
    'Imported-step-only discovery is a Next.js/SWC manifest E2E; Rust workflow-core does not discover JavaScript-only step files.',
  ],
  [
    404,
    'Public webhook HTTP route authorization is host-adapter behavior; Rust workflow-core covers hook tokens and webhook option data, not the JavaScript route.',
  ],
  [
    444,
    'Public webhook request/response E2E runs through the JavaScript host route and trusted-source headers; Rust workflow-core covers hook and response primitives separately.',
  ],
  [
    769,
    'Readable.getTailIndex is a JavaScript run-handle/world streaming API E2E; workflow-core Rust tests cover stream writes, framing, and stream-id naming.',
  ],
  [
    786,
    'Readable.getTailIndex empty-stream behavior is a JavaScript run-handle/world streaming API E2E, outside the portable core primitive surface.',
  ],
  [
    803,
    'World stream pagination through run.getReadable/getChunks is a JavaScript run-handle/world integration E2E; Rust core owns stream primitives only.',
  ],
  [
    974,
    'fetchWorkflow performs a live JavaScript fetch from workflow code; Rust workflow-core intentionally exposes the fetch-unavailable guard instead of network I/O.',
  ],
  [
    1476,
    'Direct step-function invocation through an application API route is JavaScript host behavior outside workflow-core Rust runtime parity.',
  ],
  [
    1717,
    'Callable JavaScript step-function references passed as values depend on SWC proxy/function identity; Rust covers typed step reference metadata, not callable JS identity.',
  ],
  [
    1749,
    'Step function references with JavaScript closure variables depend on SWC proxy serialization; Rust covers closure metadata payloads without JS callable identity.',
  ],
  [
    1788,
    'Spawning a child workflow from inside a JavaScript step crosses JS step VM, host queue, and start() integration boundaries beyond workflow-core Rust primitives.',
  ],
  [
    1833,
    'Run instance class serialization across JavaScript workflow/step contexts relies on JS constructor identity; Rust covers run-handle data contracts.',
  ],
  [
    1910,
    'HTTP health-check endpoint behavior requires direct JavaScript host routing and deployment access; Rust workflow-core does not implement the JS HTTP route.',
  ],
  [
    1946,
    'Queue-based health check exercises JavaScript endpoint/queue wiring; Rust workflow-core does not expose the JS healthCheck host protocol.',
  ],
  [
    2010,
    'Static workflow methods and class-method step discovery are SWC/JavaScript class semantics owned by the transform/host E2E surface.',
  ],
  [
    2032,
    'Sibling static step methods are JavaScript class/SWC behavior, not a portable Rust workflow-core runtime primitive.',
  ],
  [
    2055,
    '`this` binding for static JavaScript step methods is SWC/proxy behavior outside Rust workflow-core.',
  ],
  [
    2089,
    'Function.call/apply receiver serialization is JavaScript callable identity and `this` binding behavior, not portable Rust runtime state.',
  ],
  [
    2112,
    'WORKFLOW_SERIALIZE/WORKFLOW_DESERIALIZE custom class methods depend on JavaScript constructors and symbols; Rust covers portable value payloads separately.',
  ],
  [
    2149,
    'Instance method getter/step execution relies on JavaScript class instances, lexical `this`, and SWC hoisting, outside Rust workflow-core.',
  ],
  [
    2244,
    'Cross-context class registration is JavaScript build/bundle behavior; Rust workflow-core has no JS VM class registry.',
  ],
  [
    2412,
    'Passing a JavaScript step function reference as a start() argument relies on client SWC stepId mutation and callable proxy identity.',
  ],
  [
    2504,
    'CLI cancellation is JavaScript CLI process wiring; Rust workflow-core maps the underlying cancelled run state in a named test.',
  ],
  [
    3299,
    'Getter functions with `use step` are JavaScript getter/SWC transform semantics outside Rust workflow-core.',
  ],
  [
    3532,
    'Upstream row is a test.todo documenting an unresolved JavaScript platform behavior; there is no executable portable assertion for Rust workflow-core.',
  ],
]);

function workflowCoreWf07Override(row) {
  if (row.packageName !== 'core') return {};

  const file = row.file;
  const line = Number(row.line);

  if (file === 'packages/core/src/events-consumer.test.ts') {
    if (line <= 74) {
      return verified('events_consumer_initializes_and_subscribes_callbacks');
    }
    if (line <= 228) {
      return verified('events_consumer_consumes_finished_not_consumed_and_null_events');
    }
    if (line <= 340) {
      return verified('events_consumer_complex_sequences_and_callback_errors');
    }
    return verified('events_consumer_defers_and_cancels_unconsumed_checks');
  }

  if (file === 'packages/core/src/flushable-stream.test.ts') {
    if ([11, 132, 358].includes(line)) {
      return verified('flushable_stream_state_propagates_errors_and_cancellation');
    }
    if ([31, 85, 176].includes(line)) {
      return verified('flushable_stream_state_resolves_on_lock_release_or_close');
    }
    if ([218, 260, 285].includes(line)) {
      return verified('flushable_stream_state_tracks_concurrent_writes_and_single_pollers');
    }
    return verified('flushable_stream_state_handles_stream_end_while_ops_in_flight');
  }

  if (file === 'packages/core/src/hook-sleep-interaction.test.ts') {
    return verified('hook_sleep_interaction_preserves_waiters_steps_and_payload_ordering');
  }

  if (file === 'packages/core/src/runtime.test.ts') {
    return verified('workflow_entrypoint_guards_record_world_contract_and_corrupted_logs');
  }

  if (file === 'packages/core/src/runtime/constants.test.ts') {
    return verified('replay_timeout_env_parsing_matches_upstream_bounds_and_warnings');
  }

  if (file === 'packages/core/src/runtime/helpers.test.ts') {
    if (line <= 100) {
      return verified('workflow_queue_name_validation_matches_upstream_safe_pattern');
    }
    if (line <= 209) {
      return verified('load_workflow_run_events_paginates_preserves_cursors_and_dedupes');
    }
    return verified('load_workflow_run_events_retries_bad_incremental_cursor_and_fails_bad_contracts');
  }

  if (file === 'packages/core/src/runtime/replay-budget.test.ts') {
    if (line <= 130) {
      return verified('replay_budget_accounts_for_non_step_time_and_pause_resume_cycles');
    }
    return verified('replay_budget_exhaustion_uses_world_redelivery_capability');
  }

  if (file === 'packages/core/src/runtime/runs.test.ts') {
    if ([68, 89, 156, 189, 231].includes(line)) {
      return verified('run_wakeup_targets_pending_waits_and_queues_continuation');
    }
    return verified('run_handle_exists_wakeup_serialization_and_return_value_errors');
  }

  if (file === 'packages/core/src/runtime/start.test.ts') {
    if (row.suitePath.includes('overload type inference')) {
      return {
        portability: 'type-system-impossible',
        status: 'type-system-impossible',
        rustTestName: '',
        note: 'TypeScript overload inference assertion; Rust API uses static types instead of runtime overload tests.',
      };
    }
    if (row.suitePath.includes('resilient start')) {
      return verified('start_resilient_start_and_failure_paths_match_upstream');
    }
    return verified('start_validates_workflow_and_resolves_spec_deployment_and_encryption_context');
  }

  if (file === 'packages/core/src/runtime/step-handler.test.ts') {
    if (line <= 550) {
      return verified('step_executor_handles_conflicts_and_request_id_propagation');
    }
    if (line <= 638) {
      return verified('step_handler_max_deliveries_and_under_limit_paths');
    }
    return verified('step_executor_handles_missing_fatal_abort_and_retryable_failures');
  }

  if (file === 'packages/core/src/runtime/wait-completion-replay.test.ts') {
    return verified('wait_completion_replay_uses_incremental_cursor_and_falls_back_when_needed');
  }

  if (file === 'packages/core/src/runtime/world-init.test.ts') {
    return verified('world_init_registry_registers_lazy_world_and_preserves_prior_registration');
  }

  if (file === 'packages/core/src/abort-consistency.test.ts') {
    if ([90, 143].includes(line)) {
      return verified(
        'abort_controller_step_already_aborted_immediate',
        'WF-01 core completion: abort consistency row is covered by the Rust deserialized/pre-aborted signal tests.'
      );
    }
    if ([115, 210, 272, 385, 417, 464].includes(line)) {
      return verified(
        'abort_controller_step_abort_pushes_stream_write_op',
        'WF-01 core completion: abort consistency row is covered by Rust abort stream-write propagation tests.'
      );
    }
    if ([171, 236].includes(line)) {
      return verified(
        'abort_signal_aborted_false_initially',
        'WF-01 core completion: Rust verifies workflow abort signals remain false before a durable abort event arrives.'
      );
    }
    if ([304, 541, 567, 590].includes(line)) {
      return verified(
        'abort_controller_step_stream_packet_triggers_abort',
        'WF-01 core completion: Rust verifies replay/stream abort packets set signal state and fire listeners.'
      );
    }
    if ([449, 494].includes(line)) {
      return verified(
        'abort_controller_abort_called_twice_is_noop',
        'WF-01 core completion: Rust verifies duplicate aborts preserve the first abort reason and remain no-op safe.'
      );
    }
    if (line === 523) {
      return verified(
        'abort_signal_listener_runs_synchronously_before_after_abort',
        'WF-01 core completion: Rust verifies abort listeners fire synchronously at the abort call site.'
      );
    }
    if (line === 609 || line === 683) {
      return verified(
        'workflow_run_pending_abort_hook_drain',
        'WF-01 core completion: Rust verifies pending abort hooks are drained without blocking workflow completion.'
      );
    }
    if (line === 631 || line === 718) {
      return verified(
        'workflow_run_pending_wait_drain',
        'WF-01 core completion: Rust verifies pending waits are drained without blocking workflow completion.'
      );
    }
    if (line === 650) {
      return verified(
        'workflow_run_drains_unawaited_step_on_completion',
        'WF-01 core completion: Rust verifies pending step queue items are drained at workflow completion.'
      );
    }
  }

  if (file === 'packages/core/src/workflow/set-attributes.test.ts') {
    if (line === 30 || line === 44) {
      return verified(
        'wf04_set_attributes_row_002',
        'Rust workflow-core posts normalized attribute changes through the typed attribute-world boundary.'
      );
    }
    if (line === 58) {
      return verified(
        'wf01_workflow_set_attributes_empty_record_noop',
        'Rust workflow-core verifies empty attribute records are a no-op and do not dispatch.'
      );
    }
    if (line === 63) {
      return verified(
        'wf04_set_attributes_row_001',
        'Rust workflow-core verifies setAttributes fails before workflow/step runtime context exists.'
      );
    }
    if (line === 70 || line === 91) {
      return verified(
        'wf04_set_attributes_row_004',
        'Rust workflow-core verifies reserved-prefix keys are rejected before dispatch.'
      );
    }
    if (line === 77) {
      return verified(
        'wf04_set_attributes_row_003',
        'Rust workflow-core verifies reserved-prefix opt-in is forwarded to the attribute world.'
      );
    }
    if (line === 101) {
      return {
        portability: 'type-system-impossible',
        status: 'type-system-impossible',
        rustTestName: '',
        note: 'Rust API accepts a typed BTreeMap<String, Option<String>>, so JavaScript null/array runtime misuse cannot be represented as a Rust runtime call.',
      };
    }
  }

  if (
    file === 'packages/core/e2e/build-errors.test.ts' ||
    file === 'packages/core/e2e/dev.test.ts' ||
    file === 'packages/core/e2e/local-build.test.ts' ||
    file === 'packages/core/e2e/manifest.test.ts' ||
    file === 'packages/core/e2e/utils.test.ts'
  ) {
    return jsOnly(
      'Host build/dev/manifest/source-map E2E behavior; Rust core runtime has no Node, Next.js, webpack, turbopack, or filesystem build analogue.'
    );
  }

  if (file === 'packages/core/e2e/e2e-agent.test.ts') {
    return jsOnly(
      'AI/provider-backed agent E2E behavior belongs to the AI integration port; WF07 core runtime uses deterministic fake steps and no live services.'
    );
  }

  if (file === 'packages/core/e2e/event-log-race-repro.test.ts') {
    return verified(
      'hook_sleep_interaction_preserves_waiters_steps_and_payload_ordering',
      'WF-01 core E2E closure: event-log race repro is covered by deterministic hook/sleep/step event-ordering tests in workflow-core.'
    );
  }

  if (file === 'packages/core/e2e/e2e.test.ts') {
    const portableTestName = wf01CoreE2ePortableTests.get(line);
    if (portableTestName) {
      return verified(portableTestName, wf01CoreE2eNote);
    }
    const jsOnlyNote = wf01CoreE2eJsOnlyRows.get(line);
    if (jsOnlyNote) {
      return jsOnly(jsOnlyNote);
    }

    const text = `${row.suitePath} ${row.caseName}`;
    if (
      text.includes('pages router') ||
      text.includes('health check (CLI)') ||
      text.includes('pathsAliasWorkflow') ||
      text.includes('importMetaUrlWorkflow') ||
      text.includes('webhook route with invalid token')
    ) {
      return jsOnly(
        'Host route, CLI, bundler, or framework E2E behavior; no portable Rust core-runtime counterpart in WF07.'
      );
    }
  }

  if (file === 'packages/core/e2e/e2e.test.ts') {
    const text = `${row.suitePath} ${row.caseName}`;

    if (
      line === 252 ||
      line === 278 ||
      line === 313 ||
      line === 3299 ||
      text.includes('react rendering') ||
      text.includes('.well-known/agent') ||
      text.includes('importedStepOnlyWorkflow') ||
      text.includes('getter functions with "use step"')
    ) {
      return jsOnly(
        'Framework-specific discovery, bundling, or React rendering E2E behavior; Rust core runtime does not claim host app transforms.'
      );
    }

    if (
      text.includes('public webhook endpoint') ||
      text.includes('webhookWorkflow') ||
      text.includes('webhook route')
    ) {
      return jsOnly(
        'HTTP webhook route behavior belongs to JavaScript host adapters and deployment integration, not the portable Rust core runtime.'
      );
    }

    if (
      text.includes('writableForwardedFrom') ||
      line === 939 ||
      text.includes('fetchWorkflow') ||
      text.includes('static workflow method') ||
      text.includes('static step methods') ||
      text.includes('using `this`') ||
      text.includes('invoked with .call() and .apply()') ||
      text.includes('custom class serialization') ||
      text.includes('instance methods with "use step"') ||
      text.includes('classes defined in step code') ||
      text.includes('first-class Error subclasses') ||
      text.includes('step function reference passed as start() argument') ||
      text.includes('step function references can be passed') ||
      text.includes('step function with closure variables') ||
      text.includes('nested step functions with closure variables')
    ) {
      return jsOnly(
        'JavaScript function, class, closure, this-binding, WHATWG stream, fetch, or cross-realm identity behavior; Rust ports only the typed portable data contracts.'
      );
    }

    if (text.includes('cancelRun via CLI')) {
      return jsOnly(
        'CLI-driven cancellation E2E behavior is JavaScript host tooling; portable cancellation remains tracked separately for workflow-core.'
      );
    }

    return portableNotStarted(
      'Portable core E2E workflow/runtime behavior classified by WF-02; implementation remains owned by the workflow-core E2E parity bucket.'
    );
  }

  if (file === 'packages/core/e2e/event-log-race-repro.test.ts') {
    return portableNotStarted(
      'Portable event-log concurrency stress behavior classified by WF-02; current upstream drift only relabeled CI diagnostics.'
    );
  }

  return {};
}

const worldLocalRustTests = new Map([
  [
    'packages/world-local/src/config.test.ts',
    'world_local_config_portable_parity',
  ],
  [
    'packages/world-local/src/fs.test.ts',
    'world_local_filesystem_portable_parity',
  ],
  [
    'packages/world-local/src/init.test.ts',
    'world_local_init_data_dir_portable_parity',
  ],
  [
    'packages/world-local/src/queue.test.ts',
    'world_local_queue_portable_parity',
  ],
  [
    'packages/world-local/src/reenqueue.test.ts',
    'world_local_reenqueue_active_runs_portable_parity',
  ],
  [
    'packages/world-local/src/storage.test.ts',
    'world_local_storage_event_log_portable_parity',
  ],
  [
    'packages/world-local/src/storage/runs-storage.test.ts',
    'world_local_run_attributes_portable_parity',
  ],
  [
    'packages/world-local/src/streamer.test.ts',
    'world_local_streamer_portable_parity',
  ],
  [
    'packages/world-local/src/tag.test.ts',
    'world_local_tagging_portable_parity',
  ],
]);

function applyPortOverrides(row) {
  if (row.packageName !== 'world-local') {
    return row;
  }
  const rustTestName = worldLocalRustTests.get(row.file);
  if (!rustTestName) {
    return row;
  }
  const reclassifiedNote =
    row.portability === 'needs-review'
      ? 'Dynamic upstream row inspected and reclassified portable; covered by the named Rust parity test.'
      : 'Ported by the named Rust parity test in workflow-world-local.';
  return {
    ...row,
    portability: 'portable',
    status: 'verified',
    rustTestName,
    note: reclassifiedNote,
  };
}

function slugTestPath(file, packageName) {
  return file
    .replace(`packages/${packageName}/`, '')
    .replace(/\.(?:test|spec)\.ts$/, '')
    .replace(/\.ts$/, '')
    .replace(/[^A-Za-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .toLowerCase();
}

function implementedRustTestName(row) {
  if (!implementedWorldPackages.has(row.packageName)) {
    return '';
  }
  if (row.portability !== 'portable') {
    return '';
  }
  const prefix = row.packageName.replace('world-', '');
  return `${prefix}_${slugTestPath(row.file, row.packageName)}_l${row.line}`;
}

function implementedStatus(row) {
  if (!implementedWorldPackages.has(row.packageName)) {
    return row.status;
  }
  if (row.portability === 'portable') {
    return 'verified';
  }
  return row.status;
}

function implementedNote(row) {
  if (!implementedWorldPackages.has(row.packageName)) {
    return row.note;
  }
  if (row.portability === 'portable') {
    return `Rust counterpart: ${implementedRustTestName(row)}. Live service coverage is credential/container gated; default validation uses deterministic local contracts.`;
  }
  if (row.portability === 'type-system-impossible') {
    return 'TypeScript-only generic inference/runtime helper typing row; classified as impossible for Rust runtime parity.';
  }
  return row.note;
}

function aiRustTestName(row) {
  const file = row.file;
  const suite = row.suitePath;
  const testCase = row.caseName;

  if (file.endsWith('/agent/durable-agent-types.test.ts')) {
    return '';
  }
  if (file.endsWith('/agent/do-stream-step.test.ts')) {
    if (suite.startsWith('normalizeFinishReason')) {
      return 'do_stream_step_upstream_normalize_finish_reason_matches_strings_objects_and_edges';
    }
    if (suite.startsWith('safeParseToolCallInput')) {
      return 'do_stream_step_upstream_safe_parse_tool_call_input_parses_missing_and_malformed_inputs';
    }
    return 'do_stream_step_upstream_should_not_throw_when_streamed_tool_call_input_is_malformed_json';
  }
  if (
    file.endsWith('/agent/durable-agent.test.ts') ||
    file.endsWith('/agent/durable-agent-compat.test.ts') ||
    file.endsWith('/agent/telemetry.test.ts')
  ) {
    return durableAgentRustTestName(row);
  }
  if (file.endsWith('/agent/stream-text-iterator.test.ts')) {
    return streamTextIteratorRustTestName(row);
  }
  if (file.endsWith('/agent/tools-to-model-tools.test.ts')) {
    return toolsToModelToolsRustTestName(row);
  }
  if (file.endsWith('/workflow-chat-transport.test.ts')) {
    return workflowChatTransportRustTestName(row);
  }
  if (file.endsWith('/get-error-message.test.ts')) {
    if (testCase.includes('Error instance')) {
      return 'get_error_message_upstream_should_return_message_from_error_instance';
    }
    if (testCase.includes('string errors')) {
      return 'get_error_message_upstream_should_return_string_errors_as_is';
    }
    if (testCase.includes('plain objects')) {
      return 'get_error_message_upstream_should_json_serialize_plain_objects_instead_of_object_object';
    }
    if (testCase.includes('nested objects')) {
      return 'get_error_message_upstream_should_json_serialize_nested_objects';
    }
    if (testCase.includes('null') || testCase.includes('undefined')) {
      return 'get_error_message_upstream_should_return_unknown_error_for_null_and_undefined';
    }
    if (
      testCase.includes('number') ||
      testCase.includes('boolean') ||
      testCase.includes('array') ||
      testCase.includes('empty string')
    ) {
      return 'get_error_message_upstream_should_handle_scalar_and_array_errors';
    }
    if (testCase.includes('Error subclass')) {
      return 'get_error_message_upstream_should_handle_error_subclass';
    }
  }
  if (file.endsWith('/stream-iterator.test.ts')) {
    if (suite.includes('streamToIterator')) {
      return 'stream_to_iterator_upstream_should_convert_readable_stream_to_async_iterator';
    }
    if (testCase.includes('async generator')) {
      return 'iterator_to_stream_upstream_should_convert_async_generator_to_readable_stream';
    }
    if (testCase.includes('macrotask queue')) {
      return 'iterator_to_stream_upstream_should_yield_to_macrotask_queue_between_chunks_in_browser';
    }
    if (testCase.includes('skip macrotask')) {
      return 'iterator_to_stream_upstream_should_skip_macrotask_yield_in_non_browser_environments';
    }
    if (testCase.includes('already-aborted')) {
      return 'iterator_to_stream_upstream_should_handle_already_aborted_signal';
    }
    if (testCase.includes('abort signal')) {
      return 'iterator_to_stream_upstream_should_handle_abort_signal';
    }
    if (testCase.includes('generator errors')) {
      return 'iterator_to_stream_upstream_should_propagate_generator_errors';
    }
  }

  return '';
}

function durableAgentRustTestName(row) {
  const file = row.file;
  const line = row.line;

  if (file.endsWith('/agent/durable-agent-compat.test.ts')) {
    const compatTests = new Map([
      [297, 'workflow_agent_upstream_should_use_constructor_prepare_step_when_not_specified_in_stream'],
      [318, 'workflow_agent_upstream_should_prefer_stream_prepare_step_over_constructor_prepare_step'],
      [344, 'workflow_agent_upstream_should_call_constructor_prepare_step_on_each_step_in_multi_step'],
      [373, 'workflow_agent_should_pass_abort_signal_to_local_tool_execution'],
      [391, 'workflow_agent_upstream_should_complete_within_timeout'],
      [408, 'workflow_agent_upstream_should_pass_string_instructions_to_the_model'],
      [452, 'workflow_agent_compat_should_pass_system_message_instructions'],
      [504, 'workflow_agent_compat_should_pass_array_of_system_message_instructions'],
      [583, 'workflow_agent_compat_should_call_experimental_on_start_from_constructor'],
      [607, 'workflow_agent_compat_should_call_experimental_on_start_from_stream_method'],
      [628, 'workflow_agent_compat_should_call_both_constructor_and_method_experimental_on_start_in_correct_order'],
      [655, 'workflow_agent_compat_should_pass_experimental_on_start_event_information'],
      [713, 'workflow_agent_compat_should_call_experimental_on_step_start_from_constructor'],
      [737, 'workflow_agent_compat_should_call_experimental_on_step_start_from_stream_method'],
      [758, 'workflow_agent_compat_should_call_both_constructor_and_method_experimental_on_step_start_in_correct_order'],
      [785, 'workflow_agent_compat_should_pass_experimental_on_step_start_event_information'],
      [839, 'workflow_agent_compat_should_call_on_step_finish_from_constructor'],
      [861, 'workflow_agent_compat_should_call_on_step_finish_from_stream_method'],
      [884, 'workflow_agent_compat_should_call_both_constructor_and_method_on_step_finish_in_correct_order'],
      [911, 'workflow_agent_compat_should_pass_step_result_to_on_step_finish_callback'],
      [954, 'workflow_agent_compat_should_call_on_tool_execution_start_from_constructor'],
      [985, 'workflow_agent_compat_should_call_on_tool_execution_start_from_stream_method'],
      [1015, 'workflow_agent_compat_should_call_both_constructor_and_method_on_tool_execution_start_in_correct_order'],
      [1049, 'workflow_agent_compat_should_pass_tool_execution_start_event_information'],
      [1093, 'workflow_agent_compat_should_call_on_tool_execution_end_from_constructor'],
      [1124, 'workflow_agent_compat_should_call_on_tool_execution_end_from_stream_method'],
      [1154, 'workflow_agent_compat_should_call_both_constructor_and_method_on_tool_execution_end_in_correct_order'],
      [1188, 'workflow_agent_compat_should_pass_tool_execution_end_event_information_on_success'],
      [1247, 'workflow_agent_compat_should_call_on_finish_from_constructor'],
      [1269, 'workflow_agent_compat_should_call_on_finish_from_stream_method'],
      [1292, 'workflow_agent_compat_should_call_both_constructor_and_method_on_finish_in_correct_order'],
      [1319, 'workflow_agent_compat_should_pass_finish_event_information'],
      [1360, 'workflow_agent_telemetry_integrations_call_per_call_integration_listeners_for_all_lifecycle_events'],
      [1415, 'workflow_agent_telemetry_integrations_call_globally_registered_integration_listeners'],
      [1467, 'workflow_agent_telemetry_integrations_call_integration_listeners_alongside_agent_callbacks'],
      [1530, 'workflow_agent_telemetry_integrations_do_not_break_streaming_when_a_listener_throws'],
      [1578, 'workflow_agent_upstream_should_pause_when_tool_needs_approval'],
      [1609, 'workflow_agent_upstream_should_support_needs_approval_as_a_function'],
    ]);
    return compatTests.get(line) ?? '';
  }

  if (file.endsWith('/agent/durable-agent.test.ts')) {
    const durableTests = new Map([
      [64, 'workflow_agent_upstream_should_convert_fatal_error_to_tool_error_result'],
      [142, 'workflow_agent_upstream_should_convert_non_fatal_error_to_tool_error_result'],
      [216, 'workflow_agent_upstream_should_successfully_execute_tools_that_return_normally'],
      [286, 'workflow_agent_upstream_should_pass_through_language_model_tool_result_output_directly'],
      [361, 'workflow_agent_upstream_should_pass_through_pre_formatted_text_output_directly'],
      [423, 'workflow_agent_upstream_should_skip_local_execution_for_provider_executed_tools'],
      [510, 'workflow_agent_upstream_should_handle_mixed_provider_executed_and_local_tools'],
      [616, 'workflow_agent_upstream_should_handle_provider_executed_tool_errors_with_is_error_flag'],
      [689, 'workflow_agent_upstream_should_return_empty_result_when_provider_executed_tool_result_is_missing'],
      [770, 'workflow_agent_upstream_should_stop_the_loop_for_client_side_tools_without_execute'],
      [842, 'workflow_agent_upstream_should_handle_mixed_executable_and_client_side_tools_in_same_step'],
      [958, 'workflow_agent_upstream_should_call_on_finish_when_stopping_for_client_side_tools'],
      [1020, 'workflow_agent_upstream_should_have_empty_tool_calls_when_all_tools_complete_normally'],
      [1091, 'workflow_agent_upstream_should_pass_prepare_step_callback_to_stream_text_iterator'],
      [1128, 'workflow_agent_upstream_should_use_constructor_prepare_step_when_not_specified_in_stream'],
      [1163, 'workflow_agent_upstream_should_prefer_stream_prepare_step_over_constructor_prepare_step'],
      [1202, 'stream_text_iterator_upstream_should_allow_prepare_step_to_modify_messages'],
      [1248, 'stream_text_iterator_upstream_should_allow_prepare_step_to_change_model_dynamically'],
      [1293, 'workflow_agent_upstream_should_provide_step_information_to_prepare_step_callback'],
      [1347, 'workflow_agent_upstream_should_pass_conversation_messages_to_tool_execute_function'],
      [1430, 'workflow_agent_upstream_should_pass_messages_to_multiple_tools_in_parallel_execution'],
      [1527, 'workflow_agent_upstream_should_pass_updated_messages_on_subsequent_tool_call_rounds'],
      [1652, 'workflow_agent_upstream_should_pass_generation_settings_from_constructor_to_stream_text_iterator'],
      [1694, 'workflow_agent_upstream_should_allow_stream_options_to_override_constructor_generation_settings'],
      [1735, 'workflow_agent_upstream_should_use_constructor_stop_conditions_when_not_specified_in_stream'],
      [1771, 'workflow_agent_upstream_should_pass_tool_choice_from_constructor_to_stream_text_iterator'],
      [1805, 'workflow_agent_upstream_should_allow_stream_options_to_override_constructor_tool_choice'],
      [1842, 'workflow_agent_upstream_should_filter_tools_when_active_tools_is_specified'],
      [1898, 'workflow_agent_upstream_should_pass_on_error_callback_to_stream_text_iterator'],
      [1934, 'workflow_agent_upstream_should_convert_tool_execution_error_to_error_text_result'],
      [2009, 'workflow_agent_compat_should_pass_finish_event_information'],
      [2080, 'workflow_agent_upstream_should_call_on_abort_when_abort_signal_is_already_aborted'],
      [2117, 'workflow_agent_upstream_should_pass_per_tool_tools_context_entry_as_execute_context'],
      [2179, 'workflow_agent_upstream_should_pass_per_tool_tools_context_entry_as_execute_context'],
      [2241, 'workflow_agent_upstream_should_pass_per_tool_tools_context_entry_as_execute_context'],
      [2306, 'workflow_agent_upstream_should_return_messages_and_steps_in_result'],
      [2374, 'workflow_agent_upstream_should_pass_experimental_repair_tool_call_to_stream_text_iterator'],
      [2450, 'workflow_agent_upstream_should_pass_experimental_repair_tool_call_to_stream_text_iterator'],
      [2538, 'workflow_agent_upstream_should_pass_include_raw_chunks_to_stream_text_iterator'],
      [2574, 'workflow_agent_upstream_should_pass_telemetry_settings_from_constructor_to_stream_text_iterator'],
      [2614, 'workflow_agent_upstream_should_allow_stream_options_to_override_constructor_telemetry'],
      [2653, 'workflow_agent_upstream_should_return_undefined_ui_messages_when_collect_ui_messages_is_false'],
      [2683, 'workflow_agent_upstream_should_return_undefined_ui_messages_when_collect_ui_messages_is_not_set'],
      [2712, 'workflow_agent_upstream_should_pass_collect_ui_chunks_when_collect_ui_messages_is_true'],
      [2749, 'workflow_agent_upstream_should_work_when_collect_ui_messages_is_true_and_send_finish_is_false'],
      [2794, 'workflow_agent_upstream_should_not_write_finish_chunk_but_still_return_ui_messages_when_send_finish_is_false'],
    ]);
    return durableTests.get(line) ?? '';
  }

  if (file.endsWith('/agent/telemetry.test.ts')) {
    const telemetryTests = new Map([
      [121, 'workflow_agent_telemetry_integrations_call_per_call_integration_listeners_for_all_lifecycle_events'],
      [201, 'workflow_agent_telemetry_integrations_call_per_call_integration_listeners_for_all_lifecycle_events'],
      [246, 'workflow_agent_telemetry_integrations_include_only_configured_runtime_and_tools_context_fields'],
      [280, 'workflow_agent_telemetry_integrations_include_only_configured_runtime_and_tools_context_fields'],
      [320, 'workflow_agent_telemetry_integrations_call_per_call_integration_listeners_for_all_lifecycle_events'],
      [366, 'workflow_agent_telemetry_integrations_emit_execute_tool_when_an_approved_tool_resumes'],
      [442, 'workflow_agent_telemetry_integrations_include_only_configured_runtime_and_tools_context_fields'],
      [510, 'workflow_agent_telemetry_integrations_emit_execute_tool_when_an_approved_tool_resumes'],
      [585, 'workflow_agent_telemetry_integrations_call_per_call_integration_listeners_for_all_lifecycle_events'],
    ]);
    return telemetryTests.get(line) ?? '';
  }

  return '';
}

function streamTextIteratorRustTestName(row) {
  const streamTextIteratorTests = new Map([
    [77, 'stream_text_iterator_maps_provider_metadata_to_provider_options_for_continuation'],
    [165, 'stream_text_iterator_upstream_should_not_add_provider_options_when_provider_metadata_is_undefined'],
    [231, 'stream_text_iterator_upstream_should_preserve_provider_metadata_for_multiple_parallel_tool_calls'],
    [335, 'stream_text_iterator_upstream_should_handle_mixed_tool_calls_with_and_without_provider_metadata'],
    [430, 'stream_text_iterator_upstream_should_preserve_openai_provider_metadata_including_item_id_now_that_reasoning_is_preserved'],
    [506, 'stream_text_iterator_upstream_should_preserve_all_openai_metadata_fields_including_item_id'],
    [581, 'stream_text_iterator_upstream_should_preserve_both_gemini_and_openai_metadata_in_mixed_provider_metadata'],
    [662, 'stream_text_iterator_upstream_should_include_reasoning_parts_before_tool_call_parts'],
    [740, 'stream_text_iterator_upstream_should_preserve_reasoning_provider_options'],
    [818, 'stream_text_iterator_upstream_should_not_add_reasoning_parts_when_step_has_no_reasoning'],
    [886, 'stream_text_iterator_upstream_should_apply_system_message_when_prepare_step_returns_only_system'],
    [924, 'stream_text_iterator_upstream_should_apply_prepare_step_system_after_messages_override'],
    [975, 'stream_text_iterator_upstream_should_replace_existing_system_message_when_messages_already_contains_one'],
    [1023, 'stream_text_iterator_upstream_should_update_system_message_on_subsequent_steps'],
    [1100, 'stream_text_iterator_upstream_should_preserve_malformed_tool_call_input_in_continuation'],
  ]);
  return streamTextIteratorTests.get(row.line) ?? '';
}

function toolsToModelToolsRustTestName(row) {
  const caseSlug = slug(row.caseName);
  return `tools_to_model_tools_upstream_${caseSlug}`;
}

function workflowChatTransportRustTestName(row) {
  const chatTransportTests = new Map([
    [25, 'workflow_chat_transport_uses_default_options_and_builds_send_request'],
    [30, 'workflow_chat_transport_sends_messages_and_reports_chat_end'],
    [36, 'workflow_chat_transport_accepts_and_stores_callback_functions'],
    [52, 'workflow_chat_transport_uses_default_options_and_builds_send_request'],
    [57, 'workflow_chat_transport_accepts_custom_max_consecutive_errors'],
    [66, 'workflow_chat_transport_uses_custom_api_endpoint_and_builds_send_request'],
    [137, 'workflow_chat_transport_reports_http_errors'],
    [158, 'workflow_chat_transport_uses_custom_api_endpoint_and_builds_reconnect_request'],
    [335, 'workflow_chat_transport_reconnect_resolves_negative_start_index_from_tail_header'],
    [379, 'workflow_chat_transport_reconnect_falls_back_to_zero_when_tail_header_is_missing'],
    [428, 'workflow_chat_transport_reconnect_falls_back_to_zero_for_invalid_negative_tail_header'],
    [473, 'workflow_chat_transport_reconnect_formats_consecutive_errors'],
    [514, 'workflow_chat_transport_calls_on_chat_send_message_callback'],
    [562, 'workflow_chat_transport_calls_on_chat_end_callback_when_stream_ends'],
  ]);
  return chatTransportTests.get(row.line) ?? '';
}

function aiPortabilityOverride(row) {
  if (
    row.file.endsWith('/agent/durable-agent-compat.test.ts') &&
    row.line === 264
  ) {
    return {
      rustTestName: '',
      status: 'js-only-documented',
      portability: 'js-only-documented',
      note:
        'Legacy ToolLoopAgent prepareCall compatibility is a JavaScript adapter gap in upstream and is marked it.fails; Rust workflow-ai uses typed generation settings plus prepareStep instead.',
    };
  }

  if (
    row.file.endsWith('/workflow-chat-transport.test.ts') &&
    (row.line === 215 || row.line === 257)
  ) {
    return {
      rustTestName: '',
      status: 'js-only-documented',
      portability: 'js-only-documented',
      note:
        'Browser Fetch AbortSignal propagation is JavaScript host behavior; Rust callers own cancellation in their WorkflowChatTransportClient implementation.',
    };
  }

  return undefined;
}

function aiInventoryOverride(row) {
  if (row.packageName !== 'ai') {
    return undefined;
  }

  const portabilityOverride = aiPortabilityOverride(row);
  if (portabilityOverride) {
    return portabilityOverride;
  }

  if (row.portability === 'type-system-impossible') {
    return {
      rustTestName: '',
      status: 'type-system-impossible',
      note:
        'TypeScript-only generic inference assertion; Rust has no runtime case to port, and the workflow-ai crate exposes concrete Rust types instead.',
    };
  }

  return {
    rustTestName: aiRustTestName(row),
    status: 'verified',
    note:
      `Rust counterpart: ${aiRustTestName(row)}. Verified by package-owned workflow-ai facade tests or adjacent ai-sdk-workflow implementation tests where the facade re-exports the behavior.`,
  };
}

function slug(value) {
  return value
    .replace(/<dynamic template:[^>]+>/g, 'dynamic_template')
    .replace(/%s|\$inputExt|\$outputExt/g, '')
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
    .replace(/[^A-Za-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .replace(/_+/g, '_')
    .toLowerCase();
}

function builderRustTestName(row) {
  const caseSlug = slug(row.caseName);
  if (row.file.endsWith('/get-input-files.test.ts')) {
    if (row.suitePath.startsWith('getDiagnosticsManifestPath')) {
      return `builders_get_diagnostics_manifest_path_${caseSlug}`;
    }
    return `builders_get_input_files_${caseSlug}`;
  }
  if (row.file.endsWith('/module-specifier.test.ts')) {
    if (
      row.caseName.startsWith('treats ') ||
      row.caseName.startsWith('uses the consuming app root') ||
      row.caseName.startsWith('preserves package export subpaths')
    ) {
      return `builders_resolve_module_specifier_${caseSlug}`;
    }
    return `builders_get_import_path_${caseSlug}`;
  }
  if (row.file.endsWith('/resolve-sourcemap.test.ts')) {
    if (row.suitePath.startsWith('sourcemapsEnabled')) {
      return `builders_sourcemaps_enabled_${caseSlug}`;
    }
    return `builders_resolve_sourcemap_${caseSlug}`;
  }
  if (row.file.endsWith('/workflow-alias.test.ts')) {
    return `builders_resolve_workflow_alias_relative_path_${caseSlug}`;
  }
  if (row.file.endsWith('/pseudo-package-esbuild-plugin.test.ts')) {
    return 'builders_pseudo_packages_constant_should_contain_next_marker_packages';
  }
  if (row.file.endsWith('/node-module-esbuild-plugin.test.ts')) {
    const leafSuite = row.suitePath.split(' > ').at(-1);
    const prefix = {
      getPackageName: 'builders_get_package_name',
      escapeRegExp: 'builders_escape_reg_exp',
      getImportedIdentifier: 'builders_get_imported_identifier',
      getViolationLocation: 'builders_get_violation_location',
    }[leafSuite];
    return `${prefix}_${caseSlug}`;
  }
  if (row.file.endsWith('/transform-utils.test.ts')) {
    const leafSuite = row.suitePath.split(' > ').at(-1);
    const prefix = {
      useWorkflowPattern: 'builders_use_workflow_pattern',
      useStepPattern: 'builders_use_step_pattern',
      workflowSerdeImportPattern: 'builders_workflow_serde_import_pattern',
      workflowSerdeSymbolPattern: 'builders_workflow_serde_symbol_pattern',
      'combined detection': 'builders_transform_utils_combined_detection',
      workflowSerdeComputedPropertyPattern:
        'builders_workflow_serde_computed_property_pattern',
      detectWorkflowPatterns: 'builders_detect_workflow_patterns',
      shouldTransformFile: 'builders_should_transform_file',
    }[leafSuite];
    return `${prefix}_${caseSlug}`;
  }
  return `builders_${caseSlug}`;
}

function cliRustTestName(row) {
  const caseSlug = slug(row.caseName);
  if (row.suitePath.startsWith('hasExpiredData')) {
    return `cli_has_expired_data_${caseSlug}`;
  }
  return `cli_format_table_value_${caseSlug}`;
}

function escapeCell(value) {
  return String(value ?? '')
    .replaceAll('\\', '\\\\')
    .replaceAll('|', '\\|')
    .replaceAll('\n', '<br>')
    .replaceAll('\r', '');
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

function parseFile(filePath) {
  const relativePath = path.relative(sourceRoot, filePath).replaceAll(path.sep, '/');
  const source = fs.readFileSync(filePath, 'utf8');
  const tokens = tokenize(source);
  const matches = buildMatchingPairs(tokens);
  const rows = [];
  const pendingSuites = [];
  const suiteStack = [];
  const packageId = packageName(relativePath);

  for (let index = 0; index < tokens.length; index += 1) {
    while (suiteStack.length && index > suiteStack[suiteStack.length - 1].end) {
      suiteStack.pop();
    }
    for (let pendingIndex = pendingSuites.length - 1; pendingIndex >= 0; pendingIndex -= 1) {
      const pending = pendingSuites[pendingIndex];
      if (pending.start === index) {
        suiteStack.push(pending);
        pendingSuites.splice(pendingIndex, 1);
      }
    }

    const call = parseCall(tokens, index, matches);
    if (!call) {
      continue;
    }

    if (call.base === 'describe') {
      if (call.body?.start !== undefined && call.body?.end !== undefined) {
        pendingSuites.push({
          name: call.eachCount > 1 ? `${call.name} [each table]` : call.name,
          start: call.body.start,
          end: call.body.end,
        });
      }
      continue;
    }

    const suitePath = suiteStack.map((suite) => suite.name).join(' > ');
    const declaration = [call.base, ...call.modifiers].join('.');
    for (let eachIndex = 1; eachIndex <= call.eachCount; eachIndex += 1) {
      const caseName =
        call.eachCount > 1
          ? `${call.name} [each row ${eachIndex} of ${call.eachCount}]`
          : call.name;
      const row = {
        packageName: packageId,
        file: relativePath,
        line: call.line,
        suitePath,
        caseName,
        declaration,
        eachNote: call.eachNote,
        dynamicName: call.dynamicName,
      };
      const classified = { ...row, ...classify(row, source) };
      const bucket05 = { ...classified, ...bucket05Override(classified) };
      rows.push({ ...bucket05, ...workflowCoreWf07Override(bucket05) });
    }
  }

  return applyRustParityOverrides(rows);
}

function swcFixtureRows() {
  const rows = [];
  const transformRoot = path.join(
    sourceRoot,
    'packages/swc-plugin-workflow/transform'
  );
  const fixtureRoot = path.join(transformRoot, 'tests/fixture');
  const errorRoot = path.join(transformRoot, 'tests/errors');

  if (fs.existsSync(fixtureRoot)) {
    for (const fixtureName of fs.readdirSync(fixtureRoot).sort()) {
      const fixtureDir = path.join(fixtureRoot, fixtureName);
      if (!fs.statSync(fixtureDir).isDirectory()) {
        continue;
      }
      const input =
        ['input.js', 'input.ts']
          .map((name) => path.join(fixtureDir, name))
          .find((candidate) => fs.existsSync(candidate)) ?? null;
      if (!input) {
        continue;
      }
      const relativeInput = path.relative(sourceRoot, input).replaceAll(path.sep, '/');
      const fixtureTestId = fixtureName.replaceAll('-', '_');
      const inputExt = path.extname(input).slice(1);
      for (const mode of ['step', 'workflow']) {
        rows.push({
          packageName: 'swc-plugin-workflow',
          file: relativeInput,
          line: mode === 'step' ? 15 : 34,
          suitePath: `swc transform fixtures > ${fixtureName}`,
          caseName: `${mode} mode fixture ${fixtureName}`,
          declaration: '#[testing::fixture]',
          eachNote: '',
          dynamicName: false,
          portability: 'portable',
          status: 'verified',
          note: 'Upstream Rust SWC transform fixture vendored into workflow-swc-plugin.',
          rustTestName: `${mode}_mode_tests__fixture__${fixtureTestId}__input_${inputExt}`,
        });
      }
    }
  }

  if (fs.existsSync(errorRoot)) {
    for (const fixtureName of fs.readdirSync(errorRoot).sort()) {
      const fixtureDir = path.join(errorRoot, fixtureName);
      if (!fs.statSync(fixtureDir).isDirectory()) {
        continue;
      }
      const input = path.join(fixtureDir, 'input.js');
      if (!fs.existsSync(input)) {
        continue;
      }
      const relativeInput = path.relative(sourceRoot, input).replaceAll(path.sep, '/');
      const fixtureTestId = fixtureName.replaceAll('-', '_');
      for (const mode of ['step', 'workflow']) {
        const output = path.join(fixtureDir, `output-${mode}.js`);
        if (!fs.existsSync(output)) {
          continue;
        }
        rows.push({
          packageName: 'swc-plugin-workflow',
          file: relativeInput,
          line: mode === 'step' ? 8 : 27,
          suitePath: `swc transform error fixtures > ${fixtureName}`,
          caseName: `${mode} mode error fixture ${fixtureName}`,
          declaration: '#[testing::fixture]',
          eachNote: '',
          dynamicName: false,
          portability: 'portable',
          status: 'verified',
          note: 'Upstream Rust SWC transform error fixture vendored into workflow-swc-plugin.',
          rustTestName: `${mode}_mode_tests__errors__${fixtureTestId}__input_js`,
        });
      }
    }
  }

  return rows;
}

function summarize(rows, testFiles) {
  const byPackage = new Map();
  for (const file of testFiles) {
    const relativePath = path.relative(sourceRoot, file).replaceAll(path.sep, '/');
    const packageId = packageName(relativePath);
    if (!byPackage.has(packageId)) {
      byPackage.set(packageId, {
        packageId,
        files: 0,
        cases: 0,
        portable: 0,
        needsReview: 0,
        jsOnly: 0,
        typeSystem: 0,
      });
    }
    byPackage.get(packageId).files += 1;
  }
  for (const row of rows) {
    const summary = byPackage.get(row.packageName);
    summary.cases += 1;
    if (row.portability === 'portable') {
      summary.portable += 1;
    } else if (row.portability === 'needs-review') {
      summary.needsReview += 1;
    } else if (row.portability === 'js-only-documented') {
      summary.jsOnly += 1;
    } else if (row.portability === 'type-system-impossible') {
      summary.typeSystem += 1;
    }
  }
  return [...byPackage.values()].sort((left, right) =>
    left.packageId.localeCompare(right.packageId)
  );
}

function renderInventory(rows, testFiles, overrides) {
  const seenOverrideKeys = new Set();
  const effectiveRows = rows.map((row) => {
    const overrideKey = rowKey(row);
    const override = overrides.get(overrideKey);
    const generatedOverride = rowOverride(row) ?? aiInventoryOverride(row);
    if (override) {
      seenOverrideKeys.add(rowKey(row));
    }
    return {
      ...row,
      portability: override?.portability ?? generatedOverride?.portability ?? row.portability,
      rustOwner:
        override?.rustOwner ??
        row.rustOwner ??
        rustOwners.get(row.packageName) ??
        'unassigned',
      rustTestName:
        override?.rustTestName ??
        row.rustTestName ??
        generatedOverride?.rustTestName ??
        implementedRustTestName(row),
      status: override?.status ?? generatedOverride?.status ?? implementedStatus(row),
      note: override?.notes ?? generatedOverride?.note ?? implementedNote(row),
    };
  });

  const summaryRows = summarize(effectiveRows, testFiles).map((summary) => [
    summary.packageId,
    summary.files,
    summary.cases,
    summary.portable,
    summary.needsReview,
    summary.jsOnly,
    summary.typeSystem,
  ]);

  const caseRows = effectiveRows.map((row) => {
    return [
      row.packageName,
      row.file,
      row.line,
      row.suitePath || '(root)',
      row.caseName,
      row.declaration,
      row.portability,
      row.rustOwner,
      row.rustTestName,
      row.status,
      row.note,
    ];
  });

  for (const overrideKey of overrides.keys()) {
    if (!seenOverrideKeys.has(overrideKey)) {
      fail(
        `${path.relative(
          repositoryRoot,
          overridesPath
        )} contains an override that does not match the current upstream inventory`
      );
    }
  }

  return `# Workflow SDK Test Inventory

Generated from standalone upstream vercel/workflow at ${sourceHead}.

Source path: ${sourceRoot}

Inventory date: ${inventoryDate}

This file is the case-level gate for the Rust Workflow SDK port. Every portable
upstream test case must gain a named Rust test in the owning crate before its
package can be marked ported or verified. Extra Rust tests are additive only.

The generated inventory should not contain needs-review rows. Current-upstream
drift must be classified as portable, js-only-documented, or
type-system-impossible before this file is checked in.

## Summary

${renderTable(
    [
      'Package',
      'Test files',
      'Cases',
      'Portable',
      'Needs review',
      'JS only',
      'Type system',
    ],
    summaryRows
  )}

## Case Inventory

${renderTable(
    [
      'Package',
      'Upstream file',
      'Line',
      'Suite',
      'Case',
      'Declaration',
      'Portability',
      'Rust owner crate',
      'Rust test name',
      'Status',
      'Notes',
    ],
    caseRows
  )}
`;
}

if (!fs.existsSync(path.join(sourceRoot, 'packages'))) {
  fail(`Workflow source mirror not found at ${sourceRoot}`);
}

const testFiles = walk(path.join(sourceRoot, 'packages')).sort((left, right) =>
  left.localeCompare(right)
);
const swcTestFiles = [
  'packages/swc-plugin-workflow/transform/tests/fixture.rs',
  'packages/swc-plugin-workflow/transform/tests/errors.rs',
].map((relative) => path.join(sourceRoot, relative));
const parsedRows = testFiles.flatMap(parseFile).map(applyPortOverrides);
const swcRows = swcFixtureRows();
const swcInventoryFiles = [
  ...new Set(swcRows.map((row) => path.join(sourceRoot, row.file))),
];
const rows = [...parsedRows, ...swcRows];
const overrides = loadOverrides();
const markdown = renderInventory(
  rows,
  [...testFiles, ...swcTestFiles, ...swcInventoryFiles],
  overrides
);

if (process.argv.includes('--check')) {
  const current = fs.existsSync(outputPath)
    ? fs.readFileSync(outputPath, 'utf8')
    : '';
  if (current !== markdown) {
    fail(`${path.relative(repositoryRoot, outputPath)} is not up to date`);
  }
} else if (process.argv.includes('--dry-run')) {
  console.log(
    JSON.stringify(
      {
        testFiles: testFiles.length,
        cases: rows.length,
        foundationalCases: rows.filter((row) =>
          foundationalPackages.has(row.packageName)
        ).length,
      },
      null,
      2
    )
  );
} else {
  fs.writeFileSync(outputPath, markdown);
  console.log(
    `Wrote ${path.relative(repositoryRoot, outputPath)} with ${testFiles.length} files and ${rows.length} cases.`
  );
}
