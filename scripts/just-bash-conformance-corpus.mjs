#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..'
);

const defaultUpstreamRoot =
  '/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/just-bash/main';
const upstreamRoot = process.env.JUST_BASH_UPSTREAM_PATH ?? defaultUpstreamRoot;
const upstreamRepo = 'vercel-labs/just-bash';
const upstreamUrl = 'https://github.com/vercel-labs/just-bash';
const expectedUpstreamHead = 'd64009aef6bc1556e7c84b22ed455863275ea953';
const inventoryDate = '2026-06-02';

const corpusPath = path.join(
  repositoryRoot,
  'fixtures/just-bash-conformance/corpus.json'
);
const docsPath = path.join(
  repositoryRoot,
  'docs/open-agents/just-bash-conformance.md'
);
const parityPath = path.join(
  repositoryRoot,
  'docs/open-agents/just-bash-parity.md'
);

const comparisonRoot = 'packages/just-bash/src/comparison-tests';
const comparisonFixturesRoot = `${comparisonRoot}/fixtures`;

const unitSourceFiles = [
  'packages/just-bash/src/commands/printf/printf.test.ts',
  'packages/just-bash/src/commands/pwd/pwd.test.ts',
  'packages/just-bash/src/commands/env/env.test.ts',
  'packages/just-bash/src/commands/env/env.utf8-stdin.test.ts',
  'packages/just-bash/src/commands/cp/cp.test.ts',
  'packages/just-bash/src/commands/mv/mv.test.ts',
  'packages/just-bash/src/commands/rm/rm.test.ts',
  'packages/just-bash/src/commands/mkdir/mkdir.test.ts',
  'packages/just-bash/src/commands/touch/touch.test.ts',
  `${comparisonRoot}/cd.comparison.test.ts`,
  `${comparisonRoot}/env.comparison.test.ts`,
  `${comparisonRoot}/file-operations.comparison.test.ts`,
];

const specRunnerRows = {
  grep: {
    file: 'packages/just-bash/src/spec-tests/grep/grep-spec.test.ts',
    line: 73,
    name: '<dynamic:testName>',
    declaration: 'it',
  },
  sed: {
    file: 'packages/just-bash/src/spec-tests/sed/sed-spec.test.ts',
    line: 73,
    name: '<dynamic:testName>',
    declaration: 'it',
  },
};

const knownCommandWords = new Set([
  'alias',
  'awk',
  'basename',
  'cat',
  'cd',
  'column',
  'cp',
  'cut',
  'dirname',
  'echo',
  'env',
  'expand',
  'export',
  'find',
  'fold',
  'grep',
  'head',
  'join',
  'jq',
  'ls',
  'mkdir',
  'mv',
  'nl',
  'paste',
  'printf',
  'pwd',
  'rev',
  'rm',
  'sed',
  'sort',
  'split',
  'strings',
  'tail',
  'tar',
  'tee',
  'test',
  'touch',
  'tr',
  'unalias',
  'unexpand',
  'uniq',
  'wc',
]);

const expectedMinimums = {
  totalCases: 550,
  comparisonCases: 350,
  unitCases: 25,
  specCases: 150,
  representativeDomains: [
    'echo',
    'printf',
    'pwd-cd-env',
    'file-ops',
    'grep',
    'sed',
    'awk',
  ],
};

function usage() {
  console.log(`Usage: node scripts/just-bash-conformance-corpus.mjs [options]

Options:
  --check      Verify generated corpus JSON and docs are current.
  --dry-run    Print generated corpus summary as JSON.
  --help       Show this help text.

Environment:
  JUST_BASH_UPSTREAM_PATH  Override the OpenSrc mirror path.`);
}

function fail(message) {
  console.error(`just-bash conformance corpus: ${message}`);
  process.exit(1);
}

function parseArgs(argv) {
  const options = { check: false, dryRun: false };
  for (const arg of argv) {
    if (arg === '--help') {
      usage();
      process.exit(0);
    }
    if (arg === '--check') {
      options.check = true;
      continue;
    }
    if (arg === '--dry-run') {
      options.dryRun = true;
      continue;
    }
    fail(`unknown option: ${arg}`);
  }
  return options;
}

function walk(directory) {
  const entries = fs.readdirSync(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...walk(entryPath));
    } else if (entry.isFile()) {
      files.push(entryPath);
    }
  }
  return files;
}

function upstreamRelative(filePath) {
  return path.relative(upstreamRoot, filePath).replaceAll(path.sep, '/');
}

function readText(relativePath) {
  return fs.readFileSync(path.join(upstreamRoot, relativePath), 'utf8');
}

function readJson(relativePath) {
  return JSON.parse(readText(relativePath));
}

function sha256(text) {
  return createHash('sha256').update(text).digest('hex');
}

function fixtureIdFor(command, files) {
  const sortedFiles = Object.keys(files ?? {})
    .sort()
    .map((key) => `${key}:${files[key]}`)
    .join('|');
  return sha256(`${command}|||${sortedFiles}`).slice(0, 16);
}

function liveUpstreamHead() {
  const output = execFileSync(
    'git',
    ['ls-remote', upstreamUrl, 'HEAD', 'refs/heads/main'],
    { encoding: 'utf8' }
  );
  const heads = output
    .trim()
    .split(/\r?\n/)
    .map((line) => line.split(/\s+/)[0])
    .filter(Boolean);
  const uniqueHeads = new Set(heads);
  if (uniqueHeads.size !== 1) {
    fail(`could not verify a single upstream HEAD from git ls-remote output`);
  }
  return [...uniqueHeads][0];
}

function escapeCell(value) {
  return String(value ?? '')
    .replaceAll('\\', '\\\\')
    .replaceAll('|', '\\|')
    .replaceAll('\n', '<br>');
}

function unescapeCell(cell) {
  return cell
    .trim()
    .replaceAll('<br>', '\n')
    .replace(/\\([\\|])/g, '$1');
}

