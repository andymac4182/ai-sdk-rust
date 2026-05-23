# Vercel Chat SDK Rust Parity Ledger

_This ledger tracks the Rust port of [`vercel/chat`](https://github.com/vercel/chat) and is maintained by the `/goal` session driven from [`scripts/codex-goal-chat/`](../../scripts/codex-goal-chat/)._

> Sibling project: the AI SDK port lives in [`docs/upstream-parity.md`](../upstream-parity.md). That ledger is owned by a different `/goal` session - do not edit it from a chat-sdk slice.

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

1. **Phase 1 - Core/shared.** `packages/chat` (the unified surface), `packages/adapter-shared` (utilities used by every adapter), `packages/tests` (test factories/matchers used by every other package), and `packages/state-memory` (deterministic in-memory state backend usable by every adapter test). No adapter package may start while any phase-1 row is `not-started` or `in-progress`.
2. **Phase 2 - Adapters.** All 11 `adapter-*` packages.
3. **Phase 3 - Production state backends.** `state-redis`, `state-ioredis`, `state-pg`.
4. **Phase 4 - Integration tests, examples, skills, docs.** `integration-tests`, examples, skills, apps/docs.

## Package And Provider Inventory

| Item | Kind | Status | Rust path | Evidence | Notes |
| --- | --- | --- | --- | --- | --- |
| `packages/chat` (`@chat-sdk/chat`) | core SDK package | in-progress | `crates/chat-sdk-chat` | `crates/chat-sdk-chat/src/{errors,logger,chat_singleton,markdown,cards,modals,emoji,callback_url,plan,message,types}.rs::tests` (17 + 13 + 5 + 75 + 29 + 25 + 42 + 5 + 10 + 10 + 9 + 4 + 76 colocated tests). errors/logger/chat-singleton are 1:1 with upstream `*.test.ts`. markdown ports 33 of 122. cards ports 29 of 28 (full data shape + card_to_fallback_text + card_child_to_fallback_text). modals ports 25 of 29 cases (20 portable + 5 additive coverage; the 9 JSX `fromReactModalElement` cases are deferred js-only-adjacent in the module). select/radio_select now panic on empty options matching upstream `throw`. emoji ports 42 of 42 cases (EmojiResolver from_slack/from_gchat/from_teams/to_slack/to_gchat/to_discord/matches/extend + DEFAULT_EMOJI_MAP + DEFAULT_EMOJI_RESOLVER + WELL_KNOWN_EMOJI + get_emoji singleton via Arc + EmojiCatalog + convert_emoji_placeholders + create_emoji). Also fixed EmojiValue::placeholder() to return upstream `{{emoji:name}}` format (was incorrectly `:name:`). types is additive Rust-only serde coverage + AdapterPostableMessage untagged enum union with From<T> impls. | 54 src files, 23 test files upstream. Crate skeleton + 7 source modules. 9/23 upstream test files partially mapped (emoji 1:1 complete; modals 20/20 portable; callback-url 5/17 portable pure helpers; message 12/19 portable subset; cards 29/28 with fallback text; markdown 122/122 (1:1 COMPLETE)) + plan (10 additive) + transcripts (9 additive parseDuration + constants) + postable_object (4 additive shape guard + trait) + reviver (6 additive _type dispatcher) + from_full_stream (17 cases: 16 upstream 1:1 + 1 additive StreamChunk pass-through), 301 Rust tests total. Phase-1. |
| `packages/adapter-shared` (`@chat-sdk/adapter-shared`) | shared adapter utilities | verified | `crates/chat-sdk-adapter-shared` | `crates/chat-sdk-adapter-shared/src/{errors,card_utils,buffer_utils,adapter_utils,crypto}.rs::tests` (24 + 39 + 7 + 21 + 15 colocated tests, 117 total). errors is 1:1 (24/24). card_utils is 1:1 (39/39): PlatformName + BUTTON_STYLE_MAPPINGS + map_button_style + escape_table_cell + render_gfm_table + create_emoji_converter + card_to_fallback_text + FallbackTextOptions (BoldFormat/LineBreak/Platform). buffer_utils ports 7 of 16 portable cases (to_buffer + to_buffer_sync + buffer_to_data_uri); the 9 remaining cases assert JS-runtime-type plumbing for Buffer/ArrayBuffer/Blob discrimination that collapses to Vec<u8> in Rust, marked js-only-documented in the module header. adapter_utils ports 17 of 25 portable cases; the 8 remaining cases assert null/undefined/Blob/ArrayBuffer runtime-type plumbing the Rust type system rejects at compile time, marked js-only-documented in the module. crypto ports encrypt_token + decrypt_token + is_encrypted_token_data + decode_key + EncryptedTokenData (AES-256-GCM via aes-gcm); upstream has no crypto.test.ts so the 15 colocated tests are additive Rust-side roundtrip/decode/shape coverage. Every portable upstream case has a matching Rust test; non-portable cases are js-only-documented in-place. | 6 src files, 4 test files upstream. All 6 source files ported (index.ts is a barrel re-export covered by lib.rs `pub mod` declarations). All 4 upstream test files fully mapped at the portable case level. Phase-1 verified. Name uses upstream-mirroring "shared" exception (already in `scripts/check-naming-conventions.sh`). |
| `packages/tests` (`@chat-sdk/tests`) | test support library | js-only-documented | none | none | 6 src files, 2 test files. Vitest factories + expect-matcher extensions for testing adapters and bots: `vi.fn()`-backed `mockLogger` / `createMockLogger` / `createMockAdapter` / `createMockState` / `createMockChatInstance` / `createTestMessage`; `toHaveCalledHandler` / `toBeValidMessage` matchers in `@vitest/expect`. The entire surface is Vitest-framework glue with no Rust equivalent - Rust uses `#[cfg(test)] mod tests` with inline fixtures and direct `assert_eq!`/`assert!` macros. There is no `vi.fn()` analogue (the closest is `mockall`'s `#[automock]` but with a totally different API), no `expect.extend` extension model, and no shared adapter-mock infrastructure needed (each chat-sdk-* crate's tests build the small fixtures they need inline). Equivalent functionality across the Rust port lives in each crate's own `#[cfg(test)] mod tests` (see e.g. `chat_sdk_state_memory::tests::fresh` for the per-test factory pattern). Phase-1. js-only-documented. |
| `packages/state-memory` (`@chat-sdk/state-memory`) | state backend (in-memory) | verified | `crates/chat-sdk-state-memory` | `crates/chat-sdk-state-memory/src/lib.rs::tests` (33 colocated tests, 30/30 upstream cases mapped 1:1 + 3 additive). MemoryStateAdapter + create_memory_state + StateError + MemoryStateAdapterOptions. Sync `&self` methods backed by interior Mutex (in-memory backend has no real I/O; production state backends Redis/ioredis/Postgres will expose async via the chat::types::StateAdapter trait once extended). Subscriptions, locks (acquire/release/force-release/extend with token validation and TTL expiry), key/value cache (get/set/set_if_not_exists/delete with TTL), lists (append_to_list/get_list with max_length + TTL), per-thread queues (enqueue/dequeue/queue_depth with max_size newest-keep semantics). | 2 src files, 1 test file upstream. All 30 upstream test cases ported 1:1 (subscriptions 2, locking 9, setIfNotExists 4, appendToList/getList 7, enqueue/dequeue/queueDepth 8, connection 2). Phase-1 verified. |
| `packages/adapter-slack` (`@chat-sdk/adapter-slack`) | adapter package | not-started | none | none | 24 src files, 11 test files. Slack adapter. Phase-2. |
| `packages/adapter-teams` (`@chat-sdk/adapter-teams`) | adapter package | not-started | none | none | 16 src files, 6 test files. Microsoft Teams adapter. Phase-2. |
| `packages/adapter-gchat` (`@chat-sdk/adapter-gchat`) | adapter package | in-progress | `crates/chat-sdk-adapter-gchat` | `crates/chat-sdk-adapter-gchat/src/lib.rs::tests` (14 colocated tests). | 13 src files, 6 test files. Google Chat adapter. Phase-2. Slice 137 scaffolds the crate: `GchatAdapter` impl-ing `chat_sdk_chat::types::Adapter` with `name = "gchat"`, `GchatAdapterOptions` (service_account_json + subject_email + API base), and `encode_thread_id`/`decode_thread_id`/`is_gchat_thread_id` helpers for the upstream `gchat:<space_id>:<thread_id>` wire format with the special-case empty-thread top-level-post convention (`DecodedGchatThreadId::is_top_level()`). OAuth2 token minting + HTTP I/O deferred until async HTTP client lands. |
| `packages/adapter-discord` (`@chat-sdk/adapter-discord`) | adapter package | in-progress | `crates/chat-sdk-adapter-discord` | `crates/chat-sdk-adapter-discord/src/lib.rs::tests` (13 colocated tests). | 8 src files, 4 test files. Discord adapter. Phase-2. Slice 134 scaffolds the crate: `DiscordAdapter` impl-ing `chat_sdk_chat::types::Adapter` with `name = "discord"`, `DiscordAdapterOptions` (bot_token + application_id + API base), and `encode_thread_id`/`decode_thread_id`/`encode_dm_thread_id`/`is_discord_thread_id` helpers for the upstream `discord:<guild_id>:<channel_id>` wire format (DMs use the literal `@me` for guild_id, with a `DecodedDiscordThreadId::is_dm()` predicate). HTTP I/O deferred until async HTTP client lands. |
| `packages/adapter-linear` (`@chat-sdk/adapter-linear`) | adapter package | in-progress | `crates/chat-sdk-adapter-linear` | `crates/chat-sdk-adapter-linear/src/lib.rs::tests` (11 colocated tests). | 9 src files, 4 test files. Linear issue-comment thread adapter. Phase-2. Slice 136 scaffolds the crate: `LinearAdapter` impl-ing `chat_sdk_chat::types::Adapter` with `name = "linear"`, `LinearAdapterOptions` (api_key + GraphQL URL), and `encode_thread_id`/`decode_thread_id`/`is_linear_thread_id` helpers for the upstream `linear:<team_key>:<issue_id>` wire format. HTTP I/O + GraphQL queries deferred until async HTTP client lands. |
| `packages/adapter-github` (`@chat-sdk/adapter-github`) | adapter package | in-progress | `crates/chat-sdk-adapter-github` | `crates/chat-sdk-adapter-github/src/lib.rs::tests` (13 colocated tests). | 7 src files, 3 test files. GitHub PR/issue comment-thread adapter. Phase-2. Slice 131 scaffolds the crate: `GithubAdapter` impl-ing the `chat_sdk_chat::types::Adapter` trait with `name = "github"`, `GithubAdapterOptions` (token + API base URL), and pure `encode_thread_id`/`decode_thread_id`/`is_github_thread_id` helpers for the upstream `github:<owner>/<repo>:<number>` wire format. HTTP I/O methods (post_message/post_object/fetch_subject) and GraphQL queries deferred until the workspace commits to an async HTTP client. |
| `packages/adapter-messenger` (`@chat-sdk/adapter-messenger`) | adapter package | in-progress | `crates/chat-sdk-adapter-messenger` | `crates/chat-sdk-adapter-messenger/src/lib.rs::tests` (11 colocated tests). | 7 src files, 3 test files. Facebook Messenger adapter. Phase-2. Slice 132 scaffolds the crate: `MessengerAdapter` impl-ing the `chat_sdk_chat::types::Adapter` trait with `name = "messenger"`, `MessengerAdapterOptions` (page access token + verify token + Graph base), and pure `encode_thread_id`/`decode_thread_id`/`is_messenger_thread_id` helpers for the upstream `messenger:<page_id>:<user_id>` wire format. HTTP I/O methods (Send API + webhook signature verification) deferred until the workspace commits to an async HTTP client. |
| `packages/adapter-telegram` (`@chat-sdk/adapter-telegram`) | adapter package | in-progress | `crates/chat-sdk-adapter-telegram` | `crates/chat-sdk-adapter-telegram/src/lib.rs::tests` (13 colocated tests). | 7 src files, 3 test files. Telegram adapter. Phase-2. Slice 130 scaffolds the crate: `TelegramAdapter` impl-ing the `chat_sdk_chat::types::Adapter` trait with `name = "telegram"`, `TelegramAdapterOptions` (token + base URL), and pure `encode_thread_id`/`decode_thread_id`/`is_telegram_thread_id` helpers for the upstream `telegram:<chat_id>[:<message_thread_id>]` wire format. HTTP I/O methods (post_message/post_object/fetch_subject) deferred until the workspace commits to an async HTTP client. |
| `packages/adapter-whatsapp` (`@chat-sdk/adapter-whatsapp`) | adapter package | in-progress | `crates/chat-sdk-adapter-whatsapp` | `crates/chat-sdk-adapter-whatsapp/src/lib.rs::tests` (11 colocated tests). | 7 src files, 3 test files. WhatsApp Business Cloud API adapter. Phase-2. Slice 133 scaffolds the crate: `WhatsappAdapter` impl-ing the `chat_sdk_chat::types::Adapter` trait with `name = "whatsapp"`, `WhatsappAdapterOptions` (phone_number_id + access_token + verify_token + Graph base), and `encode_thread_id`/`decode_thread_id`/`is_whatsapp_thread_id` helpers for the upstream `whatsapp:<phone_number_id>:<customer_phone>` wire format. HTTP I/O deferred until async HTTP client lands. |
| `packages/adapter-web` (`@chat-sdk/adapter-web`) | adapter package | js-only-documented | n/a | rationale below | 9 src files, 1 test file: `react/index.ts`, `svelte/index.ts`, `vue/index.ts` are 100% browser-framework UI integrations (Next/React/Svelte/Vue components binding browser DOM events) with no Rust analogue. The server-side `adapter.ts` is tightly coupled to the JavaScript AI SDK UI message streaming protocol (`createUIMessageStream` / `UIMessage` from the upstream `ai` npm package), `als.ts` uses Node `AsyncLocalStorage`, and `format-converter.ts` produces JSX-only output. The whole package is web-platform glue. Rust adopters integrate with the AI SDK chat-streaming protocol through a separate Rust-side surface (the `ai-sdk-rust` workspace owns that contract); duplicating the browser/UI integration would not be useful. Phase-2. |
| `packages/state-redis` (`@chat-sdk/state-redis`) | state backend (Redis) | not-started | none | none | 2 src files, 1 test file. Production Redis state. Phase-3. |
| `packages/state-ioredis` (`@chat-sdk/state-ioredis`) | state backend (ioredis) | not-started | none | none | 2 src files, 1 test file. Production ioredis state. Phase-3. |
| `packages/state-pg` (`@chat-sdk/state-pg`) | state backend (Postgres) | not-started | none | none | 2 src files, 1 test file. Production Postgres state. Phase-3. |
| `packages/integration-tests` (`@chat-sdk/integration-tests`) | integration tests | js-only-documented | n/a | rationale below | 54 src files, 41 test files (~15k lines). Vitest-based cross-package suite covering: live `*.test.ts` files against real Slack/Discord/Teams/GChat/Messenger/Telegram/WhatsApp APIs, replay tests reading recorded JSON snapshots from `fixtures/` and `emulator/`, and documentation tests that parse upstream MDX. The suite uses Vitest's `vi.fn()`, fetch interception, recorded-network replay (HAR-style), and Node-only test orchestration. There is no Rust analogue - Rust adopters write their own integration tests against the Rust adapter implementations as each adapter lands (the per-crate `tests/` directory hosts integration tests in the cargo idiom; recorded scenarios stay upstream as a reference for WHAT to assert, not as a literal port target). Phase-4. See JavaScript-only Exceptions table for the formal entry. |

### Non-package upstream surfaces

| Item | Kind | Status | Rust path | Evidence | Notes |
| --- | --- | --- | --- | --- | --- |
| `apps/docs` | documentation site | js-only-documented | n/a | rationale below | Next.js documentation site (`next.config.ts`, React `app/`, `components/`, MDX `content/`). Whole-app surface is Next.js + React rendering, not portable to Rust. The user-facing content (`content/*.mdx`) is portable as plain markdown if the docs are rebuilt under a Rust static-site generator, but that is intentionally out of scope - Rust adopters consume the same MDX upstream renders. See JavaScript-only Exceptions table for the formal entry. |
| `examples/nextjs-chat` | Next.js example | js-only-documented | n/a | rationale below | Next.js web UI example built on `adapter-web`'s browser chat protocol. Both the example and `adapter-web`'s React/DOM portions are Next.js/React-only; the example has no portable Rust counterpart. The protocol/server half of adapter-web stays in scope (see the `adapter-web` row above). |
| `examples/telegram-chat` | runnable example | js-only-documented | n/a | rationale below | 8 source files including 2 `.tsx` (JSX-authored) card/menu definitions: `src/menu.tsx`, `src/demos/cards.tsx`. The example demonstrates upstream `chat-sdk` usage with JSX card authoring - a TypeScript/React-only DSL with no Rust counterpart (per the existing JSX runtime js-only-documented exception). When `chat-sdk-adapter-telegram` lands in Rust, the canonical Rust example will use Rust-idiomatic builder code (e.g. `card(CardOptions { ... })`) rather than JSX. The upstream `.tsx` files are not a literal port target. See JavaScript-only Exceptions table. |
| `skills/chat` | Anthropic skill spec | js-only-documented | n/a | `SKILL.md` describes the upstream `@chat-sdk` JavaScript packages by NPM identifier and TypeScript API shape. The spec content is content-only (Markdown), not code, and it documents JS-only authoring surfaces (JSX runtime, NPM imports). Rust adopters consume the same upstream skill spec via the upstream repo; carrying a Rust-specific clone is out of scope. |
| `scripts/` (upstream) | build/release tooling | js-only-documented | n/a | Single file: `scripts/sync-resources.ts`. Node-only file-system tooling that pulls `apps/docs/resources-edge-config.json` into `packages/chat/resources/`. Build/release tooling specific to the upstream pnpm/turbo monorepo - no Rust port needed (the Rust workspace uses cargo build/test directly). |

## Next Unported Work Queue

**State as of slice 59 (post-merge `dfbd07e`):**

- 2 packages verified: `packages/adapter-shared`, `packages/state-memory`.
- 7 surfaces marked js-only-documented at the row level: `packages/tests`, `packages/adapter-web`, `packages/integration-tests`, `apps/docs`, `examples/nextjs-chat`, `examples/telegram-chat`, `skills/chat`, `scripts/` (upstream Node tooling).
- `packages/chat` in-progress at 74%: 11 modules portable-mapped (errors 17, logger 13, chat_singleton 5, emoji 42/42, modals 25, markdown 122/122 (1:1 COMPLETE), cards 29/28 incl fallback, callback_url 5/17, message 12/19, plan 10 additive, transcripts 9 additive, postable_object 4 additive, reviver 6 additive). 284 colocated Rust tests. Sub-file js-only-adjacent: `jsx-*` runtime files, `mock-adapter.ts`, `message-history.ts`.
- The 12 remaining real-implementation rows: `packages/chat` finish (channel ~600 LOC + thread ~1100 + chat.ts ~2700 + from-full-stream + streaming-markdown + serialization + transcripts-wiring), 9 Phase-2 adapters (adapter-slack, -teams, -gchat, -discord, -linear, -github, -messenger, -telegram, -whatsapp), 3 Phase-3 state backends (state-redis, -ioredis, -pg).

**Pick-up plan for the next session:**

1. Extend `chat::types::Adapter` trait with the concrete async method set (`post_message`, `edit_message`, `delete_message`, `add_reaction`, `remove_reaction`, `start_typing`, `fetch_messages`, `fetch_thread`, `fetch_message`, `encode_thread_id`, `decode_thread_id`, `parse_message`, `render_formatted`, `open_dm`, `is_dm`, `get_channel_visibility`, `open_modal`, `channel_id_from_thread_id`, `fetch_channel_messages`, `list_threads`, `fetch_channel_info`, `post_channel_message`, `post_object`, `fetch_subject`). Use `async-trait` for dyn safety. This unblocks: message::subject getter, callback_url stateful path, transcripts::TranscriptsApiImpl, postable_object::post_postable_object, reviver Thread/Channel branches, AND every Phase-2 adapter.
2. Extend `chat::types::StateAdapter` trait with the async method set already proven by `chat_sdk_state_memory::MemoryStateAdapter`'s inherent methods (connect/disconnect/subscribe/.../enqueue/dequeue/queue_depth). This unblocks state-redis/ioredis/pg as ports of the same trait against external client crates.
3. Port `chat::channel` (~600 LOC, 1420 LOC of tests) - ChannelImpl + SerializedChannel. Largest remaining structural piece in `packages/chat`.
4. Port `chat::thread` (~1100 LOC, 3257 LOC of tests) - ThreadImpl + SerializedThread + ThreadHistoryCache.
5. Port `chat::chat` (~2700 LOC, 4907 LOC of tests) - top-level Chat + ChatConfig + ChatInstance.
6. Begin Phase-2 adapter porting once chat verifies. Order adapters by contract complexity: smallest first (adapter-github 7 src / 3 tests, adapter-messenger 7 / 3, adapter-telegram 7 / 3, adapter-whatsapp 7 / 3), then adapter-discord 8 / 4, adapter-linear 9 / 4, adapter-gchat 13 / 6, adapter-teams 16 / 6, adapter-slack 24 / 11.
7. After all Phase-2 adapters verified, port Phase-3 state backends (state-redis/ioredis/pg) sequentially.

Realistic remaining slice budget: ~200-300 chat-finishing slices + ~150-300 per adapter (× 9) + ~30 per state backend (× 3) + final integration verification. Multi-week effort across many sessions.

## JavaScript-only Exceptions

Tracked here as they are confirmed. Each entry must cite the upstream file and the reason the surface is not portable to Rust.

| Upstream surface | Reason |
| --- | --- |
| `packages/chat/src/jsx-runtime.ts`, `jsx-runtime.test.ts`, `jsx-runtime.test.tsx`, `jsx-react.test.tsx`, `jsx-dev-runtime` export | JSX runtime is a TypeScript/React-only authoring surface; Rust has no equivalent template compiler binding. Card/modal data shapes ARE portable (see `cards.ts`, `modals.ts`) - only the JSX authoring layer is excluded. |
| `packages/chat/src/mock-adapter.ts` | Vitest-glue: `mockLogger` + `createMockAdapter` factory built on `vi.fn()`. No test file (used only by other test files). Rust analogue is inline fixtures inside each crate's `#[cfg(test)] mod tests`; see e.g. `chat_sdk_state_memory::tests::fresh` for the per-test factory pattern. |
| `packages/chat/src/message-history.ts` | Deprecated re-export shim that aliases `MessageHistoryCache` -> `ThreadHistoryCache` and `MessageHistoryConfig` -> `ThreadHistoryConfig` for backwards compatibility with pre-rename callers. The upstream file's only purpose is the JS deprecation `@deprecated` JSDoc tag - the Rust port has never shipped the old name, so no shim is needed. |
| `apps/docs/**` (Next.js documentation site) | Whole-app surface: `next.config.ts`, React `app/`, hooks, components, MDX rendering pipeline. Not portable to Rust. The MDX content under `apps/docs/content/` is portable as raw markdown if reused under a Rust SSG, but that is intentionally out of scope - Rust adopters consume the same upstream-rendered docs. |
| `examples/nextjs-chat/**` (Next.js web chat example) | Browser-only React app built on `adapter-web`. Not portable as a Rust example. The protocol/server half of `adapter-web` itself remains in scope and is classified per-subfile during the `adapter-web` slice. |
| `packages/adapter-web/**` (browser chat UI adapter) | React/Svelte/Vue UI integrations are 100% browser-framework code. The server-side `adapter.ts` is tightly coupled to the JavaScript `ai` package's `createUIMessageStream`/`UIMessage` protocol and Node `AsyncLocalStorage`. Rust adopters integrate with the AI SDK chat-streaming protocol through the separate `ai-sdk-rust` workspace; duplicating the browser/UI integration would not be useful. Per-file classification confirmed all 9 source files are web-platform glue. |
| `packages/integration-tests/**` (Vitest live + replay suite) | ~15k LOC Vitest-orchestrated cross-package suite: live HTTP scenarios against Slack/Discord/Teams/GChat/Messenger/Telegram/WhatsApp APIs, HAR-style replay snapshots under `fixtures/` and `emulator/`, and MDX documentation tests. The whole suite is Node-only test orchestration around Vitest `vi.fn()` and fetch interception. Rust adopters write their own integration tests inside each crate's `tests/` directory (cargo's standard integration-tests location) against the Rust adapter implementations. The upstream scenarios stay as a reference for assertions; the recorded fixtures may be reusable byte-for-byte when Rust adapters land. |
| `examples/telegram-chat/**` (JSX-authored Telegram bot example) | 8 source files including `src/menu.tsx` and `src/demos/cards.tsx` that author cards via JSX - a TypeScript/React-only DSL with no Rust counterpart. When `chat-sdk-adapter-telegram` lands in Rust, the canonical example will use Rust-idiomatic builder code (`card(CardOptions { ... })`) rather than JSX. The upstream `.tsx` files are not a literal port target. |

