#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..'
);
const defaultLedgerPath = path.join(
  repositoryRoot,
  'docs/open-agents/open-plugin-spec.md'
);
const expectedSpecHead = 'cd5f34e7f1b9398267843d2e32f38e57a58597c2';
const expectedImplementedIds = new Set([
  'OP-01-001',
  'OP-01-002',
  'OP-01-003',
  'OP-01-004',
  'OP-01-005',
  'OP-01-006',
  'OP-01-007',
  'OP-01-008',
  'OP-01-009',
  'OP-01-010',
  'OP-01-011',
  'OP-01-012',
  'OP-01-013',
  'OP-01-014',
  'OP-01-016',
]);
const validStatuses = new Set([
  'implemented',
  'deferred-op-02',
  'deferred-op-03',
  'deferred-op-04',
  'not-supported',
]);

function usage() {
  console.log(`Usage: node scripts/open-plugin-spec-gate.mjs [options]

Options:
  --check          Validate the Open Plugin Spec conformance tracker.
  --ledger <path>  Override docs/open-agents/open-plugin-spec.md.
  --help           Show this help text.`);
}

function parseArgs(argv) {
  const options = {
    ledgerPath: defaultLedgerPath,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--help') {
      usage();
      process.exit(0);
    }
    if (arg === '--check') {
      continue;
    }
    if (arg === '--ledger') {
      const value = argv[index + 1];
      if (!value || value.startsWith('--')) {
        throw new Error('--ledger requires a path');
      }
      options.ledgerPath = path.resolve(value);
      index += 1;
      continue;
    }
    throw new Error(`unknown option: ${arg}`);
  }

  return options;
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

function isSeparator(cells) {
  return cells.every((cell) => /^:?-{3,}:?$/.test(cell.trim()));
}

function parseTable(markdown, heading, errors) {
  const lines = markdown.split(/\r?\n/);
  const headingIndex = lines.findIndex((line) => line.trim() === heading);
  if (headingIndex === -1) {
    errors.push(`missing ${heading} table`);
    return [];
  }

  let tableIndex = headingIndex + 1;
  while (tableIndex < lines.length && !lines[tableIndex].trim().startsWith('|')) {
    tableIndex += 1;
  }

  const headers = splitMarkdownRow(lines[tableIndex] ?? '');
  const separator = splitMarkdownRow(lines[tableIndex + 1] ?? '');
  if (!headers || !separator || !isSeparator(separator)) {
    errors.push(`${heading} is not followed by a markdown table`);
    return [];
  }

  const rows = [];
  for (let rowIndex = tableIndex + 2; rowIndex < lines.length; rowIndex += 1) {
    const cells = splitMarkdownRow(lines[rowIndex]);
    if (!cells) {
      break;
    }
    if (cells.length !== headers.length) {
      errors.push(
        `${heading} row ${rowIndex + 1} has ${cells.length} cells; expected ${headers.length}`
      );
      continue;
    }
    rows.push(Object.fromEntries(headers.map((header, index) => [header, cells[index]])));
  }

  return rows;
}

function walk(directory) {
  const entries = fs.readdirSync(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    if (
      entry.name === '.git' ||
      entry.name === 'target' ||
      entry.name === 'node_modules'
    ) {
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

function discoverRustTests() {
  const names = new Set();
  for (const rootName of ['crates', 'src']) {
    const root = path.join(repositoryRoot, rootName);
    if (!fs.existsSync(root)) {
      continue;
    }
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
  return names;
}

function splitRustTests(value) {
  if (!value || value === 'n/a') {
    return [];
  }
  return value
    .split(';')
    .map((entry) => entry.trim().replace(/^`|`$/g, ''))
    .filter(Boolean);
}

function validateImplementedRow(row, rustTests, errors) {
  const tests = splitRustTests(row['Rust tests']);
  if (tests.length === 0) {
    errors.push(`${row.ID} is implemented but has no Rust test mapping`);
  }
  for (const test of tests) {
    if (!rustTests.has(test)) {
      errors.push(`${row.ID} references missing Rust test ${test}`);
    }
  }
  if (row.Handoff !== 'n/a') {
    errors.push(`${row.ID} is implemented but has non-empty handoff "${row.Handoff}"`);
  }
}

function validateDeferredRow(row, errors) {
  if (row['Rust tests'] !== 'n/a') {
    errors.push(`${row.ID} is deferred but names Rust tests`);
  }
  if (!/^OP-0[234]\b/.test(row.Handoff)) {
    errors.push(`${row.ID} deferred row must hand off to OP-02, OP-03, or OP-04`);
  }
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const errors = [];

  if (!fs.existsSync(options.ledgerPath)) {
    throw new Error(`missing Open Plugin Spec ledger: ${options.ledgerPath}`);
  }

  const markdown = fs.readFileSync(options.ledgerPath, 'utf8');
  if (!markdown.includes(`| Upstream commit | \`${expectedSpecHead}\` |`)) {
    errors.push(`source snapshot must pin upstream commit ${expectedSpecHead}`);
  }

  const checklistRows = parseTable(markdown, '## Conformance Checklist', errors);
  const diagnosticRows = parseTable(markdown, '## Diagnostics Matrix', errors);
  const rustTests = discoverRustTests();

  const implementedIds = new Set();
  for (const row of checklistRows) {
    if (!row.ID || !validStatuses.has(row.Status)) {
      errors.push(`${row.ID || '(missing id)'} has invalid status "${row.Status}"`);
      continue;
    }
    if (row.Status === 'implemented') {
      implementedIds.add(row.ID);
      validateImplementedRow(row, rustTests, errors);
    } else if (row.Status.startsWith('deferred-')) {
      validateDeferredRow(row, errors);
    }
  }

  for (const expectedId of expectedImplementedIds) {
    if (!implementedIds.has(expectedId)) {
      errors.push(`missing implemented conformance row ${expectedId}`);
    }
  }

  for (const row of diagnosticRows) {
    if (row.Status === 'implemented') {
      validateImplementedRow(
        {
          ID: row['Decision point'],
          'Rust tests': row['Rust tests'],
          Handoff: row.Handoff,
        },
        rustTests,
        errors
      );
    } else if (row.Status?.startsWith('deferred-')) {
      validateDeferredRow(
        {
          ID: row['Decision point'],
          'Rust tests': row['Rust tests'],
          Handoff: row.Handoff,
        },
        errors
      );
    } else {
      errors.push(`${row['Decision point'] || '(missing decision point)'} has invalid status "${row.Status}"`);
    }
  }

  if (errors.length > 0) {
    console.error('open-plugin-spec gate failed:');
    for (const error of errors) {
      console.error(`- ${error}`);
    }
    process.exit(1);
  }

  console.log(
    `open-plugin-spec gate passed: ${implementedIds.size} implemented rows verified against Rust tests`
  );
}

try {
  main();
} catch (error) {
  console.error(`open-plugin-spec gate failed: ${error.message}`);
  process.exit(1);
}
