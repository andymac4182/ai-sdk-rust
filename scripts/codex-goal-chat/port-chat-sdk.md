# `/goal` Brief: Full Vercel Chat SDK Parity In Rust

You are an agent (Claude Code or Codex CLI) running a long-lived `/goal`
session porting upstream [`vercel/chat`](https://github.com/vercel/chat) into
Rust inside the `ai-sdk-rust` repo. This repo also hosts a separate,
concurrent port of `vercel/ai`; that work is owned by `scripts/codex-goal/`
and is not your responsibility.

You are allowed to work for a long time. This is not a one-slice task. Take
bigger, coherent slices than a normal short coding session. After every
coherent validated slice, commit it on your worktree branch and merge it back
to `main` using the merge protocol below before continuing.

## Repository

The main checkout is:

```sh
/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust
```

The launcher creates an explicit git worktree under:

```sh
/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust-chat-worktrees
```

Treat the current working directory as the only editable source workspace. Set
these variables before running merge-back commands:

```sh
main_repo="/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust"
worktree="$(git rev-parse --show-toplevel)"
branch="$(git branch --show-current)"
```

If `worktree` is the same path as `main_repo`, stop immediately. You were
launched in the wrong directory. Do not edit the main checkout. The main
checkout is only the serialized merge-back target.

If `.env.local` exists, it is ignored and may be a symlink to the main
checkout's secret file. Adapter credentials (Slack tokens, Discord bot tokens,
Telegram tokens, etc.) live there when integration validation is useful. Never
print the values, copy them into tracked files, or include them in
logs/commits.

## Coexistence with the ai-sdk port (CRITICAL)

A second `/goal` session is concurrently porting `vercel/ai` in this same repo
on the same `main` branch. Your contract with that session is:

| Resource | Owner |
| --- | --- |
| `crates/ai-sdk-*`, `src/`, root `Cargo.toml` `ai-sdk-rust` package metadata | ai-sdk session |
| `docs/upstream-parity.md`, `docs/package-progress.md`, `docs/package-progress-estimates.tsv` | ai-sdk session |
| `scripts/codex-goal/`, `scripts/run-codex-goal-port.sh`, `scripts/run-gnhf-port.sh`, `scripts/gnhf-codex-xhigh.sh` | ai-sdk session |
| `crates/chat-sdk-*` (you create these) | chat session (you) |
| `docs/chat/upstream-parity.md`, `docs/chat/package-progress.md`, `docs/chat/package-progress-estimates.tsv`, `docs/chat/goal-refinements.md` | chat session (you) |
| `scripts/codex-goal-chat/`, `scripts/run-codex-goal-chat-port.sh` | chat session (you) |
| Workspace `[workspace]` `members` list in root `Cargo.toml`, `Cargo.lock` | shared, additive edits only |
| `scripts/package-progress-table.sh`, `scripts/check-naming-conventions.sh`, `scripts/check-otel-loopback.sh` | shared, additive edits only |
| `README.md`, `.github/`, `rust-toolchain.toml`, `.gitignore` | shared, additive edits only |

Hard rules:

1. Never modify a file owned by the ai-sdk session. If a shared file needs
   structural change to accommodate the chat port, prefer adding chat-specific
   variants alongside (a new flag, a new file, a new path) instead of
   restructuring the shared file.
2. Never delete or rename any `crates/ai-sdk-*` directory or any file under
   `docs/upstream-parity.md` family of ai-sdk files. The ai-sdk session may be
   actively modifying those between your reads and writes.
3. Your crates always live at `crates/chat-sdk-*` with crate names of the form
   `chat-sdk-<upstream-package-name>`. Mirror upstream package boundaries
   1:1 just like the ai-sdk port does.
4. The naming-conventions checker (`scripts/check-naming-conventions.sh`)
   applies to every crate and file you add. If you need to mirror an upstream
   package whose name contains a banned token, add a documented exception
   inside the script alongside the existing ai-sdk exceptions, do not relax the
   rule globally, and call it out in your slice commit.
5. The workspace `Cargo.toml` `[workspace] members` list is shared. When you
   add a `chat-sdk-*` crate, append to the existing list rather than
   re-ordering or restructuring it. Resolve merge conflicts by union-merging
   the lists.
