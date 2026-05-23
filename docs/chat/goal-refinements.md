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

### 2026-05-23 - slices 32..35

Slices reviewed: slice 32 cards leaf elements (`f1ecdc8`), slice 33 modals leaf interactive elements (`350301e`), slice 34 cards ActionsElement + ActionsChild union (`826ecec`), slice 35 cards SectionElement + CardChild union + CardElement + Card (`ebfdc4d`). (Slice 31 was the last refinement, covering 26-30.)

**What the brief got wrong or left out**

- **Discriminated unions over Rust structs that already carry their own discriminator work cleanly with `#[serde(untagged)]`.** Slices 34 (`ActionsChild`) and 35 (`CardChild`) both use this pattern. Per-struct unit-enum discriminators (e.g. `ButtonKind::Button` -> wire `"button"`, `LinkButtonKind::LinkButton` -> wire `"link-button"`) mean each variant struct already has a unique `type` field, and serde's untagged matcher disambiguates from that without an outer wrapper. The end JSON is identical to upstream's discriminated-union shape. Brief should canonize this as the "ported-from-TS-discriminated-union" recipe: per-struct unit-enum discriminator + `#[serde(untagged)]` parent enum + `From<T>` impls for ergonomic construction.
- **`From<T>` impls on union enums materially improve call-site readability.** Slice 34 added `From<ButtonElement>` / `From<LinkButtonElement>` / `From<SelectElement>` / `From<RadioSelectElement>` on `ActionsChild`, and slice 35 added all 8 variant impls on `CardChild`. The cost is mechanical (one impl per variant); the payoff is `actions(vec![button(...).into(), link_button(...).into(), ...])` reading exactly like upstream's `Actions([Button(...), LinkButton(...)])`. Brief should require these impls on every untagged union.
- **Slice 35 closed cards's data-shape surface in one slice.** With the slice-34 pattern proven, the SectionElement + CardChild + CardElement + Card builder + is_card_element bundle landed cleanly as a single ~250-line slice. Lesson: once the union-of-structs recipe is established, follow-up "build the union + root + builder + type-guard" slices are mechanically reproducible and shouldn't be split.
- **Card.toAscii fallback rendering is a behavior slice, not a data-shape slice.** It belongs after the entire data-shape surface is in place, so the renderer can iterate every variant exhaustively. Deferring it to its own slice (rather than shipping it alongside slice 35) keeps slice 35 reviewable.
- **The 5-slice refinement cadence catches every meta-pattern shift.** Slice 31's refinement codified the markdown stack; slice 36 codifies the union-of-structs recipe. Without the cadence both would still be tribal knowledge in the brief author's head.

**Stale or misleading guidance**

- The brief's priority 5 (placeholder pattern) covers placeholder traits / placeholder type aliases / data-shape-plus-elided-callback. After slice 34-35 a fourth pattern is canonical: **discriminated unions over already-tagged structs via `#[serde(untagged)]` + `From<T>`**. Add as priority 5(d).
- The brief's slice-budget estimate from slice 31's refinement said "~60-80 slices to verify `packages/chat`". After slices 32-35 we are at 35 slices and `packages/chat` is at 56%. Recalibrate: 70-100 slices to *verify* (full 1:1 test floor), but the data-shape surface for cards/modals/section/card is *done* in well under that, so the next phase is largely about porting handler/event behavior + the chat-singleton consumer code, plus the deep markdown tail.

**Edits applied**

- `scripts/codex-goal-chat/port-chat-sdk.md`:
  - **Discriminated union recipe** added as priority 5(d): port a TypeScript discriminated union (`A | B | C` where each carries `type: "x"`) as a Rust `#[serde(untagged)]` enum whose variants are structs that each carry a per-struct unit-enum discriminator (e.g. `ButtonKind::Button` -> `"button"`). Always provide `From<VariantStruct>` impls on the union enum.
  - **Data-shape vs behavior slice split** added under priority 4 (layered types): when porting an upstream module whose surface is "data types + builders + then a render/extract behavior", ship the data-shape surface FIRST (one cohesive slice once the unions land), then the behavior in a follow-up slice. Mention slice 30 (`table_to_ascii` after the markdown AST) and slice 35 (data shape) -> deferred `Card.toAscii` (behavior) as canonical examples.
- `scripts/codex-goal-chat/goal-condition.md`: stable.

**Open refinements deferred**

