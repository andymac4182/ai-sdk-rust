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
  const verified = verifiedRustTestMapping(relative);
  if (verified) {
    const owner = testOwner(relative);
    return {
      portability: 'portable',
      status: 'verified',
      owner,
      rustTestName: verified,
      note: 'All portable upstream cases for this session/persistence row are mapped to deterministic named Rust tests; no live Slack, Vercel, or browser credentials required.',
    };
  }

  const testName = partialRustTestName(relative);
  if (testName) {
    const owner = testOwner(relative);
    const verified = verifiedTestFiles().has(relative);
    return {
      portability: 'portable',
      status: verified ? 'verified' : 'in-progress',
      owner,
      rustTestName: testName,
      note: verified
        ? 'Portable upstream cases are mapped to named Rust tests in the owning Rust surface.'
        : 'Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate.',
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
  const names = (...items) => items.join('; ');
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
      names(
        'package_sandbox_git_sync_stashes_local_changes_resets_to_remote_and_restores_changes',
        'package_sandbox_git_sync_returns_without_touching_local_changes_when_remote_branch_is_missing',
        'package_sandbox_git_sync_rolls_back_and_restores_local_changes_when_stash_restore_conflicts'
      ),
    ],
    [
      'packages/sandbox/vercel/sandbox.test.ts',
      names(
        'vercel_sandbox_skips_dev_server_urls_for_ports_that_are_missing_routes',
        'vercel_sandbox_uses_first_routable_declared_port_for_host_when_port_80_is_unavailable',
        'vercel_sandbox_does_not_render_an_undefined_host_in_environment_details',
        'vercel_sandbox_resolves_host_from_sdk_routes_when_reconnect_did_not_pass_ports',
        'vercel_sandbox_injects_runtime_preview_env_vars_into_command_execution',
        'vercel_sandbox_preserves_stderr_output_from_failed_commands',
        'vercel_sandbox_connects_by_persistent_sandbox_name_without_auto_resume_by_default',
        'vercel_sandbox_persists_sandbox_name_in_state_for_created_sandboxes',
        'vercel_sandbox_derives_resumed_expires_at_without_provider_stop_buffer',
        'vercel_sandbox_refreshes_state_when_current_session_changes_from_stopped_to_running',
        'vercel_sandbox_applies_setup_github_auth_when_creating_sandbox_and_then_clears_it',
        'vercel_sandbox_clears_github_auth_when_reconnecting_to_sandbox',
        'vercel_sandbox_creates_from_base_snapshot_and_clones_git_source',
        'vercel_sandbox_creates_empty_git_repo_from_base_snapshot',
        'vercel_sandbox_skips_git_workspace_bootstrap_from_base_snapshot_when_requested',
        'vercel_sandbox_returns_command_id_when_quick_failure_timer_elapses_before_command_exits',
        'vercel_sandbox_throws_when_detached_wait_fails_before_timer_elapses',
        'vercel_sandbox_throws_with_stderr_when_command_exits_quickly_with_non_zero_code',
        'vercel_sandbox_returns_file_content_as_string_via_sdk_read_file_to_buffer',
        'vercel_sandbox_throws_when_file_does_not_exist',
        'vercel_sandbox_preserves_multi_byte_utf8_content',
        'vercel_sandbox_delegates_to_sdk_write_files_with_a_buffer',
        'vercel_sandbox_creates_parent_directory_via_mkdir_before_writing',
        'vercel_sandbox_handles_large_content_without_using_run_command_for_write'
      ),
    ],
    [
      'packages/sandbox/vercel/snapshot-refresh.test.ts',
      names(
        'snapshot_refresh_creates_a_new_snapshot_from_the_configured_base_snapshot',
        'snapshot_refresh_stops_the_sandbox_and_surfaces_command_output_when_setup_fails',
        'snapshot_refresh_stops_the_sandbox_when_snapshot_support_is_unavailable',
        'live_vercel_sandbox_create_exec_read_write_list_stop_smoke'
      ),
    ],
    [
      'apps/web/app/api/chat/_lib/model-selection.test.ts',
      'chat_model_selection_resolves_direct_variant_builtin_missing_and_default_cases; open_agent_prepare_composes_prompt_context_model_and_tools',
    ],
    [
      'apps/web/app/api/chat/_lib/persist-tool-results.test.ts',
      'chat_post_route_persists_messages_activity_stream_chunks_and_model_metadata; dedupe_message_reasoning_matches_openai_azure_and_immutability_cases',
    ],
    [
      'apps/web/app/api/chat/route.test.ts',
      'chat_post_route_persists_messages_activity_stream_chunks_and_model_metadata; chat_route_thread_reply_resumes_waiting_run_and_starts_new_after_stale_terminal_run; app_mention_accepts_persists_run_and_records_outbound',
    ],
    [
      'apps/web/app/api/chat/[chatId]/stop/route.test.ts',
      'chat_stop_route_cancel_persists_abort_snapshot_and_clears_activity; block_action_cancel_cancels_waiting_run',
    ],
    [
      'apps/web/app/api/chat/[chatId]/stream/route.test.ts',
      'chat_post_route_persists_messages_activity_stream_chunks_and_model_metadata; chat_route_thread_reply_resumes_waiting_run_and_starts_new_after_stale_terminal_run; cancelable_readable_stream_semantics_match_forwarding_abort_and_idempotent_cancel_cases',
    ],
    [
      'apps/web/app/api/generate-pr/_lib/generate-pr-helpers.test.ts',
      names(
        'generate_pr_helpers_generate_branch_name_uses_initials_and_8_char_suffix',
        'generate_pr_helpers_looks_like_commit_hash_detects_commit_looking_strings',
        'generate_pr_helpers_is_permission_push_error_detects_permission_errors',
        'generate_pr_helpers_redact_github_token_removes_token_from_authenticated_urls',
        'generate_pr_helpers_extract_github_owner_from_remote_url_handles_https_and_ssh_remotes',
        'generate_pr_helpers_get_conversation_context_returns_only_text_parts_with_role_labels'
      ),
    ],
    [
      'apps/web/app/api/sandbox/reconnect/route.test.ts',
      names(
        'sandbox_reconnect_route_recovers_failed_lifecycle_state_when_reconnect_succeeds',
        'sandbox_reconnect_route_marks_sandbox_expired_when_probe_hits_410',
        'sandbox_reconnect_route_drops_missing_resume_handle_when_probe_hits_404'
      ),
    ],
    [
      'apps/web/app/api/sandbox/route.test.ts',
      names(
        'sandbox_route_uses_session_id_as_persistent_sandbox_name',
        'sandbox_route_repo_sandboxes_use_setup_only_installation_token_instead_of_embedding_it',
        'sandbox_route_rejects_repo_urls_that_only_contain_github_com_in_the_path',
        'sandbox_route_new_vercel_sandbox_does_not_sync_linked_development_env_vars_while_commented_out',
        'sandbox_route_commented_out_env_sync_does_not_run_during_sandbox_creation',
        'sandbox_route_new_sandboxes_install_global_skills',
        'sandbox_route_rejects_unsupported_sandbox_types'
      ),
    ],
    [
      'apps/web/app/api/sandbox/snapshot/route.test.ts',
      names(
        'sandbox_snapshot_route_post_pauses_named_persistent_sandbox_without_writing_legacy_snapshot',
        'sandbox_snapshot_route_put_resumes_existing_named_persistent_sandbox',
        'sandbox_snapshot_route_put_clears_broken_persistent_sandbox_handle_after_404',
        'sandbox_snapshot_route_put_lazily_migrates_legacy_snapshot_backed_session_on_first_resume'
      ),
    ],
    [
      'apps/web/app/api/sandbox/status/route.test.ts',
      names(
        'sandbox_status_route_kicks_overdue_lifecycle_immediately',
        'sandbox_status_route_recovers_failed_lifecycle_state_when_runtime_sandbox_is_still_active'
      ),
    ],
    [
      'apps/web/app/api/vercel/projects/[idOrName]/env/route.test.ts',
      'vercel_projects_env_returns_not_found_and_never_proxies_decrypted_env_values_to_browser',
    ],
    [
      'apps/web/app/api/vercel/repo-projects/route.test.ts',
      names(
        'vercel_repo_projects_returns_remembered_default_when_it_still_exists_in_live_candidates',
        'vercel_repo_projects_auto_selects_lone_matching_live_project_without_saved_default',
        'vercel_repo_projects_asks_client_to_reconnect_vercel_when_token_is_invalid'
      ),
    ],
    [
      'apps/web/app/workflows/chat.test.ts',
      'chat_post_route_persists_messages_activity_stream_chunks_and_model_metadata; chat_route_thread_reply_resumes_waiting_run_and_starts_new_after_stale_terminal_run; block_action_answer_resumes_waiting_run_to_completion; block_action_cancel_cancels_waiting_run',
    ],
    [
      'apps/web/app/workflows/chat-post-finish.test.ts',
      'chat_post_route_persists_messages_activity_stream_chunks_and_model_metadata; chat_stop_route_cancel_persists_abort_snapshot_and_clears_activity; finish_builds_pr_command_in_dry_run_mode; finish_commits_dirty_repository',
    ],
    [
      'apps/web/app/workflows/chat-post-finish-usage.test.ts',
      'open_agent_generate_records_usage_from_fake_model; run_usage_contract; chat_post_route_persists_messages_activity_stream_chunks_and_model_metadata',
    ],
    [
      'apps/web/lib/chat-auto-commit.test.ts',
      'finish_commits_dirty_repository; finish_commits_and_pushes_in_dry_run_mode; finish_builds_pr_command_in_dry_run_mode',
    ],
    [
      'apps/web/lib/chat/auto-commit-direct.test.ts',
      names(
        'auto_commit_direct_returns_early_with_no_commit_when_no_changes',
        'auto_commit_direct_returns_error_when_staging_fails',
        'auto_commit_direct_returns_error_when_repo_access_verification_fails',
        'auto_commit_direct_returns_error_when_api_commit_fails',
        'auto_commit_direct_full_success_path_returns_all_fields',
        'auto_commit_direct_uses_fallback_commit_message_when_diff_is_empty',
        'auto_commit_direct_truncates_generated_commit_message_to_72_chars',
        'auto_commit_direct_returns_early_when_no_changed_files_after_staging'
      ),
    ],
    [
      'apps/web/lib/chat/auto-pr-direct.test.ts',
      names(
        'auto_pr_direct_skips_when_current_branch_is_detached',
        'auto_pr_direct_skips_when_current_branch_matches_the_default_branch',
        'auto_pr_direct_skips_when_repository_owner_is_not_a_safe_github_path_segment',
        'auto_pr_direct_skips_when_current_branch_is_not_available_on_origin',
        'auto_pr_direct_skips_when_current_branch_is_not_fully_pushed_to_origin',
        'auto_pr_direct_syncs_an_existing_open_pull_request_instead_of_creating_a_new_one',
        'auto_pr_direct_creates_a_new_pull_request_and_persists_pr_metadata',
        'auto_pr_direct_returns_an_error_when_pr_content_generation_fails_unexpectedly'
      ),
    ],
    [
      'apps/web/lib/github/commit-intent.test.ts',
      names(
        'commit_intent_accepts_normal_repo_relative_paths',
        'commit_intent_rejects_unsafe_paths',
        'commit_message_attribution_adds_co_author_trailer_when_user_attribution_is_provided',
        'commit_message_attribution_leaves_commit_message_unchanged_without_user_attribution'
      ),
    ],
    [
      'apps/web/lib/chat-route-cleanup.test.ts',
      'chat_route_cleanup_clears_local_state_without_stopping_active_run',
    ],
    [
      'apps/web/lib/chat-streaming-state.test.ts',
      'chat_streaming_state_matches_upstream_inflight_rendering_git_and_refresh_cases',
    ],
    [
      'apps/web/lib/chat/create-cancelable-readable-stream.test.ts',
      'cancelable_readable_stream_semantics_match_forwarding_abort_and_idempotent_cancel_cases',
    ],
    [
      'apps/web/lib/chat/dedupe-message-reasoning.test.ts',
      'dedupe_message_reasoning_matches_openai_azure_and_immutability_cases',
    ],
    [
      'apps/web/lib/github/commit.test.ts',
      names(
        'github_commit_creates_a_missing_branch_from_the_captured_sandbox_head',
        'github_commit_rejects_existing_branches_when_the_remote_head_changed'
      ),
    ],
    [
      'apps/web/lib/github/pr-content.test.ts',
      names(
        'pr_content_resolve_context_section_returns_single_line_footer_with_chat_link_and_attribution',
        'pr_content_resolve_context_section_falls_back_to_plain_text_attribution_without_github_account',
        'pr_content_resolve_app_base_url_prefers_the_active_deployment_url',
        'pr_content_append_context_section_appends_footer_after_horizontal_rule'
      ),
    ],
    [
      'apps/web/lib/sandbox/lifecycle.test.ts',
      names(
        'sandbox_lifecycle_prefers_hibernate_after_when_earlier_than_expiry',
        'sandbox_lifecycle_uses_sandbox_expiry_when_it_is_earlier',
        'sandbox_lifecycle_falls_back_to_last_activity_when_hibernate_after_is_missing',
        'sandbox_lifecycle_falls_back_to_updated_at_when_last_activity_is_missing'
      ),
    ],
    [
      'apps/web/lib/sandbox/lifecycle-kick.test.ts',
      names(
        'lifecycle_kick_claims_lifecycle_lease_before_starting_so_overlapping_kicks_only_start_one_workflow',
        'lifecycle_kick_releases_claimed_lease_and_falls_back_inline_when_workflow_start_fails'
      ),
    ],
    [
      'apps/web/lib/sandbox/lifecycle-evaluate.test.ts',
      names(
        'lifecycle_evaluate_skips_hibernation_whenever_any_chat_still_has_active_stream_id',
        'lifecycle_evaluate_rechecks_for_active_stream_id_before_stopping_and_restores_active_state',
        'lifecycle_evaluate_skips_hibernation_when_lifecycle_timing_is_refreshed_before_stopping',
        'lifecycle_evaluate_hibernates_by_stopping_the_persistent_sandbox_session'
      ),
    ],
    [
      'apps/web/lib/sandbox/archive-session.test.ts',
      names(
        'archive_session_clears_runtime_sandbox_state_when_archive_finalization_fails_without_snapshot',
        'archive_session_preserves_runtime_sandbox_state_when_archive_finalization_fails_but_snapshot_exists',
        'archive_session_refreshes_merged_pr_status_before_archiving'
      ),
    ],
    [
      'apps/web/lib/merge-readiness-polling.test.ts',
      'merge_readiness_polling_matches_pending_warmup_transient_and_blocked_cases',
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
      'workspace_status_store_tracks_latest_status_and_subscribers; active_run_contract',
    ],
    [
      'packages/shared/lib/tool-state.test.ts',
      'render_progress_update; render_run_terminal',
    ],
  ]);
  return direct.get(relative);
}