6. Both sessions share the same merge lock
   `/tmp/ai-sdk-rust-main-merge.lock`. Always use it. Never use a different
   path. The lock serialization is what keeps both sessions safe.

## Objective

Replicate the full Vercel Chat SDK repository in idiomatic Rust under
`crates/chat-sdk-*` inside this workspace.

The goal is not "make progress". The goal is full parity with upstream
`vercel/chat`: every package, adapter, library, public API, example, testable
behavior, and feature should have a Rust equivalent under `crates/chat-sdk-*`,
except surfaces that are truly JavaScript-only and are explicitly documented
as intentionally non-portable.

Use upstream Vercel Chat SDK as the source of truth for shapes and behavior:

```sh
npx opensrc@latest path github:vercel/chat
# or, for refresh of cached source:
npx opensrc fetch github:vercel/chat
```

Do not decide the goal is complete until an upstream parity ledger proves there
are no unchecked upstream packages, adapters, public APIs, examples, tests, or
features left.

Non-negotiable test floor: EVERY portable test/case from the original upstream
TypeScript packages must exist as an equivalent Rust test in the matching 1:1
`chat-sdk-*` crate. Rust may add more tests for stronger coverage, but it must
never have fewer mapped original TypeScript tests, and one missing upstream
portable test is a completion blocker. Treat this as inventory containment, not
a best-effort coverage goal: enumerate the original TypeScript tests first,
port every portable test/case into Rust, and only then add Rust-specific
tests. The acceptable state is
`original TypeScript tests <= mapped Rust tests`.

## Required Parity Ledger

First action: create or update `docs/chat/upstream-parity.md`.

The ledger must include:

1. The upstream `vercel/chat` commit SHA/date used for inventory.
2. A package inventory from the upstream repo, including all packages, adapter
   packages, utility libraries, examples, skills, scripts, state-management
   add-ons, transport modules, tests, docs, and tooling that affects public
   behavior.
3. For each upstream package/feature: status (`not-started`, `in-progress`,
   `ported`, `verified`, or `js-only-documented`), Rust crate/module path
   (always under `crates/chat-sdk-*`), tests/examples covering it, and notes
   about intentional Rust differences.
4. An adapters section. Every upstream adapter package (slack, teams,
   discord, telegram, github, linear, whatsapp, google-chat, ...) must be
   listed, even if the first implementation is a typed contract and
   fake/test transport before real HTTP wiring.
5. A high-level APIs section covering message lifecycle, event dispatch,
   command parsing, state persistence, middleware, error handling, prompt/
   template surfaces, AI agent integration, and any other upstream public API.
6. A "next unported work" queue. At the end of every slice, update this queue
   before committing.
7. A named test-case parity map for every portable original upstream
   TypeScript test/case, showing the matching Rust test in the owning 1:1
   `chat-sdk-*` crate or an explicit JavaScript-only/non-portable
   justification.
8. Package-level progress estimates in
   `docs/chat/package-progress-estimates.tsv`. Keep estimates conservative
   and update the row for any package touched by a slice. These are estimates
   only for `in-progress` package rows; the generator forces `verified` and
   `js-only-documented` rows to 100% and `not-started` rows to 0%. **Editing
   the TSV requires literal tab characters** — heredocs and the `Write` tool
   may convert tabs to spaces, which makes the progress-table generator fail
   with an opaque `undefined method [] for nil` Ruby error. Write the TSV
   via `printf '%s\t%s\t%s\n' "$pkg" "$pct" "$basis" >> docs/chat/package-progress-estimates.tsv`
   (or similar `printf` with explicit `\t`), then verify with
   `awk '{ gsub(/\t/, "<TAB>"); print }' docs/chat/package-progress-estimates.tsv`.
   The generator forces `verified` and
   `js-only-documented` rows to 100% and `not-started` rows to 0%.

After updating package status or estimates, run:

```sh
scripts/package-progress-table.sh \
  --ledger docs/chat/upstream-parity.md \
  --estimates docs/chat/package-progress-estimates.tsv \
  --output docs/chat/package-progress.md \
  --title "Chat SDK Rust Package Progress"
```

Use that generated table when reporting migration progress.

Re-scan upstream often with `npx opensrc@latest path github:vercel/chat`. If
the upstream inventory changes, update the ledger and continue. Do not stop
while the ledger contains `not-started` or `in-progress` items unless you hit
a real blocker that needs human input.