- The `markdown` module is 33/122 cases. The remaining cases are mostly `stringify_markdown` (writing the inverse of the parser) which markdown-rs doesn't ship. That's a substantial slice (probably 2-3 of its own) and worth its own architectural-decision pass: either write a hand-rolled stringifier matching upstream's `remark-stringify` rules, or skip `stringify_markdown` and document it as "use upstream remark-stringify via a separate Rust crate when needed by adapter".
- The `modals` module's `ModalElement` + `ModalChild` union is straightforward now that the `CardChild` pattern is canonical - should ship as one slice mirroring slice 35.
- `chat-sdk-adapter-shared` is stuck at 25% because its remaining three modules (`adapter-utils`/`buffer-utils`/`card-utils`) all import from `chat`'s `cards.ts`. With slice 35 those imports are now available, so `chat-sdk-adapter-shared` is unblocked too.

### 2026-05-23 - slices 37..41

Slices reviewed: slice 37 modals data shape (Modal builder + ModalChild union + filter_modal_children + VALID_MODAL_CHILD_TYPES), slice 39 buffer_utils (to_buffer + to_buffer_sync + buffer_to_data_uri), slice 40 adapter_utils (extract_card + extract_files + extract_postable_attachments via typed AdapterPostableMessage), slice 41 crypto (AES-256-GCM encrypt/decrypt + decode_key + is_encrypted_token_data). Slice 36 was the last refinement.

**What the brief got wrong or left out**

- **`markdown` crate features for serde-derived structs.** Slice 40 added `PostableAst { ast: Root, ... }` to `chat-sdk-chat::types`. `Root` (and friends) don't implement `Serialize`/`Deserialize` by default; the `markdown` crate gates those impls behind a `json` feature. Brief should flag this whenever a new types-layer struct references an mdast type: `markdown = { version = "1.0.0", features = ["json"] }`.
- **Untagged enum variant ordering matters when one variant is a primitive.** `AdapterPostableMessage` is `String | PostableRaw | PostableMarkdown | PostableAst | PostableCard | CardElement`. With `#[serde(untagged)]`, place the structured variants (`Raw`, `Markdown`, `Ast`, `Card`, `CardElement`) BEFORE the primitive `Text(String)` variant. Reversed order can cause a JSON string to match a struct variant via permissive coercion. Add as a sub-rule under priority 5(d) (discriminated-union recipe).
- **Trust nothing about field shapes — always grep the Rust struct.** Slice 40's tests assumed `Attachment.name: String` (matching upstream) but the Rust port has `name: Option<String>` (it's optional in the type-layered port). Same for `FileUpload.data` which is `FileBytes` not `Option<FileBytes>`. Cost: 4 compile errors before the test suite passed. Lesson: before writing tests against any types-layer struct, run `grep -nB1 -A12 "^pub struct X" crates/chat-sdk-chat/src/types.rs` to confirm the actual field signatures.
- **Builder signatures may differ from upstream.** Slice 40's `card_text("Content")` failed: the Rust signature is `card_text(content, style: Option<TextStyle>)`, but upstream's is just `CardText(content)`. The TS port elided the style arg into `?:`. Rust ports of TS builders should preserve `Option<T>` args verbatim — but call sites in tests and other modules need to pass `None` explicitly. Add as priority 5(e): when porting builders with optional args, pass `None` at call sites; don't add a `..Default::default()` shim unless upstream uses one.
- **`serde_json` graduates from dev-deps to deps when a public API touches `serde_json::Value`.** Slice 41's `is_encrypted_token_data(&Value) -> bool` forced this. Brief should call out: if a module's public surface returns/accepts `serde_json::Value`, that crate's `serde_json` line must be in `[dependencies]`, not `[dev-dependencies]`.
- **`crypto.ts` has NO upstream test file** — but it still needs to be ported to mark adapter-shared verified. The 15 colocated Rust tests are *additive* (roundtrip, IV-randomness, tampered-tag detection, key-decode happy/error paths, shape check, serde roundtrip). Ledger format change: when an upstream `.ts` ships without a `.test.ts`, mark the Rust coverage as "additive" in the ledger, not "1:1 of N cases". This keeps the test-floor accounting honest.
- **Adapter-shared closed in 4 slices once the dependency walls fell.** Slices 38 (card_utils), 39 (buffer_utils), 40 (adapter_utils), 41 (crypto) — paced ~one source file per slice. The unblockers were slice 30 (markdown::table_element_to_ascii) and slice 35 (CardElement data shape). Lesson for the next phase-1 push (`packages/tests`, `packages/state-memory`): identify the architectural dependency that's gating everything, ship that as a dedicated infra slice, then watch the dependent modules collapse in single-file-per-slice slices.

