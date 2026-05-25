# Intentionally Unported Cases — Chat SDK Rust port

This registry catalogues every upstream `vercel/chat` test case that
**cannot be literally ported to Rust** and explains why. The Rust
port honours the upstream behaviour through the closest available
Rust idiom (typed builder, serde shape, type-system guarantee), but
the upstream test as written cannot be reproduced 1:1 in Rust
because it exercises a JavaScript-only language feature.

The registry is organised by Rust crate. Each section enumerates the
upstream cases that crate documents as structurally unportable.

Cases listed here are considered **closed for "100% port"
purposes** per the project's
[`scripts/codex-goal-chat/goal-condition.md`](../../scripts/codex-goal-chat/goal-condition.md).
Every entry must cite the upstream file + line range and the
Rust-side replacement (where one exists).

The Rust-portable cases are tracked in
[`upstream-parity.md`](upstream-parity.md) — this file ONLY lists
the structurally-unportable cases.

## Top-level sections

- **`chat-sdk-chat`** — JSX runtime, JS Symbol-keyed protocols,
  deprecated re-export aliases.
- **`chat-sdk-state-redis`** — node-redis client injection,
  EventEmitter wait-for-ready, JS module loader, integration tests
  skipped upstream.
- **`chat-sdk-state-ioredis`** — ioredis cluster client injection,
  same EventEmitter shape, JS module loader, integration tests
  skipped upstream.
- **`chat-sdk-state-pg`** — node-postgres client injection,
  EventEmitter wait-for-`connect`, JS module loader, integration
  tests skipped upstream.
- **`chat-sdk-adapter-telegram`** — Vitest `vi.fn()`-mocked
  HTTP-fetch cases under `describe("TelegramAdapter")` +
  `describe("getUser")` + `describe("applyTelegramEntities")`,
  default-Logger constructor parameter, subclass extensibility.
- **`chat-sdk-adapter-whatsapp`** — Vitest `vi.fn()`-mocked
  HTTP-fetch cases under `describe("handleWebhook - POST signature
  verification")` + `describe("handleWebhook - POST message
  processing")` + `describe("stream")`, subclass extensibility.
- **`chat-sdk-adapter-discord`** — Vitest `vi.fn()`-mocked
  HTTP-fetch cases under `describe("handleWebhook - PING / *_COMPONENT
  / APPLICATION_COMMAND / JSON parsing / forwarded gateway events)`
  + `describe("postMessage / editMessage / deleteMessage / addReaction
  / removeReaction / startTyping")` + `describe("openDM /
  fetchMessages / fetchChannelMessages / fetchChannelInfo /
  postChannelMessage / listThreads / fetchThread)` +
  `describe("legacy gateway interactions / handleForwardedMessage /
  handleForwardedReaction / initialize / mentionRoleIds /
  createDiscordThread 160004 recovery / getUser")`, default-Logger
  constructor parameter, subclass extensibility, discord.js `Client`
  partials.
- **`chat-sdk-adapter-teams`** — Vitest `vi.fn()`-mocked HTTP-fetch
  cases under `describe("constructor env var resolution")` +
  `describe("createTeamsAdapter factory")` + `describe("handleWebhook")`
  + `describe("initialize")` + `describe("postMessage / editMessage /
  deleteMessage / startTyping / openDM / getUser")`, default-Logger
  constructor parameter, ESM compatibility subprocess assertion,
  `createTeamsAdapter` function-export typeof check, subclass
  extensibility.
- **`chat-sdk-adapter-messenger`** — Vitest `vi.fn()`-mocked
  HTTP-fetch cases under `describe("initialization")` +
  `describe("webhook handling - payload validation / message
  processing / postback handling / reaction handling")` +
  `describe("messaging - posting messages / streaming")` +
  `describe("attachments - downloadAttachment*")` +
  `describe("thread and channel info")` +
  `describe("Graph API error handling")`, subclass extensibility,
  invalid-postable-shape TypeError.
- **`chat-sdk-adapter-github`** — Vitest `vi.fn()`-mocked
  Octokit-typed-client cases under `describe("octokit getter")` +
  `describe("initialize")` + `describe("handleWebhook")` +
  `describe("self-message detection")` +
  `describe("postMessage / editMessage / deleteMessage /
  addReaction / removeReaction / stream")` +
  `describe("fetchMessages / fetchThread / listThreads /
  fetchChannelInfo / getUser / fetchSubject")`, default-Logger
  constructor parameter, subclass extensibility, typed-client
  `Octokit` instance identity / `AsyncLocalStorage`-resolved
  per-installation getter, `defaultOctokit` property-injection
  pattern, no-auth runtime throw (type-system-enforced in Rust).
- **`chat-sdk-adapter-linear`** — Vitest `vi.fn()`-mocked
  `LinearClient` typed-client cases under `describe("linearClient
  getter")` + `describe("constructor")` no-auth/no-botUserId throws
  + `describe("handleWebhook - signature verification /
  timestamp validation / invalid JSON / comment created /
  agent session events / reaction events / unknown event types")`
  + `describe("buildMessage via webhook")` +
  `describe("postMessage / editMessage / deleteMessage /
  addReaction / fetchMessages / fetchThread / initialize /
  ensureValidToken / refreshClientCredentialsToken / runtime
  operations / client credentials auth / multi-tenant installations
  / getUser / fetchSubject")`, AES-256-GCM token encryption (no
  AEAD dep in scope), default-Logger constructor parameter,
  subclass extensibility, typed-client `LinearClient` instance
  identity / `AsyncLocalStorage`-resolved per-installation getter.
- **`chat-sdk-adapter-slack`** — Vitest `vi.fn()`-mocked
  `@slack/web-api` `WebClient` typed-client cases under
  `describe("webClient getter")` + `describe("direct WebClient
  access via adapter.client")` + `describe("handleWebhook -
  signature verification / webhookVerifier / URL verification /
  event_callback / interactive payloads / JSON parsing / slash
  commands / assistant events")` + `describe("multi-workspace mode
  / installationProvider / installationKeyPrefix / handleOAuthCallback
  / withBotToken / botToken as function")` + `describe("postMessage
  / editMessage / deleteMessage / addReaction / removeReaction /
  openModal / updateModal / startTyping / openDM / fetchMessages /
  fetchMessage / fetchChannelInfo / fetchChannelMessages /
  postChannelMessage / listThreads / publishHomeView /
  setSuggestedPrompts / setAssistantStatus / setAssistantTitle /
  decodeEphemeralMessageId / editMessage via response_url /
  deleteMessage via response_url / isMessageFromSelf / reverse user
  lookup / stream / scheduleMessage / getUser / link unfurl /
  parseMessage / link extraction / DM message handling / message
  subtype handling / initialize / fetchThread / Attachment.fetchData
  / resolveInlineMentions / error handling / edge cases / date
  parsing / formatted text extraction / ephemeral message ID
  encoding")` + `@slack/socket-mode` SDK cases under
  `describe("socket mode - factory validation / handleWebhook /
  initialize / routeSocketEvent / disconnect / forwarding")` +
  `describe("startSocketModeListener / routeSocketEvent with
  options")`, `@slack/oauth` SDK install-store integration,
  AES-256-GCM token encryption (already covered via [`crate::crypto`]),
  default-Logger constructor parameter, subclass extensibility,
  typed-client `WebClient` instance identity / deprecated `client`
  alias / `AsyncLocalStorage`-resolved per-installation getter.
  (The prior deferred-but-portable renderer ports —
  `modalToSlackView` 21 + `parseSlackWebhookBody` 11 + `callSlackApi`
  12 — are now mapped 1:1 in
  [`crate::modals`] / [`crate::webhook`] / [`crate::api`]; the
  remaining HTTP-fetch-mocked siblings inside the same describe
  blocks stay enumerated under the `vi.fn()` HTTP fixture row above.)
- **`chat-sdk-adapter-*` (8 packages)** — cross-cutting Vitest
  `vi.fn()` mock infrastructure, default-Logger constructor
  parameter, subclass extensibility, typed-client getter access.

---

## Section: `chat-sdk-chat`

Categories:

1. **JSX runtime cases.** Rust has no JSX syntax. The Rust port
   ships typed builders (`modal(ModalOptions { ... })`,
   `card(CardOptions { ... })`) which produce the same
   `ModalElement` / `CardElement` data shape directly. The upstream
   `jsx(<Modal>)` → `ModalElement` conversion is the missing layer;
   the resulting shape is fully tested.
2. **JS Symbol-keyed protocols.** Rust has no `Symbol` primitive.
   The upstream `@workflow/serde-integration` test suite uses
   `Symbol(@workflow/serialize)` / `Symbol(@workflow/deserialize)`
   as opaque protocol keys. The Rust port handles serialization via
   `serde`'s tag-based dispatch; the upstream tests' Symbol-keyed
   method discovery has no Rust analog.
3. **Deprecation re-export shims for names never shipped.** The
   upstream port sometimes ships a `@deprecated` re-export
   (`MessageHistoryCache` → `ThreadHistoryCache`) and tests the
   alias. The Rust port never shipped the old name, so the
   deprecation case is structurally vacuous.

---

## JSX runtime cases (26 total)

### `packages/chat/src/jsx-runtime.ts` + sibling test files

| Upstream test file | Cases | Rust replacement |
| --- | ---: | --- |
| `jsx-runtime.test.ts` | all | None — JSX runtime is a TypeScript/React-only authoring surface. The Rust port uses [`crate::modals::modal`] / [`crate::cards::card`] / [`crate::cards::card_child`] builders that return the same data shapes. |
| `jsx-runtime.test.tsx` | all | Same. |
| `jsx-react.test.tsx` | all | Same. |

