#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..'
);

const defaultUpstreamRoot =
  '/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/just-bash/main';
const upstreamRoot = path.resolve(
  process.env.JUST_BASH_UPSTREAM_PATH ?? defaultUpstreamRoot
);
const upstreamPackageRoot = path.join(upstreamRoot, 'packages', 'just-bash');
const upstreamComparisonRoot = path.join(
  upstreamPackageRoot,
  'src',
  'comparison-tests'
);
const upstreamFixtureRoot = path.join(upstreamComparisonRoot, 'fixtures');
const legacyGeneratedRoot = path.join(
  upstreamPackageRoot,
  '.ai-sdk-rust-conformance'
);
const generatedBaseRoot = path.join(
  upstreamPackageRoot,
  'node_modules',
  '.cache',
  'ai-sdk-rust-conformance'
);
let generatedRoot = generatedBaseRoot;
let generatedComparisonRoot = path.join(generatedRoot, 'comparison-tests');
const rustRunnerFixturePath = path.join(
  repositoryRoot,
  'crates',
  'just-bash',
  'tests',
  'fixtures',
  'just-bash-conformance.json'
);

const upstreamHead = 'd64009aef6bc1556e7c84b22ed455863275ea953';
const pnpmVersion = '10.33.2';
const validEngines = new Set(['typescript', 'rust']);

const defaultRustAddonPath = path.join(
  repositoryRoot,
  'crates',
  'just-bash-napi',
  `just-bash-napi.${process.platform}-${process.arch}.node`
);

const rustAddonCandidates = [
  defaultRustAddonPath,
  path.join(repositoryRoot, 'crates', 'just-bash-napi', 'index.js'),
  path.join(repositoryRoot, 'crates', 'just-bash-napi', 'index.cjs'),
  path.join(repositoryRoot, 'crates', 'just-bash-napi', 'index.mjs'),
  path.join(repositoryRoot, 'crates', 'just-bash-napi', 'just-bash-napi.node'),
  path.join(repositoryRoot, 'crates', 'just-bash-napi', 'just_bash_napi.node'),
  path.join(repositoryRoot, 'target', 'debug', 'just_bash_napi.node'),
  path.join(repositoryRoot, 'target', 'debug', 'libjust_bash_napi.node'),
];

function printHelp() {
  console.log(`Just Bash dual-engine conformance harness

Usage:
  node scripts/just-bash-conformance.mjs [options] [-- vitest args]

Options:
  --engine <typescript|rust>   Select engine. Defaults to JUST_BASH_ENGINE or typescript.
  --domain <name>             Select one upstream comparison domain. Repeatable.
  --domains <a,b>             Select comma-separated upstream comparison domains.
  --all                       Run all upstream comparison domains.
  --list-domains              Print available upstream comparison domains and exit.
  --record                    Set RECORD_FIXTURES=1 for this run.
  --record-force              Set RECORD_FIXTURES=force for this run.
  --playback                  Clear RECORD_FIXTURES for this run.
  --verified-corpus           Run the generated verified corpus instead of raw domains.
  --help                      Print this help.

Environment:
  JUST_BASH_UPSTREAM_PATH     Refreshed vercel-labs/just-bash mirror path.
  JUST_BASH_ENGINE            typescript or rust.
  JUST_BASH_DOMAINS           Comma-separated domains, for example echo,cat.
  JUST_BASH_RUST_ADDON        Path to JBC-01 napi-rs adapter JS or .node entry.
  JUST_BASH_RUST_STUB         Set to fixtures to use fixture-backed Rust stub mode.
  JUST_BASH_REQUIRE_RUST_ADDON=1
                              Treat missing Rust addon as an error instead of a skip.
  JUST_BASH_VERIFIED_CORPUS=1 Run the generated verified corpus for either engine.
  JUST_BASH_AUTO_INSTALL=1    Run pnpm install in the upstream mirror if deps are missing.

By default, the TypeScript engine runs all upstream comparison domains. The Rust
engine runs the generated verified comparison corpus; use --verified-corpus to
run the exact same generated corpus against either engine. Use --all or --domain
to run raw upstream comparison files, including pending command-family failures.
`);
}