**Stale or misleading guidance**

- The slice-31 refinement said "`chat-sdk-adapter-shared` is stuck at 25% because its remaining three modules ... all import from `chat`'s `cards.ts`." That blocker is now resolved (slice 35 + slice 30); adapter-shared just shipped to 85% over 4 slices. Update brief to reflect that the unblocker pattern (`ship the data shape, then ship the consumers`) is repeatable.
- The "10 src files" count in the adapter-shared ledger row is wrong — there are 6 source `.ts` files (adapter-utils, buffer-utils, card-utils, crypto, errors, index) + 2 build configs (tsup.config, vitest.config). Brief should require src counts to match `find packages/<x>/src -name "*.ts" -not -name "*.test.ts" | wc -l`.
- The refinement-cadence rule says "every 5 merge-backs". This pass covers 4 slices (37, 39, 40, 41 — slice 38 was card_utils which already counted toward the slice-36 refinement window). Working interpretation: count slices since the prior refinement, not raw merge-backs, and trigger when the count crosses 5. Add to brief.

**Edits applied**

- `scripts/codex-goal-chat/port-chat-sdk.md`: pending — apply on the next non-refinement slice cycle to avoid bloating this loop pass.
- `scripts/codex-goal-chat/goal-condition.md`: stable.

**Open refinements deferred**

- `crypto.rs` uses `aes-gcm` crate. Brief doesn't yet enumerate "crypto-grade" dependency rules (e.g. minimum versions, `RustCrypto` org preference). If more crypto code lands, write a short policy note.
- The adapter-shared ledger row enumerates `adapter_utils`, `buffer_utils`, `card_utils`, `crypto`, `errors` modules — that's the natural progression for *any* `packages/<x>` row. Brief could templatize the row format to enforce "N/M test files mapped + per-module case counts".
- Next phase-1 push: `packages/tests` (Vitest factories/matchers — needs a Rust analogue strategy; probably skip the test framework parity and port the *factories* as plain helper functions) and `packages/state-memory` (1 src + 1 test file, mechanical). Decision pending on packages/tests strategy.

### 2026-05-23 - slices 43..48

Slices reviewed: 43 (chat::emoji 1:1 complete), 44 (cards fallback text + adapter-shared verified), 45 (state-memory verified), 46 (tests/scripts/skills js-only-documented), 47 (callback_url pure helpers), 48 (plan types). Slice 42 was the last refinement.

**What the brief got wrong or left out**

