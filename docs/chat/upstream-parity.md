# Vercel Chat SDK Rust Parity Ledger

_This ledger tracks the Rust port of [`vercel/chat`](https://github.com/vercel/chat) and is maintained by the `/goal` session driven from [`scripts/codex-goal-chat/`](../../scripts/codex-goal-chat/)._

> Sibling project: the AI SDK port lives in [`docs/upstream-parity.md`](../upstream-parity.md). That ledger is owned by a different `/goal` session — do not edit it from a chat-sdk slice.

## Upstream Source

- Repository: `github:vercel/chat`
- Fetch command: `npx opensrc@latest fetch github:vercel/chat`
- Local cache: `~/.opensrc/repos/github.com/vercel/chat/main`
- Inventory commit: `aba6aa94fe5a2ed909ec4daa7db0e21887507fa4`
- Inventory commit message: `feat(slack): add api primitives subpath (#548)`
- Inventory commit date: `2026-05-22T00:58:40Z`
- Inventory fetched: `2026-05-23T05:41:18Z`

## Status Legend

| Status | Meaning |
| --- | --- |
| `not-started` | Upstream package/feature identified, no Rust counterpart yet. |
| `in-progress` | Some Rust scaffolding/typed contract exists; not test-parity yet. |
| `ported` | Rust implementation exists and runs deterministic fake/mock tests, but adapter live validation or full test-floor mapping is still pending. |
| `verified` | Strict 1:1 Rust crate exists, every portable upstream TypeScript test/case is mapped to a Rust test, and adapter live validation has been recorded if credentials exist. |
| `js-only-documented` | Surface is intentionally JavaScript-only (e.g. JSX runtime, browser-only UI, Next.js example); justification is recorded in the row notes. |

## Test Floor

EVERY portable original upstream TypeScript test/case must exist as an equivalent Rust test in the matching 1:1 `chat-sdk-*` crate. Rust may add more tests, but never fewer mapped original TypeScript tests; a package with even one missing portable upstream test/case is incomplete.

Upstream test extensions counted: `*.test.ts`, `*.test.tsx`, `*.test-d.ts`, `*.test-d.tsx`, `*.spec.ts`, `*.spec.tsx`. Test counts in the inventory below are file counts at the inventory commit; the test-case parity map (later in this file) tracks the individual cases inside each file.

## Required Work Order

Two-phase gate, enforced strictly:

1. **Phase 1 — Core/shared.** `packages/chat` (the unified surface), `packages/adapter-shared` (utilities used by every adapter), `packages/tests` (test factories/matchers used by every other package), and `packages/state-memory` (deterministic in-memory state backend usable by every adapter test). No adapter package may start while any phase-1 row is `not-started` or `in-progress`.
2. **Phase 2 — Adapters.** All 11 `adapter-*` packages.
3. **Phase 3 — Production state backends.** `state-redis`, `state-ioredis`, `state-pg`.
4. **Phase 4 — Integration tests, examples, skills, docs.** `integration-tests`, examples, skills, apps/docs.

## Package And Provider Inventory

| Item | Kind | Status | Rust path | Evidence | Notes |
| --- | --- | --- | --- | --- | --- |
| `packages/chat` (`@chat-sdk/chat`) | core SDK package | not-started | none | none | 54 src files, 23 test files. Unified `Chat` abstraction with adapters, AI tools integration, markdown rendering, modals/cards, JSX runtime, plan execution, streaming. Phase-1. |
| `packages/adapter-shared` (`@chat-sdk/adapter-shared`) | shared adapter utilities | not-started | none | none | 10 src files, 4 test files. Adapter utility, buffer utility, card utility, crypto, errors. Phase-1. Name uses upstream-mirroring "shared" exception; document in `scripts/check-naming-conventions.sh`. |
| `packages/tests` (`@chat-sdk/tests`) | test support library | not-started | none | none | 6 src files, 2 test files. Vitest factories, matchers, setup utilities for testing adapters and bots. Phase-1. |
| `packages/state-memory` (`@chat-sdk/state-memory`) | state backend (in-memory) | not-started | none | none | 2 src files, 1 test file. Dev/testing in-memory state adapter. Phase-1. |
| `packages/adapter-slack` (`@chat-sdk/adapter-slack`) | adapter package | not-started | none | none | 24 src files, 11 test files. Slack adapter. Phase-2. |
| `packages/adapter-teams` (`@chat-sdk/adapter-teams`) | adapter package | not-started | none | none | 16 src files, 6 test files. Microsoft Teams adapter. Phase-2. |
| `packages/adapter-gchat` (`@chat-sdk/adapter-gchat`) | adapter package | not-started | none | none | 13 src files, 6 test files. Google Chat adapter. Phase-2. |
| `packages/adapter-discord` (`@chat-sdk/adapter-discord`) | adapter package | not-started | none | none | 8 src files, 4 test files. Discord adapter. Phase-2. |
| `packages/adapter-linear` (`@chat-sdk/adapter-linear`) | adapter package | not-started | none | none | 9 src files, 4 test files. Linear issue-comment thread adapter. Phase-2. |
| `packages/adapter-github` (`@chat-sdk/adapter-github`) | adapter package | not-started | none | none | 7 src files, 3 test files. GitHub PR/issue comment-thread adapter. Phase-2. |
| `packages/adapter-messenger` (`@chat-sdk/adapter-messenger`) | adapter package | not-started | none | none | 7 src files, 3 test files. Facebook Messenger adapter. Phase-2. |
| `packages/adapter-telegram` (`@chat-sdk/adapter-telegram`) | adapter package | not-started | none | none | 7 src files, 3 test files. Telegram adapter. Phase-2. |
| `packages/adapter-whatsapp` (`@chat-sdk/adapter-whatsapp`) | adapter package | not-started | none | none | 7 src files, 3 test files. WhatsApp Business Cloud API adapter. Phase-2. |
| `packages/adapter-web` (`@chat-sdk/adapter-web`) | adapter package | not-started | none | none | 9 src files, 1 test file. Browser chat UI via AI SDK useChat protocol. Likely largely JS-only (DOM/React UI); the protocol/server half is portable. Phase-2; classify each subfile during slice. |
| `packages/state-redis` (`@chat-sdk/state-redis`) | state backend (Redis) | not-started | none | none | 2 src files, 1 test file. Production Redis state. Phase-3. |
| `packages/state-ioredis` (`@chat-sdk/state-ioredis`) | state backend (ioredis) | not-started | none | none | 2 src files, 1 test file. Production ioredis state. Phase-3. |
| `packages/state-pg` (`@chat-sdk/state-pg`) | state backend (Postgres) | not-started | none | none | 2 src files, 1 test file. Production Postgres state. Phase-3. |
| `packages/integration-tests` (`@chat-sdk/integration-tests`) | integration tests | not-started | none | none | 54 src files, 41 test files. Cross-package live/integration test suite. Phase-4. |

### Non-package upstream surfaces

| Item | Kind | Status | Rust path | Evidence | Notes |
| --- | --- | --- | --- | --- | --- |
| `apps/docs` | documentation site | not-started | none | none | Next.js documentation site. Almost certainly `js-only-documented`; confirm in slice and document. |
| `examples/nextjs-chat` | Next.js example | not-started | none | none | Web UI example using `adapter-web`. Likely `js-only-documented`. |
| `examples/telegram-chat` | runnable example | not-started | none | none | Telegram bot example. Portable target — port to a Rust example crate once `chat-sdk-adapter-telegram` exists. |
| `skills/chat` | Anthropic skill spec | not-started | none | none | `SKILL.md` Markdown skill spec. Carry the spec into the Rust docs tree verbatim; mark `js-only-documented` if the skill consumes JS-only APIs. |
| `scripts/` (upstream) | build/release tooling | not-started | none | none | Repo-local pnpm/turbo/changeset scripts. Classify case-by-case; most are tooling-only and `js-only-documented`. |

## Next Unported Work Queue

Phase-1 order (do these next, in this order):

1. **Slice 2 (planned):** Create `crates/chat-sdk-chat` skeleton mirroring `packages/chat`. Stub the public surface from `packages/chat/src/index.ts`, lift the top-level types (`Chat`, `Message`, `Channel`, `Thread`, error types), and seed serde shapes against upstream JSON contracts. Do NOT pull JSX runtime or React-only surfaces — those are `js-only-documented` and live as documented exceptions.
2. **Slice 3 (planned):** Port `packages/chat` deterministic-only modules — `errors`, `logger`, `markdown`, `streaming-markdown`, `emoji`, `serialization`, `reviver`, `postable-object`, `cards`, `modals`, `message`, `thread-history`, `message-history`, `callback-url`, `from-full-stream`, `channel`, `mock-adapter`. Each module ports its `*.test.ts` cases 1:1 into Rust tests in `crates/chat-sdk-chat/src/<module>.rs` + `tests/<module>.rs`.
3. **Slice 4 (planned):** Port `packages/adapter-shared` (depends on `crates/chat-sdk-chat::errors` and serialization). All 4 test files mapped.
4. **Slice 5 (planned):** Port `packages/tests` (depends on `chat-sdk-chat` + `chat-sdk-adapter-shared`).
5. **Slice 6 (planned):** Port `packages/state-memory` (depends on `chat-sdk-chat` state contracts).
6. **Slice 7 (planned, first refinement-pass slice):** Run the 5-cycle self-refining loop: append to `docs/chat/goal-refinements.md`, tighten `port-chat-sdk.md` based on phase-1 learnings, then start Phase 2.

Phase-2 ordering will be picked at slice 7 based on which adapters share the most contract surface with already-ported phase-1 modules.

## JavaScript-only Exceptions

Tracked here as they are confirmed. Each entry must cite the upstream file and the reason the surface is not portable to Rust.

| Upstream surface | Reason |
| --- | --- |
| `packages/chat/src/jsx-runtime.ts`, `jsx-runtime.test.ts`, `jsx-runtime.test.tsx`, `jsx-react.test.tsx`, `jsx-dev-runtime` export | JSX runtime is a TypeScript/React-only authoring surface; Rust has no equivalent template compiler binding. Card/modal data shapes ARE portable (see `cards.ts`, `modals.ts`) — only the JSX authoring layer is excluded. |

## Test-Case Parity Map

Populated as each upstream test file is mapped to Rust. Format: one row per upstream test/case.

| Upstream file:case | Rust crate::test | Status | Notes |
| --- | --- | --- | --- |
| _none yet_ | | | First mappings will land in slice 3 alongside the `chat-sdk-chat` deterministic-module port. |

## Adapter Live Validation Log

Populated as adapters gain credential-gated live tests/examples.

| Adapter | Test/example | Last run | Result | Notes |
| --- | --- | --- | --- | --- |
| _none yet_ | | | | First live validation depends on Phase-2 adapter slices. |
