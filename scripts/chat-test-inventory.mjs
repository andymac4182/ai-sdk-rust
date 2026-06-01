#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..'
);
const defaultSourceRoot =
  '/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel/chat/main';
const sourceRoot = process.env.CHAT_UPSTREAM_PATH ?? defaultSourceRoot;
const outputPath = path.join(repositoryRoot, 'docs/chat/test-inventory.md');
const upstreamParityPath = path.join(
  repositoryRoot,
  'docs/chat/upstream-parity.md'
);
const unportedPath = path.join(repositoryRoot, 'docs/chat/unported.md');

const testFilePattern = /\.(?:test|spec)(?:-d)?\.(?:[cm]?[tj]sx?|mts|cts)$/;
const packageOwner = new Map([
  ['adapter-discord', 'chat-sdk-adapter-discord'],
  ['adapter-gchat', 'chat-sdk-adapter-gchat'],
  ['adapter-github', 'chat-sdk-adapter-github'],
  ['adapter-linear', 'chat-sdk-adapter-linear'],
  ['adapter-messenger', 'chat-sdk-adapter-messenger'],
  ['adapter-shared', 'chat-sdk-adapter-shared'],
  ['adapter-slack', 'chat-sdk-adapter-slack'],
  ['adapter-teams', 'chat-sdk-adapter-teams'],
  ['adapter-telegram', 'chat-sdk-adapter-telegram'],
  ['adapter-twilio', 'chat-sdk-adapter-twilio'],
  ['adapter-whatsapp', 'chat-sdk-adapter-whatsapp'],
  ['chat', 'chat-sdk-chat'],
  ['state-ioredis', 'chat-sdk-state-ioredis'],
  ['state-memory', 'chat-sdk-state-memory'],
  ['state-pg', 'chat-sdk-state-pg'],
  ['state-redis', 'chat-sdk-state-redis'],
]);

const jsOnlyPackages = new Map([
  [
    'adapter-web',
    'Browser/React adapter surface; Rust has no DOM event-loop target in this port.',
  ],
  [
    'integration-tests',
    'Upstream app/emulator integration harness; Rust crates own deterministic unit fixtures instead.',
  ],
  [
    'tests',
    'Vitest-only shared test-kit package built from vi.fn() factories and expect matchers.',
  ],
]);

const adapterHttpDescriptors = [
  'adapter.client',
  'attachment.fetchdata',
  'auth',
  'botToken as function',
  'callback',
  'client credentials',
  'createTeamsAdapter factory',
  'date parsing',
  'deleteMessage',
  'direct WebClient access',
  'downloadAttachment',
  'edge cases',
  'editMessage',
  'ensureSpaceSubscription',
  'error handling',
  'fetchChannelInfo',
  'fetchChannelMessages',
  'fetchMessage',
  'fetchMessages',
  'fetchSubject',
  'fetchThread',
  'formatted text extraction',
  'gateway',
  'getAuthOptions',
  'getUser',
  'handleCardClick',
  'handleEventMessage',
  'handleForwardedMessage',
  'handleForwardedReaction',
  'handleGoogleChatError',
  'handleMessageEvent',
  'handleWebhook',
  'initialize',
  'installation',
  'link extraction',
  'link unfurl',
  'listThreads',
  'message subtype handling',
  'multi-tenant',
  'multi-workspace',
  'oauth',
  'octokit',
  'openDM',
  'openModal',
  'parseMessage',
  'parsePubSubMessage',
  'postChannelMessage',
  'postMessage',
  'publishHomeView',
  'reverse user lookup',
  'resolveInlineMentions',
  'routeSocketEvent',
  'scheduleMessage',
  'self-message detection',
  'setAssistantStatus',
  'setAssistantTitle',
  'setSuggestedPrompts',
  'socket mode',
  'startSocketModeListener',
  'startTyping',
  'stream',
  'token',
  'updateModal',
  'user info caching',
  'webClient getter',
  'webhook verification',
  'withBotToken',
];

const typeSystemFragments = [
  'subclass extensibility',
  'protected members',
  'protected member',
  'type inference',
  'type-level',
  'compile-time',
  'deprecated client alias',
  'instance identity',
  'linearclient getter',
  'octokit getter',
  'per-org linearclient',
  'deprecated alias',
  'null and undefined',
  'underlying linearclient',
  'undefined value',
  'webclient getter',
];