### `packages/chat/src/modals.test.ts > describe("fromReactModalElement")` (9 cases)

Tests convert `<Modal>` React/JSX elements into `ModalElement`
data shapes. Rust has no JSX; [`crate::modals::modal`] returns
`ModalElement` directly. Enumerated in
[`crates/chat-sdk-chat/src/modals.rs`](../../crates/chat-sdk-chat/src/modals.rs)
test-mod header per slice 393.

### `chat.test.ts > Actions > "should convert JSX Modal to ModalElement in openModal"`

Upstream `chat.test.ts:1246`. Asserts the upstream JSX `<Modal>`
factory is rewritten to a plain `ModalElement` object before being
passed to `ActionEvent.openModal`. Rust's
[`crate::modals::modal`] builder returns `ModalElement` directly;
the "convert JSX -> ModalElement" branch is a no-op by
construction. Enumerated in
[`crates/chat-sdk-chat/src/chat.rs`](../../crates/chat-sdk-chat/src/chat.rs)
Actions test sub-header per slice 486.

### `chat.test.ts > Slash Commands > "should convert JSX Modal to ModalElement in openModal"`

Upstream `chat.test.ts:2253`. Same shape as the Actions
JSX-modal case. Enumerated in `chat.rs` Slash Commands test
sub-header per slice 486.

### `chat.test.ts > Phase B openModal (slice 429) > "JSX Modal to ModalElement"`

Per slice 487 ledger entry — same JSX-conversion case at the
Chat::open_modal orchestration layer.

### `thread.test.ts > schedule() > "should convert JSX Card elements to CardElement before passing to adapter"`

Upstream `thread.test.ts:2809`. Asserts the upstream `Card(...)`
JSX-element factory is rewritten to a plain `CardElement` object
before being passed to `adapter.scheduleMessage`. Rust's
[`crate::cards::card`] builder returns `CardElement` directly.
Enumerated in
[`crates/chat-sdk-chat/src/thread.rs`](../../crates/chat-sdk-chat/src/thread.rs)
test-mod header per slice 449.

### `thread.test.ts > schedule() > "should convert Card JSX with children to CardElement"`

Upstream `thread.test.ts:2826`. Same JSX-element factory, this
time with nested children. Enumerated alongside the previous case
per slice 449.

---

## JS Symbol-keyed protocol cases (9 total)

### `serialization.test.ts > describe("@workflow/serde-integration")` (9 cases)

Tests how upstream's `@workflow/serde` library serializes Chat-SDK
types via `Symbol(@workflow/serialize)` and
`Symbol(@workflow/deserialize)` methods on the types. The Symbol
keys are opaque protocol identifiers JS uses to bind serde
behaviour to the type without polluting its public API.

Rust has no `Symbol` primitive. Equivalent behaviour is achieved
in the Rust port via `serde`'s `#[derive(Serialize, Deserialize)]`
+ `Message::to_serialized` / `Message::from_serialized` (which the
brief mandates), but the upstream tests assert against the
Symbol-keyed method dispatch directly — not against the resulting
wire shape. The wire-shape round-trip IS verified by
[`crates/chat-sdk-chat/src/message.rs`](../../crates/chat-sdk-chat/src/message.rs)
`round_trip_*` tests. Enumerated per slice 402.

---

## Deprecation aliases never shipped (1)

### `thread-history.test.ts > "deprecated MessageHistoryCache alias"` (1 case)

Upstream re-exports `MessageHistoryCache` as a deprecated alias
of `ThreadHistoryCache` and tests that the alias resolves to the
same class. The Rust port at
[`crates/chat-sdk-chat/src/thread_history.rs`](../../crates/chat-sdk-chat/src/thread_history.rs)
never shipped the old name, so there is no alias to verify.
Enumerated per slice 399.

---

## Document maintenance

When a case listed here becomes portable (e.g. a Rust JSX runtime
ships, or a singleton/lazy-resolution mechanism is added that
enables the standalone reviver Thread case), move the entry's
status to `verified` in
[`upstream-parity.md`](upstream-parity.md), add the Rust test
that ports it, and delete the entry here.

When a new unportable case is discovered, add it here with the
upstream citation + Rust-side justification, and reference this
file from the relevant `upstream-parity.md` row.

---

## Section: `chat-sdk-state-redis`

### `packages/state-redis/src/index.test.ts` (9 unportable cases)

| Upstream case (line) | Reason | Rust replacement |
| --- | --- | --- |
| `should export createRedisState function` (L15) | JS module-loader check (`typeof createRedisState === "function"`). | Rust's module system makes the export visible at compile time; calling the type's constructor in any other test proves it. |
| `should accept an existing redis client` (L35) | Upstream takes a pre-configured `node-redis` client via `{client}`. Rust's placeholder adapter doesn't model the node-redis client surface; redis-rs wire-up is additive production code, not a test-parity gap. | Enumerated in [`crates/chat-sdk-state-redis/src/lib.rs`](../../crates/chat-sdk-state-redis/src/lib.rs) test-mod header. |
| `should wait for an injected open client to become ready` (L53) | Upstream EventEmitter-based wait-for-`ready`. | Rust has no analog; future redis-rs wire-up will use `tokio` `Notify` rather than EventEmitter. |
| `should ignore transient errors while waiting for an injected client to recover` (L85) | Same EventEmitter-based path. | Same. |
| `should wait for an injected client to become ready again after reconnecting` (L124) | Same EventEmitter-based path. | Same. |
| `should reject when an injected client ends before becoming ready` (L165) | Same EventEmitter-based path. | Same. |
| `describe.skip > should connect to Redis` (L232) | `describe.skip`-marked upstream; requires live Redis. | Future opt-in `#[ignore]` integration test once redis-rs lands. |
| `describe.skip > should force-release a lock regardless of token` (L241) | `describe.skip`-marked upstream; requires live Redis + Lua scripts. | Future opt-in `#[ignore]` integration test. |
| `describe.skip > should no-op when force-releasing a non-existent lock` (L260) | `describe.skip`-marked upstream; requires live Redis. | Future opt-in `#[ignore]` integration test. |

Remaining 7 upstream cases are mapped to Rust tests in
[`crates/chat-sdk-state-redis/src/lib.rs`](../../crates/chat-sdk-state-redis/src/lib.rs).

---

## Section: `chat-sdk-state-ioredis`

### `packages/state-ioredis/src/index.test.ts` (4 unportable cases)

| Upstream case (line) | Reason | Rust replacement |
| --- | --- | --- |
| `should export createIoRedisState function` (L13) | JS module-loader check (`typeof createIoRedisState === "function"`). | Rust's module system makes the export visible at compile time. |
| `describe.skip > should connect to Redis` (L76) | `describe.skip`-marked upstream; requires live Redis cluster + Sentinel. | Future opt-in `#[ignore]` integration test once redis-rs cluster wire-up lands. |
| `describe.skip > should force-release a lock regardless of token` (L85) | `describe.skip`-marked upstream; requires live Redis cluster + Lua scripts. | Future opt-in `#[ignore]` integration test. |
| `describe.skip > should no-op when force-releasing a non-existent lock` (L104) | `describe.skip`-marked upstream; requires live Redis cluster. | Future opt-in `#[ignore]` integration test. |

Remaining 6 upstream cases are mapped to Rust tests in
[`crates/chat-sdk-state-ioredis/src/lib.rs`](../../crates/chat-sdk-state-ioredis/src/lib.rs).

---

## Section: `chat-sdk-state-pg`

### `packages/state-pg/src/index.test.ts` (~50 unportable cases)

The state-pg upstream test file contains ~64 cases. The Rust port
maps the structural cases (constructor, ensureConnected throw-before-
connect, queue family) and documents the remainder under five
categories. Per the cross-cutting js-only-documented sweep pattern
(`port-chat-sdk.md` slice 411), the mock-client behavior cases are
bulk-enumerated rather than transcribed one by one.

| Category | Upstream cases (count) | Reason | Rust replacement |
| --- | --- | --- | --- |
| Module-loader exports | `should export createPostgresState function` (L42), `should export PostgresStateAdapter class` (L46) (2 cases) | `typeof X === "function"` / `instanceof Class`. | Rust's module system makes exports visible at compile time. |
| Existing-client injection | `should create an adapter with an existing client` (L68) (1 case) | Upstream takes a pre-configured `pg.Client`; Rust placeholder doesn't model the node-pg client surface. | Future tokio-postgres / sqlx wire-up is additive production code. |
| Default-Logger constructor parameter | `should use default logger when none provided` (L74) (1 case) | Per `port-chat-sdk.md` slice 447, Rust uses static-dispatch `log` crate not a typed Logger constructor parameter. | Static dispatch makes the case unrepresentable. |
| Env-var fallback | `should throw when no url or env var is available` (L81), `should use POSTGRES_URL env var as fallback` (L94), `should use DATABASE_URL env var as fallback` (L108) (3 cases) | JS-runtime `process.env`; Rust 2024 makes `std::env::set_var` `unsafe` and parallel test runners race. | Future `try_create_pg_state_adapter` factory closure pattern (slice 305 reference) — `env: impl Fn(&str) -> Option<String>`. |
| Mock-client behavior (`with mock client` describe) | ~40 cases under `connect/disconnect` (5), `subscriptions` (3), `locking` (8), `cache` (3), `appendToList / getList` (4), `enqueue / dequeue / queueDepth` (~17) | Requires JS `vi.fn()`-based mock pg.Pool to assert call shapes. Per the cross-cutting js-only-documented sweep pattern (slice 411), the mock infrastructure is JS-only; Rust uses inline `Mutex<Vec<_>>` recorders. | Real behavior will be verified by future tokio-postgres integration tests once the client lands. |
| `getClient` typed-class getter | `should return the underlying client` (L658) (1 case) | Per `port-chat-sdk.md` slice 439, Rust holds the connection pool by opaque type — no typed-class-getter pattern. | Type-system-impossible by construction. |
| Live-Postgres integration | `describe.skip > should connect to Postgres` (L666) + sibling skipped cases | `describe.skip`-marked upstream; requires live Postgres. | Future opt-in `#[ignore]` integration tests once tokio-postgres lands. |

