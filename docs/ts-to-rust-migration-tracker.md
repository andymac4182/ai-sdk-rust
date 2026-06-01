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
| Workflow SDK | `ae3c833acd4f44ab84db65b44eb2ba2646eaecf9` | `~/.opensrc/repos/github.com/vercel/workflow/main` | 47 | 1019 | 149 | `docs/workflow-upstream-parity.md`, `docs/workflow-test-inventory.md` |
| Chat SDK | `ffc43fcf1f7679164be0806308bea237113c7590` | `~/.opensrc/repos/github.com/vercel/chat/main` | 23 | 504 | 131 | `docs/chat/upstream-parity.md`, `docs/chat/package-progress.md` |
| AI SDK | `ab6d66482d31afe15f4973a51c5f7cfa09c92ea6` | `~/.opensrc/repos/github.com/vercel/ai/main` | 87 | 4161 | 688 | `docs/upstream-parity.md`, `docs/package-progress.md` |

## Current Completion Picture

| Project | Current state | Evidence | Required next proof |
| --- | --- | --- | --- |
| Open Agents | Current tracked rows are closed. The Rust service has Slack webhook ingress, real Vercel AI Gateway runtime config, Vercel sandbox support, deployment docs, and a generated upstream inventory covering all 6 package manifests, 461 source files, and 106 test files. OA-03B replaced the one-poll Gateway runtime path with async completion coverage and an ignored live Gateway smoke path. | `docs/open-agents/upstream-parity.md`, `scripts/open-agents-test-inventory.mjs`, `docs/open-agents/bucket-ownership.md`, `docs/open-agents/slack-remote-agent-architecture.md`, `docs/open-agents/deployment-verification.md`, `crates/open-agents-*` | Keep the Open Agents gate green and rerun the live Slack/Gateway/Vercel proof paths when credentials or upstream drift change. |
| Workflow SDK | Current upstream portable rows are closed by the row-level gate. The gate currently passes with 2,679 inventory rows, 22 summary packages, and 28 ledger packages. `packages/core`/`workflow-core` is `verified`: the core inventory has 1,216 rows, 1,023 portable rows all mapped to named Rust tests, 189 `js-only-documented` rows, 4 `type-system-impossible` rows, and 0 `needs-review` rows. Core E2E specifically has 154 rows: 79 portable verified and 75 `js-only-documented`. | `node scripts/workflow-test-inventory.mjs --check && node scripts/workflow-parity-check.mjs`; `cargo test -p workflow-core` | Preserve the zero-`needs-review` invariant and rerun the drift audit whenever upstream `vercel/workflow` moves. |
| Chat SDK | CHAT-02 closed current-upstream drift at `ffc43fcf1f7679164be0806308bea237113c7590`: package progress is restored to 100.0%, all 19 package rows are closed, and all 16 portable package rows are strictly verified with named Rust tests or existing explicit exceptions. | `docs/chat/upstream-parity.md`, `docs/chat/package-progress.md` | Rerun the Chat SDK drift audit when upstream `vercel/chat` moves; preserve the hard rule that every portable row needs a named Rust test or explicit exception before any future 100% claim. |
| AI SDK | In progress. Package progress is 94.5% average, 47 of 52 package rows closed, 37 of 42 portable rows strictly verified, 5 in progress, and 0 not started. | `docs/upstream-parity.md`, `docs/package-progress.md` | Complete every in-progress provider package with package-owned tests, then regenerate progress to 100% portable verified or explicitly documented JS-only/type-system exceptions. |

## Dispatch Board

Each row is intentionally sized so one Codex thread can own the branch, tests,
commit, and merge-back. Threads may split a row further when the upstream package
is too large, but the parent row remains open until all child rows are merged.

