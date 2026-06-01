#!/usr/bin/env node
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

const DEFAULT_UPSTREAM_ROOT = path.join(
  os.homedir(),
  '.opensrc/repos/github.com/vercel/ai/main',
);
const DEFAULT_OUTPUT_PATH = path.join(
  repositoryRoot,
  'docs/ai-strict-test-inventory.md',
);
const DEFAULT_LEDGER_PATH = path.join(repositoryRoot, 'docs/upstream-parity.md');
const FOUNDATIONAL_INVENTORY_PATH = path.join(
  repositoryRoot,
  'docs/ai-foundational-provider-inventory.md',
);
const AI_02_INVENTORY_PATH = path.join(
  repositoryRoot,
  'docs/ai-02-openai-compatible-providers.md',
);

const UPSTREAM_REPO = 'vercel/ai';
const UPSTREAM_HEAD = 'ab6d66482d31afe15f4973a51c5f7cfa09c92ea6';
const UPSTREAM_COMMIT_DATE = '2026-05-30T00:54:18Z';
const INVENTORY_DATE = '2026-06-02';
const FETCH_COMMAND = 'npx opensrc fetch github:vercel/ai';

const TEST_FILE_PATTERN =
  /\.(?:test|spec)(?:-d)?\.(?:[cm]?[tj]sx?|mts|cts)$/;
const VALID_STATUSES = new Set([
  'portable-mapped',
  'portable-unmapped',
  'js-only-documented',
  'type-system-impossible',
]);
const EXCEPTION_STATUSES = new Set([
  'js-only-documented',
  'type-system-impossible',
]);

function usage() {
  console.log(`Usage: node scripts/ai-strict-test-inventory.mjs [options]

Options:
  --upstream-root <path>   Refreshed vercel/ai source root.
  --output <path>          Markdown inventory output path.
  --ledger <path>          Upstream parity ledger used for package dispositions.
  --check                  Fail when the generated inventory differs from --output.
  --fail-on-unmapped       Also fail when any portable upstream case is unmapped.
  --help                   Show this help.

Refresh upstream first with:
  ${FETCH_COMMAND}`);
}

function parseArgs(argv) {
  const args = {
    upstreamRoot: DEFAULT_UPSTREAM_ROOT,
    outputPath: DEFAULT_OUTPUT_PATH,
    ledgerPath: DEFAULT_LEDGER_PATH,
    check: false,
    failOnUnmapped: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    switch (arg) {
      case '--upstream-root':
        args.upstreamRoot = path.resolve(requireValue(argv, index, arg));
        index += 1;
        break;
      case '--output':
        args.outputPath = path.resolve(requireValue(argv, index, arg));
        index += 1;
        break;
      case '--ledger':
        args.ledgerPath = path.resolve(requireValue(argv, index, arg));
        index += 1;
        break;
      case '--check':
        args.check = true;
        break;
      case '--fail-on-unmapped':
        args.failOnUnmapped = true;
        break;
      case '--help':
      case '-h':
        usage();
        process.exit(0);
        break;
      default:
        throw new Error(`unknown argument: ${arg}`);
    }
  }

  return args;
}

function requireValue(argv, index, arg) {
  const value = argv[index + 1];
  if (!value || value.startsWith('--')) {
    throw new Error(`${arg} requires a value`);
  }
  return value;
}

function walkFiles(dir) {
  if (!fs.existsSync(dir)) {
    return [];
  }

  const files = [];
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const childPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...walkFiles(childPath));
    } else if (entry.isFile()) {
      files.push(childPath);
    }
  }
  return files.sort();
}

function relativePath(file, root) {
  return path.relative(root, file).split(path.sep).join('/');
}

function lineNumber(source, index) {
  return source.slice(0, index).split('\n').length;
}

function skipWhitespace(source, index) {
  let cursor = index;
  while (cursor < source.length && /\s/.test(source[cursor])) {
    cursor += 1;
  }
  return cursor;
}