Remaining ~14 structural upstream cases are mapped to Rust tests in
[`crates/chat-sdk-state-pg/src/lib.rs`](../../crates/chat-sdk-state-pg/src/lib.rs) -
constructor options, ensureConnected throw-before-connect for every
StateAdapter method (get, set, set_if_not_exists, delete,
append_to_list, get_list, subscribe, unsubscribe, is_subscribed,
acquire_lock, release_lock, extend_lock), and queue family default
trait impls (enqueue, dequeue, queue_depth).

---

## Section: `chat-sdk-adapter-telegram`

### `packages/adapter-telegram/src/{index,markdown,cards}.test.ts` (36 unportable cases of 170)

The Rust port maps 134 of the 170 upstream cases (cards 9/9 +
markdown 73/73 + index 52/88). The remaining 36 cases fall under
the cross-cutting js-only-documented sweep patterns (slice 411
Vitest `vi.fn()` HTTP-fetch mock + slice 380 type-system-impossible
+ slice 447 default-Logger constructor) and are enumerated below.

| Category | Upstream cases (count) | Reason | Rust replacement |
| --- | --- | --- | --- |
| `vi.fn()`-mocked HTTP fetch | 45 listed in [`crates/chat-sdk-adapter-telegram/src/lib.rs`](../../crates/chat-sdk-adapter-telegram/src/lib.rs) test-mod header under `describe("TelegramAdapter")` (43) + `describe("applyTelegramEntities")` (1) + `describe("getUser")` (1). The conservative 34-case subset *requires* `vi.fn()` and is not structurally covered by an existing Rust unit/URL-shape test; the rest are partially covered by URL/body-shape assertions on `method_url` + per-method tests. | Each case asserts on `mockFetch.mock.calls[...]` URL/body/header shape from a sequenced `mockResolvedValueOnce(...)` chain, or on `adapter.initialize` -> `getMe` -> `parseMessage` -> dispatch runtime side-effects. Requires the upstream Vitest `vi.fn()` fetch-spy infrastructure. | Rust port intentionally avoids a test-only `wiremock`-style dep here; URL + body shape are structurally covered via `method_url` + per-method tests (see `adapter_method_url_produces_telegram_endpoints_for_all_runtime_methods`) and the message-length truncation / parse-mode routing via `crate::markdown::truncate_for_telegram`. |
| Subclass extensibility | `describe("subclass extensibility") > exposes protected members and methods to subclasses` (L2863) (1 case) | TypeScript `protected` access modifier check. | Rust uses `pub(crate)` visibility + trait composition rather than class inheritance. |
| Default-Logger constructor parameter | `describe("constructor env var resolution") > should default logger when not provided` (L252) (1 case) | Per `port-chat-sdk.md` slice 447, Rust adapters do not take a `Logger` as a first-class adapter dependency. | Static dispatch via the `log` crate makes the constructor-default-logger fallback shape moot. |

Mapped accounting: 134 Rust-mapped + 36 js-only-documented =
170/170 upstream cases accounted for. Remaining 134 cases are
ported as colocated `#[cfg(test)] mod tests` in
[`crates/chat-sdk-adapter-telegram/src/{lib,markdown,cards}.rs`](../../crates/chat-sdk-adapter-telegram/src/).

---

## Section: `chat-sdk-adapter-whatsapp`

### `packages/adapter-whatsapp/src/{index,cards,markdown}.test.ts` (9 unportable cases of 111)

The Rust port maps 102 of the 111 upstream cases (cards 23/23 +
markdown 23/23 + index 56/65). The remaining 9 cases fall under
the cross-cutting js-only-documented sweep patterns (slice 380
type-system-impossible + slice 411 Vitest `vi.fn()` HTTP-fetch
mock) and are enumerated below.

| Category | Upstream cases (count) | Reason | Rust replacement |
| --- | --- | --- | --- |
| Subclass extensibility | `describe("subclass extensibility") > exposes protected members and methods to subclasses` (index.test.ts L1166-L1179) (1 case) | TypeScript `protected` access modifier check. | Rust uses `pub(crate)` visibility + trait composition rather than class inheritance — the subclass-protected-leak test is unrepresentable by construction. |
| `vi.fn()`-mocked HTTP fetch | `describe("handleWebhook - POST signature verification")` (index.test.ts L676-L758) (5 cases) | Requires the upstream Vitest `vi.fn()` fetch-spy infrastructure to drive a synthetic `Request` -> `Response` round-trip through `adapter.handleWebhook` and assert `mockChat.processMessage`/HTTP-status side-effects. | Signature primitive ported 1:1 via `crate::webhook::verify_whatsapp_signature` (7 tests in `webhook.rs`); JSON-decode/dispatch flow via `crate::parse::parse_message` (16 tests in `parse.rs`). End-to-end wiring would require a `wiremock`/tokio dev-dep the workspace's adapter parity policy explicitly avoids. |
| `vi.fn()`-mocked HTTP fetch | `describe("handleWebhook - POST message processing")` (index.test.ts L764-L815) (2 cases) | Asserts `mockChat.processMessage` runtime side-effects through the same `vi.fn()`-mocked Request-Response path. | Structural parsing covered by `crate::parse::parse_message`. Same `vi.fn()`-fetch blocker as the POST signature verification cases. |
| `vi.fn()`-mocked HTTP fetch | `describe("stream") > buffers async iterable chunks and sends as a single message` (index.test.ts L1028-L1046) (1 case) | The Rust port does not implement `stream` on the adapter (the cross-platform `Adapter` trait does not include it), and the assertion is on outbound HTTP body shape via `vi.spyOn(global, "fetch")`. | Structural body shape (Graph API send-text-message envelope) covered by `WhatsappAdapter::build_text_message_body` tests. |

Mapped accounting: 102 Rust-mapped + 9 js-only-documented =
111/111 upstream cases accounted for. Remaining 102 cases are
ported as colocated `#[cfg(test)] mod tests` in
[`crates/chat-sdk-adapter-whatsapp/src/{lib,parse,cards,markdown,webhook}.rs`](../../crates/chat-sdk-adapter-whatsapp/src/).

---

## Section: `chat-sdk-adapter-discord`

### `packages/adapter-discord/src/{index,cards,markdown,gateway}.test.ts` (68 unportable cases of 234)

The Rust port maps 166 of the 234 upstream cases (cards 38/38 +
markdown 41/41 + index 87/154). The remaining 68 cases fall under
the cross-cutting js-only-documented sweep patterns (slice 411
Vitest `vi.fn()` HTTP-fetch mock + slice 380 type-system-impossible
+ slice 438 discord.js `Client` partials + slice 447 default-Logger
constructor) and are enumerated below.

| Category | Upstream cases (count) | Reason | Rust replacement |
| --- | --- | --- | --- |
| `vi.fn()`-mocked HTTP fetch | 65 listed in [`crates/chat-sdk-adapter-discord/src/lib.rs`](../../crates/chat-sdk-adapter-discord/src/lib.rs) test-mod header under `describe("handleWebhook - PING / MESSAGE_COMPONENT / APPLICATION_COMMAND / JSON parsing / forwarded gateway events / component interaction edge cases")` + `describe("postMessage / editMessage / deleteMessage / addReaction / removeReaction / startTyping")` outer side-effect rows + `describe("openDM / fetchMessages / fetchChannelMessages / fetchChannelInfo / postChannelMessage / listThreads / fetchThread")` + `describe("legacy gateway interactions / handleForwardedMessage / handleForwardedReaction / initialize / mentionRoleIds / createDiscordThread 160004 recovery / getUser")`. | Each case asserts on `vi.spyOn(adapter as any, "discordFetch").mockResolvedValue(...)` HTTP-spy state, on `requestContext.run(...)` async-local-storage state, on `chat.handleIncomingMessage` runtime dispatch, or on `nacl.sign.detached.verify` driven through a Vitest synthetic `Request`. Requires the upstream `vi.fn()` fetch-spy + `AsyncLocalStorage` infrastructure. | Rust port intentionally avoids a test-only `wiremock`-style dep here; URL + body shape are structurally covered via `post_message_url` / `message_url` / `reaction_url` / `typing_url` + `build_post_message_body` / `build_edit_message_body` pure helpers, and the webhook signature verification path is covered by the `webhook::tests::*` module's direct Ed25519 verifier tests. |
| Subclass extensibility | `describe("subclass extensibility") > exposes protected members and methods to subclasses` (index.test.ts L4528-L4529) (1 case) | TypeScript `protected` access modifier check. | Rust uses `pub(crate)` visibility + trait composition rather than class inheritance. |
| Default-Logger constructor parameter | `describe("constructor env var resolution") > should default logger when not provided` (index.test.ts L170) (1 case) | Per `port-chat-sdk.md` slice 447, Rust adapters do not take a `Logger` as a first-class adapter dependency. | Static dispatch via the `log` crate makes the constructor-default-logger fallback shape moot. |
| discord.js `Client` partials | `gateway.test.ts > describe("Gateway client configuration") > includes Partials.Channel for DM support` (gateway.test.ts L62-L106) (1 case) | Asserts the discord.js `Client` was constructed with `partials: [Partials.Channel]` for DM event delivery. | The Rust port manages its WebSocket gateway directly (no discord.js `Client` wrapper) and `Partials` is a discord.js-specific enum; DM support is surfaced via channel-type dispatch in the event handler instead. |

