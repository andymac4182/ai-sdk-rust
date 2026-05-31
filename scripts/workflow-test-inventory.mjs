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
  ['builders', 'workflow-builders'],
  ['cli', 'workflow-cli'],
  ['core', 'workflow-core'],
  ['errors', 'workflow-errors'],
  ['swc-plugin-workflow', 'workflow-swc-plugin'],
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
      rows.push({ ...row, ...classify(row, source) });
    }
  }

  return rows;
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
    rustOwners.get(row.packageName) ?? 'unassigned',
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
const swcTestFiles = [
  'packages/swc-plugin-workflow/transform/tests/fixture.rs',
  'packages/swc-plugin-workflow/transform/tests/errors.rs',
].map((relative) => path.join(sourceRoot, relative));
const rows = [...testFiles.flatMap(parseFile), ...swcFixtureRows()];
const markdown = renderInventory(rows, [...testFiles, ...swcTestFiles]);

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