function isEscaped(text, index) {
  let backslashes = 0;
  for (let cursor = index - 1; cursor >= 0 && text[cursor] === '\\'; cursor -= 1) {
    backslashes += 1;
  }
  return backslashes % 2 === 1;
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

function isSeparatorRow(cells) {
  return cells.every((cell) => /^:?-{3,}:?$/.test(cell.trim()));
}

function parseTable(markdown, heading) {
  const lines = markdown.split(/\r?\n/);
  const headingIndex = lines.findIndex((line) => line.trim() === heading);
  if (headingIndex === -1) {
    return { headers: [], rows: [] };
  }

  let tableIndex = headingIndex + 1;
  while (tableIndex < lines.length && !lines[tableIndex].trim().startsWith('|')) {
    tableIndex += 1;
  }

  const headers = splitMarkdownRow(lines[tableIndex] ?? '');
  const separator = splitMarkdownRow(lines[tableIndex + 1] ?? '');
  if (!headers || !separator || !isSeparatorRow(separator)) {
    return { headers: [], rows: [] };
  }

  const rows = [];
  for (let rowIndex = tableIndex + 2; rowIndex < lines.length; rowIndex += 1) {
    const cells = splitMarkdownRow(lines[rowIndex]);
    if (!cells) {
      break;
    }
    if (cells.length !== headers.length) {
      continue;
    }
    rows.push(Object.fromEntries(headers.map((header, index) => [header, cells[index]])));
  }
  return { headers, rows };
}

function ledgerKey(row) {
  if (!row.file || !row.line || !row.declaration || !row.name) {
    return undefined;
  }
  return `${row.file}:${row.line}:${row.declaration}:${row.name}`;
}

function readParityRows() {
  if (!fs.existsSync(parityPath)) {
    fail(`missing parity ledger: ${parityPath}`);
  }

  const markdown = fs.readFileSync(parityPath, 'utf8');
  const cases = new Map();
  for (const row of parseTable(markdown, '## Test Case Inventory').rows) {
    const entry = {
      packageId: row.Package,
      domain: row.Domain,
      file: row['Upstream test file'],
      line: Number.parseInt(row.Line, 10),
      suite: row.Suite,
      name: row.Case,
      declaration: row.Declaration,
      status: row.Status,
      owner: row['Rust owner crate/module'],
      rustTest: row['Rust test name or exception'],
      notes: row.Notes,
    };
    const key = ledgerKey(entry);
    if (key) {
      cases.set(key, entry);
    }
  }
  return cases;
}

function isIdentifierStart(char) {
  return /[A-Za-z_$]/.test(char ?? '');
}

function isIdentifierPart(char) {
  return /[A-Za-z0-9_$]/.test(char ?? '');
}

function skipWhitespace(source, index) {
  let cursor = index;
  while (/\s/.test(source[cursor] ?? '')) {
    cursor += 1;
  }
  return cursor;
}

function lineStarts(source) {
  const starts = [0];
  for (let index = 0; index < source.length; index += 1) {
    if (source[index] === '\n') {
      starts.push(index + 1);
    }
  }
  return starts;
}

function lineNumberAt(starts, index) {
  let low = 0;
  let high = starts.length - 1;
  while (low <= high) {
    const middle = Math.floor((low + high) / 2);
    if (starts[middle] <= index) {
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return high + 1;
}

function findMatching(source, openIndex, openChar, closeChar) {
  let depth = 0;
  let state = 'code';
  for (let index = openIndex; index < source.length; index += 1) {
    const char = source[index];
    const next = source[index + 1];

    if (state === 'line-comment') {
      if (char === '\n') state = 'code';
      continue;
    }
    if (state === 'block-comment') {
      if (char === '*' && next === '/') {
        index += 1;
        state = 'code';
      }
      continue;
    }
    if (state === 'single') {
      if (char === '\\') index += 1;
      else if (char === "'") state = 'code';
      continue;
    }
    if (state === 'double') {
      if (char === '\\') index += 1;
      else if (char === '"') state = 'code';
      continue;
    }
    if (state === 'template') {
      if (char === '\\') index += 1;
      else if (char === '`') state = 'code';
      continue;
    }

    if (char === '/' && next === '/') {
      index += 1;
      state = 'line-comment';
      continue;
    }
    if (char === '/' && next === '*') {
      index += 1;
      state = 'block-comment';
      continue;
    }
    if (char === "'") {
      state = 'single';
      continue;
    }
    if (char === '"') {
      state = 'double';
      continue;
    }
    if (char === '`') {
      state = 'template';
      continue;
    }

    if (char === openChar) {
      depth += 1;
    } else if (char === closeChar) {
      depth -= 1;
      if (depth === 0) {
        return index;
      }
    }
  }
  return undefined;
}

function literalBounds(source, index) {
  const quote = source[index];
  if (quote !== '"' && quote !== "'" && quote !== '`') {
    return undefined;
  }
  if (quote === '`') {
    let hasTemplateExpression = false;
    for (let cursor = index + 1; cursor < source.length; cursor += 1) {
      const char = source[cursor];
      if (char === '\\') {
        cursor += 1;
      } else if (char === '$' && source[cursor + 1] === '{') {
        hasTemplateExpression = true;
      } else if (char === '`') {
        if (hasTemplateExpression) {
          return undefined;
        }
        return { start: index, end: cursor };
      }
    }
    return undefined;
  }

  for (let cursor = index + 1; cursor < source.length; cursor += 1) {
    const char = source[cursor];
    if (char === '\\') {
      cursor += 1;
    } else if (char === quote) {
      return { start: index, end: cursor };
    }
  }
  return undefined;
}

function parseLiteralAt(source, index) {
  const bounds = literalBounds(source, skipWhitespace(source, index));
  if (!bounds) {
    return undefined;
  }
  try {
    return {
      value: Function(`"use strict"; return (${source.slice(bounds.start, bounds.end + 1)});`)(),
      end: bounds.end,
      raw: source.slice(bounds.start, bounds.end + 1),
    };
  } catch {
    return undefined;
  }
}

function firstArgumentText(source, openIndex, closeIndex) {
  const literal = parseLiteralAt(source, openIndex + 1);
  if (literal) {
    return String(literal.value).trim().replace(/\s+/g, ' ') || '<empty>';
  }

  let cursor = skipWhitespace(source, openIndex + 1);
  let state = 'code';
  while (cursor < closeIndex) {
    const char = source[cursor];
    const next = source[cursor + 1];
    if (state === 'line-comment') {
      if (char === '\n') state = 'code';
      cursor += 1;
      continue;
    }
    if (state === 'block-comment') {
      if (char === '*' && next === '/') {
        cursor += 2;
        state = 'code';
      } else {
        cursor += 1;
      }
      continue;
    }
    if (state === 'single' || state === 'double' || state === 'template') {
      const end = state === 'single' ? "'" : state === 'double' ? '"' : '`';
      if (char === '\\') cursor += 2;
      else if (char === end) {
        cursor += 1;
        state = 'code';
      } else {
        cursor += 1;
      }
      continue;
    }
    if (char === '/' && next === '/') {
      cursor += 2;
      state = 'line-comment';
      continue;
    }
    if (char === '/' && next === '*') {
      cursor += 2;
      state = 'block-comment';
      continue;
    }
    if (char === "'") {
      cursor += 1;
      state = 'single';
      continue;
    }
    if (char === '"') {
      cursor += 1;
      state = 'double';
      continue;
    }
    if (char === '`') {
      cursor += 1;
      state = 'template';
      continue;
    }
    if (char === ',') {
      break;
    }
    cursor += 1;
  }

  const expression = source.slice(skipWhitespace(source, openIndex + 1), cursor).trim();
  return expression ? `<dynamic:${expression.replace(/\s+/g, ' ').slice(0, 80)}>` : '<unknown>';
}

function findBlockRange(source, startIndex) {
  const open = source.indexOf('{', startIndex);
  if (open === -1) {
    return undefined;
  }
  const close = findMatching(source, open, '{', '}');
  if (close === undefined) {
    return undefined;
  }
  return { start: open, end: close };
}

function parseTestLikeCall(source, idStart, idEnd, name) {
  let cursor = skipWhitespace(source, idEnd);
  const declaration = [name];
  while (source[cursor] === '.') {
    cursor += 1;
    cursor = skipWhitespace(source, cursor);
    if (!isIdentifierStart(source[cursor])) {
      return undefined;
    }
    const propStart = cursor;
    cursor += 1;
    while (isIdentifierPart(source[cursor])) {
      cursor += 1;
    }
    declaration.push(source.slice(propStart, cursor));
    cursor = skipWhitespace(source, cursor);
  }

  if (source[cursor] !== '(') {
    return undefined;
  }

  const firstOpen = cursor;
  const firstClose = findMatching(source, firstOpen, '(', ')');
  if (firstClose === undefined) {
    return undefined;
  }

  let titleOpen = firstOpen;
  let titleClose = firstClose;
  const afterFirst = skipWhitespace(source, firstClose + 1);
  if (source[afterFirst] === '(') {
    const secondClose = findMatching(source, afterFirst, '(', ')');
    if (secondClose === undefined) {
      return undefined;
    }
    titleOpen = afterFirst;
    titleClose = secondClose;
  }

  const titleLiteral = parseLiteralAt(source, titleOpen + 1);

  return {
    declaration: declaration.join('.'),
    name: firstArgumentText(source, titleOpen, titleClose),
    start: idStart,
    end: titleClose,
    block: findBlockRange(source, titleLiteral ? titleLiteral.end + 1 : titleOpen + 1),
  };
}

function scanIdentifiers(source, callback) {
  let state = 'code';
  for (let index = 0; index < source.length; index += 1) {
    const char = source[index];
    const next = source[index + 1];

    if (state === 'line-comment') {
      if (char === '\n') state = 'code';
      continue;
    }
    if (state === 'block-comment') {
      if (char === '*' && next === '/') {
        index += 1;
        state = 'code';
      }
      continue;
    }
    if (state === 'single') {
      if (char === '\\') index += 1;
      else if (char === "'") state = 'code';
      continue;
    }
    if (state === 'double') {
      if (char === '\\') index += 1;
      else if (char === '"') state = 'code';
      continue;
    }
    if (state === 'template') {
      if (char === '\\') index += 1;
      else if (char === '`') state = 'code';
      continue;
    }

    if (char === '/' && next === '/') {
      index += 1;
      state = 'line-comment';
      continue;
    }
    if (char === '/' && next === '*') {
      index += 1;
      state = 'block-comment';
      continue;
    }
    if (char === "'") {
      state = 'single';
      continue;
    }
    if (char === '"') {
      state = 'double';
      continue;
    }
    if (char === '`') {
      state = 'template';
      continue;
    }

    if (!isIdentifierStart(char)) {
      continue;
    }
    const start = index;
    index += 1;
    while (isIdentifierPart(source[index])) {
      index += 1;
    }
    callback(source.slice(start, index), start, index);
    index -= 1;
  }
}

function extractTestCases(relativePath) {
  const source = readText(relativePath);
  const starts = lineStarts(source);
  const describes = [];
  const cases = [];

  scanIdentifiers(source, (identifier, start, end) => {
    if (identifier !== 'describe' && identifier !== 'it' && identifier !== 'test') {
      return;
    }
    const call = parseTestLikeCall(source, start, end, identifier);
    if (!call) {
      return;
    }
    if (identifier === 'describe') {
      if (call.block) {
        describes.push(call);
      }
      return;
    }
    cases.push({
      file: relativePath,
      line: lineNumberAt(starts, start),
      declaration: call.declaration,
      name: call.name,
      start,
      block: call.block,
    });
  });

  describes.sort((left, right) => left.start - right.start);
  for (const row of cases) {
    row.suite =
      describes
        .filter((suite) => suite.block && suite.block.start < row.start && row.start < suite.block.end)
        .map((suite) => suite.name)
        .join(' > ') || '(top-level)';
  }
  return { source, starts, cases };
}

function splitTopLevelArgs(argsText) {
  const args = [];
  let depth = 0;
  let start = 0;
  let state = 'code';
  for (let index = 0; index < argsText.length; index += 1) {
    const char = argsText[index];
    const next = argsText[index + 1];

    if (state === 'line-comment') {
      if (char === '\n') state = 'code';
      continue;
    }
    if (state === 'block-comment') {
      if (char === '*' && next === '/') {
        index += 1;
        state = 'code';
      }
      continue;
    }
    if (state === 'single') {
      if (char === '\\') index += 1;
      else if (char === "'") state = 'code';
      continue;
    }
    if (state === 'double') {
      if (char === '\\') index += 1;
      else if (char === '"') state = 'code';
      continue;
    }
    if (state === 'template') {
      if (char === '\\') index += 1;
      else if (char === '`') state = 'code';
      continue;
    }

    if (char === '/' && next === '/') {
      index += 1;
      state = 'line-comment';
      continue;
    }
    if (char === '/' && next === '*') {
      index += 1;
      state = 'block-comment';
      continue;
    }
    if (char === "'") {
      state = 'single';
      continue;
    }
    if (char === '"') {
      state = 'double';
      continue;
    }
    if (char === '`') {
      state = 'template';
      continue;
    }

    if (char === '(' || char === '[' || char === '{') {
      depth += 1;
    } else if (char === ')' || char === ']' || char === '}') {
      depth -= 1;
    } else if (char === ',' && depth === 0) {
      args.push(argsText.slice(start, index).trim());
      start = index + 1;
    }
  }
  const tail = argsText.slice(start).trim();
  if (tail) {
    args.push(tail);
  }
  return args;
}

function parseExpressionLiteral(expression) {
  const trimmed = expression.trim();
  if (!trimmed) {
    return undefined;
  }
  try {
    return Function(`"use strict"; return (${trimmed});`)();
  } catch {
    return undefined;
  }
}

function findNamedCall(source, name, fromIndex = 0) {
  let cursor = fromIndex;
  while (cursor < source.length) {
    const index = source.indexOf(name, cursor);
    if (index === -1) {
      return undefined;
    }
    if (
      (index > 0 && isIdentifierPart(source[index - 1])) ||
      isIdentifierPart(source[index + name.length])
    ) {
      cursor = index + name.length;
      continue;
    }
    const open = skipWhitespace(source, index + name.length);
    if (source[open] !== '(') {
      cursor = index + name.length;
      continue;
    }
    const close = findMatching(source, open, '(', ')');
    if (close === undefined) {
      return undefined;
    }
    return { start: index, open, close, args: splitTopLevelArgs(source.slice(open + 1, close)) };
  }
  return undefined;
}

function allNamedCalls(source, name) {
  const calls = [];
  let cursor = 0;
  while (cursor < source.length) {
    const call = findNamedCall(source, name, cursor);
    if (!call) {
      break;
    }
    calls.push(call);
    cursor = call.close + 1;
  }
  return calls;
}

function parseNewBashOptions(block, beforeIndex) {
  const prefix = block.slice(0, beforeIndex);
  const marker = 'new Bash';
  const start = prefix.lastIndexOf(marker);
  if (start === -1) {
    return {};
  }
  const open = skipWhitespace(block, start + marker.length);
  if (block[open] !== '(') {
    return {};
  }
  const close = findMatching(block, open, '(', ')');
  if (close === undefined || close > beforeIndex) {
    return {};
  }
  const args = splitTopLevelArgs(block.slice(open + 1, close));
  const value = args[0] ? parseExpressionLiteral(args[0]) : undefined;
  if (!value || typeof value !== 'object') {
    return {};
  }
  return {
    cwd: typeof value.cwd === 'string' ? value.cwd : undefined,
    env: value.env && typeof value.env === 'object' ? value.env : undefined,
    initialFiles: value.files && typeof value.files === 'object' ? value.files : undefined,
  };
}

function parseSetupFiles(block, beforeIndex) {
  const prefix = block.slice(0, beforeIndex);
  const start = prefix.lastIndexOf('setupFiles');
  if (start === -1) {
    return {};
  }
  const call = findNamedCall(block, 'setupFiles', start);
  if (!call || call.start > beforeIndex || call.close > beforeIndex) {
    return {};
  }
  const files = call.args[1] ? parseExpressionLiteral(call.args[1]) : undefined;
  if (!files || typeof files !== 'object') {
    return {};
  }
  return { cwd: '.', initialFiles: files };
}

function assertionValue(block, variable, property, matcher) {
  const needle = `expect(${variable}.${property})`;
  let cursor = 0;
  while (cursor < block.length) {
    const start = block.indexOf(needle, cursor);
    if (start === -1) {
      return undefined;
    }
    const matcherIndex = block.indexOf(`.${matcher}`, start + needle.length);
    if (matcherIndex === -1) {
      return undefined;
    }
    const between = block.slice(start + needle.length, matcherIndex);
    if (between.includes('expect(')) {
      cursor = start + needle.length;
      continue;
    }
    const open = skipWhitespace(block, matcherIndex + matcher.length + 1);
    if (block[open] !== '(') {
      cursor = start + needle.length;
      continue;
    }
    const close = findMatching(block, open, '(', ')');
    if (close === undefined) {
      return undefined;
    }
    const arg = splitTopLevelArgs(block.slice(open + 1, close))[0];
    return parseExpressionLiteral(arg);
  }
  return undefined;
}

function assertionList(block, variable) {
  const assertions = [];
  const pattern = new RegExp(
    `expect\\(\\s*${variable}\\.(stdout|stderr|exitCode)\\s*\\)((?:\\.not)?\\.to(?:Be|Contain))`,
    'g'
  );
  let match;
  while ((match = pattern.exec(block))) {
    const open = skipWhitespace(block, pattern.lastIndex);
    if (block[open] !== '(') {
      continue;
    }
    const close = findMatching(block, open, '(', ')');
    if (close === undefined) {
      continue;
    }
    assertions.push({
      property: match[1],
      matcher: match[2],
      value: parseExpressionLiteral(splitTopLevelArgs(block.slice(open + 1, close))[0]),
    });
  }
  return assertions;
}

function findExecCalls(block) {
  const calls = [];
  let cursor = 0;
  while (cursor < block.length) {
    const execIndex = block.indexOf('.exec', cursor);
    if (execIndex === -1) {
      break;
    }
    const open = skipWhitespace(block, execIndex + '.exec'.length);
    if (block[open] !== '(') {
      cursor = execIndex + 5;
      continue;
    }
    const close = findMatching(block, open, '(', ')');
    if (close === undefined) {
      break;
    }
    const args = splitTopLevelArgs(block.slice(open + 1, close));
    const command = args[0] ? parseExpressionLiteral(args[0]) : undefined;
    if (typeof command !== 'string') {
      cursor = close + 1;
      continue;
    }
    const lookback = block.slice(Math.max(0, execIndex - 120), execIndex);
    const assignment = /(?:const|let)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*await\s+[A-Za-z0-9_$.[\]]+$/.exec(
      lookback.trimEnd()
    );
    calls.push({
      command,
      args: args.slice(1),
      start: execIndex,
      end: close,
      variable: assignment?.[1],
    });
    cursor = close + 1;
  }
  return calls;
}

function parityFor(testCase, parityRows) {
  const key = ledgerKey(testCase);
  const row = key ? parityRows.get(key) : undefined;
  if (!row) {
    return {
      ledgerKey: key ?? null,
      status: 'portable-pending',
      owner: 'pending:just-bash-conformance-corpus',
      rustTest: 'pending:just-bash-conformance-corpus',
      notes: 'Corpus source row did not resolve to a just-bash parity ledger row.',
      found: false,
    };
  }
  return {
    ledgerKey: key,
    status: row.status,
    owner: row.owner,
    rustTest: row.rustTest,
    notes: row.notes,
    found: true,
  };
}

function commandDomain(command, fallback = 'unknown') {
  const normalized = command.trim();
  if (/\bawk\b/.test(normalized)) {
    return 'awk';
  }
  if (/\bgrep\b/.test(normalized)) {
    return 'grep';
  }
  if (/\bsed\b/.test(normalized)) {
    return 'sed';
  }
  if (/\b(?:cd|pwd|env|printenv|export)\b/.test(normalized)) {
    return 'pwd-cd-env';
  }
  if (/\b(?:mkdir|rm|cp|mv|touch)\b/.test(normalized)) {
    return 'file-ops';
  }
  if (/^printf\b/.test(normalized)) {
    return 'printf';
  }
  if (/^(?:echo\b|.*\|\s*echo\b)/.test(normalized)) {
    return 'echo';
  }
  return fallback;
}

function baseCase({
  id,
  kind,
  classification,
  domain,
  upstream,
  command,
  initialFiles = {},
  cwd = '.',
  env = {},
  stdin = null,
  args = [],
  options = {},
  expected,
  parity,
  sourceFixtureMetadata = {},
  assertions = [],
}) {
  return {
    id,
    kind,
    classification,
    domain,
    upstream,
    command,
    script: command,
    initialFiles,
    cwd,
    env,
    stdin,
    args,
    options,
    expected: {
      stdout: expected?.stdout ?? null,
      stderr: expected?.stderr ?? null,
      exitCode: expected?.exitCode ?? null,
    },
    status: parity.status,
    parity,
    assertions,
    sourceFixtureMetadata,
  };
}

function buildComparisonCaseIndex(relativePath, parityRows) {
  if (!fs.existsSync(path.join(upstreamRoot, relativePath))) {
    return { commands: new Map(), dynamicCases: [] };
  }
  const { source, cases } = extractTestCases(relativePath);
  const index = new Map();
  const dynamicCases = [];

  for (const testCase of cases) {
    if (!testCase.block) {
      continue;
    }
    const block = source.slice(testCase.block.start, testCase.block.end + 1);
    let staticCommandCount = 0;
    for (const call of allNamedCalls(block, 'compareOutputs')) {
      const command = call.args[2] ? parseExpressionLiteral(call.args[2]) : undefined;
      if (typeof command !== 'string') {
        continue;
      }
      staticCommandCount += 1;
      const current = index.get(command) ?? [];
      current.push({
        ...testCase,
        command,
        options: call.args[3] ? parseExpressionLiteral(call.args[3]) ?? {} : {},
        parity: parityFor(testCase, parityRows),
      });
      index.set(command, current);
    }
    if (block.includes('compareOutputs') && staticCommandCount === 0) {
      dynamicCases.push({
        ...testCase,
        searchText: `${testCase.suite} ${testCase.name} ${block}`.toLowerCase(),
        parity: parityFor(testCase, parityRows),
      });
    }
  }
  return { commands: index, dynamicCases };
}

function commandWords(command) {
  return command
    .split(/[^A-Za-z0-9_-]+/)
    .map((word) => word.trim().toLowerCase())
    .filter((word) => knownCommandWords.has(word));
}

function takeDynamicComparisonCase(index, command) {
  const words = commandWords(command);
  const matchIndex = index.dynamicCases.findIndex((testCase) =>
    words.some((word) => testCase.searchText.includes(word))
  );
  if (matchIndex === -1) {
    return undefined;
  }
  return index.dynamicCases.splice(matchIndex, 1)[0];
}

function buildComparisonFixtureCases(parityRows) {
  const fixtureDir = path.join(upstreamRoot, comparisonFixturesRoot);
  const fixtureFiles = walk(fixtureDir)
    .map(upstreamRelative)
    .filter((file) => file.endsWith('.comparison.fixtures.json'))
    .sort();

  const cases = [];
  const testCaseIndexes = new Map();
  for (const fixtureFile of fixtureFiles) {
    const baseName = path
      .basename(fixtureFile, '.comparison.fixtures.json')
      .replace(/\.comparison$/, '');
    const testFile = `${comparisonRoot}/${baseName}.comparison.test.ts`;
    if (!testCaseIndexes.has(testFile)) {
      testCaseIndexes.set(testFile, buildComparisonCaseIndex(testFile, parityRows));
    }
    const testIndex = testCaseIndexes.get(testFile);
    const fixtures = readJson(fixtureFile);
    for (const [fixtureId, fixture] of Object.entries(fixtures)) {
      const expectedId = fixtureIdFor(fixture.command, fixture.files ?? {});
      const mapped =
        testIndex.commands.get(fixture.command)?.shift() ??
        takeDynamicComparisonCase(testIndex, fixture.command);
      const syntheticTestCase =
        mapped ??
        {
          file: testFile,
          line: 0,
          declaration: 'it',
          name: `<fixture:${fixtureId}>`,
          suite: '(fixture-only)',
          parity: {
            ledgerKey: null,
            status: 'portable-pending',
            owner: 'pending:just-bash-spec-comparison',
            rustTest: 'pending:just-bash-spec-comparison',
            notes: 'Fixture row could not be matched to a static compareOutputs call.',
            found: false,
          },
        };
      const parity = mapped?.parity ?? syntheticTestCase.parity;
      cases.push(
        baseCase({
          id: `comparison:${baseName}:${fixtureId}`,
          kind: 'comparison-fixture',
          classification: 'comparison-golden',
          domain: commandDomain(fixture.command, baseName),
          upstream: {
            repository: upstreamRepo,
            file: syntheticTestCase.file,
            line: syntheticTestCase.line || null,
            suite: syntheticTestCase.suite,
            testName: syntheticTestCase.name,
            declaration: syntheticTestCase.declaration,
            fixtureFile,
            fixtureId,
          },
          command: fixture.command,
          initialFiles: fixture.files ?? {},
          cwd: '.',
          env: {},
          stdin: null,
          args: [],
          options: mapped?.options ?? {},
          expected: {
            stdout: fixture.stdout,
            stderr: fixture.stderr,
            exitCode: fixture.exitCode,
          },
          parity,
          sourceFixtureMetadata: {
            fixtureId,
            expectedFixtureId: expectedId,
            fixtureIdMatchesCommandAndFiles: expectedId === fixtureId,
            locked: fixture.locked === true,
            upstreamFixtureFile: fixtureFile,
            commandHashAlgorithm: 'sha256(command + "|||" + sorted(files)).slice(0, 16)',
          },
        })
      );
    }
  }
  return cases.sort((left, right) => left.id.localeCompare(right.id));
}

function buildUnitExecCases(parityRows) {
  const cases = [];

  for (const relativePath of unitSourceFiles) {
    if (!fs.existsSync(path.join(upstreamRoot, relativePath))) {
      continue;
    }
    const { source, cases: testCases } = extractTestCases(relativePath);
    for (const testCase of testCases) {
      if (!testCase.block) {
        continue;
      }
      const block = source.slice(testCase.block.start, testCase.block.end + 1);
      const execCalls = findExecCalls(block);
      for (let index = 0; index < execCalls.length; index += 1) {
        const call = execCalls[index];
        const newBashOptions = parseNewBashOptions(block, call.start);
        const setupFilesOptions = parseSetupFiles(block, call.start);
        const variable = call.variable;
        const expected = variable
          ? {
              stdout: assertionValue(block, variable, 'stdout', 'toBe'),
              stderr: assertionValue(block, variable, 'stderr', 'toBe'),
              exitCode: assertionValue(block, variable, 'exitCode', 'toBe'),
            }
          : {};
        const assertions = variable ? assertionList(block, variable) : [];
        const hasGolden =
          expected.stdout !== undefined ||
          expected.stderr !== undefined ||
          expected.exitCode !== undefined;
        const parity = parityFor(testCase, parityRows);
        const idSuffix = execCalls.length === 1 ? '' : `:${index + 1}`;
        cases.push(
          baseCase({
            id: `unit:${relativePath}:${testCase.line}${idSuffix}`,
            kind: 'unit-exec',
            classification: hasGolden ? 'unit-golden' : 'unit-source-only',
            domain: commandDomain(call.command, path.basename(path.dirname(relativePath))),
            upstream: {
              repository: upstreamRepo,
              file: relativePath,
              line: testCase.line,
              suite: testCase.suite,
              testName: testCase.name,
              declaration: testCase.declaration,
            },
            command: call.command,
            initialFiles: newBashOptions.initialFiles ?? setupFilesOptions.initialFiles ?? {},
            cwd: newBashOptions.cwd ?? setupFilesOptions.cwd ?? '.',
            env: newBashOptions.env ?? {},
            args: call.args,
            options: {},
            expected,
            parity,
            assertions,
            sourceFixtureMetadata: {
              sourceKind: 'vitest-env-exec',
              hasExactStdout: expected.stdout !== undefined,
              hasExactStderr: expected.stderr !== undefined,
              hasExactExitCode: expected.exitCode !== undefined,
            },
          })
        );
      }
    }
  }

  return cases.sort((left, right) => left.id.localeCompare(right.id));
}

function parseShellQuotedArgs(text) {
  const args = [];
  for (let index = 0; index < text.length; index += 1) {
    if (text[index] !== '"' && text[index] !== "'") {
      continue;
    }
    const quote = text[index];
    let value = '';
    for (let cursor = index + 1; cursor < text.length; cursor += 1) {
      const char = text[cursor];
      if (char === '\\') {
        const next = text[cursor + 1];
        if (next === 'n') value += '\n';
        else if (next === 't') value += '\t';
        else if (next === 'r') value += '\r';
        else value += next ?? '';
        cursor += 1;
      } else if (char === quote) {
        args.push(value);
        index = cursor;
        break;
      } else {
        value += char;
      }
    }
  }
  return args;
}

function logicalShellLines(content) {
  const lines = content.split(/\r?\n/);
  const logical = [];
  let current = '';
  let startLine = 1;
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (!current) {
      startLine = index + 1;
    }
    if (line.endsWith('\\')) {
      current += `${line.slice(0, -1)} `;
    } else {
      logical.push({ line: current + line, lineNumber: startLine });
      current = '';
    }
  }
  if (current) {
    logical.push({ line: current, lineNumber: startLine });
  }
  return logical;
}

function parseBusyBoxSpec(content, relativePath, commandName) {
  const cases = [];
  for (const entry of logicalShellLines(content)) {
    const trimmed = entry.line.trim();
    if (!trimmed.startsWith('testing ')) {
      continue;
    }
    const args = parseShellQuotedArgs(trimmed);
    if (args.length < 5) {
      continue;
    }
    const [name, command, expectedOutput, infile, stdin] = args;
    cases.push({
      name,
      command,
      expectedOutput,
      expectedExitCode: null,
      stdin: stdin || null,
      initialFiles: infile ? { input: infile } : {},
      file: relativePath,
      line: entry.lineNumber,
      commandName,
    });
  }
  return cases;
}

function parseGnuGrepSpec(content, relativePath, isEre) {
  const cases = [];
  let pendingSkip;
  const lines = content.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index].trim();
    if (!line) {
      continue;
    }
    const skip = /^#\s*SKIP:\s*(.+)$/.exec(line);
    if (skip) {
      pendingSkip = skip[1];
      continue;
    }
    if (line.startsWith('#')) {
      continue;
    }
    const parts = line.split('@');
    if (parts.length < 3) {
      continue;
    }
    const expectedExitCode = Number.parseInt(parts[0], 10);
    if (!Number.isInteger(expectedExitCode)) {
      continue;
    }
    const pattern = parts[1];
    const testString = parts[2];
    const note = parts.slice(3).join('@');
    const escapedPattern = pattern.replace(/'/g, "'\\''");
    let expectedOutput = '';
    let stdin = 'test\n';
    if (expectedExitCode === 0) {
      expectedOutput = `${testString}\n`;
      stdin = `${testString}\n`;
    } else if (expectedExitCode === 1) {
      stdin = `${testString}\n`;
    }
    cases.push({
      name: `${isEre ? 'ERE' : 'BRE'}: /${pattern}/ vs "${testString}"${note ? ` (${note})` : ''}`,
      command: `grep ${isEre ? '-E ' : ''}'${escapedPattern}'`,
      expectedOutput,
      expectedExitCode,
      stdin,
      initialFiles: {},
      file: relativePath,
      line: index + 1,
      commandName: 'grep',
      skip: pendingSkip,
    });
    pendingSkip = undefined;
  }
  return cases;
}

function buildSedCommand(script) {
  const lines = script.split('\n').filter((line) => line.trim() !== '');
  if (lines.length === 0) {
    return "sed ''";
  }
  if (lines.length === 1) {
    return `sed '${lines[0].replace(/'/g, "'\\''")}'`;
  }
  return `sed ${lines.map((line) => `-e '${line.replace(/'/g, "'\\''")}'`).join(' ')}`;
}

