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
  ['utils', 'workflow-utils'],
  ['workflow', 'workflow'],
  ['world', 'workflow-world'],
  ['world-local', 'workflow-world-local'],
]);

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
      rows.push({ ...row, ...classify(row, source) });
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

  const caseRows = rows.map((row) => [
    row.packageName,
    row.file,
    row.line,
    row.suitePath || '(root)',
    row.caseName,
    row.declaration,
    row.portability,
    row.rustOwner ?? rustOwners.get(row.packageName) ?? 'unassigned',
    row.rustTestName ?? '',
    row.status,
    row.note,
  ]);

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
