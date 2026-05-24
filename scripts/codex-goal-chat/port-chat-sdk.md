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

## Test-count hygiene (added slice 108)

Every per-module slice that bumps a test count MUST also:

1. Run `cargo test -p chat-sdk-chat --lib <module>::` and capture
   the actual `test result: ok. N passed; ...` total.
2. Update the matching `<module> N` token in
   `docs/chat/package-progress-estimates.tsv`'s `chat` row basis
   text so the basis stays truthful.
3. Re-run `scripts/package-progress-table.sh --ledger
   docs/chat/upstream-parity.md --estimates
   docs/chat/package-progress-estimates.tsv --output
   docs/chat/package-progress.md --title "Chat SDK Rust Package
   Progress"` and commit the regenerated `package-progress.md`
   alongside the tsv.
4. Verify the diff: `git diff --stat docs/chat/` should always
   include both the tsv AND the regenerated `package-progress.md`.

Slices that touch only doc text (no test additions) can skip
step 1 but MUST still complete steps 2-4 so the generated table
stays in sync.

## Merge-back execution (added slice 108)

The atomic merge-back command chain is long (~8 piped commands).
Two recurring failure modes:

- **Background hangs.** If the harness backgrounds the chain
  (and you don't pass `run_in_background: false`), the lock dir
  can stay held while the parent shell idles. Always run the
  merge-back synchronously (`run_in_background: false`, with a
  120000ms `timeout`).
- **Trailing pipes mask exit codes.** Never put `| tail`,
  `| head`, or `| grep` inside the `&&` chain - those return 0
  even on failure. Run the gate as a separate `cargo test ...`
  command BEFORE the merge, then chain
  `... && git push origin main && rmdir lock`.

Both of these have caused validation-bypass incidents (see
`docs/chat/goal-refinements.md` slices 80, 91, 108).

## Additive-helper ceiling (added slice 114)

Slices 99-113 followed an "additive pure helper" pattern that
bumps `chat`'s percentage 1% per slice. This is real progress —
the helpers shrink the future trait-extension slice's surface —
but the chat percentage in `package-progress.md` is now an
unreliable proxy for "how complete is the chat class itself."

Hard cap: do not bump the chat row above 92% on additive
helpers alone. Anything above 92% requires a real upstream-class
port (TranscriptsApiImpl, ThreadHistoryCache, CallbackUrlImpl,
or the bigger Channel/Thread/Chat classes) and that work needs
the Adapter / StateAdapter trait extension.

When the additive-helper surface is exhausted, the next move is
the trait extension. See "Phase 1.5 trait-extension session"
below.

## Phase 1.5 trait-extension session (added slice 114)

The single highest-leverage remaining work is extending the
placeholder `chat::types::Adapter` and `chat::types::StateAdapter`
traits in `crates/chat-sdk-chat/src/types.rs` (currently both
empty). This unblocks 5 consumer modules at once:

- `callback_url`: stateful `processCardCallbackUrls`,
  `resolveCallbackUrl`, `postToCallbackUrl` paths (12 deferred
  upstream test cases).
- `transcripts`: `TranscriptsApiImpl` class (append / list /
  delete / count).
- `thread_history`: `ThreadHistoryCache` class (append /
  get_messages).
- `postable_object`: `post_postable_object` dispatch helper.
- `message`: the `subject` async getter (5 deferred upstream
  test cases).

Plan:

1. Add `async-trait` to `chat-sdk-chat`'s `Cargo.toml`.
2. Extend `StateAdapter` with the 5-method subset the consumer
   modules need: `get`, `set`, `delete`, `append_to_list`,
   `get_list`. Skip locks/queues/subscriptions — those go in a
   later slice once the production state backends are written.
3. Extend `Adapter` with the 4-method subset PostableObject
   dispatch needs: `post_message`, `post_object`,
   `fetch_subject`, `parse_message`.
4. Implement the new trait methods on `MemoryStateAdapter` via
   `async-trait`-wrapped sync delegation (the in-memory backend
   has no real I/O so its trait impl is `async fn x() { sync_x() }`).
5. Land each consumer-module slice in turn, mapping its
   previously-deferred upstream test cases.

This is a multi-slice session (5-10 slices) and should not be
attempted mid-additive-helper-streak. Start a fresh dedicated
session.

## Consumer-class port pattern (added slice 121)

After slices 117-120 landed StateAdapter + three consumer
classes (TranscriptsApiImpl, ThreadHistoryCache,
CallbackUrlStore), the pattern crystallized:

1. **Pre-pull helpers first.** Before writing the class, pull
   the upstream class's inline expressions into module-level
   pure helpers — predicates, key formatters, inverse helpers,
   default-applied getters on the config struct. See slices
   106 / 110 / 111 for the prior-art. This keeps the class
   itself thin.
2. **Write the class as a struct holding `Arc<dyn StateAdapter>`
   + the config struct.** Use `#[derive(Clone)]` so callers
   can hand it out cheaply. Implement `Debug` manually so the
   dyn trait object doesn't break the derive.
3. **Each public method is one or two `await`s on the trait.**
   Don't put business logic in the class; delegate to the
   already-shipped pure helpers (e.g. `user_transcript_key`,
   `is_tombstone`, `tombstone`). The class's job is wiring,
   not logic.
4. **Tests use an inline `MockState` + `futures_executor::block_on`.**
   Define a small `MockState` struct in the `#[cfg(test)] mod
   tests` block with `HashMap<String, Value>` (+ optional
   `HashMap<String, Vec<Value>>` for list ops). Impl the
   StateAdapter trait with `#[async_trait::async_trait]`. Each
   test calls `block_on(api.method(...))`.
5. **`futures-executor` ships as a dev-dependency.** Already
   transitively in `Cargo.lock`. Never add `tokio` as a
   direct dep just for tests — `futures-executor` is enough.
6. **Mapped-case count + percentage bump.** Each consumer-class
   slice typically bumps the module's test count by 6-8 (the
   mapped upstream cases), and bumps chat's percentage by 1%.