function parsePythonSedSuite(content, relativePath) {
  const lines = content.split(/\r?\n/);
  const cases = [];
  let index = 0;
  while (index < lines.length) {
    while (index < lines.length && lines[index].trim() !== '---') {
      index += 1;
    }
    if (index >= lines.length) break;
    const startLine = index + 1;
    index += 1;
    const description = [];
    while (index < lines.length && lines[index].trim() !== '---') {
      description.push(lines[index]);
      index += 1;
    }
    index += 1;
    const script = [];
    while (index < lines.length && lines[index].trim() !== '---') {
      script.push(lines[index]);
      index += 1;
    }
    index += 1;
    const input = [];
    while (index < lines.length && lines[index].trim() !== '---') {
      input.push(lines[index]);
      index += 1;
    }
    index += 1;
    const output = [];
    while (index < lines.length && lines[index].trim() !== '---') {
      output.push(lines[index]);
      index += 1;
    }
    if (index < lines.length && lines[index].trim() === '---') {
      index += 1;
    }

    const name = description.join('\n').trim();
    const scriptText = script.join('\n').trim();
    if (!scriptText || name.startsWith('**')) {
      continue;
    }
    let stdin = input.join('\n');
    if (stdin !== '') {
      stdin += '\n';
    }
    let expectedOutput = output.join('\n');
    if (expectedOutput !== '' && expectedOutput !== '???') {
      expectedOutput += '\n';
    }
    if (stdin.trim() === '' && expectedOutput.trim() === '') {
      continue;
    }
    if (stdin.trim() === '' && expectedOutput.trim() !== '') {
      stdin = '1\nTAG\n2\n';
    }
    cases.push({
      name: name || `test at line ${startLine}`,
      command: buildSedCommand(scriptText),
      expectedOutput,
      expectedExitCode: expectedOutput === '???' ? 1 : null,
      stdin,
      initialFiles: {},
      file: relativePath,
      line: startLine,
      commandName: 'sed',
    });
  }
  return cases;
}

