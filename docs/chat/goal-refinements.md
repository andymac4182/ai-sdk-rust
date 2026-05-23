# Chat SDK Goal Refinements

This is an append-only log of refinements to the chat-sdk Codex `/goal` brief
([`scripts/codex-goal-chat/port-chat-sdk.md`](../../scripts/codex-goal-chat/port-chat-sdk.md))
and condition file
([`scripts/codex-goal-chat/goal-condition.md`](../../scripts/codex-goal-chat/goal-condition.md)).

The brief mandates a refinement pass after every 5 successful merge-back
cycles. Each entry below should capture:

1. **Slices covered** - the slice numbers (or commit SHA range) reviewed.
2. **What the brief got wrong or left out** - concrete upstream facts that
   contradict, refine, or extend the current brief.
3. **Stale or misleading guidance** - sections of the brief that should be
   tightened, removed, or reordered.
4. **Edits applied** - the exact brief/condition changes landed alongside this
   entry.
5. **Open refinements deferred** - items spotted but not yet folded in, with a
   rationale for deferring.

## Entry template

```
### YYYY-MM-DD - slices N..N+5

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

### 2026-05-23 - slices 1..5

Slices reviewed: setup (`394a786`, `0f6fab2`, `9843ea8`, `5417c49`), slice 1 inventory (`5c64795` / merge `63615c7`), slice 2 errors (`58dd48d` / merge `112a010`), slice 3 logger + colocation (`9b7e2bb` / merge `39eba5c`), slice 4 types leaf layer (`ba31906`), slice 5 types emoji layer (`0a4f5f2` / merge `04d72bf`).

**What the brief got wrong or left out**

- **Test layout was undefined.** The brief said "every portable upstream TypeScript test/case must exist as an equivalent Rust test" but didn't say *where*. Slice 2 put them in `crates/chat-sdk-chat/tests/errors.rs` (integration tests) and the user immediately corrected it: "Why are the tests not colocated with the code? This port should be idiomatic rust." The ai-sdk-rust workspace style is `#[cfg(test)] mod tests { ... }` at the bottom of each `src/*.rs` file. The brief now mandates this.
- **TSV requires literal tab characters.** When writing `docs/chat/package-progress-estimates.tsv` via a heredoc or the `Write` tool, the tab characters get lost. Use `printf '%s\t%s\t%s\n'` to guarantee real tabs. Without them the progress-table generator fails with an opaque `undefined method [] for nil` Ruby error.
- **`types.ts` is too big to port in one slice.** Upstream `packages/chat/src/types.ts` is 2,549 lines and transitively imports from `cards`, `channel`, `message`, `modals`, `postable-object`, `thread`, and `jsx-runtime`. The brief implied a module is a slice unit; in reality `types.ts` needs a *layered* port: standalone leaf types first, then a layer per dependency module as those modules land. The first two layers (standalone + emoji) have already shipped in slices 4 and 5.
- **The shared merge lock can be left in a corrupt state.** During slice 3 the lock file appeared as a 0-byte *regular file* (not a directory) even though `mkdir` is the protocol. The ai-sdk session was demonstrably still merging despite the lock existing, meaning the lock had been left behind by some prior crash or race. Recovery: `rm /tmp/ai-sdk-rust-main-merge.lock` then `mkdir`.
- **Shared scripts accept additive flags safely.** Slices 1 and 5 added `--title` to `scripts/package-progress-table.sh`, an `adapter-shared` upstream-mirroring exception to `scripts/check-naming-conventions.sh`, and a `scripts/codex-goal-chat/*` exclusion to the naming check. The ai-sdk session has been unaffected. The "additive only" rule in the brief is correct and worked.
- **Upstream inventory snapshot is a moving target.** Across slices 1-5 the ai-sdk session merged six commits to `main`. No conflicts. The brief's "rebase on origin/main before every merge-back" rule is necessary, not optional - without it, the second merge would always conflict on `Cargo.toml` workspace members.

**Stale or misleading guidance**