## Required Work Order

The implementation order is a hard two-phase gate:

1. Finish ALL core/shared upstream packages first. This includes the upstream
   `chat` package (the unified SDK surface) and any shared transport,
   state-management, type, and test-support packages.
2. Only then resume standalone adapter packages (Slack, Teams, Discord,
   Telegram, GitHub, Linear, WhatsApp, Google Chat, etc.).

Do not pick an adapter slice while any core/shared row is still
`not-started` or `in-progress`, unless that row is explicitly documented as
intentionally non-portable.

## Priorities

1. Preserve Rust 2024 style, serde shapes, builder helpers, error/result
   style, and public exports.
2. **Colocate tests with the code they exercise.** Each `chat-sdk-*` source
   file `src/<module>.rs` must end in a
   `#[cfg(test)] mod tests { use super::*; ... }` block containing every
   `#[test]` ported from the matching upstream `*.test.ts`. The
   `crates/<crate>/tests/` directory is reserved for genuine cross-crate
   integration tests that exercise only the public API. This matches the
   ai-sdk-rust workspace style — see `crates/ai-sdk-*/src/*.rs::tests` —
   and was raised explicitly by the user during slice 2 of this port.
3. Align JSON boundaries with upstream contracts while omitting
   JavaScript-only concepts (e.g. AbortSignal, Promise) where the Rust
   equivalent differs.
4. **Port `packages/chat/src/types.ts` in layers, not in one slice.**
   Upstream `types.ts` is 2,549 lines and transitively imports from
   `cards`, `channel`, `message`, `modals`, `postable-object`, `thread`,
   and `jsx-runtime`. Land the standalone leaf types first (already
   shipped in slice 4), then the emoji layer (slice 5), then one layer
   per dependency module as that module lands. Each layer's slice
   extends `crates/chat-sdk-chat/src/types.rs` with the next batch of
   types it unblocks; do not block on porting `types.ts` whole.
5. **Placeholder pattern — defer big decisions, ship dependents now.**
   - **Placeholder traits.** For any `interface X` with more than ~5
     methods that would otherwise block downstream modules, ship an
     empty `pub trait X: Send + Sync + Debug {}` in `types.rs` (or the
     owning module) and grow the trait one slice at a time as each
     dependency module lands. NEVER define a new trait for the same
     upstream interface — every adapter/state slice extends the
     canonical placeholder. Slice 14 (`Adapter`, `StateAdapter`)
     established this; slice 20 (`LockScopeContext`) was the first
     consumer and validated that the pattern compiles cleanly.
   - **Placeholder type aliases.** When a type is defined in upstream
     as `export type X = <ExternalLib>.Y` and the decision over which
     Rust analogue of `<ExternalLib>` to bring in is itself a separate
     architectural slice, ship `pub type X = serde_json::Value;` as a
     placeholder. Every downstream type that holds `X` automatically
     picks up the future typed version when the alias is swapped. Slice
     22 (`FormattedContent = serde_json::Value`, opaque mdast) is the
     canonical example — the markdown-crate decision can land in one
     coordinated slice without re-touching AppendInput, TranscriptEntry,
     or any other holder.
   - **Data shape + elided callback recipe.** When an upstream
     interface bundles serializable data with `() => Promise<T>`
     methods (e.g. `Attachment.fetchData`, `LinkPreview.fetchMessage`),
     port the data fields only and document the elided callback inline
     by upstream name. The callback graduates to a trait method on the
     placeholder `Adapter` trait when the matching adapter module
     lands. Slice 23's `Attachment` and `LinkPreview` established this.
   - **Discriminated union recipe.** Port a TypeScript discriminated
     union (`export type X = A | B | C` where each variant struct
     carries `type: "literal"`) as a Rust `#[serde(untagged)]` enum
     wrapping variant structs that each carry a per-struct unit-enum
     discriminator field (e.g. `ButtonKind::Button` serializing as
     `"button"`). Serde's untagged matcher disambiguates variants by
     the existing `type` field — no outer wrapper, JSON shape matches
     upstream exactly. **Always provide `From<VariantStruct>` impls on
     the union enum** so call sites can write
     `actions(vec![button(...).into(), link_button(...).into()])`
     reading 1:1 with upstream `Actions([Button(...), LinkButton(...)])`.
     Slices 34's `ActionsChild` and 35's `CardChild` established this.
   - **Data-shape vs behavior slice split.** When porting a module
     that mixes "data types + builders" with "a renderer/extractor
     that walks every variant", ship the data-shape surface first as
     one cohesive slice (once the unions land), then the behavior in a
     follow-up slice. The behavior slice gets to iterate every variant
     exhaustively against the now-stable shape. Slice 30 (`table_to_ascii`
     ports after the markdown AST in slice 26) and slice 35 (cards
     data shape) -> deferred `Card.toAscii` (behavior) are canonical.
