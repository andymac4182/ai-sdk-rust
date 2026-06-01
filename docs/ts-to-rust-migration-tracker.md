# TypeScript To Rust Migration Tracker

This tracker coordinates the remaining work to make the Rust workspace match the
public behavior and portable tests from these upstream TypeScript projects:

- `vercel-labs/open-agents`
- `vercel/workflow`
- `vercel/chat`
- `vercel/ai`

The goal is parity, not compatibility by approximation. A TypeScript package,
file, or test case is closed only when the Rust workspace has an owned row in the
appropriate ledger, a named Rust test for every portable upstream case, and an
explicit exception for every JavaScript-only or TypeScript-type-system-only row.

## Hard Rules

- Refresh the upstream source before making parity claims:
  - `npx opensrc fetch https://github.com/vercel-labs/open-agents`
  - `npx opensrc fetch https://github.com/vercel/workflow`
  - `npx opensrc fetch github:vercel/chat`
  - `npx opensrc fetch github:vercel/ai`
- Work in a dedicated Codex worktree branched from current `origin/main`.
- Keep ownership narrow. Do not take behavior from another bucket unless that
  bucket is complete or the tracker row is updated with the reason.
- Every completed thread must update its owning ledger or this tracker, run the
  verification commands for the row, commit the work, and merge back to
  `origin/main` through the repo merge lane.
- Use `/tmp/ai-sdk-rust-main-merge.lock` before rebasing or pushing a completed
  thread to `origin/main`.
- Do not count live-provider, Slack, Vercel, Docker, or credential-gated checks
  as normal CI requirements. Add ignored tests and document the exact env needed.

## Source Snapshot

Refreshed on 2026-06-01 with `npx opensrc fetch`.

| Project | Upstream HEAD | Local cache | Package manifests | TS/TSX files | Test files | Current Rust tracker |
| --- | --- | --- | ---: | ---: | ---: | --- |
| Open Agents | `24d679c7ba3d274aa73814c15673aeffcbe3c1c2` | `~/.opensrc/repos/github.com/vercel-labs/open-agents/main` | 6 | 567 | 106 | `docs/open-agents/*` |
| Workflow SDK | `ae3c833acd4f44ab84db65b44eb2ba2646eaecf9` | `~/.opensrc/repos/github.com/vercel/workflow/main` | 47 | 1085 | 151 | `docs/workflow-upstream-parity.md`, `docs/workflow-test-inventory.md` |
| Chat SDK | `ffc43fcf1f7679164be0806308bea237113c7590` | `~/.opensrc/repos/github.com/vercel/chat/main` | 22 | 469 | 124 | `docs/chat/upstream-parity.md`, `docs/chat/package-progress.md` |
| AI SDK | `ab6d66482d31afe15f4973a51c5f7cfa09c92ea6` | `~/.opensrc/repos/github.com/vercel/ai/main` | 87 | 4161 | 688 | `docs/upstream-parity.md`, `docs/package-progress.md` |

## Current Completion Picture

| Project | Current state | Evidence | Required next proof |
| --- | --- | --- | --- |
| Open Agents | In progress. The Rust service has Slack webhook ingress, real Vercel AI Gateway runtime config, Vercel sandbox support, and deployment docs. The missing durable artifact is a complete upstream test/functionality inventory for all 106 upstream tests and package surfaces. | `docs/open-agents/bucket-ownership.md`, `docs/open-agents/slack-remote-agent-architecture.md`, `docs/open-agents/deployment-verification.md`, `crates/open-agents-*` | Checked-in Open Agents upstream inventory that maps every portable upstream case to Rust tests or explicit exceptions, then a gate command that fails on unmapped rows. |
| Workflow SDK | Mostly ported with a row-level gate. The gate currently passes with 2679 inventory rows, 22 summary packages, and 28 ledger packages. `packages/core`/`workflow-core` is still marked `in-progress` because remaining core E2E rows need ownership or classification. | `node scripts/workflow-test-inventory.mjs --check && node scripts/workflow-parity-check.mjs` | Close or explicitly classify every remaining `workflow-core` E2E row, then move `packages/core` and `workflow-core` to `verified` only after the named-test mapping proves it. |
| Chat SDK | Ledger reports 100% for the tracked Rust surface: 18 of 18 package rows closed and 15 of 15 portable packages verified. Because upstream was refreshed to a newer HEAD, this still needs a current-upstream drift audit before it can be treated as permanently complete. | `scripts/package-progress-table.sh --ledger docs/chat/upstream-parity.md --estimates docs/chat/package-progress-estimates.tsv --output /tmp/chat-package-progress-check.md` | Reconcile current upstream package/test inventory against `docs/chat/upstream-parity.md`; add rows for new or renamed cases, or record that no drift exists. |
| AI SDK | In progress. Package progress is 55.1% average, 21 of 52 package rows closed, 11 of 42 portable rows strictly verified, 16 in progress, and 15 not started. | `docs/upstream-parity.md`, `docs/package-progress.md` | Complete every in-progress and not-started provider package with package-owned tests, then regenerate progress to 100% portable verified or explicitly documented JS-only/type-system exceptions. |

