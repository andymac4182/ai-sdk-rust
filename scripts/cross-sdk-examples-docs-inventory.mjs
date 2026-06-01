#!/usr/bin/env node
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..'
);
const outputPath = path.join(
  repositoryRoot,
  'docs/cross-sdk-examples-docs-parity.md'
);
const inventoryDate = '2026-06-01';

const githubRoot = path.join(os.homedir(), '.opensrc/repos/github.com');

const projects = [
  {
    key: 'open-agents',
    label: 'Open Agents',
    repo: 'vercel-labs/open-agents',
    fetchCommand: 'npx opensrc fetch https://github.com/vercel-labs/open-agents',
    defaultRoot: path.join(githubRoot, 'vercel-labs/open-agents/main'),
    envPath: 'OPEN_AGENTS_UPSTREAM_PATH',
    head: '24d679c7ba3d274aa73814c15673aeffcbe3c1c2',
    docRoots: ['docs', 'packages/agent/docs', 'apps/web/docs'],
    readmeRoots: ['README.md', 'apps/web/README.md'],
    exampleRoots: [],
  },
  {
    key: 'workflow',
    label: 'Workflow SDK',
    repo: 'vercel/workflow',
    fetchCommand: 'npx opensrc fetch https://github.com/vercel/workflow',
    defaultRoot: path.join(githubRoot, 'vercel/workflow/main'),
    envPath: 'WORKFLOW_UPSTREAM_PATH',
    head: 'ae3c833acd4f44ab84db65b44eb2ba2646eaecf9',
    docRoots: ['docs/content/docs'],
    readmeRoots: [
      'README.md',
      'docs/README.md',
      'packages/*/README.md',
      'workbench/README.md',
      'workbench/*/README.md',
    ],
    exampleRoots: ['workbench/*', 'packages/swc-plugin-workflow/examples/*'],
  },
  {
    key: 'chat',
    label: 'Chat SDK',
    repo: 'vercel/chat',
    fetchCommand: 'npx opensrc fetch github:vercel/chat',
    defaultRoot: path.join(githubRoot, 'vercel/chat/main'),
    envPath: 'CHAT_UPSTREAM_PATH',
    head: 'ffc43fcf1f7679164be0806308bea237113c7590',
    docRoots: ['apps/docs/content/docs'],
    readmeRoots: ['README.md', 'packages/*/README.md', 'examples/*/README.md'],
    exampleRoots: ['examples/*'],
  },
  {
    key: 'ai',
    label: 'AI SDK',
    repo: 'vercel/ai',
    fetchCommand: 'npx opensrc fetch github:vercel/ai',
    defaultRoot: path.join(githubRoot, 'vercel/ai/main'),
    envPath: 'AI_SDK_UPSTREAM_PATH',
    head: 'ab6d66482d31afe15f4973a51c5f7cfa09c92ea6',
    docRoots: ['content/docs', 'architecture', 'contributing', 'skills'],
    readmeRoots: [
      'README.md',
      '.changeset/README.md',
      'packages/*/README.md',
      'examples/*/README.md',
      'tools/*/README.md',
    ],
    exampleRoots: ['examples/*', 'packages/devtools/examples/*'],
  },
];

const validDispositions = new Set([
  'rust-doc-covered',
  'rust-ledger-covered',
  'package-owned-handoff',
  'js-only-documented',
  'live-provider-handoff',
  'upstream-docs-only',
]);

function usage() {
  console.log(`Usage: node scripts/cross-sdk-examples-docs-inventory.mjs [options]

Options:
  --check   Fail if docs/cross-sdk-examples-docs-parity.md is stale.
  --write   Write docs/cross-sdk-examples-docs-parity.md.
  --help    Show this help text.`);
}