6. **Never panic while holding a `std::sync::Mutex` guard you might
   want to reuse.** A panic inside a `lock().expect(...)`-then-`expect`
   chain poisons the mutex for every sibling test that runs in
   parallel. Snapshot the inner value under the lock, drop the guard,
   *then* optionally panic. Every mutator that may run after a poisoned
   lock should use `.unwrap_or_else(|poisoned| poisoned.into_inner())`
   so a sibling test's panic cannot cascade. Slice 14's
   `chat_singleton::get_chat_singleton` regressed this once before the
   fix; do not let the pattern slip back.
7. **Generic upstream types must include a typed-substitution test.**
   `interface X<T = unknown>` ports as
   `pub struct X<T = serde_json::Value>` plus *two* colocated tests:
   one using the default `serde_json::Value` and one using a concrete
   user-defined struct. The second test proves the generic is not
   accidentally tied to `Value`. See slice 17's `RawMessage<TRaw>` for
   the canonical example.
8. **Every tagged-union enum gets one negative-path test.**
   `#[serde(tag = "type", ...)]` enums must include a test that asserts
   `serde_json::from_str(...)` returns `Err` for a JSON object missing
   the discriminator. Mirrors the upstream TypeScript compile-time tag
   check at Rust runtime. See slice 15's
   `stream_chunk_untagged_object_fails_to_deserialize`.
9. **`js-only-documented` is a real slice type.** Marking an upstream
   surface non-portable in the ledger plus the JavaScript-only
   Exceptions table counts as a normal slice if (a) the rationale is
   non-trivial and concrete (cite the JS-only API used: React, Next.js,
   `vi.fn`, Node Buffer, etc.) and (b) the ledger row and Exceptions
   row land together. Pure-classification slices count toward the 5-
   merge refinement cadence.
10. **Single-field structs count as proper slices.** A 9-line
    `pub struct X { pub field: T }` plus a colocated wire-format
    round-trip test is a complete slice if the type cements a stable
    upstream JSON contract. See slice 21's `PostEphemeralOptions`.
11. **Skip the `Hash` derive when any field cannot be hashed.**
    `HashMap`, `BTreeMap`, `serde_json::Map`, `serde_json::Value`, and
    `Vec<NonHash>` block the default `#[derive(Hash)]`. Data types
    containing any of these fields ship with `Debug, Clone, PartialEq,
    Eq, Serialize, Deserialize` (no `Hash`). Slice 23's `Attachment`
    regressed this once before the fix; do not let it slip back.
12. **Structurally-similar types need a wire-distinction test.** When
    two ported types share most of their fields and differ only in a
    single required key (e.g. `PostableRaw.raw` vs
    `PostableMarkdown.markdown` in slice 24), add a colocated test that
    asserts `serde_json::to_string` of each renders distinct JSON.
    Adapters branch on which key is present, so the wire-shape
    invariant is load-bearing — make it test-enforced.
13. Add focused serialization/deserialization and behavior tests for every new
   public contract.
14. **Pure-ASCII ledger and TSV.** `docs/chat/upstream-parity.md` and
    `docs/chat/package-progress-estimates.tsv` must contain only ASCII
    characters. Em-dashes (`-`), curly quotes, arrows, and similar Unicode
    punctuation break the Ruby progress-table generator (slices 26 and 30
    regressed this). When pasting prose, prefer plain hyphen, straight
    quotes, and `->`. Recovery on a regression:
    `python3 -c "import sys; p = sys.argv[1]; t = open(p, encoding='utf-8').read(); open(p, 'w', encoding='utf-8').write(t.replace(chr(8212), '-').replace(chr(8594), '->'))" docs/chat/upstream-parity.md`.
