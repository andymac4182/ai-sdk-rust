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
  'docs/open-agents/upstream-parity.md'
);

const validPortability = new Set([
  'portable',
  'js-only-documented',
  'type-system-impossible',
]);
const validStatus = new Set([
  'not-started',
  'in-progress',
  'verified',
  'js-only-documented',
  'type-system-impossible',
]);

function usage() {
  console.log(`Usage: node scripts/open-agents-parity-check.mjs [options]

Options:
  --check          CI-safe reporting mode. Validate the ledger shape, print strict gaps, exit 0.
  --strict         Fail when any portable case lacks a named Rust test in the workspace.
  --ledger <path>  Open Agents upstream parity markdown file.
  --help           Show this help text.`);
}

function parseArgs(argv) {
  const options = {
    ledgerPath: defaultLedgerPath,
    mode: 'check',
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--help') {
      usage();
      process.exit(0);
    }
    if (arg === '--check') {
      options.mode = 'check';
      continue;
    }
    if (arg === '--strict') {
      options.mode = 'strict';
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

function relativePath(filePath) {
  if (filePath.startsWith('<')) {
    return filePath;
  }
  return path.relative(repositoryRoot, filePath) || filePath;
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

function parseTable(markdown, filePath, heading, errors) {
  const lines = markdown.split(/\r?\n/);
  const headingIndex = lines.findIndex((line) => line.trim() === heading);
  if (headingIndex === -1) {
    errors.push({
      filePath,
      line: 1,
      message: `missing ${heading} table`,
    });
    return { headers: [], rows: [] };
  }

  let tableIndex = headingIndex + 1;
  while (tableIndex < lines.length && !lines[tableIndex].trim().startsWith('|')) {
    tableIndex += 1;
  }

  const headerCells = splitMarkdownRow(lines[tableIndex] ?? '');
  const separatorCells = splitMarkdownRow(lines[tableIndex + 1] ?? '');
  if (!headerCells || !separatorCells || !isSeparator(separatorCells)) {
    errors.push({
      filePath,
      line: tableIndex + 1,
      message: `${heading} is not followed by a markdown table`,
    });
    return { headers: [], rows: [] };
  }

  const rows = [];
  for (let rowIndex = tableIndex + 2; rowIndex < lines.length; rowIndex += 1) {
    const cells = splitMarkdownRow(lines[rowIndex]);
    if (!cells) {
      break;
    }
    if (cells.length !== headerCells.length) {
      errors.push({
        filePath,
        line: rowIndex + 1,
        message:
          `${heading} row has ${cells.length} cells; expected ${headerCells.length}`,
      });
      continue;
    }
    rows.push({
      line: rowIndex + 1,
      values: Object.fromEntries(headerCells.map((header, index) => [header, cells[index]])),
    });
  }

  return { headers: headerCells, rows };
}

function parseInteger(value, issue) {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isInteger(parsed) || String(parsed) !== String(value).trim()) {
    issue(`invalid integer "${value}"`);
    return undefined;
  }
  return parsed;
}

function parseLedger(markdown, filePath, errors) {
  const summaryTable = parseTable(markdown, filePath, '## Test Case Summary', errors);
  const caseTable = parseTable(markdown, filePath, '## Test Case Inventory', errors);

  const summaryRows = summaryTable.rows.map((row) => {
    const issue = (message) => errors.push({ filePath, line: row.line, message });
    return {
      line: row.line,
      packageId: row.values.Package,
      testFiles: parseInteger(row.values['Test files'], issue),
      cases: parseInteger(row.values.Cases, issue),
      portable: parseInteger(row.values['Portable cases'], issue),
      mappedPortable: parseInteger(row.values['Mapped portable cases'], issue),
      unmappedPortable: parseInteger(row.values['Unmapped portable cases'], issue),
      jsOnly: parseInteger(row.values['JS-only cases'], issue),
      typeSystem: parseInteger(row.values['Type-system cases'], issue),
    };
  });

  const caseRows = caseTable.rows.map((row) => ({
    line: row.line,
    packageId: row.values.Package,
    upstreamFile: row.values['Upstream test file'],
    upstreamLine: row.values.Line,
    suite: row.values.Suite,
    caseName: row.values.Case,
    declaration: row.values.Declaration,
    portability: row.values.Portability,
    rustOwner: row.values['Rust owner crate/module'],
    rustTestName: row.values['Rust test name or exception'],
    status: row.values.Status,
    notes: row.values.Notes,
  }));

  return { summaryRows, caseRows };
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
  const files = [
    ...walk(path.join(repositoryRoot, 'crates')),
    ...walk(path.join(repositoryRoot, 'src')),
  ].filter((file) => file.endsWith('.rs'));

  for (const file of files) {
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
      const match = /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/.exec(line);
      if (pendingTestAttribute && match) {
        names.add(match[1]);
      }
      pendingTestAttribute = false;
    }
  }

  return names;
}

function splitRustTestNames(value) {
  return value
    .split(';')
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function isPendingRustTest(value) {
  return (
    !value ||
    value === 'n/a' ||
    value.startsWith('pending:') ||
    value === 'missing'
  );
}

function checkSummaryCounts(ledger, filePath, errors) {
  const computedByPackage = new Map();
  for (const row of ledger.caseRows) {
    if (!computedByPackage.has(row.packageId)) {
      computedByPackage.set(row.packageId, {
        files: new Set(),
        cases: 0,
        portable: 0,
        mappedPortable: 0,
        unmappedPortable: 0,
        jsOnly: 0,
        typeSystem: 0,
      });
    }
    const summary = computedByPackage.get(row.packageId);
    summary.files.add(row.upstreamFile);
    summary.cases += 1;
    if (row.portability === 'portable') {
      summary.portable += 1;
      if (isPendingRustTest(row.rustTestName)) {
        summary.unmappedPortable += 1;
      } else {
        summary.mappedPortable += 1;
      }
    } else if (row.portability === 'js-only-documented') {
      summary.jsOnly += 1;
    } else if (row.portability === 'type-system-impossible') {
      summary.typeSystem += 1;
    }
  }

  const seenPackages = new Set();
  for (const row of ledger.summaryRows) {
    if (seenPackages.has(row.packageId)) {
      errors.push({
        filePath,
        line: row.line,
        message: `duplicate test-case summary row for ${row.packageId}`,
      });
      continue;
    }
    seenPackages.add(row.packageId);
    const computed = computedByPackage.get(row.packageId) ?? {
      files: new Set(),
      cases: 0,
      portable: 0,
      mappedPortable: 0,
      unmappedPortable: 0,
      jsOnly: 0,
      typeSystem: 0,
    };
    const expected = [
      ['Test files', row.testFiles, computed.files.size],
      ['Cases', row.cases, computed.cases],
      ['Portable cases', row.portable, computed.portable],
      ['Mapped portable cases', row.mappedPortable, computed.mappedPortable],
      ['Unmapped portable cases', row.unmappedPortable, computed.unmappedPortable],
      ['JS-only cases', row.jsOnly, computed.jsOnly],
      ['Type-system cases', row.typeSystem, computed.typeSystem],
    ];
    for (const [label, actual, wanted] of expected) {
      if (actual !== undefined && actual !== wanted) {
        errors.push({
          filePath,
          line: row.line,
          message: `${row.packageId} summary ${label} is ${actual}; case table computes ${wanted}`,
        });
      }
    }
  }
}

function checkRows(ledger, ledgerPath, rustTests, strictErrors, structuralErrors) {
  for (const row of ledger.caseRows) {
    if (!validPortability.has(row.portability)) {
      structuralErrors.push({
        filePath: ledgerPath,
        line: row.line,
        message: `invalid portability "${row.portability}"`,
      });
    }
    if (!validStatus.has(row.status)) {
      structuralErrors.push({
        filePath: ledgerPath,
        line: row.line,
        message: `invalid status "${row.status}"`,
      });
    }
    if (!row.packageId || !row.upstreamFile || !row.upstreamLine || !row.caseName) {
      structuralErrors.push({
        filePath: ledgerPath,
        line: row.line,
        message: 'case row is missing package, upstream file, line, or case name',
      });
    }
    if (!row.notes) {
      structuralErrors.push({
        filePath: ledgerPath,
        line: row.line,
        message: 'case row has no notes',
      });
    }
    if (row.portability !== 'portable') {
      if (row.status !== row.portability) {
        structuralErrors.push({
          filePath: ledgerPath,
          line: row.line,
          message: `${row.portability} row must keep matching status ${row.portability}`,
        });
      }
      continue;
    }

    if (!row.rustOwner || row.rustOwner === 'unassigned' || row.rustOwner.startsWith('excluded:')) {
      strictErrors.push({
        filePath: ledgerPath,
        line: row.line,
        message: 'portable case has no owning Rust crate/module',
      });
    }
    if (isPendingRustTest(row.rustTestName)) {
      strictErrors.push({
        filePath: ledgerPath,
        line: row.line,
        message: 'portable case still has only an owner-mapped pending marker',
      });
      continue;
    }
    const missingRustTests = splitRustTestNames(row.rustTestName).filter(
      (testName) => !rustTests.has(testName)
    );
    if (missingRustTests.length > 0) {
      strictErrors.push({
        filePath: ledgerPath,
        line: row.line,
        message: `named Rust test(s) not found in workspace: ${missingRustTests.join(', ')}`,
      });
      continue;
    }
    if (row.status !== 'verified') {
      strictErrors.push({
        filePath: ledgerPath,
        line: row.line,
        message: `portable case with a named Rust test must be verified, got ${row.status}`,
      });
    }
  }
}

function formatErrors(errors, limit = errors.length) {
  return errors
    .slice(0, limit)
    .map((error) => `${relativePath(error.filePath)}:${error.line}: ${error.message}`)
    .join('\n');
}

function summarizeRows(rows) {
  const total = rows.length;
  const portable = rows.filter((row) => row.portability === 'portable');
  const unmapped = portable.filter((row) => isPendingRustTest(row.rustTestName));
  const jsOnly = rows.filter((row) => row.portability === 'js-only-documented');
  const typeSystem = rows.filter((row) => row.portability === 'type-system-impossible');
  return {
    total,
    portable: portable.length,
    mappedPortable: portable.length - unmapped.length,
    unmappedPortable: unmapped.length,
    jsOnly: jsOnly.length,
    typeSystem: typeSystem.length,
  };
}

try {
  const options = parseArgs(process.argv.slice(2));
  const markdown = fs.readFileSync(options.ledgerPath, 'utf8');
  const structuralErrors = [];
  const ledger = parseLedger(markdown, options.ledgerPath, structuralErrors);
  if (structuralErrors.length === 0) {
    checkSummaryCounts(ledger, options.ledgerPath, structuralErrors);
  }

  const rustTests = discoverRustTests();
  const strictErrors = [];
  if (structuralErrors.length === 0) {
    checkRows(ledger, options.ledgerPath, rustTests, strictErrors, structuralErrors);
  }

  if (structuralErrors.length > 0) {
    console.error('Open Agents parity ledger is structurally invalid:');
    console.error(formatErrors(structuralErrors));
    process.exit(1);
  }

  const summary = summarizeRows(ledger.caseRows);
  const summaryText =
    `${summary.total} cases, ${summary.portable} portable, ` +
    `${summary.mappedPortable} mapped portable, ` +
    `${summary.unmappedPortable} unmapped portable, ` +
    `${summary.jsOnly} js-only, ${summary.typeSystem} type-system.`;

  if (strictErrors.length > 0) {
    console.error(
      `Open Agents strict parity gaps: ${strictErrors.length} strict gap row(s); ${summaryText}`
    );
    console.error(formatErrors(strictErrors, 40));
    if (strictErrors.length > 40) {
      console.error(`... ${strictErrors.length - 40} more strict gap(s) in ${relativePath(options.ledgerPath)}`);
    }
    if (options.mode === 'strict') {
      process.exit(1);
    }
    console.log('Open Agents parity check ran in non-blocking --check mode; strict gaps are reported above.');
  } else {
    console.log(`Open Agents strict parity gate passed: ${summaryText}`);
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