function readIdentifier(source, index) {
  let cursor = index;
  if (!/[A-Za-z_$]/.test(source[cursor] ?? '')) {
    return null;
  }
  cursor += 1;
  while (/[A-Za-z0-9_$]/.test(source[cursor] ?? '')) {
    cursor += 1;
  }
  return { value: source.slice(index, cursor), end: cursor };
}

function matchingParen(source, openIndex) {
  let depth = 0;
  let quote = null;
  let escaped = false;
  let lineComment = false;
  let blockComment = false;

  for (let index = openIndex; index < source.length; index += 1) {
    const char = source[index];
    const next = source[index + 1];

    if (lineComment) {
      if (char === '\n') {
        lineComment = false;
      }
      continue;
    }
    if (blockComment) {
      if (char === '*' && next === '/') {
        blockComment = false;
        index += 1;
      }
      continue;
    }
    if (quote) {
      if (escaped) {
        escaped = false;
      } else if (char === '\\') {
        escaped = true;
      } else if (char === quote) {
        quote = null;
      }
      continue;
    }
    if (char === '/' && next === '/') {
      lineComment = true;
      index += 1;
      continue;
    }
    if (char === '/' && next === '*') {
      blockComment = true;
      index += 1;
      continue;
    }
    if (char === "'" || char === '"' || char === '`') {
      quote = char;
      continue;
    }
    if (char === '(') {
      depth += 1;
    } else if (char === ')') {
      depth -= 1;
      if (depth === 0) {
        return index;
      }
    }
  }

  return -1;
}

function readQuotedString(source, startIndex) {
  let cursor = skipWhitespace(source, startIndex);
  const quote = source[cursor];
  if (quote !== "'" && quote !== '"' && quote !== '`') {
    return null;
  }

  cursor += 1;
  let value = '';
  let escaped = false;
  for (; cursor < source.length; cursor += 1) {
    const char = source[cursor];
    if (escaped) {
      value += char;
      escaped = false;
    } else if (char === '\\') {
      escaped = true;
    } else if (char === quote) {
      return {
        value: normalizeCaseName(value),
        end: cursor + 1,
      };
    } else {
      value += char;
    }
  }
  return null;
}

function countEachRows(expression) {
  const trimmed = expression.trim();
  if (!trimmed.startsWith('[')) {
    return null;
  }

  let depth = 0;
  let quote = null;
  let escaped = false;
  let count = 0;
  let sawValue = false;

  for (let index = 1; index < trimmed.length - 1; index += 1) {
    const char = trimmed[index];
    if (quote) {
      if (escaped) {
        escaped = false;
      } else if (char === '\\') {
        escaped = true;
      } else if (char === quote) {
        quote = null;
      }
      continue;
    }
    if (char === "'" || char === '"' || char === '`') {
      quote = char;
      sawValue = true;
    } else if (char === '[' || char === '{' || char === '(') {
      depth += 1;
      sawValue = true;
    } else if (char === ']' || char === '}' || char === ')') {
      depth -= 1;
    } else if (char === ',' && depth === 0) {
      count += 1;
      sawValue = false;
    } else if (!/\s/.test(char)) {
      sawValue = true;
    }
  }

  return count + (sawValue ? 1 : 0);
}

function normalizeCaseName(value) {
  return String(value).replace(/\s+/g, ' ').trim();
}

function normalizeNameKey(value) {
  return normalizeCaseName(value).toLowerCase();
}

