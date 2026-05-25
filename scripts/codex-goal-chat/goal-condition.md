Goal: full Rust parity with upstream `github:vercel/chat` in this repo under `crates/chat-sdk-*`.

Read and execute `scripts/codex-goal-chat/port-chat-sdk.md` as the source of truth. Do not summarize it and stop — work the brief in coherent slices, merging each back to `main`. Re-read it whenever in doubt.

Hard rules (the Stop condition tests against these — don't claim done unless they hold):

- `git rev-parse --show-toplevel` must NOT be `/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust`. If it is, stop immediately — you must operate in a worktree.
- Stay strictly inside chat-sdk's owned files: `crates/chat-sdk-*`, `docs/chat/`, `scripts/codex-goal-chat/`. NEVER touch ai-sdk's owned files (`crates/ai-sdk-*`, `docs/upstream-parity.md`, `docs/package-progress*.md`, `docs/package-progress-estimates.tsv`, `scripts/codex-goal/`) — a concurrent `/goal` session owns them.
- Every portable upstream TypeScript test/case must have a matching Rust test in the 1:1 `chat-sdk-*` crate. More Rust tests OK; fewer mapped tests is incomplete.
- Every **unportable** upstream case (anything that exercises a JS-only language feature — JSX syntax, `Symbol`-keyed protocols, deprecated re-export aliases never shipped in Rust, etc.) must be enumerated in [`docs/chat/unported.md`](../../docs/chat/unported.md) with the upstream citation + the Rust-side replacement (or a justification for why none exists). A case is "done" when it's either ported OR listed in `unported.md`.
- Use the shared merge lock `/tmp/ai-sdk-rust-main-merge.lock` for every merge-back to `main`. Never push a dirty or unvalidated `main`.
- After every 5 merge-back cycles, append to `docs/chat/goal-refinements.md` and tighten `scripts/codex-goal-chat/port-chat-sdk.md` plus this file.

## Definition of done

"100% port" is satisfied when both of the following hold:

1. **Portable test floor.** Every upstream TypeScript test/case that does not exercise a JS-only language feature has a matching Rust test in the 1:1 `chat-sdk-*` crate. The `docs/chat/upstream-parity.md` ledger lists per-describe-block counts; "N/N portable" indicates this is met.
2. **Unportable registry.** Every upstream case that cannot be literally ported to Rust is enumerated in [`docs/chat/unported.md`](../../docs/chat/unported.md) with the upstream citation + Rust-side replacement (or justification). The registry's three top-level categories are: (a) JSX runtime cases, (b) JS Symbol-keyed protocols, (c) deprecation aliases never shipped in Rust.

When a case currently in `unported.md` becomes portable (e.g. a Rust JSX runtime ships), it should be moved from the registry into `upstream-parity.md` with a corresponding Rust test added.

The Stop condition is met when:
- `docs/chat/upstream-parity.md` shows every upstream package `verified` or `js-only-documented` and the regenerated `docs/chat/package-progress.md` reports 100% across all rows; AND
- `docs/chat/unported.md` enumerates every upstream test case that is structurally unportable to Rust with citation + replacement.