## Dispatch Board

Each row is intentionally sized so one Codex thread can own the branch, tests,
commit, and merge-back. Threads may split a row further when the upstream package
is too large, but the parent row remains open until all child rows are merged.

| ID | Status | Owner scope | Build | Verify before merge |
| --- | --- | --- | --- | --- |
| T2R-00 | in-progress | Master tracker and dispatch coordination | Keep this file current with upstream snapshots, thread ids, and close criteria. | `git diff --check`; relevant ledger/gate commands named in changed rows. |
| OA-01 | active | Open Agents upstream inventory gate | Add `docs/open-agents/upstream-parity.md` plus a generator/checker that inventories packages, source files, and all upstream tests. | Fresh `npx opensrc fetch https://github.com/vercel-labs/open-agents`; gate command proves every row is mapped or explicitly excluded; `cargo test -p open-agents-core -p open-agents-runtime -p open-agents-sandbox -p open-agents-slack -p open-agents-service`. |
| OA-02 | active | Open Agents Slack remote-agent runtime closure | Close remaining runtime gaps surfaced by OA-01: durable resume, tool approvals/questions, finish actions, and real sandbox/model error handling. | OA-01 gate; Slack emulator E2E; ignored live Slack/Gateway/Vercel proof documented with env names. |
| WF-01 | active | Workflow core E2E parity closure | Own the remaining `packages/core` E2E rows and either port them to `workflow-core` tests or classify them with explicit exceptions. | `node scripts/workflow-test-inventory.mjs --check`; `node scripts/workflow-parity-check.mjs`; `cargo test -p workflow-core`. |
| WF-02 | active | Workflow current-upstream drift audit | Compare refreshed upstream `ae3c833...` against the checked-in inventory and update rows for new, renamed, or removed tests/packages. | `node scripts/workflow-test-inventory.mjs --check`; `node scripts/workflow-parity-check.mjs`; no unclassified `needs-review` rows. |
| CHAT-01 | active | Chat SDK current-upstream drift audit | Reconcile refreshed upstream `ffc43fc...` with `docs/chat/upstream-parity.md`; preserve 100% only if every current portable row still maps to named Rust tests. | Regenerate `docs/chat/package-progress.md`; focused `cargo test -p chat-sdk-chat` and touched adapter crates; no unmapped portable rows. |
| AI-01 | active | AI SDK large foundational providers | Port or fully inventory `@ai-sdk/anthropic`, `@ai-sdk/amazon-bedrock`, `@ai-sdk/google`, and `@ai-sdk/google-vertex`. | Package-owned upstream test mapping; provider crate tests; progress regeneration. |
| AI-02 | active | AI SDK OpenAI-compatible major providers | Port or close `@ai-sdk/xai`, `@ai-sdk/groq`, `@ai-sdk/cohere`, `@ai-sdk/fireworks`, and `@ai-sdk/togetherai`. | Package-owned tests plus shared OpenAI-compatible regression tests where applicable; progress regeneration. |
| AI-03 | active | AI SDK media generation providers | Port or close `@ai-sdk/fal`, `@ai-sdk/klingai`, `@ai-sdk/prodia`, `@ai-sdk/replicate`, `@ai-sdk/luma`, and `@ai-sdk/black-forest-labs`. | Image/video request/response fixture tests, error metadata tests, ignored live proofs where credentials exist. |
| AI-04 | active | AI SDK speech, transcription, and audio providers | Port or close `@ai-sdk/elevenlabs`, `@ai-sdk/gladia`, `@ai-sdk/deepgram`, `@ai-sdk/hume`, `@ai-sdk/lmnt`, `@ai-sdk/revai`, `@ai-sdk/assemblyai`, and `@ai-sdk/voyage`. | Speech/transcription fixture tests, warning/error mapping tests, progress regeneration. |
| AI-05 | active | AI SDK remaining in-progress wrappers | Finish `@ai-sdk/azure`, `@ai-sdk/baseten`, `@ai-sdk/bytedance`, `@ai-sdk/cerebras`, `@ai-sdk/deepinfra`, `@ai-sdk/huggingface`, `@ai-sdk/moonshotai`, and `@ai-sdk/vercel`. | Package-owned tests for every row named in `docs/upstream-parity.md`; progress regeneration. |
| AI-06 | active | AI SDK public API and examples parity | Audit root `ai` ergonomics, examples, docs snippets, and high-level generate/stream/embed/object APIs against upstream. | Root crate tests, example compile checks, docs update, progress regeneration. |
| AI-07 | active | AI SDK Alibaba provider | Port or close `@ai-sdk/alibaba`, including chat/video provider behavior, usage conversion, cache control, and message conversion. | Package-owned Alibaba tests, explicit exceptions for non-portable rows, and progress regeneration. |
| CROSS-01 | active | Cross-SDK examples and docs parity | Inventory upstream examples/docs that imply public behavior across all four projects and either port them or document exclusions. | Checked-in inventory rows; `cargo test --doc` where docs are Rust; no undocumented examples that imply missing Rust behavior. |
| CROSS-02 | active | Live integration proof registry | Centralize ignored live tests for Slack, Vercel AI Gateway, Vercel Sandbox, provider APIs, Postgres/Redis, and emulator-based local E2E. | A docs page listing env vars and commands; ignored tests compile; local emulator tests pass without live credentials. |
| CROSS-03 | complete | Master parity gate | Adds `scripts/master-parity-gate.sh`, a single CI-safe command that runs the package progress generators, Workflow gate, Open Agents gate when OA-01 is available, and drift/status checks without requiring live credentials. | `scripts/master-parity-gate.sh`; `node scripts/workflow-test-inventory.mjs --check`; `node scripts/workflow-parity-check.mjs`; Open Agents is skipped with a clear OA-01 dependency message until `docs/open-agents/upstream-parity.md` and its gate script land. |