const jsOnlyFragments = [
  '@slack/socket-mode',
  'arraybuffer',
  'blob',
  'browser',
  'constructor env var resolution',
  'custom auth',
  'default logger',
  'esm compatibility',
  'eventemitter',
  'expect matcher',
  'export create',
  'export function',
  'exported function',
  'factory default-logger',
  'fetch mock',
  'integration tests',
  'js symbol',
  'jsx',
  'module-loader',
  'process.env',
  'react',
  'remend',
  'request wrapper',
  'runtime wrapper',
  'vi.fn',
  'vi.mock',
  'vitest',
];

function usage() {
  console.log(`Usage: node scripts/chat-test-inventory.mjs [options]

Options:
  --check       Regenerate the Chat inventory and fail when docs differ.
  --write       Regenerate docs/chat/test-inventory.md.
  --summary     Print summary counts only.
  --help        Show this help text.

Environment:
  CHAT_UPSTREAM_PATH  Path to a fetched vercel/chat checkout. Defaults to
                      ${defaultSourceRoot}`);
}

function parseArgs(argv) {
  const options = {
    check: false,
    write: false,
    summary: false,
  };

  for (const arg of argv) {
    if (arg === '--help' || arg === '-h') {
      usage();
      process.exit(0);
    }
    if (arg === '--check') {
      options.check = true;
      continue;
    }
    if (arg === '--write') {
      options.write = true;
      continue;
    }
    if (arg === '--summary') {
      options.summary = true;
      continue;
    }
    throw new Error(`unknown option: ${arg}`);
  }

  return options;
}

function walkFiles(root) {
  const files = [];
  const stack = [root];
  while (stack.length > 0) {
    const current = stack.pop();
    if (!current || !fs.existsSync(current)) {
      continue;
    }
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const next = path.join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(next);
      } else if (entry.isFile()) {
        files.push(next);
      }
    }
  }
  files.sort();
  return files;
}

