#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..'
);

const defaultUpstreamRoot =
  '/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/open-agents/main';
const upstreamRoot =
  process.env.OPEN_AGENTS_UPSTREAM_PATH ?? defaultUpstreamRoot;
const outputPath = path.join(
  repositoryRoot,
  'docs/open-agents/upstream-parity.md'
);

const upstreamRepo = 'vercel-labs/open-agents';
const upstreamHead = '24d679c7ba3d274aa73814c15673aeffcbe3c1c2';
const inventoryDate = '2026-06-02';

const expectedPackageManifestCount = 6;
const expectedSourceFileCount = 461;
const expectedTestFileCount = 106;
const expectedTsxFileCount = expectedSourceFileCount + expectedTestFileCount;

const codeFilePattern = /\.(?:d\.)?(?:ts|tsx)$/;
const testFilePattern = /\.(?:test|spec)\.(?:ts|tsx)$/;
const testCallPattern =
  /(?:^|[^\w$])(?:it|test)(?:\.(?:skip|only|todo|each))?\s*\(/g;
const targetTestCalls = new Set(['it', 'test']);

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
  console.log(`Usage: node scripts/open-agents-test-inventory.mjs [options]

Options:
  --check     Verify docs/open-agents/upstream-parity.md is up to date.
  --dry-run   Print current inventory counts as JSON.
  --help      Show this help text.

Environment:
  OPEN_AGENTS_UPSTREAM_PATH  Override the OpenSrc mirror path.`);
}

