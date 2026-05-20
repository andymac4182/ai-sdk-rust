Run the ai-sdk-rust full Vercel AI SDK parity goal.

Main checkout: `/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust`
Full brief in this worktree: `scripts/codex-goal/port-ai-sdk.md`

First confirm `git rev-parse --show-toplevel` is NOT the main checkout path
above. If it is, stop immediately. Then read the full brief from the current
worktree and follow it as the source of truth. Do not summarize it and stop.
Execute it.

Goal: use the current working directory as your worktree and keep working until
ai-sdk-rust has Rust equivalents for EVERY package, provider, library, example,
testable behavior, and feature in upstream `vercel/ai`, except JavaScript-only
surfaces that are explicitly documented as intentionally non-portable.

Use `npx opensrc@latest path github:vercel/ai` as the upstream source of truth.
First build/update `docs/upstream-parity.md`: record upstream commit/package
inventory, every provider package, every core/helper/library package, public
APIs, examples, tests, and feature status. Do not mark the goal complete while
any ledger row is unported, unverified, or undocumented. Re-scan upstream often.

Test parity is a hard completion gate. EVERY portable test from the original
upstream TypeScript packages must exist as an equivalent Rust test in the
matching 1:1 crate before that package can be marked verified. The inventory is
the individual upstream test/case, not just the file, feature, or broad behavior
area. No original portable TypeScript test, table row, fixture/snapshot case,
streaming/error/provider-option case, or type-level assertion may be missing a
Rust counterpart. Rust may and should add extra tests for Rust-specific typing,
safety, live-provider proof, and edge cases, but it must never have fewer
portable tests than upstream. A broader Rust test that only generally covers the
same area is not enough unless the ledger maps it to the exact upstream cases it
covers. The original TypeScript test inventory is the floor: every portable
upstream test case must be counted, mapped to Rust, and kept visible until
ported or explicitly documented as JavaScript-only. Future Rust coverage may
only be a superset of the upstream tests, never a reduced or sampled subset.

Required order: finish ALL common/core SDK packages together with Vercel AI
Gateway provider coverage before taking more unrelated standalone provider
slices. This is a hard ordering gate, not a scheduling preference. The first
phase includes
`packages/ai`, `packages/provider`, `packages/provider-utils`,
`packages/openai-compatible`, `packages/open-responses`, `packages/gateway`,
Vercel AI Gateway OpenAI-compatible and Open Responses routes, and portable
non-provider rows such as MCP, OTel, Workflow, telemetry, UI transport,
chat/completion transport, and test-server support. Treat Vercel AI Gateway as
part of the first phase, not as a later standalone provider. Other provider
packages resume only after those rows are verified or explicitly documented as
intentionally non-portable. Gateway progress does not unlock other providers by
itself; the whole common/core plus Vercel AI Gateway phase must be closed first.

Preserve Rust 2024 style, serde shapes, builders, public exports, tests, and
workspace boundaries that align with upstream package responsibilities. Build
against deterministic fake models first, then use the ignored `.env.local`
Vercel AI Gateway variables only for optional integration validation. Never
print or commit secrets. For OTel/telemetry rows, deterministic span tests are
not enough for verification: also prove OTLP/HTTP export against the loopback
receiver or a local collector, and once root telemetry wiring exists pair
provider live tests with that telemetry export assertion.

Work in coherent slices. For each slice: rebase on latest main, implement,
test, update the parity ledger, commit, then merge yourself back to `main`
using the serialized `/tmp/ai-sdk-rust-main-merge.lock` protocol in the full
brief, validate again on `main`, and push `main`. Repeat until the parity
ledger proves full upstream coverage or a real blocker appears.

Use Codex agent/team/background-worker features if available to parallelize
upstream research, ledger updates, implementation, and verification. Integrate
the work yourself before committing.

Run the strongest available gates: `cargo fmt --all --check`, `cargo clippy
--all-targets --all-features -- -D warnings`, `scripts/check-naming-conventions.sh`,
and `cargo test --all-features`. Stop instead of forcing state if main is
dirty, merge conflicts are ambiguous, or validation cannot be made green.