| ID | Status | Owner scope | Build | Verify before merge |
| --- | --- | --- | --- | --- |
| T2R-00 | in-progress | Master tracker and dispatch coordination | Keep this file current with upstream snapshots, thread ids, and close criteria. | `git diff --check`; relevant ledger/gate commands named in changed rows. |
| OA-01 | complete | Open Agents upstream inventory gate | Added `docs/open-agents/upstream-parity.md` plus `scripts/open-agents-test-inventory.mjs` to inventory packages, source files, and all 106 upstream tests. | Fresh `npx opensrc fetch https://github.com/vercel-labs/open-agents`; `node scripts/open-agents-test-inventory.mjs --check`; `cargo test -p open-agents-core -p open-agents-runtime -p open-agents-sandbox -p open-agents-slack -p open-agents-service`. |
| OA-02 | complete | Open Agents Slack remote-agent runtime closure | Closed the local Slack runtime gaps surfaced by OA-01: durable active-run resume, question and approval block-action resumes, finish-action Slack summaries, terminal sandbox/model error reporting, and emulator approval coverage. | `node scripts/open-agents-test-inventory.mjs --check`; `cargo test -p open-agents-core -p open-agents-runtime -p open-agents-sandbox -p open-agents-slack -p open-agents-service -p chat-sdk-adapter-slack`; `scripts/open-agents-local-e2e.sh --all-local`; ignored Slack/Gateway/Vercel proof envs documented in `docs/open-agents/deployment-verification.md`. |
| OA-03B | complete | Open Agents real Gateway runtime proof | Replaced the one-poll `poll_ready` Gateway path with a real async execution path, added deterministic Pending-then-Ready coverage, preserved fixture/durable Slack behavior, and deployed the production service with `runtime=gateway`. | `cargo test -p open-agents-service`; `scripts/master-parity-gate.sh`; ignored live Gateway smoke when credentials exist. |
| WF-01 | verified | Workflow core E2E parity closure | Closed the remaining `packages/core` E2E rows: 79 portable rows mapped to named `workflow-core` Rust tests and 75 rows classified `js-only-documented`; also closed adjacent abort consistency and workflow set-attributes core rows needed before `packages/core` could be verified. | `node scripts/workflow-test-inventory.mjs --check`; `node scripts/workflow-parity-check.mjs`; `cargo test -p workflow-core`. |
| WF-02 | complete | Workflow current-upstream drift audit | Refreshed upstream `ae3c833...`, found only `packages/core/e2e/event-log-race-repro.test.ts` diagnostic-label drift, no package/test inventory row churn, and classified all current rows so zero `needs-review` rows remain. | `node scripts/workflow-test-inventory.mjs --check`; `node scripts/workflow-parity-check.mjs`; no unclassified `needs-review` rows. |
| CHAT-01 | complete | Chat SDK current-upstream drift audit | Reconciled refreshed upstream `ffc43fc...` with `docs/chat/upstream-parity.md`; drift found, so the prior 100% claim was retired. | `docs/chat/package-progress.md` regenerated; focused chat and affected adapter crate tests; no claim that current portable rows are all mapped. |
| CHAT-02 | complete | Chat SDK drift closure | Closed the CHAT-01 current-upstream drift: `packages/chat`, Slack, GChat, Telegram, WhatsApp, and new Twilio all have named Rust tests for every portable drift case; no new js-only/type-system exceptions were needed. | `docs/chat/package-progress.md` regenerated to 100%; `cargo test -p chat-sdk-chat -p chat-sdk-adapter-slack -p chat-sdk-adapter-gchat -p chat-sdk-adapter-telegram -p chat-sdk-adapter-whatsapp -p chat-sdk-adapter-twilio`; final merge gate passed. |
| AI-01 | complete | AI SDK large foundational providers | Ported and verified `@ai-sdk/anthropic`, `@ai-sdk/amazon-bedrock`, `@ai-sdk/google`, and `@ai-sdk/google-vertex` with row-level upstream mappings and package-owned tests. | Package-owned upstream test mapping; provider crate tests; progress regeneration. |
| AI-02 | active | AI SDK OpenAI-compatible major providers | Initial provider foundations for `@ai-sdk/xai`, `@ai-sdk/groq`, `@ai-sdk/cohere`, `@ai-sdk/fireworks`, and `@ai-sdk/togetherai` landed; all five package rows remain in-progress. AI-02B owns verified closure. | Package-owned tests plus shared OpenAI-compatible regression tests where applicable; progress regeneration. |
| AI-02B | active | AI SDK OpenAI-compatible provider verified closure | Map every remaining portable xAI, Groq, Cohere, Fireworks, and TogetherAI upstream case to named Rust tests or explicit exceptions. | Focused tests for all five providers; regenerated `docs/package-progress.md`; `scripts/master-parity-gate.sh`. |
| AI-03 | complete | AI SDK media generation providers | Ported the portable image/video media generation surface for `@ai-sdk/fal`, `@ai-sdk/klingai`, `@ai-sdk/prodia`, and `@ai-sdk/replicate`; kept already-green `@ai-sdk/luma` and `@ai-sdk/black-forest-labs` minimal. AI-03B owns driving these package rows from in-progress to verified or documenting exact remaining exceptions. | `cargo test -p ai-sdk-black-forest-labs -p ai-sdk-luma -p ai-sdk-replicate -p ai-sdk-fal -p ai-sdk-klingai -p ai-sdk-prodia`; `docs/ai-03-media-live-proofs.md`. |
| AI-03B | complete | AI SDK media provider full parity closure | Closed the remaining `@ai-sdk/black-forest-labs`, `@ai-sdk/luma`, `@ai-sdk/fal`, `@ai-sdk/klingai`, `@ai-sdk/prodia`, and `@ai-sdk/replicate` rows after AI-03's initial media slice. | Focused tests for all six provider crates; package row mapping in `docs/upstream-parity.md`; regenerated `docs/package-progress.md`; `scripts/master-parity-gate.sh`. |
| AI-04 | complete | AI SDK speech, transcription, and audio providers | Port or close `@ai-sdk/elevenlabs`, `@ai-sdk/gladia`, `@ai-sdk/deepgram`, `@ai-sdk/hume`, `@ai-sdk/lmnt`, `@ai-sdk/revai`, `@ai-sdk/assemblyai`, and `@ai-sdk/voyage`. | Speech/transcription fixture tests, warning/error mapping tests, progress regeneration. |
| AI-05 | complete | AI SDK remaining in-progress wrappers | Finished `@ai-sdk/azure`, `@ai-sdk/baseten`, `@ai-sdk/bytedance`, `@ai-sdk/cerebras`, `@ai-sdk/deepinfra`, `@ai-sdk/huggingface`, `@ai-sdk/moonshotai`, and `@ai-sdk/vercel`. | Package-owned tests for every row named in `docs/upstream-parity.md`; progress regenerated. |
| AI-06 | complete | AI SDK public API and examples parity | Audited current root `ai` ergonomics, examples, docs snippets, and high-level generate/stream/embed/object APIs against upstream `ab6d66482d31afe15f4973a51c5f7cfa09c92ea6`; added root facade coverage for structured-output helpers, stream callbacks, and agent UI stream helpers. | `cargo test -p ai-sdk-rust --lib`; `cargo check --examples`; docs update; progress regeneration. |
| AI-07 | complete | AI SDK Alibaba provider | Initial `crates/ai-sdk-alibaba` crate landed and AI-07B closed the remaining row-level gaps before the verified claim. | Package-owned Alibaba tests, explicit exceptions for non-portable rows, and progress regeneration. |
| AI-07B | complete | AI SDK Alibaba verified closure | Mapped every remaining portable Alibaba upstream case to named Rust tests or explicit exceptions and moved the package to verified with row-level evidence. | `cargo test -p ai-sdk-alibaba`; regenerated `docs/package-progress.md`; `scripts/master-parity-gate.sh`. |
| CROSS-01 | complete | Cross-SDK examples and docs parity | Added `docs/cross-sdk-examples-docs-parity.md` plus `scripts/cross-sdk-examples-docs-inventory.mjs` to inventory public docs, README files, and example units across all four projects. | `node scripts/cross-sdk-examples-docs-inventory.mjs --check`; `cargo test --doc`; `cargo check --examples`; no undocumented docs/examples in the checked scanner scope. |
| CROSS-02 | complete | Live integration proof registry | Centralized ignored live tests and no-credentials emulator proofs in `docs/live-integration-proof-registry.md` for Slack, Vercel AI Gateway, Vercel Sandbox, provider APIs, Postgres/Redis, and Open Agents local E2E. | Registry lists env vars, commands, expected behavior, and skip semantics; ignored tests compile; local emulator tests pass without live credentials. |
| CROSS-03 | complete | Master parity gate | Adds `scripts/master-parity-gate.sh`, a single CI-safe command that runs the package progress generators, Workflow gate, Open Agents gate, and drift/status checks without requiring live credentials. | `scripts/master-parity-gate.sh`; `node scripts/workflow-test-inventory.mjs --check`; `node scripts/workflow-parity-check.mjs`; `node scripts/open-agents-test-inventory.mjs --check`; Open Agents now runs as part of the gate after OA-01 landed. |