- Brief says "First action: create or update `docs/chat/upstream-parity.md`." That's done; future slices should treat the ledger as live state, not a one-time bootstrap artifact. Tighten to: "Re-read the ledger at the start of every slice; never invent a queue, always pull from `## Next Unported Work Queue`."
- Brief's `Validation` section lists `cargo test --all-features` but the workspace actually requires `cargo test --workspace --all-features` to exercise sibling crates. Without `--workspace`, only the root crate's tests run, masking failures in `chat-sdk-*`. Real-world correctness issue.
- Brief examples for the progress-table command should include `--title "Chat SDK Rust Package Progress"` everywhere. Already fixed during this slice but flagged here.
- Brief mentions "Use Codex agent/team/background-worker features if available" - this session is running under Claude Code, which has a different agent surface (`Agent` tool with subagent types). The wording was already softened from "Codex CLI" to "an agent (Claude Code or Codex CLI)"; the parallelization paragraph should be similarly generalized.

**Edits applied**

- `scripts/codex-goal-chat/port-chat-sdk.md`:
  - **Test layout rule** added: tests live in `#[cfg(test)] mod tests { use super::*; ... }` at the bottom of each `src/*.rs`; `crates/<crate>/tests/` is reserved for genuine cross-crate integration tests.
  - **TSV tab requirement** noted: use `printf` with literal `\t` - heredocs and the `Write` tool may strip tabs.
  - **Layered-types-port note** added: `types.ts` is ported in layers keyed to dependency modules.
  - **Stale-lock recovery** documented in the Merge-Back Protocol.
  - **Validation** updated to `cargo test --workspace --all-features`.
- `scripts/codex-goal-chat/goal-condition.md`: stable; no change needed.

**Open refinements deferred**

- The brief still implies a single chat package can be one slice; in reality `chat` is going to need 20+ slices (one per source module). Consider adding an explicit slice-budget guidance ("expect 25-30 slices to verify `packages/chat`, ~2-5 per other phase-1 package, ~5-15 per adapter").
- No automated check verifies that new `chat-sdk-*` source files have a colocated `#[cfg(test)] mod tests` block when there is a matching upstream `*.test.ts`. A future refinement could add a script that asserts this.
- The opensrc cache path `~/.opensrc/repos/github.com/vercel/chat/main` may go stale across sessions. The brief should mention `npx opensrc fetch github:vercel/chat` (not just `path`) to refresh it.

### 2026-05-23 - slices 7..12

Slices reviewed: slice 7 Lock primitive (`13dc8b1`), slice 8 concurrency/Author/UserInfo (`4555418`), slice 9 DurationString/TranscriptsConfig (`9e28f3f`), slice 10 adapter-shared crate + errors (`9768f5a`), slice 11 naming-check exceptions follow-up (`b5bf035`), slice 12 ChannelInfo/ListThreadsOptions (`4cba962`).

**What the brief got wrong or left out**

