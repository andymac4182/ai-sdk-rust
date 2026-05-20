# ai-sdk-rust Codex Goal Runner

Use this when you want Codex CLI `/goal` to pursue full portable parity with
upstream `vercel/ai` without GNHF. The launcher creates an explicit sibling git
worktree, starts Codex inside that worktree, and copies a compact `/goal`
condition to the clipboard.

```sh
cd /Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust
scripts/run-codex-goal-port.sh
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
/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust-goal-worktrees
```

If the main checkout has an ignored `.env.local`, the launcher symlinks it into
the worktree. That makes `AI_GATEWAY_API_KEY` and
`AI_SDK_RUST_AI_GATEWAY_API_KEY` available for integration tests without
putting secrets in git.

The goal condition tells Codex to maintain `docs/upstream-parity.md` and keep
working until every upstream package, provider, public API, example, testable
behavior, and portable feature is verified or explicitly documented as
JavaScript-only.

Future iterations must treat the original upstream TypeScript test inventory as
the floor for every package. Every portable original TypeScript test/case must
exist as an equivalent Rust test in the matching 1:1 crate before that package
can be marked verified. Rust may add more tests for Rust-specific safety,
typing, live-provider proof, and edge cases, but the Rust suite must never have
fewer portable tests than the original TypeScript package.
Put plainly: EVERY portable original TypeScript test/case must exist in Rust;
additional Rust tests are welcome, but a crate with even one fewer portable
original TypeScript test/case is incomplete.