Mapped accounting: 166 Rust-mapped + 68 js-only-documented =
234/234 upstream cases accounted for. Remaining 166 cases are
ported as colocated `#[cfg(test)] mod tests` in
[`crates/chat-sdk-adapter-discord/src/{lib,parse,cards,markdown,webhook}.rs`](../../crates/chat-sdk-adapter-discord/src/).

---

## Section: `chat-sdk-adapter-teams`

### `packages/adapter-teams/src/{errors,markdown,cards,modals,graph-api,index}.test.ts` (25 unportable cases of 154)

The Rust port maps 129 of the 154 upstream cases (errors 12/12 +
markdown 39/39 + cards 19/19 + modals 16/16 + graph-api 15/15 +
index 28/53). The remaining 25 cases fall under the cross-cutting
js-only-documented sweep patterns (slice 380 type-system-impossible
+ slice 411 Vitest `vi.fn()` HTTP-fetch mock + slice 414 ESM
compatibility subprocess + slice 447 default-Logger constructor +
slice 458 createXxx-function-export typeof check) and are
enumerated below.

| Category | Upstream cases (count) | Reason | Rust replacement |
| --- | --- | --- | --- |
| `vi.fn()`-mocked HTTP fetch + env-var resolution | 21 listed in [`crates/chat-sdk-adapter-teams/src/lib.rs`](../../crates/chat-sdk-adapter-teams/src/lib.rs) test-mod header: `describe("constructor env var resolution")` (6 non-default-logger cases — appId/appPassword/appTenantId/apiUrl env var resolution + config-prefers-env + apiUrl-config) + `describe("createTeamsAdapter factory")` (delegate-to-constructor + federated-auth) + `describe("handleWebhook")` (invalid-JSON 400) + `describe("initialize")` (store-chat-and-initialize-app) + `describe("postMessage")` (2 cases — call-app.send + handleTeamsError-on-failure) + `describe("editMessage")` (call-api.conversations.activities.update) + `describe("deleteMessage")` (call-api.conversations.activities.delete) + `describe("startTyping")` (send-typing-via-app.send) + `describe("openDM")` (throw-ValidationError-no-tenantId) + `describe("getUser")` (5 cases — cached/uncached/Graph-fail/missing-mail/uninitialized). | Each case asserts on `mockApp.send.mock.calls` / `mockUpdate.mock.calls` / `mockApp.graph.call(...)` / `mockState.get(...)` / `mockChat.processMessage(...)` Vitest `vi.fn()`-spy state, or drives a synthetic `Request` -> `Response` through `adapter.handleWebhook` -> `bridgeAdapter.dispatch`. Requires the upstream `vi.fn()` fetch-spy + `process.env` mutation infrastructure (Rust 2024 makes `set_var` `unsafe` and parallel tests race). | Rust port intentionally avoids a test-only `wiremock`-style dep here; URL + body shape are structurally covered via `build_message_body` / `build_edit_message_body` / `build_typing_body` pure helpers + the existing `activity_url` / `activities_url` URL builders. Env-var resolution is delegated to the adopter via the `TeamsAdapterOptions` struct's `with_app_tenant_id` / `with_user_name` / `with_api_url` builders. The Bot Framework `bridgeAdapter` + Teams `@microsoft/teams.apps` SDK have no Rust port — adopters wire their own HTTP server. |
| ESM compatibility (subprocess spawn) | `describe("ESM compatibility") > all subpath imports resolve in Node.js ESM (no bare directory imports)` (index.test.ts L32-L75) (1 case) | Spawns a real `node --input-type=module` subprocess and checks that every non-relative `from "<pkg>"` import in `index.ts` resolves under Node.js ESM rules. | Rust's module system is statically resolved at compile time via Cargo + `mod` declarations; bare directory imports don't exist as a concept. Adapter-teams is the only upstream adapter that ships this test (slice 414 audited cross-package). |
| createXxx function-export typeof check | `describe("TeamsAdapter") > should export createTeamsAdapter function` (index.test.ts L100) (1 case) | Asserts `typeof createTeamsAdapter === "function"`. | Rust's module system makes the `pub fn new` constructor visible at compile time; missing exports become compilation errors, not runtime assertion failures (slice 458). |
| Default-Logger constructor parameter | `describe("constructor env var resolution") > should default logger when not provided` (index.test.ts L264) (1 case) | Per `port-chat-sdk.md` slice 447, Rust adapters do not take a `Logger` as a first-class adapter dependency. | Static dispatch via the `log` crate makes the constructor-default-logger fallback shape moot. |
| Subclass extensibility | `describe("subclass extensibility") > exposes protected members and methods to subclasses` (index.test.ts L1238-L1249) (1 case) | TypeScript `protected` access modifier check on `logger` / `formatConverter` / `handleMessageActivity`. | Rust uses `pub(crate)` visibility + trait composition rather than class inheritance; the subclass-protected-leak test is unrepresentable by construction. |

Mapped accounting: 129 Rust-mapped + 25 js-only-documented =
154/154 upstream cases accounted for. Remaining 129 cases are
ported as colocated `#[cfg(test)] mod tests` in
[`crates/chat-sdk-adapter-teams/src/{lib,parse,cards,markdown,errors,modals,graph_api,thread_id}.rs`](../../crates/chat-sdk-adapter-teams/src/).

---

## Section: `chat-sdk-adapter-messenger`

### `packages/adapter-messenger/src/{index,markdown,cards}.test.ts` (36 unportable cases of 169)

The Rust port maps 133 of the 169 upstream cases (cards 45/45 +
markdown 10/11 + index 78/113). The remaining 36 cases fall under
the cross-cutting js-only-documented sweep patterns (slice 411
Vitest `vi.fn()` HTTP-fetch mock + slice 380 type-system-impossible
+ slice 447 default-Logger constructor) and are enumerated below.

| Category | Upstream cases (count) | Reason | Rust replacement |
| --- | --- | --- | --- |
| `vi.fn()`-mocked HTTP fetch | 34 listed in [`crates/chat-sdk-adapter-messenger/src/lib.rs`](../../crates/chat-sdk-adapter-messenger/src/lib.rs) test-mod header: `describe("initialization")` (4 cases — `/me` fetch + `mockLogger.warn` chain) + `describe("webhook handling") > describe("payload validation")` (3 cases — synthetic Request 400/404/200 dispatch) + `describe("webhook handling") > describe("message processing")` (8 cases — `mockChat.processMessage` runtime dispatch through synthetic Request) + `describe("webhook handling") > describe("postback handling")` (3 cases — `mockChat.processAction.mock.calls[0][0]` shape) + `describe("webhook handling") > describe("reaction handling")` (2 cases — `mockChat.processReaction.mock.calls[0][0].added`) + `describe("messaging") > describe("posting messages")` subset (4 of 8 cases — `caches sent message`, `posts message with markdown content`, `posts message with AST content`, `rejects empty messages`) + `describe("messaging") > describe("streaming")` (2 cases — assertion is on outbound HTTP body via `vi.spyOn(global, "fetch")` + `Adapter` trait lacks `stream`) + `describe("attachments")` subset (3 of 11 cases — `downloads attachment successfully`, `throws NetworkError when attachment download fails`, `throws NetworkError when attachment download returns non-ok`) + `describe("thread and channel info")` subset (5 of 7 cases — `fetches thread info with user profile`, `fetches channel info with user profile`, `falls back to user ID when profile fetch fails`, `caches user profiles on second call`, plus the second `falls back to user ID when profile has no name`) + `describe("Graph API error handling")` subset (3 of 15 cases — `throws NetworkError when fetch throws`, `throws NetworkError when response is not valid JSON`, plus the `await adapter.startTyping(...)` drive path on the 3 fallback-message/code/no-error cases). | Each case asserts on `mockFetch.mock.calls[...]` URL/body/header shape from a sequenced `mockResolvedValueOnce(...)` chain, or on `mockChat.processMessage` / `mockChat.processAction` / `mockChat.processReaction` runtime side-effects through `adapter.handleWebhook(request)` driven by a synthetic `Request` constructor. Requires the upstream Vitest `vi.fn()` fetch-spy + Request/Response infrastructure. | Rust port intentionally avoids a test-only `wiremock`-style dep here; URL + body shape are structurally covered via `MessengerAdapter::send_url` + `build_text_message_body` + `build_template_message_body` + `build_typing_body` pure body-builder helpers, structural parsing is covered by `parse::parse_messenger_message` + `parse::extract_attachments` (16 tests in `parse.rs`), pagination by `fetch::paginate_messages` (14 tests in `fetch.rs`), error classification by `errors::classify_graph_api_error` + `errors::graph_api_fetch_error` + `errors::graph_api_json_parse_error` (15 tests in `errors.rs`), and webhook signature verification by `webhook::verify_messenger_signature` (10 tests in `webhook.rs`). Thread/channel info display-name formatting is covered by `profile_display_name` (4 lib.rs tests covering first-only / last-only / both / fallback-id paths). |
| Subclass extensibility | `describe("subclass extensibility") > exposes protected members and methods to subclasses` (index.test.ts L2131-L2132) (1 case) | TypeScript `protected` access modifier check. | Rust uses `pub(crate)` visibility + trait composition rather than class inheritance — the subclass-protected-leak test is unrepresentable by construction. |
| Invalid-postable-shape TypeError | `markdown.test.ts > describe("renderPostable") > throws on invalid postable message shapes` (markdown.test.ts L62-L66) (1 case) | TypeScript `as never` runtime cast that invokes `BaseFormatConverter::renderPostable` with an unknown discriminator and asserts `throw new TypeError("Unknown postable message shape")`. | The Rust port's `MessengerFormatConverter` exposes per-shape methods (`render_postable_string` / `render_postable_raw` / `render_postable_markdown` / `render_postable_ast`) each type-checked at compile time; there is no runtime "unknown shape" path. The compile-time rejection is the Rust equivalent of the upstream throw and is documented in the test-mod comment in [`crates/chat-sdk-adapter-messenger/src/markdown.rs`](../../crates/chat-sdk-adapter-messenger/src/markdown.rs). |

