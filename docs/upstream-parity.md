# Upstream Parity Ledger

This ledger is maintained by long-running Codex `/goal` sessions.

Codex must update this file while porting the upstream
[`vercel/ai`](https://github.com/vercel/ai) repository to Rust. The goal is full
portable feature parity, not a single progress slice.

## Inventory Rules

- Record the upstream commit SHA/date used for each inventory pass.
- List every upstream package, provider package, utility library, framework
  adapter, example, testable behavior, public API, and feature.
- Use one of these statuses for each row: `not-started`, `in-progress`,
  `ported`, `verified`, `js-only-documented`.
- A row may be `verified` only when there is a Rust equivalent plus tests,
  examples, or documented validation evidence.
- A row may be `js-only-documented` only when the behavior is truly not
  portable to Rust and the Rust-facing alternative is documented.
- Do not remove upstream items just because they are hard or large.

## Latest Upstream Inventory

| Field | Value |
| --- | --- |
| Upstream repo | `vercel/ai` |
| Inventory command | `npx opensrc@latest path github:vercel/ai` |
| Upstream commit | `TODO` |
| Inventory date | `TODO` |

## Package And Provider Inventory

Codex must replace this placeholder with an exhaustive upstream package list.

| Upstream item | Kind | Status | Rust path | Evidence | Notes |
| --- | --- | --- | --- | --- | --- |
| `TODO` | package/provider/api/example/test | not-started | `TODO` | `TODO` | Inventory upstream first. |

## High-Level API Inventory

Track all portable public APIs here, including generate text/object/image,
streaming, tools, structured output, embeddings, transcription, speech, video,
reranking, files, registry/gateway support, middleware, provider utilities,
warnings/errors, and prompt/message parts.

| Upstream API | Status | Rust path | Evidence | Notes |
| --- | --- | --- | --- | --- |
| `TODO` | not-started | `TODO` | `TODO` | Inventory upstream first. |

## Next Unported Work Queue

1. Inventory upstream `vercel/ai` packages/providers/APIs/examples/tests.
2. Replace the placeholder rows above with the real exhaustive ledger.
3. Pick the highest-value `not-started` item and port it with validation.
