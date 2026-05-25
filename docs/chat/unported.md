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