- **"JS-only-documented at file/module level"** is now a working pattern. The chat row already excludes the JSX runtime files. The adapter-shared row excludes the Buffer/ArrayBuffer/Blob discrimination cases inside buffer_utils.test.ts and the null/undefined plumbing inside adapter-utils.test.ts, documented js-only-adjacent in the module headers. Brief should formalize: a package can reach `verified` when every PORTABLE upstream case is mapped 1:1, with non-portable cases documented in-place (module doc comment) AND ledger row notes. Avoid invoking the `js-only-documented` STATUS for individual files inside an otherwise-portable package; use module-header js-only-adjacent notes instead so the package row reads cleanly.
- **Whole-package `js-only-documented` is the right call for Vitest-glue + build-tooling + content-only surfaces.** Slice 46 marked packages/tests (Vitest factories + expect matchers, no Rust analogue), scripts/ (pnpm/turbo tooling), and skills/chat (SKILL.md content) as js-only-documented. Brief should canonize three concrete justifications callers can quote when classifying: (a) "Vitest/Jest-only test framework glue — Rust uses inline #[cfg(test)] mod tests + assert!", (b) "Node/pnpm/turbo build tooling — Rust workspace uses cargo build/test directly", (c) "Content-only Markdown spec — adopters consume the upstream copy verbatim". A surface that doesn't fit (a)/(b)/(c) needs a fresh defensible rationale.
- **The "pure helpers first, stateful/network deferred" pattern** played out cleanly on callback_url. The whole module's 17 test cases split 5/12 along the pure/stateful line; the 5 pure cases ported in one tight slice, the rest deferred until StateAdapter trait extension. Brief should canonize: when porting a chat module, first scan the upstream test file and triage cases as `pure` (no external deps), `stateful` (needs StateAdapter), `async-stream` (needs futures::Stream), `network` (needs HTTP), `class-bound` (needs Message/Channel/Thread). Ship the pure cases as a single slice; queue the rest behind concrete trait/dependency slices.
- **`std::sync::Mutex<State>` with `unwrap_or_else(|p| p.into_inner())` is the right pattern for in-memory state.** state-memory uses this pattern across 17 methods. The shared `with_state` helper closes over the mutex acquisition and recovers from poison via `into_inner`. Brief should canonize: don't propagate `PoisonError` to callers — recover via `into_inner`. Don't switch to `tokio::sync::Mutex` for in-memory backends; only use it when the lock spans an `.await` (network adapters).
- **Async strategy decision for state backends.** state-memory ships sync `&self` methods because the in-memory backend has no real I/O. Production backends (Redis, ioredis, Postgres) will be async. The `chat::types::StateAdapter` trait is currently the empty placeholder; the design decision to keep methods OFF the trait (and on inherent impls) defers the async-trait question until at least 2 backends exist. Brief should document this decision and the migration plan: when ioredis or pg lands, lift methods into the trait via either `async fn in trait` (Rust 1.85+) or `async-trait` macro (boxed futures, dyn-safe).
- **Untagged enum with primitive variant + struct wrappers needs explicit wrapper structs for `{ markdown: ... }` shapes.** Slice 48 modeled `PlanContent` (upstream `string | string[] | { markdown } | { ast }`) as `enum PlanContent { Text(String), Strings(Vec<String>), Markdown(PlanMarkdownContent), Ast(PlanAstContent) }`. Inlining `Markdown(String)` would have failed serde untagged disambiguation (a JSON string would match `Text`, not `Markdown`). The wrapper struct carries the key as a field; serde's untagged matcher picks the variant that has the matching field shape. Brief: when a TS discriminated union mixes primitive and object variants, model the object variants as named wrapper structs even if they have a single field. This compiles into a clean wire shape and disambiguates correctly.
- **NoUpstreamTestFile means additive Rust coverage, not "skip the module".** Slice 41 (crypto, 15 additive tests) and slice 48 (plan, 10 additive tests) ported modules whose upstream `.ts` ships without a `.test.ts`. Brief: when a source file has no test file, the ledger row should say "additive Rust coverage (N tests)" — not "1:1 of M cases". The Rust tests verify wire shape + each public function's branches; they are an audit trail for the port, not a parity claim.
- **Per-slice merge-back cycle is reliably ~10 minutes** when no surprise dependencies surface. Slice 47 (callback_url, 5 cases, ~5 min) and slice 48 (plan, 10 cases, ~5 min) both completed cleanly. Slice 44 (adapter-shared verified) was the outlier at ~20 min because it required adding card_to_fallback_text to chat::cards FIRST. Brief: when a slice has an "X depends on Y but Y isn't ported" wall, plan the slice as TWO commits chained (chat::Y → adapter::X) rather than one mega-slice. Reduces rebase risk and keeps each commit reviewable.

**Stale or misleading guidance**

- The brief still implies "every slice must extend the StateAdapter trait". After slice 45 (state-memory) we know that inherent impls + an empty trait placeholder is fine until at least 2 backends exist. Update brief to: "extend StateAdapter when a second backend lands and the method set is settled; until then ship inherent methods on the concrete struct".
- The slice-31 refinement said the slice-budget for `packages/chat` was 70-100 slices to verify. After 48 slices total (with chat at 70% and the structurally-heavy modules — channel, chat, thread — not started), the realistic estimate is closer to 200-300 slices for chat alone, plus 100-200 per adapter. Update the estimate.

**Edits applied**

- `scripts/codex-goal-chat/port-chat-sdk.md`: pending — captures the seven new canonical rules above plus the StateAdapter-trait decision. Apply on the next non-refinement slice cycle.
- `scripts/codex-goal-chat/goal-condition.md`: stable.

**Open refinements deferred**

- `chat::message` is the highest-leverage remaining module (unblocks thread, channel, transcripts, reviver, thread-history). Its serialization shape is documented; porting the Message data struct + SerializedMessage + Message::fromJSON / toJSON would unblock 4+ downstream modules in subsequent slices.
- The `chat` row's test-count math is getting cumbersome ("17 + 13 + 5 + 33 + 29 + 13 + 42 + 5 + 10 + 76"). Brief should suggest just citing the rolled-up total ("243 tests across N modules") and putting per-module breakdowns in a separate section.
- The brief's "every 5 merge-back cycles" refinement cadence has produced consistently useful refinements (entries for slices 1-5, 7-12, 14-18, 20-24, 26-30, 32-35, 37-41, 43-48). Keep it.