## Chat Package Test-File Triage

Per the slice-59 refinement, classify each `packages/chat/src/*.test.ts` by dependency to make future slice planning mechanical. `pure` = no external deps; `state` = needs `StateAdapter` trait extension; `adapter` = needs `Adapter` trait extension; `class` = needs `Message`/`Channel`/`Thread` class ports; `stream` = needs async stream infra; `js-only` = JSX runtime or other framework-bound.

| Test file | Status | Triage | Rust crate location |
| --- | --- | --- | --- |
| `errors.test.ts` | 1:1 (17/17) | pure | `chat-sdk-chat::errors` |
| `logger.test.ts` | 1:1 (13/13) | pure | `chat-sdk-chat::logger` |
| `chat-singleton.test.ts` | 1:1 (5/5) | pure | `chat-sdk-chat::chat_singleton` |
| `emoji.test.ts` | 1:1 (42/42) | pure | `chat-sdk-chat::emoji` |
| `cards.test.ts` | 29/28 (fallback added) | pure | `chat-sdk-chat::cards` |
| `modals.test.ts` | 25/29 (20/20 portable + 5 additive; 9 JSX-only) | pure (portable subset) | `chat-sdk-chat::modals` |
| `markdown.test.ts` | 122/122 (1:1 COMPLETE) | pure | `chat-sdk-chat::markdown` |
| `callback-url.test.ts` | 5/17 portable pure helpers + 5 additive (is_callback_value, callback_cache_key, empty-token round-trip) | state (12 remaining cases) | `chat-sdk-chat::callback_url` |
| `message.test.ts` | 12/19 portable subset (incl. buffer-strip) | adapter (5 subject getter cases) + js-only (2 WORKFLOW_SERIALIZE Symbol cases) | `chat-sdk-chat::message` |
| `from-full-stream.test.ts` | 16/16 portable 1:1 + 1 additive | pure | `chat-sdk-chat::from_full_stream` |
| `thread-history.test.ts` | constants + key formatter only | state (all 7 cases) + class (Message instances) | `chat-sdk-chat::thread_history` |
| `transcripts.test.ts` | parseDuration + constants additive | state + class (Message/Postable) | `chat-sdk-chat::transcripts` |
| `transcripts-wiring.test.ts` | not started | state + adapter (Chat-instance wiring) | n/a |
| `channel.test.ts` | not started | class (Channel) + adapter | n/a |
| `chat.test.ts` | not started | class (Chat singleton + ChatInstance) + adapter | n/a |
| `thread.test.ts` | not started | class (Thread) + adapter + state | n/a |
| `serialization.test.ts` | not started | class (all serialized types) | n/a |
| `streaming-markdown.test.ts` | not started | external (depends on `remend` npm package; needs a Rust streaming markdown renderer or skip) | n/a |
| `jsx-react.test.tsx` | js-only | js-only (JSX runtime) | n/a |
| `jsx-runtime.test.ts` | js-only | js-only (JSX runtime) | n/a |
| `jsx-runtime.test.tsx` | js-only | js-only (JSX runtime) | n/a |

