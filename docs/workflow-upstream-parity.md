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
| Upstream test files | 144 test sources under `packages/*`, including JavaScript/TypeScript `*.test.*`/`*.spec.*` files and the upstream Rust SWC fixture harnesses |
| Upstream test cases | 2,679 executable `it`/`test` rows and SWC fixture rows in [Workflow SDK Test Inventory](workflow-test-inventory.md), including expanded simple `*.each` tables |
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

## Package Inventory

| Upstream package | Version | Class | Status | Rust owner | Major source and test files | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `packages/ai` (`@workflow/ai`) | `5.0.0-beta.6` | portable Rust runtime | not-started | none | Source: `src/index.ts`, `src/workflow-chat-transport.ts`, `src/stream-iterator.ts`, `src/agent/*.ts`, `src/providers/*.ts`. Tests: 10 files including `src/agent/durable-agent.test.ts`, `src/agent/do-stream-step.test.ts`, `src/workflow-chat-transport.test.ts`. | AI SDK compatibility helpers and durable agent integration. Port after the Workflow core/runtime crates expose stable Rust contracts. |
| `packages/astro` (`@workflow/astro`) | `5.0.0-beta.10` | host-framework binding | not-started | none | Source: `src/builder.ts`, `src/index.ts`, `src/plugin.ts`. Tests: none. | Astro integration; defer until portable runtime and builder contracts exist. |
| `packages/builders` (`@workflow/builders`) | `5.0.0-beta.10` | Rust tooling | verified | `crates/workflow-builders` | Source: `src/base-builder.ts`, `src/build-queue.ts`, `src/workflows-extractor.ts`, `src/swc-esbuild-plugin.ts`, `src/standalone.ts`. Tests: 11 files including `src/discover-entries-esbuild-plugin.test.ts`, `src/get-input-files.test.ts`, `src/workflow-alias.test.ts`. | Portable helper rows are verified in `workflow-builders`: input discovery, diagnostics/sourcemap resolution, module specifier/import-path resolution, workflow aliases, regexp pre-scan helpers, pseudo-package constants, and node-module helper parsing. Esbuild build plugins, external package warning orchestration, dynamic import bundling, and SWC-esbuild wrapper behavior are `js-only-documented`. |
| `packages/cli` (`@workflow/cli`) | `5.0.0-beta.10` | Rust tooling | verified | `crates/workflow-cli` | Source: `bin/run.js`, `src/base.ts`, `src/commands/*.ts`, `src/lib/inspect/*.ts`. Tests: `src/lib/inspect/output.test.ts`. | Portable inspect output expired-data formatting is verified in `workflow-cli`. Command execution, update checks, Vercel API access, and terminal UI orchestration remain JavaScript host tooling. |
| `packages/core` (`@workflow/core`) | `5.0.0-beta.10` | portable Rust runtime | in-progress | `crates/workflow-core` | Source: `runtime.js`, `runtime.d.ts`, `src/workflow.ts`, `src/step.ts`, `src/runtime/*.ts`, `src/serialization/*.ts`, `src/vm/*.ts`, `e2e/*.ts`. Tests: 52 files including `src/workflow.test.ts`, `src/step.test.ts`, `src/runtime/start.test.ts`, `src/serialization/serialization.test.ts`, `src/vm/index.test.ts`, and e2e tests. | Skeleton crate only. Immediate follow-up: port public workflow/step/runtime contracts and enumerate all 52 upstream test files case-by-case. |
| `packages/docs-typecheck` (`@workflow/docs-typecheck`) | `0.0.1-beta.12` | docs/test-only | not-started | none | Source: `scripts/find-incomplete.ts`, `src/extractor.ts`, `src/type-checker.ts`. Tests: `src/__tests__/docs.test.ts`, `src/__tests__/sitemap-guard.test.ts`. | Documentation validation tooling; not part of runtime parity. |
| `packages/errors` (`@workflow/errors`) | `5.0.0-beta.6` | portable Rust runtime | in-progress | `crates/workflow-errors` | Source: `src/index.ts`, `src/ansi.ts`, `src/error-codes.ts`, `src/internal-chalk.ts`. Tests: 6 files including `src/fatal-error.test.ts`, `src/serialization-error.test.ts`, `src/runtime-decryption-error.test.ts`. | Skeleton crate only. Immediate follow-up: port error taxonomy and framed-message behavior before core error handling lands. |
| `packages/nest` (`@workflow/nest`) | `5.0.0-beta.10` | host-framework binding | not-started | none | Source: `src/index.ts`, `src/builder.ts`, `src/cli.ts`, `src/workflow.controller.ts`, `src/workflow.module.ts`. Tests: `src/cjs-rewrite.test.ts`, `src/parse-module-type.test.ts`. | NestJS integration; defer until runtime and builder crates exist. |
| `packages/next` (`@workflow/next`) | `5.0.0-beta.10` | host-framework binding | not-started | none | Source: `src/index.ts`, `src/builder.ts`, `src/builder-deferred.ts`, `src/builder-eager.ts`, `src/runtime.ts`, `src/loader.ts`, `src/socket-server.ts`. Tests: `src/builder.test.ts`, `src/index.test.ts`. | Next.js integration with runtime/socket pieces. Defer host-specific behavior. |
| `packages/nitro` (`@workflow/nitro`) | `5.0.0-beta.10` | host-framework binding | not-started | none | Source: `src/index.ts`, `src/builders.ts`, `src/types.ts`, `src/vite.ts`. Tests: `src/index.test.ts`. | Nitro integration; defer host binding. |
| `packages/nuxt` (`@workflow/nuxt`) | `5.0.0-beta.10` | host-framework binding | not-started | none | Source: `src/module.ts`. Tests: none. | Nuxt module wrapper. |
| `packages/rollup` (`@workflow/rollup`) | `5.0.0-beta.10` | JavaScript host tooling | js-only-documented | none | Source: `src/index.ts`. Tests: none. | Rollup plugin wrapper around build transforms; no Rust runtime surface is forced into this port. |
| `packages/serde` (`@workflow/serde`) | `5.0.0-beta.2` | portable Rust runtime | not-started | none | Source: `src/index.ts`. Tests: none. | Serialization symbols are a core dependency. Immediate follow-up: decide whether this needs its own `workflow-serde` crate before porting core serialization. |
| `packages/sveltekit` (`@workflow/sveltekit`) | `5.0.0-beta.10` | host-framework binding | not-started | none | Source: `src/index.ts`, `src/builder.ts`, `src/plugin.ts`, `src/vc-config.ts`. Tests: `src/vc-config.test.ts`. | SvelteKit integration. |
| `packages/swc-plugin-workflow` (`@workflow/swc-plugin`) | `5.0.0-beta.4` | Rust tooling | verified | `crates/workflow-swc-plugin` | Source: `Cargo.toml`, `src/lib.rs`, `transform/src/lib.rs`, `transform/src/naming.rs`, `spec.md`, `examples/use-step-example.ts`. Tests: transform fixture directories under `transform/tests/*`. | Upstream Rust transform and fixture/error trees are vendored into `workflow-swc-plugin` with the upstream SWC dependency line pinned to compatible patches. NPM/WASM packaging glue remains host tooling. |
| `packages/tsconfig` (`@workflow/tsconfig`) | `5.0.0-beta.0` | docs/test-only | not-started | none | Source: `base.json`. Tests: none. | TypeScript config package; no Rust runtime surface expected. |
| `packages/typescript-plugin` (`@workflow/typescript-plugin`) | `5.0.0-beta.4` | TypeScript language-service tooling | type-system-impossible | none | Source: `src/index.ts`, `src/diagnostics.ts`, `src/code-fixes.ts`, `src/completions.ts`, `src/hover.ts`. Tests: 5 files including `src/diagnostics.test.ts`, `src/code-fixes.test.ts`, `src/hover.test.ts`. | All 96 rows assert TypeScript language-service diagnostics, code fixes, completions, hover, or type-system behavior. They are `type-system-impossible` until a Rust diagnostic analogue is intentionally designed with compile/runtime tests. |
| `packages/utils` (`@workflow/utils`) | `5.0.0-beta.3` | portable Rust runtime | in-progress | `crates/workflow-utils` | Source: `src/index.ts`, `src/check-data-dir.ts`, `src/get-port.ts`, `src/parse-name.ts`, `src/pluralize.ts`, `src/promise.ts`, `src/time.ts`, `src/world-target.ts`. Tests: 8 files including `src/check-data-dir.test.ts`, `src/parse-name.test.ts`, `src/time.test.ts`, `src/world-target.test.ts`. | Skeleton crate only. Immediate follow-up: port low-level naming/time/world-target helpers because core and errors depend on them. |
| `packages/vite` (`@workflow/vite`) | `5.0.0-beta.10` | JavaScript host tooling | js-only-documented | none | Source: `src/index.ts`, `src/hot-update.ts`. Tests: none. | Vite plugin wrapper around transforms and hot updates; no Rust runtime surface is forced into this port. |
| `packages/vitest` (`@workflow/vitest`) | `5.0.0-beta.10` | docs/test-only | not-started | none | Source: `src/index.ts`, `src/options.ts`, `src/global-setup.ts`, `src/setup-file.ts`, `src/vitest-context.d.ts`. Tests: `src/index.test.ts`. | Test harness integration. Defer until runtime testing surface exists. |
| `packages/web` (`@workflow/web`) | `5.0.0-beta.10` | web-only | not-started | none | Source: `app/root.tsx`, `app/routes/*.tsx`, `app/components/**/*.tsx`, `app/lib/client/*.ts`, `server/app.ts`, `server.js`. Tests: 9 files including `app/root.test.tsx`, `app/lib/client/workflow-actions.test.ts`, `app/lib/client/hooks/use-trace-viewer.test.ts`. | Observability UI. Not portable runtime, but its client contracts may inform later API serialization tests. |
| `packages/web-shared` (`@workflow/web-shared`) | `5.0.0-beta.10` | web-only | not-started | none | Source: `src/components/**/*.tsx`, `src/lib/*.ts`, `src/hooks/*.ts`, `src/styles.css`. Tests: 6 files including `test/trace-builder-v1.test.ts`, `test/hydration.test.ts`, `test/exact-event-search-id.test.ts`. | Shared UI components and trace rendering. |
| `packages/workflow` (`workflow`) | `5.0.0-beta.10` | portable Rust runtime | in-progress | `crates/workflow` | Source: `src/index.ts`, `src/workflow.ts`, `src/stdlib.ts`, `src/api.ts`, `src/runtime.ts`, `src/observability.ts`, host subpath files, `bin/run.js`. Tests: `src/internal/builtins.test.ts`, `src/observability.test.ts`, `src/stdlib.test.ts`. | Skeleton facade crate only. Immediate follow-up: keep it as a facade over package-owned crates and avoid putting core-owned behavior here. |
| `packages/world` (`@workflow/world`) | `5.0.0-beta.5` | portable Rust runtime | in-progress | `crates/workflow-world` | Source: `src/index.ts`, `src/interfaces.ts`, `src/runs.ts`, `src/steps.ts`, `src/events.ts`, `src/hooks.ts`, `src/queue.ts`, `src/serialization.ts`, `src/spec-version.ts`, `src/waits.ts`. Tests: `src/attributes.test.ts`. | Skeleton crate only. Immediate follow-up: port the World trait/contracts before world implementations. |
| `packages/world-local` (`@workflow/world-local`) | `5.0.0-beta.11` | portable Rust runtime | in-progress | `crates/workflow-world-local` | Source: `src/index.ts`, `src/config.ts`, `src/fs.ts`, `src/init.ts`, `src/queue.ts`, `src/storage/**/*.ts`, `src/streamer.ts`, `src/telemetry.ts`. Tests: 9 files including `src/storage.test.ts`, `src/queue.test.ts`, `src/reenqueue.test.ts`, `src/tag.test.ts`. | Skeleton crate only. Immediate follow-up: port local storage/queue after `workflow-world` interfaces exist. |
| `packages/world-postgres` (`@workflow/world-postgres`) | `5.0.0-beta.9` | portable Rust runtime | not-started | none | Source: `bin/setup.js`, `src/index.ts`, `src/config.ts`, `src/drizzle/**/*.ts`, SQL migrations, `src/queue.ts`, `src/storage.ts`, `src/streamer.ts`. Tests: `src/queue.test.ts`, `src/reenqueue.test.ts`, `src/util.test.ts`, `test/spec.test.ts`, `test/storage.test.ts`. | PostgreSQL world implementation. Needs a separate crate after the shared world interface is stable. |
| `packages/world-testing` (`@workflow/world-testing`) | `5.0.0-beta.10` | docs/test-only | not-started | none | Source: `src/*.mts`, `workflows/*`, `scripts/generate-well-known-dts.mjs`. Tests: `test/embedded.test.ts`, `test/inline-batches-debug.test.ts`. | Test workflows and harness utilities. |
| `packages/world-vercel` (`@workflow/world-vercel`) | `5.0.0-beta.9` | host-framework binding | not-started | none | Source: `src/index.ts`, `src/http-client.ts`, `src/queue.ts`, `src/runs.ts`, `src/steps.ts`, `src/storage.ts`, `src/streamer.ts`, `src/run-id/*.ts`, `src/telemetry.ts`. Tests: 7 files including `src/encryption.test.ts`, `src/run-id/index.test.ts`, `src/streamer.test.ts`. | Vercel platform world implementation; port after shared and local world contracts prove the boundary. |