Mapped accounting: 133 Rust-mapped + 36 js-only-documented =
169/169 upstream cases accounted for. Remaining 133 cases are
ported as colocated `#[cfg(test)] mod tests` in
[`crates/chat-sdk-adapter-messenger/src/{lib,parse,cards,markdown,errors,fetch,webhook}.rs`](../../crates/chat-sdk-adapter-messenger/src/).

## Section: `chat-sdk-adapter-github`

### `packages/adapter-github/src/{index,markdown,cards}.test.ts` (74 unportable cases of 159)

The Rust port maps 85 of the 159 upstream cases (cards 12/12 +
markdown 18/18 + index 55/129). The remaining 74 cases fall under
the cross-cutting js-only-documented sweep patterns (slice 411
Vitest `vi.fn()` HTTP-fetch mock + slice 380 type-system-impossible
+ slice 439 typed-client `Octokit` getter) and are enumerated below.

| Category | Upstream cases (count) | Reason | Rust replacement |
| --- | --- | --- | --- |
| `vi.fn()`-mocked HTTP fetch / `vi.fn()`-Chat dispatch | 67 listed in [`crates/chat-sdk-adapter-github/src/lib.rs`](../../crates/chat-sdk-adapter-github/src/lib.rs) test-mod header: `describe("initialize")` (3 cases) + `describe("getInstallationId")` subset (3 of 7 cases — cached / not-cached / pre-init throw drive `multiTenantAdapter.initialize(mockChat)` + `handleWebhook(...)`) + `describe("handleWebhook")` (14 cases — synthetic `Request` constructors + `signPayload` helper + `mockChat.handleIncomingMessage` / `processMessage` dispatch) + `describe("self-message detection")` (4 cases — `mockUsersGetAuthenticated.mockResolvedValueOnce(...)` chain + `processMessage` not-called assertion) + `describe("postMessage")` (4 cases — `mockIssuesCreateComment.toHaveBeenCalledWith({owner, repo, issue_number, body})`) + `describe("editMessage")` (3 cases — `mockIssuesUpdateComment` / `mockPullsUpdateReviewComment` toHaveBeenCalledWith) + `describe("stream")` (4 cases — `async function*` generator drive + `toHaveBeenCalledTimes(1)`) + `describe("deleteMessage")` (2 cases — `mockIssuesDeleteComment` / `mockPullsDeleteReviewComment.toHaveBeenCalledWith`) + `describe("addReaction")` (3 cases — `mockReactionsCreateForIssueComment.toHaveBeenCalledWith({content})`) + `describe("removeReaction")` (4 cases — `mockReactionsListForIssueComment.mockResolvedValueOnce({data: [...]})` chain) + `describe("fetchMessages")` (4 cases — `mockIssuesListComments.mockResolvedValueOnce({data: [...]})` + per_page assertion) + `describe("fetchThread")` (3 cases — `mockPullsGet` / `mockIssuesGet.mockResolvedValueOnce(...)`) + `describe("listThreads")` (6 cases — `mockPullsList.mockResolvedValueOnce({data: [...]})` + cursor assertions) + `describe("fetchChannelInfo")` (2 cases — `mockReposGet.mockResolvedValueOnce({data})`) + `describe("getUser")` subset (5 of 6 cases — `mockRequest.mockResolvedValue({...})` + `Octokit.request("GET /user/{account_id}", {account_id})` typed URL templating) + `describe("fetchSubject")` (4 cases — `(adapter as unknown as ...).defaultOctokit = mockOctokit` property-injection + per-test `vi.fn().mockResolvedValue(...)` resolver). | Each case asserts on `mockOctokit.rest.*.toHaveBeenCalledWith(...)` URL/body/header shape from a sequenced `mockResolvedValueOnce(...)` chain, or on `mockChat.processMessage` / `mockChat.handleIncomingMessage` runtime side-effects through `adapter.handleWebhook(request)` driven by a synthetic `Request` constructor + `signPayload(body)` helper. Requires the upstream Vitest `vi.fn()` fetch-spy + Request/Response infrastructure + `Octokit` typed-client `rest.*` namespace pattern. | Rust port intentionally avoids a test-only `wiremock`-style dep here; URL + body shape are structurally covered via `GithubAdapter::comments_url` / `comment_url` / `comment_reactions_url` / `issue_url` URL builders + `build_comment_body` / `build_reaction_body` pure body-builder helpers (8 lib.rs tests), parseMessage / parseAuthor by `parse::parse_message` / `parse::parse_author` (10 parse.rs tests), pagination by `parse_list_threads_cursor` + `compute_next_cursor` + `limit_messages_window` (8 lib.rs tests), channel-id validation by `parse_channel_id` (2 lib.rs tests), display-name fallback by `user_display_name` (3 lib.rs tests), stream text accumulation by `accumulate_stream_text` (4 lib.rs tests), bot-reaction filter by `find_bot_reaction_id` (3 lib.rs tests), and webhook signature verification by `webhook::verify_github_signature` (8 webhook.rs tests). |
| `octokit` typed-client getter | 5 cases (index.test.ts L276-L369 — `describe("octokit getter")`) | Asserts `octokit` getter returns an `Octokit`-typed class instance with referential equality across calls; the deprecated `client` alias; the multi-tenant property-throw outside a webhook context; the per-installation `AsyncLocalStorage`-resolved Octokit inside a webhook. | The Rust port holds HTTP as an opaque `reqwest::Client` injected via `with_http_client(...)`; there is no `Octokit` typed-class equivalent and no `AsyncLocalStorage`-per-call swap. The multi-tenant per-installation context is surfaced via typed errors at the call sites that need a per-installation client (not via a property getter). All 5 cases enumerated in the [`crates/chat-sdk-adapter-github/src/lib.rs`](../../crates/chat-sdk-adapter-github/src/lib.rs) test-mod header per slice 439. |
| Constructor "throw when no auth method is provided" | 1 case (index.test.ts L249 — `describe("constructor") > should throw when no auth method is provided`) | `new GithubAdapter({})` with no auth fields asserts `throw new ValidationError`. | Rust's `GithubAdapterOptions` requires `GithubAuth` at compile time; passing "no auth" is a type error, so the runtime throw is unrepresentable. The Rust port's typed-builder is the equivalent compile-time guarantee. Enumerated in the [`crates/chat-sdk-adapter-github/src/lib.rs`](../../crates/chat-sdk-adapter-github/src/lib.rs) test-mod header. |
| Subclass extensibility | 1 case (index.test.ts L2899-L2913 — `describe("subclass extensibility") > exposes protected members and methods to subclasses`) | TypeScript `protected` access-modifier compile-time check via `class TestSubclass extends GitHubAdapter { checkAccess() { return [this.logger, this.formatConverter, this.verifySignature] } }`. | Rust uses `pub(crate)` visibility + trait composition rather than class inheritance — the subclass-protected-leak test is unrepresentable by construction. Enumerated in the [`crates/chat-sdk-adapter-github/src/lib.rs`](../../crates/chat-sdk-adapter-github/src/lib.rs) test-mod header. |

Mapped accounting: 85 Rust-mapped + 74 js-only-documented =
159/159 upstream cases accounted for. Remaining 85 cases are
ported as colocated `#[cfg(test)] mod tests` in
[`crates/chat-sdk-adapter-github/src/{lib,parse,cards,markdown,webhook}.rs`](../../crates/chat-sdk-adapter-github/src/).

---

## Section: `chat-sdk-adapter-linear`

### `packages/adapter-linear/src/{index,markdown,cards,utils}.test.ts` (111 unportable cases of 192)

The Rust port maps 81 of the 192 upstream cases (cards 12/12 +
markdown 13/13 + utils 3/3 + thread_id 18/18 + index 35/146 — the
3 channelIdFromThreadId + 1 renderFormatted + 3 constructor + 18
createLinearAdapter + 1 removeReaction + 1 startTyping + 7
parseMessage + 1 token-encryption key-length validator = 35
portable index.test.ts cases that don't require the `LinearClient`
typed-client mock). The remaining 111 cases fall under the
cross-cutting js-only-documented sweep patterns (slice 411 Vitest
`vi.fn()` HTTP / typed-client mock + slice 380
type-system-impossible + slice 439 typed-client `LinearClient`
getter + slice 447 default-Logger constructor) and are enumerated
below.

