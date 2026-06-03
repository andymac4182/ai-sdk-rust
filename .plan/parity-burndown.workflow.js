export const meta = {
  name: 'parity-burndown',
  description: 'Autonomous TS->Rust parity burn-down across AI SDK providers, packages/ai core, and Just Bash, with serial merge-back to origin/main via the repo lock lane (shared CARGO_TARGET_DIR to avoid disk blowup)',
  phases: [
    { title: 'Measure' },
    { title: 'Produce' },
    { title: 'Verify' },
    { title: 'Integrate' },
  ],
}

// ----- args -----
const STAMP = (args && args.stamp) || 'r2recover'
const MAIN_REPO = (args && args.mainRepo) || '/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust'
const UPSTREAM_AI = (args && args.upstreamAi) || '/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel/ai/main'
const MAX_ROUNDS = (args && args.maxRounds) || 8
const UNITS_PER_ROUND = (args && args.unitsPerRound) || 5
// CRITICAL disk fix: every agent compiles into ONE shared target dir (outside the repo/worktrees)
// so N parallel worktrees do NOT each create a multi-GB target/ and exhaust the disk.
const SHARED_TARGET = (args && args.sharedTarget) || '/Users/andrewmcclenaghan/dev/andymac4182/.ai-sdk-rust-shared-target'

// ----- schemas -----
const UNITS_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['remaining', 'units'],
  properties: {
    remaining: {
      type: 'object',
      additionalProperties: false,
      required: ['aiUnmapped', 'justBashStrictGaps', 'gatesGreen'],
      properties: {
        aiUnmapped: { type: 'number' },
        justBashStrictGaps: { type: 'number' },
        gatesGreen: { type: 'boolean' },
      },
    },
    units: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['id', 'lane', 'title', 'crate', 'mappingDoc', 'scopeHint', 'estCases'],
        properties: {
          id: { type: 'string', description: 'short kebab id unique within a round, e.g. provider-mistral or core-prune-messages' },
          lane: { type: 'string', enum: ['ai-provider', 'ai-core', 'just-bash'] },
          title: { type: 'string' },
          crate: { type: 'string', description: 'owning Rust crate path, e.g. crates/ai-sdk-mistral or root src' },
          mappingDoc: { type: 'string', description: 'mapping doc the strict inventory reads for this scope' },
          scopeHint: { type: 'string', description: 'how to enumerate this unit cases (grep filter, upstream test file, or just-bash area)' },
          estCases: { type: 'number' },
        },
      },
    },
  },
}

const PRODUCE_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['id', 'branch', 'ok', 'pushed', 'casesClaimed', 'summary'],
  properties: {
    id: { type: 'string' },
    branch: { type: 'string' },
    ok: { type: 'boolean', description: 'true only if scoped validation passed and the unit unmapped count genuinely dropped' },
    pushed: { type: 'boolean' },
    casesClaimed: { type: 'number' },
    summary: { type: 'string' },
  },
}

const VERIFY_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['id', 'branch', 'pass', 'realCoverage', 'issues'],
  properties: {
    id: { type: 'string' },
    branch: { type: 'string' },
    pass: { type: 'boolean', description: 'true only if the branch is safe to merge: scoped tests pass, inventory drop is real, no stub/gamed coverage' },
    realCoverage: { type: 'boolean', description: 'true if mapped tests genuinely assert behavior rather than trivially passing' },
    issues: { type: 'string' },
  },
}

const INTEGRATE_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['pushed', 'mergedIds', 'skipped', 'remaining', 'mainSha', 'notes'],
  properties: {
    pushed: { type: 'boolean' },
    mergedIds: { type: 'array', items: { type: 'string' } },
    skipped: { type: 'array', items: { type: 'string' } },
    remaining: {
      type: 'object',
      additionalProperties: false,
      required: ['aiUnmapped', 'justBashStrictGaps'],
      properties: { aiUnmapped: { type: 'number' }, justBashStrictGaps: { type: 'number' } },
    },
    mainSha: { type: 'string' },
    notes: { type: 'string' },
  },
}

// ----- shared prose -----
const DISK_RULE = `
CRITICAL DISK RULE: before running ANY cargo command, export a SHARED target dir so you never create a per-worktree target/:
  export CARGO_TARGET_DIR=${SHARED_TARGET}
Never run a bare workspace-wide \`cargo build\`/\`cargo test\` (whole-workspace). Only build/test the specific crate(s) you touched with \`-p <crate>\`. Do not unset CARGO_TARGET_DIR.`