function parseArgs(argv) {
  const domains = [];
  const vitestArgs = [];
  let engine = process.env.JUST_BASH_ENGINE ?? 'typescript';
  let recordMode = process.env.RECORD_FIXTURES;
  let listDomains = false;
  let runAll = false;
  let parsingVitestArgs = false;
  let verifiedCorpus = process.env.JUST_BASH_VERIFIED_CORPUS === '1';

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (parsingVitestArgs) {
      vitestArgs.push(arg);
      continue;
    }
    if (arg === '--') {
      parsingVitestArgs = true;
      continue;
    }
    if (arg === '--help' || arg === '-h') {
      printHelp();
      process.exit(0);
    }
    if (arg === '--engine') {
      engine = requireValue(argv, (i += 1), arg);
      continue;
    }
    if (arg.startsWith('--engine=')) {
      engine = arg.slice('--engine='.length);
      continue;
    }
    if (arg === '--domain') {
      domains.push(requireValue(argv, (i += 1), arg));
      continue;
    }
    if (arg.startsWith('--domain=')) {
      domains.push(arg.slice('--domain='.length));
      continue;
    }
    if (arg === '--domains') {
      domains.push(...splitDomains(requireValue(argv, (i += 1), arg)));
      continue;
    }
    if (arg.startsWith('--domains=')) {
      domains.push(...splitDomains(arg.slice('--domains='.length)));
      continue;
    }
    if (arg === '--all') {
      runAll = true;
      continue;
    }
    if (arg === '--list-domains') {
      listDomains = true;
      continue;
    }
    if (arg === '--record') {
      recordMode = '1';
      continue;
    }
    if (arg === '--record-force') {
      recordMode = 'force';
      continue;
    }
    if (arg === '--playback') {
      recordMode = '';
      continue;
    }
    if (arg === '--verified-corpus') {
      verifiedCorpus = true;
      continue;
    }
    throw new Error(`Unknown argument: ${arg}`);
  }

  if (!runAll && domains.length === 0 && process.env.JUST_BASH_DOMAINS) {
    domains.push(...splitDomains(process.env.JUST_BASH_DOMAINS));
  }

  return {
    domains,
    engine,
    listDomains,
    recordMode,
    runAll,
    verifiedCorpus,
    vitestArgs,
  };
}

function requireValue(argv, index, flag) {
  const value = argv[index];
  if (!value || value.startsWith('--')) {
    throw new Error(`${flag} requires a value`);
  }
  return value;
}