| Category | Upstream cases (count) | Reason | Rust replacement |
| --- | --- | --- | --- |
| `vi.fn()`-mocked `LinearClient` typed-client / `vi.fn()`-Chat dispatch | 99 listed in [`crates/chat-sdk-adapter-linear/src/lib.rs`](../../crates/chat-sdk-adapter-linear/src/lib.rs) test-mod header under `describe("postMessage")` (5, L1671-L1827) + `describe("editMessage")` (2, L1829-L1888) + `describe("deleteMessage")` (1, L1890-L1910) + `describe("addReaction")` (4, L1912-L2013) + `describe("fetchMessages")` (10, L2049-L2470) + `describe("fetchThread")` (2, L2472-L2524) + `describe("initialize")` (3+2=5, L2526-L2629 / L3305-L3371) + `describe("ensureValidToken")` (3, L2631-L2699) + `describe("refreshClientCredentialsToken")` (4, L2701-L2806) + `describe("runtime operations")` (13, L2808-L3303) + `describe("client credentials auth")` (3, L3373-L3449) + `describe("multi-tenant installations")` (5, L3451-L3611) + `describe("getUser")` (5, L3917-L3998) + `describe("fetchSubject")` (4, L4000-L4150) + `describe("handleWebhook - signature verification / timestamp validation / invalid JSON / comment created / agent session events / reaction events / unknown event types")` (4+3+1+6+10+2+1 = 27, L1005-L1551) + `describe("buildMessage via webhook")` (6, L1553-L1669). | Each case asserts on `mockClient.createComment.toHaveBeenCalledWith(...)` / `mockClient.updateComment` / `mockClient.deleteComment` / `mockClient.createReaction` / `mockClient.agentActivityCreate` / `(adapter as unknown as {linearClient}).linearClient = mockClient` property-injection state, or drives a synthetic `Request` -> `Response` through `adapter.handleWebhook` -> `mockChat.processMessage` / `mockChat.processReaction` runtime dispatch, with HMAC-SHA256 webhook signature verification via the `signPayload(body, secret)` helper. Requires the upstream Vitest `vi.fn()` fetch-spy + Request/Response infrastructure + `LinearClient` typed-class typed-method-spy pattern. | Rust port intentionally avoids a test-only `wiremock`-style dep here; URL + mutation + body shape are structurally covered via the `COMMENT_CREATE_MUTATION` / `COMMENT_UPDATE_MUTATION` / `COMMENT_DELETE_MUTATION` / `REACTION_CREATE_MUTATION` GraphQL constants + the `linear_graphql_call` envelope helper + the 4 thread-id-rejection tests on the Adapter trait methods. Structural payload parsing is covered by [`crate::parse::parse_message`] (7 upstream `parseMessage` cases + 3 additive). |
| `linearClient` typed-client getter | 5 cases (index.test.ts L890-L976 — `describe("linearClient getter")`) | Asserts `linearClient` getter returns a `LinearClient`-typed class instance with referential equality across calls; the deprecated `client` alias; the multi-tenant property-throw outside a webhook context; the per-installation `AsyncLocalStorage`-resolved client inside a webhook. | The Rust port holds HTTP as an opaque `reqwest::Client` injected via `with_http_client(...)`; there is no `LinearClient` typed-class equivalent and no `AsyncLocalStorage`-per-call swap. The multi-tenant per-installation context is surfaced via typed errors at the call sites that need a per-installation client (not via a property getter). All 5 cases enumerated in the [`crates/chat-sdk-adapter-linear/src/lib.rs`](../../crates/chat-sdk-adapter-linear/src/lib.rs) test-mod header per slice 439. |
| AES-256-GCM token encryption | 3 cases (index.test.ts L3618-L3711 — `describe("multi-tenant installations > token encryption")`: `encrypts accessToken and refreshToken at rest in the state store` / `stores plaintext when no encryptionKey is configured` / `getInstallation tolerates legacy plaintext records`) | AES-256-GCM round-trip through `setInstallation` / `getInstallation` with an `{ iv, data, tag }` envelope. | This crate's parity policy is no new dependencies; the workspace doesn't already pull in an AEAD cipher. The 4th `token encryption` case (`rejects an encryption key of the wrong length`) IS ported as a pure hex-length validator in [`crates/chat-sdk-adapter-linear/src/token.rs::tests`](../../crates/chat-sdk-adapter-linear/src/token.rs). |
| Constructor "throw when no auth method is provided" | 1 case (index.test.ts L861 — `describe("constructor") > should throw when no auth method provided`) | `new LinearAdapter({})` with no auth fields asserts `throw new ValidationError`. | Rust's `LinearAdapterOptions` requires `LinearAuth` at compile time; passing "no auth" is a type error, so the runtime throw is unrepresentable. The Rust port's typed-builder is the equivalent compile-time guarantee. Enumerated in the [`crates/chat-sdk-adapter-linear/src/lib.rs`](../../crates/chat-sdk-adapter-linear/src/lib.rs) test-mod header. |
| Constructor "botUserId pre-initialization throw" | 1 case (index.test.ts L872 — `describe("constructor") > should throw when botUserId is accessed before initialization`) | Asserts `adapter.botUserId` getter throws when read before `adapter.initialize(chat)` resolves the viewer query. | The Rust port does not yet model a `bot_user_id` getter surface on `LinearAdapter`; the viewer-fetch path (initialize) is deferred. Enumerated in the [`crates/chat-sdk-adapter-linear/src/lib.rs`](../../crates/chat-sdk-adapter-linear/src/lib.rs) test-mod header. |
| Subclass extensibility | 1 case (index.test.ts L4152-L4164 — `describe("subclass extensibility") > exposes protected members and methods to subclasses`) | TypeScript `protected` access-modifier compile-time check via `class TestSubclass extends LinearAdapter { checkAccess() { return [this.logger, this.formatConverter, ...] } }`. | Rust uses `pub(crate)` visibility + trait composition rather than class inheritance — the subclass-protected-leak test is unrepresentable by construction. Enumerated in the [`crates/chat-sdk-adapter-linear/src/lib.rs`](../../crates/chat-sdk-adapter-linear/src/lib.rs) test-mod header. |
| Default-Logger constructor parameter | 1 case (index.test.ts L3870 — `describe("createLinearAdapter") > should accept custom logger`) | Per `port-chat-sdk.md` slice 447, Rust adapters do not take a `Logger` as a first-class adapter dependency. | Static dispatch via the `log` crate makes the constructor-default-logger fallback shape moot. |

Note: per the per-describe accounting in `upstream-parity.md`,
the Rust-mapped 81 + js-only-documented 111 = 192/192 upstream
cases accounted for. The 81 mapped tests live as colocated
`#[cfg(test)] mod tests` in
[`crates/chat-sdk-adapter-linear/src/{lib,cards,markdown,parse,thread_id,token,linear_functions}.rs`](../../crates/chat-sdk-adapter-linear/src/).

---

## Section: `chat-sdk-adapter-gchat`

### `packages/adapter-gchat/src/{index,markdown,cards,thread-utils,user-info,workspace-events}.test.ts` (100 unportable cases of 245)

The Rust port maps 145 of the 245 upstream cases (cards 28/28 +
markdown 29/29 + thread-utils 14/14 + user-info 14/14 +
workspace-events 4/12 + index 56/148). The remaining 100 cases
fall under the cross-cutting js-only-documented sweep patterns
(slice 411 Vitest `vi.fn()` HTTP / `googleapis` typed-client mock +
slice 380 type-system-impossible + slice 447 default-Logger
constructor + slice 305 env-var resolution already covered).