15. **Heredoc commit messages when the body mentions Rust generics or
    TypeScript unions.** `git commit -m "<...Option<Node>...>"` blows up
    because the shell interprets `<`, `>`, `|`, and `(`. Always wrap such
    messages in `git commit -m "$(cat <<'EOF' ... EOF)"`.
16. **markdown-rs is the chosen markdown stack.** `markdown = "1.0.0"` is
    the Rust analogue of upstream `remark-*` + `mdast`. `Node::to_string()`
    is the upstream `mdast-util-to-string` plain-text extractor. When
    porting an AST visitor or transformer over `markdown::mdast::Node`,
    keep `markdown.rs::children_mut` updated as the canonical
    "container variants" enumeration — every new visitor reuses the same
    helper rather than re-enumerating the variant list.
4. Port EVERY portable test from the original upstream TypeScript package
   into Rust before marking that package row `verified`. This is a hard
   minimum: Rust may add more tests for Rust-specific safety, typing, and
   failure modes, but it must never have fewer portable tests than upstream.
   Every `*.test.ts`, `*.test.tsx`, `*.test-d.ts`, `*.test-d.tsx`, `*.spec.ts`,
   and `*.spec.tsx` case must have an equivalent Rust test in the matching
   `chat-sdk-*` crate, including table-driven cases, fixture/snapshot-
   equivalent cases, streaming edge cases, error paths, adapter option
   serialization, and type-level assertions where Rust can express them.
5. For adapter-backed behavior, require two layers of proof before marking a
   row `verified`: deterministic fake/mock/transport tests that run in normal
   validation, plus credential-gated live adapter validation when a usable
   credential exists. Live validation must be opt-in (`#[ignore]` tests or
   runnable examples), skip cleanly when credentials are missing, never print
   secrets, and be recorded in the ledger with the test/example name and date.
6. Enforce strict 1:1 crate/package ownership. Every portable upstream
   `vercel/chat` TypeScript package must have exactly one matching Rust
   workspace crate under `crates/chat-sdk-*`, and no Rust crate may own APIs
   from more than one upstream package.
7. Never stage chat-sdk implementation in the existing `ai-sdk-rust` root
   crate or any `crates/ai-sdk-*` crate. If the matching `chat-sdk-*` crate
   does not exist yet, create it first.
8. Build and verify high-level APIs against deterministic fake/test
   transports before adding real adapter networking.
9. Add deterministic end-to-end tests for every public surface (incoming
   message routing, command parsing, outgoing send, state read/write, error
   propagation, retries, rate-limit handling, streaming/event sequences,
   middleware ordering, ...).
10. Ban vague generic bucket naming in source paths, module names, crate names,
    public APIs, and docs. Prefer precise responsibility names and treat the
    naming-conventions checker as the source of truth for the banned-token list.
    The shared `scripts/check-naming-conventions.sh` enforces this; add
    explicit exceptions only when mirroring upstream package names, and
    document each new exception in the script comments.
11. Do not churn dependencies, CI, or unrelated modules unless the next slice
    genuinely requires it.
12. Port examples and docs once the corresponding API works. Rust examples
    should be runnable and map clearly to upstream examples.
13. When enough works end to end, add a kitchen-sink example app that
    demonstrates the unified surface working across two or more adapters
    (e.g. Slack + Discord) using deterministic fake transports for CI and
    optional live transports for manual validation.
14. Keep expanding until the parity ledger is complete. A single slice is
    never enough unless the ledger already proves full upstream parity.

## Self-Refining Loop

The brief is allowed to be wrong about upstream details. You are required to
correct it.

After every 5 successful merge-back cycles (i.e. every 5 times you complete
the Work Loop and push `main`), do a brief-refinement pass:

1. Read `docs/chat/goal-refinements.md`. Append a new dated entry capturing:
   - What you learned in the last 5 slices that the current brief does not
     reflect (upstream package boundaries that don't match the assumed
     adapter set, test conventions that differ from `vercel/ai`, transport
     contracts, etc.).
   - Any guidance in the brief that is now stale, contradictory, or
     misleading.
   - Concrete brief edits proposed.
2. Apply the proposed edits to `scripts/codex-goal-chat/port-chat-sdk.md`
   and, if the compact `/goal` text needs updating, to
   `scripts/codex-goal-chat/goal-condition.md`.
