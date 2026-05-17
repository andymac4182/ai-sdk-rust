Run the ai-sdk-rust Vercel AI SDK port goal.

Main checkout: `/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust`
Full brief in this worktree: `scripts/codex-goal/port-ai-sdk.md`

First confirm `git rev-parse --show-toplevel` is NOT the main checkout path
above. If it is, stop immediately. Then read the full brief from the current
worktree and follow it as the source of truth. Do not summarize it and stop.
Execute it.

Goal: use the current working directory as your worktree and make substantial,
validated progress on an idiomatic Rust port of the Vercel AI SDK. Prioritize a
working `generate_text(...)` vertical slice with deterministic tool-loop
execution before adding more horizontal provider surface area.

Use `npx opensrc@latest path github:vercel/ai` as the upstream source of truth.
Preserve Rust 2024 style, serde shapes, builders, public exports, tests, and
workspace boundaries that align with upstream package responsibilities. Build
against fake/test `LanguageModel` first, then use the ignored `.env.local`
Vercel AI Gateway variables only for optional integration validation. Never
print or commit secrets.

Work in coherent slices. For each slice: rebase on latest main, implement,
test, commit, then merge yourself back to `main` using the serialized
`/tmp/ai-sdk-rust-main-merge.lock` protocol in the full brief, validate again
on `main`, and push `main`. Repeat until no useful next SDK slice remains or a
real blocker appears.

Use Codex agent/team/background-worker features if available to parallelize
upstream research, implementation, and verification. Integrate the work
yourself before committing.

Run the strongest available gates: `cargo fmt --all --check`, `cargo clippy
--all-targets --all-features -- -D warnings`, and `cargo test --all-features`.
Stop instead of forcing state if main is dirty, merge conflicts are ambiguous,
or validation cannot be made green.