function unescapeJsString(value) {
  return value
    .replace(/\\`/g, '`')
    .replace(/\\"/g, '"')
    .replace(/\\'/g, "'")
    .replace(/\\n/g, '\n')
    .replace(/\\t/g, '\t');
}

function firstStringLiteral(text) {
  const match = text.match(/(['"`])((?:\\.|(?!\1).)*)\1/s);
  return match ? unescapeJsString(match[2]) : undefined;
}

function extractCasesFromFile(filePath) {
  const relative = path.relative(sourceRoot, filePath);
  const packageName = relative.split(path.sep)[1] ?? '';
  const lines = fs.readFileSync(filePath, 'utf8').split(/\r?\n/);
  const stack = [];
  const cases = [];

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const indent = line.match(/^\s*/)?.[0].length ?? 0;
    const trimmed = line.trim();

    while (
      stack.length > 0 &&
      indent <= stack[stack.length - 1].indent &&
      /^\}\);/.test(trimmed)
    ) {
      stack.pop();
    }

    const describeMatch = trimmed.match(
      /^describe(?:\.(skip|only))?\(\s*(['"`])((?:\\.|(?!\2).)*)\2/s
    );
    if (describeMatch) {
      stack.push({
        name: unescapeJsString(describeMatch[3]),
        indent,
        skip: describeMatch[1] === 'skip',
      });
      continue;
    }

    const directCaseMatch = trimmed.match(
      /^(?:it|test)(?:\.(skip|todo|only))?\(\s*(['"`])((?:\\.|(?!\2).)*)\2/s
    );
    if (directCaseMatch) {
      cases.push({
        packageName,
        file: relative.split(path.sep).join('/'),
        line: index + 1,
        name: unescapeJsString(directCaseMatch[3]),
        describe: stack.map((entry) => entry.name),
        skipped:
          directCaseMatch[1] === 'skip' ||
          directCaseMatch[1] === 'todo' ||
          stack.some((entry) => entry.skip),
      });
      continue;
    }

    if (/^(?:it|test)\(\s*$/.test(trimmed)) {
      const lookahead = lines.slice(index, index + 8).join('\n');
      const name = firstStringLiteral(lookahead);
      if (name) {
        cases.push({
          packageName,
          file: relative.split(path.sep).join('/'),
          line: index + 1,
          name,
          describe: stack.map((entry) => entry.name),
          skipped: stack.some((entry) => entry.skip),
        });
      }
      continue;
    }

    if (/^(?:it|test)\.each\(/.test(trimmed)) {
      const lookahead = lines.slice(index, index + 160).join('\n');
      const match = lookahead.match(/\)\s*\(\s*(['"`])((?:\\.|(?!\1).)*)\1/s);
      if (match) {
        cases.push({
          packageName,
          file: relative.split(path.sep).join('/'),
          line: index + 1,
          name: unescapeJsString(match[2]),
          describe: stack.map((entry) => entry.name),
          skipped: stack.some((entry) => entry.skip),
        });
      }
    }
  }

  return cases;
}

function extractUpstreamCases() {
  const packagesRoot = path.join(sourceRoot, 'packages');
  if (!fs.existsSync(packagesRoot)) {
    throw new Error(
      `missing fetched vercel/chat packages directory: ${packagesRoot}`
    );
  }

  return walkFiles(packagesRoot)
    .filter((file) => testFilePattern.test(file))
    .flatMap(extractCasesFromFile)
    .sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line);
}

function rustModuleFromPath(filePath) {
  const relative = path.relative(repositoryRoot, filePath).split(path.sep);
  const srcIndex = relative.indexOf('src');
  const afterSrc = relative.slice(srcIndex + 1);
  const basename = afterSrc.pop()?.replace(/\.rs$/, '') ?? '';
  const modules = [...afterSrc, basename].filter(
    (part) => part !== 'lib' && part !== 'mod'
  );
  return modules;
}

function extractRustTests() {
  const cratesRoot = path.join(repositoryRoot, 'crates');
  const testsByCrate = new Map();
  const sourceByName = new Map();

  for (const crateName of fs
    .readdirSync(cratesRoot)
    .filter((name) => name.startsWith('chat-sdk-'))
    .sort()) {
    const srcRoot = path.join(cratesRoot, crateName, 'src');
    const tests = [];
    for (const filePath of walkFiles(srcRoot).filter((file) =>
      file.endsWith('.rs')
    )) {
      const lines = fs.readFileSync(filePath, 'utf8').split(/\r?\n/);
      for (let index = 0; index < lines.length; index += 1) {
        if (!/^\s*#\[(?:tokio::)?test(?:\([^)]*\))?\]/.test(lines[index])) {
          continue;
        }
        for (
          let cursor = index + 1;
          cursor < Math.min(index + 8, lines.length);
          cursor += 1
        ) {
          const match = lines[cursor].match(
            /^\s*(?:async\s+)?fn\s+([a-zA-Z0-9_]+)\s*\(/
          );
          if (!match) {
            continue;
          }
          const modules = rustModuleFromPath(filePath);
          const testPath = [
            crateName,
            ...modules,
            'tests',
            match[1],
          ].join('::');
          const record = {
            crateName,
            file: path.relative(repositoryRoot, filePath),
            line: cursor + 1,
            module: modules.join('::'),
            name: match[1],
            normalized: normalize(match[1]),
            tokens: tokens(match[1]),
            testPath,
          };
          tests.push(record);
          sourceByName.set(testPath, record);
          break;
        }
      }
    }
    testsByCrate.set(crateName, tests);
  }

  return { testsByCrate, sourceByName };
}

function normalize(value) {
  return value
    .toLowerCase()
    .replace(/\$\w+/g, '')
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .replace(/\bshould_/g, '')
    .replace(/_should_/g, '_')
    .replace(/_/g, '');
}

const stopWords = new Set([
  'a',
  'an',
  'and',
  'are',
  'as',
  'at',
  'be',
  'by',
  'for',
  'from',
  'in',
  'into',
  'is',
  'it',
  'of',
  'on',
  'or',
  'the',
  'to',
  'via',
  'when',
  'with',
  'without',
  'should',
]);

function tokens(value) {
  return new Set(
    String(value)
      .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
      .toLowerCase()
      .replace(/\$\w+/g, '')
      .split(/[^a-z0-9]+/)
      .map((token) =>
        token.length > 3 && token.endsWith('s') ? token.slice(0, -1) : token
      )
      .map((token) => token.trim())
      .filter((token) => token.length >= 2 && !stopWords.has(token))
  );
}

function moduleHints(upstreamFile) {
  const basename = path.basename(upstreamFile).replace(/\.(?:test|spec).*$/, '');
  const parent = path.basename(path.dirname(upstreamFile));
  const hints = new Set();
  const push = (value) => {
    if (value && value !== 'src') {
      hints.add(value.replace(/-/g, '_'));
    }
  };

  push(basename);
  push(parent);

  if (basename === 'thread-utils') {
    hints.add('thread_id');
  }
  if (basename === 'index' && parent !== 'src') {
    push(parent);
  }
  if (basename === 'index' && parent === 'ai') {
    hints.add('ai');
  }
  if (parent === 'api') {
    hints.add('api');
  }
  if (parent === 'format') {
    hints.add('format');
  }
  if (parent === 'webhook') {
    hints.add('webhook');
  }
  if (parent === 'voice') {
    hints.add('voice');
  }
  if (parent === 'blocks') {
    hints.add('blocks');
  }

  return hints;
}

function findRustTest(upstreamCase, testsByCrate) {
  const crateName = packageOwner.get(upstreamCase.packageName);
  if (!crateName) {
    return undefined;
  }

  const tests = testsByCrate.get(crateName) ?? [];
  const hints = moduleHints(upstreamCase.file);
  const lowerFullPath = lowerCasePath(upstreamCase);
  if (lowerFullPath.includes('formatconverter')) {
    hints.add('markdown');
  }
  if (lowerFullPath.includes('threadid') || lowerFullPath.includes('thread id')) {
    hints.add('thread_id');
  }
  const words = [...upstreamCase.describe, upstreamCase.name]
    .map(normalize)
    .filter((word) => word.length >= 7);
  const full = normalize([...upstreamCase.describe, upstreamCase.name].join(' '));
  const caseTokens = tokens([...upstreamCase.describe, upstreamCase.name].join(' '));
  const requiresAiModule = upstreamCase.file.includes('/src/ai/');

  const score = (test) => {
    if (requiresAiModule && !test.module.includes('ai')) {
      return 0;
    }
    if (full.includes('getparticipants') && !test.name.includes('get_participants')) {
      return 0;
    }
    let result = 0;
    if (hints.size > 0 && hints.has(test.module.replace(/::/g, '_'))) {
      result += 20;
    }
    for (const hint of hints) {
      if (test.module.includes(hint) || test.name.includes(hint)) {
        result += 6;
      }
    }
    for (const word of words) {
      if (test.normalized.includes(word)) {
        result += Math.min(word.length, 24);
      }
      if (word.includes(test.normalized) && test.normalized.length >= 10) {
        result += Math.min(test.normalized.length, 18);
      }
    }
    const sharedTokens = [...caseTokens].filter((token) => test.tokens.has(token));
    if (sharedTokens.length >= 2) {
      result += sharedTokens.length * 6;
      result += sharedTokens.filter((token) => token.length >= 5).length * 3;
    }
    if (
      full.length >= 12 &&
      (test.normalized.includes(full) || full.includes(test.normalized))
    ) {
      result += 40;
    }
    return result;
  };

  const candidates = tests
    .map((test) => ({ test, score: score(test) }))
    .filter((entry) => entry.score >= 12)
    .sort((a, b) => b.score - a.score || a.test.testPath.localeCompare(b.test.testPath));

  return candidates[0]?.test;
}

function lowerCasePath(upstreamCase) {
  return [...upstreamCase.describe, upstreamCase.name].join(' > ').toLowerCase();
}

function exceptionForCase(upstreamCase) {
  const packageReason = jsOnlyPackages.get(upstreamCase.packageName);
  if (packageReason) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note: packageReason,
    };
  }

  const lowerFile = upstreamCase.file.toLowerCase();
  const lowerPath = lowerCasePath(upstreamCase);

  if (
    lowerFile.endsWith('.tsx') ||
    lowerFile.includes('/jsx-') ||
    lowerPath.includes('jsx')
  ) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note: 'JSX/React runtime case; Rust builders construct the same data shape directly.',
    };
  }

  if (upstreamCase.skipped) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note: 'Upstream marks this case skipped and it requires an external service or live runtime.',
    };
  }

  if (typeSystemFragments.some((fragment) => lowerPath.includes(fragment))) {
    return {
      portability: 'type-system-impossible',
      status: 'type-system-impossible',
      note: 'TypeScript runtime/type-system assertion is unrepresentable in Rust.',
    };
  }

  if (jsOnlyFragments.some((fragment) => lowerPath.includes(fragment))) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note: 'JavaScript runtime/framework harness case documented in docs/chat/unported.md.',
    };
  }

  if (
    upstreamCase.packageName.startsWith('adapter-') &&
    adapterHttpDescriptors.some((fragment) => lowerPath.includes(fragment.toLowerCase()))
  ) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note: 'Adapter runtime case depends on upstream vi.fn()/typed-client HTTP harness; pure shape helpers are mapped separately.',
    };
  }

  if (
    ['state-redis', 'state-ioredis', 'state-pg'].includes(
      upstreamCase.packageName
    ) &&
    /client|connect|integration|mock|redis|postgres|url|env var|wait|underlying/.test(
      lowerPath
    )
  ) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      note: 'Production state backend client/runtime case documented in docs/chat/unported.md.',
    };
  }

  return undefined;
}

function classifyCases(cases, testsByCrate) {
  return cases.map((upstreamCase) => {
    const owner = packageOwner.get(upstreamCase.packageName) ?? 'none';
    const exception = exceptionForCase(upstreamCase);
    if (exception) {
      return {
        ...upstreamCase,
        owner,
        rustTest: 'none',
        ...exception,
      };
    }

    const rustTest = findRustTest(upstreamCase, testsByCrate);
    if (rustTest) {
      return {
        ...upstreamCase,
        owner,
        portability: 'portable',
        status: 'verified',
        rustTest: rustTest.testPath,
        note: `Mapped to ${rustTest.file}:${rustTest.line}.`,
      };
    }

    return {
      ...upstreamCase,
      owner,
      portability: 'portable',
      status: 'needs-review',
      rustTest: 'none',
      note: 'No named Rust test or explicit nonportable exception matched.',
    };
  });
}

function markdownEscape(value) {
  return String(value)
    .replace(/\\/g, '\\\\')
    .replace(/\|/g, '\\|')
    .replace(/\r?\n/g, '<br>');
}

function inlineCode(value) {
  if (!value || value === 'none') {
    return 'none';
  }
  return `\`${String(value).replace(/`/g, '\\`')}\``;
}