function buildSpecCases(parityRows) {
  const specs = [];
  const grepCasesDir = path.join(upstreamRoot, 'packages/just-bash/src/spec-tests/grep/cases');
  for (const absolutePath of walk(grepCasesDir).sort()) {
    if (!absolutePath.endsWith('.tests')) {
      continue;
    }
    const relativePath = upstreamRelative(absolutePath);
    const fileName = path.basename(absolutePath);
    const content = fs.readFileSync(absolutePath, 'utf8');
    if (fileName.startsWith('gnu-')) {
      specs.push(...parseGnuGrepSpec(content, relativePath, fileName.includes('ere') || fileName.includes('spencer')));
    } else {
      specs.push(...parseBusyBoxSpec(content, relativePath, 'grep'));
    }
  }

  const sedCasesDir = path.join(upstreamRoot, 'packages/just-bash/src/spec-tests/sed/cases');
  for (const absolutePath of walk(sedCasesDir).sort()) {
    if (!absolutePath.endsWith('.tests') && !absolutePath.endsWith('.suite')) {
      continue;
    }
    const relativePath = upstreamRelative(absolutePath);
    const content = fs.readFileSync(absolutePath, 'utf8');
    if (absolutePath.endsWith('.suite')) {
      specs.push(...parsePythonSedSuite(content, relativePath));
    } else {
      specs.push(...parseBusyBoxSpec(content, relativePath, 'sed'));
    }
  }

  return specs
    .map((specCase, index) => {
      const runner = specRunnerRows[specCase.commandName];
      const parity = parityFor(runner, parityRows);
      return baseCase({
        id: `spec:${specCase.commandName}:${sha256(`${specCase.file}:${specCase.line}:${specCase.name}:${index}`).slice(0, 16)}`,
        kind: 'spec-case',
        classification: specCase.skip ? 'spec-skipped-golden' : 'spec-golden',
        domain: specCase.commandName,
        upstream: {
          repository: upstreamRepo,
          file: runner.file,
          line: runner.line,
          suite: '(dynamic spec runner)',
          testName: runner.name,
          declaration: runner.declaration,
          sourceCaseFile: specCase.file,
          sourceCaseLine: specCase.line,
          sourceCaseName: specCase.name,
        },
        command: specCase.command,
        initialFiles: specCase.initialFiles,
        cwd: '.',
        stdin: specCase.stdin,
        expected: {
          stdout: specCase.expectedOutput,
          stderr: null,
          exitCode: specCase.expectedExitCode,
        },
        parity,
        assertions: specCase.skip
          ? [{ property: 'skip', matcher: 'reason', value: specCase.skip }]
          : [],
        sourceFixtureMetadata: {
          sourceKind: `${specCase.commandName}-spec-parser`,
          sourceCaseFile: specCase.file,
          sourceCaseLine: specCase.line,
          skip: specCase.skip ?? null,
        },
      });
    })
    .sort((left, right) => left.id.localeCompare(right.id));
}

