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

## Required Work Order

The implementation order is a hard two-phase gate:

1. Finish ALL common/core SDK packages together with Vercel AI Gateway provider
   coverage, including Gateway's OpenAI-compatible and Open Responses routes.
2. Only then resume unrelated standalone provider packages.

The first phase includes `packages/ai`, `packages/provider`,
`packages/provider-utils`, `packages/openai-compatible`,
`packages/open-responses`, `packages/gateway`, Vercel AI Gateway's
OpenAI-compatible and Open Responses routes, and portable non-provider rows such
as MCP, OTel, Workflow, telemetry, logger, UI transport, chat/completion
transport, and test-server support. Treat Vercel AI Gateway as part of this
first phase, not as one of the later standalone provider packages. Do not pick
another standalone provider slice while any first-phase row is still
`not-started` or `in-progress`, unless that row is explicitly documented as
intentionally non-portable.

## Priorities

1. Preserve the existing Rust 2024 crate style, serde shapes, builder helpers,
   error/result style, and public exports.
2. Align JSON boundaries with upstream provider-v4 contracts while omitting
   JavaScript-only concepts such as `AbortSignal`.
3. Add focused serialization/deserialization and behavior tests for every new
   public contract.
4. Enforce strict 1:1 crate/package ownership now. Every portable upstream
   `vercel/ai` TypeScript package that gains Rust API must have exactly one
   matching Rust workspace crate before that API lands, and no Rust crate may
   own APIs from more than one upstream package. This is a merge-blocking
   acceptance gate for every iteration, not cleanup for a later pass.
5. Treat the current root-crate consolidation as active architecture debt. We
   are already merging multiple TypeScript packages into one Rust crate today,
   and every additional package folded into that crate makes the future split
   harder, more coupled, and more breaking. New package-owned implementation
   must not be staged in the root crate or any other consolidated crate. If the
   matching crate does not exist yet, create it first.
6. Treat the root crate as a facade, not an implementation home or staging
   area. It may aggregate re-exports and compatibility shims, and if it is the
   Rust equivalent of `packages/ai`, it may own only that package's API. It
   must not also own provider contracts, provider utilities, provider
   implementations, MCP, workflow, telemetry, adapters, or other
   package-owned surfaces.
7. Before adding or reviewing API for any upstream TypeScript package, create
   or use its matching Rust crate and put the package-owned types and
   implementation there. A parity slice that ports a TypeScript package without
   creating or using its matching Rust crate is blocked, incomplete, and not
   mergeable even if the implementation itself works and has passing tests.
   Passing tests in the wrong crate prove behavior, not parity. A package row
   cannot be marked `verified` while its portable implementation is owned by
   the wrong crate.
8. Do not use temporary staging exceptions for new package-owned
   implementation. A temporary exception may only cover an unavoidable
   transitional shim or extraction of existing root-crate debt, and it must be
   documented in the ledger with the destination crate, the reason the matching
   crate cannot land in the same slice, and the smallest concrete extraction
   follow-up. Do not use this exception for convenience, to land a working
   implementation faster, or to keep merging unrelated packages into one crate.
9. Build and verify high-level APIs against deterministic fake/test models
   before adding real provider networking.
10. `generate_text(...)` and tool loops remain an early vertical-slice priority:
   prove prompts/settings, model calls, tool calls, typed Rust tools, tool
   results, continuation until final text/max steps, and `GenerateTextResult`.
11. Add deterministic end-to-end tests for plain text generation, single tool
   call, multi-step tool call, tool error, unknown tool, invalid tool args, max
   step exhaustion, streaming/event sequences, structured output, provider
   metadata, and every additional high-level API as it lands.
12. Ban vague generic naming such as `helpers`, `utils`, `common`, `misc`,
   `stuff`, `shared`, and similar buckets in source paths, module names, crate
   names, public APIs, and docs. Prefer precise responsibility names. Add or
   improve a custom check to enforce this convention. Document explicit
   exceptions only when mirroring upstream package names.
13. Do not churn dependencies, CI, or unrelated modules unless the next SDK
    slice genuinely requires it.
14. Work in this order as a hard gate: finish ALL common/core SDK packages
    together with Vercel AI Gateway provider coverage first, then return to the
    remaining standalone providers. The first phase includes `packages/ai`,
    `packages/provider`, `packages/provider-utils`, `packages/openai-compatible`,
    `packages/open-responses`, `packages/gateway`, the Vercel AI Gateway
    OpenAI-compatible and Open Responses routes, and portable non-provider
    package rows such as MCP, OTel, Workflow, telemetry, logger, UI transport,
    chat/completion transport, and test-server support. Standalone provider
    slices are blocked while any of those rows are not yet verified or
    explicitly documented as intentionally non-portable.
15. Port every upstream provider package in its matching crate. Prefer
    contract-first typed provider crates with fake/deterministic tests, then add
    HTTP/gateway-backed integration tests where credentials are available. Do
    not add root modules for provider-owned API except re-export shims or
    cross-crate primitives that are not owned by the provider package.
16. Port examples and docs once the corresponding API works. Rust examples
    should be runnable and should map clearly to upstream examples.
17. When enough works end to end, add a kitchen sink example app that
    demonstrates working generate text, tool execution, provider contracts, and
    any available gateway-backed validation.
18. Keep expanding until the parity ledger is complete. A single slice is never
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
3. Pick the highest-value unported or unverified upstream package/API/provider
   from the first-phase queue until that queue is closed: ALL common/core SDK
   packages together with Vercel AI Gateway provider coverage, including the
   Gateway OpenAI-compatible and Open Responses routes. Do not select an
   unrelated standalone provider slice while any first-phase row is still
   `not-started` or `in-progress`.
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
4. The Rust workspace has a strict 1:1 crate mapping for every portable
   upstream TypeScript package: one matching Rust crate per package, no Rust
   crate owning APIs from multiple upstream packages, and the root crate limited
   to the `packages/ai` facade plus aggregate re-exports and compatibility
   shims. Existing root-crate package debt must be extracted before completion.
   Until then it is incomplete and cannot count as verified parity.
5. The full validation suite passes.
6. The final complete slice is merged to `main` and pushed.

If any ledger item remains `not-started` or `in-progress`, keep working.
