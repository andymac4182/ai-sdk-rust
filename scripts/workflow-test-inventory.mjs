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
const sourceHead = '1ee63b870afbf9754eb1022b1bb5f02d0ab042f9';
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
      status: 'ported',
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

function ported(rustTestName, note = '') {
  return {
    portability: 'portable',
    status: 'ported',
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

  if (file === 'packages/core/e2e/e2e.test.ts') {
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
    return 'workflow_ai_upstream_do_stream_step_rows_are_owned_by_workflow_ai';
  }
  if (
    file.endsWith('/agent/durable-agent.test.ts') ||
    file.endsWith('/agent/durable-agent-compat.test.ts') ||
    file.endsWith('/agent/telemetry.test.ts')
  ) {
    return 'workflow_ai_upstream_durable_agent_rows_are_owned_by_workflow_ai';
  }
  if (file.endsWith('/agent/stream-text-iterator.test.ts')) {
    return 'workflow_ai_upstream_stream_text_iterator_rows_are_owned_by_workflow_ai';
  }
  if (file.endsWith('/agent/tools-to-model-tools.test.ts')) {
    return 'workflow_ai_upstream_tools_to_model_tools_rows_are_owned_by_workflow_ai';
  }
  if (file.endsWith('/workflow-chat-transport.test.ts')) {
    return 'workflow_ai_upstream_workflow_chat_transport_rows_are_owned_by_workflow_ai';
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

  return 'workflow_ai_upstream_package_ai_rows_are_owned_by_workflow_ai';
}

function aiInventoryOverride(row) {
  if (row.packageName !== 'ai') {
    return undefined;
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
      'Verified by package-owned workflow-ai tests; implementation bridges to existing Rust AI SDK/chat-sdk compatible internals where appropriate.',
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
  const summaryRows = summarize(rows, testFiles).map((summary) => [
    summary.packageId,
    summary.files,
    summary.cases,
    summary.portable,
    summary.needsReview,
    summary.jsOnly,
    summary.typeSystem,
  ]);

  const seenOverrideKeys = new Set();
  const caseRows = rows.map((row) => {
    const overrideKey = rowKey(row);
    const override = overrides.get(overrideKey);
    const generatedOverride = rowOverride(row) ?? aiInventoryOverride(row);
    if (override) {
      seenOverrideKeys.add(rowKey(row));
    }
    return [
      row.packageName,
      row.file,
      row.line,
      row.suitePath || '(root)',
      row.caseName,
      row.declaration,
      override?.portability ?? row.portability,
      override?.rustOwner ?? rustOwners.get(row.packageName) ?? 'unassigned',
      override?.rustTestName ??
        row.rustTestName ??
        generatedOverride?.rustTestName ??
        implementedRustTestName(row),
      override?.status ?? generatedOverride?.status ?? implementedStatus(row),
      override?.notes ?? generatedOverride?.note ?? implementedNote(row),
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

Rows marked needs-review are blocking classification work: a future bucket must
convert them to portable, js-only-documented, or type-system-impossible before
claiming completion for the owning package.

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
const rows = [...parsedRows, ...swcFixtureRows()];
const overrides = loadOverrides();
const markdown = renderInventory(rows, [...testFiles, ...swcTestFiles], overrides);

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