### 2026-05-23 - slices 50..58

Slices reviewed: 50 (chat::message portable subset), 51 (modals empty-options panics + missing cases), 52 (adapter-web js-only-documented), 53 (mock-adapter + message-history js-only documented), 54 (integration-tests js-only-documented), 55 (examples/telegram-chat js-only-documented), 56 (chat::transcripts parse_duration), 57 (chat::postable_object trait + shape guard), 58 (chat::reviver _type dispatcher). Slice 49 was the last refinement.

**What the brief got wrong or left out**

- **Whole-package js-only-documented carries the port forward when an entire surface is Vitest/browser/JSX-bound.** Slices 52 (adapter-web), 54 (integration-tests), 55 (examples/telegram-chat) all hit the same pattern: every src file uses a JS-only runtime (DOM/AsyncLocalStorage/createUIMessageStream, JSX `.tsx`, Vitest `vi.fn()` + replay snapshots). Brief should expand the three vetted justifications from slice 42 with a fourth: (d) "browser-framework UI integration (React/Svelte/Vue) + tightly-coupled JS streaming protocol". The chat package row's "Phase-2 adapters cannot start until Phase-1 verified" constraint should also explicitly exempt adapter-web since it can never reach verified.
- **Sub-file js-only-adjacent notes vs. whole-row js-only-documented status.** Slice 53 added `mock-adapter.ts` and `message-history.ts` to the JavaScript-only Exceptions table (not the inventory row status). Reaffirms the slice-42 rule: the STATUS column applies to whole rows; per-file Vitest-glue or deprecated-shim exclusions go in the Exceptions table. Brief should list every legitimate sub-file justification: Vitest glue (mock-adapter), deprecated re-export shim (message-history), JSX runtime (jsx-runtime/jsx-react), Symbol-method-only (workflow serde Symbol entries on Message).
- **The Adapter trait gates 8+ chat modules.** Slices 47, 50, 56, 57, 58 all hit the same wall: callback_url's stateful path, message::subject, transcripts::TranscriptsApiImpl, postable_object::post_postable_object, reviver's Thread/Channel branches all need methods on the chat::types::Adapter trait that doesn't have them yet. Brief should canonize: when a chat module has X portable cases + Y Adapter-bound cases, ship the X cases as a slice, document the Y as deferred, and move on. Don't block the slice on a giant trait extension.
- **Trait Debug derivation needs hand-rolled impl when fields hold `Arc<dyn Trait>` where Trait doesn't require Debug.** Slice 57's PostableObjectContext: Adapter requires `+ Debug`, Logger does not. `#[derive(Debug)]` fails on the latter. Hand-write the Debug impl that elides the Logger field. Brief should canonize this as a sub-rule under the placeholder-trait priority: when adding a context struct holding multiple placeholder-trait pointers, audit each trait's Debug bound before deriving.
- **`Symbol.for("...")` symbols collapse to string literals on the Rust wire.** Slice 57: upstream `POSTABLE_OBJECT = Symbol.for("chat.postable")` becomes `const POSTABLE_OBJECT_DISCRIMINATOR = "chat.postable"`. The Rust port can't have JS Symbol identity but the string-typed wire format gives equivalent semantics across a network boundary, and `is_postable_object` checks the lowered string just like upstream's `JSON.parse(reviver)` would once a symbol crosses a boundary. Brief should add this as a JS-symbol-lowering rule under priority 5(e).
- **`JSON.parse(s, reviver)` has no direct Rust analogue.** Slice 58's reviver port exposes `revive_value(Value) -> Revived` as a post-parse step instead of a callback. Brief should canonize: when a TS source uses a JSON.parse reviver, the Rust port adds a `revive_*` function that takes the already-parsed `serde_json::Value` and returns a typed enum (`Revived::Message(...)` etc). Tests assert the dispatch + pass-through branches.
- **Permissive fall-through on malformed payloads.** Reviver and any other JSON dispatcher should NOT panic on shape mismatch; it should pass the raw Value through unchanged. Mirrors upstream's `try/catch` posture inside JSON.parse(reviver). Brief should canonize: revive-style helpers return PassThrough for invalid shapes; don't propagate `serde_json::Error`.

**Stale or misleading guidance**

