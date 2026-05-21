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

Non-negotiable test floor: EVERY portable test/case from the original upstream
TypeScript packages must exist as an equivalent Rust test in the matching 1:1
crate. Rust may add more tests for stronger coverage, but it must never have
fewer mapped original TypeScript tests, and one missing upstream portable test is
a completion blocker.

Future-iteration test note: the matching Rust crate must contain EVERY portable
test/case from the original TypeScript package as a named Rust counterpart.
Rust may include potentially more tests for Rust-specific proof, but never
fewer mapped original TypeScript tests; extra Rust tests are additive only and
do not offset or replace any missing upstream case. A crate is incomplete until
the original TypeScript test inventory is fully represented in Rust,
test-for-test or case-for-case, or explicitly documented as JavaScript-only.

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
7. A named test-case parity map for every portable original upstream
   TypeScript test/case, showing the matching Rust test in the owning 1:1 crate
   or an explicit JavaScript-only/non-portable justification. This map must be
   based on the original TypeScript test inventory, not Rust test counts; Rust
   may add more tests, but every original portable TypeScript test must exist
   in Rust before a package can be marked `verified`.
   Future iterations must treat this as a test-for-test Rust superset
   requirement: EVERY portable original TypeScript test/case must exist in the
   matching Rust crate, Rust may add extra tests on top, and a crate with fewer
   mapped original tests than upstream is incomplete.

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
intentionally non-portable. This gate applies to the entire first-phase set as
a group: do not use Gateway progress as a reason to resume Anthropic, Google,
Bedrock, xAI, or any other unrelated provider while a common/core package row is
still open.

## Priorities

1. Preserve the existing Rust 2024 crate style, serde shapes, builder helpers,
   error/result style, and public exports.
2. Align JSON boundaries with upstream provider-v4 contracts while omitting
   JavaScript-only concepts such as `AbortSignal`.
3. Add focused serialization/deserialization and behavior tests for every new
   public contract.
4. Port EVERY portable test from the original upstream TypeScript package into
   Rust before marking that package row `verified`. This is a hard minimum:
   Rust may add more tests for Rust-specific safety, typing, and failure modes,
   but it must never have fewer portable tests than upstream. Every
   future iteration must prove that EVERY original portable TypeScript test
   exists in Rust in the matching crate; extra Rust tests are allowed only on
   top of that floor, never instead of it, and "more Rust tests overall" is not
   acceptable when any original upstream test/case is missing. Every
   `*.test.ts`, `*.test.tsx`, `*.test-d.ts`, `*.test-d.tsx`, `*.spec.ts`, and
   `*.spec.tsx` case must have an equivalent Rust test in the matching 1:1
   crate, including table-driven cases, fixture/snapshot-equivalent cases,
   streaming edge cases, error paths, provider option serialization, and
   type-level assertions where Rust can express them. Count the individual
   upstream test/case as the unit of parity; do not collapse the inventory to a
   file-level, feature-level, or broad behavioral checklist. Missing upstream
   tests are missing parity, even if broader Rust tests happen to cover similar
   behavior.
   Treat the upstream TypeScript test inventory as the floor and the Rust test
   inventory as a required superset: every portable upstream case must be
   counted, mapped to a Rust counterpart, and kept in the ledger until ported or
   explicitly documented as JavaScript-only.
   Future iterations must replicate the original upstream test list
   one-to-one first, then add Rust-specific tests on top where useful. Extra
   Rust tests never substitute for an unported upstream case, and a smaller
   Rust-specific suite is not acceptable parity.
   In practical terms, the matching Rust crate must include every portable test
   from the original TypeScript package and may include more Rust tests, but
   never fewer. Missing one original upstream test/case is a parity failure
   until it is ported or explicitly documented as JavaScript-only/non-portable
   in the ledger.
   The acceptance rule is strict: EVERY original portable TypeScript test/case
   must exist in Rust in the matching crate. Rust can add extra coverage on top,
   but it cannot have a smaller test inventory than the original TypeScript
   package for any portable surface.
   Future-run note: when a worker claims a slice or package is complete, that
   claim must be backed by the original upstream TypeScript test list and a
   Rust counterpart for every portable original test/case. Additional Rust tests
   are welcome only as additive coverage; a Rust crate with even one fewer
   portable original TypeScript test/case is incomplete.
   The minimum passing state is the complete portable original TypeScript test
   inventory recreated in Rust for that matching crate, with any Rust-only tests
   counted only as extra coverage on top.
   Do not accept a slice that reports more Rust tests overall while any original
   portable TypeScript test/case remains unmapped or unported.
   Future handoffs must show the named mapping from each original portable
   TypeScript test/case to its Rust counterpart in the matching crate. Total
   Rust test count is not proof of parity by itself, because Rust-only tests
   cannot offset a missing original upstream case.
   Explicit no-less-tests rule: for each portable upstream TypeScript package,
   the matching Rust crate must contain every original upstream portable
   test/case before any extra Rust tests are counted. The acceptable end state
   is the full original TypeScript test inventory recreated in Rust, plus
   optional additional Rust tests; anything less is incomplete. Treat this as
   inventory containment: every original portable TypeScript test exists in
   Rust, and Rust may have more tests, but no less mapped upstream coverage.
   Count parity from the original upstream TypeScript test list, not from the
   number of Rust tests. A Rust crate with extra Rust-specific tests but even
   one missing original portable upstream test/case is still incomplete. The
   only acceptable comparison is: every original portable TypeScript test/case
   exists in the matching Rust crate, and Rust may then add more tests on top,
   but never fewer.
   Read EVERY literally: enumerate the original TypeScript tests first, port
   each portable case into Rust, document any JavaScript-only exception, and
   only then count additional Rust tests as additive coverage.