3. Commit the refinement as its own slice, with a message like
   `chat-sdk: refine goal brief from slices N..N+5`.
4. Merge that refinement slice back to `main` via the standard protocol
   before starting the next implementation slice.

The refinement pass is mandatory, not optional. Skipping it after 5 slices
is a process failure. The first refinement pass is also when you re-confirm
the upstream inventory: if `vercel/chat` has gained or lost packages, update
the ledger and the brief.

## Validation

Run the strongest relevant validation you can before each commit:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
scripts/check-naming-conventions.sh
scripts/package-progress-table.sh \
  --ledger docs/chat/upstream-parity.md \
  --estimates docs/chat/package-progress-estimates.tsv \
  --output docs/chat/package-progress.md \
  --title "Chat SDK Rust Package Progress"
cargo test --workspace --all-features
```

If an optional integration test is added, make it opt-in and documented. It
may load `.env.local`, but it must skip cleanly when adapter credentials are
absent.

## Work Loop

Repeat this loop until the goal is complete or you hit a real blocker:

1. Pull the latest `main` into your worktree branch (rebase).
2. Re-scan or consult `docs/chat/upstream-parity.md`.
3. Pick the highest-value unported or unverified upstream package/API/adapter
   from the first-phase queue (core/shared) until that queue is closed. Then
   move to standalone adapters.
4. Implement it under `crates/chat-sdk-<name>` with tests and docs/examples
   where useful. Never edit `crates/ai-sdk-*`.
5. Update `docs/chat/upstream-parity.md` with status, evidence, and next
   queue. Update `docs/chat/package-progress-estimates.tsv` for any touched
   `in-progress` package, then run the chat progress-table generator above.
6. Run validation.
7. Commit the slice.
8. Merge the slice back to `main` using the protocol below.
9. If this was the 5th, 10th, 15th, ... merge-back since session start,
   run the Self-Refining Loop pass.
10. Continue with the next unported item, building on the updated `main`.

Use commit messages like:

```sh
git commit -m "chat-sdk: port <upstream package or API> parity"
```

## Serialized Merge-Back Protocol

Use this after each validated commit. The lock is shared with the ai-sdk
session, so expect to wait sometimes.

```sh
main_repo="/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust"
lock="/tmp/ai-sdk-rust-main-merge.lock"

while ! mkdir "$lock" 2>/dev/null; do
  echo "Waiting for another goal session to finish merging to main..."
  sleep 20
done

cleanup_lock() {
  rmdir "$lock" 2>/dev/null || true
}
trap cleanup_lock EXIT
```

**Stale-lock recovery.** If `mkdir` fails repeatedly and the lock path
exists as a *regular file* rather than a directory
(`ls -lad /tmp/ai-sdk-rust-main-merge.lock` showing `-rw-…` instead of
`drwx…`), the lock was leaked by a previous crashed process. Verify the
lock's mtime is older than the most recent `origin/main` commit, then
`rm /tmp/ai-sdk-rust-main-merge.lock` and retry `mkdir`. Observed once
during slice 3 of this port — the ai-sdk session had been merging right
through the leaked lock because `mkdir` was failing for it too and it
had its own recovery path, leaving the chat session stuck.

While holding the lock:

```sh
cd "$worktree"
git fetch origin main
git rebase origin/main
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
scripts/check-naming-conventions.sh
scripts/package-progress-table.sh \
  --ledger docs/chat/upstream-parity.md \
  --estimates docs/chat/package-progress-estimates.tsv \
  --output docs/chat/package-progress.md \
  --title "Chat SDK Rust Package Progress"
cargo test --workspace --all-features

git -C "$main_repo" checkout main
git -C "$main_repo" pull --ff-only origin main
git -C "$main_repo" status --short
```

If the main checkout is dirty, stop and report. Do not stash, reset, or
overwrite it.

Merge and push (atomic — push only runs if every check passes):

```sh
if ! (
  set -e
  git -C "$main_repo" merge --no-ff "$branch" -m "Merge chat-sdk parity slice"
  cd "$main_repo"
  cargo fmt --all --check
  cargo clippy --all-targets --all-features -- -D warnings
  scripts/check-naming-conventions.sh
  scripts/package-progress-table.sh \
    --ledger docs/chat/upstream-parity.md \
    --estimates docs/chat/package-progress-estimates.tsv \
    --output docs/chat/package-progress.md \
    --title "Chat SDK Rust Package Progress"
  cargo test --workspace --all-features
); then
  echo "VALIDATION FAILED on main, refusing to push"
  exit 1