function fail(message) {
  console.error(`open-agents inventory: ${message}`);
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
    if (
      entry.name === '.git' ||
      entry.name === '.next' ||
      entry.name === '.turbo' ||
      entry.name === 'coverage' ||
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
  const roots = ['crates', 'src']
    .map((entry) => path.join(repositoryRoot, entry))
    .filter((entry) => fs.existsSync(entry));

  for (const root of roots) {
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

function splitRustTestNames(value) {
  return String(value)
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

function relativePath(filePath) {
  return path.relative(upstreamRoot, filePath).replaceAll(path.sep, '/');
}

function escapeCell(value) {
  return String(value)
    .replaceAll('\\', '\\\\')
    .replaceAll('|', '\\|')
    .replaceAll('\n', '<br>');
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

function packageIdFor(relative) {
  if (relative === 'package.json') {
    return 'root';
  }
  if (relative.startsWith('apps/web/')) {
    return 'apps/web';
  }
  if (relative.startsWith('packages/')) {
    return relative.split('/').slice(0, 2).join('/');
  }
  if (relative.startsWith('scripts/')) {
    return 'scripts';
  }
  if (relative.startsWith('.agents/')) {
    return '.agents';
  }
  return relative.split('/')[0] ?? 'root';
}

function packageName(manifestRelativePath) {
  const manifest = JSON.parse(
    fs.readFileSync(path.join(upstreamRoot, manifestRelativePath), 'utf8')
  );
  return manifest.name ?? '(unnamed)';
}

function countTestCalls(relative) {
  const source = fs.readFileSync(path.join(upstreamRoot, relative), 'utf8');
  return [...source.matchAll(testCallPattern)].length;
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

function isIdentifierCharacter(char) {
  return /[\w$]/.test(char ?? '');
}

function skipWhitespace(source, index) {
  let cursor = index;
  while (/\s/.test(source[cursor] ?? '')) {
    cursor += 1;
  }
  return cursor;
}

function readStringLiteral(source, index) {
  const quote = source[index];
  if (quote !== '"' && quote !== "'" && quote !== '`') {
    return undefined;
  }

  let cursor = index + 1;
  let value = '';
  while (cursor < source.length) {
    const char = source[cursor];
    if (char === '\\') {
      const next = source[cursor + 1];
      value += next ?? '';
      cursor += 2;
      continue;
    }
    if (char === quote) {
      return { value, end: cursor + 1 };
    }
    value += char;
    cursor += 1;
  }
  return undefined;
}

function skipString(source, index) {
  return readStringLiteral(source, index)?.end ?? index + 1;
}

function skipLineComment(source, index) {
  const next = source.indexOf('\n', index + 2);
  return next === -1 ? source.length : next + 1;
}

function skipBlockComment(source, index) {
  const next = source.indexOf('*/', index + 2);
  return next === -1 ? source.length : next + 2;
}

function nextCodeCharacter(source, index, target) {
  let cursor = index;
  while (cursor < source.length) {
    const char = source[cursor];
    const next = source[cursor + 1];
    if (char === '"' || char === "'" || char === '`') {
      cursor = skipString(source, cursor);
      continue;
    }
    if (char === '/' && next === '/') {
      cursor = skipLineComment(source, cursor);
      continue;
    }
    if (char === '/' && next === '*') {
      cursor = skipBlockComment(source, cursor);
      continue;
    }
    if (char === target) {
      return cursor;
    }
    cursor += 1;
  }
  return undefined;
}

function matchingBrace(source, openIndex, openChar, closeChar) {
  let depth = 0;
  let cursor = openIndex;
  while (cursor < source.length) {
    const char = source[cursor];
    const next = source[cursor + 1];
    if (char === '"' || char === "'" || char === '`') {
      cursor = skipString(source, cursor);
      continue;
    }
    if (char === '/' && next === '/') {
      cursor = skipLineComment(source, cursor);
      continue;
    }
    if (char === '/' && next === '*') {
      cursor = skipBlockComment(source, cursor);
      continue;
    }
    if (char === openChar) {
      depth += 1;
    } else if (char === closeChar) {
      depth -= 1;
      if (depth === 0) {
        return cursor;
      }
    }
    cursor += 1;
  }
  return undefined;
}

function extractCallTitle(source, openParenIndex) {
  const titleStart = skipWhitespace(source, openParenIndex + 1);
  return readStringLiteral(source, titleStart);
}

function extractTestCalls(relative) {
  const source = fs.readFileSync(path.join(upstreamRoot, relative), 'utf8');
  const starts = lineStarts(source);
  const calls = [];

  let cursor = 0;
  while (cursor < source.length) {
    const char = source[cursor];
    const next = source[cursor + 1];
    if (char === '"' || char === "'" || char === '`') {
      cursor = skipString(source, cursor);
      continue;
    }
    if (char === '/' && next === '/') {
      cursor = skipLineComment(source, cursor);
      continue;
    }
    if (char === '/' && next === '*') {
      cursor = skipBlockComment(source, cursor);
      continue;
    }

    const match = /^(describe|it|test)\b/.exec(source.slice(cursor));
    if (!match || isIdentifierCharacter(source[cursor - 1])) {
      cursor += 1;
      continue;
    }

    const name = match[1];
    const afterName = skipWhitespace(source, cursor + name.length);
    if (source[afterName] !== '(') {
      cursor += name.length;
      continue;
    }

    const title = extractCallTitle(source, afterName);
    const line = lineNumberAt(starts, cursor);
    const call = {
      kind: name,
      index: cursor,
      line,
      title: title?.value ?? '(dynamic title)',
      declaration: name,
      bodyStart: undefined,
      bodyEnd: undefined,
    };

    if (name === 'describe') {
      const openBrace = title ? nextCodeCharacter(source, title.end, '{') : undefined;
      if (openBrace !== undefined) {
        call.bodyStart = openBrace;
        call.bodyEnd = matchingBrace(source, openBrace, '{', '}');
      }
    }

    calls.push(call);
    cursor += name.length;
  }

  const suites = calls
    .filter((call) => call.kind === 'describe' && call.bodyStart !== undefined)
    .sort((left, right) => left.bodyStart - right.bodyStart);

  return calls
    .filter((call) => targetTestCalls.has(call.kind))
    .map((call) => {
      const suitePath = suites
        .filter(
          (suite) =>
            suite.bodyStart < call.index &&
            suite.bodyEnd !== undefined &&
            call.index < suite.bodyEnd
        )
        .map((suite) => suite.title)
        .join(' > ');
      return {
        packageId: packageIdFor(relative),
        file: relative,
        line: call.line,
        suitePath: suitePath || '(root)',
        caseName: call.title,
        declaration: call.declaration,
      };
    });
}

function sourceMapping(relative) {
  if (relative.startsWith('packages/agent/tools/')) {
    return portable(
      'ai-sdk-rust::open_agents_tools',
      'Agent tool definitions, approvals, path security, and sandbox-bound execution.'
    );
  }
  if (relative.startsWith('packages/agent/skills/')) {
    return portable(
      'ai-sdk-rust::skills',
      'Skill discovery, frontmatter, slash-command invocation, and loaded-skill prompts.'
    );
  }
  if (
    relative.startsWith('packages/agent/subagents/') ||
    relative.startsWith('packages/agent/context-management/')
  ) {
    return portable(
      'ai-sdk-rust::subagents',
      'Subagent profiles, inherited context, usage folding, and cache-control equivalents.'
    );
  }
  if (relative.startsWith('packages/agent/')) {
    return portable(
      'open-agents-runtime',
      'Open Agent model selection, system prompt, runtime adapter, and usage hooks.'
    );
  }
  if (relative.startsWith('packages/sandbox/')) {
    return portable(
      'open-agents-sandbox',
      'Typed sandbox boundary, Vercel backend, file/exec/snapshot/state, and git helpers.'
    );
  }
  if (relative === 'packages/shared/lib/tool-state.ts') {
    return portable(
      'open-agents-slack',
      'Tool state labels and approval status map to Slack outbound status rendering.'
    );
  }
  if (relative === 'packages/shared/lib/diff.ts') {
    return portable(
      'open-agents-sandbox',
      'Diff formatting and file-change summaries map to sandbox git automation.'
    );
  }
  if (relative.startsWith('packages/shared/')) {
    return jsOnly('Shared React/web helper; Slack release does not expose this browser UI surface.');
  }
  if (relative.startsWith('apps/web/app/workflows/')) {
    return portable(
      'open-agents-service',
      'Durable workflow orchestration maps to the Slack service local runtime and workflow bridge.'
    );
  }
  if (relative.startsWith('apps/web/app/api/chat/')) {
    return portable(
      'open-agents-service',
      'Chat start, stop, streaming, request parsing, model selection, and tool-result persistence.'
    );
  }
  if (relative.startsWith('apps/web/app/api/sandbox/')) {
    return portable(
      'open-agents-service/open-agents-sandbox',
      'Sandbox provision, reconnect, snapshot, status, and lifecycle API semantics.'
    );
  }
  if (relative.startsWith('apps/web/app/api/sessions/')) {
    return portable(
      'open-agents-service/open-agents-persistence',
      'Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs.'
    );
  }
  if (
    relative.startsWith('apps/web/lib/db/') ||
    relative.startsWith('apps/web/drizzle.config.ts')
  ) {
    return portable(
      'open-agents-persistence',
      'Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts.'
    );
  }
  if (relative.startsWith('apps/web/lib/sandbox/')) {
    return portable(
      'open-agents-service/open-agents-sandbox',
      'Sandbox lifecycle, provisioning kick, archive, and configuration behavior.'
    );
  }
  if (
    relative.startsWith('apps/web/lib/assistant-file-links') ||
    relative.startsWith('apps/web/lib/chat/') ||
    relative.startsWith('apps/web/lib/chat-') ||
    relative.startsWith('apps/web/lib/workspace-status-store') ||
    relative.startsWith('apps/web/lib/usage/')
  ) {
    return portable(
      'open-agents-runtime/open-agents-service',
      'Runtime cancellation, stream state, finish actions, usage, and workspace status behavior.'
    );
  }
  if (
    relative.startsWith('apps/web/lib/github/') ||
    relative.startsWith('apps/web/app/api/generate-pr/')
  ) {
    return portable(
      'open-agents-sandbox',
      'GitHub commit, PR, repository, token, readiness, and deployment-polling automation.'
    );
  }
  if (
    relative.startsWith('apps/web/lib/model') ||
    relative.startsWith('apps/web/app/api/models/') ||
    relative.startsWith('apps/web/app/api/generate-title/') ||
    relative.startsWith('apps/web/app/api/settings/model-variants/')
  ) {
    return portable(
      'open-agents-runtime',
      'Model catalog, model variants, provider options, and access policy selection.'
    );
  }
  if (
    relative.startsWith('apps/web/lib/skills') ||
    relative.startsWith('apps/web/app/api/sessions/[sessionId]/skills/')
  ) {
    return portable(
      'ai-sdk-rust::skills',
      'Global/project skill discovery, cache, refs, and installation semantics.'
    );
  }
  if (
    relative.startsWith('apps/web/app/api/vercel/') ||
    relative.startsWith('apps/web/lib/vercel/')
  ) {
    return portable(
      'open-agents-service',
      'Deployment-time Vercel project/env lookup and operator configuration.'
    );
  }
  if (relative.startsWith('scripts/vercel-refresh-base-snapshot.ts')) {
    return portable(
      'open-agents-sandbox',
      'Base snapshot refresh maps to sandbox snapshot setup and ignored live proof.'
    );
  }
  if (relative.startsWith('scripts/')) {
    return jsOnly('Bun/Next test harness helper with no Rust runtime surface.');
  }
  if (
    relative.startsWith('apps/web/components/') ||
    relative.startsWith('apps/web/hooks/') ||
    relative.startsWith('apps/web/app/sessions/') ||
    relative.startsWith('apps/web/app/settings/') ||
    relative.startsWith('apps/web/app/shared/') ||
    relative.startsWith('apps/web/app/get-started/') ||
    relative.startsWith('apps/web/app/codespace/') ||
    relative.startsWith('apps/web/app/[username]/') ||
    relative.startsWith('apps/web/app/deploy-your-own/') ||
    relative.startsWith('apps/web/app/home-page') ||
    relative.startsWith('apps/web/app/page') ||
    relative.startsWith('apps/web/app/layout') ||
    relative.startsWith('apps/web/app/providers') ||
    relative.startsWith('apps/web/app/opengraph-image') ||
    relative.startsWith('apps/web/app/favicon') ||
    relative.startsWith('apps/web/app/globals') ||
    relative.startsWith('apps/web/app/lib/render-tool') ||
    relative.startsWith('apps/web/instrumentation-client') ||
    relative.startsWith('apps/web/proxy') ||
    relative.startsWith('apps/web/next.config') ||
    relative.startsWith('apps/web/shiki-custom-themes')
  ) {
    return jsOnly('Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port.');
  }
  if (
    relative.startsWith('apps/web/app/api/auth/') ||
    relative.startsWith('apps/web/app/api/github/') ||
    (relative.startsWith('apps/web/app/api/settings/') &&
      !relative.startsWith('apps/web/app/api/settings/model-variants/')) ||
    relative.startsWith('apps/web/app/api/shared/') ||
    relative.startsWith('apps/web/app/api/transcribe/') ||
    relative.startsWith('apps/web/app/api/usage/') ||
    relative.startsWith('apps/web/lib/auth/') ||
    relative.startsWith('apps/web/lib/admin/') ||
    relative.startsWith('apps/web/lib/botid') ||
    relative.startsWith('apps/web/lib/onboarding') ||
    relative.startsWith('apps/web/lib/public') ||
    relative.startsWith('apps/web/lib/random-city') ||
    relative.startsWith('apps/web/lib/redirect-safety') ||
    relative.startsWith('apps/web/lib/rate-limit') ||
    relative.startsWith('apps/web/lib/swr') ||
    relative.startsWith('apps/web/lib/text-attachment-utils') ||
    relative.startsWith('apps/web/lib/utils') ||
    relative.startsWith('apps/web/lib/vercel-themes') ||
    relative.startsWith('apps/web/lib/image-utils') ||
    relative.startsWith('apps/web/lib/file-suggestions') ||
    relative.startsWith('apps/web/lib/diffs-config') ||
    relative.startsWith('apps/web/lib/git/') ||
    relative.startsWith('apps/web/lib/deployment/')
  ) {
    return jsOnly('Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port.');
  }
  if (relative.startsWith('apps/web/')) {
    return jsOnly('Open Agents web application surface; no direct Rust Slack release counterpart.');
  }
  return unmapped('No source owner rule matched this file.');
}

function testMapping(relative) {
  const testName = partialRustTestName(relative);
  if (testName) {
    const owner = testOwner(relative);
    return {
      portability: 'portable',
      status: relative.includes('/sandbox/') ? 'in-progress' : 'in-progress',
      owner,
      rustTestName: testName,
      note: 'Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate.',
    };
  }

  if (isJsOnlyTest(relative)) {
    return {
      portability: 'js-only-documented',
      status: 'js-only-documented',
      owner: 'excluded: js-only-documented',
      rustTestName: 'n/a',
      note: jsOnlyReason(relative),
    };
  }

  const owner = testOwner(relative);
  if (owner !== 'unassigned') {
    return {
      portability: 'portable',
      status: 'in-progress',
      owner,
      rustTestName: 'pending: owner mapped; no named Rust test yet',
      note: portableReason(relative),
    };
  }

  return {
    portability: 'portable',
    status: 'not-started',
    owner: 'unassigned',
    rustTestName: '',
    note: 'No test owner rule matched this file.',
  };
}

function testCaseMapping(testCase) {
  const mapping = testMapping(testCase.file);
  if (mapping.portability === 'portable') {
    const hasNamedTest =
      mapping.rustTestName &&
      mapping.rustTestName !== 'n/a' &&
      !mapping.rustTestName.startsWith('pending:');
    return {
      ...mapping,
      status: hasNamedTest ? 'verified' : 'in-progress',
      note: hasNamedTest
        ? 'Named Rust parity test is mapped at case level; strict gate also verifies the test name exists.'
        : mapping.note,
    };
  }
  return mapping;
}

function portable(owner, note) {
  return {
    classification: 'portable',
    owner,
    note,
  };
}

function jsOnly(note) {
  return {
    classification: 'js-only-documented',
    owner: 'excluded: js-only-documented',
    note,
  };
}

function unmapped(note) {
  return {
    classification: 'unmapped',
    owner: 'unassigned',
    note,
  };
}

function partialRustTestName(relative) {
  const direct = new Map([
    [
      'packages/agent/models.test.ts',
      'open_agent_prepare_composes_prompt_context_model_and_tools; open_agent_generate_records_usage_from_fake_model',
    ],
    [
      'packages/agent/tools/tools.test.ts',
      'open_agent_file_tools_read_write_and_edit_with_fake_sandbox; open_agent_search_bash_and_todo_tools_execute_with_fake_sandbox; open_agent_tool_schemas_serialize_for_tool_loop_agent',
    ],
    [
      'packages/agent/tools/utils.test.ts',
      'open_agent_path_security_blocks_escape_dotenv_and_symlink_escape',
    ],
    [
      'packages/sandbox/git.test.ts',
      'clone_branch_status_diff_and_commit_stay_inside_sandbox; finish_commits_dirty_repository',
    ],
    [
      'packages/sandbox/vercel/sandbox.test.ts',
      'vercel_sandbox_backend_connects_execs_reads_writes_lists_and_stops; vercel_client_create_sandbox_sends_upstream_shape',
    ],
    [
      'packages/sandbox/vercel/snapshot-refresh.test.ts',
      'vercel_client_extend_timeout_and_snapshot_parse_session_updates; live_vercel_sandbox_create_exec_read_write_list_stop_smoke',
    ],
    [
      'apps/web/app/api/chat/_lib/model-selection.test.ts',
      'open_agent_prepare_composes_prompt_context_model_and_tools',
    ],
    [
      'apps/web/app/api/chat/route.test.ts',
      'app_mention_accepts_persists_run_and_records_outbound',
    ],
    [
      'apps/web/app/api/chat/[chatId]/stop/route.test.ts',
      'block_action_cancel_cancels_waiting_run',
    ],
    [
      'apps/web/app/api/chat/[chatId]/stream/route.test.ts',
      'block_action_answer_resumes_waiting_run_to_completion',
    ],
    [
      'apps/web/app/api/sandbox/reconnect/route.test.ts',
      'sandbox_vercel_state_serializes_upstream_factory_shape',
    ],
    [
      'apps/web/app/api/sandbox/route.test.ts',
      'vercel_sandbox_backend_connects_execs_reads_writes_lists_and_stops',
    ],
    [
      'apps/web/app/api/sandbox/snapshot/route.test.ts',
      'vercel_client_extend_timeout_and_snapshot_parse_session_updates',
    ],
    [
      'apps/web/app/api/sandbox/status/route.test.ts',
      'sandbox_context_round_trips_with_optional_fields',
    ],
    [
      'apps/web/app/workflows/chat.test.ts',
      'app_mention_accepts_persists_run_and_records_outbound',
    ],
    [
      'apps/web/app/workflows/chat-post-finish.test.ts',
      'finish_builds_pr_command_in_dry_run_mode',
    ],
    [
      'apps/web/app/workflows/chat-post-finish-usage.test.ts',
      'open_agent_generate_records_usage_from_fake_model; run_usage_contract',
    ],
    [
      'apps/web/lib/chat/auto-commit-direct.test.ts',
      'finish_commits_dirty_repository',
    ],
    [
      'apps/web/lib/chat/auto-pr-direct.test.ts',
      'finish_builds_pr_command_in_dry_run_mode',
    ],
    [
      'apps/web/lib/github/commit.test.ts',
      'finish_commits_dirty_repository',
    ],
    [
      'apps/web/lib/github/pr-content.test.ts',
      'finish_builds_pr_command_in_dry_run_mode',
    ],
    [
      'apps/web/lib/sandbox/lifecycle.test.ts',
      'sandbox_state_serializes_and_reconnects_local_workspace',
    ],
    [
      'apps/web/lib/sandbox/lifecycle-evaluate.test.ts',
      'connect_options_debug_redacts_credentials_and_env_values',
    ],
    [
      'apps/web/lib/sandbox/archive-session.test.ts',
      'sandbox_lifecycle_contract',
    ],
    [
      'apps/web/lib/skills-cache.test.ts',
      'discovers_project_claude_and_global_skills_with_skip_diagnostics',
    ],
    [
      'apps/web/lib/skills/global-skill-installer.test.ts',
      'invoke_skill_injects_directory_and_substitutes_arguments',
    ],
    [
      'apps/web/lib/skills/global-skill-refs.test.ts',
      'loaded_skill_allowed_tools_are_deduplicated',
    ],
    [
      'apps/web/lib/workspace-status-store.test.ts',
      'active_run_contract',
    ],
    [
      'packages/shared/lib/tool-state.test.ts',
      'render_progress_update; render_run_terminal',
    ],
  ]);
  return direct.get(relative);
}

function isJsOnlyTest(relative) {
  return (
    relative.startsWith('apps/web/components/') ||
    relative.startsWith('apps/web/hooks/') ||
    relative.startsWith('apps/web/app/sessions/') ||
    relative.startsWith('apps/web/app/shared/') ||
    relative.startsWith('apps/web/app/api/auth/') ||
    relative.startsWith('apps/web/app/api/github/') ||
    relative.startsWith('apps/web/app/api/settings/') ||
    relative.startsWith('apps/web/app/api/shared/') ||
    relative.startsWith('apps/web/components/') ||
    relative.startsWith('apps/web/instrumentation-client') ||
    relative.startsWith('apps/web/proxy') ||
    relative === 'packages/shared/lib/paste-blocks.test.ts' ||
    relative.startsWith('apps/web/lib/auth/') ||
    relative.startsWith('apps/web/lib/db/public-usage-profile') ||
    relative.startsWith('apps/web/lib/db/usage-domain-leaderboard') ||
    relative.startsWith('apps/web/lib/db/user-preferences') ||
    relative.startsWith('apps/web/lib/diff/') ||
    relative.startsWith('apps/web/lib/pr-deployment-polling') ||
    relative.startsWith('apps/web/lib/random-city') ||
    relative.startsWith('apps/web/lib/rate-limit') ||
    relative.startsWith('apps/web/lib/redis') ||
    relative.startsWith('apps/web/lib/streamdown-config') ||
    relative.startsWith('apps/web/lib/swr') ||
    relative.startsWith('apps/web/lib/usage/') ||
    relative.startsWith('apps/web/lib/vercel/') ||
    relative.startsWith('apps/web/lib/github/client') ||
    relative.startsWith('apps/web/lib/github/installation') ||
    relative.startsWith('apps/web/lib/github/installations') ||
    relative.startsWith('apps/web/lib/github/repo-identifiers') ||
    relative.startsWith('apps/web/lib/github/token')
  );
}

function jsOnlyReason(relative) {
  if (relative.includes('/api/auth/') || relative.includes('/lib/auth/')) {
    return 'Better Auth and browser session identity are web-only; Slack user/team identity is the Rust release surface.';
  }
  if (relative.includes('/api/github/app/') || relative.includes('/lib/github/')) {
    return 'GitHub web app installation/token UI is web-only unless mapped by a separate sandbox git automation row.';
  }
  if (
    relative.includes('/components/') ||
    relative.includes('/hooks/') ||
    relative.includes('/app/sessions/') ||
    relative.includes('/app/shared/')
  ) {
    return 'React/Next page, component, hook, or browser route with no Slack-first Rust UI counterpart.';
  }
  if (relative === 'packages/shared/lib/paste-blocks.test.ts') {
    return 'Paste-token placeholders are browser text-entry helpers; Slack message ingestion uses platform payloads instead.';
  }
  return 'Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port.';
}

function testOwner(relative) {
  if (relative.startsWith('packages/agent/tools/')) {
    return 'ai-sdk-rust::open_agents_tools';
  }
  if (relative.startsWith('packages/agent/')) {
    return 'open-agents-runtime';
  }
  if (relative.startsWith('packages/sandbox/')) {
    return 'open-agents-sandbox';
  }
  if (relative === 'packages/shared/lib/tool-state.test.ts') {
    return 'open-agents-slack';
  }
  if (relative === 'packages/shared/lib/paste-blocks.test.ts') {
    return 'excluded: js-only-documented';
  }
  if (relative.startsWith('apps/web/app/workflows/')) {
    return 'open-agents-service';
  }
  if (relative.startsWith('apps/web/app/api/chat/')) {
    return 'open-agents-service';
  }
  if (relative.startsWith('apps/web/app/api/sandbox/')) {
    return 'open-agents-service/open-agents-sandbox';
  }
  if (relative.startsWith('apps/web/app/api/sessions/')) {
    return 'open-agents-service/open-agents-persistence';
  }
  if (relative.startsWith('apps/web/app/api/models/')) {
    return 'open-agents-runtime';
  }
  if (relative.startsWith('apps/web/app/api/settings/model-variants/')) {
    return 'open-agents-runtime';
  }
  if (relative.startsWith('apps/web/app/api/generate-title/')) {
    return 'open-agents-runtime';
  }
  if (relative.startsWith('apps/web/app/api/generate-pr/')) {
    return 'open-agents-sandbox';
  }
  if (relative.startsWith('apps/web/app/api/vercel/')) {
    return 'open-agents-service';
  }
  if (relative.startsWith('apps/web/lib/db/sessions')) {
    return 'open-agents-persistence';
  }
  if (relative.startsWith('apps/web/lib/assistant-file-links')) {
    return 'open-agents-slack';
  }
  if (relative.startsWith('apps/web/lib/chat/')) {
    return 'open-agents-runtime/open-agents-sandbox';
  }
  if (
    relative.startsWith('apps/web/lib/chat-') ||
    relative.startsWith('apps/web/lib/merge-readiness-polling') ||
    relative.startsWith('apps/web/lib/workspace-status-store')
  ) {
    return 'open-agents-runtime/open-agents-service';
  }
  if (relative.startsWith('apps/web/lib/github/')) {
    return 'open-agents-sandbox';
  }
  if (
    relative.startsWith('apps/web/lib/model') ||
    relative.startsWith('apps/web/lib/models')
  ) {
    return 'open-agents-runtime';
  }
  if (relative.startsWith('apps/web/lib/sandbox/')) {
    return 'open-agents-service/open-agents-sandbox';
  }
  if (relative.startsWith('apps/web/lib/skills')) {
    return 'ai-sdk-rust::skills';
  }
  return 'unassigned';
}

function portableReason(relative) {
  if (relative.includes('/sessions/') || relative.includes('/lib/db/sessions')) {
    return 'Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work.';
  }
  if (relative.includes('/sandbox/')) {
    return 'Sandbox lifecycle/provisioning behavior maps to open-agents-sandbox and service wiring; remaining cases are owner-mapped.';
  }
  if (relative.includes('/github/') || relative.includes('generate-pr')) {
    return 'Git/PR behavior maps to sandbox git automation and finish actions; remaining web-specific fixtures need owner review.';
  }
  if (relative.includes('/chat') || relative.includes('/workflows/')) {
    return 'Remote-agent chat, stream, stop, persistence, and finish behavior maps to Open Agents runtime/service.';
  }
  if (relative.includes('/skills')) {
    return 'Skill discovery/cache/ref behavior maps to ai-sdk-rust skill support.';
  }
  return 'Portable Open Agents behavior is assigned to the Rust owner; no named parity test exists yet.';
}

function buildInventory() {
  if (!fs.existsSync(upstreamRoot)) {
    fail(`Open Agents source mirror not found at ${upstreamRoot}`);
  }

  const allFiles = walk(upstreamRoot)
    .map(relativePath)
    .sort((left, right) => left.localeCompare(right));
  const packageManifests = allFiles.filter((file) => file.endsWith('package.json'));
  const codeFiles = allFiles.filter((file) => codeFilePattern.test(file));
  const testFiles = codeFiles.filter((file) => testFilePattern.test(file));
  const sourceFiles = codeFiles.filter((file) => !testFilePattern.test(file));

  const packageRows = packageManifests.map((file) => [
    packageIdFor(file),
    file,
    packageName(file),
    sourceFiles.filter((source) => packageIdFor(source) === packageIdFor(file))
      .length,
    testFiles.filter((test) => packageIdFor(test) === packageIdFor(file)).length,
    sourceMappingForPackage(packageIdFor(file)),
  ]);

  const sourceRows = sourceFiles.map((file) => {
    const mapping = sourceMapping(file);
    return [
      packageIdFor(file),
      file,
      mapping.classification,
      mapping.owner,
      mapping.note,
    ];
  });

  const testRows = testFiles.map((file) => {
    const mapping = testMapping(file);
    return [
      packageIdFor(file),
      file,
      countTestCalls(file),
      mapping.portability,
      mapping.owner,
      mapping.rustTestName,
      mapping.status,
      mapping.note,
    ];
  });

  const rustTestNames = discoverRustTests();
  const testCaseRows = testFiles.flatMap((file) =>
    extractTestCalls(file).map((testCase) => {
      const mapping = testCaseMapping(testCase);
      const row = [
        testCase.packageId,
        testCase.file,
        testCase.line,
        testCase.suitePath,
        testCase.caseName,
        testCase.declaration,
        mapping.portability,
        mapping.owner,
        mapping.rustTestName,
        mapping.status,
        mapping.note,
      ];
      const missingRustTests = strictGapReason(row, rustTestNames);
      if (missingRustTests && !isPendingRustTest(mapping.rustTestName)) {
        row[9] = 'in-progress';
        row[10] =
          'Named Rust parity marker exists, but strict gate did not find every referenced Rust test in the workspace.';
      }
      return row;
    })
  );
  const strictGapRows = buildStrictGapRows(testCaseRows, rustTestNames);

  return {
    allFiles,
    packageManifests,
    codeFiles,
    sourceFiles,
    testFiles,
    packageRows,
    sourceRows,
    testRows,
    testCaseRows,
    strictGapRows,
  };
}

function sourceMappingForPackage(packageId) {
  if (packageId === 'packages/agent') {
    return 'open-agents-runtime plus ai-sdk-rust::open_agents_tools, ::subagents, and ::skills';
  }
  if (packageId === 'packages/sandbox') {
    return 'open-agents-sandbox';
  }
  if (packageId === 'packages/shared') {
    return 'open-agents-slack/open-agents-sandbox for portable helpers; React hooks excluded';
  }
  if (packageId === 'apps/web') {
    return 'open-agents-service/open-agents-runtime/open-agents-sandbox for portable server behavior; Next/React UI excluded';
  }
  if (packageId === 'root') {
    return 'workspace metadata only';
  }
  if (packageId === 'packages/tsconfig') {
    return 'js-only-documented TypeScript build config';
  }
  return 'js-only-documented';
}

function validateInventory(inventory) {
  const errors = [];
  if (inventory.packageManifests.length !== expectedPackageManifestCount) {
    errors.push(
      `expected ${expectedPackageManifestCount} package manifests, found ${inventory.packageManifests.length}`
    );
  }
  if (inventory.sourceFiles.length !== expectedSourceFileCount) {
    errors.push(
      `expected ${expectedSourceFileCount} source files, found ${inventory.sourceFiles.length}`
    );
  }
  if (inventory.testFiles.length !== expectedTestFileCount) {
    errors.push(
      `expected ${expectedTestFileCount} test files, found ${inventory.testFiles.length}`
    );
  }
  if (inventory.codeFiles.length !== expectedTsxFileCount) {
    errors.push(
      `expected ${expectedTsxFileCount} TS/TSX files, found ${inventory.codeFiles.length}`
    );
  }

  for (const row of inventory.sourceRows) {
    const [packageId, file, classification, owner, note] = row;
    if (!validPortability.has(classification)) {
      errors.push(`${file} has invalid source classification ${classification}`);
    }
    if (!owner || owner === 'unassigned') {
      errors.push(`${file} has no Rust source owner or documented exclusion`);
    }
    if (!note) {
      errors.push(`${file} has no source note`);
    }
    if (!packageId) {
      errors.push(`${file} has no package id`);
    }
  }

  for (const row of inventory.testRows) {
    const [, file, cases, portability, owner, rustTestName, status, note] = row;
    if (!Number.isInteger(Number(cases)) || Number(cases) < 0) {
      errors.push(`${file} has invalid test case count ${cases}`);
    }
    if (!validPortability.has(portability)) {
      errors.push(`${file} has invalid portability ${portability}`);
    }
    if (!validStatus.has(status)) {
      errors.push(`${file} has invalid status ${status}`);
    }
    if (!owner || owner === 'unassigned') {
      errors.push(`${file} has no Rust test owner or documented exception`);
    }
    if (!rustTestName) {
      errors.push(`${file} has no named Rust test, pending owner marker, or n/a exception`);
    }
    if (!note) {
      errors.push(`${file} has no test note`);
    }
    if (portability === 'portable' && status === 'verified' && rustTestName.startsWith('pending:')) {
      errors.push(`${file} is verified but still has a pending Rust test marker`);
    }
    if (portability !== 'portable' && !status.endsWith('-documented') && status !== portability) {
      errors.push(`${file} exclusion has inconsistent status ${status}`);
    }
  }

  const caseCountByFile = new Map();
  for (const row of inventory.testCaseRows) {
    const [
      packageId,
      file,
      line,
      suite,
      caseName,
      declaration,
      portability,
      owner,
      rustTestName,
      status,
      note,
    ] = row;
    caseCountByFile.set(file, (caseCountByFile.get(file) ?? 0) + 1);
    if (!packageId) {
      errors.push(`${file}:${line} has no package id`);
    }
    if (!Number.isInteger(Number(line)) || Number(line) < 1) {
      errors.push(`${file} has invalid test case line ${line}`);
    }
    if (!suite) {
      errors.push(`${file}:${line} has no suite path`);
    }
    if (!caseName) {
      errors.push(`${file}:${line} has no case name`);
    }
    if (declaration !== 'it' && declaration !== 'test') {
      errors.push(`${file}:${line} has invalid test declaration ${declaration}`);
    }
    if (!validPortability.has(portability)) {
      errors.push(`${file}:${line} has invalid test-case portability ${portability}`);
    }
    if (!validStatus.has(status)) {
      errors.push(`${file}:${line} has invalid test-case status ${status}`);
    }
    if (!owner || owner === 'unassigned') {
      errors.push(`${file}:${line} has no Rust test owner or documented exception`);
    }
    if (!rustTestName) {
      errors.push(`${file}:${line} has no named Rust test, pending owner marker, or n/a exception`);
    }
    if (!note) {
      errors.push(`${file}:${line} has no test-case note`);
    }
  }

  for (const row of inventory.testRows) {
    const [, file, cases] = row;
    const extracted = caseCountByFile.get(file) ?? 0;
    if (Number(cases) !== extracted) {
      errors.push(`${file} counted ${cases} test calls but extracted ${extracted} test case rows`);
    }
  }

  return errors;
}

function summarizeTests(testRows) {
  const summary = new Map();
  for (const row of testRows) {
    const [packageId, , cases, portability, , , status] = row;
    if (!summary.has(packageId)) {
      summary.set(packageId, {
        packageId,
        files: 0,
        cases: 0,
        portable: 0,
        verified: 0,
        inProgress: 0,
        jsOnly: 0,
        typeSystem: 0,
      });
    }
    const entry = summary.get(packageId);
    entry.files += 1;
    entry.cases += Number(cases);
    if (portability === 'portable') {
      entry.portable += 1;
    } else if (portability === 'js-only-documented') {
      entry.jsOnly += 1;
    } else if (portability === 'type-system-impossible') {
      entry.typeSystem += 1;
    }
    if (status === 'verified') {
      entry.verified += 1;
    } else if (status === 'in-progress' || status === 'not-started') {
      entry.inProgress += 1;
    }
  }
  return [...summary.values()]
    .sort((left, right) => left.packageId.localeCompare(right.packageId))
    .map((entry) => [
      entry.packageId,
      entry.files,
      entry.cases,
      entry.portable,
      entry.verified,
      entry.inProgress,
      entry.jsOnly,
      entry.typeSystem,
    ]);
}

function summarizeTestCases(testCaseRows, testFiles) {
  const summary = new Map();
  for (const file of testFiles) {
    const packageId = packageIdFor(file);
    if (!summary.has(packageId)) {
      summary.set(packageId, {
        packageId,
        files: 0,
        cases: 0,
        portable: 0,
        mappedPortable: 0,
        unmappedPortable: 0,
        jsOnly: 0,
        typeSystem: 0,
      });
    }
    summary.get(packageId).files += 1;
  }
  for (const row of testCaseRows) {
    const [packageId, , , , , , portability, , rustTestName] = row;
    const entry = summary.get(packageId);
    entry.cases += 1;
    if (portability === 'portable') {
      entry.portable += 1;
      if (rustTestName && !rustTestName.startsWith('pending:')) {
        entry.mappedPortable += 1;
      } else {
        entry.unmappedPortable += 1;
      }
    } else if (portability === 'js-only-documented') {
      entry.jsOnly += 1;
    } else if (portability === 'type-system-impossible') {
      entry.typeSystem += 1;
    }
  }
  return [...summary.values()]
    .sort((left, right) => left.packageId.localeCompare(right.packageId))
    .map((entry) => [
      entry.packageId,
      entry.files,
      entry.cases,
      entry.portable,
      entry.mappedPortable,
      entry.unmappedPortable,
      entry.jsOnly,
      entry.typeSystem,
    ]);
}

function strictGapReason(row, rustTestNames) {
  const [, , , , , , portability, , rustTestName] = row;
  if (portability !== 'portable') {
    return undefined;
  }
  if (isPendingRustTest(rustTestName)) {
    return 'missing named Rust test';
  }
  const missingNames = splitRustTestNames(rustTestName).filter(
    (testName) => !rustTestNames.has(testName)
  );
  if (missingNames.length > 0) {
    return `named Rust test not found: ${missingNames.join(', ')}`;
  }
  return undefined;
}

function buildStrictGapRows(testCaseRows, rustTestNames) {
  return testCaseRows.flatMap((row) => {
    const [
      packageId,
      file,
      line,
      suite,
      caseName,
      ,
      ,
      owner,
      rustTestName,
      ,
      note,
    ] = row;
    const reason = strictGapReason(row, rustTestNames);
    if (!reason) {
      return [];
    }
    return [[packageId, file, line, suite, caseName, owner, reason, rustTestName, note]];
  });
}

function summarizeStrictGaps(strictGapRows) {
  const summary = new Map();
  for (const row of strictGapRows) {
    const [, , , , , owner, reason] = row;
    if (!summary.has(owner)) {
      summary.set(owner, {
        owner,
        missingNamed: 0,
        missingWorkspaceTest: 0,
        total: 0,
      });
    }
    const entry = summary.get(owner);
    entry.total += 1;
    if (reason === 'missing named Rust test') {
      entry.missingNamed += 1;
    } else {
      entry.missingWorkspaceTest += 1;
    }
  }
  return [...summary.values()]
    .sort((left, right) => left.owner.localeCompare(right.owner))
    .map((entry) => [
      entry.owner,
      entry.missingNamed,
      entry.missingWorkspaceTest,
      entry.total,
    ]);
}

function summarizeSources(sourceRows) {
  const summary = new Map();
  for (const row of sourceRows) {
    const [packageId, , classification] = row;
    if (!summary.has(packageId)) {
      summary.set(packageId, {
        packageId,
        files: 0,
        portable: 0,
        jsOnly: 0,
        typeSystem: 0,
      });
    }
    const entry = summary.get(packageId);
    entry.files += 1;
    if (classification === 'portable') {
      entry.portable += 1;
    } else if (classification === 'js-only-documented') {
      entry.jsOnly += 1;
    } else if (classification === 'type-system-impossible') {
      entry.typeSystem += 1;
    }
  }
  return [...summary.values()]
    .sort((left, right) => left.packageId.localeCompare(right.packageId))
    .map((entry) => [
      entry.packageId,
      entry.files,
      entry.portable,
      entry.jsOnly,
      entry.typeSystem,
    ]);
}

function renderMarkdown(inventory) {
  return `# Open Agents Upstream Parity

This ledger is generated from the refreshed upstream Open Agents mirror and is
the OA-01 gate for future Rust implementation buckets. Every current upstream
package manifest, non-test TS/TSX source file, and test file must have a Rust
owner, a named Rust test, or an explicit documented exception.
The test inventory is case-level: each portable upstream \`it(...)\` or
\`test(...)\` call maps to a named Rust test or remains a strict-gate failure
until the owning child bucket closes it.

## Source Snapshot

| Field | Value |
| --- | --- |
| Upstream repo | \`${upstreamRepo}\` |
| Inventory command | \`npx opensrc fetch https://github.com/vercel-labs/open-agents\` |
| Local source path | \`${upstreamRoot}\` |
| Remote HEAD verification | \`git ls-remote https://github.com/vercel-labs/open-agents HEAD\` |
| Upstream commit | \`${upstreamHead}\` |
| Inventory date | \`${inventoryDate}\` |
| Package manifests | ${inventory.packageManifests.length} |
| TS/TSX files | ${inventory.codeFiles.length} |
| Non-test source files | ${inventory.sourceFiles.length} |
| Test files | ${inventory.testFiles.length} |
| Test cases | ${inventory.testCaseRows.length} |
| Strict gate gaps | ${inventory.strictGapRows.length} |
| Gate command | \`node scripts/open-agents-test-inventory.mjs --check\` |
| Strict gate command | \`node scripts/open-agents-parity-check.mjs --strict\` |
| CI report command | \`node scripts/open-agents-parity-check.mjs --check\` |

## Status Rules

Use \`portable\` for behavior that must be owned by Rust, \`js-only-documented\`
for browser, Next.js, Bun, React, Better Auth, or web-account behavior outside
the Slack-first Rust release, and \`type-system-impossible\` only for TypeScript
language-service assertions that cannot become Rust runtime checks.

Rows marked \`in-progress\` are not complete parity. They are owner-mapped
inventory rows that future Open Agents buckets must close with named Rust tests.
The strict gate fails every portable case-level row that keeps an owner-only
pending marker. Rows marked \`js-only-documented\` are explicit exclusions and
must carry the reason in the notes column.

## Gate Rules

- The refreshed upstream count is expected to be exactly ${expectedPackageManifestCount} package manifests, ${expectedSourceFileCount} non-test source files, and ${expectedTestFileCount} test files. If upstream changes, update this ledger in the same commit that explains the drift.
- The checker fails when any source file lacks a Rust owner or documented exclusion.
- The inventory checker fails when any test file or test case lacks a Rust owner, named Rust test, pending owner marker, or explicit exception.
- The strict gate fails when any portable test case lacks a named Rust test, keeps an owner-only pending marker, or names a Rust test that is not present in the workspace.
- Extra Rust tests are additive. They do not close a portable upstream row unless the row names the Rust test.

## Package Inventory

${renderTable(
    ['Package id', 'Manifest', 'Package name', 'Source files', 'Test files', 'Rust owner/exclusion'],
    inventory.packageRows
  )}

## Source Summary

${renderTable(
    ['Package', 'Source files', 'Portable', 'JS only', 'Type system'],
    summarizeSources(inventory.sourceRows)
  )}

## Test Summary

${renderTable(
    [
      'Package',
      'Test files',
      'Case calls',
      'Portable files',
      'Verified files',
      'In-progress files',
      'JS-only files',
      'Type-system files',
    ],
    summarizeTests(inventory.testRows)
  )}

## Test Case Summary

${renderTable(
    [
      'Package',
      'Test files',
      'Cases',
      'Portable cases',
      'Mapped portable cases',
      'Unmapped portable cases',
      'JS-only cases',
      'Type-system cases',
    ],
    summarizeTestCases(inventory.testCaseRows, inventory.testFiles)
  )}

## Strict Gap Summary

${renderTable(
    [
      'Rust owner crate/module',
      'Missing named Rust test',
      'Named Rust test not found',
      'Total strict gaps',
    ],
    summarizeStrictGaps(inventory.strictGapRows)
  )}

## Strict Gap Handoff

${renderTable(
    [
      'Package',
      'Upstream test file',
      'Line',
      'Suite',
      'Case',
      'Rust owner crate/module',
      'Gap reason',
      'Current marker',
      'Notes',
    ],
    inventory.strictGapRows
  )}

## Source File Inventory

${renderTable(
    ['Package', 'Upstream source file', 'Classification', 'Rust owner/exclusion', 'Notes'],
    inventory.sourceRows
  )}

## Test File Inventory

${renderTable(
    [
      'Package',
      'Upstream test file',
      'Case calls',
      'Portability',
      'Rust owner/exclusion',
      'Named Rust test or marker',
      'Status',
      'Notes',
    ],
    inventory.testRows
  )}

## Test Case Inventory

${renderTable(
    [
      'Package',
      'Upstream test file',
      'Line',
      'Suite',
      'Case',
      'Declaration',
      'Portability',
      'Rust owner crate/module',
      'Rust test name or exception',
      'Status',
      'Notes',
    ],
    inventory.testCaseRows
  )}
`;
}

const options = parseArgs(process.argv.slice(2));
const inventory = buildInventory();
const errors = validateInventory(inventory);
if (errors.length > 0) {
  fail(`found ${errors.length} inventory error(s):\n- ${errors.join('\n- ')}`);
}

if (options.dryRun) {
  console.log(
    JSON.stringify(
      {
        packageManifests: inventory.packageManifests.length,
        sourceFiles: inventory.sourceFiles.length,
        testFiles: inventory.testFiles.length,
        testCases: inventory.testCaseRows.length,
        portableTestCases: inventory.testCaseRows.filter(
          (row) => row[6] === 'portable'
        ).length,
        unmappedPortableTestCases: inventory.testCaseRows.filter(
          (row) => row[6] === 'portable' && row[8].startsWith('pending:')
        ).length,
        strictGateGaps: inventory.strictGapRows.length,
        tsxFiles: inventory.codeFiles.length,
        packageIds: [...new Set(inventory.packageRows.map((row) => row[0]))],
      },
      null,
      2
    )
  );
} else {
  const markdown = renderMarkdown(inventory);
  if (options.check) {
    const current = fs.existsSync(outputPath)
      ? fs.readFileSync(outputPath, 'utf8')
      : '';
    if (current !== markdown) {
      fail(`${path.relative(repositoryRoot, outputPath)} is not up to date`);
    }
    console.log(
      `ok: ${path.relative(repositoryRoot, outputPath)} covers ${inventory.packageManifests.length} package manifests, ${inventory.sourceFiles.length} source files, ${inventory.testFiles.length} test files, and ${inventory.testCaseRows.length} test cases.`
    );
  } else {
    fs.writeFileSync(outputPath, markdown);
    console.log(
      `Wrote ${path.relative(repositoryRoot, outputPath)} with ${inventory.packageManifests.length} package manifests, ${inventory.sourceFiles.length} source files, ${inventory.testFiles.length} test files, and ${inventory.testCaseRows.length} test cases.`
    );
  }
}