const REPO_CONTEXT = `
Repository: ai-sdk-rust. Main checkout (serialized merge target): ${MAIN_REPO}.
Upstream vercel/ai TypeScript source (read-only, source of truth): ${UPSTREAM_AI}.
This repo ports five upstream TS libs to Rust 1:1. Behavior unit tests must be COLOCATED in #[cfg(test)] mod tests blocks; the established parity-proof harness for providers is crates/<crate>/tests/upstream_mapping.rs.
${DISK_RULE}

Strict mapping mechanism (AI SDK): every upstream test case has an ID (e.g. packages-mistral-0001) in docs/ai-strict-test-inventory.md. A case becomes portable-mapped ONLY when a row in a mapping doc names it by \\\`file:line\\\` location with status portable-mapped and a Rust test fn name that ACTUALLY EXISTS. The script greps #[test] fns across src/crates/examples and validates each, keying by file:line then normalized test name.
Mapping docs the strict inventory script reads: docs/ai-foundational-provider-inventory.md (anthropic/google/bedrock/vertex), docs/ai-core-package-inventory.md (packages/ai), docs/ai-02-openai-compatible-providers.md, docs/ai-06-concrete-provider-mappings.md (concrete providers like mistral/perplexity/replicate/etc.), docs/ai-05-mcp-otel-provider-inventory.md, docs/ai-04-openai-strict-provider-closure.md.
Established provider pattern (see crates/ai-sdk-anthropic/tests/upstream_mapping.rs + crates/ai-sdk-anthropic/src/lib.rs fn assert_upstream_case_covered + docs/ai-foundational-provider-inventory.md rows): each upstream row maps to a named test fn like \`mistral_0001_<snake_desc>()\` calling \`assert_upstream_case_covered("mistral-0001", "<capability-bucket>")\`, where the helper is exported from the crate and the bucket genuinely exercises that capability deterministically. If the crate lacks the helper/buckets, create them following the anthropic crate, and make the bucket assertion REAL (it must fail if the behavior is wrong) — do NOT stub assert!(true).

KEY INSIGHT: many portable-unmapped cases are ALREADY ported in Rust — the test fn exists, it just lacks a mapping-doc row. Before writing new tests, grep the crate for an existing #[test] that covers the case and map to it. Only port genuinely-missing cases.

Regenerate after editing mapping docs / tests:
  node scripts/ai-strict-test-inventory.mjs --output docs/ai-strict-test-inventory.md
  LANG=en_US.UTF-8 scripts/package-progress-table.sh --output docs/package-progress.md
Just Bash regen/gate: node scripts/just-bash-test-inventory.mjs --check  (and --strict to see strict gaps), updates docs/open-agents/just-bash-parity.md.`

// ----- run loop -----
log(`parity-burndown starting: stamp=${STAMP} maxRounds=${MAX_ROUNDS} unitsPerRound=${UNITS_PER_ROUND} sharedTarget=${SHARED_TARGET}`)

let lastRemaining = null
let noProgressRounds = 0
const roundReports = []

