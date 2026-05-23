# Vercel Chat SDK Rust Parity Ledger

_This ledger tracks the Rust port of [`vercel/chat`](https://github.com/vercel/chat) and is maintained by the `/goal` session driven from [`scripts/codex-goal-chat/`](../scripts/codex-goal-chat/)._

> Sibling project: the AI SDK port lives in [`docs/upstream-parity.md`](upstream-parity.md). That ledger is owned by a different `/goal` session — do not edit it from a chat-sdk slice.

## Upstream Source

- Repository: `github:vercel/chat`
- Fetch command: `npx opensrc@latest path github:vercel/chat`
- Inventory commit: _to be filled in on the first slice_
- Inventory date: _to be filled in on the first slice_

## Status Legend

| Status | Meaning |
| --- | --- |
| `not-started` | Upstream package/feature identified, no Rust counterpart yet. |
| `in-progress` | Some Rust scaffolding/typed contract exists; not test-parity yet. |
| `ported` | Rust implementation exists and runs deterministic fake/mock tests, but adapter live validation or full test-floor mapping is still pending. |
| `verified` | Strict 1:1 Rust crate exists, every portable upstream TypeScript test/case is mapped to a Rust test, and adapter live validation has been recorded if credentials exist. |
| `js-only-documented` | Surface is intentionally JavaScript-only (e.g. React-Native-only UI); justification is recorded below. |

## Test Floor

EVERY portable original upstream TypeScript test/case must exist as an equivalent Rust test in the matching 1:1 `chat-sdk-*` crate. Rust may add more tests, but never fewer mapped original TypeScript tests; a package with even one missing portable upstream test/case is incomplete.

## Package And Provider Inventory

_To be populated on the first slice using `npx opensrc@latest path github:vercel/chat`. Required row format (matches [`scripts/package-progress-table.sh`](../scripts/package-progress-table.sh)):_

```
| `packages/<dir>` (`<display>`) | <kind> | <status> | <rust_path> | <evidence> | <notes> |
```

The first slice must replace this section with the real inventory before
running the progress-table generator.

## Next Unported Work Queue

1. Run the first upstream scan: `npx opensrc@latest path github:vercel/chat`.
2. Record upstream commit SHA and date in **Upstream Source** above.
3. Populate **Package And Provider Inventory** with one row per upstream `packages/*` directory.
4. Seed `docs/package-progress-estimates-chat.tsv` with conservative `in-progress` rows for any package the first slice touches.
5. Identify the core/shared first-phase queue (the upstream `chat` package plus transport/state/types/test-support packages) and order them ahead of any adapter slices.

## Test-Case Parity Map

_Populated as each upstream test file is mapped. Format: one row per upstream test/case, columns `Upstream file:case`, `Rust crate::test`, `Status`, `Notes`._

## Adapter Live Validation Log

_Populated as adapters gain credential-gated live tests/examples. Format: adapter, test/example name, date last run, result, notes._