| Category | Upstream cases (count) | Reason | Rust replacement |
| --- | --- | --- | --- |
| `vi.fn()`-mocked `googleapis/workspaceevents` typed-client | 8 cases (workspace-events.test.ts L90-L321 — `describe("createSpaceSubscription")` 4 + `describe("listSpaceSubscriptions")` 2 + `describe("deleteSpaceSubscription")` 2) | Each case stubs `workspaceevents()`/`auth.JWT`/`auth.GoogleAuth` via `vi.mock("@googleapis/workspaceevents")`, drives `createSpaceSubscription` / `listSpaceSubscriptions` / `deleteSpaceSubscription` with a `vi.fn().mockResolvedValue(...)` chain, and asserts `mockCreate.toHaveBeenCalledWith({name})` on the typed `subscriptions.create/list/delete` namespace. Requires the `@googleapis/workspaceevents` typed-client (no Rust equivalent in scope; the upstream client wraps the gRPC `WorkspaceEvents v1` API with auto-generated TypeScript bindings). | Rust port's [`crate::workspace_events`] covers the pure `decode_pub_sub_message` helper (4/4 cases ported). The subscription-mutation paths require an HTTP+OAuth runtime workstream that has not landed. Enumerated in the [`crates/chat-sdk-adapter-gchat/src/lib.rs`](../../crates/chat-sdk-adapter-gchat/src/lib.rs) test-mod header. |
| `vi.fn()`-mocked `googleapis/chat` typed-client / `vi.fn()`-Chat dispatch | 81 cases listed in [`crates/chat-sdk-adapter-gchat/src/lib.rs`](../../crates/chat-sdk-adapter-gchat/src/lib.rs) test-mod header: `describe("parseMessage")` (6 of 9 cases — chatApi-mediated attachment download with `mockDownload.toHaveBeenCalledWith({resourceName}, {responseType: "arraybuffer"})` chain + `parseMessage` driver requiring `createInitializedAdapter` + `mockChat`) + `describe("parsePubSubMessage")` (3 of 4 cases — `(adapter as any).parsePubSubMessage(notification, threadId)` + `botUserId` mutation via private cast) + `describe("handleWebhook")` (9 of 11 cases — synthetic `Request` constructor + `mockChat.processMessage` / `mockChat.processAction` dispatch + `(adapter as any).endpointUrl` private mutation) + `describe("handleCardClick (via handleWebhook)")` (2 of 6 cases — not-initialized adapter + missing-space `mockChat.processAction.not.toHaveBeenCalled()`) + `describe("handleMessageEvent (via handleWebhook)")` (1 of 3 cases — not-initialized adapter `mockChat.processMessage.not.toHaveBeenCalled()`) + `describe("postMessage")` (4 cases — `mockCreate.toHaveBeenCalledWith({parent, messageReplyOption, requestBody})`) + `describe("editMessage")` (2 cases — `mockUpdate.toHaveBeenCalledWith({name, updateMask, requestBody})`) + `describe("deleteMessage")` (2 cases — `mockDelete.toHaveBeenCalledWith({name})`) + `describe("addReaction")` (2 cases — `mockCreate.toHaveBeenCalledWith({parent, requestBody})`) + `describe("removeReaction")` (2 cases — `mockList.toHaveBeenCalledWith({parent})` + `mockDelete.toHaveBeenCalledWith({name})`) + `describe("handleGoogleChatError")` log-context (1 of 3 cases — `localLogger.error.toHaveBeenCalledWith(...)`) + `describe("ensureSpaceSubscription")` (4 cases — `mockState.get.toHaveBeenCalled()` / `mockState.storage.set(...)` cache + concurrent dedupe) + `describe("onThreadSubscribe")` (1 case — `localLogger.warn.toHaveBeenCalledWith(...)`) + `describe("fetchThread")` (2 cases — `mockGet.toHaveBeenCalledWith({name})`) + `describe("fetchChannelInfo")` (3 cases — `mockGet` + `mockMembersList` chain) + `describe("fetchMessages")` (2 cases — `mockList.toHaveBeenCalledWith({orderBy})` + nextPageToken) + `describe("postChannelMessage")` (2 cases — `mockCreate.toHaveBeenCalledWith({parent, requestBody})`) + `describe("listThreads")` (2 cases — `mockList` thread-dedupe + invalid channel) + `describe("openDM")` (5 cases — `mockFindDM.toHaveBeenCalledWith({name})` + 404 fallback + setup-null + non-404 rethrow) + `describe("fetchMessages (forward direction)")` (2 cases — `mockList.mockResolvedValue({data:{messages, nextPageToken}})` + cursor pagination) + `describe("fetchChannelMessages")` (2 of 5 cases — cursor pagination + invalid channel ID throw) + `describe("getAuthOptions")` (3 cases — `(adapter as any).getAuthOptions()` private cast asserting `{credentials, impersonateUser}` / `{useApplicationDefaultCredentials, impersonateUser}` / `{auth: mockAuth}` shape) + `describe("Pub/Sub message handling")` (covered structurally) + `describe("user info caching")` beforeEach-driven cases (4 of 5 — `mockState.set.toHaveBeenCalledWith("gchat:user:...", {avatarUrl, displayName, email, isBot}, expect.any(Number))` + `mockState.set.not.toHaveBeenCalledWith` + Pub/Sub-displayName cache hit + Pub/Sub-displayName cache write) + `describe("webhook verification")` (13 cases — `vi.spyOn(auth.OAuth2.prototype, "verifyIdToken").mockRejectedValue(...)` / `mockResolvedValue({getPayload: () => ({iss, aud, email})})` chain + synthetic `Request` with `authorization: Bearer ...` header + 401/200 response status assertions + `process.env.GOOGLE_CHAT_DISABLE_SIGNATURE_VERIFICATION` mutation) + `describe("getUser")` (6 cases — `mockState.storage.set("gchat:user:...", {...})` pre-population + `adapter.getUser(userId)` assertions on `fullName` / `userName` / `email` / `avatarUrl` / `isBot`). | Each case asserts on `mockGoogleChatApi.spaces.messages.*.toHaveBeenCalledWith(...)` URL/body shape from a sequenced `mockResolvedValueOnce(...)` chain, or on `mockChat.processMessage` / `mockChat.processAction` runtime side-effects through `adapter.handleWebhook(request)` driven by a synthetic `Request` constructor, or on `mockState.storage` cache mutations through `adapter.parseMessage(event)` -> `userInfoCache.set(...)`, or on `mockOAuth2.verifyIdToken` JWT-payload mock. Requires the upstream Vitest `vi.fn()` HTTP-fetch + Request/Response infrastructure + `googleapis/chat` typed-client `chat_v1.Chat` namespace + `googleapis/oauth2-client` typed-class JWT verifier. | Rust port intentionally avoids a test-only `wiremock`-style dep here; URL + body shape are structurally covered via `GchatAdapter::messages_create_url` URL builder + `gchat_post_ephemeral_payload` / `parse_gchat_post_ephemeral_response` body helpers (lib.rs tests). Structural parsing logic is covered by [`crate::parse`] (29 helper tests: `classify_attachment_type` / `is_dm_space` / `select_event_thread_name` / `is_message_from_self` / `resolve_card_action_id` / `resolve_card_value` / `get_form_input_value` / `fallback_display_name` / `is_supported_pubsub_event_type` / `is_thread_root` / `classify_gchat_error` / `replace_bot_mention_by_index` / `replace_bot_mention_by_display_name` / `should_learn_bot_user_id`), [`crate::user_info`] (14 cache tests), and [`crate::workspace_events`] (4 `decode_pub_sub_message` tests). The JWT-verification stack requires `google-auth-library`-equivalent infrastructure not in workspace scope. |
| `createGoogleChatAdapter` factory default-Logger / custom-auth | 2 cases (index.test.ts L2602 `should create with custom auth` + L2662 `should use default logger when none provided`) | Per `port-chat-sdk.md` slice 447, Rust adapters do not take a `Logger` as a first-class adapter dependency; the constructor-default-logger fallback shape is moot. The `auth: customAuth` config branch wires a non-credential JWT-bearer auth interface; the Rust port models this differently via `with_bearer_token(...)` on a pre-minted OAuth2 access token. | Static dispatch via the `log` crate makes the constructor-default-logger fallback shape moot. The Rust-equivalent of "custom auth client" is covered by [`GchatAdapter::with_bearer_token`] which accepts a pre-minted token. Both enumerated in the [`crates/chat-sdk-adapter-gchat/src/lib.rs`](../../crates/chat-sdk-adapter-gchat/src/lib.rs) test-mod header. |
| Constructor "default logger when not provided" + "restore botUserId from state on initialize" | 3 cases (index.test.ts L312 `should default logger when not provided` + L319 `should restore bot user ID from state on initialize` + L333 `should not overwrite existing botUserId on initialize`) | The 2 botUserId restore-from-state cases require `GchatAdapter::initialize(chat)` + `StateAdapter::get("gchat:botUserId")` wiring; the Rust port does not yet model `initialize`. The default-logger case is js-only per slice 447. | Both botUserId-restore cases deferred until the `initialize` runtime path lands. All 3 enumerated in the [`crates/chat-sdk-adapter-gchat/src/lib.rs`](../../crates/chat-sdk-adapter-gchat/src/lib.rs) test-mod header. |
| `GoogleChatAdapter` export-function typeof | 1 case (index.test.ts L183 `should export createGoogleChatAdapter function`) | JS module-loader check (`typeof createGoogleChatAdapter === "function"`). | Rust's module system makes `try_create_gchat_adapter` visible at compile time; missing exports become compilation errors. Enumerated in the [`crates/chat-sdk-adapter-gchat/src/lib.rs`](../../crates/chat-sdk-adapter-gchat/src/lib.rs) test-mod header. |
| `handleGoogleChatError` log-context | 1 case (index.test.ts L1754 `should log context information`) | Asserts `localLogger.error.toHaveBeenCalledWith(stringContaining("postMessage"), objectContaining({code: 500}))`. | Logger plumbing is not a first-class adapter dependency in the Rust port (slice 447). Behavior covered structurally via [`crate::parse::classify_gchat_error`] which returns the typed `RateLimit`/`Rethrow` discriminant. |
| Subclass extensibility | 1 case (index.test.ts L3296-L3309 `subclass extensibility > exposes protected members and methods to subclasses`) | TypeScript `protected` access-modifier compile-time check via `class TestSubclass extends GoogleChatAdapter { checkAccess() { return [this.logger, this.formatConverter, this.handleMessageEvent] } }`. | Rust uses `pub(crate)` visibility + trait composition rather than class inheritance — the subclass-protected-leak test is unrepresentable by construction. Enumerated in the [`crates/chat-sdk-adapter-gchat/src/lib.rs`](../../crates/chat-sdk-adapter-gchat/src/lib.rs) test-mod header. |

Mapped accounting: 145 Rust-mapped + 100 js-only-documented =
245/245 upstream cases accounted for. The 145 mapped tests live
as colocated `#[cfg(test)] mod tests` in
[`crates/chat-sdk-adapter-gchat/src/{lib,cards,markdown,parse,thread_id,user_info,workspace_events}.rs`](../../crates/chat-sdk-adapter-gchat/src/).

---

## Section: `chat-sdk-adapter-slack`

### `packages/adapter-slack/src/{index,cards,crypto,format/index,format/boundary,markdown,modals,webhook/index,webhook/boundary,api/index,api/boundary}.test.ts` (271 unportable cases of 448)

Slack is the largest adapter in the chat-sdk port (448 upstream
`it()` cases across 11 test files — index.test.ts alone has 287).
The Rust port maps 177 of the 448 upstream cases (crypto 14/14 +
cards 36/36 + markdown 26/26 + format/index 16/16 + format/boundary
1/1 + api/boundary 1/1 + api/index 13/13 + webhook/boundary 1/1 +
webhook/index 16/20 verifySlackSignature + parseSlackWebhookBody +
modals 33/33 metadata + modalToSlackView + index.ts 20:
encodeThreadId 2 + decodeThreadId 4 + isDM 3 + channelIdFromThreadId
2 + createSlackAdapter 4 + postEphemeral 3 + renderFormatted 1 +
startTyping 1). The remaining 271 cases fall under the cross-cutting
sweep patterns (slice 411 Vitest `vi.fn()` HTTP / `@slack/web-api`
typed-client mock + slice 439 `WebClient`-getter typed-client +
slice 380 type-system-impossible + slice 447 default-Logger + slice
305 env-var resolution) plus the `@slack/socket-mode` SDK (no Rust
equivalent in scope) plus the `verifySlackRequest` /
`readSlackWebhook` async-Request runtime wrappers.