- The brief says "Adapter and StateAdapter are placeholder traits, extend them when adapters land." After 20 slices, the practical pattern is "do NOT extend the placeholder until 2 concrete impls exist." For chat modules whose Adapter-bound parts ship now, document them as deferred and move on. Update the brief.
- The slice-budget estimate from slice 49's refinement (200-300 slices for chat alone) still holds — slices 50-58 added 7 chat modules (callback_url, message, plan, transcripts, postable_object, reviver, plus expanded modals) but only nudged chat from 70% to 74%. The remaining structurally heavy modules (channel ~600 LOC, thread ~1100, chat.ts ~2700) plus the full markdown stringifier remain.

**Edits applied**

- `scripts/codex-goal-chat/port-chat-sdk.md`: pending — captures the six new canonical rules above (whole-package vs sub-file js-only split, Adapter trait deferred-until-2-impls, hand-rolled Debug for placeholder-trait contexts, Symbol -> string discriminator lowering, JSON.parse(reviver) -> revive_value pattern, permissive PassThrough for malformed payloads). Apply on the next non-refinement slice.
- `scripts/codex-goal-chat/goal-condition.md`: stable.

**Open refinements deferred**

- The chat ledger row is at 74% with 11 chat modules now portable-mapped. The remaining 9 unmapped test files (channel, chat, from-full-stream, serialization, streaming-markdown, thread-history, thread, transcripts-wiring, transcripts) need either real ports (channel, chat, thread are structurally massive) or trait extension (transcripts.test.ts maps to TranscriptsApiImpl which is Adapter-bound). Brief should add a per-test-file triage column: pure / state-bound / adapter-bound / class-bound, so future slice planning is mechanical.
- Phase-2 adapter packages cannot reasonably reach verified within this `/goal` session's budget. Each adapter is 7-24 src files of platform-specific HTTP/SDK code (Slack RTM/Web API, Teams Bot Framework, Google Chat REST, Discord gateway, Linear GraphQL, GitHub REST/GraphQL, Messenger webhook, Telegram bot API, WhatsApp Cloud API). Reality check: each adapter is its own multi-day port effort. They will land across many subsequent sessions, not this one.

### 2026-05-23 - slice 78 critical: validation-gate-bypass incident

Slice 78 introduced a compile error in
parse_markdown_code_block_without_language_has_none_lang (a
get_node_children temp-lifetime issue). The atomic-validation-gate
caught the failure locally — but the `git push` step still ran,
landing the broken commit on main as `4f696bf`. Slice 79 fixed it
within minutes (`89b9b77`), but the bypass is a structural flaw in
the merge protocol that future sessions MUST fix:

**Root cause.** The brief's atomic-merge-gate recipe in
`scripts/codex-goal-chat/port-chat-sdk.md` documents
`if ! ( set -e; <validation>; ); then exit 1; fi`. When this is
used inside a one-liner chain
`merge && if ! (...); then exit 1; fi && push`,
the `exit 1` inside the if-block only exits the SUBSHELL containing
the validation, not the outer one-liner. Bash treats the if as
having exit code 0 (the if-block matched), so the `&&` chain
continues to the push.

**Fix.** Restructure the merge-back protocol so push is OUTSIDE the
&&-chain that runs validation, and gate it on an explicit `$?`
check. Equivalent to:

```bash
merge_back() {
  cargo fmt --all --check || return 1
  bash scripts/check-naming-conventions.sh || return 1
  cargo clippy --all-targets --all-features -- -D warnings || return 1
  cargo test --workspace --all-features || return 1
}
merge_back && git push origin main && rmdir /tmp/...
```

The `&&` operator propagates failures correctly when EACH gate is a
direct command return value, not a wrapped if-block.

Brief should canonize: the merge-back protocol uses straight-line
&&-chains of individual gate commands, NOT wrapped if-blocks. Add
a one-line shell sanity test to ensure a deliberate `false` inside
the gate stops the push (regression test for this incident).

**Open refinement** (deferred):

- Add a CI hook that runs validation gates BEFORE accepting a
  merge to main, so even if the local shell script has a logic
  flaw, the bad commit can't ship to origin. The current protocol
  relies entirely on the developer's local validation succeeding.

### 2026-05-23 - slice 89 second validation-bypass incident

Slice 89 added a markdown test asserting `plain.len() >= 2500` on
a 500-repetition "word " input. markdown-rs collapses inline
whitespace runs so the actual rendered text was shorter; the
assertion failed locally. Slice 90 fixed it (count tokens, not
bytes) and shipped — but the BROKEN slice 89 commit ALSO reached
main as `b67acd7`.

Root cause: the slice-80 refinement said "use straight-line && chains
without wrapped if-blocks", but the current protocol still has each
gate command piped through `tail -3` to trim output. `command | tail -3`
returns 0 whenever `tail` reads any bytes — masking `command`'s
exit code. The `&&` chain sees `tail -3` succeed and proceeds to
`git push`.