function extractTestCases(file, upstreamRoot) {
  const source = fs.readFileSync(file, 'utf8');
  const relativeFile = relativePath(file, upstreamRoot);
  const cases = [];
  const callPattern = /(^|[^\w$.])(it|test)\b/g;

  for (const match of source.matchAll(callPattern)) {
    const start = match.index + match[1].length;
    let cursor = start + match[2].length;
    const modifiers = [];
    let tableRows = null;

    while (true) {
      cursor = skipWhitespace(source, cursor);
      if (source[cursor] !== '.') {
        break;
      }

      const identifier = readIdentifier(source, cursor + 1);
      if (!identifier) {
        break;
      }
      const modifier = identifier.value;
      modifiers.push(modifier);
      cursor = skipWhitespace(source, identifier.end);

      if (source[cursor] === '(') {
        const closeIndex = matchingParen(source, cursor);
        if (closeIndex === -1) {
          break;
        }
        if (modifier === 'each') {
          tableRows = countEachRows(source.slice(cursor + 1, closeIndex));
        }
        cursor = closeIndex + 1;
      }
    }

    if (modifiers.includes('describe')) {
      continue;
    }

    cursor = skipWhitespace(source, cursor);
    if (source[cursor] !== '(') {
      continue;
    }

    const testName = readQuotedString(source, cursor + 1);
    if (!testName) {
      continue;
    }

    cases.push({
      file: relativeFile,
      line: lineNumber(source, start),
      kind: [match[2], ...modifiers].join('.'),
      name: testName.value,
      tableRows,
    });
  }

  return cases.sort((left, right) => left.line - right.line || left.name.localeCompare(right.name));
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
  return cells.length > 0 && cells.every(cell => /^:?-{3,}:?$/.test(cell.trim()));
}

function stripInlineCode(value) {
  return String(value).trim().replace(/^`|`$/g, '').trim();
}

function parseLedgerPackages(ledgerPath) {
  const rows = new Map();
  const markdown = fs.readFileSync(ledgerPath, 'utf8');
  const lines = markdown.split(/\r?\n/);
  let inPackageInventory = false;
  let headers = null;

  for (const line of lines) {
    if (line.startsWith('## Package And Provider Inventory')) {
      inPackageInventory = true;
      headers = null;
      continue;
    }
    if (inPackageInventory && line.startsWith('## ')) {
      break;
    }
    if (!inPackageInventory || !line.trim().startsWith('|')) {
      continue;
    }

    const cells = splitMarkdownRow(line);
    if (!cells || isSeparator(cells)) {
      continue;
    }
    if (!headers) {
      headers = cells;
      continue;
    }
    if (cells.length !== headers.length) {
      continue;
    }

    const values = Object.fromEntries(headers.map((header, index) => [header, cells[index]]));
    const packageDir = values['Upstream item']?.match(/`packages\/([^`]+)`/)?.[1];
    if (!packageDir) {
      continue;
    }
    const displayName =
      values['Upstream item']?.match(/\(`([^`]+)`\)/)?.[1] ?? packageDir;
    rows.set(packageDir, {
      displayName,
      kind: values.Kind,
      ledgerStatus: stripInlineCode(values.Status),
      owner: values['Rust path'],
      evidence: values.Evidence,
      notes: values.Notes,
    });
  }

  return rows;
}

function parsePackageJsonPackages(upstreamRoot, ledgerPackages) {
  const packagesRoot = path.join(upstreamRoot, 'packages');
  const packageJsonFiles = walkFiles(packagesRoot)
    .filter(file => path.basename(file) === 'package.json')
    .filter(file => path.dirname(file).split(path.sep).at(-2) === 'packages');

  return packageJsonFiles.map(file => {
    const packageDir = path.basename(path.dirname(file));
    const parsed = JSON.parse(fs.readFileSync(file, 'utf8'));
    const ledger = ledgerPackages.get(packageDir);
    return {
      item: `packages/${packageDir}`,
      area: 'packages',
      packageDir,
      displayName: parsed.name ?? ledger?.displayName ?? packageDir,
      kind: ledger?.kind ?? 'upstream package',
      ledgerStatus: ledger?.ledgerStatus ?? 'not-started',
      owner: ledger?.owner ?? 'unassigned',
      root: path.dirname(file),
    };
  }).sort((left, right) => left.item.localeCompare(right.item));
}

function discoverExampleRoots(upstreamRoot) {
  const examplesRoot = path.join(upstreamRoot, 'examples');
  if (!fs.existsSync(examplesRoot)) {
    return [];
  }

  return fs.readdirSync(examplesRoot, { withFileTypes: true })
    .filter(entry => entry.isDirectory())
    .map(entry => ({
      item: `examples/${entry.name}`,
      area: 'examples',
      packageDir: null,
      displayName: `examples/${entry.name}`,
      kind: 'example',
      ledgerStatus: 'not-started',
      owner: 'unassigned',
      root: path.join(examplesRoot, entry.name),
    }))
    .sort((left, right) => left.item.localeCompare(right.item));
}

