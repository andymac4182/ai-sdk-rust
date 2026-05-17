# ai-sdk-rust Codex Goal Runner

Use this when you want Codex CLI `/goal` to continue the port without GNHF.
The launcher creates an explicit sibling git worktree, starts Codex inside that
worktree, and copies a compact `/goal` condition to the clipboard.

```sh
cd /Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust
scripts/run-codex-goal-port.sh
```

In Codex CLI, run `/goal` and paste the clipboard contents.

The launcher uses:

- `-C <worktree>` so Codex's root is the explicit worktree.
- `-m gpt-5.5`.
- `-c 'model_reasoning_effort="xhigh"'`.
- `--dangerously-bypass-approvals-and-sandbox -a never` so it does not stop for
  tool approvals.
- `tmux` and `caffeinate` when available so it can keep running.

Worktrees are created under:

```sh
/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust-goal-worktrees
```

If the main checkout has an ignored `.env.local`, the launcher symlinks it into
the worktree. That makes `AI_GATEWAY_API_KEY` and
`AI_SDK_RUST_AI_GATEWAY_API_KEY` available for integration tests without
putting secrets in git.