function summarizeCases(cases) {
  const byKind = {};
  const byDomain = {};
  const byStatus = {};
  const byClassification = {};
  for (const testCase of cases) {
    byKind[testCase.kind] = (byKind[testCase.kind] ?? 0) + 1;
    byDomain[testCase.domain] = (byDomain[testCase.domain] ?? 0) + 1;
    byStatus[testCase.status] = (byStatus[testCase.status] ?? 0) + 1;
    byClassification[testCase.classification] =
      (byClassification[testCase.classification] ?? 0) + 1;
  }
  return { byKind, byDomain, byStatus, byClassification };
}

function validateCorpus(corpus) {
  const errors = [];
  if (corpus.cases.length < expectedMinimums.totalCases) {
    errors.push(
      `expected at least ${expectedMinimums.totalCases} cases, found ${corpus.cases.length}`
    );
  }
  if ((corpus.summary.byKind['comparison-fixture'] ?? 0) < expectedMinimums.comparisonCases) {
    errors.push('comparison fixture slice is unexpectedly small');
  }
  if ((corpus.summary.byKind['unit-exec'] ?? 0) < expectedMinimums.unitCases) {
    errors.push('unit exec slice is unexpectedly small');
  }
  if ((corpus.summary.byKind['spec-case'] ?? 0) < expectedMinimums.specCases) {
    errors.push('spec case slice is unexpectedly small');
  }
  for (const domain of expectedMinimums.representativeDomains) {
    if (!corpus.summary.byDomain[domain]) {
      errors.push(`missing representative domain: ${domain}`);
    }
  }
  for (const testCase of corpus.cases) {
    if (!testCase.id || !testCase.command || !testCase.upstream?.file) {
      errors.push(`${testCase.id ?? '<missing-id>'}: missing id, command, or upstream file`);
    }
    if (!testCase.parity?.ledgerKey) {
      errors.push(`${testCase.id}: missing parity ledger key`);
    }
    if (!testCase.status) {
      errors.push(`${testCase.id}: missing status`);
    }
    if (!Object.hasOwn(testCase.expected, 'stdout')) {
      errors.push(`${testCase.id}: missing expected.stdout field`);
    }
    if (!Object.hasOwn(testCase.expected, 'stderr')) {
      errors.push(`${testCase.id}: missing expected.stderr field`);
    }
    if (!Object.hasOwn(testCase.expected, 'exitCode')) {
      errors.push(`${testCase.id}: missing expected.exitCode field`);
    }
  }
  return errors;
}