function extractRustTestNames(value) {
  const names = new Set();
  const text = String(value ?? '');
  const codeSpanPattern = /`([^`]+)`/g;
  let sawCodeSpan = false;

  for (const match of text.matchAll(codeSpanPattern)) {
    sawCodeSpan = true;
    addRustToken(match[1], names);
  }

  if (!sawCodeSpan) {
    addRustToken(text, names);
  }

  return [...names].sort();
}

function addRustToken(value, names) {
  for (const rawPart of String(value).split(/[;,]/)) {
    const part = rawPart.trim();
    if (!part || part === 'none' || part === 'missing' || part.startsWith('exception:')) {
      continue;
    }
    if (/^cargo\s+/.test(part) || part.includes(' --example ')) {
      continue;
    }
    const pathTarget = part.match(/::([A-Za-z_][A-Za-z0-9_]*)$/);
    if (pathTarget) {
      names.add(pathTarget[1]);
      continue;
    }
    const testTarget = part.match(/\btest:\s*([A-Za-z_][A-Za-z0-9_]*)\b/);
    if (testTarget) {
      names.add(testTarget[1]);
      continue;
    }
    if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(part)) {
      names.add(part);
    }
  }
}

function parseFoundationalMappings(docPath) {
  const mappings = {
    byLocation: new Map(),
    byName: new Map(),
  };
  if (!fs.existsSync(docPath)) {
    return mappings;
  }

  const markdown = fs.readFileSync(docPath, 'utf8');
  const lines = markdown.split(/\r?\n/);
  let headers = null;

  for (const line of lines) {
    if (!line.trim().startsWith('|')) {
      continue;
    }
    const cells = splitMarkdownRow(line);
    if (!cells || isSeparator(cells)) {
      continue;
    }
    if (cells.includes('ID') && cells.includes('Upstream case')) {
      headers = cells;
      continue;
    }
    if (!headers || cells.length !== headers.length) {
      continue;
    }

    const values = Object.fromEntries(headers.map((header, index) => [header, cells[index]]));
    const location = values['Upstream case']?.match(/`(packages\/[^`]+):(\d+)`/);
    if (!location) {
      continue;
    }
    const caseName = parseInventoryCaseName(values['Upstream case']);
    const status = stripInlineCode(values.Classification);
    if (!VALID_STATUSES.has(status)) {
      continue;
    }

    const entry = {
      source: 'docs/ai-foundational-provider-inventory.md',
      status,
      rustTarget: values['Rust test / exception'],
      rustTests: status === 'portable-mapped'
        ? extractRustTestNames(values['Rust test / exception'])
        : [],
      notes: values.Notes,
    };
    mappings.byLocation.set(`${location[1]}:${location[2]}`, entry);
    if (caseName) {
      const key = `${location[1]}|${normalizeNameKey(caseName)}`;
      const queue = mappings.byName.get(key) ?? [];
      queue.push(entry);
      mappings.byName.set(key, queue);
    }
  }

  return mappings;
}

