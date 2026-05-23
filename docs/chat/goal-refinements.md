# Chat SDK Goal Refinements

This is an append-only log of refinements to the chat-sdk Codex `/goal` brief
([`scripts/codex-goal-chat/port-chat-sdk.md`](../../scripts/codex-goal-chat/port-chat-sdk.md))
and condition file
([`scripts/codex-goal-chat/goal-condition.md`](../../scripts/codex-goal-chat/goal-condition.md)).

The brief mandates a refinement pass after every 5 successful merge-back
cycles. Each entry below should capture:

1. **Slices covered** — the slice numbers (or commit SHA range) reviewed.
2. **What the brief got wrong or left out** — concrete upstream facts that
   contradict, refine, or extend the current brief.
3. **Stale or misleading guidance** — sections of the brief that should be
   tightened, removed, or reordered.
4. **Edits applied** — the exact brief/condition changes landed alongside this
   entry.
5. **Open refinements deferred** — items spotted but not yet folded in, with a
   rationale for deferring.

## Entry template

```
### YYYY-MM-DD — slices N..N+5

**What the brief got wrong or left out**
- ...

**Stale or misleading guidance**
- ...

**Edits applied**
- `scripts/codex-goal-chat/port-chat-sdk.md`: ...
- `scripts/codex-goal-chat/goal-condition.md`: ...

**Open refinements deferred**
- ...
```

## Entries

### 2026-05-23 — slices 1..5

Slices reviewed: setup (`394a786`, `0f6fab2`, `9843ea8`, `5417c49`), slice 1 inventory (`5c64795` / merge `63615c7`), slice 2 errors (`58dd48d` / merge `112a010`), slice 3 logger + colocation (`9b7e2bb` / merge `39eba5c`), slice 4 types leaf layer (`ba31906`), slice 5 types emoji layer (`0a4f5f2` / merge `04d72bf`).

**What the brief got wrong or left out**

- **Test layout was undefined.** The brief said "every portable upstream TypeScript test/case must exist as an equivalent Rust test" but didn't say *where*. Slice 2 put them in `crates/chat-sdk-chat/tests/errors.rs` (integration tests) and the user immediately corrected it: "Why are the tests not colocated with the code? This port should be idiomatic rust." The ai-sdk-rust workspace style is `#[cfg(test)] mod tests { ... }` at the bottom of each `src/*.rs` file. The brief now mandates this.
- **TSV requires literal tab characters.** When writing `docs/chat/package-progress-estimates.tsv` via a heredoc or the `Write` tool, the tab characters get lost. Use `printf '%s\t%s\t%s\n'` to guarantee real tabs. Without them the progress-table generator fails with an opaque `undefined method [] for nil` Ruby error.
- **`types.ts` is too big to port in one slice.** Upstream `packages/chat/src/types.ts` is 2,549 lines and transitively imports from `cards`, `channel`, `message`, `modals`, `postable-object`, `thread`, and `jsx-runtime`. The brief implied a module is a slice unit; in reality `types.ts` needs a *layered* port: standalone leaf types first, then a layer per dependency module as those modules land. The first two layers (standalone + emoji) have already shipped in slices 4 and 5.
- **The shared merge lock can be left in a corrupt state.** During slice 3 the lock file appeared as a 0-byte *regular file* (not a directory) even though `mkdir` is the protocol. The ai-sdk session was demonstrably still merging despite the lock existing, meaning the lock had been left behind by some prior crash or race. Recovery: `rm /tmp/ai-sdk-rust-main-merge.lock` then `mkdir`.
- **Shared scripts accept additive flags safely.** Slices 1 and 5 added `--title` to `scripts/package-progress-table.sh`, an `adapter-shared` upstream-mirroring exception to `scripts/check-naming-conventions.sh`, and a `scripts/codex-goal-chat/*` exclusion to the naming check. The ai-sdk session has been unaffected. The "additive only" rule in the brief is correct and worked.
- **Upstream inventory snapshot is a moving target.** Across slices 1–5 the ai-sdk session merged six commits to `main`. No conflicts. The brief's "rebase on origin/main before every merge-back" rule is necessary, not optional — without it, the second merge would always conflict on `Cargo.toml` workspace members.

**Stale or misleading guidance**

- Brief says "First action: create or update `docs/chat/upstream-parity.md`." That's done; future slices should treat the ledger as live state, not a one-time bootstrap artifact. Tighten to: "Re-read the ledger at the start of every slice; never invent a queue, always pull from `## Next Unported Work Queue`."
- Brief's `Validation` section lists `cargo test --all-features` but the workspace actually requires `cargo test --workspace --all-features` to exercise sibling crates. Without `--workspace`, only the root crate's tests run, masking failures in `chat-sdk-*`. Real-world correctness issue.
- Brief examples for the progress-table command should include `--title "Chat SDK Rust Package Progress"` everywhere. Already fixed during this slice but flagged here.
- Brief mentions "Use Codex agent/team/background-worker features if available" — this session is running under Claude Code, which has a different agent surface (`Agent` tool with subagent types). The wording was already softened from "Codex CLI" to "an agent (Claude Code or Codex CLI)"; the parallelization paragraph should be similarly generalized.

**Edits applied**

- `scripts/codex-goal-chat/port-chat-sdk.md`:
  - **Test layout rule** added: tests live in `#[cfg(test)] mod tests { use super::*; ... }` at the bottom of each `src/*.rs`; `crates/<crate>/tests/` is reserved for genuine cross-crate integration tests.
  - **TSV tab requirement** noted: use `printf` with literal `\t` — heredocs and the `Write` tool may strip tabs.
  - **Layered-types-port note** added: `types.ts` is ported in layers keyed to dependency modules.
  - **Stale-lock recovery** documented in the Merge-Back Protocol.
  - **Validation** updated to `cargo test --workspace --all-features`.
- `scripts/codex-goal-chat/goal-condition.md`: stable; no change needed.

**Open refinements deferred**

- The brief still implies a single chat package can be one slice; in reality `chat` is going to need 20+ slices (one per source module). Consider adding an explicit slice-budget guidance ("expect 25–30 slices to verify `packages/chat`, ~2–5 per other phase-1 package, ~5–15 per adapter").
- No automated check verifies that new `chat-sdk-*` source files have a colocated `#[cfg(test)] mod tests` block when there is a matching upstream `*.test.ts`. A future refinement could add a script that asserts this.
- The opensrc cache path `~/.opensrc/repos/github.com/vercel/chat/main` may go stale across sessions. The brief should mention `npx opensrc fetch github:vercel/chat` (not just `path`) to refresh it.
