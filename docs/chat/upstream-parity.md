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
| `packages/chat` (`@chat-sdk/chat`) | core SDK package | in-progress | `crates/chat-sdk-chat` | `crates/chat-sdk-chat/src/{errors,logger,types}.rs::tests` (17 + 13 + 9 colocated tests; first two are 1:1 ports of upstream `*.test.ts`, the `types` tests are additive Rust-only serde coverage because upstream `types.ts` has no test file). | 54 src files, 23 test files upstream. Crate skeleton + `errors`, `logger`, and the first leaf layer of `types` (ChannelVisibility, LockScope, ConcurrencyStrategy, FetchDirection, TranscriptRole, WellKnownEmoji, THREAD_STATE_TTL_MS) ported. 2/23 upstream test files mapped, 39 Rust tests total. Phase-1. |
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
| `packages/chat/src/errors.test.ts`:`ChatError > should set message, code, and name` | `chat-sdk-chat::errors::tests::chat_error_should_set_message_code_and_name` | mapped | |
| `packages/chat/src/errors.test.ts`:`ChatError > should be instanceof Error` | `chat-sdk-chat::errors::tests::chat_error_should_be_instanceof_error` | mapped | Adapted: Rust uses `dyn std::error::Error` + enum-variant `matches!` rather than JS prototype chain. |
| `packages/chat/src/errors.test.ts`:`ChatError > should propagate cause` | `chat-sdk-chat::errors::tests::chat_error_should_propagate_cause` | mapped | |
| `packages/chat/src/errors.test.ts`:`ChatError > should allow undefined cause` | `chat-sdk-chat::errors::tests::chat_error_should_allow_undefined_cause` | mapped | |
| `packages/chat/src/errors.test.ts`:`RateLimitError > should set code to RATE_LIMITED` | `chat-sdk-chat::errors::tests::rate_limit_error_should_set_code_to_rate_limited` | mapped | |
| `packages/chat/src/errors.test.ts`:`RateLimitError > should store retryAfterMs` | `chat-sdk-chat::errors::tests::rate_limit_error_should_store_retry_after_ms` | mapped | |
| `packages/chat/src/errors.test.ts`:`RateLimitError > should allow undefined retryAfterMs` | `chat-sdk-chat::errors::tests::rate_limit_error_should_allow_undefined_retry_after_ms` | mapped | |
| `packages/chat/src/errors.test.ts`:`RateLimitError > should be instanceof ChatError and Error` | `chat-sdk-chat::errors::tests::rate_limit_error_should_be_instanceof_chat_error_and_error` | mapped | Adapted (see above). |
| `packages/chat/src/errors.test.ts`:`RateLimitError > should propagate cause` | `chat-sdk-chat::errors::tests::rate_limit_error_should_propagate_cause` | mapped | |
| `packages/chat/src/errors.test.ts`:`LockError > should set code to LOCK_FAILED` | `chat-sdk-chat::errors::tests::lock_error_should_set_code_to_lock_failed` | mapped | |
| `packages/chat/src/errors.test.ts`:`LockError > should be instanceof ChatError` | `chat-sdk-chat::errors::tests::lock_error_should_be_instanceof_chat_error` | mapped | Adapted. |
| `packages/chat/src/errors.test.ts`:`LockError > should propagate cause` | `chat-sdk-chat::errors::tests::lock_error_should_propagate_cause` | mapped | |
| `packages/chat/src/errors.test.ts`:`NotImplementedError > should set code to NOT_IMPLEMENTED` | `chat-sdk-chat::errors::tests::not_implemented_error_should_set_code_to_not_implemented` | mapped | |
| `packages/chat/src/errors.test.ts`:`NotImplementedError > should store feature field` | `chat-sdk-chat::errors::tests::not_implemented_error_should_store_feature_field` | mapped | |
| `packages/chat/src/errors.test.ts`:`NotImplementedError > should allow undefined feature` | `chat-sdk-chat::errors::tests::not_implemented_error_should_allow_undefined_feature` | mapped | |
| `packages/chat/src/errors.test.ts`:`NotImplementedError > should be instanceof ChatError` | `chat-sdk-chat::errors::tests::not_implemented_error_should_be_instanceof_chat_error` | mapped | Adapted. |
| `packages/chat/src/errors.test.ts`:`NotImplementedError > should propagate cause` | `chat-sdk-chat::errors::tests::not_implemented_error_should_propagate_cause` | mapped | |
| `packages/chat/src/logger.test.ts`:`ConsoleLogger > default level (info) > should not log debug messages` | `chat-sdk-chat::logger::tests::default_level_info_should_not_log_debug_messages` | mapped | |
| `packages/chat/src/logger.test.ts`:`ConsoleLogger > default level (info) > should log info messages` | `chat-sdk-chat::logger::tests::default_level_info_should_log_info_messages` | mapped | |
| `packages/chat/src/logger.test.ts`:`ConsoleLogger > default level (info) > should log warn messages` | `chat-sdk-chat::logger::tests::default_level_info_should_log_warn_messages` | mapped | |
| `packages/chat/src/logger.test.ts`:`ConsoleLogger > default level (info) > should log error messages` | `chat-sdk-chat::logger::tests::default_level_info_should_log_error_messages` | mapped | |
| `packages/chat/src/logger.test.ts`:`ConsoleLogger > debug level > should log all levels including debug` | `chat-sdk-chat::logger::tests::debug_level_should_log_all_levels_including_debug` | mapped | |
| `packages/chat/src/logger.test.ts`:`ConsoleLogger > warn level > should only log warn and error` | `chat-sdk-chat::logger::tests::warn_level_should_only_log_warn_and_error` | mapped | |
| `packages/chat/src/logger.test.ts`:`ConsoleLogger > error level > should only log errors` | `chat-sdk-chat::logger::tests::error_level_should_only_log_errors` | mapped | |
| `packages/chat/src/logger.test.ts`:`ConsoleLogger > silent level > should not log anything` | `chat-sdk-chat::logger::tests::silent_level_should_not_log_anything` | mapped | |
| `packages/chat/src/logger.test.ts`:`ConsoleLogger > prefix formatting > should use default prefix` | `chat-sdk-chat::logger::tests::prefix_formatting_should_use_default_prefix` | mapped | |
| `packages/chat/src/logger.test.ts`:`ConsoleLogger > prefix formatting > should use custom prefix` | `chat-sdk-chat::logger::tests::prefix_formatting_should_use_custom_prefix` | mapped | |
| `packages/chat/src/logger.test.ts`:`ConsoleLogger > extra args passthrough > should forward extra arguments` | `chat-sdk-chat::logger::tests::extra_args_passthrough_should_forward_extra_arguments` | mapped | Adapted: Rust has no variadic-args `console.*`. Extras are formatted into the captured line; asserted against the joined string. |
| `packages/chat/src/logger.test.ts`:`ConsoleLogger > child logger > should create child with combined prefix` | `chat-sdk-chat::logger::tests::child_logger_should_create_child_with_combined_prefix` | mapped | |
| `packages/chat/src/logger.test.ts`:`ConsoleLogger > child logger > should inherit log level` | `chat-sdk-chat::logger::tests::child_logger_should_inherit_log_level` | mapped | |

## Adapter Live Validation Log

Populated as adapters gain credential-gated live tests/examples.

| Adapter | Test/example | Last run | Result | Notes |
| --- | --- | --- | --- | --- |
| _none yet_ | | | | First live validation depends on Phase-2 adapter slices. |