5. For provider-backed behavior, require two layers of proof before marking a
   row `verified`: deterministic fake/mock/transport tests that run in normal
   validation, plus credential-gated live provider validation when a usable
   credential exists. Live validation must be opt-in (`#[ignore]` tests or
   runnable examples), skip cleanly when credentials are missing, never print
   secrets, and be recorded in the ledger with the test/example name and date.
   If live credentials or the upstream API are unavailable, the ledger must say
   so explicitly; passing deterministic tests alone is not enough to claim
   real-provider verification.
6. For OTel/telemetry behavior, require deterministic span-attribute tests plus
   local OTLP/HTTP export validation before marking OTel-backed rows
   `verified`. The local validation should use the loopback OTLP receiver or a
   local OpenTelemetry Collector endpoint, assert the emitted wire payload, and
   not rely on external credentials. Once root telemetry wiring exists,
   provider live tests should run with telemetry enabled and verify the emitted
   OTLP data through that local receiver or collector.
7. Enforce strict 1:1 crate/package ownership now. Every portable upstream
   `vercel/ai` TypeScript package that gains Rust API must have exactly one
   matching Rust workspace crate before that API lands, and no Rust crate may
   own APIs from more than one upstream package. This is a merge-blocking
   acceptance gate for every iteration, not cleanup for a later pass.
8. Treat the current root-crate consolidation as active architecture debt. We
   are already merging multiple TypeScript packages into one Rust crate today,
   and every additional package folded into that crate makes the future split
   harder, more coupled, and more breaking. New package-owned implementation
   must not be staged in the root crate or any other consolidated crate. If the
   matching crate does not exist yet, create it first.
9. Treat the root crate as a facade, not an implementation home or staging
   area. It may aggregate re-exports and compatibility shims, and if it is the
   Rust equivalent of `packages/ai`, it may own only that package's API. It
   must not also own provider contracts, provider utilities, provider
   implementations, MCP, workflow, telemetry, adapters, or other
   package-owned surfaces.
10. Before adding or reviewing API for any upstream TypeScript package, create
   or use its matching Rust crate and put the package-owned types and
   implementation there. A parity slice that ports a TypeScript package without
   creating or using its matching Rust crate is blocked, incomplete, and not
   mergeable even if the implementation itself works and has passing tests.
   Passing tests in the wrong crate prove behavior, not parity. A package row
   cannot be marked `verified` while its portable implementation is owned by
   the wrong crate.
11. Do not use temporary staging exceptions for new package-owned
   implementation. A temporary exception may only cover an unavoidable
   transitional shim or extraction of existing root-crate debt, and it must be
   documented in the ledger with the destination crate, the reason the matching
   crate cannot land in the same slice, and the smallest concrete extraction
   follow-up. Do not use this exception for convenience, to land a working
   implementation faster, or to keep merging unrelated packages into one crate.
12. Build and verify high-level APIs against deterministic fake/test models
   before adding real provider networking.
13. `generate_text(...)` and tool loops remain an early vertical-slice priority:
   prove prompts/settings, model calls, tool calls, typed Rust tools, tool
   results, continuation until final text/max steps, and `GenerateTextResult`.
14. Add deterministic end-to-end tests for plain text generation, single tool
   call, multi-step tool call, tool error, unknown tool, invalid tool args, max
   step exhaustion, streaming/event sequences, structured output, provider
   metadata, and every additional high-level API as it lands.