## Workflow Rust Crates

| Rust crate | Upstream package | Status | Scope in this pass |
| --- | --- | --- | --- |
| `workflow` | `packages/workflow` (`workflow`) | in-progress | Facade crate with source metadata and re-export placeholders for foundational crates. |
| `workflow-builders` | `packages/builders` (`@workflow/builders`) | verified | Portable builder helper surface with named Rust tests for all portable builder rows; JavaScript build-host plugin rows are documented as JS-only. |
| `workflow-cli` | `packages/cli` (`@workflow/cli`) | verified | Portable inspect output expired-data helpers with named Rust tests for all CLI rows. |
| `workflow-core` | `packages/core` (`@workflow/core`) | in-progress | Core runtime ownership marker with source metadata only. |
| `workflow-errors` | `packages/errors` (`@workflow/errors`) | in-progress | Error package ownership marker with source metadata only. |
| `workflow-swc-plugin` | `packages/swc-plugin-workflow` (`@workflow/swc-plugin`) | verified | Vendored upstream Rust SWC transform plus fixture/error trees; package host/WASM packaging glue remains outside the Rust runtime surface. |
| `workflow-utils` | `packages/utils` (`@workflow/utils`) | in-progress | Utility package ownership marker with source metadata only. |
| `workflow-world` | `packages/world` (`@workflow/world`) | in-progress | World interface ownership marker with source metadata only. |
| `workflow-world-local` | `packages/world-local` (`@workflow/world-local`) | in-progress | Local World implementation ownership marker and `workflow-world` re-export placeholder. |

## Immediate Follow-Up Queue

1. Port `packages/utils` helpers first enough to unblock `packages/errors` and
   `packages/core`.
2. Port `packages/errors` error taxonomy and upstream tests before core runtime
   error handling.
3. Decide the `packages/serde` crate boundary before implementing core
   serialization, because upstream core depends on it directly.
4. Expand `workflow-world` into concrete Rust contracts, then use those
   contracts to start `workflow-world-local`.
5. Keep `workflow` as a facade; behavior should land in the matching
   package-owned crates and only be re-exported from the facade.
6. Revisit the remaining `core` and `world-local` `needs-review` rows in their
   owning runtime buckets; this tooling pass only resolved the rows whose
   behavior belongs to builders, CLI, SWC, Vite/Rollup wrappers, or the
   TypeScript language-service plugin.
