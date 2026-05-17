# Codex `/goal` Brief: Full Vercel AI SDK Parity In Rust

You are Codex CLI running a long-lived `/goal` session for `ai-sdk-rust`.

You are allowed to work for a long time. This is not a one-slice task. Take
bigger, coherent slices than a normal short coding session. After every
coherent validated slice, commit it on your worktree branch and merge it back
to `main` using the merge protocol below before continuing.

Do not use GNHF for this run. Do not write new `.gnhf` run state.

## Repository

The main checkout is:

```sh
/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust
```

The launcher creates an explicit git worktree under:

```sh
/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust-goal-worktrees
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
checkout's secret file. It contains Vercel AI Gateway credentials under:

- `AI_GATEWAY_API_KEY`
- `AI_SDK_RUST_AI_GATEWAY_API_KEY`

Use those only when integration validation is useful. Never print the values,
copy them into tracked files, or include them in logs/commits.

## Objective

Replicate the full Vercel AI SDK repository in idiomatic Rust.

The goal is not "make progress". The goal is full parity with upstream
`vercel/ai`: every package, provider, library, public API, example, testable
behavior, and feature should have a Rust equivalent, except surfaces that are
truly JavaScript-only and are explicitly documented as intentionally
non-portable.

Use upstream Vercel AI SDK as the source of truth for shapes and behavior:

```sh
npx opensrc@latest path github:vercel/ai
```

Do not decide the goal is complete until an upstream parity ledger proves there
are no unchecked upstream packages, providers, public APIs, examples, tests, or
features left.

## Required Parity Ledger

First action: create or update `docs/upstream-parity.md`.

The ledger must include:

1. The upstream `vercel/ai` commit SHA/date used for inventory.
2. A package inventory from the upstream repo, including all packages,
   provider packages, utility libraries, framework adapters, examples,
   provider registry/gateway pieces, model spec/provider-v4 surfaces, tests,
   docs, and tooling that affects public behavior.
3. For each upstream package/feature: status (`not-started`, `in-progress`,
   `ported`, `verified`, or `js-only-documented`), Rust crate/module path,
   tests/examples covering it, and notes about intentional Rust differences.
4. A providers section. Every upstream provider package must be listed, even if
   the first implementation is a typed contract and fake/test model before
   real HTTP wiring.
5. A high-level APIs section covering generate text/object/image/speech/video,
   stream APIs, tool calling, structured output, embeddings, transcription,
   reranking, files, registry/gateway support, middleware, provider utilities,
   warnings/errors, prompt/message parts, and any other upstream public API.
6. A "next unported work" queue. At the end of every slice, update this queue
   before committing.

Re-scan upstream often with `npx opensrc@latest path github:vercel/ai`. If the
upstream inventory changes, update the ledger and continue. Do not stop while
the ledger contains `not-started` or `in-progress` items unless you hit a real
blocker that needs human input.

## Priorities

1. Preserve the existing Rust 2024 crate style, serde shapes, builder helpers,
   error/result style, and public exports.
2. Align JSON boundaries with upstream provider-v4 contracts while omitting
   JavaScript-only concepts such as `AbortSignal`.
3. Add focused serialization/deserialization and behavior tests for every new
   public contract.
4. Plan the Rust workspace around upstream Vercel AI SDK package boundaries:
   core AI APIs, provider contracts, provider utilities, and provider
   implementations. Introduce crates when there is enough real API surface to
   justify the boundary; avoid empty placeholder crates.
5. When adding a new surface, decide whether it belongs in the current crate or
   should start/move into a workspace crate that matches the upstream package
   it came from.
6. Build and verify high-level APIs against deterministic fake/test models
   before adding real provider networking.
7. `generate_text(...)` and tool loops remain an early vertical-slice priority:
   prove prompts/settings, model calls, tool calls, typed Rust tools, tool
   results, continuation until final text/max steps, and `GenerateTextResult`.
8. Add deterministic end-to-end tests for plain text generation, single tool
   call, multi-step tool call, tool error, unknown tool, invalid tool args, max
   step exhaustion, streaming/event sequences, structured output, provider
   metadata, and every additional high-level API as it lands.
9. Ban vague generic naming such as `helpers`, `utils`, `common`, `misc`,
   `stuff`, `shared`, and similar buckets in source paths, module names, crate
   names, public APIs, and docs. Prefer precise responsibility names. Add or
   improve a custom check to enforce this convention. Document explicit
   exceptions only when mirroring upstream package names.
10. Do not churn dependencies, CI, or unrelated modules unless the next SDK
    slice genuinely requires it.
11. Port every upstream provider package. Prefer contract-first typed provider
    crates/modules with fake/deterministic tests, then add HTTP/gateway-backed
    integration tests where credentials are available.
12. Port examples and docs once the corresponding API works. Rust examples
    should be runnable and should map clearly to upstream examples.
13. When enough works end to end, add a kitchen sink example app that
    demonstrates working generate text, tool execution, provider contracts, and
    any available gateway-backed validation.
14. Keep expanding until the parity ledger is complete. A single slice is never
    enough unless the ledger already proves full upstream parity.

## Parallel Work

Use Codex agent/team/background-worker features if available inside this goal.
Parallelize independent work so the goal moves fast:

- one worker can inspect upstream `vercel/ai` package/provider shapes,
- one worker can update the parity ledger and identify the next unported item,
- one worker can implement a focused Rust slice,
- one worker can verify tests/docs/examples and edge cases.

Keep ownership clear and integrate all worker output in this worktree before
committing. Do not let workers edit the main checkout directly.

## Validation

Run the strongest relevant validation you can before each commit:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
scripts/check-naming-conventions.sh
cargo test --all-features
```