15. Ban vague generic naming such as `helpers`, `utils`, `common`, `misc`,
   `stuff`, `shared`, and similar buckets in source paths, module names, crate
   names, public APIs, and docs. Prefer precise responsibility names. Add or
   improve a custom check to enforce this convention. Document explicit
   exceptions only when mirroring upstream package names.
16. Do not churn dependencies, CI, or unrelated modules unless the next SDK
    slice genuinely requires it.
17. Work in this order as a hard gate: finish ALL common/core SDK packages
    together with Vercel AI Gateway provider coverage first, then return to the
    remaining standalone providers. The first phase includes `packages/ai`,
    `packages/provider`, `packages/provider-utils`, `packages/openai-compatible`,
    `packages/open-responses`, `packages/gateway`, the Vercel AI Gateway
    OpenAI-compatible and Open Responses routes, and portable non-provider
    package rows such as MCP, OTel, Workflow, telemetry, logger, UI transport,
    chat/completion transport, and test-server support. Standalone provider
    slices are blocked while any of those rows are not yet verified or
    explicitly documented as intentionally non-portable. The correct order is
    not "pick another provider after Gateway has some coverage"; it is "finish
    the whole common/core plus Vercel AI Gateway phase, then pick the remaining
    providers."
18. Port every upstream provider package in its matching crate. Prefer
    contract-first typed provider crates with fake/deterministic tests, then add
    HTTP/gateway-backed integration tests where credentials are available. Do
    not add root modules for provider-owned API except re-export shims or
    cross-crate primitives that are not owned by the provider package.
19. Port examples and docs once the corresponding API works. Rust examples
    should be runnable and should map clearly to upstream examples.
20. When enough works end to end, add a kitchen sink example app that
    demonstrates working generate text, tool execution, provider contracts, and
    any available gateway-backed validation.
21. Keep expanding until the parity ledger is complete. A single slice is never
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
For provider or Gateway slices, add or maintain a targeted live test/example
that exercises the real upstream service when credentials are present. Run that
live validation before changing a provider-backed row to `verified`; otherwise
leave the row `in-progress` and document the missing live proof.
For OTel/telemetry slices, add or maintain a local OTLP/HTTP receiver/export
test that captures the actual wire payload. Use `scripts/check-otel-loopback.sh`
as the required local proof command: it runs the package-owned
`LocalOtlpTraceReceiver` checks and the real Rust `opentelemetry` SDK/exporter
probe against that receiver. Use
`cargo run -p ai-sdk-otel --example local_otlp_receiver` for manual
daemon-style validation. Once provider telemetry wiring is available, run
`scripts/check-otel-loopback.sh --live-gateway` or an equivalent
credential-gated provider test with the local OTLP receiver so the same run
proves both provider behavior and emitted telemetry.

## Work Loop

Repeat this loop until the goal is complete or you hit a real blocker:

1. Pull the latest `main` into your worktree branch.
2. Re-scan or consult `docs/upstream-parity.md`.
3. Pick the highest-value unported or unverified upstream package/API/provider
   from the first-phase queue until that queue is closed: ALL common/core SDK
   packages together with Vercel AI Gateway provider coverage, including the
   Gateway OpenAI-compatible and Open Responses routes. Do not select an
   unrelated standalone provider slice while any first-phase row is still
   `not-started` or `in-progress`. If a slice is not part of that common/core
   plus Vercel AI Gateway phase, it is out of order until the first phase is
   closed.
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
4. Every portable test from the original upstream TypeScript packages exists as
   an equivalent Rust test in the matching crate. The unit is each original
   upstream test/case, including table rows, fixtures, snapshots, streaming
   cases, error paths, provider options, and portable type-level assertions.
   Rust may have more tests, but it must not have fewer portable tests than
   upstream. Completion requires an explicit test inventory mapping, not a
   sampled, reduced, file-level, or feature-level Rust suite.
   More Rust tests are allowed and encouraged; fewer mapped tests than the
   original TypeScript package is a completion blocker.
   No package passes completion by having "enough" Rust-native coverage. It
   passes only when every original portable TypeScript test/case exists in the
   matching Rust crate, with any additional Rust tests counted strictly on top.
5. The Rust workspace has a strict 1:1 crate mapping for every portable
   upstream TypeScript package: one matching Rust crate per package, no Rust
   crate owning APIs from multiple upstream packages, and the root crate limited
   to the `packages/ai` facade plus aggregate re-exports and compatibility
   shims. Existing root-crate package debt must be extracted before completion.
   Until then it is incomplete and cannot count as verified parity.
6. The full validation suite passes.
7. The final complete slice is merged to `main` and pushed.

If any ledger item remains `not-started` or `in-progress`, keep working.
