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
  ['core', 'workflow-core'],
  ['errors', 'workflow-errors'],
  ['serde', 'workflow-serde'],
  ['utils', 'workflow-utils'],
  ['workflow', 'workflow'],
  ['world', 'workflow-world'],
  ['world-local', 'workflow-world-local'],
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

function rowOverride(row) {
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

function fail(message) {
  console.error(message);
  process.exit(1);
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
  if (hostFrameworkPackages.has(row.packageName)) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note: 'JavaScript framework binding; no Rust runtime counterpart unless a future host adapter is defined.',
    };
  }
  if (webOnlyPackages.has(row.packageName)) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note: 'Browser/UI behavior; no Rust runtime counterpart unless a future UI crate is defined.',
    };
  }
  if (docsOnlyPackages.has(row.packageName)) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note: 'Documentation or test harness package outside the Rust runtime parity target.',
    };
  }
  if (toolingPackages.has(row.packageName)) {
    return {
      portability: 'needs-review',
      status: 'needs-review',
      note: 'Tooling/build behavior; classify before any bucket can claim completion.',
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
      rows.push({ ...classified, ...bucket05Override(classified) });
    }
  }

  return applyRustParityOverrides(rows);
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

function renderInventory(rows, testFiles) {
  const summaryRows = summarize(rows, testFiles).map((summary) => [
    summary.packageId,
    summary.files,
    summary.cases,
    summary.portable,
    summary.needsReview,
    summary.jsOnly,
    summary.typeSystem,
  ]);

  const caseRows = rows.map((row) => {
    const override = rowOverride(row);
    return [
      row.packageName,
      row.file,
      row.line,
      row.suitePath || '(root)',
      row.caseName,
      row.declaration,
      row.portability,
      rustOwners.get(row.packageName) ?? 'unassigned',
      row.rustTestName ?? override?.rustTestName ?? '',
      override?.status ?? row.status,
      override?.note ?? row.note,
    ];
  });

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
const rows = testFiles.flatMap(parseFile);
const markdown = renderInventory(rows, testFiles);

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