function caseName(row) {
  return [...row.describe, row.name].join(' > ');
}

function summarize(rows) {
  const packages = new Map();
  const totals = {
    testFiles: new Set(),
    cases: rows.length,
    portable: 0,
    verified: 0,
    jsOnly: 0,
    typeSystem: 0,
    needsReview: 0,
  };

  for (const row of rows) {
    totals.testFiles.add(row.file);
    if (!packages.has(row.packageName)) {
      packages.set(row.packageName, {
        packageName: row.packageName,
        owner: row.owner,
        files: new Set(),
        cases: 0,
        portable: 0,
        verified: 0,
        jsOnly: 0,
        typeSystem: 0,
        needsReview: 0,
      });
    }
    const summary = packages.get(row.packageName);
    summary.files.add(row.file);
    summary.cases += 1;
    if (row.portability === 'portable') {
      summary.portable += 1;
      totals.portable += 1;
    }
    if (row.status === 'verified') {
      summary.verified += 1;
      totals.verified += 1;
    } else if (row.status === 'js-only-documented') {
      summary.jsOnly += 1;
      totals.jsOnly += 1;
    } else if (row.status === 'type-system-impossible') {
      summary.typeSystem += 1;
      totals.typeSystem += 1;
    } else if (row.status === 'needs-review') {
      summary.needsReview += 1;
      totals.needsReview += 1;
    }
  }

  return {
    totals: {
      ...totals,
      testFiles: totals.testFiles.size,
    },
    packages: [...packages.values()].sort((a, b) =>
      a.packageName.localeCompare(b.packageName)
    ),
  };
}