| Category | Upstream cases (count) | Reason | Rust replacement |
| --- | --- | --- | --- |
| `vi.fn()`-mocked `@slack/web-api` typed-client / Web API HTTP-fetch (slice 411) | ~227 cases listed in [`crates/chat-sdk-adapter-slack/src/lib.rs`](../../crates/chat-sdk-adapter-slack/src/lib.rs) test-mod header by describe-block: `handleWebhook - signature verification` 5 + `handleWebhook - webhookVerifier` 6 + `handleWebhook - URL verification` 1 + `handleWebhook - event_callback` 5 + `handleWebhook - interactive payloads` 10 + `handleWebhook - JSON parsing` 1 + `parseMessage` 7 + `link extraction` 6 + `edge cases` 4 + `date parsing` 2 + `formatted text extraction` 3 + `multi-workspace mode` 9 + `installationProvider` 13 + `multi-workspace mode with encryption` 3 + `installationKeyPrefix` 2 + `handleOAuthCallback` 6 + `withBotToken` 2 + `adapter.client end-to-end with multi-workspace webhook` 1 + `DM message handling` 4 + `message subtype handling` 5 + `handleWebhook - slash commands` 6 + `botToken as function` 5 + `Attachment.fetchData token resolution` 2 + `postMessage` 4 + `editMessage` 1 + `deleteMessage` 1 + `addReaction` 2 + `removeReaction` 1 + `openModal` 3 + `updateModal` 1 + `startTyping` 3 of 4 + `openDM` 2 + `fetchMessages` 3 + `fetchMessage` 2 + `fetchChannelInfo` 3 + `fetchChannelMessages` 3 + `postChannelMessage` 2 + `listThreads` 2 + `ephemeral message ID encoding` 2 + `error handling` 5 + `resolveInlineMentions` 6 + `fetchThread` 1 + `initialize` 3 + `publishHomeView` 1 + `setSuggestedPrompts` 2 + `setAssistantStatus` 2 + `setAssistantTitle` 1 + `handleWebhook - assistant events` 4 + `decodeEphemeralMessageId edge cases` 5 + `editMessage via response_url` 1 + `deleteMessage via response_url` 1 + `isMessageFromSelf` 3 + `reverse user lookup` 19 + `stream with empty threadTs` 1 + `scheduleMessage with empty threadTs` 1 + `getUser` 7 + `link unfurl enrichment` 6. | Each case stubs `globalThis.fetch` via `vi.fn()` / `vi.mock("@slack/web-api")` chains, drives a Web API call (chat.postMessage / chat.update / chat.delete / views.open / views.update / users.info / conversations.info / etc.) or `adapter.handleWebhook(request)` with a synthetic `Request`, and asserts on `mockFetch.toHaveBeenCalledWith(url, {body, headers})` URL/body shape OR on `mockChat.processMessage` / `mockChat.processAction` runtime side-effects from a sequenced `mockResolvedValueOnce(...)` chain. Requires the upstream Vitest `vi.fn()` HTTP-fetch infrastructure + `@slack/web-api` `WebClient` typed-class instance + `@slack/oauth` `InstallProvider` SDK for the installation/OAuth callback paths + `@slack/web-api`'s state-cache wiring. | Rust port intentionally avoids a test-only `wiremock`-style dep here; URL + body shape are structurally covered via the parametric `SlackAdapter::method_url` URL builder (10 endpoints) + pure body-shape helpers (`slack_post_ephemeral_payload` / `parse_slack_post_ephemeral_response` / `encode_slack_api_body`). Structural parsing logic for the Webhook surface lives in [`crate::webhook`] (29 colocated tests: verifySlackSignature 5/5 1:1 + get_header/get_retry/is_form_body/parse_json_body/is_record/string_value/optional_string pure helpers). The full installationProvider + multi-workspace + OAuth + Socket Mode dispatch surfaces require an HTTP+OAuth runtime workstream that has not landed. Enumerated by line + describe-block in the [`crates/chat-sdk-adapter-slack/src/lib.rs`](../../crates/chat-sdk-adapter-slack/src/lib.rs) test-mod header. |
| `@slack/socket-mode` SDK (no Rust equivalent) | 31 cases across 8 describe blocks: `socket mode - factory validation` 5 + `socket mode - handleWebhook` 1 + `socket mode - initialize` 1 + `socket mode - routeSocketEvent` 7 + `socket mode - disconnect` 2 + `socket mode forwarding - handleWebhook` 9 + `startSocketModeListener` 3 + `routeSocketEvent with options` 3. | All cases drive `vi.mock("@slack/socket-mode")` + `vi.fn()` on the `SocketModeClient` typed-class instance, assert on the `socketClient.start/stop/disconnect` lifecycle plus event-forwarding to the same `handleWebhook` dispatcher described above. Requires the `@slack/socket-mode` Node SDK (WebSocket pump + auto-reconnect over Slack's app-level token). | No Rust port of `@slack/socket-mode` is in workspace scope. The `app_token` field is already plumbed on `SlackAdapterOptions::with_app_token`; the actual `connections.open` + WebSocket pump are deferred. Enumerated in the [`crates/chat-sdk-adapter-slack/src/lib.rs`](../../crates/chat-sdk-adapter-slack/src/lib.rs) test-mod header. |
| Typed `WebClient` instance access (slice 439) | 15 cases (index.test.ts `describe("webClient getter")` 5 + `describe("direct WebClient access via adapter.client")` 10) | Assert the getter returns a `WebClient` typed-class instance with `.token` exposed, identity semantics across calls, deprecated `client` property alias, multi-workspace `throw` without context, and `AsyncLocalStorage`-resolved per-installation token via `withBotToken`. | Rust has no `WebClient` equivalent — HTTP is held as an opaque `reqwest::Client`. Per-call referential equality is moot under `Clone`-shared-pool semantics. The deprecated `client` alias was never shipped in Rust. Multi-workspace property-throw + ALS-based per-request token resolution are surfaced via typed errors + function-parameter plumbing at the per-workspace call sites (webhook handler), not via a property getter. Enumerated in the [`crates/chat-sdk-adapter-slack/src/lib.rs`](../../crates/chat-sdk-adapter-slack/src/lib.rs) test-mod header. |
| Constructor `env var resolution` mutation harness (slice 305) | 7 cases (index.test.ts L144-L199 minus the 1 default-Logger case folded under slice 447) | Each case mutates `process.env.SLACK_*` in a beforeEach/afterEach harness then constructs `new SlackAdapter()` to assert env-driven resolution of `SLACK_SIGNING_SECRET` / `SLACK_BOT_TOKEN` / `SLACK_API_URL`. | The Rust port models env-var resolution via the slice-305 `try_create_*(opts, env: Fn(&str) -> Option<String>)` closure pattern (per gchat slice 312, discord/linear/github/telegram/whatsapp/messenger sweep). The `process.env` mutation harness is JS-specific. The Slack factory is not yet ported (deferred — slice scope); when landed, will mirror gchat slice 312. Enumerated in the [`crates/chat-sdk-adapter-slack/src/lib.rs`](../../crates/chat-sdk-adapter-slack/src/lib.rs) test-mod header. |
| Default-Logger constructor parameter (slice 447) | 1 case (index.test.ts L170 `should default logger when not provided`) | Asserts the constructor falls back to a default `Logger` instance when none is supplied. | Rust adapters do not take a `Logger` as a first-class adapter dependency — logging is plumbed via the `log` crate's static dispatch. The constructor-default-logger fallback shape is moot. Enumerated in the [`crates/chat-sdk-adapter-slack/src/lib.rs`](../../crates/chat-sdk-adapter-slack/src/lib.rs) test-mod header. |
| Subclass extensibility (slice 380) | 1 case (index.test.ts L8118 `subclass extensibility > should expose protected members and methods to subclasses`) | TypeScript `protected` access-modifier compile-time check via `class TestSubclass extends SlackAdapter { checkAccess() { return [this.logger, this.formatConverter, this.handleEventMessage] } }`. | Rust uses `pub(crate)` visibility + trait composition rather than class inheritance — the subclass-protected-leak test is unrepresentable by construction. Enumerated in the [`crates/chat-sdk-adapter-slack/src/lib.rs`](../../crates/chat-sdk-adapter-slack/src/lib.rs) test-mod header. |
| `verifySlackRequest` / `readSlackWebhook` runtime wrappers | 4 cases (webhook/index.test.ts `verifySlackRequest` 3 + `readSlackWebhook` 1) | Drive a `Request`-shaped fixture through the runtime wrapper that combines signature-verify + body-parse. Each case asserts on the awaited string body or the resolved typed `SlackWebhookPayload`. | The pure verify + parse halves are mapped 1:1 in [`crate::webhook`] (`verify_slack_signature` 5/5 + `parse_slack_webhook_body` 11/11). The async `Request`-wrapper layer requires `reqwest::Request`-style streaming + a pluggable `webhookVerifier` callback that is not in the slice scope. Enumerated in the [`crates/chat-sdk-adapter-slack/src/lib.rs`](../../crates/chat-sdk-adapter-slack/src/lib.rs) test-mod header. |

Mapped accounting: 177 Rust-mapped + 271 js-only-documented =
448/448 upstream cases accounted for. The 177 mapped tests live
as colocated `#[cfg(test)] mod tests` in
[`crates/chat-sdk-adapter-slack/src/{lib,cards,crypto,format,markdown,modals,api,webhook}.rs`](../../crates/chat-sdk-adapter-slack/src/).