**Concrete protocol fix.** The merge-back validation gate must invoke
each gate command WITHOUT a trailing pipe. Compare:

```bash
# BROKEN (hides exit code via tail's exit 0):
cargo test --workspace 2>&1 | tail -3 && git push origin main

# CORRECT (preserves exit code; output goes to stderr/stdout as-is):
cargo test --workspace && git push origin main
```

Captured-output is for monitoring, not gates. If you need to trim
output, pipe to `tail` OUTSIDE the &&-chain inside a subshell whose
exit code you don't care about, or use `set -o pipefail` so the
pipe inherits the failed command's exit code.

**Updated brief rule (apply on next non-refinement slice):** all
validation gates in the merge-back protocol must be plain commands
with their raw exit codes propagated through `&&`. No trailing
`| tail`, `| head`, `| grep` in the chain. If output trimming is
needed for readability, `set -o pipefail` MUST be active in the
shell so the pipe inherits the failing command's exit code.

**Open refinement:** add a `make merge-back` target (or shell
function) that codifies this protocol so individual slice commits
can't accidentally re-introduce a pipe-in-the-chain regression.

### 2026-05-23 - slices 90..97

Slices reviewed: 90 (slice-89 fix for very-long-paragraph
assertion), 91 (second validation-bypass post-mortem), 92 (chat
row bumped to 80% reflecting markdown 1:1 complete), 93 (Plan
data struct + getters + fallback text + post_data), 94 (Plan
model-update helpers add_task/update_task_in_model/complete_in_model),
95 (StreamingPlan + StreamingPlanOptions + GroupTasksMode), 96
(transcripts is_tombstone + tombstone factory +
user_transcript_key), 97 (chat row bumped to 82%).

**What the brief got wrong or left out**

- **Avoid asserting byte length on parser output.** Slice 89's
  failing test asserted `plain.len() >= 2500` on a 500-token
  "word " input, expecting whitespace preservation. markdown-rs
  collapses inline whitespace runs (which is also CommonMark
  behavior for the rendered text payload). Brief should canonize:
  asserting raw byte/length on a parser's output is brittle;
  count meaningful tokens via `matches("token").count()` instead.
- **Class-with-adapter-binding ports split cleanly into "model"
  and "adapter" surfaces.** Slices 93-94 ported the in-memory
  model portion of upstream class Plan (constructor, getters,
  fallback text, add_task, update_task_in_model, complete_in_model)
  without touching the Adapter trait. Brief should canonize: when
  porting a class that mixes in-memory state with adapter-bound
  dispatch, split the surface — ship the in-memory portion now,
  defer the dispatch portion until the Adapter trait lands. The
  in-memory portion is usually 60-80% of the class's footprint.
- **AsyncIterable -> `Vec<Value>` for non-stream-consuming code
  paths.** Slice 95's StreamingPlan stores its event stream as
  `Vec<serde_json::Value>` rather than picking an async-runtime
  Stream type. Adapters consume the values via from_full_stream's
  sync iterator. Brief should canonize: until an async-runtime
  decision lands, `AsyncIterable<T>` -> `Vec<T>` is the conservative
  port. Document a TODO in the struct header to swap to
  `futures::Stream` in a future slice.

**Stale or misleading guidance**

- The slice 80/91 refinements documented that the merge-back &&
  chain must not contain trailing pipes (`| tail`, `| head`,
  `| grep`). Slice 97's commit used a `2>&1 | grep "test result"`
  fragment in the chain — the gate happened to pass, but the pipe
  still masks exit codes. The remaining application of this rule
  is to switch the per-slice protocol to use `grep test result`
  AFTER the && validation succeeds, or to use `cargo test ...; echo
  RESULT=$?` patterns. Open refinement: codify a `make merge-back`
  target so individual slices can't accidentally re-introduce
  pipe-in-chain.

**Edits applied**

- `scripts/codex-goal-chat/port-chat-sdk.md`: pending (apply on next
  non-refinement slice).
- `scripts/codex-goal-chat/goal-condition.md`: stable.

**Open refinements deferred**

- `chat::callback_url`, `chat::message::subject`,
  `chat::postable_object::post_postable_object`,
  `chat::transcripts::TranscriptsApiImpl`, and most chat-bound
  reviver branches all sit behind a `chat::types::Adapter` and/or
  `chat::types::StateAdapter` trait extension. A dedicated
  trait-extension slice with `async-trait` for dyn safety would
  unblock 5+ chat modules at once and accelerate progress
  measurably.

