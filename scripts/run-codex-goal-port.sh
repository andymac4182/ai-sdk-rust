#!/usr/bin/env bash
set -euo pipefail

main_repo="/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust"
worktree_root="/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust-goal-worktrees"
goal_file="$main_repo/scripts/codex-goal/goal-condition.md"
stamp="$(date +%Y%m%d-%H%M%S)"
branch="goal/ai-sdk-port-$stamp"
worktree="$worktree_root/ai-sdk-port-$stamp"
tmux_session="ai_sdk_rust_goal_$stamp"

if ! command -v codex >/dev/null 2>&1; then
  echo "error: codex is not installed or not on PATH. Install it and run: codex login" >&2
  exit 127
fi

cd "$main_repo"

if [[ -n "$(git status --short)" ]]; then
  echo "ai-sdk-rust main checkout is dirty. Commit or stash changes before launching Codex goal." >&2
  git status --short >&2
  exit 1
fi

git fetch origin main
git pull --ff-only origin main
mkdir -p "$worktree_root"
git worktree add -b "$branch" "$worktree" main

if [[ -f "$main_repo/.env.local" && ! -e "$worktree/.env.local" ]]; then
  ln -s "$main_repo/.env.local" "$worktree/.env.local"
fi

if command -v pbcopy >/dev/null 2>&1; then
  pbcopy < "$goal_file"
  echo "Copied compact /goal condition to clipboard: $goal_file"
else
  echo "pbcopy not found. Paste this file into /goal manually: $goal_file"
fi

echo "Created Codex goal worktree: $worktree"
echo "Branch: $branch"
echo "In Codex CLI, run /goal and paste the clipboard contents."

codex_cmd=(
  caffeinate -dimsu codex
  -C "$worktree"
  -m gpt-5.5
  -c 'model_reasoning_effort="xhigh"'
  --dangerously-bypass-approvals-and-sandbox
  -a never
)

if command -v tmux >/dev/null 2>&1; then
  printf -v tmux_cmd '%q ' "${codex_cmd[@]}"
  exec tmux new-session -A -s "$tmux_session" -c "$worktree" "$tmux_cmd"
fi

exec "${codex_cmd[@]}"
