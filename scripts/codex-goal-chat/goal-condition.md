Goal: full Rust parity with upstream `github:vercel/chat` in this repo under `crates/chat-sdk-*`.

Read and execute `scripts/codex-goal-chat/port-chat-sdk.md` as the source of truth. Do not summarize it and stop — work the brief in coherent slices, merging each back to `main`. Re-read it whenever in doubt.

Hard rules (the Stop condition tests against these — don't claim done unless they hold):

- `git rev-parse --show-toplevel` must NOT be `/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust`. If it is, stop immediately — you must operate in a worktree.
- Stay strictly inside chat-sdk's owned files: `crates/chat-sdk-*`, `docs/chat/`, `scripts/codex-goal-chat/`. NEVER touch ai-sdk's owned files (`crates/ai-sdk-*`, `docs/upstream-parity.md`, `docs/package-progress*.md`, `docs/package-progress-estimates.tsv`, `scripts/codex-goal/`) — a concurrent `/goal` session owns them.
- Every portable upstream TypeScript test/case must have a matching Rust test in the 1:1 `chat-sdk-*` crate. More Rust tests OK; fewer mapped tests is incomplete.
- Use the shared merge lock `/tmp/ai-sdk-rust-main-merge.lock` for every merge-back to `main`. Never push a dirty or unvalidated `main`.
- After every 5 merge-back cycles, append to `docs/chat/goal-refinements.md` and tighten `scripts/codex-goal-chat/port-chat-sdk.md` plus this file.

Done only when `docs/chat/upstream-parity.md` shows every upstream package `verified` or `js-only-documented` and the regenerated `docs/chat/package-progress.md` reports 100% across all rows.