### 2026-05-24 - slices 103..107

**Slices covered**

103 (cards 4 more 1:1 cases), 104 (chat row bumped to 84%),
105 (message buffer-strip helper: Attachment::without_inline_data
+ Message::to_serialized_stripped, message 10/19 -> 12/19), 106
(callback_url additive helpers: is_callback_value +
callback_cache_key, callback_url tests 5 -> 10), 107 (reviver
revive_str helper: 1:1 with JSON.parse(text, reviver), chat
bumped 84% -> 85%; reviver tests 6 -> 10).

**What the brief got right (validated)**

- The "model/adapter split" rule from slices 93-94 keeps paying
  off. Slice 105 pulled `Attachment::without_inline_data` (pure
  helper) plus `Message::to_serialized_stripped` (uses it across
  attachments) without touching the Adapter trait, mirroring
  upstream Message.toJSON()'s buffer-strip behavior. The
  remaining 5 subject getter cases stay deferred until trait
  extension - the line between "ship now" and "defer" remains
  clean.
- Pure-helper formatters analogous to user_transcript_key are
  high-yield: slice 106 added `is_callback_value` and
  `callback_cache_key` to mirror upstream's inline
  `value.startsWith(...)` and `${CALLBACK_CACHE_KEY_PREFIX}${token}`
  patterns. These get pulled out of upstream's inline templates
  with zero behavioral risk and let the future stateful slice
  call into a single helper rather than re-inlining the format
  literal.
- Combining helpers ("parse + revive in one step", "encode +
  prefix", etc.) tend to map directly to canonical upstream call
  sites. Slice 107's `revive_str` is the 1:1 of upstream's
  canonical `JSON.parse(text, reviver)` and earns its
  test-count.

**What the brief got wrong or left out**

- **Test-count bumps in `package-progress-estimates.tsv` need
  the same care as the per-test ledger.** Slice 105 missed
  bumping the message count in the tsv basis text; slice 107
  caught both the reviver count AND the percentage. Open
  refinement: every per-module slice should re-run
  `scripts/package-progress-table.sh` and verify the basis-text
  module count matches the actual `cargo test ... | grep` output
  before the merge-back. Codify as a final-step checklist item
  in `scripts/codex-goal-chat/port-chat-sdk.md`.
- **The atomic merge-back protocol works when the main worktree
  is clean and the lock dir is owned only by the current
  session.** Slice 105 hit a hang when the bash backgrounded
  itself and the lock dir stayed held; killing the parent shell
  recovered. Open refinement: the merge-back command should
  always foreground; explicitly pass `run_in_background: false`
  on the Bash call so the harness doesn't decide for us.
- **Additive pure helpers are still worth shipping in their own
  slices even when they don't bump the percentage.** Slice 106
  added 5 callback_url tests but didn't move the percentage
  (the % math weights upstream-mapped cases more than additive
  ones). That is fine - the helpers shrink the future stateful
  slice's surface and make it strictly less complex.

**Stale or misleading guidance**

- Refinement entry on slice 97 said: "use `grep test result`
  AFTER the && validation succeeds". Slice 105's first attempt
  did exactly that (test ran before push). The remaining
  stale-guidance issue: the merge-back command has gotten long
  enough that the `until mkdir lock; ...; rmdir lock` chain is
  ~8 piped commands. A `Makefile` target or a helper script
  would be safer than relying on bash chain hygiene each slice.
  Open refinement: ship a `scripts/codex-goal-chat/merge-back.sh`
  that takes the slice number and message, runs the gate, and
  pushes - then per-slice commits only have to call the script.

**Edits applied**

- `scripts/codex-goal-chat/port-chat-sdk.md`: pending (apply on
  next dedicated refinement slice).
- `scripts/codex-goal-chat/goal-condition.md`: stable.

**Open refinements deferred**

- Same single open item as slice 97's entry: the
  `chat::types::Adapter` / `chat::types::StateAdapter` trait
  extension is now the only realistic path to bumping chat past
  ~88%. Recent slices have been hand-picking pure helpers; the
  pure surface is approaching exhausted. Next refinement cycle
  should plan the trait-extension slice explicitly (method list,
  `async-trait` dependency, MemoryStateAdapter trait impl shim)
  so the next 5 slices can land it across the trait + 4 consumer
  modules (callback_url, transcripts, thread_history,
  postable_object).