function parseInventoryCaseName(upstreamCaseCell) {
  const withoutLocation = String(upstreamCaseCell)
    .replace(/`packages\/[^`]+:\d+`/, '')
    .trim();
  return withoutLocation
    .replace(/^(?:it|test)(?:\.[A-Za-z0-9_$]+)*\s+/, '')
    .replace(/\s+\(\d+ table rows\)$/, '')
    .trim();
}

function parseAi02Mappings(docPath) {
  const mappings = new Map();
  if (!fs.existsSync(docPath)) {
    return mappings;
  }

  const markdown = fs.readFileSync(docPath, 'utf8');
  const lines = markdown.split(/\r?\n/);
  let headers = null;

  for (const line of lines) {
    if (!line.trim().startsWith('|')) {
      headers = null;
      continue;
    }

    const cells = splitMarkdownRow(line);
    if (!cells || isSeparator(cells)) {
      continue;
    }
    if (
      cells.includes('Upstream file') &&
      (cells.includes('Current upstream case') || cells.includes('Current case')) &&
      cells.includes('Rust mapping')
    ) {
      headers = cells;
      continue;
    }
    if (!headers || cells.length !== headers.length) {
      continue;
    }

    const values = Object.fromEntries(headers.map((header, index) => [header, cells[index]]));
    const file = stripInlineCode(values['Upstream file']);
    if (!file.startsWith('packages/')) {
      continue;
    }
    const caseName = normalizeCaseName(
      values['Current upstream case'] ?? values['Current case'],
    );
    const exception = values['Remaining exception'];
    const exceptionStatus = exception.match(/`?(js-only-documented|type-system-impossible)`?/)?.[1];
    const rustTests = extractRustTestNames(values['Rust mapping']);
    const status = rustTests.length > 0
      ? 'portable-mapped'
      : (exceptionStatus ?? 'portable-unmapped');
    const entry = {
      source: 'docs/ai-02-openai-compatible-providers.md',
      status,
      rustTarget: values['Rust mapping'],
      rustTests,
      notes: exception && exception !== 'none'
        ? `${values['Remaining exception']}`
        : 'Mapped by the AI-02 exact case map.',
    };
    for (const keyName of caseNameAliases(caseName)) {
      const key = `${file}|${normalizeNameKey(keyName)}`;
      const queue = mappings.get(key) ?? [];
      queue.push(entry);
      mappings.set(key, queue);
    }
  }

  return mappings;
}

function caseNameAliases(caseName) {
  const names = new Set([caseName]);
  names.add(caseName.replace(/\s+\([^)]*\)$/, '').trim());
  return [...names].filter(Boolean);
}

function collectRustTestNames() {
  const roots = ['src', 'crates', 'examples']
    .map(root => path.join(repositoryRoot, root))
    .filter(root => fs.existsSync(root));
  const files = roots.flatMap(root => walkFiles(root)).filter(file => file.endsWith('.rs'));
  const names = new Set();

  for (const file of files) {
    const source = fs.readFileSync(file, 'utf8');
    const attrPattern = /#\[(?:[A-Za-z_][A-Za-z0-9_]*::)?(?:test|rstest|test_case)(?:[^\]]*)\]/g;
    for (const match of source.matchAll(attrPattern)) {
      const after = source.slice(match.index + match[0].length, match.index + match[0].length + 1200);
      const fnMatch = after.match(/\b(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\b/);
      if (fnMatch) {
        names.add(fnMatch[1]);
      }
    }
  }

  return names;
}

function defaultClassification(testCase, item) {
  if (item.area === 'packages' && item.ledgerStatus === 'js-only-documented') {
    return {
      status: 'js-only-documented',
      rustTarget: 'exception: package row is documented as JavaScript-only',
      rustTests: [],
      notes:
        'The package-level ledger marks this upstream package as JavaScript-only or ecosystem-specific, so its tests are explicit non-portable exceptions.',
      source: 'docs/upstream-parity.md',
    };
  }

  if (testCase.file.endsWith('.test-d.ts') || testCase.file.endsWith('.test-d.tsx')) {
    return {
      status: 'type-system-impossible',
      rustTarget: 'exception: TypeScript compiler-only assertion',
      rustTests: [],
      notes:
        'This upstream case is a TypeScript compile-time type assertion; a Rust closure lane may replace it with a named compile/runtime test when the API behavior is portable.',
      source: 'built-in type-test rule',
    };
  }

  if (/^packages\/ai\/src\/model\/as-[^/]+\.test\.ts$/.test(testCase.file)) {
    return {
      status: 'js-only-documented',
      rustTarget: 'exception: JavaScript provider-version adapter identity',
      rustTests: [],
      notes:
        'The upstream test exercises JavaScript provider object version adapters, prototypes, Web stream instances, property descriptors, and compatibility warnings. Rust exposes current provider traits instead.',
      source: 'docs/upstream-parity.md Provider-v2/v3 compatibility adapters',
    };
  }

  if (testCase.file.startsWith('packages/provider-utils/src/to-json-schema/')) {
    return {
      status: 'js-only-documented',
      rustTarget: 'exception: JavaScript Zod runtime adapter',
      rustTests: [],
      notes:
        'The upstream test exercises Zod runtime internals and Vitest snapshots. Rust-facing portable schema behavior is tracked through JSON Schema and StandardSchema rows instead.',
      source: 'docs/upstream-parity.md Provider-utils Zod JSON-schema adapters',
    };
  }

  if (testCase.file === 'packages/gateway/src/vercel-environment.test.ts') {
    return {
      status: 'js-only-documented',
      rustTarget: 'exception: Vercel JavaScript request-context global',
      rustTests: [],
      notes:
        'The upstream test reads Vercel request context from a JavaScript global symbol. Rust callers pass request identifiers through typed settings and headers.',
      source: 'docs/upstream-parity.md Gateway request-context helper',
    };
  }

  return {
    status: 'portable-unmapped',
    rustTarget: 'missing',
    rustTests: [],
    notes:
      'Portable upstream case still needs a named Rust test or an explicit non-portable exception.',
    source: 'strict inventory default',
  };
}

function classifyCase(testCase, item, foundationalMappings, ai02Mappings) {
  const locationKey = `${testCase.file}:${testCase.line}`;
  const foundational = foundationalMappings.byLocation.get(locationKey);
  if (foundational) {
    return foundational;
  }

  const foundationalKey = `${testCase.file}|${normalizeNameKey(testCase.name)}`;
  const foundationalQueue = foundationalMappings.byName.get(foundationalKey);
  if (foundationalQueue?.length) {
    return foundationalQueue.shift();
  }

  const ai02Key = `${testCase.file}|${normalizeNameKey(testCase.name)}`;
  const ai02Queue = ai02Mappings.get(ai02Key);
  if (ai02Queue?.length) {
    return ai02Queue.shift();
  }

  return defaultClassification(testCase, item);
}

function formatCaseName(testCase) {
  const base = `${testCase.kind} ${testCase.name}`;
  return testCase.tableRows == null ? base : `${base} (${testCase.tableRows} table rows)`;
}

function rowId(item, index) {
  return `${item.item.replace(/[^A-Za-z0-9]+/g, '-').replace(/^-|-$/g, '')}-${String(index + 1).padStart(4, '0')}`;
}

function inventoryItems(upstreamRoot, ledgerPackages) {
  return [
    ...parsePackageJsonPackages(upstreamRoot, ledgerPackages),
    ...discoverExampleRoots(upstreamRoot),
  ];
}

function buildInventory(args) {
  if (!fs.existsSync(args.upstreamRoot)) {
    throw new Error(`upstream root not found: ${args.upstreamRoot}`);
  }

  const ledgerPackages = parseLedgerPackages(args.ledgerPath);
  const foundationalMappings = parseFoundationalMappings(FOUNDATIONAL_INVENTORY_PATH);
  const ai02Mappings = parseAi02Mappings(AI_02_INVENTORY_PATH);
  const rustTests = collectRustTestNames();
  const validationErrors = [];
  const inventories = [];

  for (const item of inventoryItems(args.upstreamRoot, ledgerPackages)) {
    const testFiles = walkFiles(item.root)
      .filter(file => TEST_FILE_PATTERN.test(file))
      .sort();
    const cases = [];

    for (const file of testFiles) {
      const extracted = extractTestCases(file, args.upstreamRoot);
      for (const testCase of extracted) {
        const classification = classifyCase(
          testCase,
          item,
          foundationalMappings,
          ai02Mappings,
        );
        if (!VALID_STATUSES.has(classification.status)) {
          validationErrors.push(
            `${testCase.file}:${testCase.line}: invalid status ${classification.status}`,
          );
        }
        if (classification.status === 'portable-mapped') {
          if (classification.rustTests.length === 0) {
            validationErrors.push(
              `${testCase.file}:${testCase.line}: portable-mapped row has no named Rust test`,
            );
          }
          for (const rustTest of classification.rustTests) {
            if (!rustTests.has(rustTest)) {
              validationErrors.push(
                `${testCase.file}:${testCase.line}: mapped Rust test not found: ${rustTest}`,
              );
            }
          }
        }
        if (EXCEPTION_STATUSES.has(classification.status) && classification.notes.trim() === '') {
          validationErrors.push(
            `${testCase.file}:${testCase.line}: exception row requires notes`,
          );
        }

        cases.push({
          ...testCase,
          id: rowId(item, cases.length),
          status: classification.status,
          rustTarget: classification.rustTarget,
          rustTests: classification.rustTests,
          notes: classification.notes,
          mappingSource: classification.source,
        });
      }
    }

    const statusCounts = countStatuses(cases);
    inventories.push({
      ...item,
      testFiles: testFiles.map(file => relativePath(file, args.upstreamRoot)),
      cases,
      ...statusCounts,
    });
  }

  return {
    inventories,
    validationErrors,
    upstreamRoot: args.upstreamRoot,
  };
}

function countStatuses(cases) {
  const counts = {
    totalCases: cases.length,
    portableMapped: 0,
    portableUnmapped: 0,
    jsOnly: 0,
    typeSystemImpossible: 0,
  };

  for (const testCase of cases) {
    if (testCase.status === 'portable-mapped') {
      counts.portableMapped += 1;
    } else if (testCase.status === 'portable-unmapped') {
      counts.portableUnmapped += 1;
    } else if (testCase.status === 'js-only-documented') {
      counts.jsOnly += 1;
    } else if (testCase.status === 'type-system-impossible') {
      counts.typeSystemImpossible += 1;
    }
  }

  return counts;
}

function md(value) {
  return String(value ?? '')
    .replaceAll('\\', '\\\\')
    .replaceAll('|', '\\|')
    .replace(/\s+/g, ' ')
    .trim();
}

function code(value) {
  return `\`${md(value)}\``;
}