function buildCorpus() {
  if (!fs.existsSync(upstreamRoot)) {
    fail(
      `upstream path not found: ${upstreamRoot}; run npx opensrc fetch ${upstreamUrl}`
    );
  }

  const remoteHead = liveUpstreamHead();
  if (remoteHead !== expectedUpstreamHead) {
    fail(
      `upstream drift: expected ${expectedUpstreamHead}, got ${remoteHead}; refresh corpus and update script metadata`
    );
  }

  const parityRows = readParityRows();
  const comparisonCases = buildComparisonFixtureCases(parityRows);
  const unitCases = buildUnitExecCases(parityRows);
  const specCases = buildSpecCases(parityRows);
  const cases = [...comparisonCases, ...unitCases, ...specCases].sort((left, right) =>
    left.id.localeCompare(right.id)
  );
  const summary = summarizeCases(cases);

  return {
    schemaVersion: 1,
    generatedOn: inventoryDate,
    source: {
      repository: upstreamRepo,
      upstreamUrl,
      upstreamHead: remoteHead,
      upstreamHeadVerification: `git ls-remote ${upstreamUrl} HEAD refs/heads/main`,
      localSourcePath: upstreamRoot,
      comparisonFixturesRoot,
      parityLedger: 'docs/open-agents/just-bash-parity.md',
    },
    runnerContract: {
      cwd: 'Relative paths resolve inside an isolated virtual test root.',
      initialFiles: 'Map of relative or absolute virtual paths to UTF-8 file contents.',
      stdin: 'String stdin for the command, or null when stdin is not specified.',
      expected:
        'stdout/stderr/exitCode are exact golden values when non-null; null means the upstream source row did not provide an exact value.',
      parity:
        'Each case carries a docs/open-agents/just-bash-parity.md row key in parity.ledgerKey.',
    },
    summary: {
      totalCases: cases.length,
      ...summary,
    },
    cases,
  };
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

function renderDocs(corpus) {
  const kindRows = Object.entries(corpus.summary.byKind)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([kind, count]) => [kind, count]);
  const domainRows = Object.entries(corpus.summary.byDomain)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([domain, count]) => [domain, count]);
  const statusRows = Object.entries(corpus.summary.byStatus)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([status, count]) => [status, count]);

  return `# Just Bash Conformance Plan

This plan extends the parent TypeScript-to-Rust parity goal with a shared
conformance path for \`vercel-labs/just-bash\`.

## Objective

The Rust \`crates/just-bash\` engine must run the same portable command behavior
as upstream TypeScript Just Bash. The proof path is not a hand-picked Rust test
suite: upstream Just Bash cases are inventoried first, then each portable case
is mapped to a named Rust test, a generated conformance-corpus case, or an
explicit \`js-only-documented\` / \`type-system-impossible\` exception.

The shared harness must support running the same case corpus against both
engines:

- \`JUST_BASH_ENGINE=typescript\` runs upstream TypeScript Just Bash.
- \`JUST_BASH_ENGINE=rust\` runs Rust Just Bash through a small \`napi-rs\` bridge.

Rust-specific tests may add coverage, but they never replace the upstream case
inventory. Strict parity closes only when every portable upstream row in
\`docs/open-agents/just-bash-parity.md\` is verified or explicitly excepted.

## Source Snapshot

| Field | Value |
| --- | --- |
| Upstream repo | \`${corpus.source.repository}\` |
| Upstream refresh | \`npx opensrc fetch https://github.com/vercel-labs/just-bash\` |
| Upstream verification | \`${corpus.source.upstreamHeadVerification}\` |
| Current tracked upstream commit | \`${corpus.source.upstreamHead}\` |
| OpenSrc cache | \`${corpus.source.localSourcePath}\` |
| Corpus path | \`fixtures/just-bash-conformance/corpus.json\` |
| Parity ledger | \`${corpus.source.parityLedger}\` |
| Corpus check command | \`node scripts/just-bash-conformance-corpus.mjs --check\` |

## Work Buckets

| ID | Scope | Required output | Verification |
| --- | --- | --- | --- |
| JBC-01 | Rust-to-JavaScript NAPI adapter | \`crates/just-bash-napi\` exposes the Rust engine to JS with constructor/session setup, \`exec\`, virtual filesystem helpers, cwd/env helpers, and command discovery. | \`cargo test -p just-bash-napi\`; \`cargo clippy -p just-bash-napi --all-targets --all-features -- -D warnings\`; \`npm test --prefix crates/just-bash-napi\`. |
| JBC-02 | JavaScript dual-engine harness | A JS runner executes selected upstream cases with \`JUST_BASH_ENGINE=typescript\` or \`JUST_BASH_ENGINE=rust\` without polluting the upstream OpenSrc inventory. | TypeScript-engine smoke; Rust-engine smoke or explicit missing-addon diagnostic; \`node scripts/just-bash-test-inventory.mjs --check\`. |
| JBC-03 | Conformance corpus generator | Stable JSON corpus with upstream case ids, command domains, fixtures, env/cwd/stdin, expected stdout/stderr/exit code, and ledger links. | Corpus generator dry run; fixture hash check; no missing ledger links. |
| JBC-04 | Rust corpus runner | Data-driven Rust tests load the shared corpus, seed virtual state, run \`just_bash::Bash\`, and report upstream ids on mismatch. | \`cargo test -p just-bash --test conformance_corpus\`; \`cargo test -p just-bash\`. |
| JBC-05 | Ledger and CI gates | This plan, tracker rows, master-gate integration, and documented strict close criteria. | \`node scripts/just-bash-test-inventory.mjs --check\`; \`scripts/master-parity-gate.sh --check\`. |
| JBC-06 | Core command parity | Close portable filesystem command rows such as \`cat\`, \`ls\`, \`mkdir\`, \`rm\`, \`cp\`, and \`mv\` with named Rust tests and ledger mappings. | Focused command tests; inventory check; shared fmt/clippy/naming/diff gates. |
| JBC-07 | Text and structured command parity | Close text/search/structured command rows such as \`grep\`, \`rg\`, \`sed\`, \`awk\`, \`head\`, \`tail\`, \`wc\`, \`sort\`, \`uniq\`, \`cut\`, \`tr\`, and \`jq\`. | Focused text/search/structured tests; corpus subset; inventory check; shared fmt/clippy/naming/diff gates. |
| JBC-08 | Open Agents service proof | Prove Open Agents service/local-E2E reaches crate-backed Just Bash, persists virtual filesystem state, maps failures, and avoids host \`/bin/bash\` fallback. | \`cargo test -p open-agents-service\`; \`scripts/open-agents-local-e2e.sh --just-bash-conformance\`; \`cargo test -p open-agents-service -p open-agents-sandbox -p just-bash\`. |
| JBC-09 | AWK command parity | Close exact portable \`command:awk\` rows for print, fields, separators, BEGIN/END, patterns, stdin/files, and diagnostics. | Focused AWK tests; \`cargo test -p just-bash\`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-10 | Ripgrep command parity | Close exact portable \`command:rg\` rows for recursive virtual search, filters, flags, context, hidden/binary handling, and stdin where portable. | Focused rg tests; \`cargo test -p just-bash\`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-11 | Comparison corpus closure | Promote generated comparison corpus rows into Rust/JS closure where they pass without masking command-family gaps. | \`cargo test -p just-bash --test conformance_corpus\`; inventory/corpus checks; \`cargo test -p just-bash\`. |
| JBC-12 | Syntax and transform parity | Close exact portable parser/shell/AST transform rows for syntax, quoting, redirection, expansion, functions, and control flow. | Focused parser/shell tests; \`cargo test -p just-bash\`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-13 | Advanced filesystem parity | Close exact portable overlay/core/read-write/mountable filesystem rows, path behavior, symlinks, encoding, and error shapes. | Focused FS tests; \`cargo test -p just-bash\`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-14 | Security and sandbox parity | Close exact portable security/sandbox/fuzz/prototype-pollution rows or classify true JS-only worker/browser rows narrowly. | Focused security tests; \`cargo test -p just-bash\`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-15 | Interpreter core and expansion parity | Close exact portable interpreter builtins/core/expansion rows for dispatch, assignment, expansion, substitution, arithmetic, arrays, loops, and diagnostics. | Focused interpreter tests; \`cargo test -p just-bash\`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-16 | Structured and data command parity | Close exact portable \`jq\`, \`yq\`, \`xan\`, \`sqlite3\`, and adjacent data/query command rows that can run deterministically in-memory. | Focused structured command tests; \`cargo test -p just-bash\`; inventory/corpus checks; fmt/clippy/naming/diff gates. |

## Corpus Contract

- Every row has a stable \`id\`, \`kind\`, \`classification\`, command \`script\`,
  isolated \`cwd\`, \`env\`, \`stdin\`, \`args\`, \`options\`, \`initialFiles\`, and
  \`expected.stdout/stderr/exitCode\` fields.
- Exact golden expectations are stored as strings or numbers. A null
  expectation means the upstream row is source-traceable but does not expose
  that exact value.
- \`parity.ledgerKey\` points to the matching row in
  \`docs/open-agents/just-bash-parity.md\` using
  \`file:line:declaration:testName\`.
- \`sourceFixtureMetadata\` preserves comparison fixture IDs, locked fixture
  status, spec fixture source lines, and extraction details needed for drift
  review.
- The generator verifies the live upstream HEAD. Upstream drift fails \`--check\`
  until the corpus and metadata are refreshed.

## Coverage Summary

${renderTable(['Kind', 'Cases'], kindRows)}

## Domain Summary

${renderTable(['Domain', 'Cases'], domainRows)}

## Ledger Status Summary

${renderTable(['Status', 'Cases'], statusRows)}

## Representative Slice

The generated corpus includes the required representative domains: echo,
printf, pwd/cd/env, file operations, grep, sed, and awk. Comparison fixtures
provide exact Bash goldens for echo/grep/sed/awk and many file/text-processing
commands. Unit rows add command-level printf, pwd, env, cd, and file-operation
source traceability. Spec rows add imported grep/sed fixture cases with stdin
and expected output.

## Refresh Workflow

1. Run \`npx opensrc fetch https://github.com/vercel-labs/just-bash\`.
2. Verify \`git ls-remote https://github.com/vercel-labs/just-bash HEAD refs/heads/main\`.
3. Run \`node scripts/just-bash-conformance-corpus.mjs\`.
4. Run \`node scripts/just-bash-conformance-corpus.mjs --check\`.
5. Run \`node scripts/just-bash-test-inventory.mjs --check\` so the parity ledger and corpus stay aligned.

## Close Criteria

Just Bash is not parity-complete until all of these pass on current
\`origin/main\`:

\`\`\`sh
node scripts/just-bash-test-inventory.mjs --check
node scripts/just-bash-test-inventory.mjs --strict
node scripts/just-bash-conformance-corpus.mjs --check
cargo test -p just-bash
cargo test -p just-bash --test conformance_corpus
cargo test -p just-bash-napi
npm test --prefix crates/just-bash-napi
scripts/open-agents-local-e2e.sh --just-bash-conformance
scripts/master-parity-gate.sh --check
scripts/check-naming-conventions.sh
git diff --check
\`\`\`

Credential-gated or deployment-only checks may be documented as ignored live
proofs, but they do not replace the local strict inventory and conformance
gates.
`;
}