function verifiedTestFiles() {
  return new Set([
    'apps/web/app/api/generate-pr/_lib/generate-pr-helpers.test.ts',
    'apps/web/app/api/sandbox/reconnect/route.test.ts',
    'apps/web/app/api/sandbox/route.test.ts',
    'apps/web/app/api/sandbox/snapshot/route.test.ts',
    'apps/web/app/api/sandbox/status/route.test.ts',
    'apps/web/app/api/vercel/projects/[idOrName]/env/route.test.ts',
    'apps/web/app/api/vercel/repo-projects/route.test.ts',
    'apps/web/lib/chat/auto-commit-direct.test.ts',
    'apps/web/lib/chat/auto-pr-direct.test.ts',
    'apps/web/lib/github/commit-intent.test.ts',
    'apps/web/lib/github/commit.test.ts',
    'apps/web/lib/github/pr-content.test.ts',
    'apps/web/lib/sandbox/archive-session.test.ts',
    'apps/web/lib/sandbox/lifecycle-evaluate.test.ts',
    'apps/web/lib/sandbox/lifecycle-kick.test.ts',
    'apps/web/lib/sandbox/lifecycle.test.ts',
    'packages/sandbox/git.test.ts',
    'packages/sandbox/vercel/sandbox.test.ts',
    'packages/sandbox/vercel/snapshot-refresh.test.ts',
  ]);
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

function verifiedRustTestMapping(relative) {
  const direct = new Map([
    [
      'apps/web/app/api/sessions/_lib/session-context.test.ts',
      'session_context_guard_contract',
    ],
    [
      'apps/web/app/api/sessions/[sessionId]/chats/[chatId]/fork/route.test.ts',
      'session_chat_fork_route_copies_messages_through_selected_assistant',
    ],
    [
      'apps/web/app/api/sessions/[sessionId]/chats/[chatId]/messages/[messageId]/route.test.ts',
      'session_chat_messages_route_scoped_upsert_and_delete_contract',
    ],
    [
      'apps/web/app/api/sessions/[sessionId]/chats/[chatId]/messages/route.test.ts',
      'session_chat_messages_route_scoped_upsert_and_delete_contract',
    ],
    [
      'apps/web/app/api/sessions/[sessionId]/chats/[chatId]/read/route.test.ts',
      'session_chat_read_route_marks_authenticated_owned_chat_read',
    ],
    [
      'apps/web/app/api/sessions/[sessionId]/chats/[chatId]/route.test.ts',
      'session_chats_route_lists_creates_updates_and_deletes_chats',
    ],
    [
      'apps/web/app/api/sessions/[sessionId]/chats/[chatId]/share/route.test.ts',
      'session_chat_share_route_creates_reuses_and_revokes_share',
    ],
    [
      'apps/web/app/api/sessions/[sessionId]/chats/route.test.ts',
      'session_chats_route_lists_creates_updates_and_deletes_chats',
    ],
    [
      'apps/web/app/api/sessions/[sessionId]/code-editor/route.test.ts',
      'session_code_editor_route_reuses_owned_process_and_rejects_unrelated_ports',
    ],
    [
      'apps/web/app/api/sessions/[sessionId]/dev-server/route.test.ts',
      'session_dev_server_route_prefers_app_reuses_state_and_plans_dependency_installs',
    ],
    [
      'apps/web/app/api/sessions/[sessionId]/diff/_lib/diff-utils.test.ts',
      'session_diff_route_parses_git_output_and_untracked_files',
    ],
    [
      'apps/web/app/api/sessions/[sessionId]/files/content/route.test.ts',
      'session_files_content_route_normalizes_paths_and_classifies_sandbox_failures',
    ],
    [
      'apps/web/app/api/sessions/[sessionId]/share/route.test.ts',
      'deprecated_session_share_route_returns_gone_guidance',
    ],
    [
      'apps/web/app/api/sessions/[sessionId]/skills/route.test.ts',
      'session_skills_route_uses_cache_until_refresh_requests_discovery',
    ],
    [
      'apps/web/app/api/sessions/route.test.ts',
      'sessions_route_enforces_trial_vercel_linking_skill_and_auto_pr_policy',
    ],
    [
      'apps/web/lib/db/sessions.test.ts',
      'db_sessions_normalizes_legacy_sandbox_state_and_deduplicates_titles; session_chat_messages_route_scoped_upsert_and_delete_contract',
    ],
  ]);
  return direct.get(relative);
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