- **The merge-back validation block is not atomic with the push.** Slice 10's merge protocol used `cargo fmt --check && naming-check && cargo test && push`, but my shell wrapper put the push outside the `&& ` chain. The naming check failed (introduced `\`adapter-utils\`` etc. references), yet the push still landed on main. The ai-sdk session noticed and corrected my naming docs in commit `edd036b "Fix chat adapter naming docs"`, briefly crossing the ownership boundary. Slice 11 fixed it with proper script exceptions; slice 12 onward used a guarded `if ! ( ... ); then echo VALIDATION FAILED; exit 1; fi` block before push. The brief should mandate this guard explicitly.
- **The "shared, additive only" boundary on shared files isn't strong enough to prevent edits to chat-sdk-owned files when a validation failure on main blocks the ai-sdk session too.** Pragmatically that's the right call (a broken `main` blocks both sessions), but the brief should name this pattern: "if the *other* session's broken state is blocking your validation, fix the minimum required to unblock and document the cross-boundary edit in `goal-refinements.md`." Treat that as a recovery event, not the default mode.
- **The ledger row is a high-conflict line.** Slice 12 hit a merge conflict between origin/main (with the ai-sdk session's small edit to my chat-sdk row) and my newer slice 12 update. Manual resolution in favor of the most recent slice-update is the right answer because subsequent slices include the prior content. The brief should call this out: "if `docs/chat/upstream-parity.md` conflicts on a chat-sdk row during rebase, prefer your branch's version - the ledger row is overwritten wholesale by each slice that touches that package."
- **`serde_json::Map<String, Value>` is required for upstream `Record<string, unknown>` metadata fields.** I bumped `chat-sdk-chat`'s `serde_json` from a dev-dependency to a runtime dependency in slice 12. The brief should mention this expected dep promotion so future slices don't try to invent their own opaque-JSON wrapper.
- **`types.ts` layering works.** Six layers landed without `cards`/`message`/`channel` being touched. The recipe: scan `types.ts` for interfaces whose imports are exclusively (a) primitive TS types or (b) already-ported chat-sdk-chat types. Port those. Bump the estimate by ~2% per layer. The brief's slice 4 plan didn't anticipate how productive this approach is.
- **`adapter-shared/buffer-utils.ts` is borderline-JS-only.** Buffer/ArrayBuffer/Blob conversions are JS runtime types; the Rust equivalent is just `Vec<u8>` with no `Buffer.from()` distinction. Document this as "trivial port + most upstream tests degenerate to identity" if/when buffer-utils lands.

**Stale or misleading guidance**

- The brief's Merge-Back Protocol still shows the lock-acquire loop separate from the push. Combine them into a single guarded shell block - see "Edits applied".
- The brief lists `packages/adapter-shared` as Phase-1 dependency-free, but `buffer-utils.ts` depends on `card-utils.ts`'s `PlatformName` (a string-union type that could be extracted independently). Document the dependency.
- The brief doesn't mention that upstream test counts in the ledger should be tracked as "test files" vs "test cases" separately. Slice 1 conflated them. Going forward each row's `Evidence` cell tracks both.

**Edits applied**

- `scripts/codex-goal-chat/port-chat-sdk.md`:
  - **Atomic merge gate** added to the Serialized Merge-Back Protocol: wrap merge+validate inside `if ! ( set -e; ... ); then echo VALIDATION FAILED; exit 1; fi` so `git push origin main` is unreachable when any validation step fails.
  - **Cross-boundary fix-up rule** added: if main is broken in a way that blocks the *other* session's validation, the unblocking session may make the minimum cross-boundary edit needed, but must record it in the next refinement entry.
  - **Ledger conflict resolution** documented: prefer your branch's slice-row when `docs/chat/upstream-parity.md` rebase-conflicts on a chat-sdk-owned row.
  - **serde_json runtime dep** noted as the canonical Rust mirror of upstream `Record<string, unknown>` shapes.
- `scripts/codex-goal-chat/goal-condition.md`: stable; no change needed.

**Open refinements deferred**

- A pre-push git hook on the main checkout could enforce naming + clippy + workspace-test atomically, removing the foot-gun entirely. Worth a slice once both sessions agree on the contract.
- The progress-table generator currently aborts hard when `package-progress-estimates.tsv` mentions a package that is not `in-progress` - useful, but means a slice that flips a row from `in-progress` to `verified` must edit both files in lock-step. A future refinement could relax this.
- Cross-session merge conflicts on the ledger row would be eliminated by splitting each package row into its own file under `docs/chat/packages/<name>.md` and stitching them via the generator. Not urgent yet (only one conflict so far).

### 2026-05-23 - slices 14..18

Slices reviewed: slice 14 chat_singleton + placeholder traits (`42dac89`), slice 15 StreamChunk/StreamOptions (`30a0d71`), slice 16 MessageSubject/ThreadInfo (`efd5d4f`), slice 17 RawMessage + transcript queries (`f00cba3`), slice 18 apps/docs + examples/nextjs-chat classified js-only-documented (`8044ef2`).

**What the brief got wrong or left out**

- **Mutex-protected globals + panic = test poisoning.** Slice 14 introduced a static `Mutex<Option<Arc<dyn ChatSingleton>>>` plus a `get_chat_singleton()` that panicked while *holding* the lock when no singleton was registered. The next test in the parallel runner saw a poisoned mutex, blew up on its own `.expect(...)` call, and dragged 2-3 sibling tests with it. Fix landed in the same slice - *snapshot under the lock, drop the lock, then optionally panic* - and every mutator switched to `unwrap_or_else(|poisoned| poisoned.into_inner())` so a separate test panic can't cascade. This pattern needs to be a brief priority: **never panic while holding a `std::sync::Mutex` guard you might want to reuse**.
- **Placeholder traits unblock cross-module ports faster than expected.** Adding `pub trait Adapter: Send + Sync + Debug {}` and `pub trait StateAdapter: Send + Sync + Debug {}` to `types.rs` with zero methods let `chat_singleton` ship without needing the full Adapter/StateAdapter method set. Future module slices (cards/channel/message) can extend these traits incrementally. The brief should explicitly endorse "land the trait first, grow it per slice" as the canonical pattern for upstream interfaces with large method sets.
- **`js-only-documented` is a real, productive slice.** Slice 18 classified `apps/docs` and `examples/nextjs-chat` as non-portable in a single 4-line ledger edit (no Rust code), permanently shrinking the remaining-work queue. The brief should call out classification slices as a first-class slice type alongside type-layer and module-port slices.
- **Generic types need typed-substitution tests too.** Slice 17's `RawMessage<TRaw = serde_json::Value>` shipped with two tests: one with the default, one with a concrete user-defined struct. The second test proves the generic isn't accidentally tied to `Value`. Pattern recommended for every upstream `interface X<T = unknown>`.
- **Untagged-deserialization rejection is worth testing.** Slice 15's `StreamChunk` is `#[serde(tag = "type", rename_all = "snake_case")]`. The colocated test `stream_chunk_untagged_object_fails_to_deserialize` asserts that a JSON object missing the `type` field fails to parse - mirrors TypeScript's compile-time tag check at runtime. Add this assertion for every tagged-union slice.
- **The types.rs layered approach has shipped 9 layers and ~42 types without unblocking any module-port work**, because every module still ultimately requires `Message`, `Channel`, `Thread`, `Card`, or `Modal`. Those need either upstream `markdown.ts` (122 tests, external mdast dep) or a deliberate scaffold-first approach. The brief should now name the next architectural slice: **pick a Rust markdown crate**.

**Stale or misleading guidance**

- The brief's "Required Work Order" lists `packages/tests` and `packages/state-memory` as straightforward phase-1 ports. In reality `packages/tests` is almost entirely Vitest mocking helpers - likely a future classification slice (parts `js-only-documented`, parts moved to a `chat-sdk-test-support` crate when phase-1 modules land). `packages/state-memory` cannot start until QueueEntry/StateAdapter/Lock-friendly traits are real (only Lock is real so far). Reorder.
- The brief still suggests progressing through phase-1 packages roughly in parallel. In practice nine `types.rs` layers + one `chat_singleton` shipped before `packages/adapter-shared` got its second module - the type-layering productivity dominates. The brief should explicitly bless that sequencing.

**Edits applied**

- `scripts/codex-goal-chat/port-chat-sdk.md`:
  - **Mutex panic rule** added: never call `expect`/`unwrap` while holding a `std::sync::Mutex` guard if the panic could poison the lock for sibling tests. Snapshot then panic.
  - **Placeholder-trait pattern** documented: large upstream interfaces (>5 methods) ship as empty `pub trait X: Send + Sync + Debug {}` first; per-slice growth as dependency modules land. Sliced upstream interfaces always extend the same trait, never define a new one.
  - **Classification slices** added to the canonical slice-type list: marking an upstream surface `js-only-documented` (or `verified` with no code, when behavior is intrinsic) counts as a real slice if the ledger and JavaScript-only Exceptions table are both updated and the entry is non-trivially justified.
  - **Generic-type test pattern** documented: `interface X<T = unknown>` ports as `pub struct X<T = serde_json::Value>` plus a test using a custom concrete type to prove the default is opt-in.
  - **Tagged-union test pattern** documented: every `#[serde(tag = "type")]` enum gets one negative-path test rejecting an untagged object.
  - **Next architectural slice flagged**: the dependency wall is now markdown.ts. The next major slice should be the markdown-crate decision (`markdown-rs` recommended) and `chat-sdk-chat::markdown` skeleton.
- `scripts/codex-goal-chat/goal-condition.md`: stable.

**Open refinements deferred**

- A `cargo test --workspace --all-features --test-threads=1` integration job would catch parallel-test foot-guns like the slice-14 poisoning before they reach `main`. Worth a slice once enough modules are in place that the matrix payoff is clear.
- The brief still calls out provider-style adapters by name (Slack, Teams, etc.) for the Phase-2 queue, but upstream may have added new adapters since the slice-1 inventory. Re-running `npx opensrc fetch github:vercel/chat` and diffing should be on the slice-25 refinement agenda.

### 2026-05-23 - slices 20..24

Slices reviewed: slice 20 LockScopeContext/FileUpload/FetchOptions (`7becb09`), slice 21 PostEphemeralOptions (`7c92f99`), slice 22 FormattedContent placeholder + AppendInput + TranscriptEntry (`284700f`), slice 23 Attachment + LinkPreview data shapes (`ce21b14`), slice 24 PostableRaw + PostableMarkdown (`2947103`).

**What the brief got wrong or left out**

- **Placeholder traits actually work.** Slice 20's `LockScopeContext` was the first non-trivial consumer of the slice-14 placeholder `Adapter` trait. Holds an `Arc<dyn Adapter>`, compiles cleanly, future adapter slices grow the trait without changing the storage shape. Confirms the brief's slice-19 priority #5 - promote it from "untested theory" to "validated pattern" in the next brief revision.
- **Placeholder type aliases extend the pattern to data, not just behavior.** Slice 22 shipped `pub type FormattedContent = serde_json::Value;` so AppendInput / TranscriptEntry could carry opaque mdast through the wire without forcing the markdown-crate decision today. When `chat-sdk-chat::markdown` lands, swapping the alias to a typed AST automatically updates every downstream type that holds a `FormattedContent`. The brief should name this pattern alongside the placeholder-trait pattern.
- **`#[derive(Hash)]` is incompatible with `HashMap`/`Vec` fields containing non-Hash types.** Slice 23 first shipped `Attachment` with `#[derive(Hash)]`; the build broke on the `Option<HashMap<String, String>> fetch_metadata` field because the default Hash derive requires every field to be `Hash`. Real foot-gun for serde-derived data types with map members. Brief should call this out: data types containing `HashMap`/`BTreeMap` / `serde_json::Map` / `serde_json::Value` / `Vec<NonHash>` must skip the `Hash` derive.
- **Structurally-similar types deserve a wire-distinction test.** Slice 24's `PostableRaw` and `PostableMarkdown` share `attachments`/`files` and differ only in their body field (`raw` vs `markdown`). The third colocated test asserts `serde_json::to_string` produces JSON whose required key differs between the two - adapters branch on which key is present. Brief priority candidate: when two ported types share most of their shape but distinguish on a single required field, add an explicit assertion that their wire JSON differs.
- **"Ship data shape + document the callback elision" works for behavior-carrying interfaces.** Slice 23's Attachment and LinkPreview each declared a JS-only async callback (`fetchData`, `fetchMessage`); the Rust port emits the data fields only and documents the callback as a future Adapter trait method. The doc comments cite the upstream member they elide, which keeps the trail honest. Brief should formalize this as a recipe for any upstream interface with `methods: () => Promise<T>` shapes.
- **Single-field structs still count as proper slices.** Slice 21 was a 9-line `PostEphemeralOptions { fallback_to_dm: bool }` and 1 wire-format test. Documented this in its commit message - small slices that nail down a stable wire contract are valuable. Brief should drop any implicit "slices must port multiple types" expectation.

**Stale or misleading guidance**

- The brief's Phase-1/Phase-2 ordering still implies whole-package fronts. Reality: the types-layer approach has shipped 14 layers covering 54 upstream interfaces while `packages/adapter-shared` is still at 1/4 modules and 11 adapters are 0/0. The brief should re-frame Phase-1 as "core/shared layers ready for adapter consumers" - measured by which dependency modules unblock the most downstream surface, not by package row count.
- The brief's Next Unported Work Queue section in `docs/chat/upstream-parity.md` is stale - it still names "Slice 2 (planned)" through "Slice 7 (planned)" from the initial inventory. Future refinement passes should keep that queue current (or remove the stale entries entirely and rely on the package-row Evidence cells).

**Edits applied**

- `scripts/codex-goal-chat/port-chat-sdk.md`:
  - **Placeholder type aliases** added alongside the placeholder-trait pattern (priority 5 -> expanded). Cite slice-22 `FormattedContent = serde_json::Value` as the canonical example.
  - **Hash derive caveat** added as a new priority: data types containing `HashMap`/`BTreeMap`/`serde_json::Map`/`serde_json::Value`/`Vec<NonHash>` must skip the `Hash` derive.
  - **Wire-distinction test pattern** added: structurally-similar types whose only required difference is a single key must include a colocated assertion that their JSON renders differ.
  - **Data-shape-plus-elided-callback recipe** added for upstream interfaces with `() => Promise<T>` methods. Doc-comment the elided callback by upstream name; promote to Adapter trait method when adapters land.
  - **Single-field slice acknowledgement** added: slices may port a single one-field struct if the wire contract is non-trivial and the colocated test exercises the round-trip.
- `scripts/codex-goal-chat/goal-condition.md`: stable.

**Open refinements deferred**

- The `docs/chat/upstream-parity.md` Next Unported Work Queue is stale and should be replaced (or rewritten as "next types-layer candidates", "next module-port candidates", and "next architectural slice"). Defer to slice 30's refinement so this entry stays focused.
- An automated upstream-test-inventory diff (`scripts/check-upstream-test-inventory.sh`?) would compute the count of upstream `*.test.ts` files vs colocated `#[cfg(test)] mod tests` blocks per crate and report the gap. Would catch any future regression where the ledger drifts from reality. Worth a dedicated slice when the pattern is more entrenched.

### 2026-05-23 - slices 26..30

Slices reviewed: slice 26 markdown architectural scaffold (`927a40f`), slice 27 is_* type guards + getNodeChildren + getNodeValue (`a825aa8`), slice 28 to_plain_text + markdown_to_plain_text (`1402370`), slice 29 walk_ast (`c5b4113`), slice 30 table_to_ascii + table_element_to_ascii (`35883a8`).

**What the brief got wrong or left out**

- **The dependency wall is breached.** Slice 26 picked `markdown = "1.0.0"` (markdown-rs) and shipped enough of `chat-sdk-chat::markdown` to unblock cards.ts. As of slice 30, `tableElementToAscii` (cards.ts's only markdown.ts import beyond AST types) is ported. The next module-port slice can be cards itself - the layered-types-first detour that ran from slices 4 through 24 was the right call given the dep wall but is no longer needed.
- **`Node::to_string()` is exactly the upstream mdast-util-to-string plain-text extractor.** Slice 28 leaned on this - `to_plain_text(ast)` is a one-liner `ast.to_string()`. Worth documenting in the brief because it's not obvious without checking the markdown-rs source.
- **`walk_ast` requires a `children_mut(&mut Node) -> Option<&mut Vec<Node>>` enumerator.** The markdown-rs `Node` enum doesn't expose a `children_mut()` helper, only the immutable `children()`. The Rust port enumerates every container variant (Root/Paragraph/Heading/Blockquote/List/ListItem/Emphasis/Strong/Delete/Link/LinkReference/FootnoteDefinition/Table/TableRow/TableCell) in a private helper. Future variants would need to be added. The brief should call this out: when porting visitors over `markdown::mdast::Node`, keep `children_mut` updated as a single source of truth.
- **Em-dashes and arrows in the ledger break the progress-table generator.** Slice 26 and slice 30 both regressed this: `docs/chat/upstream-parity.md` picked up `-` and `->` chars during regular edits, which made the Ruby progress-table script fail with `invalid byte sequence in US-ASCII`. The fix is mechanical (`python3 -c "...read().replace('-', '-').replace('->', '->')..."`) but the brief should mandate pure-ASCII content in the ledger and the TSV. The shared script is ASCII-locale by default and can't be safely changed from the chat session.
- **Shell heredocs are mandatory for commit messages containing Rust generics or unions.** Slice 29's first commit attempt blew up because the message contained `FnMut(Node) -> Option<Node>` and `Content | null`; the shell parser tried to evaluate `>` and `|` as I/O redirection. Use `git commit -m "$(cat <<'EOF' ... EOF)"` always when the body mentions type syntax.
- **The off-by-one in `table_element_to_ascii` separator length.** Slice 30's first version of `table_element_to_ascii_pads_columns_to_max_width` had me write the expected separator as `"------|--------"` (15 chars) when the actual was `"------|-------"` (14 chars). The bug was in the test, not the impl. The Rust implementation correctly produces `"-".repeat(left_width) + "-|-" + "-".repeat(right_width)` for column widths `[5, 6]` = `"-----" + "-|-" + "------"` = 14 chars. Lesson: when porting JS string-concat code, count chars and prefer per-line structural assertions over a single golden-string `assert_eq!`.

**Stale or misleading guidance**

- The brief still implies markdown-rs is "the leading candidate" (slice 19 refinement entry). It's no longer a candidate; it's the chosen and shipped dependency. Tightening: remove "candidate" language; markdown-rs IS the markdown stack.
- The brief's slice-budget rough estimate from slice-5's refinement entry implied "expect 25-30 slices to verify `packages/chat`". We are at slice 30 and `packages/chat` is at 46%, not verified. Recalibrate to "expect 60-80 slices to verify `packages/chat`, factoring in markdown.ts's 122-test surface".

**Edits applied**

- `scripts/codex-goal-chat/port-chat-sdk.md`:
  - **Pure-ASCII ledger + TSV rule** added: `docs/chat/upstream-parity.md` and `docs/chat/package-progress-estimates.tsv` must contain only ASCII characters. Em-dashes (`-`), curly quotes (`""''`), arrows (`->`), and similar Unicode punctuation break the Ruby progress-table generator. Recovery: `python3 -c "..."` with `.replace('-', '-').replace('->', '->')` etc.
  - **Shell-heredoc rule for commit messages** added: when the body mentions Rust generics (`<T>`, `Option<Node>`, `FnMut(...) -> ...`) or TypeScript unions (`A | B`), wrap the message in `"$(cat <<'EOF' ... EOF)"` to keep the shell from interpreting `<`, `>`, `|`, and `(`.
  - **`children_mut` single-source-of-truth note** added under the placeholder-trait section: when porting an upstream AST visitor over `markdown::mdast::Node`, keep the `children_mut` enumerator updated; treat it as the canonical "container variants in this port" list.
  - Markdown decision finalized: replace "leading candidate" with "chosen dependency" everywhere in the brief.
- `scripts/codex-goal-chat/goal-condition.md`: stable.

**Open refinements deferred**

- Replace the slice-5 refinement entry's "expect 25-30 slices" estimate with the slice-30 recalibration (60-80 slices for `packages/chat`). Will be done in the next brief revision if not before.
- The Ruby progress-table generator could be patched to call `force_encoding("UTF-8")` and skip non-ASCII gracefully. That's a shared-script edit that needs coordination with the ai-sdk session - defer until both sides agree on the contract.