function asciiJson(value) {
  return `${JSON.stringify(value, null, 2).replace(/[^\x09\x0A\x0D\x20-\x7E]/g, (char) =>
    `\\u${char.charCodeAt(0).toString(16).padStart(4, '0')}`
  )}\n`;
}

function ensureParent(filePath) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const corpus = buildCorpus();
  const errors = validateCorpus(corpus);
  if (errors.length > 0) {
    fail(errors.join('\n'));
  }

  if (options.dryRun) {
    console.log(JSON.stringify(corpus.summary, null, 2));
    return;
  }

  const corpusJson = asciiJson(corpus);
  const docsMarkdown = renderDocs(corpus);

  if (options.check) {
    const mismatches = [];
    const currentCorpus = fs.existsSync(corpusPath)
      ? fs.readFileSync(corpusPath, 'utf8')
      : undefined;
    if (currentCorpus !== corpusJson) {
      mismatches.push('fixtures/just-bash-conformance/corpus.json is stale');
    }
    const currentDocs = fs.existsSync(docsPath)
      ? fs.readFileSync(docsPath, 'utf8')
      : undefined;
    if (currentDocs !== docsMarkdown) {
      mismatches.push('docs/open-agents/just-bash-conformance.md is stale');
    }
    if (mismatches.length > 0) {
      fail(mismatches.join('\n'));
    }
    console.log(
      `just-bash conformance corpus check passed (${corpus.summary.totalCases} cases, ${corpus.source.upstreamHead})`
    );
    return;
  }

  ensureParent(corpusPath);
  ensureParent(docsPath);
  fs.writeFileSync(corpusPath, corpusJson);
  fs.writeFileSync(docsPath, docsMarkdown);
  console.log(
    `wrote ${path.relative(repositoryRoot, corpusPath)} and ${path.relative(repositoryRoot, docsPath)} (${corpus.summary.totalCases} cases)`
  );
}

main();