If a slice doesn't fit this template (e.g. needs HTTP or
non-trivial business logic), split it into multiple slices
following the model/adapter split rule.

## Phase 1.5 closed (added slice 128)

As of slice 127 the Phase 1.5 trait-extension work is closed.
Trait surface:

- `chat::types::StateAdapter`: 5 key/value+list methods (get,
  set, delete, append_to_list, get_list) + set_if_not_exists +
  4 lock methods (acquire / release / force_release / extend).
- `chat::types::Adapter`: name + fetch_subject + post_message +
  post_object + parse_message.

Consumer modules ported on top of those traits:

- `chat::transcripts::TranscriptsApiImpl` (append/list/delete/count).
- `chat::thread_history::ThreadHistoryCache` (append/get_messages/count).
- `chat::callback_url::CallbackUrlStore` (issue/resolve).
- `chat::message::MessageSubjectResolver` (resolve with cache +
  invalidate).
- `chat::postable_object::post_postable_object` (dispatch with
  fallback-to-post_message).
- `chat::channel::Channel` (post/post_object/clone).
- `chat::thread::Thread` (post/post_object/subject/clone).

Future Adapter trait methods (open_dm, open_modal,
fetch_messages, edit_message, delete_message, add_reaction,
remove_reaction, start_typing, get_channel_info, list_threads,
get_channel_visibility, parse_message, post_channel_message,
encode_thread_id, decode_thread_id) will be added per-adapter
as the Phase-2 adapter crates need them. Each new method:

1. Add the method to the `Adapter` trait with a sensible
   default (typically `Err(AdapterError::Unsupported(...))`).
2. Add a corresponding wrapper to `Channel` / `Thread` if the
   method is channel- or thread-shaped.
3. Add a default-impl test on a `MinimalAdapter`-like fixture.

## Phase 2 / Phase 3 prep (added slice 128)

The next major work is Phase 2 (adapter packages) and Phase 3
(state backends). Both need a workspace-level async runtime
decision. Recommended:

- **Runtime**: `tokio` (most ecosystem, most upstream-equivalent
  for HTTP servers/clients). Adopt it as a workspace dependency
  only when the first real adapter or state backend needs it —
  don't commit pre-emptively.
- **HTTP client**: `reqwest` (most natural for Slack/Teams/etc
  HTTPS APIs).
- **Redis client**: `redis` crate + `bb8-redis` for pooling.
- **Postgres client**: `tokio-postgres` or `sqlx`.

The first not-started adapter port should be the smallest one
(Telegram: 7 src files, 3 test files) so the adapter scaffolding
is small enough to review per-slice.

## Adapter-scaffold template (added slice 135)

Slices 130-134 ported 5 adapters using a near-identical
template. Each adapter scaffold ships:

1. **`crates/chat-sdk-adapter-<name>/Cargo.toml`** with deps:
   `async-trait`, `chat-sdk-chat = { path = "../chat-sdk-chat" }`,
   `serde`, `serde_json`. Dev-dep: `futures-executor` (for
   block_on in tests).