## Active Thread Map

First dispatch wave created on 2026-06-01 with `gpt-5.5` and `xhigh`
reasoning.

| Tracker row | Thread id | Thread title |
| --- | --- | --- |
| OA-01 | `019e830a-7e74-7e10-829a-4dcf0f930dd3` | Add Open Agents parity inventory |
| OA-02 | `019e830b-41e9-73f2-9136-babce50bbc2a` | Close OA-02 runtime gaps |
| OA-03B | `019e8336-56e1-7691-a400-7da8d5ec959f` | T2R OA-03B Gateway Runtime Fix |
| WF-01 | `019e830b-4400-7193-834f-cca74a33063f` | T2R WF-01 Workflow Core E2E Closure |
| WF-02 | `019e830b-441a-7503-943d-06fb626f1f44` | Audit workflow upstream drift |
| CHAT-01 | `019e830b-44d2-7063-9030-5a3cd764a76c` | Audit Chat SDK drift |
| CHAT-02 | `019e8320-d5ae-7a11-897f-2804b78d64a4` | T2R CHAT-02 Chat SDK Drift Closure |
| AI-01 | `019e830b-fd70-7600-a380-3d6567ac3ec2` | Port foundational AI providers |
| AI-01A | `019e8320-d5be-7551-9c94-fc4bf623bdb4` | T2R AI-01A Anthropic Provider |
| AI-01B | `019e8320-d5bd-7161-9a3c-a181315d2f9a` | T2R AI-01B Amazon Bedrock Provider |
| AI-01C | `019e8320-d593-79f1-942f-b1eac9d76531` | T2R AI-01C Google Provider |
| AI-01D | `019e8320-d5c5-78c2-9c36-5f64142efa47` | T2R AI-01D Google Vertex Provider |
| AI-02 | `019e830b-fd70-7600-a380-3d77ad85215a` | Port OpenAI-compatible providers |
| AI-02B | `019e8339-3f9a-7421-97f7-f94c398ffbc3` | T2R AI-02B Provider Verified Closure |
| AI-03 | `019e830b-feaf-7322-9ef1-37f3a740f1ed` | Port media generation providers |
| AI-03B | `019e8337-2b22-73f1-96d3-31f07b23bb4a` | Close media provider parity |
| AI-04 | `019e830c-0437-7d61-9b03-0d5715493ff7` | Port AI-04 audio providers |
| AI-05 | `019e830c-029b-7440-8f0e-d64f43f6f1f8` | Finish AI-05 provider wrappers |
| AI-06 | `019e830c-0dc3-7c91-b704-0111e1c66a18` | Audit AI root API parity |
| AI-07 | `019e830c-8e7d-7b63-88d2-3034e30347d7` | Add Alibaba AI SDK provider |
| AI-07B | `019e8337-2cb3-7501-83ef-bd5625ce5f34` | Verify Alibaba provider closure |
| CROSS-01 | `019e830c-8e75-7bf3-9a9c-c4925d0bd572` | Examples and docs parity |
| CROSS-02 | `019e830c-0e17-76e3-9363-7dc7377c32c6` | Add live proof registry |
| CROSS-03 | `019e830b-48a7-7713-a041-766f1b5efcb6` | Add master parity gate |

