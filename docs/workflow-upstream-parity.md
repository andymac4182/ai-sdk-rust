# Workflow SDK Upstream Parity

This ledger tracks the standalone Vercel Workflow SDK from
[`vercel/workflow`](https://github.com/vercel/workflow). It must not use
`vercel/ai/packages/workflow` as the source of truth.

## Source Snapshot

| Field | Value |
| --- | --- |
| Upstream repo | `vercel/workflow` |
| Inventory command | `npx opensrc fetch https://github.com/vercel/workflow` |
| Local source path | `/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel/workflow/main` |
| Remote HEAD verification | `git ls-remote https://github.com/vercel/workflow.git refs/heads/main` |
| Upstream commit | `1ee63b870afbf9754eb1022b1bb5f02d0ab042f9` |
| Upstream commit date | `2026-05-31T08:48:21Z` |
| Inventory date | `2026-06-01` |
| Upstream package count | 28 packages under `packages/*/package.json` |
| Upstream test files | 144 `*.test.ts`, `*.test.tsx`, `*.spec.ts`, `*.spec.tsx`, `*.test.mts`, and `*.spec.mts` files under `packages/*` |
| Upstream test cases | 2,531 executable `it`/`test` rows in [Workflow SDK Test Inventory](workflow-test-inventory.md), including expanded simple `*.each` tables |
| Foundational runtime test inventory | 79 files and 1,692 executable rows across `packages/errors`, `packages/utils`, `packages/world`, `packages/workflow`, `packages/core`, and `packages/world-local` |

## Status Rules

Use one of these statuses for every row: `not-started`, `in-progress`,
`ported`, `verified`, `js-only-documented`, `type-system-impossible`, or
`needs-review`.

A portable package can only become `verified` after its matching Rust crate owns
the public API, implementation, docs, and every portable upstream test/case.
The skeleton crates in this pass are intentionally `in-progress`: they record
ownership, source metadata, and crate boundaries only.

## Hard Test Parity Gate

Workflow SDK parity follows the same rule as the AI SDK and Chat SDK ledgers:
every portable upstream TypeScript test/case from `vercel/workflow` must have a
named Rust counterpart in the owning Rust crate before the package can be
marked `ported` or `verified`.

Extra Rust tests are additive only. They do not compensate for any missing
upstream TypeScript test/case, table row, fixture-backed scenario, e2e case,
serialization snapshot equivalent, or error-path assertion.

Every upstream row in [Workflow SDK Test Inventory](workflow-test-inventory.md)
must end in exactly one of these states before its package can complete:

- `verified`: the owning Rust crate has a named Rust test counterpart and the
  package validation command is recorded.
- `ported`: the named Rust counterpart exists but the package still awaits
  broader verification.
- `js-only-documented`: the row is explicitly JavaScript, browser, Node,
  framework, or Vercel-host-runtime specific, with the Rust-facing alternative
  documented.
- `type-system-impossible`: the row only proves TypeScript language-service or
  TypeScript type-system behavior that cannot be represented as Rust runtime
  behavior; use Rust compile tests where a meaningful Rust analogue exists.

Rows marked `needs-review` are blocking inventory debt, not exclusions. A
future bucket must reclassify each one as portable, `js-only-documented`, or
`type-system-impossible` before claiming package completion.

Future implementation buckets must start from the case inventory, port all
portable rows owned by their package, fill in the `Rust test name` column, and
leave no portable upstream row without a Rust counterpart. A bucket that
implements behavior without mapping the upstream test rows is incomplete even
if its Rust-only tests pass.

## WF07 Core Runtime Engine Slice

Branch: `codex/workflow-sdk-07-core-runtime-engine`

Base: `098eb03f10bfadd04d4b76cea6320b55eb938906`

Upstream refresh: `npx opensrc fetch https://github.com/vercel/workflow`

Upstream files inspected for this slice:

- `packages/core/src/runtime.ts`
- `packages/core/src/events-consumer.ts`
- `packages/core/src/flushable-stream.ts`
- `packages/core/src/hook-sleep-interaction.test.ts`
- `packages/core/src/runtime.test.ts`
- `packages/core/src/runtime/constants.ts`
- `packages/core/src/runtime/helpers.ts`
- `packages/core/src/runtime/replay-budget.ts`
- `packages/core/src/runtime/run.ts`
- `packages/core/src/runtime/runs.ts`
- `packages/core/src/runtime/start.ts`
- `packages/core/src/runtime/step-executor.ts`
- `packages/core/src/runtime/step-handler.ts`
- `packages/core/src/runtime/suspension-handler.ts`
- `packages/core/src/runtime/wait-completion-replay.test.ts`
- `packages/core/src/runtime/world-init.ts`
- `packages/core/src/runtime/world.ts`
- `packages/core/e2e/*.test.ts`

Implemented runtime surface in `crates/workflow-core`:

- deterministic `RuntimeWorld` trait with `InMemoryWorld` fake for event
  creation, event pagination, run/step state, queue dispatch, deployment
  resolution, encryption-key context capture, hook-token conflicts, and
  retry/error injection;
- start/run APIs with spec-version selection, deployment `latest` resolution,
  resilient start, run serialization, return-value failure hydration fallback,
  and wake-up of pending waits;
- event consumer, replay-budget, replay-timeout handling, queue-name
  validation, event-log loading, wait-completion replay refresh, corrupted
  wait/hook replay guards, step executor/handler, suspension handling, world
  init registry seam, and flushable stream state.

Case mapping in [Workflow SDK Test Inventory](workflow-test-inventory.md):

| Upstream slice | Rows | Classification after WF07 |
| --- | ---: | --- |
| `src/events-consumer.test.ts` | 22 | `verified` with named Rust tests |
| `src/flushable-stream.test.ts` | 10 | `verified` with named Rust tests |
| `src/hook-sleep-interaction.test.ts` | 18 | `verified` with named Rust tests |
| `src/runtime.test.ts` | 5 | `verified` with named Rust tests |
| `src/runtime/constants.test.ts` | 13 | `verified` with named Rust tests |
| `src/runtime/helpers.test.ts` | 19 | `verified` with named Rust tests |
| `src/runtime/replay-budget.test.ts` | 13 | `verified` with named Rust tests |
| `src/runtime/runs.test.ts` | 15 | `verified` with named Rust tests |
| `src/runtime/start.test.ts` | 17 portable + 3 type-only | portable rows `verified`; overload inference rows `type-system-impossible` |
| `src/runtime/step-handler.test.ts` | 18 | `verified` with named Rust tests |
| `src/runtime/wait-completion-replay.test.ts` | 4 | `verified` with named Rust tests |
| `src/runtime/world-init.test.ts` | 3 | `verified` with named Rust tests |
| Host/build/AI E2E rows (`build-errors`, `dev`, `e2e-agent`, `local-build`, `manifest`, source-map helper E2E, selected route/CLI/bundler rows) | 48 | `js-only-documented` |

WF07 verified 157 portable core runtime rows. The generator now emits these
row mappings so `node scripts/workflow-test-inventory.mjs --check` remains the
authoritative gate.

Remaining blocked WF07-adjacent rows:

- 106 `packages/core/e2e/e2e.test.ts` and
  `packages/core/e2e/event-log-race-repro.test.ts` rows remain
  `needs-review`. They mix portable workflow semantics with host VM,
  serializer, class/closure transform, distributed abort, HTTP route, and
  framework runner behavior. A follow-up bucket must classify them
  case-by-case before `packages/core` can be marked complete.
- 3 `packages/core/src/runtime/start.test.ts` overload inference rows are
  explicitly `type-system-impossible` because they assert TypeScript overload
  behavior, not runtime semantics.

## Package Inventory

| Upstream package | Version | Class | Status | Rust owner | Major source and test files | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `packages/ai` (`@workflow/ai`) | `5.0.0-beta.6` | portable Rust runtime | not-started | none | Source: `src/index.ts`, `src/workflow-chat-transport.ts`, `src/stream-iterator.ts`, `src/agent/*.ts`, `src/providers/*.ts`. Tests: 10 files including `src/agent/durable-agent.test.ts`, `src/agent/do-stream-step.test.ts`, `src/workflow-chat-transport.test.ts`. | AI SDK compatibility helpers and durable agent integration. Port after the Workflow core/runtime crates expose stable Rust contracts. |
| `packages/astro` (`@workflow/astro`) | `5.0.0-beta.10` | host-framework binding | not-started | none | Source: `src/builder.ts`, `src/index.ts`, `src/plugin.ts`. Tests: none. | Astro integration; defer until portable runtime and builder contracts exist. |
| `packages/builders` (`@workflow/builders`) | `5.0.0-beta.10` | Rust tooling | not-started | none | Source: `src/base-builder.ts`, `src/build-queue.ts`, `src/workflows-extractor.ts`, `src/swc-esbuild-plugin.ts`, `src/standalone.ts`. Tests: 11 files including `src/discover-entries-esbuild-plugin.test.ts`, `src/get-input-files.test.ts`, `src/workflow-alias.test.ts`. | Build pipeline and transform infrastructure. Needs a separate tooling crate decision before porting behavior. |
| `packages/cli` (`@workflow/cli`) | `5.0.0-beta.10` | Rust tooling | not-started | none | Source: `bin/run.js`, `src/base.ts`, `src/commands/*.ts`, `src/lib/inspect/*.ts`. Tests: `src/lib/inspect/output.test.ts`. | Command surface for build/dev/start/inspect flows. |
| `packages/core` (`@workflow/core`) | `5.0.0-beta.10` | portable Rust runtime | in-progress | `crates/workflow-core` | Source: `runtime.js`, `runtime.d.ts`, `src/workflow.ts`, `src/step.ts`, `src/runtime/*.ts`, `src/serialization/*.ts`, `src/vm/*.ts`, e2e files, and WF04 primitives/diagnostics files listed below. Tests: 52 files including `src/workflow.test.ts`, `src/step.test.ts`, `src/runtime/start.test.ts`, `src/serialization/serialization.test.ts`, `src/vm/index.test.ts`, and e2e tests. | WF04 verifies primitive/diagnostic rows; WF05 verifies portable serialization, encryption, stream framing, observability hydration, async-deserialization ordering, UUID, and base64/hex utility rows; WF06 verifies workflow/step/hook/sleep/abort/context-storage/request-response/writable-stream API rows; WF07 verifies 157 runtime rows with deterministic fake-World tests. Package remains in-progress until remaining core E2E rows are owned/classified. |
| `packages/docs-typecheck` (`@workflow/docs-typecheck`) | `0.0.1-beta.12` | docs/test-only | not-started | none | Source: `scripts/find-incomplete.ts`, `src/extractor.ts`, `src/type-checker.ts`. Tests: `src/__tests__/docs.test.ts`, `src/__tests__/sitemap-guard.test.ts`. | Documentation validation tooling; not part of runtime parity. |
| `packages/errors` (`@workflow/errors`) | `5.0.0-beta.6` | portable Rust runtime | verified | `crates/workflow-errors` | Source: `src/index.ts`, `src/ansi.ts`, `src/error-codes.ts`, `src/internal-chalk.ts`. Tests: 6 files including `src/fatal-error.test.ts`, `src/serialization-error.test.ts`, `src/runtime-decryption-error.test.ts`. | Ports run error codes, framed ANSI/plain rendering, fatal/build/serialization/runtime-decryption/corrupted-event-log errors, and serde-backed stable error contracts. All 36 portable upstream rows are mapped in the test inventory and validated by `cargo test -p workflow-errors`. |
| `packages/nest` (`@workflow/nest`) | `5.0.0-beta.10` | host-framework binding | not-started | none | Source: `src/index.ts`, `src/builder.ts`, `src/cli.ts`, `src/workflow.controller.ts`, `src/workflow.module.ts`. Tests: `src/cjs-rewrite.test.ts`, `src/parse-module-type.test.ts`. | NestJS integration; defer until runtime and builder crates exist. |
| `packages/next` (`@workflow/next`) | `5.0.0-beta.10` | host-framework binding | not-started | none | Source: `src/index.ts`, `src/builder.ts`, `src/builder-deferred.ts`, `src/builder-eager.ts`, `src/runtime.ts`, `src/loader.ts`, `src/socket-server.ts`. Tests: `src/builder.test.ts`, `src/index.test.ts`. | Next.js integration with runtime/socket pieces. Defer host-specific behavior. |
| `packages/nitro` (`@workflow/nitro`) | `5.0.0-beta.10` | host-framework binding | not-started | none | Source: `src/index.ts`, `src/builders.ts`, `src/types.ts`, `src/vite.ts`. Tests: `src/index.test.ts`. | Nitro integration; defer host binding. |
| `packages/nuxt` (`@workflow/nuxt`) | `5.0.0-beta.10` | host-framework binding | not-started | none | Source: `src/module.ts`. Tests: none. | Nuxt module wrapper. |
| `packages/rollup` (`@workflow/rollup`) | `5.0.0-beta.10` | Rust tooling | not-started | none | Source: `src/index.ts`. Tests: none. | Rollup plugin wrapper around build transforms. |
| `packages/serde` (`@workflow/serde`) | `5.0.0-beta.2` | portable Rust runtime | verified | `crates/workflow-serde` | Source: `src/index.ts`. Tests: none. | Owns the distinct upstream serialization marker boundary as `WORKFLOW_SERIALIZE` and `WORKFLOW_DESERIALIZE`; upstream has no test files for this package, so crate-local symbol tests and `cargo test -p workflow-serde` validate the full current surface. |
| `packages/sveltekit` (`@workflow/sveltekit`) | `5.0.0-beta.10` | host-framework binding | not-started | none | Source: `src/index.ts`, `src/builder.ts`, `src/plugin.ts`, `src/vc-config.ts`. Tests: `src/vc-config.test.ts`. | SvelteKit integration. |
| `packages/swc-plugin-workflow` (`@workflow/swc-plugin`) | `5.0.0-beta.4` | Rust tooling | not-started | none | Source: `Cargo.toml`, `src/lib.rs`, `transform/src/lib.rs`, `transform/src/naming.rs`, `spec.md`, `examples/use-step-example.ts`. Tests: transform fixture directories under `transform/tests/*`. | Upstream already includes Rust SWC plugin code. Porting should compare whether to vendor, reference, or reimplement. |
| `packages/tsconfig` (`@workflow/tsconfig`) | `5.0.0-beta.0` | docs/test-only | not-started | none | Source: `base.json`. Tests: none. | TypeScript config package; no Rust runtime surface expected. |
| `packages/typescript-plugin` (`@workflow/typescript-plugin`) | `5.0.0-beta.4` | Rust tooling | not-started | none | Source: `src/index.ts`, `src/diagnostics.ts`, `src/code-fixes.ts`, `src/completions.ts`, `src/hover.ts`. Tests: 5 files including `src/diagnostics.test.ts`, `src/code-fixes.test.ts`, `src/hover.test.ts`. | Language-service plugin behavior. Likely tooling-only unless Rust emits diagnostics independently. |
| `packages/utils` (`@workflow/utils`) | `5.0.0-beta.3` | portable Rust runtime | verified | `crates/workflow-utils` | Source: `src/index.ts`, `src/check-data-dir.ts`, `src/get-port.ts`, `src/parse-name.ts`, `src/pluralize.ts`, `src/promise.ts`, `src/time.ts`, `src/world-target.ts`. Tests: 8 files including `src/check-data-dir.test.ts`, `src/parse-name.test.ts`, `src/time.test.ts`, `src/world-target.test.ts`. | Ports name parsing/formatting, pluralization, duration parsing, world-target resolution, Rust promise/once equivalents, workflow data-dir discovery, process port discovery, and workflow-port probing. All 94 portable upstream rows are mapped in the test inventory and validated by `cargo test -p workflow-utils`. |
| `packages/vite` (`@workflow/vite`) | `5.0.0-beta.10` | Rust tooling | not-started | none | Source: `src/index.ts`, `src/hot-update.ts`. Tests: none. | Vite plugin wrapper around transforms and hot updates. |
| `packages/vitest` (`@workflow/vitest`) | `5.0.0-beta.10` | docs/test-only | not-started | none | Source: `src/index.ts`, `src/options.ts`, `src/global-setup.ts`, `src/setup-file.ts`, `src/vitest-context.d.ts`. Tests: `src/index.test.ts`. | Test harness integration. Defer until runtime testing surface exists. |
| `packages/web` (`@workflow/web`) | `5.0.0-beta.10` | web-only | not-started | none | Source: `app/root.tsx`, `app/routes/*.tsx`, `app/components/**/*.tsx`, `app/lib/client/*.ts`, `server/app.ts`, `server.js`. Tests: 9 files including `app/root.test.tsx`, `app/lib/client/workflow-actions.test.ts`, `app/lib/client/hooks/use-trace-viewer.test.ts`. | Observability UI. Not portable runtime, but its client contracts may inform later API serialization tests. |
| `packages/web-shared` (`@workflow/web-shared`) | `5.0.0-beta.10` | web-only | not-started | none | Source: `src/components/**/*.tsx`, `src/lib/*.ts`, `src/hooks/*.ts`, `src/styles.css`. Tests: 6 files including `test/trace-builder-v1.test.ts`, `test/hydration.test.ts`, `test/exact-event-search-id.test.ts`. | Shared UI components and trace rendering. |
| `packages/workflow` (`workflow`) | `5.0.0-beta.10` | portable Rust runtime | in-progress | `crates/workflow` | Source: `src/index.ts`, `src/workflow.ts`, `src/stdlib.ts`, `src/api.ts`, `src/runtime.ts`, `src/observability.ts`, host subpath files, `bin/run.js`. Tests: `src/internal/builtins.test.ts`, `src/observability.test.ts`, `src/stdlib.test.ts`. | Skeleton facade crate only. Immediate follow-up: keep it as a facade over package-owned crates and avoid putting core-owned behavior here. |
| `packages/world` (`@workflow/world`) | `5.0.0-beta.5` | portable Rust runtime | verified | `crates/workflow-world` | Source: `src/index.ts`, `src/attributes.ts`, `src/interfaces.ts`, `src/runs.ts`, `src/steps.ts`, `src/events.ts`, `src/hooks.ts`, `src/queue.ts`, `src/recovery.ts`, `src/serialization.ts`, `src/shared.ts`, `src/spec-version.ts`, `src/ulid.ts`, `src/waits.ts`. Tests: `src/attributes.test.ts`. | Shared World boundary ported as Rust contracts for attributes, events, hooks, queue, recovery, runs, serialization, shared pagination/stream types, spec versions, steps, ULID timestamp checks, waits, and traits for local/postgres/vercel implementations. All 23 portable upstream `attributes.test.ts` rows have named Rust tests in `workflow-world::attributes`, verified by `cargo test -p workflow-world`. |
| `packages/world-local` (`@workflow/world-local`) | `5.0.0-beta.11` | portable Rust runtime | verified | `crates/workflow-world-local` | Source: `src/index.ts`, `src/config.ts`, `src/fs.ts`, `src/init.ts`, `src/queue.ts`, `src/storage/**/*.ts`, `src/streamer.ts`, `src/telemetry.ts`. Tests: 9 files including `src/storage.test.ts`, `src/queue.test.ts`, `src/reenqueue.test.ts`, `src/tag.test.ts`. | WF 08 ports local config/data-dir handling, safe filesystem helpers, event-sourced storage, queue, streams, re-enqueue, tags, and telemetry helpers. All 314 portable rows in `workflow-test-inventory.md` map to named Rust tests; the 9 prior dynamic `needs-review` rows were inspected and reclassified portable. Validation: `cargo test -p workflow-world-local`. |
| `packages/world-postgres` (`@workflow/world-postgres`) | `5.0.0-beta.9` | portable Rust runtime | verified | `crates/workflow-world-postgres` | Source: `bin/setup.js`, `src/index.ts`, `src/config.ts`, `src/drizzle/**/*.ts`, SQL migrations, `src/queue.ts`, `src/storage.ts`, `src/streamer.ts`. Tests: `src/queue.test.ts`, `src/reenqueue.test.ts`, `src/util.test.ts`, `test/spec.test.ts`, `test/storage.test.ts`. | Ported deterministic Postgres config, Graphile queue planning, SQL migration metadata, in-memory event-sourced storage, and stream pagination. Inventory maps all 103 portable rows to named Rust tests; 2 TypeScript helper/generic rows are classified `type-system-impossible`. Live Docker/Postgres coverage remains credential/container gated outside normal tests. |
| `packages/world-testing` (`@workflow/world-testing`) | `5.0.0-beta.10` | docs/test-only | not-started | none | Source: `src/*.mts`, `workflows/*`, `scripts/generate-well-known-dts.mjs`. Tests: `test/embedded.test.ts`, `test/inline-batches-debug.test.ts`. | Test workflows and harness utilities. |
| `packages/world-vercel` (`@workflow/world-vercel`) | `5.0.0-beta.9` | portable Rust runtime with Vercel HTTP contracts | verified | `crates/workflow-world-vercel` | Source: `src/index.ts`, `src/http-client.ts`, `src/queue.ts`, `src/runs.ts`, `src/steps.ts`, `src/storage.ts`, `src/streamer.ts`, `src/run-id/*.ts`, `src/telemetry.ts`. Tests: 7 files including `src/encryption.test.ts`, `src/run-id/index.test.ts`, `src/streamer.test.ts`. | Ported tagged run-id codec, region table, HKDF run-key derivation, Vercel API request/response contracts, queue header/retry/re-enqueue planning, stream multi-chunk framing, and storage endpoint contracts. Inventory maps all 110 portable rows to named Rust tests; live Vercel calls remain credential gated outside normal tests. |

## Crate Status

| Rust crate | Upstream package | Status | Scope in this pass |
| --- | --- | --- | --- |
| `workflow` | `packages/workflow` (`workflow`) | in-progress | Facade crate with source metadata and re-export placeholders for foundational crates. |
| `workflow-core` | `packages/core` (`@workflow/core`) | in-progress | WF04 adds core primitives, schemas, utilities, and diagnostics; WF05 adds portable serialization, encryption, stream framing, observability hydration, ordering, UUID, and base64/hex utilities; WF06 adds workflow/step/hook/sleep/abort/context-storage/request-response/writable-stream APIs; WF07 adds deterministic runtime engine tests over fake World/event-log seams. Remaining core E2E rows stay classified separately. |
| `workflow-errors` | `packages/errors` (`@workflow/errors`) | verified | Error taxonomy, framed rendering, fatal classification, runtime decryption context, serialization/corrupted-log errors, and stable serde/display contracts. |
| `workflow-serde` | `packages/serde` (`@workflow/serde`) | verified | Dedicated marker crate for the upstream serialization/deserialization symbol names. |
| `workflow-utils` | `packages/utils` (`@workflow/utils`) | verified | Foundation utilities covering every portable upstream `packages/utils` test row. |
| `workflow-world` | `packages/world` (`@workflow/world`) | verified | Shared World contracts plus all 23 portable upstream attributes tests. |
| `workflow-world-local` | `packages/world-local` (`@workflow/world-local`) | verified | Local World implementation with filesystem-backed config/init/storage/events/queue/streaming/recovery/tagging and named Rust parity tests for every portable upstream row. |
| `workflow-world-postgres` | `packages/world-postgres` (`@workflow/world-postgres`) | verified | Package-owned Postgres world port with config validation, SQL migration metadata, Graphile queue contracts, deterministic in-memory storage/streamer behavior, and 103 named portable row tests. |
| `workflow-world-vercel` | `packages/world-vercel` (`@workflow/world-vercel`) | verified | Package-owned Vercel world port with run-id, encryption, HTTP, queue, storage endpoint, and streamer contracts plus 110 named portable row tests. |

## WF 05 Core Serialization, Encryption, and VM Utilities

| Area | Upstream rows | Rust status | Notes |
| --- | --- | --- | --- |
| Format prefixes and serialization codec | `packages/core/src/serialization/serialization.test.ts`, `packages/core/src/serialization-format.test.ts`, portable rows in `packages/core/src/serialization.test.ts` | ported in `workflow-core` | Rust uses a tagged, serde-backed portable value model under the upstream `devl` prefix, with deterministic JSON snapshot assertions for wire payloads. |
| AES-256-GCM encryption envelopes | `packages/core/src/encryption.test.ts`, encryption rows in `packages/core/src/serialization*.test.ts` | ported in `workflow-core` | Preserves `[nonce][ciphertext+tag]`, 32-byte key validation, `encr` envelope wrapping, wrong-key/tamper errors, and diagnostic context. |
| Stream frame serialization | `getSerializeStream`, `getDeserializeStream`, and encrypted stream rows in `packages/core/src/serialization.test.ts` | ported in `workflow-core` | Portable framing is `[4-byte big-endian length][format-prefixed payload]`; JS Web Streams are not exposed as Rust objects. |
| Observability hydration helpers | hydrate/resource/stream/class-ref rows in `packages/core/src/serialization-format.test.ts` | ported in `workflow-core` | Covers serialized display/reference payloads such as stream IDs, class-instance refs, workflow refs, encrypted placeholders, and expired stubs. |
| Async deserialization ordering | `packages/core/src/async-deserialization-ordering.test.ts` | ported in `workflow-core` | Rust helper verifies concurrent hydration work is published in event-log order. |
| VM UUID/base64 utilities | `packages/core/src/vm/uuid.test.ts`, `packages/core/src/vm/uint8array-base64.test.ts`, portable `btoa`/`atob` rows in `packages/core/src/vm/index.test.ts` | ported in `workflow-core` | Covers deterministic UUID v4 formatting and TC39-style base64/hex byte conversions. |
| JavaScript-only serialization/runtime identity | JS function proxies, class constructors/static symbols, cross-VM constructor identity, AbortController/AbortSignal event objects, Request/Response/ReadableStream/WritableStream object transfer, and Node `vm.Context` global patching | documented as `js-only-documented` in the inventory | Rust keeps typed reference/data payloads where portable and does not claim callable JS identity or Node VM behavior. |

## Immediate Follow-Up Queue

1. Continue with the workflow facade, AI integration, builders/CLI tooling, and
   host classification buckets now that shared, local, Postgres, and Vercel
   world contracts are merged.
2. Keep `workflow` as a facade; behavior should land in the matching
   package-owned crates and only be re-exported from the facade.

## WF04 Core Primitives, Schemas, Utilities, And Diagnostics

Bucket branch `codex/workflow-sdk-04-core-primitives` owns the portable upstream
rows for these `packages/core/src` files:

| Upstream test file | Rows | Verified portable rows | JS-only rows | Rust test name pattern |
| --- | --- | --- | --- | --- |
| `capabilities.test.ts` | 14 | 14 | 0 | `wf04_capabilities_row_NNN` |
| `classify-error.test.ts` | 14 | 14 | 0 | `wf04_classify_error_row_NNN` |
| `context-errors.test.ts` | 16 | 15 | 1 | `wf04_context_errors_row_NNN` |
| `define-hook.test.ts` | 3 | 3 | 0 | `wf04_define_hook_row_NNN` |
| `describe-error.test.ts` | 30 | 30 | 0 | `wf04_describe_error_row_NNN` |
| `global.test.ts` | 23 | 23 | 0 | `wf04_global_row_NNN` |
| `log-format.test.ts` | 8 | 8 | 0 | `wf04_log_format_row_NNN` |
| `logger.test.ts` | 12 | 12 | 0 | `wf04_logger_row_NNN` |
| `schemas.test.ts` | 2 | 2 | 0 | `wf04_schemas_row_NNN` |
| `set-attributes.test.ts` | 5 | 5 | 0 | `wf04_set_attributes_row_NNN` |
| `source-map.test.ts` | 1 | 1 | 0 | `wf04_source_map_row_NNN` |
| `types.test.ts` | 11 | 11 | 0 | `wf04_types_row_NNN` |
| `util.test.ts` | 23 | 23 | 0 | `wf04_utility_row_NNN` |
| **Total** | **162** | **161** | **1** |  |

The single JS-only row is
`packages/core/src/context-errors.test.ts:209`, which asserts V8
`Error.captureStackTrace` stack-frame rewriting. Rust has no equivalent V8
stack-frame redirection behavior; Rust reporting uses native call sites and
backtraces instead.

Implementation scope landed in `crates/workflow-core` for capabilities,
classification, context errors, hook schema validation/resume wiring,
run-error descriptions, workflow suspension primitives, log formatting,
structured logger metadata merging, invoke payload schemas, host-side
attribute normalization/posting seams, inline source-map extraction failure
behavior, abort-error helpers, and queue/stream ID utilities. A minimal
`workflow-errors` taxonomy was added only to support the core diagnostic
surfaces above.

## Verified Core WF06 Rows

WF06 ports the portable upstream rows owned by the core workflow, step, hook,
sleep, abort, context-storage, request/response, and writable stream API slice.
The generated case inventory records the exact Rust test name for each row via
`docs/workflow-test-overrides.json`.

| Upstream file | Verified rows | Rust test module |
| --- | ---: | --- |
| `packages/core/src/abort-controller-step.test.ts` | 18 | `crates/workflow-core/tests/upstream_parity.rs` |
| `packages/core/src/abort-controller.test.ts` | 22 | `crates/workflow-core/tests/upstream_parity.rs` |
| `packages/core/src/define-hook.test.ts` | 3 | `crates/workflow-core/tests/upstream_parity.rs` |
| `packages/core/src/step.test.ts` | 30 | `crates/workflow-core/tests/upstream_parity.rs` |
| `packages/core/src/step/context-storage.test.ts` | 3 | `crates/workflow-core/tests/upstream_parity.rs` |
| `packages/core/src/step/writable-stream.test.ts` | 5 | `crates/workflow-core/tests/upstream_parity.rs` |
| `packages/core/src/types.test.ts` | 3 | `crates/workflow-core/tests/upstream_parity.rs` |
| `packages/core/src/workflow.test.ts` | 75 | `crates/workflow-core/tests/upstream_parity.rs` |
| `packages/core/src/workflow/hook.test.ts` | 30 | `crates/workflow-core/tests/upstream_parity.rs` |
| `packages/core/src/workflow/sleep.test.ts` | 12 | `crates/workflow-core/tests/upstream_parity.rs` |
| `packages/core/src/writable-stream.test.ts` | 18 | `crates/workflow-core/tests/upstream_parity.rs` |
| **Total** | **219** |  |