2. **`crates/chat-sdk-adapter-<name>/src/lib.rs`** with:
   - `ADAPTER_NAME` / `THREAD_ID_PREFIX` / `DEFAULT_API_BASE`
     (or `DEFAULT_GRAPH_BASE`) constants.
   - `<Name>AdapterOptions` struct: required credentials +
     optional API base. `.new(...)` constructor + `.with_*`
     builders + `.effective_*` getters with default applied.
   - `<Name>Adapter` struct holding the options, impl-ing
     `chat_sdk_chat::types::Adapter` with `name()` overridden.
     All other Adapter methods take the trait defaults
     (return `AdapterError::Unsupported`).
   - `encode_thread_id(...)` / `decode_thread_id(...)` /
     `is_<name>_thread_id(...)` for the upstream thread-id
     wire format (per-adapter shape: numeric pair, owner/repo,
     opaque IDs, etc).
3. **Workspace `Cargo.toml`** members entry (alphabetized
   between `chat-sdk-adapter-*` siblings).
4. **`docs/chat/upstream-parity.md`** row flipped from
   `not-started` to `in-progress` with the crate path and a
   one-line basis.
5. **`docs/chat/package-progress-estimates.tsv`** row added at
   `10%` with the one-line basis.
6. **`docs/chat/package-progress.md`** regenerated via the
   table script.
7. **11-13 colocated tests** covering: adapter name, options
   construction + defaults + overrides, encode/decode happy +
   miss paths, encode/decode round-trip, inherited
   `post_message` returning `Unsupported`, credential
   accessor sanity.

Variance is exclusively in the thread-id wire format and
required credentials. Total per scaffold: ~250 LOC of source
+ tests, ~5-15 min to draft.

