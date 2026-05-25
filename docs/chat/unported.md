# Intentionally Unported Cases — `chat-sdk-chat`

This registry catalogues every upstream `vercel/chat` test case that
**cannot be literally ported to Rust** and explains why. The Rust
port honours the upstream behaviour through the closest available
Rust idiom (typed builder, serde shape, type-system guarantee), but
the upstream test as written cannot be reproduced 1:1 in Rust
because it exercises a JavaScript-only language feature.

Cases listed here are considered **closed for "100% port"
purposes** per the project's
[`scripts/codex-goal-chat/goal-condition.md`](../../scripts/codex-goal-chat/goal-condition.md).
Every entry must cite the upstream file + line range and the
Rust-side replacement (where one exists).

The Rust-portable cases are tracked in
[`upstream-parity.md`](upstream-parity.md) — this file ONLY lists
the structurally-unportable cases.

## Categories

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