## Active Thread Map

First dispatch wave created on 2026-06-01 with `gpt-5.5` and `xhigh`
reasoning.

| Tracker row | Thread id | Thread title |
| --- | --- | --- |
| OA-01 | `019e830a-7e74-7e10-829a-4dcf0f930dd3` | Add Open Agents parity inventory |
| OA-02 | `019e830b-41e9-73f2-9136-babce50bbc2a` | Close OA-02 runtime gaps |
| WF-01 | `019e830b-4400-7193-834f-cca74a33063f` | T2R WF-01 Workflow Core E2E Closure |
| WF-02 | `019e830b-441a-7503-943d-06fb626f1f44` | Audit workflow upstream drift |
| CHAT-01 | `019e830b-44d2-7063-9030-5a3cd764a76c` | Audit Chat SDK drift |
| AI-01 | `019e830b-fd70-7600-a380-3d6567ac3ec2` | Port foundational AI providers |
| AI-02 | `019e830b-fd70-7600-a380-3d77ad85215a` | Port OpenAI-compatible providers |
| AI-03 | `019e830b-feaf-7322-9ef1-37f3a740f1ed` | Port media generation providers |
| AI-04 | `019e830c-0437-7d61-9b03-0d5715493ff7` | Port AI-04 audio providers |
| AI-05 | `019e830c-029b-7440-8f0e-d64f43f6f1f8` | Finish AI-05 provider wrappers |
| AI-06 | `019e830c-0dc3-7c91-b704-0111e1c66a18` | Audit AI root API parity |
| AI-07 | `019e830c-8e7d-7b63-88d2-3034e30347d7` | Add Alibaba AI SDK provider |
| CROSS-01 | `019e830c-8e75-7bf3-9a9c-c4925d0bd572` | Examples and docs parity |
| CROSS-02 | `019e830c-0e17-76e3-9363-7dc7377c32c6` | Add live proof registry |
| CROSS-03 | `019e830b-48a7-7713-a041-766f1b5efcb6` | Add master parity gate |

## AI SDK Package Queue

The AI SDK is the largest open surface. Current generated progress shows:

- 21 of 52 package rows closed.
- 11 of 42 portable package rows strictly verified.
- 16 package rows in progress.
- 15 package rows not started.

In-progress packages:

`@ai-sdk/azure`, `@ai-sdk/baseten`, `@ai-sdk/black-forest-labs`,
`@ai-sdk/bytedance`, `@ai-sdk/cerebras`, `@ai-sdk/deepgram`,
`@ai-sdk/deepinfra`, `@ai-sdk/huggingface`, `@ai-sdk/hume`,
`@ai-sdk/lmnt`, `@ai-sdk/luma`, `@ai-sdk/moonshotai`, `@ai-sdk/revai`,
`@ai-sdk/togetherai`, `@ai-sdk/vercel`, `@ai-sdk/voyage`.

Not-started packages:

`@ai-sdk/anthropic`, `@ai-sdk/amazon-bedrock`, `@ai-sdk/google`,
`@ai-sdk/google-vertex`, `@ai-sdk/xai`, `@ai-sdk/alibaba`,
`@ai-sdk/cohere`, `@ai-sdk/elevenlabs`, `@ai-sdk/fal`,
`@ai-sdk/fireworks`, `@ai-sdk/gladia`, `@ai-sdk/groq`,
`@ai-sdk/klingai`, `@ai-sdk/prodia`, `@ai-sdk/replicate`.

## Merge-Back Checklist

Every implementation thread must finish with:

1. Fresh upstream fetch for its source project.
2. Updated ledger rows for every upstream test or functionality row touched.
3. Named Rust tests for every portable upstream case.
4. Explicit `js-only-documented` or `type-system-impossible` rows for every
   excluded case.
5. Regenerated package progress or inventory files when applicable.
6. Focused crate tests for touched crates.
7. Repository hygiene checks:
   - `cargo fmt --all --check`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `scripts/check-naming-conventions.sh`
   - `git diff --check`
8. Merge to `origin/main` only after acquiring `/tmp/ai-sdk-rust-main-merge.lock`.