function renderInventory(rows) {
  const { totals, packages } = summarize(rows);
  const lines = [];
  lines.push('# Chat SDK Strict Upstream Test Inventory');
  lines.push('');
  lines.push(
    '_Generated by `node scripts/chat-test-inventory.mjs --write` from the fetched `vercel/chat` source tree._'
  );
  lines.push('');
  lines.push('## Source');
  lines.push('');
  lines.push(`- Upstream path: \`${sourceRoot}\``);
  lines.push(`- Upstream test files: ${totals.testFiles}`);
  lines.push(`- Upstream test cases: ${totals.cases}`);
  lines.push(`- Portable verified cases: ${totals.verified} / ${totals.portable}`);
  lines.push(`- JavaScript-only documented cases: ${totals.jsOnly}`);
  lines.push(`- Type-system-impossible cases: ${totals.typeSystem}`);
  lines.push(`- Needs-review cases: ${totals.needsReview}`);
  lines.push('');
  lines.push('## Summary');
  lines.push('');
  lines.push(
    '| Package | Test files | Cases | Portable verified | JS only | Type system | Needs review | Rust owner |'
  );
  lines.push('| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |');
  for (const row of packages) {
    lines.push(
      `| \`${row.packageName}\` | ${row.files.size} | ${row.cases} | ${row.verified} / ${row.portable} | ${row.jsOnly} | ${row.typeSystem} | ${row.needsReview} | ${inlineCode(row.owner)} |`
    );
  }
  lines.push('');
  lines.push('## Case Inventory');
  lines.push('');
  lines.push(
    '| Package | Upstream file | Line | Case | Portability | Rust owner | Rust test | Status | Notes |'
  );
  lines.push('| --- | --- | ---: | --- | --- | --- | --- | --- | --- |');
  for (const row of rows) {
    lines.push(
      `| \`${row.packageName}\` | \`${row.file}\` | ${row.line} | ${markdownEscape(
        caseName(row)
      )} | \`${row.portability}\` | ${inlineCode(row.owner)} | ${inlineCode(
        row.rustTest
      )} | \`${row.status}\` | ${markdownEscape(row.note)} |`
    );
  }
  lines.push('');
  return `${lines.join('\n')}\n`;
}

