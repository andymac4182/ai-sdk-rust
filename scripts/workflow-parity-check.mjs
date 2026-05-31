#!/usr/bin/env node
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..'
);

const defaultInventoryPath = path.join(
  repositoryRoot,
  'docs/workflow-test-inventory.md'
);
const defaultLedgerPath = path.join(
  repositoryRoot,
  'docs/workflow-upstream-parity.md'
);

const validPortability = new Set([
  'portable',
  'needs-review',
  'js-only-documented',
  'type-system-impossible',
]);

const validRowStatus = new Set([
  'not-started',
  'in-progress',
  'ported',
  'verified',
  'js-only-documented',
  'type-system-impossible',
  'needs-review',
]);

const validPackageStatus = new Set([
  'not-started',
  'in-progress',
  'ported',
  'verified',
  'js-only-documented',
  'type-system-impossible',
]);

const portableStatuses = new Set([
  'not-started',
  'in-progress',
  'ported',
  'verified',
]);

function usage() {
  console.log(`Usage: node scripts/workflow-parity-check.mjs [options]

Options:
  --inventory <path>   Workflow test inventory markdown file.
  --ledger <path>      Workflow upstream parity ledger markdown file.
  --self-test          Run built-in fixture checks instead of checking docs.
  --help               Show this help text.`);
}