When the workspace commits to an async HTTP client (per "Phase
2 / Phase 3 prep" above), the adapter scaffolds become the
landing points for the real I/O methods — each adapter then
needs ~30-50 additional slices for full upstream parity.

## State-backend scaffold variant (added slice 143)

State backends follow the same template as adapters with three
swaps:

1. Impl `chat_sdk_chat::types::StateAdapter` (not `Adapter`).
   The 5 required methods (get, set, delete, append_to_list,
   get_list) have NO defaults, so each impl MUST provide a
   body. For scaffolds, return
   `Err(StateAdapterError::NotConnected)` from every method —
   that's the minimal valid impl, matches upstream's
   "not connected" throw, and exercises the trait shape in
   tests. The 5 default-impl methods (set_if_not_exists +
   4 lock methods from slice 125) inherit the trait defaults
   automatically.
2. No thread-id codec. State backends are agnostic to the
   chat-sdk thread-id format — adapters build the keys.
3. Per-backend config struct shape varies: single-node URL
   (`state-redis`), cluster + Sentinel (`state-ioredis`),
   database URL + table prefix (`state-pg`). Match the
   upstream package's options interface.

Tests: 10-11 colocated cases covering options construction +
overrides + accessors + 5 `NotConnected` returns. The full
recipe lives in `crates/chat-sdk-state-redis/src/lib.rs` and
`crates/chat-sdk-state-pg/src/lib.rs` as live examples.

## Session 2 kickoff checklist (added slice 143)

All 18 upstream packages now have either a verified mark, a
js-only-documented mark, or an in-progress scaffold. The next
session's first slice should ship the workspace runtime
commitment that unblocks every in-progress package's HTTP /
DB layer:

1. **DONE in slice 144.** Added `tokio = { version = "1",
   features = ["rt-multi-thread", "macros"] }` and
   `reqwest = { version = "0.13", features = ["json", "rustls"],
   default-features = false }` to
   `crates/chat-sdk-adapter-shared/Cargo.toml`.
   `chat_sdk_adapter_shared::runtime` re-exports both crates +
   provides `default_http_client()` with the chat-sdk defaults
   (30s timeout, `chat-sdk-rust/<version>` User-Agent).
2. Pick the Postgres client (recommend `sqlx` for compile-time
   query checking; `tokio-postgres` for lower dep footprint).
3. Add `redis = { version = "0.27", features = ["tokio-comp"] }`
   and `bb8-redis = "0.18"` to
   `crates/chat-sdk-state-redis/Cargo.toml`. The cluster
   variant goes into `chat-sdk-state-ioredis` with
   `redis = { features = ["cluster-async", "tokio-comp"] }`.
4. Pick the smallest adapter (Telegram) and ship the
   `post_message` HTTP slice end-to-end as the reference
   implementation. The other 8 adapters' `post_message`
   methods follow the same recipe.
5. After 1 adapter + 1 state backend have real HTTP/DB layer,
   re-baseline the percent scale in
   `docs/chat/package-progress-estimates.tsv` so the 10%
   scaffold mark and the 100% verified target have empirical
   anchor points.

Estimated session 2 budget: ~50-100 slices for HTTP wire-up
across 9 adapters + 3 state backends. Subsequent sessions
ship verified marks per package as full upstream test parity
lands.

## Adapter-HTTP-method port pattern (added slice 150)

Slices 145-149 ported `post_message` on 5 adapters using a
near-identical template. Each adapter port ships:

1. Add `chat-sdk-adapter-shared` as a direct `[dependencies]`
   entry in the adapter's `Cargo.toml`.
2. Add an `http: chat_sdk_adapter_shared::runtime::reqwest::Client`
   field to the adapter struct. Default to
   `chat_sdk_adapter_shared::runtime::default_http_client()` in
   `::new(...)`. Add `.with_http_client(client)` builder for
   tests pointing at a wiremock server.
3. Add a private URL-template helper method on the adapter
   struct (`channel_messages_url`, `comments_url`,
   `send_url`, etc.) matching upstream's inline endpoint
   construction.
4. Override `async fn post_message(&self, thread_id, text)` on
   the `Adapter` trait impl:
   - **Pre-HTTP validation**: call `decode_thread_id` and
     return `AdapterError::InvalidPayload` for ids that don't
     belong to this adapter. Cheap and tested without a
     tokio runtime.
   - **Build the request**: `self.http.post(&url)` + per-platform
     auth (`bearer_auth`, manual `Authorization` header, URL
     query param, etc.) + `.json(&body)` for the per-platform
     request envelope.
   - **Send + parse**: `.send().await.map_err(|e|
     AdapterError::Io(Box::new(e)))?`, then parse the
     response JSON. Surface non-200 / `ok:false` responses as
     `AdapterError::InvalidPayload` with the platform's error
     message field.
   - **Extract the id**: per-platform location (`result.message_id`,
     top-level `id`, `messages[0].id`, etc.). Return as a
     `String`.
5. Drop the slice 130's `adapter_default_methods_return_unsupported`
   test (no longer applicable — post_message is overridden).
   Add two new tests:
   - `adapter_post_message_rejects_non_<platform>_thread_ids`:
     hits the pre-HTTP validation path.
   - `adapter_<url-template>_builds_the_upstream_endpoint`:
     covers the URL template helper.
6. Update `docs/chat/upstream-parity.md` row text to reflect
   the post_message landing.
7. Bump the per-adapter row in
   `docs/chat/package-progress-estimates.tsv` from 10% to 15%
   (post_message is roughly 1 of 8 Adapter trait methods).

**Auth-scheme variants observed so far:**

- Telegram: path-token (`/bot<token>/<method>`).
- GitHub: `Authorization: Bearer <token>` + per-API headers
  (`Accept: application/vnd.github+json`,
  `X-GitHub-Api-Version: 2022-11-28`).
- Messenger: URL query param `access_token=<page_token>`.
- WhatsApp: `Authorization: Bearer <access_token>`.
- Discord: non-standard `Authorization: Bot <bot_token>` —
  set manually since `reqwest::RequestBuilder::bearer_auth`
  hardcodes `Bearer `.

The remaining 4 adapters (Linear/GChat/Teams/Slack) each
follow the same recipe with their own auth + endpoint +
response-shape variances.

## Adapter method matrix (added slice 157; revised slice 168)

Per-adapter progress across the 8 Adapter trait methods. Each
cell tracks the slice number that landed the method on that
platform. `—` = not yet shipped. `n/a` = upstream does not
implement this method on this adapter (1:1 with upstream means
we return `AdapterError::InvalidPayload` with the upstream-style
message, or `Ok(())` for the documented no-op cases).

Slice-159 extended the Adapter trait with the 4 universal
upstream methods (edit_message, delete_message, add_reaction,
start_typing). The slice-158/159 refinement entry documents
that `fetchSubject` is **upstream-optional** and only Linear
implements it - the entries below for Telegram/GitHub/Slack
are additive Rust-port HTTP wiring, not 1:1 upstream parity.

| Adapter   | post_msg | fetch_subj | post_obj | edit_msg | delete_msg | add_react | start_typing | parse_msg |
|-----------|----------|------------|----------|----------|------------|-----------|--------------|-----------|
| telegram  | 145      | 155-add    | —        | 161      | 161        | 161       | 161          | —         |
| github    | 146      | 156-add    | —        | 162      | 162        | 162       | 162-noop     | —         |
| messenger | 147      | n/a        | —        | 163-n/a  | 163-n/a    | 163-n/a   | 163          | —         |
| whatsapp  | 148      | n/a        | —        | 164-n/a  | 164-n/a    | 164       | 164-noop     | —         |
| discord   | 149      | n/a        | —        | 165      | 165        | 165       | 165          | —         |
| linear    | 151      | TODO-real  | —        | 166      | 166        | 166       | 166-noop     | —         |
| slack     | 152      | 158-add    | —        | 160      | 160        | 160       | 160          | —         |
| teams     | 153      | n/a        | —        | 167      | 167        | 167-n/a   | 167          | —         |
| gchat     | 154      | n/a        | —        | 168      | 168        | 168       | 168-noop     | —         |

Legend:
- Slice number alone: real HTTP method ported 1:1 from upstream.
- `<slice>-add`: additive Rust-port HTTP wiring; not in upstream.
- `<slice>-n/a`: upstream throws / not-implemented; Rust returns
  InvalidPayload with the upstream-style message.
- `<slice>-noop`: upstream returns void; Rust returns Ok(()).
- `n/a` (no slice): upstream doesn't implement; we use the
  default trait impl returning Unsupported or Ok(None).
- `TODO-real`: Linear's fetchSubject is real upstream — needs
  porting in a future slice with the rich MessageSubject shape.

Status: **49 of 49 universal cells filled** (5 universal upstream
methods × 9 adapters + Linear's `fetchSubject` placeholder + 4
universal trait extensions counting only the upstream-supported
methods, after slice 159 added the trait surface and slices
160-168 rolled out 4 methods per adapter).

Remaining work to verify any adapter:
1. **post_object** (9 adapters): Block Kit / Adaptive Cards /
   cards v2 / Discord embeds / Linear GraphQL / Telegram inline
   keyboards / WhatsApp interactive messages. ~3-5 slices each.
2. **parse_message** (9 adapters): inverse of post_message —
   parse webhook payloads into the cross-platform Message
   shape.
3. **Real Linear fetchSubject** (1 adapter).
4. **Token-mint helpers** for Teams / GChat in
   chat-sdk-adapter-shared.
5. **Slack Socket Mode + signature verification**.
6. **State-backend client wire-up** (state-redis, state-ioredis,
   state-pg currently at 10% NotConnected placeholder).

## Env-var-resolution port pattern (added slice 305)

Upstream adapter constructors fall through to `process.env.<PREFIX>_*`
when config fields are omitted (Discord, Telegram, WhatsApp,
Messenger, Linear, GitHub, GChat all share this shape). Do **not**
port this by calling `std::env::var` inside the constructor:

- `std::env::set_var` is `unsafe` in Rust 2024 edition.
- Cargo's test runner shares one process; parallel tests racing
  on `process.env` are unreliable.
- A constructor that reads global state can't be exercised
  deterministically.

Instead, port the env-var-resolution path as a factory function
that takes an explicit env-reader closure:

```rust
pub struct XxxCreateOptions {
    pub field_a: Option<String>,
    pub field_b: Option<String>,
    // ...
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XxxCreateError {
    FieldARequired,
    FieldBRequired,
}

pub fn try_create_xxx_adapter(
    opts: XxxCreateOptions,
    env: impl Fn(&str) -> Option<String>,
) -> Result<XxxAdapter, XxxCreateError> {
    let field_a = opts
        .field_a
        .or_else(|| env("XXX_FIELD_A"))
        .ok_or(XxxCreateError::FieldARequired)?;
    // ...
}
```

Tests pass bespoke per-case closures:

```rust
let env = |key: &str| match key {
    "XXX_FIELD_A" => Some("env-value".to_string()),
    _ => None,
};
let adapter = try_create_xxx_adapter(opts, env)?;
```

The prod entry point (if any) can wrap
`|k| std::env::var(k).ok()` at the top of a binary's `main`.
Reference port: Discord slice 304 (`try_create_discord_adapter`
in `crates/chat-sdk-adapter-discord/src/lib.rs`).

This pattern unblocks the env-var-resolution describe blocks for
Telegram, Messenger, WhatsApp, Linear, GitHub, GChat (and any
future adapters with `process.env.PREFIX_*` resolution).

## Optional-adapter-method port pattern (added slice 355)

Upstream's `Adapter` interface uses TypeScript's `?` modifier on
methods that an adapter may or may not implement (e.g. `openDM?`,
`postEphemeral?`, `postChannelMessage?`, `isDM?`,
`channelIdFromThreadId?`, `getUser?`, `fetchChannelInfo?`,
`onThreadSubscribe?`, `parseMessage?`, `postObject?`). Callers
detect the missing-method case via `if (adapter.method)` and
choose a fallback branch.

Rust traits cannot mark methods optional. The canonical port:

1. Add the method to the `Adapter` trait with a default
   implementation that returns
   `Err(AdapterError::Unsupported("<method_name>"))`.
2. In the dispatcher (Chat::*, Thread::*, Channel::*) that
   would call this method, match on the result and treat the
   `Unsupported` variant the same way upstream treats a missing
   method (fallback, return `Ok(None)`, etc.):

```rust
match self.adapter.optional_method(args).await {
    Ok(v) => return Ok(Some(v)),
    Err(AdapterError::Unsupported(_)) => {} // fall through to fallback
    Err(other) => return Err(other),
}
// ... fallback branch
```

3. Adapters that natively support the method override the
   default with their real impl. Adapters that don't simply
   don't override (the default returns Unsupported).
4. Test the dispatcher by injecting a test adapter that toggles
   `Unsupported`-vs-supported via a boolean flag, reproducing
   upstream's `mockAdapter.method = undefined` mutation
   pattern. Example test adapter shape:

```rust
struct TestAdapter {
    supports_x: bool,
    supports_y: bool,
}

#[async_trait::async_trait]
impl Adapter for TestAdapter {
    async fn x(&self, ...) -> AdapterResult<X> {
        if !self.supports_x { return Err(AdapterError::Unsupported("x")); }
        // real impl ...
    }
}
```

For predicate-style optional methods that upstream returns
`boolean` rather than throwing (e.g. `isDM?`,
`channelIdFromThreadId?`), use `Option<T>` return types instead
of `Result<T, AdapterError>` — `None` is the
"not-implemented-by-this-adapter" signal.

Reference ports:
- `Thread::post_ephemeral` (slice 354) — `Adapter::post_ephemeral`
  with `Unsupported` fallback to `open_dm + post_message`.
- `Thread::post_object` — `Adapter::post_object` with
  `Unsupported` fallback to `post_message`.
- `Channel::post` — `Adapter::post_channel_message` with
  `Unsupported` fallback to `post_message`.
- `Adapter::is_dm` returning `Option<bool>` — `None` for
  adapters that don't model channel/DM separation.

**Do not** introduce a separate `supports_method() -> bool` trait
method as a workaround. The `Unsupported`-sentinel pattern
already encodes that question and matches upstream's runtime
detection 1:1.

## Author-overload sibling-method pattern (added slice 348)

Upstream signatures of the form `method(user: string | Author, ...)`
branch on `typeof user === "string" ? user : user.userId` at
runtime. The canonical port adds two Rust methods rather than
a `Union<&str, &Author>` or `Into<UserKey>` trait:

```rust
impl Chat {
    pub async fn open_dm(&self, user_id: &str) -> Result<Thread, OpenDmError> {
        // ... real impl
    }
    pub async fn open_dm_for_author(&self, author: &Author) -> Result<Thread, OpenDmError> {
        self.open_dm(&author.user_id).await
    }
}
```

Rationale: simpler API surface, no trait-bound complexity, no
need for `From<&Author> for UserKey`-style impls, and each
method has an unambiguous signature for IDE assistance. The
upstream-mapped describe blocks port as two test cases
exercising the two sibling methods (the upstream "should accept
Author object" case maps to the `_for_author` variant test).

Reference ports:
- `Chat::open_dm` + `Chat::open_dm_for_author` (slice 348)
- `Chat::get_user` + `Chat::get_user_for_author` (slice 348)
- `Thread::post_ephemeral` + `Thread::post_ephemeral_for_author`
  (slice 354)

## Trait-impl sweep pattern (added slice 366)

Once a new method lands on the `Adapter` (or `StateAdapter`) trait
in `chat-sdk-chat::types`, the per-adapter trait impl bodies should
be added in a **single sweep slice** (one commit) rather than per-
adapter (N commits). This:

1. Reduces merge overhead — the brief mandates a separate
   merge-back per slice, so each per-adapter commit costs a full
   merge cycle.
2. Surfaces compile-time mismatches in one place — when the trait
   default doesn't compile against one adapter's inherent method
   signature (e.g. `String` vs `Option<String>`), the diff shows
   the fix at the wrap site.
3. Documents the wiring once — the commit message lists which
   adapters got which trait-impl pairs.

Template:

```rust
#[async_trait]
impl Adapter for XxxAdapter {
    fn name(&self) -> &str {
        ADAPTER_NAME
    }

    /// 1:1 with upstream `adapter.<methodName>(args)`. Delegates to
    /// the inherent [`XxxAdapter::<method_name>`].
    fn <method_name>(&self, args: ...) -> ReturnType {
        self.<method_name>(args)
    }

    // ... existing trait impl body
}
```

Rust's inherent-method-takes-precedence resolution means `self.method(args)`
inside the trait impl body calls the inherent method (not the trait
method itself), so no recursion. When the inherent method returns
`String` / `bool` directly but the trait wants `Option<String>` /
`Option<bool>`, wrap in `Some(_)`.

No new tests are needed for the wiring itself — the trait impl is
exercised by callers via the trait object, while the inherent
method's tests already cover the implementation. Add tests only
when the wrap (`Some(_)`) changes observable behavior at the trait
boundary.

Reference port: slice 366 sweep added `Adapter::channel_id_from_thread_id`
+ `Adapter::is_dm` impls across 8 adapters (Slack, Discord, GChat,
WhatsApp, Messenger, Telegram, Linear, GitHub) in one commit.

## Type-system-impossible upstream cases (added slice 380)

Some upstream tests assert behavior that's impossible-by-construction
in idiomatic Rust:

- Upstream tests `it("returns null for null input")` when the JS
  signature is `extractCard(message: ChatElement | null | undefined)`.
  In Rust the equivalent is `pub fn extract_card(message:
  &AdapterPostableMessage) -> Option<&CardElement>` — `message`
  cannot be null, so the test case is unreachable.
- Upstream tests `it("does not preserve fetchMessage callback")`
  when the JS `LinkPreview` interface has an optional async
  `fetchMessage` callback. In Rust the equivalent
  `LinkPreview` struct has no callback field at all by
  construction, so the absence is enforced at compile time.
- Upstream tests `it("rejects unsupported type at runtime")` when
  the JS function takes `unknown`. In Rust the equivalent is a
  typed enum that lists exactly the supported variants; the
  unsupported case can't be constructed.

The brief mandates a matching Rust test for every portable upstream
case. **Type-system-impossible cases satisfy the parity contract
via the type system itself** — don't bypass the rule with a fake
Rust test that asserts a tautology. Instead, document the mapping
in the module header or test-section comment so the parity audit
can verify the case is intentionally not portable:

```rust
// ---------- describe("extractCard") (14 upstream cases, 10 portable) ----------
// Upstream's "returns null for null input" / "returns null for
// undefined input" / "returns null for object without card or type"
// cases are 1:1 via the type system: the Rust signature takes
// `&AdapterPostableMessage` (a typed enum, not `unknown | null |
// undefined`), so the cases are unreachable.
```

Reference ports:
- `Message::to_serialized` slice 377 — `fetchMessage` / `data`
  callback fields don't exist on the Rust `LinkPreview` /
  `Attachment` types.
- `chat_sdk_adapter_shared::buffer_utils` slice 379 — the
  TS-runtime-only `Buffer` / `ArrayBuffer` / `Blob` cases
  collapse to a single Rust `FileBytes` (`Vec<u8>`) round-trip
  case.

## js-only-documented enumeration pattern (added slice 396)

For upstream test files that contain a mix of portable cases and
unreachable-by-construction cases (JS module-loader, JS runtime
types like Blob/ArrayBuffer/EventEmitter, JSX runtime, JS-specific
callback fields, JS `process.env`, `describe.skip` integration
tests), document the unreachable cases explicitly in the Rust test
module header so the parity audit can match them 1:1 without a
fake tautological test.

Canonical section header (use this exact shape):

```rust
// ---------- upstream js-only-documented cases (per slice-380 pattern) ----------
//
// The following N upstream `<file>.test.ts` cases are js-only or
// type-system-impossible and have no matching Rust test:
//
// - `<case name>`: <reason for being unreachable>
// - `<case name>`: <reason>
// ...
//
// Remaining upstream cases are mapped (<short summary of where>).
```

Then in `docs/chat/upstream-parity.md`, the per-package row should
state the total accounting:

```
N Rust-mapped + M js-only-documented = (N+M)/(N+M) total upstream cases accounted for
```

Reference ports:
- `crates/chat-sdk-chat/src/modals.rs`: 9 `fromReactModalElement`
  JSX cases (slice 393).
- `crates/chat-sdk-state-redis/src/lib.rs`: 8 cases (slice 394).
- `crates/chat-sdk-state-ioredis/src/lib.rs`: 4 cases (slice 395).
- `crates/chat-sdk-state-pg/src/lib.rs`: 8 cases (slice 395).
- `crates/chat-sdk-adapter-shared/src/buffer_utils.rs`: 5
  Blob/ArrayBuffer cases (slice 391).
- `crates/chat-sdk-adapter-shared/src/adapter_utils.rs`: 4
  extract_card + 4 extract_files + 2 extract_postable_attachments
  null/undefined cases (slices 382, 383).
- `crates/chat-sdk-chat/src/transcripts.rs`: 2 user_key-required
  cases (slice 384).
- `crates/chat-sdk-chat/src/chat.rs`: 1 sync IdentityResolver case
  (slice 387).

A package can flip to **verified** when every upstream case
(in *all* its test files) is either mapped to a Rust test or
documented as js-only in this pattern. The state-backends are
currently in this position pending runtime client wire-up.

## Deferred-adapter-method port pattern (added slice 411)

For optional upstream methods that need a non-trivial Rust port,
the canonical cadence is 3 slices.

**Phase A — trait extension + dispatcher + basic delegation
(1 slice)**
Add the trait method to `Adapter` (or `StateAdapter`) with
default `Err(AdapterError::Unsupported("method_name"))`. The
calling site dispatches through it and maps `Unsupported` back
to the appropriate `ChatError` variant (typically `NotImplemented`).
Port the 3-5 upstream "basic delegation + return shape" cases via
a single-purpose test mock. Existing `Err(Unsupported)` callers
keep passing because the dispatcher preserves the same upstream
error message.

**Phase B — richer test mocks (1 slice)**
Add 1-2 more test mocks (e.g. failing variant for error
propagation; bundled variant for verifying other method calls
aren't accidentally invoked). Port the 5-7 next-tier upstream
cases (propagate-errors, no-side-effects, multiple-invocations,
arg-passthrough, etc.).

**Phase C — wrapper handle for closure-bound methods (1 slice)**
Upstream's return values often carry closures (e.g.
`ScheduledMessage.cancel(): Promise<void>`,
`SentMessage.edit(text): Promise<void>`). Rust closures are not
`Serialize + Eq`, so wrap the upstream data struct in a
thread-bound handle that carries `Arc<dyn Adapter>` alongside.
The handle exposes accessor methods for the data fields + the
closure-style methods that dispatch through a second adapter
trait method (e.g. `cancel_scheduled_message`). Port the 3-5
closure-bound upstream cases via a mock that records the second
trait method's invocations.

Reference port: slices 403/404/405 (Thread::schedule
end-to-end). The 3 phases ported 15 of 24 upstream
`thread.test.ts > describe("schedule()")` cases; the remaining
6 are PostableMessage-input cases that need a 4th phase
(input-enum extension).

## Triage-table refresh reminder (added slice 411)

When closing a slice, audit `docs/chat/upstream-parity.md`
**twice**:

1. **Package description column** (around lines 46-66): append
   the new slice's contribution to the inline audit trail.
2. **Per-test-file table** (around lines 117-137): refresh the
   row(s) for the affected `*.test.ts` file(s) to reflect the
   new portable / js-only-documented count.

The per-test-file table tends to drift because slices update the
description column but not the per-file count column. The Stop
hook reads the per-file table to infer "13 in-progress packages
remain" — keep these synchronized. Slices 408 and 410 are the
canonical examples of the doc-only sync slice that catches up the
table.

## Cross-cutting js-only-documented sweep pattern (added slice 411)

When the same upstream pattern is unrepresentable across N
crates (e.g. `subclass extensibility` across 9 adapters,
`@workflow/serde` symbols across 2 classes), enumerate the case
once per crate in a single sweep slice. Don't make N slices.

Cross-cutting candidates spotted but not yet ported:
- All 9 adapters' `subclass extensibility` cases (slice 409
  closed this).
- The Vitest `mockLogger` / `vi.fn()` test-glue cases that
  appear across every adapter test file — these are not portable
  because Rust uses inline `Mutex<Vec<_>>` recorders. Worth
  enumerating as js-only-documented when porting each adapter's
  test file.

## Update Brief reminder (added slice 411)

The brief (this file, `scripts/codex-goal-chat/port-chat-sdk.md`)
is the source of truth that future Claude sessions read on
startup. After every 5 merge-back cycles:
1. Append a new entry to `docs/chat/goal-refinements.md` covering
   the 5 slices.
2. Append a new section here ("## ... (added slice N)") if a new
   canonical pattern emerged.
3. Edit existing sections in place if their wording / examples
   became stale.

Don't bulk-rewrite the brief — accretive growth keeps the audit
trail intact.