function printSummary(rows) {
  const { totals } = summarize(rows);
  console.log(
    JSON.stringify(
      {
        upstreamPath: sourceRoot,
        testFiles: totals.testFiles,
        cases: totals.cases,
        portable: totals.portable,
        verified: totals.verified,
        jsOnly: totals.jsOnly,
        typeSystem: totals.typeSystem,
        needsReview: totals.needsReview,
      },
      null,
      2
    )
  );
}

function assertLedgerFilesExist() {
  for (const filePath of [upstreamParityPath, unportedPath]) {
    if (!fs.existsSync(filePath)) {
      throw new Error(`missing Chat parity document: ${path.relative(repositoryRoot, filePath)}`);
    }
  }
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  assertLedgerFilesExist();
  const { testsByCrate } = extractRustTests();
  const rows = classifyCases(extractUpstreamCases(), testsByCrate);
  const rendered = renderInventory(rows);
  const { totals } = summarize(rows);

  if (options.summary) {
    printSummary(rows);
  }

  if (options.write) {
    fs.writeFileSync(outputPath, rendered);
  }

  if (options.check) {
    if (!fs.existsSync(outputPath)) {
      throw new Error(
        `missing generated inventory: ${path.relative(repositoryRoot, outputPath)}`
      );
    }
    const current = fs.readFileSync(outputPath, 'utf8');
    if (current !== rendered) {
      throw new Error(
        `${path.relative(repositoryRoot, outputPath)} is stale; run node scripts/chat-test-inventory.mjs --write`
      );
    }
    if (totals.needsReview !== 0) {
      throw new Error(
        `Chat strict inventory has ${totals.needsReview} needs-review cases`
      );
    }
    if (totals.verified !== totals.portable) {
      throw new Error(
        `Chat strict inventory verified ${totals.verified} of ${totals.portable} portable cases`
      );
    }
  }

  if (!options.summary && !options.write && !options.check) {
    console.log(rendered);
  }
}

main();
