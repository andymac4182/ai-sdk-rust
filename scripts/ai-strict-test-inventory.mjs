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
const AI_CORE_INVENTORY_PATH = path.join(
  repositoryRoot,
  'docs/ai-core-package-inventory.md',
);
const AI_02_INVENTORY_PATH = path.join(
  repositoryRoot,
  'docs/ai-02-openai-compatible-providers.md',
);
const AI_06_INVENTORY_PATH = path.join(
  repositoryRoot,
  'docs/ai-06-concrete-provider-mappings.md',
);
const AI_05_INVENTORY_PATH = path.join(
  repositoryRoot,
  'docs/ai-05-mcp-otel-provider-inventory.md',
);
const AI_04_INVENTORY_PATH = path.join(
  repositoryRoot,
  'docs/ai-04-openai-strict-provider-closure.md',
);

const UPSTREAM_REPO = 'vercel/ai';
const UPSTREAM_HEAD = '43e84c8e39e540aa23e25986031183227a77d531';
const UPSTREAM_COMMIT_DATE = '2026-06-01T20:12:00Z';
const INVENTORY_DATE = '2026-06-02';
const FETCH_COMMAND = 'npx opensrc fetch https://github.com/vercel/ai';

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
const STRICT_PACKAGE_OVERRIDES = new Map([
  [
    'mcp',
    {
      kind: 'MCP transport package',
      ledgerStatus: 'in-progress',
      owner: 'crates/ai-sdk-mcp',
    },
  ],
  [
    'workflow',
    {
      kind: 'AI SDK workflow package',
      ledgerStatus: 'verified',
      owner: 'crates/ai-sdk-workflow; crates/workflow facade',
    },
  ],
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
    const override = STRICT_PACKAGE_OVERRIDES.get(packageDir);
    return {
      item: `packages/${packageDir}`,
      area: 'packages',
      packageDir,
      displayName: parsed.name ?? ledger?.displayName ?? packageDir,
      kind: ledger?.kind ?? override?.kind ?? 'upstream package',
      ledgerStatus: ledger?.ledgerStatus ?? override?.ledgerStatus ?? 'not-started',
      owner: ledger?.owner ?? override?.owner ?? 'unassigned',
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

function parseFoundationalMappings(
  docPath,
  source = relativePath(docPath, repositoryRoot),
) {
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

    const locationKey = `${location[1]}:${location[2]}`;
    const entry = {
      source,
      status,
      rustTarget: values['Rust test / exception'],
      rustTests: status === 'portable-mapped'
        ? extractRustTestNames(values['Rust test / exception'])
        : [],
      notes: values.Notes,
      locationKey,
    };
    mappings.byLocation.set(locationKey, entry);
    if (caseName) {
      const key = `${location[1]}|${normalizeNameKey(caseName)}`;
      const queue = mappings.byName.get(key) ?? [];
      queue.push(entry);
      mappings.byName.set(key, queue);
    }
  }

  return mappings;
}

function mergeStrictCaseMappings(docPaths) {
  const merged = {
    byLocation: new Map(),
    byName: new Map(),
  };

  for (const docPath of docPaths) {
    const mappings = parseFoundationalMappings(docPath);
    for (const [key, entry] of mappings.byLocation) {
      merged.byLocation.set(key, entry);
    }
    for (const [key, entries] of mappings.byName) {
      const queue = merged.byName.get(key) ?? [];
      queue.push(...entries);
      merged.byName.set(key, queue);
    }
  }

  return merged;
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

function consumeExactNameFallback(mappings, testCase, entry) {
  const key = `${testCase.file}|${normalizeNameKey(testCase.name)}`;
  const queue = mappings.byName.get(key);
  if (!queue?.length) {
    return;
  }
  const index = queue.indexOf(entry);
  if (index !== -1) {
    queue.splice(index, 1);
  }
}

function shiftNameFallback(mappings, testCase, currentCaseLocations) {
  const key = `${testCase.file}|${normalizeNameKey(testCase.name)}`;
  const queue = mappings.byName.get(key);
  if (!queue?.length) {
    return null;
  }
  const index = queue.findIndex(
    entry => !entry.locationKey || !currentCaseLocations.has(entry.locationKey),
  );
  if (index === -1) {
    return null;
  }
  return queue.splice(index, 1)[0];
}

function parseAi02Mappings(
  docPath,
  sourceLabel = relativePath(docPath, repositoryRoot),
  defaultNote = 'Mapped by the AI-02 exact case map.',
) {
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
      source: sourceLabel,
      status,
      rustTarget: values['Rust mapping'],
      rustTests,
      notes: exception && exception !== 'none'
        ? `${values['Remaining exception']}`
        : defaultNote,
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

function mergeNamedCaseMappings(...mappingSets) {
  const merged = new Map();
  for (const mappings of mappingSets) {
    for (const [key, entries] of mappings) {
      const queue = merged.get(key) ?? [];
      queue.push(...entries);
      merged.set(key, queue);
    }
  }
  return merged;
}

function exactCaseMappingValidationErrors(mappings, requiredSource) {
  const errors = [];
  for (const [key, entries] of mappings) {
    const unusedEntries = entries.filter(entry =>
      !entry.used && (!requiredSource || entry.source === requiredSource)
    );
    if (unusedEntries.length === 0) {
      continue;
    }
    const sources = [...new Set(unusedEntries.map(entry => entry.source))].join(', ');
    errors.push(`${key}: unused exact-case mappings from ${sources}`);
  }
  return errors;
}

function caseNameAliases(caseName) {
  const names = new Set([caseName]);
  names.add(caseName.replace(/\s+\([^)]*\)$/, '').trim());
  return [...names].filter(Boolean);
}

const GATEWAY_VERCEL_MAPPING_SOURCE = 'AIS-03 Gateway/Vercel strict case map';

function gatewayVercelException(testCase) {
  const key = `${testCase.file}:${testCase.line}`;
  const exceptions = {
    'packages/gateway/src/errors/as-gateway-error.test.ts:66': {
      status: 'js-only-documented',
      rustTarget: 'exception: JavaScript unknown-error GatewayError instance identity',
      notes:
        'The upstream case passes an existing GatewayError through an unknown JavaScript error boundary and asserts object identity. Rust routes GatewayError through typed results instead of erased runtime class checks.',
    },
    'packages/gateway/src/errors/gateway-error-types.test.ts:284': {
      status: 'js-only-documented',
      rustTarget: 'exception: JavaScript symbol-based cross-realm error marker',
      notes:
        'The upstream case verifies JavaScript Symbol.for markers used for cross-realm class detection. Rust error variants are statically typed and do not expose JavaScript symbol markers.',
    },
    'packages/gateway/src/errors/gateway-error-types.test.ts:304': {
      status: 'js-only-documented',
      rustTarget: 'exception: JavaScript Error inheritance chain',
      notes:
        'The upstream case asserts JavaScript instanceof inheritance. Rust represents Gateway errors with concrete error structs and enum variants instead of JavaScript prototype chains.',
    },
    'packages/gateway/src/errors/gateway-error-types.test.ts:312': {
      status: 'js-only-documented',
      rustTarget: 'exception: JavaScript Error stack trace formatting',
      notes:
        'The upstream case asserts JavaScript stack trace contents. Rust error values expose messages, types, status codes, retryability, causes, and generation IDs without JavaScript stack strings.',
    },
    'packages/gateway/src/gateway-language-model.test.ts:1250': {
      status: 'js-only-documented',
      rustTarget: 'exception: JavaScript Date object identity',
      notes:
        'The upstream stream mapper preserves an already-created JavaScript Date object. Rust parses response-metadata timestamps into OffsetDateTime and has no JavaScript Date object identity to preserve.',
    },
    'packages/gateway/src/gateway-provider.test.ts:239': {
      status: 'type-system-impossible',
      rustTarget: 'exception: JavaScript callable provider constructor misuse',
      notes:
        'The upstream case rejects using the callable provider factory with new. Rust exposes constructors and builder methods through the type system, so this JavaScript misuse is not expressible.',
    },
  };

  return exceptions[key] ?? null;
}

function gatewayVercelMappedClassification(testCase, rustTests) {
  if (
    !testCase.file.startsWith('packages/gateway/') &&
    !testCase.file.startsWith('packages/vercel/')
  ) {
    return null;
  }

  const exception = gatewayVercelException(testCase);
  if (exception) {
    return {
      source: GATEWAY_VERCEL_MAPPING_SOURCE,
      rustTests: [],
      ...exception,
    };
  }

  const mappedRustTests = gatewayVercelRustTests(testCase, rustTests);
  if (!mappedRustTests) {
    return null;
  }

  return {
    source: GATEWAY_VERCEL_MAPPING_SOURCE,
    status: 'portable-mapped',
    rustTarget: mappedRustTests.join('; '),
    rustTests: mappedRustTests,
    notes:
      'Mapped by AIS-03 from the refreshed upstream Gateway/Vercel provider case to deterministic Rust tests using fake transports and fixtures.',
  };
}

function gatewayVercelRustTests(testCase, rustTests) {
  const file = testCase.file;
  const line = testCase.line;
  const key = `${file}:${line}`;

  const exact = {
    'packages/gateway/src/gateway-embedding-model.test.ts:104': [
      'gateway_embedding_model_sends_values_as_array',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:65': [
      'gateway_fetch_metadata_fetches_available_models_from_correct_endpoint',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:95': [
      'gateway_fetch_metadata_maps_cache_pricing_fields_to_sdk_names',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:201': [
      'gateway_fetch_metadata_filters_unknown_model_type_values',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:257': [
      'gateway_fetch_metadata_keeps_known_models_and_filters_unknown_from_mixed_response',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:414': [
      'gateway_fetch_metadata_does_not_double_wrap_existing_gateway_errors',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:434': [
      'gateway_fetch_metadata_handles_rate_limit_server_errors',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:461': [
      'gateway_fetch_metadata_handles_internal_server_errors',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:692': [
      'gateway_fetch_metadata_uses_custom_fetch_function_for_credits',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:730': [
      'gateway_fetch_metadata_converts_credits_api_call_errors_to_gateway_errors',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:756': [
      'gateway_fetch_metadata_handles_credits_malformed_json_error_responses',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:776': [
      'gateway_fetch_metadata_does_not_double_wrap_existing_credit_gateway_errors',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:794': [
      'gateway_fetch_metadata_preserves_credits_error_cause_chain',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:818': [
      'gateway_fetch_metadata_handles_empty_credits_response',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:571': [
      'gateway_fetch_metadata_fetches_credits_from_correct_endpoint',
    ],
    'packages/gateway/src/gateway-generation-info.test.ts:59': [
      'gateway_provider_generation_info_fetches_from_correct_endpoint_with_generation_id',
    ],
    'packages/gateway/src/gateway-generation-info.test.ts:101': [
      'gateway_provider_generation_info_unwraps_data_envelope',
    ],
    'packages/gateway/src/gateway-generation-info.test.ts:113': [
      'gateway_provider_generation_info_omits_snake_case_fields_from_serialized_result',
    ],
    'packages/gateway/src/gateway-generation-info.test.ts:214': [
      'gateway_provider_generation_info_uses_custom_transport',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:298': [
      'gateway_language_model_does_not_modify_prompt_without_image_parts_for_generate',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:47': [
      'gateway_language_model_sets_basic_properties',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:129': [
      'gateway_language_model_removes_abort_signal_from_request_body',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:180': [
      'gateway_language_model_includes_o11y_headers_in_request',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:309': [
      'gateway_language_model_encodes_uint8_array_image_part_to_inline_base64_data_with_default_mime_type_for_generate',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:343': [
      'gateway_language_model_encodes_uint8_array_image_part_to_inline_base64_data_with_specified_mime_type_for_generate',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:377': [
      'gateway_language_model_does_not_modify_image_part_with_url_for_generate',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:409': [
      'gateway_language_model_handles_mixed_content_types_correctly_for_generate',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:853': [
      'gateway_language_model_does_not_modify_prompt_without_image_parts_for_streaming',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:673': [
      'gateway_language_model_removes_abort_signal_from_streaming_request_body',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:733': [
      'gateway_language_model_includes_o11y_headers_in_streaming_request',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:865': [
      'gateway_language_model_encodes_uint8_array_image_part_to_inline_base64_data_with_default_mime_type_for_streaming',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:900': [
      'gateway_language_model_encodes_uint8_array_image_part_to_inline_base64_data_with_specified_mime_type_for_streaming',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:936': [
      'gateway_language_model_does_not_modify_image_part_with_url_for_streaming',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:970': [
      'gateway_language_model_handles_mixed_content_types_correctly_for_streaming',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:1019': [
      'gateway_model_preserves_structured_gateway_error_metadata',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:1041': [
      'gateway_model_classifies_transport_timeout_errors',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:1060': [
      'gateway_model_stream_classifies_transport_timeout_errors',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:1082': [
      'gateway_model_preserves_structured_gateway_error_metadata',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:1211': [
      'gateway_language_model_converts_timestamp_strings_to_offset_date_time_in_response_metadata_chunks',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:1283': [
      'gateway_language_model_preserves_response_metadata_without_timestamp',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:1313': [
      'gateway_language_model_handles_null_response_metadata_timestamp_gracefully',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:1343': [
      'gateway_language_model_ignores_extra_timestamp_fields_on_non_metadata_stream_parts',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:1442': [
      'gateway_language_model_passes_provider_routing_order_for_generate',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:1499': [
      'gateway_language_model_passes_provider_routing_order_for_stream',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:1541': [
      'gateway_language_model_passes_provider_timeouts_for_generate',
    ],
    'packages/gateway/src/gateway-language-model.test.ts:1567': [
      'gateway_language_model_passes_provider_timeouts_for_stream',
    ],
    'packages/gateway/src/gateway-provider.test.ts:183': [
      'create_gateway_language_model_uses_custom_configuration',
    ],
    'packages/gateway/src/gateway-provider.test.ts:217': [
      'create_gateway_language_model_uses_oidc_when_api_key_is_absent',
    ],
    'packages/gateway/src/gateway-provider.test.ts:254': [
      'create_gateway_embedding_model_returns_gateway_embedding_model',
    ],
    'packages/gateway/src/gateway-provider.test.ts:263': [
      'create_gateway_image_model_uses_custom_base_url',
    ],
    'packages/gateway/src/gateway-provider.test.ts:280': [
      'create_gateway_image_model_reuses_headers_transport_and_observability',
    ],
    'packages/gateway/src/gateway-provider.test.ts:311': [
      'create_gateway_video_model_uses_custom_base_url',
    ],
    'packages/gateway/src/gateway-provider.test.ts:328': [
      'create_gateway_video_model_reuses_headers_transport_and_observability',
    ],
    'packages/gateway/src/gateway-provider.test.ts:359': [
      'create_gateway_reranking_model_uses_custom_base_url',
    ],
    'packages/gateway/src/gateway-provider.test.ts:376': [
      'create_gateway_reranking_alias_returns_gateway_reranking_model',
    ],
    'packages/gateway/src/gateway-provider.test.ts:389': [
      'create_gateway_fetches_available_models_with_custom_base_url',
    ],
    'packages/gateway/src/gateway-provider.test.ts:409': [
      'create_gateway_caches_metadata_for_configured_refresh_interval',
    ],
    'packages/gateway/src/gateway-provider.test.ts:442': [
      'create_gateway_uses_default_five_minute_metadata_refresh_interval',
    ],
    'packages/gateway/src/gateway-provider.test.ts:471': [
      'create_gateway_language_model_passes_observability_headers_from_environment',
    ],
    'packages/gateway/src/gateway-provider.test.ts:514': [
      'create_gateway_language_model_omits_missing_observability_headers',
    ],
    'packages/gateway/src/gateway-provider.test.ts:552': [
      'default_gateway_export_exposes_provider_instance',
    ],
    'packages/gateway/src/gateway-provider.test.ts:559': [
      'create_gateway_uses_default_base_url_when_none_is_provided',
    ],
    'packages/gateway/src/gateway-provider.test.ts:579': [
      'create_gateway_accepts_empty_options',
    ],
    'packages/gateway/src/gateway-provider.test.ts:587': [
      'default_gateway_export_constructs_image_model',
    ],
    'packages/gateway/src/gateway-provider.test.ts:600': [
      'default_gateway_export_constructs_video_model',
    ],
    'packages/gateway/src/gateway-provider.test.ts:613': [
      'create_gateway_overrides_default_base_url_when_provided',
    ],
    'packages/gateway/src/gateway-provider.test.ts:638': [
      'create_gateway_prefers_api_key_over_oidc_token',
    ],
    'packages/gateway/src/gateway-provider.test.ts:793': [
      'get_gateway_auth_token_matches_upstream_precedence',
      'get_gateway_auth_token_handles_no_auth_at_all',
      'get_gateway_auth_token_handles_valid_oidc_invalid_api_key',
      'get_gateway_auth_token_handles_invalid_oidc_valid_api_key',
      'get_gateway_auth_token_handles_no_oidc_invalid_api_key',
      'get_gateway_auth_token_handles_no_oidc_valid_api_key',
      'get_gateway_auth_token_handles_valid_oidc_no_api_key',
      'get_gateway_auth_token_handles_valid_oidc_valid_api_key',
      'get_gateway_auth_token_handles_valid_oidc_valid_options_api_key',
      'get_gateway_auth_token_handles_invalid_oidc_invalid_api_key',
    ],
    'packages/gateway/src/gateway-provider.test.ts:858': [
      'create_gateway_authentication_handles_no_auth_at_all',
      'create_gateway_authentication_handles_valid_oidc_invalid_api_key',
      'create_gateway_authentication_handles_invalid_oidc_valid_api_key',
      'create_gateway_authentication_handles_no_oidc_invalid_api_key',
      'create_gateway_authentication_handles_no_oidc_valid_api_key',
      'create_gateway_authentication_handles_valid_oidc_no_api_key',
      'create_gateway_authentication_handles_valid_oidc_valid_api_key',
      'create_gateway_authentication_handles_valid_oidc_valid_options_api_key',
      'create_gateway_authentication_handles_invalid_oidc_invalid_api_key',
    ],
    'packages/gateway/src/gateway-provider.test.ts:940': [
      'get_gateway_auth_token_treats_empty_environment_variables_as_missing',
    ],
    'packages/gateway/src/gateway-provider.test.ts:956': [
      'get_gateway_auth_token_uses_whitespace_environment_api_key',
    ],
    'packages/gateway/src/gateway-provider.test.ts:969': [
      'get_gateway_auth_token_prioritizes_options_api_key_over_all_environment_variables',
    ],
    'packages/gateway/src/gateway-provider.test.ts:984': [
      'gateway_authentication_contextual_messages_match_auth_source',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1033': [
      'get_gateway_auth_token_prefers_options_api_key_over_ai_gateway_api_key',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1047': [
      'get_gateway_auth_token_prefers_ai_gateway_api_key_over_oidc_token',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1061': [
      'get_gateway_auth_token_falls_back_to_oidc_when_no_api_keys_are_available',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1078': [
      'gateway_provider_real_world_vercel_deployment_uses_oidc_authentication',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1102': [
      'gateway_provider_real_world_local_development_uses_api_key_authentication',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1116': [
      'gateway_provider_real_world_explicit_api_key_override_wins_over_environment',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1138': [
      'gateway_provider_get_credits_fetches_successfully',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1158': [
      'gateway_provider_get_credits_handles_authentication_errors',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1165': [
      'gateway_provider_get_credits_uses_custom_base_url',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1181': [
      'gateway_provider_get_credits_uses_oidc_authentication_headers',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1192': [
      'gateway_provider_get_credits_surfaces_endpoint_errors',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1205': [
      'gateway_provider_get_credits_includes_upstream_headers',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1225': [
      'gateway_provider_get_credits_is_available_on_provider_interface',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1232': [
      'gateway_provider_get_spend_report_fetches_successfully',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1257': [
      'gateway_provider_get_spend_report_passes_params_through',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1283': [
      'gateway_provider_get_spend_report_uses_custom_base_url',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1302': [
      'gateway_provider_get_spend_report_uses_custom_transport',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1321': [
      'gateway_provider_get_spend_report_surfaces_endpoint_errors',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1337': [
      'gateway_provider_get_spend_report_is_available_on_provider_interface',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1342': [
      'default_gateway_export_get_spend_report_is_available',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1348': [
      'gateway_provider_metadata_fetch_errors_convert_to_gateway_errors',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1368': [
      'gateway_provider_metadata_gateway_errors_are_not_double_wrapped',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1395': [
      'gateway_provider_language_model_handles_model_specification_errors',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1432': [
      'gateway_provider_language_model_accepts_any_model_id',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1478': [
      'gateway_provider_language_model_accepts_non_existent_model_id',
    ],
    'packages/gateway/src/gateway-reranking-model.test.ts:191': [
      'gateway_reranking_model_maps_invalid_request_error_response',
    ],
    'packages/gateway/src/gateway-reranking-model.test.ts:214': [
      'gateway_reranking_model_maps_internal_server_error_response',
    ],
    'packages/gateway/src/gateway-spend-report.test.ts:39': [
      'gateway_provider_spend_report_fetches_from_correct_endpoint_with_required_params',
    ],
    'packages/gateway/src/gateway-spend-report.test.ts:97': [
      'gateway_provider_spend_report_omits_empty_tags_query_param',
    ],
    'packages/gateway/src/gateway-spend-report.test.ts:149': [
      'gateway_provider_spend_report_transforms_credential_type_response_field',
    ],
    'packages/gateway/src/gateway-spend-report.test.ts:363': [
      'gateway_provider_spend_report_uses_custom_transport',
    ],
    'packages/gateway/src/gateway-video-model.test.ts:670': [
      'gateway_video_model_ignores_sse_heartbeat_comments_and_parses_data_event',
    ],
    'packages/vercel/src/vercel-provider.test.ts:40': [
      'vercel_provider_uses_vercel_api_key_environment_when_api_key_omitted',
      'vercel_provider_uses_default_base_url_and_function_alias',
    ],
    'packages/vercel/src/vercel-provider.test.ts:57': [
      'vercel_provider_creates_openai_compatible_chat_model',
    ],
    'packages/vercel/src/vercel-provider.test.ts:78': [
      'vercel_provider_uses_default_base_url_and_function_alias',
    ],
    'packages/vercel/src/vercel-provider.test.ts:87': [
      'vercel_provider_creates_openai_compatible_chat_model',
      'vercel_provider_implements_provider_trait',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:259': [
      'gateway_fetch_metadata_keeps_known_models_and_filters_unknown_from_mixed_response',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:416': [
      'gateway_fetch_metadata_does_not_double_wrap_existing_gateway_errors',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:436': [
      'gateway_fetch_metadata_handles_rate_limit_server_errors',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:573': [
      'gateway_fetch_metadata_fetches_credits_from_correct_endpoint',
    ],
    'packages/gateway/src/gateway-fetch-metadata.test.ts:778': [
      'gateway_fetch_metadata_does_not_double_wrap_existing_credit_gateway_errors',
    ],
    'packages/gateway/src/gateway-provider.test.ts:297': [
      'create_gateway_language_model_uses_oidc_when_api_key_is_absent',
    ],
    'packages/gateway/src/gateway-provider.test.ts:423': [
      'create_gateway_embedding_model_returns_gateway_embedding_model',
    ],
    'packages/gateway/src/gateway-provider.test.ts:432': [
      'create_gateway_image_model_uses_custom_base_url',
    ],
    'packages/gateway/src/gateway-provider.test.ts:449': [
      'create_gateway_image_model_reuses_headers_transport_and_observability',
    ],
    'packages/gateway/src/gateway-provider.test.ts:480': [
      'create_gateway_video_model_uses_custom_base_url',
    ],
    'packages/gateway/src/gateway-provider.test.ts:497': [
      'create_gateway_video_model_reuses_headers_transport_and_observability',
    ],
    'packages/gateway/src/gateway-provider.test.ts:528': [
      'create_gateway_reranking_model_uses_custom_base_url',
    ],
    'packages/gateway/src/gateway-provider.test.ts:545': [
      'create_gateway_reranking_alias_returns_gateway_reranking_model',
    ],
    'packages/gateway/src/gateway-provider.test.ts:674': [
      'gateway_provider_fetches_available_models_metadata',
    ],
    'packages/gateway/src/gateway-provider.test.ts:694': [
      'gateway_provider_caches_available_models_until_refresh',
    ],
    'packages/gateway/src/gateway-provider.test.ts:728': [
      'gateway_provider_uses_default_metadata_cache_refresh_interval',
    ],
    'packages/gateway/src/gateway-provider.test.ts:758': [
      'create_gateway_language_model_passes_observability_headers_from_environment',
    ],
    'packages/gateway/src/gateway-provider.test.ts:801': [
      'create_gateway_language_model_omits_missing_observability_headers',
    ],
    'packages/gateway/src/gateway-provider.test.ts:839': [
      'default_gateway_export_exposes_provider_instance',
    ],
    'packages/gateway/src/gateway-provider.test.ts:846': [
      'create_gateway_uses_default_base_url_when_none_is_provided',
    ],
    'packages/gateway/src/gateway-provider.test.ts:866': [
      'create_gateway_accepts_empty_options',
    ],
    'packages/gateway/src/gateway-provider.test.ts:874': [
      'default_gateway_export_constructs_image_model',
    ],
    'packages/gateway/src/gateway-provider.test.ts:887': [
      'default_gateway_export_constructs_video_model',
    ],
    'packages/gateway/src/gateway-provider.test.ts:928': [
      'create_gateway_overrides_default_base_url_when_provided',
    ],
    'packages/gateway/src/gateway-provider.test.ts:953': [
      'create_gateway_prefers_api_key_over_oidc_token',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1286': [
      'get_gateway_auth_token_treats_empty_environment_variables_as_missing',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1315': [
      'get_gateway_auth_token_prioritizes_options_api_key_over_all_environment_variables',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1363': [
      'get_gateway_auth_token_prefers_options_api_key_over_ai_gateway_api_key',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1377': [
      'get_gateway_auth_token_prefers_ai_gateway_api_key_over_oidc_token',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1391': [
      'get_gateway_auth_token_falls_back_to_oidc_when_no_api_keys_are_available',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1408': [
      'gateway_provider_real_world_vercel_deployment_uses_oidc_authentication',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1439': [
      'gateway_provider_real_world_local_development_uses_api_key_authentication',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1453': [
      'gateway_provider_real_world_explicit_api_key_override_wins_over_environment',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1475': [
      'gateway_provider_get_credits_fetches_successfully',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1495': [
      'gateway_provider_get_credits_uses_custom_base_url',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1511': [
      'gateway_provider_get_credits_uses_oidc_authentication_headers',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1522': [
      'gateway_provider_get_credits_surfaces_endpoint_errors',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1535': [
      'gateway_provider_get_credits_includes_upstream_headers',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1555': [
      'gateway_provider_get_credits_is_available_on_provider_interface',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1562': [
      'gateway_provider_get_spend_report_fetches_successfully',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1587': [
      'gateway_provider_get_spend_report_passes_params_through',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1613': [
      'gateway_provider_get_spend_report_uses_custom_base_url',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1632': [
      'gateway_provider_get_spend_report_uses_custom_transport',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1651': [
      'gateway_provider_get_spend_report_surfaces_endpoint_errors',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1681': [
      'gateway_provider_get_spend_report_is_available_on_provider_interface',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1686': [
      'default_gateway_export_get_spend_report_is_available',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1692': [
      'gateway_provider_fetches_generation_info_and_unwraps_data',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1721': [
      'gateway_provider_metadata_fetch_errors_convert_to_gateway_errors',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1741': [
      'gateway_provider_metadata_gateway_errors_are_not_double_wrapped',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1768': [
      'gateway_provider_language_model_handles_model_specification_errors',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1805': [
      'gateway_provider_language_model_accepts_any_model_id',
    ],
    'packages/gateway/src/gateway-provider.test.ts:1851': [
      'gateway_provider_language_model_accepts_non_existent_model_id',
    ],
  };

  if (exact[key]) {
    return exact[key];
  }

  const grouped = gatewayVercelGroupedRustTests(testCase);
  if (grouped) {
    return grouped;
  }

  const inferred = gatewayVercelInferredRustTest(testCase, rustTests);
  return inferred ? [inferred] : null;
}

function gatewayVercelGroupedRustTests(testCase) {
  const line = testCase.line;

  if (testCase.file === 'packages/gateway/src/errors/as-gateway-error.test.ts') {
    if ([12, 23, 33, 102, 112, 122].includes(line)) {
      return ['as_gateway_error_detects_all_undici_timeout_codes'];
    }
    if ([45, 55, 76, 86, 93, 152].includes(line)) {
      return ['as_gateway_error_maps_non_timeout_original_errors_to_response_errors'];
    }
    if (line === 134) {
      return ['as_gateway_error_maps_handled_fetch_errors'];
    }
  }

  if (testCase.file === 'packages/gateway/src/errors/create-gateway-error.test.ts') {
    if ([13, 32, 50, 68, 108, 126, 349].includes(line)) {
      return ['create_gateway_error_from_response_maps_gateway_error_types'];
    }
    if ([90, 368].includes(line)) {
      return ['create_gateway_error_from_response_handles_model_not_found_param_edges'];
    }
    if (line === 146) {
      return ['create_gateway_error_from_response_preserves_empty_auth_messages_with_context'];
    }
    if (line === 163) {
      return ['create_gateway_error_from_response_uses_default_message_for_null_message'];
    }
    if (line === 189) {
      return ['create_gateway_error_from_response_handles_null_error_type_as_internal'];
    }
    if (line === 205) {
      return ['create_gateway_error_from_response_includes_cause_message'];
    }
    if ([225, 247, 269, 288, 307, 328].includes(line)) {
      return ['create_gateway_error_from_response_maps_malformed_responses'];
    }
    if (line === 386) {
      return ['create_gateway_error_from_response_ignores_extra_fields'];
    }
    if (line === 407) {
      return ['create_gateway_error_from_response_preserves_error_properties'];
    }
    if ([433, 451, 470, 488, 507, 523].includes(line)) {
      return ['create_gateway_error_from_response_maps_generation_id_to_error_variants'];
    }
    if ([540, 558, 576].includes(line)) {
      return ['create_gateway_error_from_response_creates_contextual_auth_errors'];
    }
  }

  if (testCase.file === 'packages/gateway/src/errors/extract-api-call-response.test.ts') {
    if (line === 7) {
      return ['extract_gateway_api_call_response_prefers_data_then_json_then_raw_body'];
    }
    if ([26, 42].includes(line)) {
      return ['extract_gateway_api_call_response_prefers_explicit_data_even_null_or_empty'];
    }
    if ([61, 80, 97, 115, 131, 148].includes(line)) {
      return ['extract_gateway_api_call_response_parses_json_or_returns_raw_text'];
    }
    if ([187, 203].includes(line)) {
      return ['extract_gateway_api_call_response_returns_empty_object_without_body'];
    }
    if ([221, 237, 253].includes(line)) {
      return ['extract_gateway_api_call_response_parses_scalar_and_array_bodies'];
    }
  }

  if (testCase.file === 'packages/gateway/src/errors/gateway-error-types.test.ts') {
    if (line >= 14 && line <= 39) {
      return ['gateway_authentication_error_matches_default_and_custom_upstream_values'];
    }
    if (line >= 49 && line <= 94) {
      return ['gateway_authentication_contextual_error_matches_upstream_matrix'];
    }
    if (line >= 109 && line <= 128) {
      return ['gateway_invalid_request_error_matches_default_custom_and_variant_checks'];
    }
    if (line >= 138 && line <= 147) {
      return ['gateway_rate_limit_error_matches_default_and_variant_checks'];
    }
    if (line >= 156 && line <= 176) {
      return ['gateway_model_not_found_error_matches_default_custom_and_variant_checks'];
    }
    if (line >= 185 && line <= 194) {
      return ['gateway_internal_server_error_matches_default_and_variant_checks'];
    }
    if (line >= 203 && line <= 238) {
      return ['gateway_retryability_matches_upstream_status_matrix'];
    }
    if (line >= 245 && line <= 275) {
      return ['gateway_response_error_matches_default_custom_and_variant_checks'];
    }
  }

  if (testCase.file === 'packages/gateway/src/errors/parse-auth-method.test.ts') {
    if (line === 9) {
      return ['gateway_auth_method_header_matches_upstream_name'];
    }
    if (line === 13) {
      return ['vercel_ai_gateway_team_header_matches_upstream_name'];
    }
    if ([20, 30, 40].includes(line)) {
      return ['parse_gateway_auth_method_accepts_valid_values_and_extra_headers'];
    }
    if ([55, 62, 69, 76, 83, 90].includes(line)) {
      return ['parse_gateway_auth_method_rejects_invalid_values'];
    }
    if ([99, 106, 113, 134].includes(line)) {
      return ['parse_gateway_auth_method_returns_none_for_missing_or_nullish_headers'];
    }
    if ([120, 127].includes(line)) {
      return ['parse_gateway_auth_method_rejects_whitespace'];
    }
  }

  return null;
}

const GATEWAY_VERCEL_PREFIXES = {
  'packages/gateway/src/gateway-embedding-model.test.ts': 'gateway_embedding_model',
  'packages/gateway/src/gateway-fetch-metadata.test.ts': 'gateway_fetch_metadata',
  'packages/gateway/src/gateway-generation-info.test.ts': 'gateway_provider_generation_info',
  'packages/gateway/src/gateway-image-model.test.ts': 'gateway_image_model',
  'packages/gateway/src/gateway-language-model.test.ts': 'gateway_language_model',
  'packages/gateway/src/gateway-reranking-model.test.ts': 'gateway_reranking_model',
  'packages/gateway/src/gateway-spend-report.test.ts': 'gateway_provider_spend_report',
  'packages/gateway/src/gateway-video-model.test.ts': 'gateway_video_model',
};

function gatewayVercelInferredRustTest(testCase, rustTests) {
  const prefix = GATEWAY_VERCEL_PREFIXES[testCase.file];
  if (!prefix) {
    return null;
  }

  const base = gatewayVercelSnakeName(testCase.name.replace(/^should\s+/, ''));
  const candidates = [];
  for (const candidateBase of gatewayVercelCandidateBaseNames(base)) {
    for (const verbForm of gatewayVercelVerbForms(candidateBase)) {
      candidates.push(`${prefix}_${verbForm}`);
    }
  }

  return candidates.find(candidate => rustTests.has(candidate)) ?? null;
}

function gatewayVercelCandidateBaseNames(base) {
  const candidates = new Set([base]);
  candidates.add(base.replace(/date_objects?/g, 'offset_date_time'));
  candidates.add(base.replace(/o11y/g, 'observability'));
  candidates.add(base.replace(/^not_include_/, 'omit_'));
  candidates.add(base.replace(/^not_pass_/, 'not_pass_'));
  candidates.add(base.replace(/^post_to_/, 'posts_to_'));
  return [...candidates].filter(Boolean);
}

function gatewayVercelVerbForms(base) {
  const forms = new Set([base]);
  for (const [pattern, replacement] of [
    [/^accept_/, 'accepts_'],
    [/^avoid_/, 'avoids_'],
    [/^cache_/, 'caches_'],
    [/^convert_/, 'converts_'],
    [/^create_/, 'creates_'],
    [/^encode_/, 'encodes_'],
    [/^extract_/, 'extracts_'],
    [/^fetch_/, 'fetches_'],
    [/^filter_/, 'filters_'],
    [/^handle_/, 'handles_'],
    [/^ignore_/, 'ignores_'],
    [/^include_/, 'includes_'],
    [/^merge_/, 'merges_'],
    [/^omit_/, 'omits_'],
    [/^parse_/, 'parses_'],
    [/^pass_/, 'passes_'],
    [/^preserve_/, 'preserves_'],
    [/^reject_/, 'rejects_'],
    [/^remove_/, 'removes_'],
    [/^return_/, 'returns_'],
    [/^send_/, 'sends_'],
    [/^serialize_/, 'serializes_'],
    [/^stream_/, 'streams_'],
    [/^throw_/, 'throws_'],
    [/^transform_/, 'transforms_'],
    [/^unwrap_/, 'unwraps_'],
    [/^use_/, 'uses_'],
    [/^validate_/, 'validates_'],
    [/^work_/, 'works_'],
    [/^not_include_/, 'does_not_include_'],
    [/^not_modify_/, 'does_not_modify_'],
    [/^not_pass_/, 'does_not_pass_'],
  ]) {
    if (pattern.test(base)) {
      forms.add(base.replace(pattern, replacement));
    }
  }
  return [...forms];
}

function gatewayVercelSnakeName(value) {
  return value
    .replace(/\$\{[^}]+}/g, 'template value')
    .replace(/APICallError/g, 'API call error')
    .replace(/Uint8Array/g, 'uint8 array')
    .replace(/OIDC/g, 'oidc')
    .replace(/BYOK/g, 'byok')
    .replace(/BFL/g, 'bfl')
    .replace(/SDK/g, 'sdk')
    .replace(/Gateway/g, 'gateway')
    .replace(/providerOptions/g, 'provider options')
    .replace(/abortSignal/g, 'abort signal')
    .replace(/includeRawChunks/g, 'include raw chunks')
    .replace(/zeroDataRetention/g, 'zero data retention')
    .replace(/disallowPromptTraining/g, 'disallow prompt training')
    .replace(/hipaaCompliant/g, 'hipaa compliant')
    .replace(/quotaEntityId/g, 'quota entity id')
    .replace(/baseURL/g, 'base url')
    .replace(/modelId/g, 'model id')
    .replace(/modelType/g, 'model type')
    .replace(/topN/g, 'top n')
    .replace(/generationId/g, 'generation id')
    .replace(/authMethod/g, 'auth method')
    .replace(/getCredits/g, 'get credits')
    .replace(/groupBy/g, 'group by')
    .replace(/credential_type/g, 'credential type')
    .replace(/credentialType/g, 'credential type')
    .replace(/snake_case/g, 'snake case')
    .replace(/camelCase/g, 'camel case')
    .replace(/URL/g, 'url')
    .replace(/SSE/g, 'sse')
    .replace(/API/g, 'api')
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .toLowerCase()
    .replace(/`([^`]+)`/g, '$1')
    .replace(/"/g, '')
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .replace(/_+/g, '_');
}

function collectRustTests() {
  const roots = ['src', 'crates', 'examples']
    .map(root => path.join(repositoryRoot, root))
    .filter(root => fs.existsSync(root));
  const files = roots.flatMap(root => walkFiles(root)).filter(file => file.endsWith('.rs'));
  const names = new Set();
  const ordered = [];

  for (const file of files) {
    const source = fs.readFileSync(file, 'utf8');
    const attrPattern = /#\[(?:[A-Za-z_][A-Za-z0-9_]*::)?(?:test|rstest|test_case)(?:[^\]]*)\]/g;
    for (const match of source.matchAll(attrPattern)) {
      const after = source.slice(match.index + match[0].length, match.index + match[0].length + 1200);
      const fnMatch = after.match(/\b(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\b/);
      if (fnMatch) {
        names.add(fnMatch[1]);
        ordered.push(fnMatch[1]);
      }
    }
  }

  return { names, ordered };
}

function buildProviderUtilsMappings(rustTests) {
  const byPrefix = prefix => rustTests
    .filter(name => name.startsWith(prefix))
    .map(name => [name]);
  const byPredicate = predicate => rustTests
    .filter(name => predicate(name))
    .map(name => [name]);
  const mediaTypeToExtensionRows = rustTests.filter(name =>
    name.startsWith('media_type_to_extension_maps_audio_') ||
    name.startsWith('media_type_to_extension_maps_uppercase_') ||
    name === 'media_type_to_extension_maps_invalid_media_type_to_empty_string',
  );

  const byFile = new Map(Object.entries({
    'packages/provider-utils/src/add-additional-properties-to-json-schema.test.ts':
      byPrefix('add_additional_properties_to_json_schema_upstream_'),
    'packages/provider-utils/src/as-array.test.ts':
      byPrefix('as_array_upstream_'),
    'packages/provider-utils/src/convert-async-iterator-to-readable-stream.test.ts':
      byPrefix('convert_async_iterator_stream_upstream_'),
    'packages/provider-utils/src/convert-image-model-file-to-data-uri.test.ts':
      byPrefix('convert_image_model_file_to_data_uri_upstream_'),
    'packages/provider-utils/src/convert-to-form-data.test.ts':
      byPrefix('convert_to_form_data_upstream_'),
    'packages/provider-utils/src/create-tool-name-mapping.test.ts':
      byPrefix('create_tool_name_mapping_upstream_'),
    'packages/provider-utils/src/delay.test.ts':
      byPrefix('delay_upstream_'),
    'packages/provider-utils/src/delayed-promise.test.ts':
      byPrefix('delayed_promise_upstream_'),
    'packages/provider-utils/src/detect-media-type.test.ts':
      byPredicate(name =>
        name.startsWith('detect_media_type_upstream_') ||
        name.startsWith('get_top_level_media_type_upstream_') ||
        name.startsWith('is_full_media_type_upstream_'),
      ),
    'packages/provider-utils/src/download-blob.test.ts':
      byPredicate(name =>
        name.startsWith('download_blob_upstream_') ||
        name.startsWith('download_blob_ssrf_upstream_') ||
        name.startsWith('download_error_upstream_'),
      ),
    'packages/provider-utils/src/extract-lines.test.ts':
      [
        ['extract_lines_upstream_returns_input_when_no_start_or_end_line_is_set'],
        ...byPrefix('extract_lines_upstream_')
          .filter(rustTests => rustTests[0] !== 'extract_lines_upstream_returns_input_when_no_start_or_end_line_is_set'),
      ],
    'packages/provider-utils/src/filter-nullable.test.ts': [
      ['filter_nullable_removes_null_and_undefined_values_from_value_list'],
      ['filter_nullable_preserves_other_falsy_values'],
    ],
    'packages/provider-utils/src/generate-id.test.ts':
      byPredicate(name =>
        name.startsWith('create_id_generator_upstream_') ||
        name.startsWith('generate_id_upstream_'),
      ),
    'packages/provider-utils/src/get-from-api.test.ts':
      byPrefix('get_from_api_upstream_'),
    'packages/provider-utils/src/get-runtime-environment-user-agent.test.ts':
      byPrefix('get_runtime_environment_user_agent_upstream_'),
    'packages/provider-utils/src/handle-fetch-error.test.ts':
      byPrefix('handle_fetch_error_upstream_'),
    'packages/provider-utils/src/inject-json-instruction.test.ts':
      byPredicate(name =>
        name.startsWith('inject_json_instruction_upstream_') ||
        name.startsWith('inject_json_instruction_into_messages_upstream_'),
      ),
    'packages/provider-utils/src/is-json-serializable.test.ts':
      byPrefix('is_json_serializable_upstream_'),
    'packages/provider-utils/src/is-provider-reference.test.ts':
      byPrefix('is_provider_reference_upstream_'),
    'packages/provider-utils/src/is-url-supported.test.ts':
      byPrefix('is_url_supported_upstream_'),
    'packages/provider-utils/src/map-reasoning-to-provider.test.ts':
      byPredicate(name =>
        name.startsWith('map_reasoning_to_provider_effort_upstream_') ||
        name.startsWith('is_custom_reasoning_upstream_') ||
        name.startsWith('map_reasoning_to_provider_budget_upstream_'),
      ),
    'packages/provider-utils/src/media-type-to-extension.test.ts': [
      mediaTypeToExtensionRows,
    ],
    'packages/provider-utils/src/normalize-headers.test.ts':
      byPrefix('normalize_headers_upstream_'),
    'packages/provider-utils/src/parse-json.test.ts':
      byPredicate(name =>
        name.startsWith('parse_json_upstream_') ||
        name.startsWith('safe_parse_json_upstream_') ||
        name.startsWith('is_parsable_json_upstream_'),
      ),
    'packages/provider-utils/src/read-response-with-size-limit.test.ts':
      byPrefix('read_response_with_size_limit_upstream_'),
    'packages/provider-utils/src/remove-undefined-entries.test.ts':
      byPrefix('remove_undefined_entries_should_'),
    'packages/provider-utils/src/resolve-full-media-type.test.ts':
      byPredicate(name =>
        name.startsWith('resolve_full_media_type_') &&
        !name.startsWith('resolve_full_media_type_upstream_'),
      ),
    'packages/provider-utils/src/resolve-provider-reference.test.ts':
      byPrefix('resolve_provider_reference_upstream_'),
    'packages/provider-utils/src/resolve.test.ts':
      byPredicate(name =>
        name.startsWith('resolve_upstream_') ||
        name.startsWith('resolve_headers_upstream_'),
      ),
    'packages/provider-utils/src/response-handler.test.ts':
      byPrefix('response_handler_upstream_'),
    'packages/provider-utils/src/schema.test.ts': [
      ...byPrefix('as_schema_upstream_'),
      ...byPrefix('standard_schema_upstream_'),
    ],
    'packages/provider-utils/src/secure-json-parse.test.ts':
      byPrefix('secure_json_parse_upstream_'),
    'packages/provider-utils/src/serialize-model-options.test.ts':
      byPrefix('serialize_model_options_upstream_'),
    'packages/provider-utils/src/streaming-tool-call-tracker.test.ts':
      byPrefix('streaming_tool_call_tracker_upstream_'),
    'packages/provider-utils/src/strip-file-extension.test.ts':
      byPrefix('strip_file_extension_upstream_'),
    'packages/provider-utils/src/types/executable-tool.test.ts':
      byPrefix('is_executable_tool_upstream_'),
    'packages/provider-utils/src/types/execute-tool.test.ts':
      byPrefix('execute_tool_upstream_'),
    'packages/provider-utils/src/validate-download-url.test.ts':
      byPrefix('validate_download_url_upstream_'),
    'packages/provider-utils/src/validate-types.test.ts':
      byPredicate(name =>
        name.startsWith('validate_types_upstream_') ||
        name.startsWith('safe_validate_types_upstream_'),
      ),
    'packages/provider-utils/src/with-user-agent-suffix.test.ts':
      byPrefix('with_user_agent_suffix_upstream_'),
  }));

  const exceptionsByLocation = new Map();
  const addException = (location, status, rustTarget, notes) => {
    exceptionsByLocation.set(location, {
      source: 'provider-utils exact Rust test map',
      status,
      rustTarget,
      rustTests: [],
      notes,
    });
  };
  const addZodSnapshotException = location => addException(
    location,
    'js-only-documented',
    'exception: JavaScript Zod v4 runtime adapter snapshot',
    'The upstream zodSchema case depends on Zod v4 runtime JSON-schema conversion and Vitest snapshots; Rust tracks portable StandardSchema and explicit JSON Schema behavior.',
  );

  addException(
    'packages/provider-utils/src/download-blob.test.ts:158',
    'js-only-documented',
    'exception: JavaScript fetch AbortSignal passthrough',
    'JavaScript downloadBlob passes AbortSignal into fetch; Rust download_blob uses an injected transport and documents abort as a transport integration concern.',
  );
  addException(
    'packages/provider-utils/src/get-from-api.test.ts:176',
    'js-only-documented',
    'exception: JavaScript global fetch default',
    'Upstream falls back to JavaScript global fetch when no fetch implementation is supplied; Rust provider-utils requires an injected transport and maps request construction separately.',
  );
  addException(
    'packages/provider-utils/src/inject-json-instruction.test.ts:167',
    'type-system-impossible',
    'exception: JavaScript null optional-parameter coercion',
    'The upstream case passes null through any-typed optional fields; Rust uses typed Option fields, so null cannot be supplied as a distinct runtime value.',
  );
  addException(
    'packages/provider-utils/src/is-provider-reference.test.ts:33',
    'js-only-documented',
    'exception: JavaScript URL instance identity',
    'The upstream case checks a JavaScript URL object; Rust provider reference detection operates on JSON-like records and has no URL class instance boundary.',
  );

  for (const line of [21, 32, 43, 54, 64, 74, 84, 94, 110, 127, 150, 160, 171]) {
    addZodSnapshotException(`packages/provider-utils/src/schema.test.ts:${line}`);
  }
  addException(
    'packages/provider-utils/src/schema.test.ts:189',
    'js-only-documented',
    'exception: JavaScript Zod v4 runtime adapter validation',
    'The upstream case validates Zod transform output through the JavaScript zodSchema adapter; Rust maps portable transform validation through StandardSchema rows.',
  );
  addException(
    'packages/provider-utils/src/serialize-model-options.test.ts:97',
    'js-only-documented',
    'exception: JavaScript promise-returning header callback',
    'The upstream case observes resolveSync rejecting an async JavaScript headers callback; Rust serialize_model_options accepts already-materialized JSON config entries, not function-valued callbacks.',
  );
  addException(
    'packages/provider-utils/src/serialize-model-options.test.ts:109',
    'js-only-documented',
    'exception: JavaScript promise-returning header callback',
    'The upstream case observes resolveSync rejecting a Promise-returning JavaScript headers callback; Rust serialize_model_options accepts already-materialized JSON config entries, not function-valued callbacks.',
  );

  return { byFile, exceptionsByLocation };
}

function classifyProviderUtilsCase(testCase, providerUtilsMappings) {
  if (!testCase.file.startsWith('packages/provider-utils/')) {
    return null;
  }

  const locationKey = `${testCase.file}:${testCase.line}`;
  const exception = providerUtilsMappings.exceptionsByLocation.get(locationKey);
  if (exception) {
    return exception;
  }

  const queue = providerUtilsMappings.byFile.get(testCase.file);
  if (!queue?.length) {
    return null;
  }

  const rustTests = queue.shift();
  return {
    source: 'provider-utils exact Rust test map',
    status: 'portable-mapped',
    rustTarget: rustTests.map(test => `test: ${test}`).join('; '),
    rustTests,
    notes: 'Mapped by the provider-utils row-level closure map.',
  };
}

function providerUtilsMappingValidationErrors(providerUtilsMappings) {
  const errors = [];
  for (const [file, queue] of providerUtilsMappings.byFile) {
    if (queue.length > 0) {
      errors.push(`${file}: unused provider-utils Rust test mappings: ${queue.flat().join(', ')}`);
    }
  }
  return errors;
}

const WORKFLOW_MAPPING_SOURCE = 'AIS-11 workflow strict case map';

const WORKFLOW_CASE_TESTS = new Map(Object.entries({
  'packages/workflow/src/serializable-schema.test.ts:10': [
    'serialize_tool_set_serializes_function_tools_with_description_and_input_schema',
  ],
  'packages/workflow/src/serializable-schema.test.ts:36': [
    'serialize_tool_set_preserves_provider_tool_identity_and_args',
  ],
  'packages/workflow/src/serializable-schema.test.ts:71': [
    'resolve_serializable_tools_reconstructs_function_tools',
  ],
  'packages/workflow/src/serializable-schema.test.ts:90': [
    'resolve_serializable_tools_reconstructs_provider_tools',
  ],
  'packages/workflow/src/stream-text-iterator.test.ts:146': [
    'stream_text_iterator_maps_provider_metadata_to_provider_options_for_continuation',
  ],
  'packages/workflow/src/stream-text-iterator.test.ts:235': [
    'stream_text_iterator_upstream_should_not_add_provider_options_when_provider_metadata_is_undefined',
  ],
  'packages/workflow/src/stream-text-iterator.test.ts:301': [
    'stream_text_iterator_upstream_should_preserve_provider_metadata_for_multiple_parallel_tool_calls',
  ],
  'packages/workflow/src/stream-text-iterator.test.ts:406': [
    'stream_text_iterator_upstream_should_handle_mixed_tool_calls_with_and_without_provider_metadata',
  ],
  'packages/workflow/src/stream-text-iterator.test.ts:501': [
    'stream_text_iterator_upstream_should_strip_openai_item_id_from_provider_metadata_to_avoid_reasoning_item_errors',
  ],
  'packages/workflow/src/stream-text-iterator.test.ts:572': [
    'stream_text_iterator_upstream_should_preserve_other_openai_metadata_while_stripping_item_id',
  ],
  'packages/workflow/src/stream-text-iterator.test.ts:648': [
    'stream_text_iterator_upstream_should_preserve_gemini_metadata_while_stripping_openai_item_id_in_mixed_provider_metadata',
  ],
  'packages/workflow/src/stream-text-iterator.test.ts:728': [
    'stream_text_iterator_passes_contexts_to_executor_and_yields_them',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:260': [
    'workflow_agent_compat_should_use_prepare_call_provider_options',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:289': [
    'workflow_agent_compat_should_pass_abort_signal_to_stream_text_iterator',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:307': [
    'workflow_agent_compat_should_pass_timeout_to_stream_text_iterator',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:324': [
    'workflow_agent_upstream_should_pass_string_instructions_to_the_model',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:361': [
    'workflow_agent_compat_should_pass_system_message_instructions',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:405': [
    'workflow_agent_compat_should_pass_array_of_system_message_instructions',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:476': [
    'workflow_agent_compat_should_call_experimental_on_start_from_constructor',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:499': [
    'workflow_agent_compat_should_call_experimental_on_start_from_stream_method',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:520': [
    'workflow_agent_compat_should_call_both_constructor_and_method_experimental_on_start_in_correct_order',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:547': [
    'workflow_agent_compat_should_pass_experimental_on_start_event_information',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:656': [
    'workflow_agent_compat_should_call_experimental_on_step_start_from_constructor',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:679': [
    'workflow_agent_compat_should_call_experimental_on_step_start_from_stream_method',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:700': [
    'workflow_agent_compat_should_call_both_constructor_and_method_experimental_on_step_start_in_correct_order',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:727': [
    'workflow_agent_compat_should_pass_experimental_on_step_start_event_information',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:827': [
    'workflow_agent_compat_should_call_on_step_finish_from_constructor',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:849': [
    'workflow_agent_compat_should_call_on_step_finish_from_stream_method',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:872': [
    'workflow_agent_compat_should_call_both_constructor_and_method_on_step_finish_in_correct_order',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:899': [
    'workflow_agent_compat_should_pass_step_result_to_on_step_finish_callback',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:942': [
    'workflow_agent_compat_should_call_on_tool_execution_start_from_constructor',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:972': [
    'workflow_agent_compat_should_call_on_tool_execution_start_from_stream_method',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1002': [
    'workflow_agent_compat_should_call_both_constructor_and_method_on_tool_execution_start_in_correct_order',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1036': [
    'workflow_agent_compat_should_pass_tool_execution_start_event_information',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1082': [
    'workflow_agent_compat_should_call_on_tool_execution_end_from_constructor',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1112': [
    'workflow_agent_compat_should_call_on_tool_execution_end_from_stream_method',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1142': [
    'workflow_agent_compat_should_call_both_constructor_and_method_on_tool_execution_end_in_correct_order',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1176': [
    'workflow_agent_compat_should_pass_tool_execution_end_event_information_on_success',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1235': [
    'workflow_agent_compat_should_call_on_finish_from_constructor',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1257': [
    'workflow_agent_compat_should_call_on_finish_from_stream_method',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1280': [
    'workflow_agent_compat_should_call_both_constructor_and_method_on_finish_in_correct_order',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1307': [
    'workflow_agent_compat_should_pass_finish_event_information',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1348': [
    'workflow_agent_telemetry_integrations_call_per_call_integration_listeners_for_all_lifecycle_events',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1402': [
    'workflow_agent_telemetry_integrations_include_only_configured_runtime_and_tools_context_fields',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1480': [
    'workflow_agent_telemetry_integrations_call_globally_registered_integration_listeners',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1530': [
    'workflow_agent_telemetry_integrations_call_integration_listeners_alongside_agent_callbacks',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1591': [
    'workflow_agent_telemetry_integrations_do_not_break_streaming_when_a_listener_throws',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1637': [
    'workflow_agent_upstream_should_pause_when_tool_needs_approval',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1665': [
    'workflow_agent_upstream_should_support_needs_approval_as_a_function',
  ],
  'packages/workflow/src/workflow-agent-compat.test.ts:1697': [
    'workflow_agent_telemetry_integrations_emit_execute_tool_when_an_approved_tool_resumes',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:38': [
    'workflow_agent_upstream_should_generate_basic_text_response',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:47': [
    'workflow_agent_upstream_should_successfully_execute_tools_that_return_normally',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:54': [
    'workflow_agent_upstream_should_pass_updated_messages_on_subsequent_tool_call_rounds',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:63': [
    'workflow_agent_upstream_should_convert_tool_execution_error_to_error_text_result',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:78': [
    'workflow_agent_upstream_should_support_tool_input_schemas_across_step_boundaries',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:91': [
    'workflow_agent_upstream_should_pass_experimental_repair_tool_call_to_stream_text_iterator',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:104': [
    'workflow_agent_compat_should_call_both_constructor_and_method_on_step_finish_in_correct_order',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:124': [
    'workflow_agent_compat_should_call_both_constructor_and_method_on_finish_in_correct_order',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:145': [
    'workflow_agent_upstream_should_pass_string_instructions_to_the_model',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:158': [
    'workflow_agent_upstream_should_complete_within_timeout',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:173': [
    'workflow_agent_compat_should_call_experimental_on_start_from_constructor',
    'workflow_agent_compat_should_call_experimental_on_start_from_stream_method',
    'workflow_agent_compat_should_call_both_constructor_and_method_experimental_on_start_in_correct_order',
    'workflow_agent_compat_should_pass_experimental_on_start_event_information',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:182': [
    'workflow_agent_compat_should_call_experimental_on_step_start_from_constructor',
    'workflow_agent_compat_should_call_experimental_on_step_start_from_stream_method',
    'workflow_agent_compat_should_call_both_constructor_and_method_experimental_on_step_start_in_correct_order',
    'workflow_agent_compat_should_pass_experimental_on_step_start_event_information',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:191': [
    'workflow_agent_compat_should_call_on_tool_execution_start_from_constructor',
    'workflow_agent_compat_should_call_on_tool_execution_start_from_stream_method',
    'workflow_agent_compat_should_call_both_constructor_and_method_on_tool_execution_start_in_correct_order',
    'workflow_agent_compat_should_pass_tool_execution_start_event_information',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:200': [
    'workflow_agent_compat_should_call_on_tool_execution_end_from_constructor',
    'workflow_agent_compat_should_call_on_tool_execution_end_from_stream_method',
    'workflow_agent_compat_should_call_both_constructor_and_method_on_tool_execution_end_in_correct_order',
    'workflow_agent_compat_should_pass_tool_execution_end_event_information_on_success',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:211': [
    'workflow_agent_compat_should_use_prepare_call_provider_options',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:219': [
    'workflow_agent_upstream_should_pause_when_tool_needs_approval',
    'workflow_agent_upstream_should_support_needs_approval_as_a_function',
    'workflow_agent_upstream_should_execute_approved_tools_and_continue_with_results',
    'workflow_agent_upstream_should_create_denial_results_for_denied_tools_and_continue',
  ],
  'packages/workflow/src/workflow-agent-e2e.integration.test.ts:230': [
    'workflow_agent_upstream_should_flow_through_runtime_context_and_tools_context_e2e',
  ],
  'packages/workflow/src/workflow-agent.test.ts:58': [
    'workflow_agent_upstream_should_expose_id_when_provided_in_constructor',
  ],
  'packages/workflow/src/workflow-agent.test.ts:66': [
    'workflow_agent_upstream_should_have_undefined_id_when_not_provided',
  ],
  'packages/workflow/src/workflow-agent.test.ts:75': [
    'workflow_agent_upstream_should_convert_fatal_error_to_tool_error_result',
  ],
  'packages/workflow/src/workflow-agent.test.ts:153': [
    'workflow_agent_upstream_should_convert_non_fatal_error_to_tool_error_result',
  ],
  'packages/workflow/src/workflow-agent.test.ts:227': [
    'workflow_agent_upstream_should_successfully_execute_tools_that_return_normally',
  ],
  'packages/workflow/src/workflow-agent.test.ts:297': [
    'workflow_agent_upstream_should_skip_local_execution_for_provider_executed_tools',
  ],
  'packages/workflow/src/workflow-agent.test.ts:384': [
    'workflow_agent_upstream_should_handle_mixed_provider_executed_and_local_tools',
  ],
  'packages/workflow/src/workflow-agent.test.ts:490': [
    'workflow_agent_upstream_should_handle_provider_executed_tool_errors_with_is_error_flag',
  ],
  'packages/workflow/src/workflow-agent.test.ts:563': [
    'workflow_agent_upstream_should_return_empty_result_when_provider_executed_tool_result_is_missing',
  ],
  'packages/workflow/src/workflow-agent.test.ts:642': [
    'workflow_agent_upstream_should_keep_invalid_tool_calls_on_error_path_without_executing',
  ],
  'packages/workflow/src/workflow-agent.test.ts:750': [
    'workflow_agent_upstream_should_stop_the_loop_for_client_side_tools_without_execute',
  ],
  'packages/workflow/src/workflow-agent.test.ts:822': [
    'workflow_agent_upstream_should_handle_mixed_executable_and_client_side_tools_in_same_step',
  ],
  'packages/workflow/src/workflow-agent.test.ts:930': [
    'workflow_agent_upstream_should_call_on_finish_when_stopping_for_client_side_tools',
  ],
  'packages/workflow/src/workflow-agent.test.ts:992': [
    'workflow_agent_upstream_should_have_empty_tool_calls_when_all_tools_complete_normally',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1063': [
    'workflow_agent_upstream_should_pass_prepare_step_callback_to_stream_text_iterator',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1100': [
    'stream_text_iterator_upstream_should_allow_prepare_step_to_modify_messages',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1146': [
    'stream_text_iterator_upstream_should_allow_prepare_step_to_change_model_dynamically',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1191': [
    'workflow_agent_upstream_should_provide_step_information_to_prepare_step_callback',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1245': [
    'workflow_agent_upstream_should_pass_conversation_messages_to_tool_execute_function',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1328': [
    'workflow_agent_upstream_should_pass_messages_to_multiple_tools_in_parallel_execution',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1425': [
    'workflow_agent_upstream_should_pass_updated_messages_on_subsequent_tool_call_rounds',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1550': [
    'workflow_agent_upstream_should_pass_generation_settings_from_constructor_to_stream_text_iterator',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1592': [
    'workflow_agent_upstream_should_allow_stream_options_to_override_constructor_generation_settings',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1633': [
    'workflow_agent_upstream_should_pass_tool_choice_from_constructor_to_stream_text_iterator',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1667': [
    'workflow_agent_upstream_should_allow_stream_options_to_override_constructor_tool_choice',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1704': [
    'workflow_agent_upstream_should_filter_tools_when_active_tools_is_specified',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1760': [
    'workflow_agent_upstream_should_use_constructor_stop_conditions_when_not_specified_in_stream',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1794': [
    'workflow_agent_upstream_should_allow_stream_options_to_override_constructor_stop_conditions',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1830': [
    'workflow_agent_upstream_should_use_constructor_active_tools_when_not_specified_in_stream',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1876': [
    'workflow_agent_upstream_should_use_constructor_experimental_repair_tool_call_when_not_specified_in_stream',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1912': [
    'workflow_agent_upstream_should_pass_on_error_callback_to_stream_text_iterator',
  ],
  'packages/workflow/src/workflow-agent.test.ts:1948': [
    'workflow_agent_upstream_should_convert_tool_execution_error_to_error_text_result',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2023': [
    'workflow_agent_compat_should_pass_finish_event_information',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2095': [
    'workflow_agent_upstream_should_call_on_abort_when_abort_signal_is_already_aborted',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2130': [
    'workflow_agent_upstream_should_pass_step_number_to_tool_execution_start_and_use_success_union_on_end',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2214': [
    'workflow_agent_upstream_should_pass_success_false_in_tool_execution_end_when_tool_errors',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2283': [
    'workflow_agent_compat_should_pass_experimental_on_step_start_event_information',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2355': [
    'workflow_agent_upstream_should_pass_per_tool_tools_context_entry_as_execute_context',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2390': [
    'workflow_agent_upstream_should_pass_undefined_context_when_no_tools_context_entry_exists',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2419': [
    'workflow_agent_upstream_should_validate_per_tool_context_against_context_schema',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2453': [
    'workflow_agent_upstream_should_pass_runtime_context_to_on_finish',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2529': [
    'workflow_agent_upstream_should_return_messages_and_steps_in_result',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2597': [
    'workflow_agent_upstream_should_accept_a_string_prompt_in_stream',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2627': [
    'workflow_agent_upstream_should_accept_an_array_of_messages_as_prompt',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2658': [
    'workflow_agent_upstream_should_pass_experimental_repair_tool_call_to_stream_text_iterator',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2698': [
    'workflow_agent_upstream_should_pass_include_raw_chunks_to_stream_text_iterator',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2734': [
    'workflow_agent_upstream_should_pass_telemetry_settings_from_constructor_to_stream_text_iterator',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2774': [
    'workflow_agent_upstream_should_allow_stream_options_to_override_constructor_telemetry',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2813': [
    'workflow_agent_upstream_should_return_undefined_ui_messages_when_collect_ui_messages_is_false',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2843': [
    'workflow_agent_upstream_should_return_undefined_ui_messages_when_collect_ui_messages_is_not_set',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2872': [
    'workflow_agent_upstream_should_pass_collect_ui_chunks_when_collect_ui_messages_is_true',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2909': [
    'workflow_agent_upstream_should_work_when_collect_ui_messages_is_true_and_send_finish_is_false',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2954': [
    'workflow_agent_upstream_should_not_write_finish_chunk_but_still_return_ui_messages_when_send_finish_is_false',
  ],
  'packages/workflow/src/workflow-agent.test.ts:2993': [
    'workflow_agent_upstream_should_execute_approved_tools_and_continue_with_results',
  ],
  'packages/workflow/src/workflow-agent.test.ts:3072': [
    'workflow_agent_upstream_should_create_denial_results_for_denied_tools_and_continue',
  ],
  'packages/workflow/src/workflow-agent.test.ts:3145': [
    'workflow_agent_upstream_should_pass_through_messages_without_approval_responses_unchanged',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:32': [
    'workflow_chat_transport_sends_messages_and_reports_chat_end',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:38': [
    'workflow_chat_transport_accepts_and_stores_callback_functions',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:54': [
    'workflow_chat_transport_uses_default_options_and_builds_send_request',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:59': [
    'workflow_chat_transport_accepts_custom_max_consecutive_errors',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:68': [
    'workflow_chat_transport_uses_custom_api_endpoint_and_builds_send_request',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:139': [
    'workflow_chat_transport_reports_send_message_http_errors',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:160': [
    'workflow_chat_transport_uses_custom_api_endpoint_and_builds_reconnect_request',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:217': [
    'workflow_chat_transport_passes_abort_signal_to_reconnect_requests',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:259': [
    'workflow_chat_transport_reuses_abort_signal_for_reconnect_after_interrupted_send',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:337': [
    'workflow_chat_transport_reconnect_uses_positive_initial_start_index_for_retries',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:391': [
    'workflow_chat_transport_reconnect_resolves_negative_start_index_from_tail_header',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:435': [
    'workflow_chat_transport_reconnect_falls_back_to_zero_when_tail_header_is_missing',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:484': [
    'workflow_chat_transport_reconnect_falls_back_to_zero_for_invalid_negative_tail_header',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:570': [
    'workflow_chat_transport_calls_on_chat_send_message_callback',
  ],
  'packages/workflow/src/workflow-chat-transport.test.ts:618': [
    'workflow_chat_transport_calls_on_chat_end_callback_when_stream_ends',
  ],
  'packages/workflow/src/workflow-smoke.integration.test.ts:6': [
    'workflow_smoke_should_compute_the_correct_result',
  ],
}));

const WORKFLOW_EXCEPTIONS = new Map(Object.entries({
  'packages/workflow/src/workflow-chat-transport.test.ts:27': {
    status: 'js-only-documented',
    rustTarget: 'exception: JavaScript default fetch constructor fallback',
    notes:
      'Upstream only checks that WorkflowChatTransport can be constructed without an explicit JavaScript fetch. Rust uses an injected WorkflowChatTransportClient for actual requests, with deterministic client behavior mapped in the adjacent chat-transport rows.',
  },
  'packages/workflow/src/workflow-chat-transport.test.ts:529': {
    status: 'js-only-documented',
    rustTarget: 'exception: JavaScript object error stringification',
    notes:
      'The upstream row guards against JavaScript coercing thrown plain objects to [object Object]. Rust transport errors are typed string/HTTP variants, so the object-coercion boundary is not expressible; reconnect failure formatting is covered by named Rust tests.',
  },
}));

function classifyWorkflowCase(testCase) {
  if (!testCase.file.startsWith('packages/workflow/')) {
    return null;
  }

  const key = `${testCase.file}:${testCase.line}`;
  const exception = WORKFLOW_EXCEPTIONS.get(key);
  if (exception) {
    return {
      source: WORKFLOW_MAPPING_SOURCE,
      rustTests: [],
      ...exception,
    };
  }

  const rustTests = WORKFLOW_CASE_TESTS.get(key);
  if (!rustTests) {
    return null;
  }

  return {
    source: WORKFLOW_MAPPING_SOURCE,
    status: 'portable-mapped',
    rustTarget: rustTests.map(test => `test: ${test}`).join('; '),
    rustTests,
    notes:
      'Mapped by AIS-11 from the refreshed upstream packages/workflow row to deterministic Rust workflow tests using fake models, fake stream executors, or in-memory chat transport clients.',
  };
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

function classifyCase(
  testCase,
  item,
  foundationalMappings,
  aiCoreMappings,
  ai02Mappings,
  providerUtilsMappings,
  rustTests,
  currentCaseLocations,
) {
  const locationKey = `${testCase.file}:${testCase.line}`;
  const foundational = foundationalMappings.byLocation.get(locationKey);
  if (foundational) {
    consumeExactNameFallback(foundationalMappings, testCase, foundational);
    return foundational;
  }

  const foundationalByName = shiftNameFallback(
    foundationalMappings,
    testCase,
    currentCaseLocations,
  );
  if (foundationalByName) {
    return foundationalByName;
  }

  const aiCore = aiCoreMappings.byLocation.get(locationKey);
  if (aiCore) {
    consumeExactNameFallback(aiCoreMappings, testCase, aiCore);
    return aiCore;
  }

  const aiCoreByName = shiftNameFallback(
    aiCoreMappings,
    testCase,
    currentCaseLocations,
  );
  if (aiCoreByName) {
    return aiCoreByName;
  }

  const ai02Key = `${testCase.file}|${normalizeNameKey(testCase.name)}`;
  const ai02Queue = ai02Mappings.get(ai02Key);
  if (ai02Queue?.length) {
    const mapping = ai02Queue.shift();
    mapping.used = true;
    return mapping;
  }

  const providerUtils = classifyProviderUtilsCase(testCase, providerUtilsMappings);
  if (providerUtils) {
    return providerUtils;
  }

  const gatewayVercelMapping = gatewayVercelMappedClassification(testCase, rustTests.names);
  if (gatewayVercelMapping) {
    return gatewayVercelMapping;
  }

  const workflowMapping = classifyWorkflowCase(testCase);
  if (workflowMapping) {
    return workflowMapping;
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
  const foundationalMappings = mergeStrictCaseMappings([
    FOUNDATIONAL_INVENTORY_PATH,
    AI_05_INVENTORY_PATH,
  ]);
  const aiCoreMappings = parseFoundationalMappings(
    AI_CORE_INVENTORY_PATH,
    'docs/ai-core-package-inventory.md',
  );
  const ai02Mappings = mergeNamedCaseMappings(
    parseAi02Mappings(AI_02_INVENTORY_PATH),
    parseAi02Mappings(
      AI_06_INVENTORY_PATH,
      relativePath(AI_06_INVENTORY_PATH, repositoryRoot),
      'Mapped by the AI-06 exact case map.',
    ),
    parseAi02Mappings(
      AI_04_INVENTORY_PATH,
      relativePath(AI_04_INVENTORY_PATH, repositoryRoot),
      'Mapped by the AIS-04 exact case map.',
    ),
  );
  const rustTests = collectRustTests();
  const providerUtilsMappings = buildProviderUtilsMappings(rustTests.ordered);
  const validationErrors = [];
  const inventories = [];

  for (const item of inventoryItems(args.upstreamRoot, ledgerPackages)) {
    const testFiles = walkFiles(item.root)
      .filter(file => TEST_FILE_PATTERN.test(file))
      .sort();
    const extractedCases = testFiles.flatMap(file => extractTestCases(file, args.upstreamRoot));
    const currentCaseLocations = new Set(
      extractedCases.map(testCase => `${testCase.file}:${testCase.line}`),
    );
    const cases = [];

    for (const testCase of extractedCases) {
      const classification = classifyCase(
        testCase,
        item,
        foundationalMappings,
        aiCoreMappings,
        ai02Mappings,
        providerUtilsMappings,
        rustTests,
        currentCaseLocations,
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
          if (!rustTests.names.has(rustTest)) {
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

    const statusCounts = countStatuses(cases);
    inventories.push({
      ...item,
      testFiles: testFiles.map(file => relativePath(file, args.upstreamRoot)),
      cases,
      ...statusCounts,
    });
  }

  validationErrors.push(...providerUtilsMappingValidationErrors(providerUtilsMappings));
  validationErrors.push(
    ...exactCaseMappingValidationErrors(
      ai02Mappings,
      relativePath(AI_04_INVENTORY_PATH, repositoryRoot),
    ),
  );

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