function parseArgs(argv) {
  const options = { check: false, write: false };
  for (const arg of argv) {
    if (arg === '--help') {
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
    throw new Error(`unknown option: ${arg}`);
  }
  if (!options.check && !options.write) {
    options.check = true;
  }
  return options;
}

function assertProjectRoot(project) {
  const root = process.env[project.envPath] ?? project.defaultRoot;
  if (!fs.existsSync(root)) {
    throw new Error(
      `${project.label} upstream mirror is missing at ${root}. Run: ${project.fetchCommand}`
    );
  }
  return root;
}

function listFiles(root) {
  const files = [];
  const entries = fs.readdirSync(root, { withFileTypes: true });
  for (const entry of entries) {
    const entryPath = path.join(root, entry.name);
    if (entry.isDirectory()) {
      files.push(...listFiles(entryPath));
    } else if (entry.isFile() || entry.isSymbolicLink()) {
      files.push(entryPath);
    }
  }
  return files;
}

function relativeToRoot(root, filePath) {
  return path.relative(root, filePath).split(path.sep).join('/');
}

function isMarkdownDoc(filePath) {
  return /\.(?:md|mdx)$/.test(filePath);
}

function globToRegExp(pattern) {
  const escaped = pattern
    .split('*')
    .map((part) => part.replace(/[\\^$+?.()|[\]{}]/g, '\\$&'))
    .join('[^/]+');
  return new RegExp(`^${escaped}$`);
}

function matchesAnyGlob(relativePath, patterns) {
  return patterns.some((pattern) => globToRegExp(pattern).test(relativePath));
}

function listMarkdownUnder(root, relRoot) {
  const absoluteRoot = path.join(root, relRoot);
  if (!fs.existsSync(absoluteRoot)) {
    return [];
  }
  return listFiles(absoluteRoot)
    .map((filePath) => relativeToRoot(root, filePath))
    .filter(isMarkdownDoc);
}

function listReadmes(root, patterns) {
  return listFiles(root)
    .map((filePath) => relativeToRoot(root, filePath))
    .filter((relativePath) => relativePath.endsWith('README.md'))
    .filter((relativePath) => matchesAnyGlob(relativePath, patterns));
}

function listExampleUnits(root, patterns) {
  const units = [];
  for (const pattern of patterns) {
    if (!pattern.includes('*')) {
      if (fs.existsSync(path.join(root, pattern))) {
        units.push(pattern);
      }
      continue;
    }
    const parent = pattern.slice(0, pattern.indexOf('*')).replace(/\/$/, '');
    const parentPath = path.join(root, parent);
    if (!fs.existsSync(parentPath)) {
      continue;
    }
    for (const entry of fs.readdirSync(parentPath, { withFileTypes: true })) {
      if (entry.name.startsWith('.')) {
        continue;
      }
      const unit = `${parent}/${entry.name}`;
      if (globToRegExp(pattern).test(unit)) {
        units.push(unit);
      }
    }
  }
  return units.sort();
}

function titleFromPath(relativePath) {
  const basename = path.basename(relativePath, path.extname(relativePath));
  const parent = path.basename(path.dirname(relativePath));
  const raw = basename.toLowerCase() === 'index' ? parent : basename;
  return raw
    .replace(/^\d+-/, '')
    .replaceAll('-', ' ')
    .replaceAll('_', ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

function providerBucketForAiPath(relativePath) {
  const providerBuckets = [
    ['anthropic', 'AI-01'],
    ['amazon-bedrock', 'AI-01'],
    ['google-vertex', 'AI-01'],
    ['google', 'AI-01'],
    ['xai', 'AI-02'],
    ['groq', 'AI-02'],
    ['cohere', 'AI-02'],
    ['fireworks', 'AI-02'],
    ['togetherai', 'AI-02'],
    ['fal', 'AI-03'],
    ['klingai', 'AI-03'],
    ['prodia', 'AI-03'],
    ['replicate', 'AI-03'],
    ['luma', 'AI-03'],
    ['black-forest-labs', 'AI-03'],
    ['elevenlabs', 'AI-04'],
    ['gladia', 'AI-04'],
    ['deepgram', 'AI-04'],
    ['hume', 'AI-04'],
    ['lmnt', 'AI-04'],
    ['revai', 'AI-04'],
    ['assemblyai', 'AI-04'],
    ['voyage', 'AI-04'],
    ['azure', 'AI-05'],
    ['baseten', 'AI-05'],
    ['bytedance', 'AI-05'],
    ['cerebras', 'AI-05'],
    ['deepinfra', 'AI-05'],
    ['huggingface', 'AI-05'],
    ['moonshotai', 'AI-05'],
    ['vercel', 'AI-05'],
    ['alibaba', 'AI-07'],
  ];

  for (const [slug, row] of providerBuckets) {
    if (
      relativePath.includes(`/${slug}/`) ||
      relativePath.includes(`-${slug}-`) ||
      relativePath.includes(`packages/${slug}/`) ||
      relativePath.includes(`examples/${slug}`) ||
      relativePath.endsWith(`/${slug}.mdx`)
    ) {
      return row;
    }
  }
  return undefined;
}

function classifyOpenAgents(unit) {
  const { path: relativePath } = unit;
  if (relativePath.startsWith('apps/web/') || relativePath.includes('react')) {
    return {
      disposition: 'js-only-documented',
      evidence: 'n/a',
      notes: 'Upstream web app and React design docs are TypeScript UI surfaces.',
    };
  }
  if (relativePath.includes('approval-system')) {
    return {
      disposition: 'package-owned-handoff',
      evidence: 'OA-02, docs/open-agents/slack-remote-agent-architecture.md',
      notes: 'Portable approval behavior belongs with the Open Agents runtime closure bucket.',
    };
  }
  if (
    relativePath.includes('/plans/') ||
    relativePath.includes('lessons-learned') ||
    relativePath.includes('code-style') ||
    relativePath.includes('release')
  ) {
    return {
      disposition: 'upstream-docs-only',
      evidence: 'n/a',
      notes: 'Planning, process, or release guidance; no Rust public API contract.',
    };
  }
  return {
    disposition: 'rust-doc-covered',
    evidence:
      'docs/open-agents/bucket-ownership.md, docs/open-agents/slack-remote-agent-architecture.md, docs/open-agents/deployment-verification.md',
    notes: 'Rust docs record the service/runtime boundary; OA-01 owns test inventory details.',
  };
}

function classifyWorkflow(unit) {
  const { path: relativePath } = unit;
  const pathSegments = relativePath.split('/');
  const frameworkSegments = new Set([
    'astro',
    'express',
    'fastify',
    'hono',
    'nest',
    'nestjs',
    'next',
    'nextjs-turbopack',
    'nextjs-webpack',
    'nitro',
    'nitro-v2',
    'nitro-v3',
    'nuxt',
    'python',
    'rollup',
    'sveltekit',
    'swc-playground',
    'swc-plugin-workflow',
    'tanstack-start',
    'typescript-plugin',
    'vite',
    'vitest',
    'web',
    'web-shared',
  ]);
  if (
    relativePath.includes('/getting-started/') ||
    relativePath.includes('/workflow-next/') ||
    relativePath.startsWith('workbench/') ||
    pathSegments.some((segment) => frameworkSegments.has(segment))
  ) {
    return {
      disposition: 'js-only-documented',
      evidence: 'n/a',
      notes: 'Framework, compiler-plugin, or workbench app surface specific to the JS SDK.',
    };
  }
  if (
    relativePath.includes('/changelog/') ||
    relativePath.includes('/migration-guides/') ||
    relativePath.includes('/internal/') ||
    relativePath === 'docs/README.md'
  ) {
    return {
      disposition: 'upstream-docs-only',
      evidence: 'n/a',
      notes: 'Historical or docs-site guidance; not a standalone Rust behavior contract.',
    };
  }
  return {
    disposition: 'package-owned-handoff',
    evidence: 'WF-01, WF-02, docs/workflow-upstream-parity.md',
    notes: 'Portable workflow API behavior is owned by the Workflow SDK parity buckets.',
  };
}

function classifyChat(unit) {
  const { path: relativePath } = unit;
  if (
    relativePath.startsWith('examples/') ||
    relativePath.includes('/contributing/') ||
    relativePath.includes('packages/integration-tests')
  ) {
    return {
      disposition: 'js-only-documented',
      evidence: 'docs/chat/upstream-parity.md, docs/chat/unported.md',
      notes: 'Upstream examples/docs are Next.js, JSX, Vitest, or live-service surfaces.',
    };
  }
  if (relativePath.includes('adapter-web')) {
    return {
      disposition: 'js-only-documented',
      evidence: 'docs/chat/unported.md',
      notes: 'Browser web adapter UI is JavaScript-only.',
    };
  }
  return {
    disposition: 'rust-ledger-covered',
    evidence: 'docs/chat/upstream-parity.md',
    notes: 'Chat package/adapters are tracked by the Chat SDK parity ledger; CHAT-01 owns drift.',
  };
}

function classifyAi(unit) {
  const { path: relativePath } = unit;
  const providerBucket = providerBucketForAiPath(relativePath);
  if (providerBucket) {
    return {
      disposition: 'package-owned-handoff',
      evidence: `${providerBucket}, docs/upstream-parity.md`,
      notes: 'Provider-specific docs/examples stay with the provider implementation bucket.',
    };
  }
  if (relativePath.startsWith('architecture/')) {
    return {
      disposition: 'rust-ledger-covered',
      evidence: 'docs/upstream-parity.md',
      notes:
        'Architecture notes describe portable provider/root semantics already tracked by package ledger rows.',
    };
  }
  if (
    relativePath.startsWith('contributing/') ||
    relativePath.startsWith('skills/') ||
    relativePath.startsWith('tools/') ||
    relativePath.startsWith('.changeset/')
  ) {
    return {
      disposition: 'upstream-docs-only',
      evidence: 'n/a',
      notes:
        'Repository process, contributor workflow, package-publishing, skill, or tooling guidance; no standalone Rust public API contract.',
    };
  }
  if (relativePath.startsWith('examples/ai-e2e-next')) {
    return {
      disposition: 'js-only-documented',
      evidence: 'docs/upstream-parity.md',
      notes:
        'Next.js, React/RSC, browser UI, and live-provider demo app shell; portable provider, agent, MCP, and stream behavior is tracked by package ledger rows.',
    };
  }
  if (relativePath.startsWith('examples/ai-functions')) {
    return {
      disposition: 'rust-ledger-covered',
      evidence:
        'docs/upstream-parity.md, examples/kitchen_sink.rs, examples/vercel_ai_gateway_text.rs',
      notes:
        'Function script and live e2e provider smoke surfaces are covered by root/provider package rows and ignored live proofs.',
    };
  }
  if (relativePath.startsWith('examples/mcp')) {
    return {
      disposition: 'rust-ledger-covered',
      evidence: 'docs/upstream-parity.md, crates/ai-sdk-mcp/examples',
      notes:
        'Portable MCP client/server examples are covered by package-owned MCP examples; hosted service demos remain live-provider guidance.',
    };
  }
  if (
    relativePath.startsWith('examples/express') ||
    relativePath.startsWith('examples/fastify') ||
    relativePath.startsWith('examples/hono') ||
    relativePath.startsWith('examples/node-http-server') ||
    relativePath.startsWith('examples/next-fastapi')
  ) {
    return {
      disposition: 'rust-doc-covered',
      evidence: 'examples/http_ui_message_server.rs, src/ui_message_stream.rs, src/stream_text.rs',
      notes:
        'Framework-neutral Rust server example covers streamText UI-message SSE responses, text-stream responses, custom data parts, and pipe-to-response helpers; JS/Python framework shells are not ported literally.',
    };
  }
  if (
    /(?:ai-sdk-ui|ai-sdk-rsc|react|rsc|svelte|vue|angular|nextjs?|nuxt|expo|tanstack|browser|client-components|server-actions|jsx)/.test(
      relativePath
    ) ||
    relativePath.startsWith('examples/angular') ||
    relativePath.startsWith('examples/next') ||
    relativePath.startsWith('examples/nuxt') ||
    relativePath.startsWith('examples/sveltekit')
  ) {
    return {
      disposition: 'js-only-documented',
      evidence: 'n/a',
      notes: 'Framework, hook, RSC, browser, or TypeScript authoring surface.',
    };
  }
  if (
    relativePath.includes('/troubleshooting/') ||
    relativePath.includes('/vercel-deployment-guide') ||
    relativePath.includes('packages/devtools')
  ) {
    return {
      disposition: 'live-provider-handoff',
      evidence: 'AI-06, CROSS-02, docs/upstream-parity.md',
      notes: 'Live-service, devtools, or deployment behavior needs ignored/live proof or package-owned tests.',
    };
  }
  if (relativePath.includes('/migration-guides/')) {
    return {
      disposition: 'upstream-docs-only',
      evidence: 'n/a',
      notes: 'Migration or overview content; no new Rust behavior beyond package rows.',
    };
  }
  return {
    disposition: 'package-owned-handoff',
    evidence: 'AI-06, docs/upstream-parity.md',
    notes: 'Root AI SDK examples/docs parity is owned by the public API and examples bucket.',
  };
}

function classify(project, unit) {
  const classifiers = {
    'open-agents': classifyOpenAgents,
    workflow: classifyWorkflow,
    chat: classifyChat,
    ai: classifyAi,
  };
  const classification = classifiers[project.key](unit);
  if (!validDispositions.has(classification.disposition)) {
    throw new Error(
      `invalid disposition ${classification.disposition} for ${project.key}:${unit.path}`
    );
  }
  return classification;
}

function collectUnits(project) {
  const root = assertProjectRoot(project);
  const units = [];
  const seen = new Set();

  function push(kind, relativePath) {
    const key = `${kind}:${relativePath}`;
    if (seen.has(key)) {
      return;
    }
    seen.add(key);
    units.push({
      project: project.label,
      projectKey: project.key,
      kind,
      path: relativePath,
      surface: titleFromPath(relativePath),
    });
  }

  for (const docRoot of project.docRoots) {
    for (const relativePath of listMarkdownUnder(root, docRoot)) {
      push('doc', relativePath);
    }
  }
  for (const relativePath of listReadmes(root, project.readmeRoots)) {
    push('doc', relativePath);
  }
  for (const relativePath of listExampleUnits(root, project.exampleRoots)) {
    push('example', relativePath);
  }

  return units
    .map((unit) => ({
      ...unit,
      ...classify(project, unit),
    }))
    .sort((left, right) =>
      `${left.projectKey}:${left.kind}:${left.path}`.localeCompare(
        `${right.projectKey}:${right.kind}:${right.path}`
      )
    );
}

function escapeMarkdownCell(value) {
  return String(value)
    .replaceAll('\\', '\\\\')
    .replaceAll('|', '\\|')
    .replace(/\r?\n/g, '<br>');
}

function table(headers, rows) {
  const lines = [
    `| ${headers.map(escapeMarkdownCell).join(' | ')} |`,
    `| ${headers.map(() => '---').join(' | ')} |`,
  ];
  for (const row of rows) {
    lines.push(`| ${row.map(escapeMarkdownCell).join(' | ')} |`);
  }
  return lines.join('\n');
}

function countBy(units, keyFn) {
  const counts = new Map();
  for (const unit of units) {
    const key = keyFn(unit);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return [...counts.entries()].sort((left, right) =>
    String(left[0]).localeCompare(String(right[0]))
  );
}

function renderInventory(allUnits) {
  const byProject = new Map(projects.map((project) => [project.label, []]));
  for (const unit of allUnits) {
    byProject.get(unit.project).push(unit);
  }

  const snapshotRows = projects.map((project) => {
    const rows = byProject.get(project.label);
    return [
      project.label,
      project.repo,
      `\`${project.head}\``,
      `\`${project.defaultRoot}\``,
      String(rows.filter((row) => row.kind === 'doc').length),
      String(rows.filter((row) => row.kind === 'example').length),
      project.fetchCommand,
    ];
  });

  const dispositionRows = countBy(allUnits, (unit) => unit.disposition).map(
    ([disposition, count]) => [disposition, String(count)]
  );

  const projectRows = [...byProject.entries()].map(([project, rows]) => [
    project,
    String(rows.length),
    String(rows.filter((row) => row.kind === 'doc').length),
    String(rows.filter((row) => row.kind === 'example').length),
    ...[...validDispositions].map((disposition) =>
      String(rows.filter((row) => row.disposition === disposition).length)
    ),
  ]);

  const handoffRows = [
    [
      'OA-01/OA-02',
      'Open Agents inventory and runtime approvals',
      'Open Agents architecture and approval docs that are portable but owned by Open Agents buckets.',
    ],
    [
      'WF-01/WF-02',
      'Workflow SDK runtime and drift',
      'Workflow API, cookbook, and serde/world docs/examples remain in the Workflow parity ledgers.',
    ],
    [
      'CHAT-01',
      'Chat SDK current-upstream drift',
      'Chat package docs are already ledger-covered; drift audit owns any current-upstream changes.',
    ],
    [
      'AI-01..AI-07',
      'AI SDK provider packages',
      'Provider-specific README/docs/examples stay with their package-owned implementation buckets.',
    ],
    [
      'AI-06',
      'AI SDK public API and examples',
      'Root ai ergonomics, generate/stream/embed/object docs, and framework-neutral examples stay there.',
    ],
    [
      'CROSS-02',
      'Live integration proof registry',
      'Live/provider/deployment examples need ignored proof commands and env-var registry entries.',
    ],
  ];

  const inventoryRows = allUnits.map((unit) => [
    unit.project,
    unit.kind,
    `\`${unit.path}\``,
    unit.surface,
    unit.disposition,
    unit.evidence,
    unit.notes,
  ]);

  return `# Cross-SDK Examples And Docs Parity

Generated by \`node scripts/cross-sdk-examples-docs-inventory.mjs --write\`.

This inventory owns \`CROSS-01\` from
[\`docs/ts-to-rust-migration-tracker.md\`](ts-to-rust-migration-tracker.md).
It covers upstream documentation pages, README files, and example units that can
imply public behavior across Open Agents, Workflow SDK, Chat SDK, and AI SDK.

## Scope

- Docs rows include Markdown/MDX files under the public docs roots plus root,
  package, workbench, and example README files named in the checker.
- Example rows are executable example units, not every implementation file inside
  those apps. The owning row records whether the example is a Rust parity target,
  a package-owned handoff, a live-provider handoff, or an explicit JS-only
  exclusion.
- Provider implementation behavior remains with provider/package buckets; this
  row only records the user-facing docs/examples obligation so it is not lost.
- Framework, browser, React/RSC, Next.js, Vitest, TypeScript-plugin, and docs-site
  rendering surfaces are documented as JS-only rather than ported literally.

## Source Snapshot

${table(
    [
      'Project',
      'Repository',
      'Upstream HEAD',
      'Local mirror',
      'Doc rows',
      'Example rows',
      'Refresh command',
    ],
    snapshotRows
  )}

## Disposition Legend

| Disposition | Meaning |
| --- | --- |
| \`rust-doc-covered\` | Existing Rust docs/examples directly cover the public behavior at this cross-SDK level. |
| \`rust-ledger-covered\` | The package parity ledger already owns the docs/API behavior and no cross-SDK duplicate is needed. |
| \`package-owned-handoff\` | Portable behavior exists, but it belongs to an active package/provider/runtime bucket. |
| \`js-only-documented\` | The upstream surface is JavaScript, TypeScript, browser, framework, React/RSC, Vitest, or docs-site specific. |
| \`live-provider-handoff\` | The surface is live-provider, devtools, deployment, or credential-gated behavior that needs ignored/live proof. |
| \`upstream-docs-only\` | Historical, planning, release, migration, or contributing content that does not define Rust public behavior. |

## Summary By Disposition

${table(['Disposition', 'Rows'], dispositionRows)}

## Summary By Project

${table(
    [
      'Project',
      'Rows',
      'Docs',
      'Examples',
      ...[...validDispositions],
    ],
    projectRows
  )}

## Remaining Package-Owned Handoffs

${table(['Owner', 'Surface', 'Reason'], handoffRows)}

## Inventory Rows

${table(
    [
      'Project',
      'Kind',
      'Upstream unit',
      'Surface',
      'Disposition',
      'Rust evidence or handoff',
      'Notes',
    ],
    inventoryRows
  )}
`;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const allUnits = projects.flatMap(collectUnits);
  const content = renderInventory(allUnits);

  if (options.write) {
    fs.writeFileSync(outputPath, content);
  }

  if (options.check) {
    if (!fs.existsSync(outputPath)) {
      throw new Error(`${path.relative(repositoryRoot, outputPath)} is missing`);
    }
    const current = fs.readFileSync(outputPath, 'utf8');
    if (current !== content) {
      throw new Error(
        `${path.relative(
          repositoryRoot,
          outputPath
        )} is stale; run node scripts/cross-sdk-examples-docs-inventory.mjs --write`
      );
    }
  }

  console.log(
    `cross-sdk docs/examples inventory: ${allUnits.length} rows across ${projects.length} projects`
  );
}

main();