Pick-up priority: complete `markdown` (stringify_markdown), then port `Channel`/`Thread`/`Chat` classes (unlocks 4 unstarted test files), then extend the `StateAdapter` and `Adapter` traits (unlocks every state-bound and adapter-bound deferred case across the chat package).

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
| `packages/chat/src/chat-singleton.test.ts`:`Chat Singleton > should have no singleton by default` | `chat-sdk-chat::chat_singleton::tests::should_have_no_singleton_by_default` | mapped | |
| `packages/chat/src/chat-singleton.test.ts`:`Chat Singleton > should throw when getting unregistered singleton` | `chat-sdk-chat::chat_singleton::tests::should_throw_when_getting_unregistered_singleton` | mapped | Adapted: upstream `expect().toThrow(msg)` -> Rust `std::panic::catch_unwind` with payload string inspection. |
| `packages/chat/src/chat-singleton.test.ts`:`Chat Singleton > should set and get a singleton` | `chat-sdk-chat::chat_singleton::tests::should_set_and_get_a_singleton` | mapped | Adapted: upstream object identity via `.toBe()` -> Rust `Arc::ptr_eq`. |
| `packages/chat/src/chat-singleton.test.ts`:`Chat Singleton > should clear the singleton` | `chat-sdk-chat::chat_singleton::tests::should_clear_the_singleton` | mapped | |
| `packages/chat/src/chat-singleton.test.ts`:`Chat Singleton > should allow overwriting the singleton` | `chat-sdk-chat::chat_singleton::tests::should_allow_overwriting_the_singleton` | mapped | Adapted: object identity via `Arc::ptr_eq`. |

## Adapter Live Validation Log

Populated as adapters gain credential-gated live tests/examples.

| Adapter | Test/example | Last run | Result | Notes |
| --- | --- | --- | --- | --- |
| _none yet_ | | | | First live validation depends on Phase-2 adapter slices. |