function parseArgs(argv) {
  const options = {
    inventoryPath: defaultInventoryPath,
    ledgerPath: defaultLedgerPath,
    selfTest: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--help') {
      usage();
      process.exit(0);
    }
    if (arg === '--self-test') {
      options.selfTest = true;
      continue;
    }
    if (arg === '--inventory' || arg === '--ledger') {
      const value = argv[index + 1];
      if (!value || value.startsWith('--')) {
        throw new Error(`${arg} requires a path`);
      }
      if (arg === '--inventory') {
        options.inventoryPath = path.resolve(value);
      } else {
        options.ledgerPath = path.resolve(value);
      }
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

  if (tableIndex >= lines.length) {
    errors.push({
      filePath,
      line: headingIndex + 1,
      message: `${heading} has no markdown table`,
    });
    return { headers: [], rows: [] };
  }

  const headers = splitMarkdownRow(lines[tableIndex]) ?? [];
  const separator = splitMarkdownRow(lines[tableIndex + 1] ?? '') ?? [];
  if (headers.length === 0 || !isSeparator(separator)) {
    errors.push({
      filePath,
      line: tableIndex + 1,
      message: `${heading} table header is malformed`,
    });
    return { headers, rows: [] };
  }

  const rows = [];
  for (let index = tableIndex + 2; index < lines.length; index += 1) {
    if (!lines[index].trim().startsWith('|')) {
      break;
    }
    const cells = splitMarkdownRow(lines[index]) ?? [];
    if (cells.length !== headers.length) {
      errors.push({
        filePath,
        line: index + 1,
        message: `${heading} row has ${cells.length} cells; expected ${headers.length}`,
      });
      continue;
    }
    const values = Object.fromEntries(headers.map((header, cellIndex) => [
      header,
      cells[cellIndex],
    ]));
    rows.push({ line: index + 1, values });
  }

  return { headers, rows };
}

function parseInteger(value, issue) {
  if (!/^\d+$/.test(value.trim())) {
    issue(`expected a non-negative integer, got "${value}"`);
    return undefined;
  }
  return Number.parseInt(value, 10);
}

function stripInlineCode(value) {
  return value.trim().replace(/^`|`$/g, '').trim();
}

function isPresentOwner(value) {
  const normalized = stripInlineCode(value).toLowerCase();
  return normalized !== '' && normalized !== 'unassigned' && normalized !== 'none';
}

function isPresentRustTest(value) {
  const normalized = stripInlineCode(value).toLowerCase();
  return normalized !== '' && normalized !== 'unassigned' && normalized !== 'none';
}

function packageNameFromLedgerCell(value) {
  const packagePath = value.match(/`packages\/([^`]+)`/);
  if (packagePath) {
    return packagePath[1];
  }
  const code = value.match(/`([^`]+)`/);
  return code ? code[1] : value.trim();
}

function parseInventory(markdown, filePath, errors) {
  const summaryTable = parseTable(markdown, filePath, '## Summary', errors);
  const caseTable = parseTable(markdown, filePath, '## Case Inventory', errors);

  const summaryRows = summaryTable.rows.map((row) => {
    const issue = (message) => errors.push({ filePath, line: row.line, message });
    return {
      line: row.line,
      packageName: row.values.Package,
      testFiles: parseInteger(row.values['Test files'], issue),
      cases: parseInteger(row.values.Cases, issue),
      portable: parseInteger(row.values.Portable, issue),
      needsReview: parseInteger(row.values['Needs review'], issue),
      jsOnly: parseInteger(row.values['JS only'], issue),
      typeSystem: parseInteger(row.values['Type system'], issue),
    };
  });

  const caseRows = caseTable.rows.map((row) => ({
    line: row.line,
    packageName: row.values.Package,
    upstreamFile: row.values['Upstream file'],
    portability: row.values.Portability,
    rustOwner: row.values['Rust owner crate'],
    rustTestName: row.values['Rust test name'],
    status: row.values.Status,
  }));

  return { summaryRows, caseRows };
}

function parseLedger(markdown, filePath, errors) {
  const packageTable = parseTable(markdown, filePath, '## Package Inventory', errors);
  return packageTable.rows.map((row) => ({
    line: row.line,
    packageName: packageNameFromLedgerCell(row.values['Upstream package']),
    status: row.values.Status,
  }));
}

function rowLocation(inventoryPath, row) {
  return `${relativePath(inventoryPath)}:${row.line}`;
}

function checkSummaryCounts(inventory, inventoryPath, errors) {
  const computedByPackage = new Map();
  for (const row of inventory.caseRows) {
    if (!computedByPackage.has(row.packageName)) {
      computedByPackage.set(row.packageName, {
        cases: 0,
        portable: 0,
        needsReview: 0,
        jsOnly: 0,
        typeSystem: 0,
        files: new Set(),
      });
    }
    const summary = computedByPackage.get(row.packageName);
    summary.cases += 1;
    summary.files.add(row.upstreamFile);
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

  const seenSummaryPackages = new Set();
  for (const row of inventory.summaryRows) {
    if (seenSummaryPackages.has(row.packageName)) {
      errors.push({
        filePath: inventoryPath,
        line: row.line,
        message: `duplicate summary row for package ${row.packageName}`,
      });
      continue;
    }
    seenSummaryPackages.add(row.packageName);

    const computed = computedByPackage.get(row.packageName) ?? {
      cases: 0,
      portable: 0,
      needsReview: 0,
      jsOnly: 0,
      typeSystem: 0,
      files: new Set(),
    };

    const expected = [
      ['Cases', row.cases, computed.cases],
      ['Portable', row.portable, computed.portable],
      ['Needs review', row.needsReview, computed.needsReview],
      ['JS only', row.jsOnly, computed.jsOnly],
      ['Type system', row.typeSystem, computed.typeSystem],
    ];
    for (const [label, actual, wanted] of expected) {
      if (actual !== undefined && actual !== wanted) {
        errors.push({
          filePath: inventoryPath,
          line: row.line,
          message: `${row.packageName} summary ${label} is ${actual}; row table computes ${wanted}`,
        });
      }
    }

    if (computed.cases > 0 && row.testFiles !== undefined) {
      const fileCount = computed.files.size;
      if (row.testFiles < fileCount) {
        errors.push({
          filePath: inventoryPath,
          line: row.line,
          message:
            `${row.packageName} summary Test files is ${row.testFiles}; ` +
            `row table references at least ${fileCount}`,
        });
      }
    }
  }

  for (const packageName of computedByPackage.keys()) {
    if (!seenSummaryPackages.has(packageName)) {
      const firstRow = inventory.caseRows.find((row) => row.packageName === packageName);
      errors.push({
        filePath: inventoryPath,
        line: firstRow?.line ?? 1,
        message: `case inventory package ${packageName} is missing from the summary table`,
      });
    }
  }
}

function checkRowStatuses(inventory, inventoryPath, errors) {
  for (const row of inventory.caseRows) {
    if (!validPortability.has(row.portability)) {
      errors.push({
        filePath: inventoryPath,
        line: row.line,
        message: `invalid portability "${row.portability}"`,
      });
    }
    if (!validRowStatus.has(row.status)) {
      errors.push({
        filePath: inventoryPath,
        line: row.line,
        message: `invalid status "${row.status}"`,
      });
    }
    if (!validPortability.has(row.portability) || !validRowStatus.has(row.status)) {
      continue;
    }

    if (row.portability === 'portable' && !portableStatuses.has(row.status)) {
      errors.push({
        filePath: inventoryPath,
        line: row.line,
        message: `portable row cannot have status ${row.status}`,
      });
    } else if (row.portability !== 'portable' && row.status !== row.portability) {
      errors.push({
        filePath: inventoryPath,
        line: row.line,
        message: `${row.portability} row must keep matching status ${row.portability}, got ${row.status}`,
      });
    }

    if (
      row.portability === 'portable' &&
      (row.status === 'ported' || row.status === 'verified') &&
      (!isPresentOwner(row.rustOwner) || !isPresentRustTest(row.rustTestName))
    ) {
      errors.push({
        filePath: inventoryPath,
        line: row.line,
        message: `portable ${row.status} row needs a Rust owner crate and Rust test name`,
      });
    }
  }
}

function checkLedgerOverclaims(inventory, ledgerRows, inventoryPath, ledgerPath, errors) {
  const rowsByPackage = new Map();
  for (const row of inventory.caseRows) {
    if (!rowsByPackage.has(row.packageName)) {
      rowsByPackage.set(row.packageName, []);
    }
    rowsByPackage.get(row.packageName).push(row);
  }

  const seenLedgerPackages = new Set();
  for (const packageRow of ledgerRows) {
    if (seenLedgerPackages.has(packageRow.packageName)) {
      errors.push({
        filePath: ledgerPath,
        line: packageRow.line,
        message: `duplicate package inventory row for ${packageRow.packageName}`,
      });
      continue;
    }
    seenLedgerPackages.add(packageRow.packageName);

    if (!validPackageStatus.has(packageRow.status)) {
      errors.push({
        filePath: ledgerPath,
        line: packageRow.line,
        message: `invalid package status "${packageRow.status}" for ${packageRow.packageName}`,
      });
      continue;
    }

    if (packageRow.status !== 'ported' && packageRow.status !== 'verified') {
      continue;
    }

    const rows = rowsByPackage.get(packageRow.packageName) ?? [];
    const needsReviewRows = rows.filter(
      (row) => row.portability === 'needs-review' || row.status === 'needs-review'
    );
    if (needsReviewRows.length > 0) {
      errors.push({
        filePath: ledgerPath,
        line: packageRow.line,
        message:
          `${packageRow.packageName} is ${packageRow.status} but still has ` +
          `${needsReviewRows.length} needs-review row(s); first at ` +
          rowLocation(inventoryPath, needsReviewRows[0]),
      });
    }

    const portableRows = rows.filter((row) => row.portability === 'portable');
    const underClaimedRows =
      packageRow.status === 'verified'
        ? portableRows.filter((row) => row.status !== 'verified')
        : portableRows.filter(
            (row) => row.status !== 'ported' && row.status !== 'verified'
          );
    if (underClaimedRows.length > 0) {
      const required =
        packageRow.status === 'verified' ? 'verified' : 'ported or verified';
      errors.push({
        filePath: ledgerPath,
        line: packageRow.line,
        message:
          `${packageRow.packageName} is ${packageRow.status} but has ` +
          `${underClaimedRows.length} portable row(s) not ${required}; first at ` +
          `${rowLocation(inventoryPath, underClaimedRows[0])} ` +
          `(status ${underClaimedRows[0].status})`,
      });
    }
  }
}

function checkMarkdownDocuments({
  inventoryMarkdown,
  ledgerMarkdown,
  inventoryPath,
  ledgerPath,
}) {
  const errors = [];
  const inventory = parseInventory(inventoryMarkdown, inventoryPath, errors);
  const ledgerRows = parseLedger(ledgerMarkdown, ledgerPath, errors);

  if (errors.length === 0) {
    checkSummaryCounts(inventory, inventoryPath, errors);
    checkRowStatuses(inventory, inventoryPath, errors);
    checkLedgerOverclaims(inventory, ledgerRows, inventoryPath, ledgerPath, errors);
  }

  return {
    errors,
    rowCount: inventory.caseRows.length,
    summaryPackageCount: inventory.summaryRows.length,
    ledgerPackageCount: ledgerRows.length,
  };
}

function formatErrors(errors) {
  return errors
    .map((error) => `${relativePath(error.filePath)}:${error.line}: ${error.message}`)
    .join('\n');
}

function checkFiles(inventoryPath, ledgerPath) {
  const inventoryMarkdown = fs.readFileSync(inventoryPath, 'utf8');
  const ledgerMarkdown = fs.readFileSync(ledgerPath, 'utf8');
  return checkMarkdownDocuments({
    inventoryMarkdown,
    ledgerMarkdown,
    inventoryPath,
    ledgerPath,
  });
}

const passingInventory = `# Workflow SDK Test Inventory

## Summary

| Package | Test files | Cases | Portable | Needs review | JS only | Type system |
| --- | --- | --- | --- | --- | --- | --- |
| core | 2 | 2 | 1 | 0 | 1 | 0 |

## Case Inventory

| Package | Upstream file | Line | Suite | Case | Declaration | Portability | Rust owner crate | Rust test name | Status | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| core | packages/core/src/a.test.ts | 1 | root | works | it | portable | workflow-core | core_works | verified |  |
| core | packages/core/src/b.test.ts | 2 | root | browser only | it | js-only-documented | workflow-core |  | js-only-documented | documented |
`;

const passingLedger = `# Workflow SDK Upstream Parity

## Package Inventory

| Upstream package | Version | Class | Status | Rust owner | Major source and test files | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| \`packages/core\` (\`@workflow/core\`) | 1.0.0 | portable Rust runtime | verified | \`crates/workflow-core\` | Tests |  |
`;

function expectFixtureFailure(name, inventoryMarkdown, ledgerMarkdown, pattern) {
  const result = checkMarkdownDocuments({
    inventoryMarkdown,
    ledgerMarkdown,
    inventoryPath: '<fixture-inventory>',
    ledgerPath: '<fixture-ledger>',
  });
  assert.notEqual(result.errors.length, 0, `${name} should fail`);
  assert.match(formatErrors(result.errors), pattern, name);
}

function runSelfTest() {
  const passResult = checkMarkdownDocuments({
    inventoryMarkdown: passingInventory,
    ledgerMarkdown: passingLedger,
    inventoryPath: '<fixture-inventory>',
    ledgerPath: '<fixture-ledger>',
  });
  assert.deepEqual(passResult.errors, []);

  expectFixtureFailure(
    'invalid row values',
    passingInventory.replace('portable | workflow-core', 'runtime | workflow-core'),
    passingLedger,
    /invalid portability "runtime"/
  );
  expectFixtureFailure(
    'missing rust test name',
    passingInventory.replace('core_works | verified', ' | verified'),
    passingLedger,
    /needs a Rust owner crate and Rust test name/
  );
  expectFixtureFailure(
    'summary drift',
    passingInventory.replace('| core | 2 | 2 | 1 | 0 | 1 | 0 |', '| core | 2 | 3 | 1 | 0 | 1 | 0 |'),
    passingLedger,
    /summary Cases is 3; row table computes 2/
  );
  expectFixtureFailure(
    'needs review overclaim',
    passingInventory.replace(
      '| core | packages/core/src/b.test.ts | 2 | root | browser only | it | js-only-documented | workflow-core |  | js-only-documented | documented |',
      '| core | packages/core/src/b.test.ts | 2 | root | inspect me | it | needs-review | workflow-core |  | needs-review | blocking |'
    ),
    passingLedger,
    /still has 1 needs-review row/
  );
  expectFixtureFailure(
    'verified package overclaim',
    passingInventory.replace('core_works | verified', 'core_works | ported'),
    passingLedger,
    /portable row\(s\) not verified/
  );

  console.log('Workflow parity self-test fixtures passed (6 cases).');
}

try {
  const options = parseArgs(process.argv.slice(2));
  if (options.selfTest) {
    runSelfTest();
  } else {
    const result = checkFiles(options.inventoryPath, options.ledgerPath);
    if (result.errors.length > 0) {
      console.error('Workflow parity gate failed:');
      console.error(formatErrors(result.errors));
      process.exit(1);
    }
    console.log(
      `Workflow parity gate passed: ${result.rowCount} inventory rows, ` +
        `${result.summaryPackageCount} summary packages, ` +
        `${result.ledgerPackageCount} ledger packages.`
    );
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