If an optional integration test is added, make it opt-in and documented. It may
load `.env.local`, but it must skip cleanly when the gateway key is absent.

## Work Loop

Repeat this loop until the goal is complete or you hit a real blocker:

1. Pull the latest `main` into your worktree branch.
2. Re-scan or consult `docs/upstream-parity.md`.
3. Pick the highest-value unported or unverified upstream package/API/provider.
4. Implement it with tests and docs/examples where useful.
5. Update `docs/upstream-parity.md` with status, evidence, and next queue.
6. Run validation.
7. Commit the slice.
8. Merge the slice back to `main` using the protocol below.
9. Continue with the next unported item, building on the updated `main`.

Use commit messages like:

```sh
git commit -m "Port <upstream package or API> parity"
```

## Serialized Merge-Back Protocol

Use this after each validated commit:

```sh
main_repo="/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust"
lock="/tmp/ai-sdk-rust-main-merge.lock"

while ! mkdir "$lock" 2>/dev/null; do
  echo "Waiting for another ai-sdk-rust goal session to finish merging to main..."
  sleep 20
done

cleanup_lock() {
  rmdir "$lock" 2>/dev/null || true
}
trap cleanup_lock EXIT
```

While holding the lock:

```sh
cd "$worktree"
git fetch origin main
git rebase origin/main
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
scripts/check-naming-conventions.sh
cargo test --all-features

git -C "$main_repo" checkout main
git -C "$main_repo" pull --ff-only origin main
git -C "$main_repo" status --short
```

If the main checkout is dirty, stop and report. Do not stash, reset, or
overwrite it.

Merge and push:

```sh
git -C "$main_repo" merge --no-ff "$branch" -m "Merge ai-sdk-rust parity slice"
(
  cd "$main_repo"
  cargo fmt --all --check
  cargo clippy --all-targets --all-features -- -D warnings
  scripts/check-naming-conventions.sh
  cargo test --all-features
)
git -C "$main_repo" push origin main
```

If merge conflicts occur, abort the merge in the main checkout, release the
lock, resolve the conflict in your worktree by rebasing on latest
`origin/main`, rerun validation, recommit if needed, and then retry the
merge-back protocol. Do not push a broken `main`.

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

1. `docs/upstream-parity.md` lists every upstream `vercel/ai` package,
   provider, public API, example, testable behavior, and feature.
2. Every ledger item is `verified` or `js-only-documented`.
3. The Rust crate/workspace has validated equivalents for all portable
   upstream surfaces.
4. The full validation suite passes.
5. The final complete slice is merged to `main` and pushed.

If any ledger item remains `not-started` or `in-progress`, keep working.