function splitDomains(value) {
  return value
    .split(',')
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function ensureUpstreamMirror() {
  for (const requiredPath of [
    path.join(upstreamRoot, 'package.json'),
    path.join(upstreamRoot, 'pnpm-lock.yaml'),
    path.join(upstreamPackageRoot, 'package.json'),
    upstreamComparisonRoot,
    upstreamFixtureRoot,
  ]) {
    if (!fs.existsSync(requiredPath)) {
      throw new Error(
        `Missing upstream Just Bash path: ${requiredPath}\n` +
          `Refresh it with: npx opensrc fetch https://github.com/vercel-labs/just-bash`
      );
    }
  }
}

function discoverDomains() {
  return fs
    .readdirSync(upstreamComparisonRoot)
    .filter((entry) => entry.endsWith('.comparison.test.ts'))
    .map((fileName) => ({
      domain: fileName.slice(0, -'.comparison.test.ts'.length),
      fileName,
      sourcePath: path.join(upstreamComparisonRoot, fileName),
    }))
    .sort((a, b) => a.domain.localeCompare(b.domain));
}

function selectDomains(allDomains, requestedDomains, runAll) {
  if (runAll || requestedDomains.length === 0) {
    return allDomains;
  }

  const byDomain = new Map(allDomains.map((entry) => [entry.domain, entry]));
  const selected = [];
  for (const domain of requestedDomains) {
    const entry = byDomain.get(domain);
    if (!entry) {
      throw new Error(
        `Unknown Just Bash comparison domain: ${domain}\n` +
          `Run node scripts/just-bash-conformance.mjs --list-domains to see valid domains.`
      );
    }
    selected.push(entry);
  }
  return selected;
}

function ensureUpstreamDependencies() {
  if (
    fs.existsSync(path.join(upstreamRoot, 'node_modules')) &&
    fs.existsSync(path.join(upstreamPackageRoot, 'node_modules', '.bin', 'vitest'))
  ) {
    return;
  }

  const installArgs = [
    '--yes',
    `pnpm@${pnpmVersion}`,
    '--dir',
    upstreamRoot,
    'install',
    '--frozen-lockfile',
  ];
  if (process.env.JUST_BASH_AUTO_INSTALL === '1') {
    console.log(
      `[just-bash-conformance] Installing upstream dependencies with npx ${installArgs.join(
        ' '
      )}`
    );
    const install = spawnSync('npx', installArgs, {
      cwd: upstreamRoot,
      stdio: 'inherit',
    });
    if (install.status !== 0) {
      process.exit(install.status ?? 1);
    }
    return;
  }

  throw new Error(
    `Upstream Just Bash dependencies are not installed at ${upstreamRoot}, or package-level vitest is missing.\n` +
      `Run: npx --yes pnpm@${pnpmVersion} --dir ${upstreamRoot} install --frozen-lockfile\n` +
      `Or set JUST_BASH_AUTO_INSTALL=1 for the harness to run that command.`
  );
}

function configureGeneratedRoot(engine) {
  const runId = process.env.JUST_BASH_CONFORMANCE_RUN_ID;
  const suffix = sanitizeGeneratedRootPart(
    runId && runId.trim() ? `${engine}-${runId}` : `${engine}-${process.pid}`
  );
  generatedRoot = path.join(generatedBaseRoot, suffix);
  generatedComparisonRoot = path.join(generatedRoot, 'comparison-tests');
}

function sanitizeGeneratedRootPart(value) {
  return value.replace(/[^A-Za-z0-9._-]/g, '-');
}

function resolveRustAddonPath() {
  if (process.env.JUST_BASH_RUST_ADDON) {
    const requestedPath = path.resolve(process.env.JUST_BASH_RUST_ADDON);
    return isUsableRustAddonCandidate(requestedPath) ? requestedPath : null;
  }
  return rustAddonCandidates.find((candidate) => isUsableRustAddonCandidate(candidate)) ?? null;
}

function isUsableRustAddonCandidate(candidate) {
  if (!fs.existsSync(candidate)) {
    return false;
  }

  const packageRoot = path.join(repositoryRoot, 'crates', 'just-bash-napi');
  const isPackageWrapper =
    path.dirname(candidate) === packageRoot &&
    ['index.cjs', 'index.js', 'index.mjs'].includes(path.basename(candidate));
  return !isPackageWrapper || fs.existsSync(defaultRustAddonPath);
}

function requestedRustAddonDiagnostic() {
  if (!process.env.JUST_BASH_RUST_ADDON) {
    return '';
  }

  const requestedPath = path.resolve(process.env.JUST_BASH_RUST_ADDON);
  if (!fs.existsSync(requestedPath)) {
    return `Requested JUST_BASH_RUST_ADDON does not exist: ${requestedPath}\n`;
  }
  if (!isUsableRustAddonCandidate(requestedPath)) {
    return (
      `Requested JUST_BASH_RUST_ADDON is a package wrapper, but the native addon is not built: ${requestedPath}\n` +
      `Run: npm run build --prefix crates/just-bash-napi\n`
    );
  }
  return '';
}

function maybeSkipMissingRustAddon(engine, rustAddonPath) {
  if (engine !== 'rust' || process.env.JUST_BASH_RUST_STUB === 'fixtures') {
    return;
  }
  if (rustAddonPath) {
    return;
  }

  const buildHint = fs.existsSync(defaultRustAddonPath)
    ? ''
    : `Run: npm run build --prefix crates/just-bash-napi\n`;
  const message =
    `[just-bash-conformance] JUST_BASH_ENGINE=rust requested, but the JBC-01 napi-rs adapter is not available.\n` +
    requestedRustAddonDiagnostic() +
    buildHint +
    `Set JUST_BASH_RUST_ADDON=/absolute/path/to/adapter or run the fixture-backed stub with JUST_BASH_RUST_STUB=fixtures.\n` +
    `Checked candidate paths:\n` +
    rustAddonCandidates.map((candidate) => `  - ${candidate}`).join('\n');

  if (process.env.JUST_BASH_REQUIRE_RUST_ADDON === '1') {
    throw new Error(message);
  }
  console.log(`${message}\n[just-bash-conformance] Skipping Rust engine run.`);
  process.exit(0);
}

function prepareGeneratedSuite(selectedDomains, engine, rustAddonPath) {
  cleanGeneratedRoot();
  fs.mkdirSync(generatedComparisonRoot, { recursive: true });

  writeFixtureRunner(engine, rustAddonPath);

  const generatedTestPaths = [];
  for (const entry of selectedDomains) {
    const targetPath = path.join(generatedComparisonRoot, entry.fileName);
    let content = fs.readFileSync(entry.sourcePath, 'utf8');
    content = content.replace(
      /import \{ Bash \} from "\.\.\/Bash\.js";/g,
      'import { Bash } from "./fixture-runner.js";'
    );
    fs.writeFileSync(targetPath, content);
    generatedTestPaths.push(targetPath);
  }

  writeVitestSetup();

  return writeVitestConfig(generatedTestPaths);
}

function prepareGeneratedVerifiedCorpusSuite(engine, rustAddonPath) {
  cleanGeneratedRoot();
  fs.mkdirSync(generatedComparisonRoot, { recursive: true });

  writeFixtureRunner(engine, rustAddonPath);

  if (!fs.existsSync(rustRunnerFixturePath)) {
    throw new Error(
      `Missing Rust runner fixture: ${rustRunnerFixturePath}\n` +
        `Run: node scripts/just-bash-conformance-corpus.mjs`
    );
  }

  const rustRunnerFixture = JSON.parse(fs.readFileSync(rustRunnerFixturePath, 'utf8'));
  const generatedTestPath = path.join(generatedComparisonRoot, 'rust-corpus.test.ts');
  fs.writeFileSync(generatedTestPath, rustCorpusTestSource(rustRunnerFixture));
  writeVitestSetup();
  console.log(
    `[just-bash-conformance] verified corpus cases=${rustRunnerFixture.summary?.totalCases ?? 0}`
  );
  return writeVitestConfig([generatedTestPath]);
}

function cleanGeneratedRoot() {
  if (!generatedRoot.startsWith(generatedBaseRoot + path.sep)) {
    throw new Error(
      `Refusing to clean generated root outside conformance cache: ${generatedRoot}`
    );
  }

  if (fs.existsSync(legacyGeneratedRoot)) {
    fs.rmSync(legacyGeneratedRoot, { recursive: true, force: true });
  }
  fs.rmSync(generatedRoot, { recursive: true, force: true });
}

function writeFixtureRunner(engine, rustAddonPath) {
  fs.writeFileSync(
    path.join(generatedComparisonRoot, 'fixture-runner.ts'),
    fixtureRunnerSource({
      engine,
      fixtureRoot: upstreamFixtureRoot,
      rustAddonPath,
      typescriptBashImport: relativeImportSpecifier(
        generatedComparisonRoot,
        path.join(upstreamPackageRoot, 'src', 'Bash.js')
      ),
      upstreamHead,
    })
  );
}

function writeVitestSetup() {
  fs.writeFileSync(
    path.join(generatedComparisonRoot, 'vitest.setup.ts'),
    `import { afterAll } from "vitest";\n` +
      `import { isRecordMode, writeAllFixtures } from "./fixture-runner.js";\n\n` +
      `afterAll(async () => {\n` +
      `  if (isRecordMode) {\n` +
      `    await writeAllFixtures();\n` +
      `  }\n` +
      `});\n`
  );
}

function writeVitestConfig(generatedTestPaths) {
  const vitestConfigPath = path.join(generatedRoot, 'vitest.config.ts');
  fs.writeFileSync(
    vitestConfigPath,
    `import { defineConfig } from "vitest/config";\n\n` +
      `export default defineConfig({\n` +
      `  root: ${JSON.stringify(upstreamPackageRoot)},\n` +
      `  test: {\n` +
      `    globals: true,\n` +
      `    include: ${JSON.stringify(generatedTestPaths)},\n` +
      `    exclude: ["**/dist/**"],\n` +
      `    setupFiles: [${JSON.stringify(
        path.join(generatedComparisonRoot, 'vitest.setup.ts')
      )}],\n` +
      `    testTimeout: 30000,\n` +
      `    hookTimeout: 30000,\n` +
      `  },\n` +
      `});\n`
  );

  return vitestConfigPath;
}

function rustCorpusTestSource(rustRunnerFixture) {
  return `import { describe, expect, it } from "vitest";\n` +
    `import { Bash } from "./fixture-runner.js";\n\n` +
    `const corpus = ${JSON.stringify(rustRunnerFixture, null, 2)};\n` +
    `const defaultCwd = corpus.defaultCwd ?? "/workspace";\n\n` +
    `function normalizePath(input) {\n` +
    `  const parts = [];\n` +
    `  for (const part of input.split("/")) {\n` +
    `    if (!part || part === ".") continue;\n` +
    `    if (part === "..") parts.pop();\n` +
    `    else parts.push(part);\n` +
    `  }\n` +
    `  return \`/\${parts.join("/")}\`;\n` +
    `}\n\n` +
    `function resolvePath(cwd, input) {\n` +
    `  if (input.startsWith("/")) return normalizePath(input);\n` +
    `  return normalizePath(\`\${cwd}/\${input}\`);\n` +
    `}\n\n` +
    `function effectiveCwd(cwd) {\n` +
    `  if (cwd && cwd.trim() && cwd !== ".") {\n` +
    `    return cwd.startsWith("/") ? cwd : resolvePath(defaultCwd, cwd);\n` +
    `  }\n` +
    `  return defaultCwd;\n` +
    `}\n\n` +
    `function seedFiles(testCase) {\n` +
    `  const cwd = effectiveCwd(testCase.cwd);\n` +
    `  const files = Object.create(null);\n` +
    `  for (const [filePath, content] of Object.entries(testCase.initialFiles ?? {})) {\n` +
    `    files[filePath.startsWith("/") ? filePath : resolvePath(cwd, filePath)] = content;\n` +
    `  }\n` +
    `  return files;\n` +
    `}\n\n` +
    `function execOptions(testCase) {\n` +
    `  const options = testCase.options ?? {};\n` +
    `  const exec = Object.create(null);\n` +
    `  if (options.env) exec.env = options.env;\n` +
    `  if (options.replaceEnv !== undefined) exec.replaceEnv = options.replaceEnv;\n` +
    `  if (options.cwd) exec.cwd = options.cwd;\n` +
    `  if (options.stdin !== undefined && options.stdin !== null) exec.stdin = options.stdin;\n` +
    `  if (Array.isArray(options.args) && options.args.length > 0) exec.args = options.args;\n` +
    `  if (options.timeoutMs !== undefined) exec.timeoutMs = options.timeoutMs;\n` +
    `  return Object.keys(exec).length > 0 ? exec : undefined;\n` +
    `}\n\n` +
    `describe("Just Bash Rust verified comparison corpus", () => {\n` +
    `  for (const testCase of corpus.cases ?? []) {\n` +
    `    it(testCase.rustTestName ?? testCase.id, async () => {\n` +
    `      expect(testCase.status).toBe("portable-verified");\n` +
    `      const bash = new Bash({\n` +
    `        files: seedFiles(testCase),\n` +
    `        env: testCase.env ?? {},\n` +
    `        cwd: effectiveCwd(testCase.cwd),\n` +
    `        commands: testCase.commands,\n` +
    `      });\n` +
    `      const result = await bash.exec(testCase.command, execOptions(testCase));\n` +
    `      expect(result.stdout).toBe(testCase.expected.stdout);\n` +
    `      expect(result.stderr).toBe(testCase.expected.stderr);\n` +
    `      expect(result.exitCode).toBe(testCase.expected.exitCode);\n` +
    `    });\n` +
    `  }\n` +
    `});\n`;
}

function relativeImportSpecifier(fromDir, toPath) {
  const relativePath = path.relative(fromDir, toPath).replaceAll(path.sep, '/');
  return relativePath.startsWith('.') ? relativePath : `./${relativePath}`;
}

function fixtureRunnerSource({
  engine,
  fixtureRoot,
  rustAddonPath,
  typescriptBashImport,
  upstreamHead,
}) {
  return `import { exec } from "node:child_process";
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";
import { promisify } from "node:util";
import { Bash as TypeScriptBash } from ${JSON.stringify(typescriptBashImport)};

const execAsync = promisify(exec);
const require = createRequire(import.meta.url);
const selectedEngine = ${JSON.stringify(engine)};
const fixtureRoot = ${JSON.stringify(fixtureRoot)};
const rustAddonPath = ${JSON.stringify(rustAddonPath)};
const upstreamHead = ${JSON.stringify(upstreamHead)};

export const isRecordMode =
  process.env.RECORD_FIXTURES === "1" ||
  process.env.RECORD_FIXTURES === "force";

const isForceRecordMode = process.env.RECORD_FIXTURES === "force";

const fixturesCache = new Map();
const pendingFixtures = new Map();
const setupFilesRegistry = new Map();
const skippedLockedFixtures = [];
let rustAddonModulePromise;

function generateFixtureId(command, files) {
  const sortedFiles = Object.keys(files)
    .sort()
    .map((key) => \`\${key}:\${files[key]}\`)
    .join("|");
  const content = \`\${command}|||\${sortedFiles}\`;
  return createHash("sha256").update(content).digest("hex").slice(0, 16);
}

function getFixturesPath(testFile) {
  const base = path.basename(testFile, ".test.ts");
  return path.join(fixtureRoot, \`\${base}.fixtures.json\`);
}

async function loadFixtures(testFile) {
  const fixturesPath = getFixturesPath(testFile);
  const cached = fixturesCache.get(fixturesPath);
  if (cached) {
    return cached;
  }
  try {
    const content = await fs.readFile(fixturesPath, "utf8");
    const fixtures = JSON.parse(content);
    fixturesCache.set(fixturesPath, fixtures);
    return fixtures;
  } catch {
    const empty = Object.create(null);
    fixturesCache.set(fixturesPath, empty);
    return empty;
  }
}

async function recordFixture(testFile, fixtureId, entry) {
  if (!isForceRecordMode) {
    const existingFixtures = await loadFixtures(testFile);
    const existing = existingFixtures[fixtureId];
    if (existing?.locked) {
      skippedLockedFixtures.push({ testFile, fixtureId, command: entry.command });
      return false;
    }
  }

  const fixturesPath = getFixturesPath(testFile);
  let fixtures = pendingFixtures.get(fixturesPath);
  if (!fixtures) {
    fixtures = {};
    pendingFixtures.set(fixturesPath, fixtures);
  }
  fixtures[fixtureId] = entry;
  return true;
}

export async function writeAllFixtures() {
  for (const [fixturesPath, newFixtures] of pendingFixtures.entries()) {
    await fs.mkdir(path.dirname(fixturesPath), { recursive: true });

    let existingFixtures = Object.create(null);
    try {
      const content = await fs.readFile(fixturesPath, "utf8");
      existingFixtures = JSON.parse(content);
    } catch {
      // No existing fixture file yet.
    }

    const mergedFixtures = { ...existingFixtures };
    for (const [key, value] of Object.entries(newFixtures)) {
      const existing = existingFixtures[key];
      if (existing?.locked && !isForceRecordMode) {
        continue;
      }
      mergedFixtures[key] = value;
    }

    const sortedFixtures = Object.create(null);
    for (const key of Object.keys(mergedFixtures).sort()) {
      sortedFixtures[key] = mergedFixtures[key];
    }

    await fs.writeFile(fixturesPath, \`\${JSON.stringify(sortedFixtures, null, 2)}\\n\`);
    console.log(\`Wrote fixtures to \${fixturesPath}\`);
  }

  if (skippedLockedFixtures.length > 0) {
    console.log("\\nSkipped locked fixtures; use RECORD_FIXTURES=force to override:");
    for (const { testFile, command } of skippedLockedFixtures) {
      console.log(\`   - \${path.basename(testFile)}: "\${command}"\`);
    }
  }
}

export async function createTestDir() {
  const testDir = path.join(
    os.tmpdir(),
    \`bashenv-test-\${Date.now()}-\${Math.random().toString(36).slice(2)}\`
  );
  await fs.mkdir(testDir, { recursive: true });
  return testDir;
}

export async function cleanupTestDir(testDir) {
  setupFilesRegistry.delete(testDir);
  try {
    await fs.rm(testDir, { recursive: true, force: true });
  } catch {
    // Ignore cleanup errors.
  }
}

export async function setupFiles(testDir, files) {
  setupFilesRegistry.set(testDir, files);

  for (const [filePath, content] of Object.entries(files)) {
    const fullPath = path.join(testDir, filePath);
    await fs.mkdir(path.dirname(fullPath), { recursive: true });
    await fs.writeFile(fullPath, content);
  }

  const bashEnvFiles = Object.create(null);
  for (const [filePath, content] of Object.entries(files)) {
    bashEnvFiles[path.join(testDir, filePath)] = content;
  }

  return new Bash({
    files: bashEnvFiles,
    cwd: testDir,
  });
}

export async function runRealBash(command, cwd, env = {}) {
  try {
    const { stdout, stderr } = await execAsync(command, {
      cwd,
      env: { ...process.env, ...env },
      shell: "/bin/bash",
    });
    return { stdout, stderr, exitCode: 0 };
  } catch (error) {
    return {
      stdout: error.stdout || "",
      stderr: error.stderr || "",
      exitCode: error.code || error.status || 1,
    };
  }
}

function normalizeWhitespace(str) {
  return str
    .split("\\n")
    .map((line) => line.trim().replace(/\\s+/g, " "))
    .join("\\n");
}

function fileUrlToPath(url) {
  if (!url) return "";
  if (url.startsWith("file://")) {
    return url.slice(7);
  }
  return url;
}

function getCallingTestFile() {
  const stack = new Error().stack || "";
  const lines = stack.split("\\n");

  for (const line of lines) {
    let match = line.match(/file:\\/\\/([^):]+\\.comparison\\.test\\.ts)/);
    if (match) return match[1];
    match = line.match(/\\(([^):]+\\.comparison\\.test\\.ts)/);
    if (match) return match[1];
    match = line.match(/at\\s+([^():]+\\.comparison\\.test\\.ts)/);
    if (match) return match[1].trim();
  }

  for (const line of lines) {
    let match = line.match(/file:\\/\\/([^):]+\\.test\\.ts)/);
    if (match) return match[1];
    match = line.match(/\\(([^):]+\\.test\\.ts)/);
    if (match) return match[1];
  }

  throw new Error(\`Could not determine calling test file from stack trace:\\n\${stack}\`);
}

async function compareOutputsInternal(env, testDir, command, files, testFile, options) {
  if (typeof env.__setComparisonContext === "function") {
    env.__setComparisonContext({ files, testFile, testDir });
  }
  const bashEnvResult = normalizeExecResult(await env.exec(command));
  const fixtureId = generateFixtureId(command, files);

  let realBashStdout;
  let realBashStderr;
  let realBashExitCode;

  if (isRecordMode) {
    const existingFixtures = await loadFixtures(testFile);
    const existingFixture = existingFixtures[fixtureId];

    if (existingFixture?.locked && !isForceRecordMode) {
      realBashStdout = existingFixture.stdout;
      realBashStderr = existingFixture.stderr;
      realBashExitCode = existingFixture.exitCode;
      skippedLockedFixtures.push({ testFile, fixtureId, command });
    } else {
      const realBashResult = await runRealBash(command, testDir);
      realBashStdout = realBashResult.stdout;
      realBashStderr = realBashResult.stderr;
      realBashExitCode = realBashResult.exitCode;

      await recordFixture(testFile, fixtureId, {
        command,
        files,
        stdout: realBashStdout,
        stderr: realBashStderr,
        exitCode: realBashExitCode,
      });
    }
  } else {
    const fixtures = await loadFixtures(testFile);
    const fixture = fixtures[fixtureId];

    if (!fixture) {
      throw new Error(
        \`No fixture found for command "\${command}" with files \${JSON.stringify(files)}.\\n\` +
          \`Fixture ID: \${fixtureId}\\n\` +
          "Run with RECORD_FIXTURES=1 to record fixtures."
      );
    }

    realBashStdout = fixture.stdout;
    realBashStderr = fixture.stderr;
    realBashExitCode = fixture.exitCode;
  }

  let bashEnvStdout = bashEnvResult.stdout;
  let expectedStdout = realBashStdout;

  if (options?.normalizeWhitespace) {
    bashEnvStdout = normalizeWhitespace(bashEnvStdout);
    expectedStdout = normalizeWhitespace(expectedStdout);
  }

  if (bashEnvStdout !== expectedStdout) {
    throw new Error(
      \`stdout mismatch for "\${command}" using \${selectedEngine} engine\\n\` +
        \`Expected (recorded bash): \${JSON.stringify(realBashStdout)}\\n\` +
        \`Received (BashEnv):       \${JSON.stringify(bashEnvResult.stdout)}\`
    );
  }

  if (options?.compareStderr) {
    if (bashEnvResult.stderr !== realBashStderr) {
      throw new Error(
        \`stderr mismatch for "\${command}" using \${selectedEngine} engine\\n\` +
          \`Expected (recorded bash): \${JSON.stringify(realBashStderr)}\\n\` +
          \`Received (BashEnv):       \${JSON.stringify(bashEnvResult.stderr)}\`
      );
    }
  }

  if (options?.compareExitCode !== false && bashEnvResult.exitCode !== realBashExitCode) {
    throw new Error(
      \`exitCode mismatch for "\${command}" using \${selectedEngine} engine\\n\` +
        \`Expected (recorded bash): \${realBashExitCode}\\n\` +
        \`Received (BashEnv):       \${bashEnvResult.exitCode}\`
    );
  }
}

export async function compareOutputs(
  env,
  testDir,
  command,
  options,
  files,
  testFileUrl
) {
  const testFile = testFileUrl ? fileUrlToPath(testFileUrl) : getCallingTestFile();
  const testFiles = files || setupFilesRegistry.get(testDir) || Object.create(null);
  return compareOutputsInternal(env, testDir, command, testFiles, testFile, options);
}

export class Bash {
  constructor(options = {}) {
    this.options = options;
    this.context = undefined;
    this.sessionPromise = createEngineSession(options);
  }

  __setComparisonContext(context) {
    this.context = context;
  }

  async exec(command, options) {
    const session = await this.sessionPromise;
    if (typeof session.__setComparisonContext === "function") {
      session.__setComparisonContext(this.context);
    }
    return normalizeExecResult(await session.exec(command, options));
  }

  async readFile(filePath) {
    return this.delegate("readFile", filePath);
  }

  async writeFile(filePath, content) {
    return this.delegate("writeFile", filePath, content);
  }

  async fileExists(filePath) {
    return this.delegate("fileExists", filePath);
  }

  async getCwd() {
    return this.delegate("getCwd");
  }

  async getEnv() {
    return this.delegate("getEnv");
  }

  async registeredCommandNames() {
    return this.delegate("registeredCommandNames");
  }

  async delegate(methodName, ...args) {
    const session = await this.sessionPromise;
    if (typeof session[methodName] !== "function") {
      throw new Error(\`Engine session does not support \${methodName}\`);
    }
    return session[methodName](...args);
  }
}

async function createEngineSession(options) {
  if (selectedEngine === "typescript") {
    return new TypeScriptBash(options);
  }

  if (selectedEngine === "rust" && process.env.JUST_BASH_RUST_STUB === "fixtures") {
    return new FixtureBackedRustStub(options);
  }

  if (selectedEngine === "rust") {
    const addon = await loadRustAddon();
    return instantiateRustAddon(addon, options);
  }

  throw new Error(\`Unsupported JUST_BASH_ENGINE=\${selectedEngine}\`);
}

async function loadRustAddon() {
  if (!rustAddonPath) {
    throw new Error(
      "JUST_BASH_ENGINE=rust requires JUST_BASH_RUST_ADDON unless " +
        "JUST_BASH_RUST_STUB=fixtures is set."
    );
  }
  if (!rustAddonModulePromise) {
    rustAddonModulePromise = rustAddonPath.endsWith(".node")
      ? Promise.resolve(require(rustAddonPath))
      : import(pathToFileUrl(rustAddonPath));
  }
  return rustAddonModulePromise;
}

function pathToFileUrl(filePath) {
  let resolved = path.resolve(filePath).replace(/\\\\/g, "/");
  if (!resolved.startsWith("/")) {
    resolved = \`/\${resolved}\`;
  }
  return encodeURI(\`file://\${resolved}\`);
}

function instantiateRustAddon(addon, options) {
  if (typeof addon.createBash === "function") {
    return addon.createBash(options);
  }
  if (typeof addon.createJustBash === "function") {
    return addon.createJustBash(options);
  }
  const Constructor =
    addon.Bash ??
    addon.RustBash ??
    addon.JustBash ??
    addon.JustBashSession ??
    addon.default;
  if (typeof Constructor === "function") {
    return new Constructor(options);
  }
  throw new Error(
    "Rust addon must export createBash, createJustBash, Bash, JustBash, " +
      "JustBashSession, or a default constructor."
  );
}

class FixtureBackedRustStub {
  constructor(options = {}) {
    this.cwd = options.cwd || process.cwd();
    this.env = options.env || {};
    this.files = options.files || {};
    this.context = undefined;
  }

  __setComparisonContext(context) {
    this.context = context;
  }

  async exec(command) {
    const contextFiles = this.context?.files ?? this.files;
    const testFile = this.context?.testFile;
    if (testFile) {
      const fixtureId = generateFixtureId(command, contextFiles);
      const fixtures = await loadFixtures(testFile);
      const fixture = fixtures[fixtureId];
      if (fixture) {
        return {
          stdout: fixture.stdout,
          stderr: fixture.stderr,
          exitCode: fixture.exitCode,
        };
      }
    }

    return runRealBash(command, this.context?.testDir ?? this.cwd, this.env);
  }
}

function normalizeExecResult(result) {
  if (!result || typeof result !== "object") {
    throw new Error(\`Engine returned an invalid result for upstream \${upstreamHead}\`);
  }
  return {
    stdout: String(result.stdout ?? ""),
    stderr: String(result.stderr ?? ""),
    exitCode: Number(result.exitCode ?? result.exit_code ?? result.code ?? result.status ?? 0),
  };
}

export { path, fs };
`;
}

function runVitest(vitestConfigPath, options) {
  const env = { ...process.env, JUST_BASH_ENGINE: options.engine };
  if (options.recordMode) {
    env.RECORD_FIXTURES = options.recordMode;
  } else {
    delete env.RECORD_FIXTURES;
  }
  if (options.rustAddonPath) {
    env.JUST_BASH_RUST_ADDON = options.rustAddonPath;
  }

  const args = [
    '--yes',
    `pnpm@${pnpmVersion}`,
    '--dir',
    upstreamPackageRoot,
    'exec',
    'vitest',
    'run',
    '--config',
    vitestConfigPath,
    ...options.vitestArgs,
  ];

  console.log(
    `[just-bash-conformance] upstream=${upstreamHead} engine=${options.engine} domains=${options.domains.join(
      ','
    )}`
  );
  console.log(`[just-bash-conformance] generated=${generatedRoot}`);

  const result = spawnSync('npx', args, {
    cwd: upstreamPackageRoot,
    env,
    stdio: 'inherit',
  });

  if (result.error) {
    throw result.error;
  }
  process.exit(result.status ?? 1);
}

try {
  const options = parseArgs(process.argv.slice(2));
  options.engine = options.engine.toLowerCase();
  if (!validEngines.has(options.engine)) {
    throw new Error(
      `Invalid engine: ${options.engine}. Expected one of: ${[...validEngines].join(', ')}`
    );
  }

  ensureUpstreamMirror();
  const allDomains = discoverDomains();

  if (options.listDomains) {
    for (const entry of allDomains) {
      console.log(entry.domain);
    }
    process.exit(0);
  }

  if (options.verifiedCorpus && (options.runAll || options.domains.length > 0)) {
    throw new Error('--verified-corpus cannot be combined with --all or --domain/--domains');
  }
  const useRustVerifiedCorpus =
    options.engine === 'rust' && !options.runAll && options.domains.length === 0;
  const useVerifiedCorpus = options.verifiedCorpus || useRustVerifiedCorpus;
  configureGeneratedRoot(options.engine);
  const selectedDomains = useVerifiedCorpus
    ? []
    : selectDomains(allDomains, options.domains, options.runAll);
  const rustAddonPath = resolveRustAddonPath();
  maybeSkipMissingRustAddon(options.engine, rustAddonPath);
  ensureUpstreamDependencies();
  const vitestConfigPath = useVerifiedCorpus
    ? prepareGeneratedVerifiedCorpusSuite(options.engine, rustAddonPath)
    : prepareGeneratedSuite(selectedDomains, options.engine, rustAddonPath);
  runVitest(vitestConfigPath, {
    domains: useVerifiedCorpus
      ? ['verified-comparison-corpus']
      : selectedDomains.map((entry) => entry.domain),
    engine: options.engine,
    recordMode: options.recordMode,
    rustAddonPath,
    vitestArgs: options.vitestArgs,
  });
} catch (error) {
  console.error(`[just-bash-conformance] ${error.message}`);
  process.exit(1);
}
