# chat-sdk Codex Goal Runner

Use this when you want Codex CLI `/goal` to pursue full portable parity with
upstream [`vercel/chat`](https://github.com/vercel/chat) inside this repo.
The launcher creates an explicit sibling git worktree, starts Codex inside that
worktree, and copies a compact `/goal` condition to the clipboard.

```sh
cd /Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust
scripts/run-codex-goal-chat-port.sh
```

In Codex CLI, run `/goal` and paste the clipboard contents.

The launcher uses:

- `-C <worktree>` so Codex's root is the explicit worktree.
- `-m gpt-5.5`.
- `-c 'model_reasoning_effort="xhigh"'`.
- `--dangerously-bypass-approvals-and-sandbox` so it does not stop for tool
  approvals.
- `tmux` and `caffeinate` when available so it can keep running.

Worktrees are created under:

```sh
/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust-chat-worktrees
```

## Coexistence with the ai-sdk `/goal` session

This runner shares the repo and `main` branch with
[`scripts/codex-goal/`](../codex-goal/) (the Vercel AI SDK port). To run both
sessions safely in parallel:

| Concern | ai-sdk port | chat-sdk port |
| --- | --- | --- |
| Brief | [`scripts/codex-goal/port-ai-sdk.md`](../codex-goal/port-ai-sdk.md) | [`scripts/codex-goal-chat/port-chat-sdk.md`](port-chat-sdk.md) |
| Worktree root | `~/dev/andymac4182/ai-sdk-rust-goal-worktrees` | `~/dev/andymac4182/ai-sdk-rust-chat-worktrees` |
| Branch prefix | `goal/ai-sdk-port-…` | `goal/chat-sdk-port-…` |
| tmux session | `ai_sdk_rust_goal_…` | `chat_sdk_rust_goal_…` |
| Crate prefix | `ai-sdk-*` | `chat-sdk-*` |
| Ledger | `docs/upstream-parity.md` | `docs/upstream-parity-chat.md` |
| Progress table | `docs/package-progress.md` | `docs/package-progress-chat.md` |
| Estimates TSV | `docs/package-progress-estimates.tsv` | `docs/package-progress-estimates-chat.tsv` |
| Refinement log | n/a | `docs/goal-refinements-chat.md` |
| Upstream fetch | `npx opensrc@latest path github:vercel/ai` | `npx opensrc@latest path github:vercel/chat` |

**Both sessions share the same merge lock** (`/tmp/ai-sdk-rust-main-merge.lock`)
because both push to the same `main`. They will serialize merge-backs, never
deadlock, and never overwrite each other's files because their ledgers, progress
tables, estimate TSVs, briefs, and crate prefixes are all namespaced.

Neither session may edit the other's brief, ledger, estimate TSV, progress
table, worktree root, or crate tree. The chat session must not delete or
restage existing `crates/ai-sdk-*`; the ai-sdk session must not delete or
restage future `crates/chat-sdk-*`.

## Self-refining loop

After every 5 successful merge-back cycles the agent appends an entry to
`docs/goal-refinements-chat.md` summarizing what it learned (upstream surprises,
recurring blockers, mismatches between brief and reality). It then tightens
[`port-chat-sdk.md`](port-chat-sdk.md) and [`goal-condition.md`](goal-condition.md)
so the next session starts from a sharpened spec.

## Test floor (non-negotiable)

EVERY portable original upstream TypeScript test/case must exist as an equivalent
Rust test in the matching 1:1 `chat-sdk-*` crate. Rust may add more tests, but
never fewer mapped original TypeScript tests; a package with even one missing
portable upstream test/case is incomplete.

## Progress reporting

Regenerated, not hand-maintained:

```sh
scripts/package-progress-table.sh \
  --ledger docs/upstream-parity-chat.md \
  --estimates docs/package-progress-estimates-chat.tsv \
  --output docs/package-progress-chat.md
```
