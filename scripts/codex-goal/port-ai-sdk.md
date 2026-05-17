# Codex `/goal` Brief: Port The Vercel AI SDK To Rust

You are Codex CLI running a long-lived `/goal` session for `ai-sdk-rust`.

You are allowed to work for a long time. Take bigger, coherent slices than a
normal short coding session. After every coherent validated slice, commit it on
your worktree branch and merge it back to `main` using the merge protocol below
before continuing.

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

Port the Vercel AI SDK to idiomatic Rust incrementally.

Continue from the existing code. Prioritize a working `generate_text(...)`
vertical slice with tool-loop execution over adding more horizontal provider
surface area.

Use upstream Vercel AI SDK as the source of truth for shapes and behavior:

```sh
npx opensrc@latest path github:vercel/ai
```

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
6. Build `generate_text(...)` against a deterministic fake/test
   `LanguageModel` before adding real provider networking.
7. Prove the high-level loop can accept prompts/settings, call a model, detect
   tool calls, execute typed Rust tools, append tool results, continue until
   final text or max steps, and return `GenerateTextResult`.
8. Add deterministic end-to-end tests for plain text generation, single tool
   call, multi-step tool call, tool error, unknown tool, invalid tool args, and
   max-step exhaustion.
9. Ban vague generic naming such as `helpers`, `utils`, `common`, `misc`,
   `stuff`, `shared`, and similar buckets in source paths, module names, crate
   names, public APIs, and docs. Prefer precise responsibility names. Add or
   improve a custom check to enforce this convention. Document explicit
   exceptions only when mirroring upstream package names.
10. Do not churn dependencies, CI, or unrelated modules unless the next SDK
    slice genuinely requires it.
11. When enough works end to end, add a kitchen sink example app that
    demonstrates working generate text, tool execution, provider contracts, and
    any available gateway-backed validation. This is not urgent before the
    vertical slice works.

## Parallel Work

Use Codex agent/team/background-worker features if available inside this goal.
Parallelize independent work so the goal moves fast:

- one worker can inspect upstream `vercel/ai` package shapes,
- one worker can implement a focused Rust slice,
- one worker can verify tests/docs and edge cases.

Keep ownership clear and integrate all worker output in this worktree before
committing. Do not let workers edit the main checkout directly.

## Validation

Run the strongest relevant validation you can before each commit:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

If an optional integration test is added, make it opt-in and documented. It may
load `.env.local`, but it must skip cleanly when the gateway key is absent.

## Work Loop

Repeat this loop until the goal is complete or you hit a real blocker:

1. Pull the latest `main` into your worktree branch.
2. Pick a coherent SDK slice, preferably one that advances generate text or the
   tool loop.
3. Implement it with tests and docs/examples where useful.
4. Run validation.
5. Commit the slice.
6. Merge the slice back to `main` using the protocol below.
7. Continue with the next slice, building on the updated `main`.

Use commit messages like:

```sh
git commit -m "Advance generate text tool loop"
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
cargo test --all-features

git -C "$main_repo" checkout main
git -C "$main_repo" pull --ff-only origin main
git -C "$main_repo" status --short
```

If the main checkout is dirty, stop and report. Do not stash, reset, or
overwrite it.

Merge and push:

```sh
git -C "$main_repo" merge --no-ff "$branch" -m "Merge ai-sdk-rust goal slice"
(
  cd "$main_repo"
  cargo fmt --all --check
  cargo clippy --all-targets --all-features -- -D warnings
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

You are done only when you have shipped meaningful SDK progress, validated it,
merged it to `main`, pushed `main`, and left a short summary of what landed plus
the next likely upstream surface to port.