## AI-01 Child Rows

Created by the AI-01 inventory slice on 2026-06-01. These rows remain open
until each package maps every portable row in
`docs/ai-foundational-provider-inventory.md` to named Rust tests in the owning
crate, while preserving explicit JavaScript-only and type-system exceptions.

| Child row | Status | Owner scope | Required proof |
| --- | --- | --- | --- |
| AI-01A | complete | `@ai-sdk/anthropic` in `crates/ai-sdk-anthropic` | Ported Anthropic language model, files, skills, cache control, prompt conversion, provider tools, usage conversion, error mapping, fixtures, and ignored live-provider proof; 418 portable upstream rows map to named Rust tests, 6 TypeScript-only generic-inference rows remain documented exceptions. |
| AI-01B | complete | `@ai-sdk/amazon-bedrock` in `crates/ai-sdk-amazon-bedrock` | Ported Bedrock chat, Anthropic-on-Bedrock, embeddings, image, reranking, event-stream handling, SigV4/API-key fetch wrappers, tool preparation, usage conversion, settings, fixtures, and ignored live-provider proof; 380 portable upstream rows map to named Rust tests and 3 JavaScript callable-constructor rows remain documented exceptions. |
| AI-01C | verified | `@ai-sdk/google` in `crates/ai-sdk-google` | All 568 current upstream Google cases are closed: 566 portable rows map to named Rust tests in `crates/ai-sdk-google`, 2 TypeScript compile-error rows are explicit `type-system-impossible` exceptions, and an ignored live-provider proof is credential-gated on `GOOGLE_GENERATIVE_AI_API_KEY`. |
| AI-01D | complete | `@ai-sdk/google-vertex` in `crates/ai-sdk-google-vertex` | Ported Vertex auth, provider base/edge variants, embedding, image, video, Anthropic-on-Vertex, MaaS, xAI-on-Vertex, fixtures, and ignored live-provider proof. |

## AI SDK Package Queue

The AI SDK is the largest open surface. Current generated progress shows:

- 47 of 52 package rows closed.
- 37 of 42 portable package rows strictly verified.
- 5 package rows in progress.
- 0 package rows not started.

In-progress packages:

`@ai-sdk/xai`, `@ai-sdk/cohere`, `@ai-sdk/fireworks`, `@ai-sdk/groq`,
`@ai-sdk/togetherai`.

Not-started packages:

None. All AI SDK provider package rows are now at least in-progress, but
5 package rows still require row-level closure before AI SDK parity is done.

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