fi
git -C "$main_repo" push origin main
```

The `if ! ( set -e; … )` wrapper is non-negotiable: an earlier iteration of
the protocol had a plain `&&` chain followed by a separate `git push`, which
let a naming-check failure ship to `origin/main` (slice 10 of this port).
Always guard the push with this block.

**Ledger row conflicts on rebase.** Each slice that touches a chat-sdk
package overwrites that package's whole row in `docs/chat/upstream-parity.md`
(test counts, evidence, basis, estimate). If a rebase hits a conflict on
that row, prefer your branch's version — subsequent slices include the
prior content. Conflicts on rows owned by the *other* session (any
non-`chat-sdk-*` row in `docs/upstream-parity.md`, or any
`scripts/codex-goal/*` file) mean you violated the ownership table — back
out the edit and find a non-conflicting way to land the same chat-sdk
behavior.

**Cross-boundary fix-up rule.** If `main` is broken in a way that blocks
the *other* session's validation (e.g. chat-sdk pushed a naming-check
failure that the ai-sdk session now hits during its own merge-back), the
unblocking session may make the minimum cross-boundary edit needed to
restore green `main`. This is a recovery event, not the default mode — it
MUST be recorded in the next entry of `docs/chat/goal-refinements.md` (or
`docs/goal-refinements.md`, when the ai-sdk session writes one) so the
boundary crossing is visible and the underlying foot-gun is fixed by the
owning session within one slice.

After a successful push, release the lock and continue from your worktree:

```sh
trap - EXIT
rmdir "$lock"
cd "$worktree"
git fetch origin main
git rebase origin/main
```

## Definition Of Done

You are done only when:

1. `docs/chat/upstream-parity.md` lists every upstream `vercel/chat` package,
   adapter, public API, example, testable behavior, and feature.
2. Every ledger item is `verified` or `js-only-documented`.
3. The chat-sdk progress-table command reports 100% completion for every
   package row, with no remaining `in-progress` or `not-started` packages.
4. The Rust crate/workspace has validated `chat-sdk-*` equivalents for all
   portable upstream surfaces.
5. Every portable test from the original upstream TypeScript packages exists
   as an equivalent Rust test in the matching `chat-sdk-*` crate. Rust may
   have more tests, but it must not have fewer portable tests than upstream.
6. The Rust workspace has a strict 1:1 crate mapping for every portable
   upstream TypeScript package: one matching `chat-sdk-*` crate per package,
   no Rust crate owning APIs from multiple upstream packages.
7. The full validation suite passes on `main` after the final slice.
8. The final complete slice is merged to `main` and pushed.

If any ledger item remains `not-started` or `in-progress`, keep working.

## Multi-session reality (added slice 65)

This port is a multi-session effort. The slice 64 reality check
(documented in `docs/chat/goal-refinements.md` 2026-05-23 entries
and `docs/chat/upstream-parity.md`'s "Next Unported Work Queue"):

- `packages/chat` alone needs ~200-300 more slices to finish the
  remaining heavy modules: channel ~600 LOC, thread ~1100 LOC,
  chat.ts ~2700 LOC, plus markdown stringifier, serialization,
  transcripts-wiring, streaming-markdown.
- Each of the 9 Phase-2 adapter packages is a multi-day port
  effort with platform-specific HTTP/SDK code (Slack RTM + Web
  API, Teams Bot Framework, Google Chat REST, Discord gateway,
  Linear GraphQL, GitHub REST/GraphQL, Messenger webhook,
  Telegram bot API, WhatsApp Cloud API). Roughly 150-300 slices
  per adapter.
- 3 Phase-3 state backends (Redis, ioredis, Postgres) each need
  ~30 slices behind a `StateAdapter` trait extension.

Total realistic slice budget: low thousands of slices spanning
many sessions. Each session inherits the ledger + per-file triage
in `docs/chat/upstream-parity.md` as its pick-up point. The Stop
hook will not be satisfiable inside a single conversation window
until all of that lands.