function formatRustTarget(testCase) {
  if (testCase.status === 'portable-mapped') {
    return testCase.rustTests.map(name => code(name)).join('; ');
  }
  return md(testCase.rustTarget);
}

function renderInventory(inventory) {
  const totals = inventory.inventories.reduce((acc, item) => {
    acc.testFiles += item.testFiles.length;
    acc.totalCases += item.totalCases;
    acc.portableMapped += item.portableMapped;
    acc.portableUnmapped += item.portableUnmapped;
    acc.jsOnly += item.jsOnly;
    acc.typeSystemImpossible += item.typeSystemImpossible;
    return acc;
  }, {
    testFiles: 0,
    totalCases: 0,
    portableMapped: 0,
    portableUnmapped: 0,
    jsOnly: 0,
    typeSystemImpossible: 0,
  });
  const portableTotal = totals.portableMapped + totals.portableUnmapped;
  const lines = [];

  lines.push('# AI SDK Strict Test Inventory');
  lines.push('');
  lines.push(`Generated from upstream \`${UPSTREAM_REPO}\` after \`${FETCH_COMMAND}\`.`);
  lines.push('');
  lines.push('| Field | Value |');
  lines.push('| --- | --- |');
  lines.push(`| Upstream commit | ${code(UPSTREAM_HEAD)} |`);
  lines.push(`| Upstream commit date | ${code(UPSTREAM_COMMIT_DATE)} |`);
  lines.push(`| Inventory date | ${code(INVENTORY_DATE)} |`);
  lines.push(`| Local upstream source | ${code(inventory.upstreamRoot)} |`);
  lines.push(`| Test files scanned | ${totals.testFiles} |`);
  lines.push(`| Upstream cases scanned | ${totals.totalCases} |`);
  lines.push(`| Portable cases mapped to named Rust tests | ${totals.portableMapped} |`);
  lines.push(`| Portable cases still missing named Rust tests | ${totals.portableUnmapped} |`);
  lines.push(`| JavaScript-only exceptions | ${totals.jsOnly} |`);
  lines.push(`| Type-system-impossible exceptions | ${totals.typeSystemImpossible} |`);
  lines.push(`| Portable mapped denominator | ${totals.portableMapped} / ${portableTotal} |`);
  lines.push('');
  lines.push('This inventory is intentionally stricter than package-level progress. A package can only claim case-level parity after every portable upstream row below is either `portable-mapped` to a named Rust test or explicitly classified as `js-only-documented` or `type-system-impossible`.');
  lines.push('');
  lines.push('## Package Summary');
  lines.push('');
  lines.push('| Item | Package | Kind | Ledger status | Owner | Test files | Cases | Portable mapped | Portable unmapped | JS-only | Type-system impossible | Sample failing IDs |');
  lines.push('| --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |');
  for (const item of inventory.inventories) {
    if (item.area !== 'packages' && item.totalCases === 0) {
      continue;
    }
    const failingIds = item.cases
      .filter(testCase => testCase.status === 'portable-unmapped')
      .slice(0, 5)
      .map(testCase => code(testCase.id))
      .join('; ');
    lines.push(
      `| ${code(item.item)} | ${code(item.displayName)} | ${md(item.kind)} | ${code(item.ledgerStatus)} | ${md(item.owner)} | ${item.testFiles.length} | ${item.totalCases} | ${item.portableMapped} | ${item.portableUnmapped} | ${item.jsOnly} | ${item.typeSystemImpossible} | ${failingIds || 'none'} |`,
    );
  }
  lines.push('');
  lines.push('## Case Inventory');
  lines.push('');
  lines.push('| ID | Item | Upstream case | Classification | Owner | Rust test / exception | Mapping source | Notes |');
  lines.push('| --- | --- | --- | --- | --- | --- | --- | --- |');
  for (const item of inventory.inventories) {
    for (const testCase of item.cases) {
      const location = `${testCase.file}:${testCase.line}`;
      lines.push(
        `| ${code(testCase.id)} | ${code(item.item)} | ${code(location)} ${md(formatCaseName(testCase))} | ${code(testCase.status)} | ${md(item.owner)} | ${formatRustTarget(testCase)} | ${md(testCase.mappingSource)} | ${md(testCase.notes)} |`,
      );
    }
  }
  lines.push('');

  return `${lines.join('\n')}\n`;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const inventory = buildInventory(args);
  const document = renderInventory(inventory);
  const totalUnmapped = inventory.inventories.reduce(
    (sum, item) => sum + item.portableUnmapped,
    0,
  );

  if (inventory.validationErrors.length > 0) {
    for (const error of inventory.validationErrors) {
      console.error(error);
    }
    throw new Error(`${inventory.validationErrors.length} strict inventory validation errors`);
  }

  if (args.check) {
    const existing = fs.existsSync(args.outputPath)
      ? fs.readFileSync(args.outputPath, 'utf8')
      : '';
    if (existing !== document) {
      console.error(`${relativePath(args.outputPath, repositoryRoot)} is out of date; regenerate it with scripts/ai-strict-test-inventory.mjs --output ${relativePath(args.outputPath, repositoryRoot)}.`);
      process.exit(1);
    }
  } else {
    fs.writeFileSync(args.outputPath, document);
  }

  if (args.failOnUnmapped && totalUnmapped > 0) {
    throw new Error(`${totalUnmapped} portable upstream cases remain unmapped`);
  }

  const totalCases = inventory.inventories.reduce((sum, item) => sum + item.totalCases, 0);
  const mapped = inventory.inventories.reduce((sum, item) => sum + item.portableMapped, 0);
  const exceptions = inventory.inventories.reduce(
    (sum, item) => sum + item.jsOnly + item.typeSystemImpossible,
    0,
  );
  console.log(
    `AI strict test inventory OK: ${totalCases} cases, ${mapped} portable mapped, ${totalUnmapped} portable unmapped, ${exceptions} exceptions.`,
  );
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