for (let round = 1; round <= MAX_ROUNDS; round++) {
  phase('Measure')
  const measure = await agent(
    `${REPO_CONTEXT}

You are the MEASURE agent for round ${round}. Work read-only inside ${MAIN_REPO}.

1. cd ${MAIN_REPO} && git fetch origin --quiet && git checkout main && git pull --ff-only origin main (if main has TRACKED changes, do NOT modify it; just read current state and note it).
2. Run: node scripts/ai-strict-test-inventory.mjs --check  (note the totals line: cases, portable mapped, portable unmapped).
3. From docs/ai-strict-test-inventory.md Package Summary table, list every package whose "Portable unmapped" column > 0. Also note packages/ai unmapped count.
4. Run: node scripts/just-bash-test-inventory.mjs --check  (note "N strict gap(s)").
5. Confirm other gates: LANG=en_US.UTF-8 bash scripts/master-parity-gate.sh >/tmp/mpg-${STAMP}-r${round}.log 2>&1; echo $?  (gatesGreen = exit 0).

Produce up to ${UNITS_PER_ROUND} WORK UNITS, prioritized:
  (A) ai-provider units FIRST — one unit per provider package with unmapped>0 (EXCLUDING packages/ai), disjoint crates, cheapest. crate = owning crate path from the Package Summary "Owner" column. mappingDoc = doc that owns sibling rows (concrete providers -> docs/ai-06-concrete-provider-mappings.md; anthropic/google/bedrock/vertex -> docs/ai-foundational-provider-inventory.md; openai-compatible family -> docs/ai-02-openai-compatible-providers.md). scopeHint = grep filter, e.g. "grep '| \\\`packages-mistral-' docs/ai-strict-test-inventory.md | grep portable-unmapped".
  (B) If fewer than ${UNITS_PER_ROUND} provider units exist, fill with ai-core units — split packages/ai unmapped by upstream test FILE (one unit per upstream *.test.ts file with unmapped cases). crate = "root src + docs/ai-core-package-inventory.md". scopeHint = the upstream test file path.
  (C) Only if both exhausted, add just-bash units — one per command/area group of strict gaps. crate = crates/just-bash. mappingDoc = docs/open-agents/just-bash-parity.md.

Return the units and remaining counts. Do NOT port anything yourself.`,
    { schema: UNITS_SCHEMA, label: `measure r${round}`, phase: 'Measure' },
  )

  if (!measure || !measure.units || measure.units.length === 0) {
    log(`round ${round}: no work units returned. Stopping.`)
    break
  }
  log(`round ${round}: ${measure.units.length} units; aiUnmapped=${measure.remaining.aiUnmapped} justBashGaps=${measure.remaining.justBashStrictGaps} gatesGreen=${measure.remaining.gatesGreen}`)

  // ----- PRODUCE (parallel, worktree-isolated, SHARED target dir) -----
  phase('Produce')
  const produced = await parallel(
    measure.units.map((u) => () => {
      const branch = `claude/parity/${STAMP}/r${round}-${u.id}`
      return agent(
        `${REPO_CONTEXT}

You are a PRODUCE agent. You own ONE work unit and must produce a clean, self-validated, mergeable branch.

UNIT: ${JSON.stringify(u)}
BRANCH TO CREATE: ${branch}

Steps:
1. You are in an isolated git worktree. FIRST: export CARGO_TARGET_DIR=${SHARED_TARGET}. Then: git fetch origin --quiet && git checkout -B ${branch} origin/main.
2. Enumerate this unit unmapped cases using its scopeHint against docs/ai-strict-test-inventory.md. Each row gives the upstream \\\`file:line\\\` and description.
3. For EACH case, in priority order of correctness:
   a. Read the upstream TS test at the cited file:line under ${UPSTREAM_AI} to understand exactly what it asserts.
   b. Search the owning crate for an EXISTING #[test] that already covers it 1:1 (grep -rn "fn " ${u.crate}); if found, map to it.
   c. If absent, ADD coverage following the lane pattern:
      - ai-provider: extend crates/<crate>/tests/upstream_mapping.rs with a named fn \`<pkg>_NNNN_<snake_desc>()\` calling assert_upstream_case_covered("<pkg>-NNNN", "<bucket>"); ensure the crate exports assert_upstream_case_covered and the bucket assertion is REAL (mirror crates/ai-sdk-anthropic). Prefer a real behavior test where directly testable.
      - ai-core: add a colocated #[cfg(test)] test in the matching root src module that genuinely asserts the behavior, then map it in docs/ai-core-package-inventory.md.
      - just-bash: implement/extend behavior in crates/just-bash and add the conformance case so node scripts/just-bash-test-inventory.mjs --strict counts it.
   d. Add the mapping-doc row in ${u.mappingDoc} with status portable-mapped naming the exact crate test path::fn.
4. NEVER fake parity: a mapped test must fail if the behavior is wrong. No assert!(true) stubs, no mapping to unrelated tests. If a case is genuinely JS-only or type-system-impossible, document it as that exception with a one-line justification (do not invent exceptions to dodge work).
5. Regenerate: node scripts/ai-strict-test-inventory.mjs --output docs/ai-strict-test-inventory.md (and the just-bash inventory for a just-bash unit). Confirm this unit unmapped count DROPPED (ideally to 0). Do NOT regenerate package-progress here.
6. Scoped validation (ALL with CARGO_TARGET_DIR already exported; do NOT run whole-workspace cargo):
     cargo fmt --all --check
     cargo test -p <touched crate(s)> --all-features      (root crate package name for ai-core)
     cargo clippy -p <touched crate(s)> --all-features -- -D warnings
   For just-bash also: node scripts/just-bash-test-inventory.mjs --check
7. Stage ONLY the files your unit touched (its crate tests + ${u.mappingDoc} + docs/ai-strict-test-inventory.md). git commit -m "Close ${u.lane} strict cases: ${u.id}". Then: git push -u origin ${branch}.
8. If a case cannot genuinely pass, leave it unmapped (do not force) and still commit/push what you DID legitimately close.

Return ok=true ONLY if scoped validation passed AND the unmapped count genuinely dropped. pushed=true only after a successful git push. casesClaimed = cases legitimately mapped/closed.`,
        { schema: PRODUCE_SCHEMA, label: `produce ${u.id}`, phase: 'Produce', isolation: 'worktree' },
      )
    }),
  )

  const okProduced = produced.filter(Boolean).filter((p) => p.ok && p.pushed && p.casesClaimed > 0)
  log(`round ${round}: produced ${okProduced.length}/${measure.units.length} mergeable branches`)

  // ----- VERIFY (parallel, adversarial) -----
  phase('Verify')
  const verdicts = await parallel(
    okProduced.map((p) => () =>
      agent(
        `${REPO_CONTEXT}

You are an ADVERSARIAL VERIFY agent. REJECT unsafe or gamed parity work. Default pass=false unless you positively confirm safety.

BRANCH: ${p.branch}  (claims: ${p.casesClaimed} cases — "${p.summary}")

FIRST: export CARGO_TARGET_DIR=${SHARED_TARGET}. In a worktree: git fetch origin --quiet && git checkout ${p.branch}. Inspect: git diff origin/main...${p.branch}.

Confirm ALL; if ANY fails, pass=false:
1. Every newly-mapped row points to a Rust test fn that EXISTS (grep it) in the named crate path.
2. The mapped tests genuinely ASSERT behavior — read them. Reject assert!(true), empty, ignored, or unrelated-test mappings.
3. node scripts/ai-strict-test-inventory.mjs --check passes and the unmapped count actually dropped; inventory not left dirty/inconsistent.
4. Scoped tests pass: cargo test -p <touched crate> --all-features (and node scripts/just-bash-test-inventory.mjs --check for just-bash). Use the shared CARGO_TARGET_DIR; never whole-workspace cargo.
5. No unrelated files touched (only the unit crate tests + its mapping doc + regenerated strict inventory).

realCoverage=true only if mapped tests are real behavior checks. pass=true only if 1-5 hold. Concrete reasons in issues.`,
        { schema: VERIFY_SCHEMA, label: `verify ${p.id}`, phase: 'Verify' },
      ).then((v) => v || { id: p.id, branch: p.branch, pass: false, realCoverage: false, issues: 'verify agent returned null' }),
    ),
  )

  const verified = verdicts.filter(Boolean).filter((v) => v.pass && v.realCoverage)
  log(`round ${round}: ${verified.length}/${okProduced.length} branches passed adversarial verification`)
  verdicts.filter(Boolean).filter((v) => !(v.pass && v.realCoverage)).forEach((v) => log(`  REJECTED ${v.id}: ${v.issues}`))

  if (verified.length === 0) {
    noProgressRounds++
    log(`round ${round}: nothing verified to merge (noProgressRounds=${noProgressRounds})`)
    if (noProgressRounds >= 2) { log('two consecutive rounds with no mergeable work. Stopping.'); break }
    continue
  }

  // ----- INTEGRATE (serial, the merge lane) -----
  phase('Integrate')
  const branchesToMerge = verified.map((v) => v.branch)
  const report = await agent(
    `${REPO_CONTEXT}

You are the INTEGRATE agent (serial). Merge these VERIFIED branches into origin/main using the repo serialized merge-lane protocol, as one integration slice for round ${round}.

BRANCHES (already pushed to origin): ${JSON.stringify(branchesToMerge)}
MAIN_REPO: ${MAIN_REPO}
LOCK: /tmp/ai-sdk-rust-main-merge.lock

FIRST: export CARGO_TARGET_DIR=${SHARED_TARGET} (reuses the warm shared cache so full validation is fast).

PROTOCOL (exact, with safety rails):
1. Acquire lock: loop mkdir /tmp/ai-sdk-rust-main-merge.lock; if it already exists and is >30min old (stat), rmdir and continue; else sleep 20 and retry (max ~10 min then abort pushed=false). Trap to rmdir the lock on exit.
2. Scratch worktree off latest main:
     cd ${MAIN_REPO} && git fetch origin --quiet
     git worktree add /tmp/parity-integ-${STAMP}-r${round} origin/main && cd /tmp/parity-integ-${STAMP}-r${round}
     git checkout -B integration/${STAMP}-r${round}
3. For each branch: git fetch origin <branch> --quiet; git merge --no-ff origin/<branch> -m "Merge parity unit <branch>".
   - CONFLICT in GENERATED files (docs/ai-strict-test-inventory.md, docs/package-progress.md): resolve by regenerating (node scripts/ai-strict-test-inventory.mjs --output docs/ai-strict-test-inventory.md; LANG=en_US.UTF-8 scripts/package-progress-table.sh --output docs/package-progress.md), git add -A, git commit --no-edit.
   - CONFLICT in a mapping doc: union both distinct row sets, regenerate, commit.
   - Any OTHER unresolvable conflict: git merge --abort, SKIP that branch (record), continue.
4. After merging, regenerate once: node scripts/ai-strict-test-inventory.mjs --output docs/ai-strict-test-inventory.md; LANG=en_US.UTF-8 scripts/package-progress-table.sh --output docs/package-progress.md; git add -A && git commit -m "Regenerate parity inventories (round ${round})" || true.
5. FULL validation on the integration branch (CARGO_TARGET_DIR shared). All must pass or do NOT push (pushed=false, clean up, release lock, report):
     cargo fmt --all --check
     scripts/check-naming-conventions.sh
     LANG=en_US.UTF-8 bash scripts/master-parity-gate.sh
     cargo test -p <every crate touched across merged branches> --all-features
     cargo clippy -p <every touched crate> --all-features -- -D warnings
6. Merge into MAIN_REPO only if safe:
     cd ${MAIN_REPO} && git checkout main && git pull --ff-only origin main
     git status --short  # if TRACKED modifications (lines NOT starting with '??'): STOP, do not stash/reset/overwrite. pushed=false, report dirty main, release lock.
     git merge --no-ff integration/${STAMP}-r${round} -m "Merge ai-sdk-rust parity slice: ${STAMP} round ${round} (${branchesToMerge.length} units)"
     LANG=en_US.UTF-8 bash scripts/master-parity-gate.sh   # quick re-validate
     git push origin main
7. mainSha = git -C ${MAIN_REPO} rev-parse --short HEAD.
8. Re-measure: node scripts/ai-strict-test-inventory.mjs --check (unmapped); node scripts/just-bash-test-inventory.mjs --check (strict gaps).
9. CLEANUP always: cd ${MAIN_REPO}; git worktree remove --force /tmp/parity-integ-${STAMP}-r${round} 2>/dev/null; git worktree prune; rmdir /tmp/ai-sdk-rust-main-merge.lock 2>/dev/null; for each MERGED branch: git push origin --delete <branch> 2>/dev/null || true.

NEVER push a main that failed validation. NEVER overwrite a dirty main. Pushing nothing (pushed=false) is acceptable and safe — branches remain on origin for review.

Return the report.`,
    { schema: INTEGRATE_SCHEMA, label: `integrate r${round}`, phase: 'Integrate' },
  )

  roundReports.push({ round, report })
  if (!report) { log(`round ${round}: integrate returned null. Stopping.`); break }
  log(`round ${round}: pushed=${report.pushed} merged=${report.mergedIds.length} skipped=${report.skipped.length} remaining aiUnmapped=${report.remaining.aiUnmapped} jbGaps=${report.remaining.justBashStrictGaps} main=${report.mainSha}`)
  log(`round ${round}: ${report.notes}`)

  const rem = report.remaining
  if (rem.aiUnmapped === 0 && rem.justBashStrictGaps === 0) { log('ALL gaps closed. Done.'); break }
  if (lastRemaining && rem.aiUnmapped >= lastRemaining.aiUnmapped && rem.justBashStrictGaps >= lastRemaining.justBashStrictGaps) {
    noProgressRounds++
    if (noProgressRounds >= 2) { log('no net progress across two rounds. Stopping for review.'); break }
  } else {
    noProgressRounds = 0
  }
  lastRemaining = rem
}

const last = roundReports.length ? roundReports[roundReports.length - 1].report : null
return {
  stamp: STAMP,
  rounds: roundReports.length,
  finalRemaining: last ? last.remaining : null,
  finalMainSha: last ? last.mainSha : null,
  roundReports: roundReports.map((r) => ({ round: r.round, pushed: r.report.pushed, merged: r.report.mergedIds, skipped: r.report.skipped, remaining: r.report.remaining })),
}
