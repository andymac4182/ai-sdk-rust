Run the chat-sdk-rust full Vercel Chat SDK parity goal.

Main checkout: `/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust`
Full brief in this worktree: `scripts/codex-goal-chat/port-chat-sdk.md`

First confirm `git rev-parse --show-toplevel` is NOT the main checkout path
above. If it is, stop immediately. Then read the full brief from the current
worktree and follow it as the source of truth. Do not summarize it and stop.
Execute it.

Goal: use the current working directory as your worktree and keep working until
this repo contains Rust equivalents for EVERY package, adapter, library,
example, testable behavior, and feature in upstream `vercel/chat`, except
JavaScript-only surfaces that are explicitly documented as intentionally
non-portable. All chat-sdk Rust crates live under `crates/chat-sdk-*` and never
touch `crates/ai-sdk-*` or any other unrelated tree.

Use `npx opensrc@latest path github:vercel/chat` (or `npx opensrc fetch
github:vercel/chat`) as the upstream source of truth. First build/update
`docs/upstream-parity-chat.md`: record upstream commit/package inventory, every
adapter package, every core/helper/library package, public APIs, examples,
tests, and feature status. Do not mark the goal complete while any ledger row
is unported, unverified, or undocumented. Re-scan upstream often. Maintain
`docs/package-progress-estimates-chat.tsv` for every package you touch while it
remains `in-progress`, then run

```sh
scripts/package-progress-table.sh \
  --ledger docs/upstream-parity-chat.md \
  --estimates docs/package-progress-estimates-chat.tsv \
  --output docs/package-progress-chat.md
```

and use that generated table for progress reporting. Do not hand-maintain
package progress summaries; keep the ledger and estimate TSV current.

Non-negotiable test floor: EVERY portable original upstream TypeScript test/case
must exist as an equivalent Rust test in the matching 1:1 `chat-sdk-*` crate.
Rust may add more tests, but never fewer mapped original TypeScript tests; a
package with even one missing portable upstream test/case is incomplete.
Enumerate the original TypeScript test inventory first, map every portable
test/case into Rust, and only then count any Rust-specific tests as additive
coverage. The required relationship is
`original TypeScript tests <= mapped Rust tests`.

Required order: finish ALL core/shared upstream packages (the upstream `chat`
package itself plus shared transport/state/types/test-support packages) before
taking any standalone adapter slices (slack, teams, discord, telegram, github,
linear, whatsapp, google-chat, ...). Adapter packages resume only after the
core rows are verified or explicitly documented as intentionally non-portable.

Preserve Rust 2024 style, serde shapes, builders, public exports, tests, and
workspace boundaries that align with upstream package responsibilities. Build
against deterministic fake transports first; use any credential-gated live
adapter tests only as opt-in `#[ignore]` validation that skips cleanly when
credentials are missing. Never print or commit secrets.

Coexistence with the ai-sdk port (CRITICAL):
- Another Codex `/goal` session is concurrently porting `vercel/ai` in this
  same repo. It owns `crates/ai-sdk-*`, `docs/upstream-parity.md`,
  `docs/package-progress.md`, `docs/package-progress-estimates.tsv`, and
  `scripts/codex-goal/`. NEVER touch those.
- The chat port owns `crates/chat-sdk-*`, `docs/upstream-parity-chat.md`,
  `docs/package-progress-chat.md`,
  `docs/package-progress-estimates-chat.tsv`,
  `docs/goal-refinements-chat.md`, and `scripts/codex-goal-chat/`. The ai-sdk
  agent has been told to leave those alone.
- Shared files (the workspace root `Cargo.toml`, `Cargo.lock`, `scripts/`
  shared utilities, top-level `README.md`, and CI configs) may be edited but
  only additively for chat-sdk needs. Rebase/merge carefully; assume the
  ai-sdk session is concurrently touching them.
- Both sessions share the same merge lock
  `/tmp/ai-sdk-rust-main-merge.lock`. Always acquire it via the protocol in
  the full brief before merging to main.

Self-refining loop: after every 5 successful merge-back cycles, append a
refinement entry to `docs/goal-refinements-chat.md` summarizing what you
learned (upstream surprises, recurring blockers, mismatches between brief and
reality). Then update `scripts/codex-goal-chat/port-chat-sdk.md` and this
`goal-condition.md` so the next session benefits from the refinement. Treat
brief refinement as a first-class deliverable, not a meta-distraction.

Work in coherent slices. For each slice: rebase on latest main, implement,
test, update the parity ledger and package progress estimates, run the package
progress table generator, commit, then merge yourself back to `main` using the
serialized lock protocol in the full brief, validate again on `main`, and push
`main`. Repeat until the parity ledger and generated progress table prove full
upstream coverage or a real blocker appears.

Run the strongest available gates: `cargo fmt --all --check`, `cargo clippy
--all-targets --all-features -- -D warnings`, `scripts/check-naming-conventions.sh`,
the chat progress-table command above, and `cargo test --all-features`. Stop
instead of forcing state if main is dirty, merge conflicts are ambiguous, or
validation cannot be made green.
