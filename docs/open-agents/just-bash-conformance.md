# Just Bash Conformance Plan

This plan extends the parent TypeScript-to-Rust parity goal with a shared
conformance path for `vercel-labs/just-bash`.

## Objective

The Rust `crates/just-bash` engine must run the same portable command behavior
as upstream TypeScript Just Bash. The proof path is not a hand-picked Rust test
suite: upstream Just Bash cases are inventoried first, then each portable case
is mapped to a named Rust test, a generated conformance-corpus case, or an
explicit `js-only-documented` / `type-system-impossible` exception.

The shared harness must support running the same case corpus against both
engines:

- `JUST_BASH_ENGINE=typescript` runs upstream TypeScript Just Bash.
- `JUST_BASH_ENGINE=rust` runs Rust Just Bash through a small `napi-rs` bridge.

Rust-specific tests may add coverage, but they never replace the upstream case
inventory. Strict parity closes only when every portable upstream row in
`docs/open-agents/just-bash-parity.md` is verified or explicitly excepted.

## Current Status

Just Bash is now part of the parent TypeScript-to-Rust parity goal and tracked
alongside Open Agents, AI SDK, Chat SDK, Workflow SDK, and Open Plugin Spec.
The current parity ledger maps 2,078 upstream rows to named Rust tests, NAPI-backed JS proofs, or generated corpus proofs, leaves
7,699 rows `portable-pending`, documents 159 JS-only exceptions, and has 7,710 strict gate gaps. The remaining closure wave is tracked as JBC-19 through
JBC-32 in `docs/ts-to-rust-migration-tracker.md`.

## Source Snapshot

| Field | Value |
| --- | --- |
| Upstream repo | `vercel-labs/just-bash` |
| Upstream refresh | `npx opensrc fetch https://github.com/vercel-labs/just-bash` |
| Upstream verification | `git ls-remote https://github.com/vercel-labs/just-bash HEAD refs/heads/main` |
| Current tracked upstream commit | `d64009aef6bc1556e7c84b22ed455863275ea953` |
| OpenSrc cache | `/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/just-bash/main` |
| Corpus path | `fixtures/just-bash-conformance/corpus.json` |
| Rust runner fixture | `crates/just-bash/tests/fixtures/just-bash-conformance.json` |
| Parity ledger | `docs/open-agents/just-bash-parity.md` |
| Corpus check command | `node scripts/just-bash-conformance-corpus.mjs --check` |

## Work Buckets

| ID | Scope | Required output | Verification |
| --- | --- | --- | --- |
| JBC-01 | Rust-to-JavaScript NAPI adapter | `crates/just-bash-napi` exposes the Rust engine to JS with constructor/session setup, `exec`, virtual filesystem helpers, cwd/env helpers, and command discovery. | `cargo test -p just-bash-napi`; `cargo clippy -p just-bash-napi --all-targets --all-features -- -D warnings`; `npm test --prefix crates/just-bash-napi`. |
| JBC-02 | JavaScript dual-engine harness | A JS runner executes selected upstream cases with `JUST_BASH_ENGINE=typescript` or `JUST_BASH_ENGINE=rust` without polluting the upstream OpenSrc inventory. | TypeScript-engine smoke; Rust-engine smoke or explicit missing-addon diagnostic; `node scripts/just-bash-test-inventory.mjs --check`. |
| JBC-03 | Conformance corpus generator | Stable JSON corpus with upstream case ids, command domains, fixtures, env/cwd/stdin, expected stdout/stderr/exit code, and ledger links. | Corpus generator dry run; fixture hash check; no missing ledger links. |
| JBC-04 | Rust corpus runner | Data-driven Rust tests load the shared corpus, seed virtual state, run `just_bash::Bash`, and report upstream ids on mismatch. | `cargo test -p just-bash --test conformance_corpus`; `cargo test -p just-bash`. |
| JBC-05 | Ledger and CI gates | This plan, tracker rows, master-gate integration, and documented strict close criteria. | `node scripts/just-bash-test-inventory.mjs --check`; `scripts/master-parity-gate.sh --check`. |
| JBC-06 | Core command parity | Close portable filesystem command rows such as `cat`, `ls`, `mkdir`, `rm`, `cp`, and `mv` with named Rust tests and ledger mappings. | Focused command tests; inventory check; shared fmt/clippy/naming/diff gates. |
| JBC-07 | Text and structured command parity | Close text/search/structured command rows such as `grep`, `rg`, `sed`, `awk`, `head`, `tail`, `wc`, `sort`, `uniq`, `cut`, `tr`, and `jq`. | Focused text/search/structured tests; corpus subset; inventory check; shared fmt/clippy/naming/diff gates. |
| JBC-08 | Open Agents service proof | Prove Open Agents service/local-E2E reaches crate-backed Just Bash, persists virtual filesystem state, maps failures, and avoids host `/bin/bash` fallback. | `cargo test -p open-agents-service`; `scripts/open-agents-local-e2e.sh --just-bash-conformance`; `cargo test -p open-agents-service -p open-agents-sandbox -p just-bash`. |
| JBC-09 | AWK command parity | Close exact portable `command:awk` rows for print, fields, separators, BEGIN/END, patterns, stdin/files, and diagnostics. | Focused AWK tests; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-10 | Ripgrep command parity | Close exact portable `command:rg` rows for recursive virtual search, filters, flags, context, hidden/binary handling, and stdin where portable. | Focused rg tests; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-11 | Comparison corpus closure | Promote generated comparison corpus rows into Rust/JS closure where they pass without masking command-family gaps. | `cargo test -p just-bash --test conformance_corpus`; inventory/corpus checks; `cargo test -p just-bash`. |
| JBC-12 | Syntax and transform parity | Close exact portable parser/shell/AST transform rows for syntax, quoting, redirection, expansion, functions, and control flow. | Focused parser/shell tests; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-13 | Advanced filesystem parity | Close exact portable overlay/core/read-write/mountable filesystem rows, path behavior, symlinks, encoding, and error shapes. | Focused FS tests; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-14 | Security and sandbox parity | Close exact portable security/sandbox/fuzz/prototype-pollution rows or classify true JS-only worker/browser rows narrowly. | Focused security tests; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-15 | Interpreter core and expansion parity | Close exact portable interpreter builtins/core/expansion rows for dispatch, assignment, expansion, substitution, arithmetic, arrays, loops, and diagnostics. | Focused interpreter tests; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-16 | Structured and data command parity | Close exact portable `jq`, `yq`, `xan`, `sqlite3`, and adjacent data/query command rows that can run deterministically in-memory. | Focused structured command tests; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-17 | Executor package and examples closure | Close remaining `packages/just-bash-executor` rows plus portable example-package behavior with the shared session/executor API, no-host-shell semantics, fixture seeding, and named Rust or NAPI-backed JS comparison tests. | Fresh upstream fetch; focused executor/example tests; `cargo test -p just-bash -p just-bash-napi`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-18 | CLI, package entrypoint, and distribution closure | Close portable CLI/package rows for argument parsing, command invocation shape, CJS/ESM package entry behavior, help/version output, and documented JS-only packaging exceptions where runtime Rust parity is impossible. | CLI fixture tests or NAPI harness tests; `cargo test -p just-bash`; `npm test --prefix crates/just-bash-napi` where relevant; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-19 | Shell syntax, transform, heredoc, and plugin pipeline closure | Close remaining parser/serializer/transform/plugin/heredoc rows, including AST round trips, transform metadata, plugin ordering, quoting, redirection, here-doc execution equivalence, and shell pipeline edge cases. | Focused parser/transform tests; `cargo test -p just-bash`; syntax/transform corpus subset; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-20 | Core runtime, environment, and session semantics closure | Close remaining core/runtime rows for cwd/env persistence, scoped variables, command status, cancellation/time limits, error formatting, sandbox result metadata, and comparison rows that exercise core execution semantics. | Focused runtime/session tests; `cargo test -p just-bash`; conformance corpus subset; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-21 | Small POSIX-style command family closure | Close portable rows for bounded commands such as `base64`, `basename`, `chmod`, `column`, `comm`, `date`, `dirname`, `du`, `expand`, `file`, `fold`, `gzip`, `join`, `ln`, `md5sum`, `nl`, `od`, `paste`, `readlink`, `rev`, `seq`, `sleep`, `split`, `stat`, `strings`, `tar`, `tee`, `test`, `timeout`, `tree`, `which`, `whoami`, and `xargs`. | Focused command-family tests; conformance corpus command subsets; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-22 | Find, curl, network, and external resource seams closure | Close portable `find`, `curl`, network policy, and resource-fetch rows with deterministic in-memory or fake HTTP transports, while narrowly documenting browser/worker/live-network-only rows as JS-only only when proven. | Fake network/resource tests; focused `find`/`curl` tests; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-23 | Text command deep parity closure | Close remaining portable rows for `grep`, `sed`, `sort`, `printf`, `echo`, `head`, `tail`, `cut`, `tr`, `uniq`, `wc`, and related text-stream behavior, preserving exact stdout/stderr/exit-code expectations. | Focused text command tests; conformance corpus text subsets; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-24 | Structured data command deep parity closure | Continue exact row closure for the larger `jq`, `yq`, `xan`, `sqlite3`, query-engine, and search-engine surfaces beyond the bounded JBC-16 slice, using deterministic in-memory data fixtures. | Focused structured/data tests; corpus structured subsets; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-25 | AWK extended parity closure | Close remaining portable `command:awk` rows beyond the JBC-09 slice, including expressions, builtins, variables, ranges, arrays where supported, diagnostics, stdin/file behavior, and comparison fixtures. | Focused AWK tests; conformance corpus AWK subset; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-26 | Filesystem semantics and path edge closure | Close remaining `fs:*`, file-operation, path normalization, symlink, mount routing, overlay precedence, permissions, binary/text encoding, and error-shape rows not covered by JBC-13. | Focused filesystem/path tests; corpus file-op subsets; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-27 | Security, limits, and attack corpus closure | Close remaining security attack, limits, sandbox, fuzzing, prototype-pollution, and policy rows with deterministic Rust tests, documenting only true JavaScript worker/browser hardening rows as JS-only exceptions. | Focused security/fuzz/limits tests; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-28 | Host-runtime command and JS/Python boundary closure | Close portable `js-exec`, `python3`, worker-bridge, and host-runtime boundary rows through deterministic adapters or explicit JS-only exceptions, without enabling silent host shell fallback in Open Agents. | Adapter/fake-runtime tests; `cargo test -p just-bash -p open-agents-sandbox`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-29 | Comparison fixture broad closure | Run and promote the remaining portable comparison fixture rows through the dual-engine corpus, splitting command-family failures back to owning rows instead of broad smoke-mapping them. | `JUST_BASH_ENGINE=typescript node scripts/just-bash-conformance.mjs`; `JUST_BASH_ENGINE=rust node scripts/just-bash-conformance.mjs`; `cargo test -p just-bash --test conformance_corpus`; inventory/corpus checks. |
| JBC-30 | Agent examples and Open Agents command integration closure | Close `agent-examples` and Open Agents command-execution rows proving Slack remote-agent tasks can use crate-backed Just Bash for multi-command workflows, stateful virtual files, failure surfaces, and no sandbox fallback unless explicitly selected. | `cargo test -p open-agents-service -p open-agents-sandbox -p just-bash`; `scripts/open-agents-local-e2e.sh --just-bash-conformance`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-31 | Upstream docs/examples parity closure | Close portable README, docs, examples, custom-command, website, and bash-agent example behavior that represents public Just Bash API usage, with explicit exceptions for docs-only or browser-only rows. | Docs/example inventory check; NAPI/JS harness examples where applicable; `cargo test -p just-bash`; inventory/corpus checks; fmt/clippy/naming/diff gates. |
| JBC-32 | Just Bash strict parity burn-down and final audit | Reconcile all remaining rows after JBC-19 through JBC-31, remove stale pending owners, prove every portable row maps to a named Rust test or corpus case, document final exceptions, and flip the strict gate only when zero `portable-pending` rows remain. | `node scripts/just-bash-test-inventory.mjs --strict`; `node scripts/just-bash-conformance-corpus.mjs --check`; TypeScript and Rust dual-engine conformance runs; `scripts/master-parity-gate.sh --check`; full fmt/clippy/naming/diff gates. |

## Corpus Contract

- Every row has a stable `id`, `kind`, `classification`, command `script`,
  isolated `cwd`, `env`, `stdin`, `args`, `options`, `initialFiles`, and
  `expected.stdout/stderr/exitCode` fields.
- Exact golden expectations are stored as strings or numbers. A null
  expectation means the upstream row is source-traceable but does not expose
  that exact value.
- `parity.ledgerKey` points to the matching row in
  `docs/open-agents/just-bash-parity.md` using
  `file:line:declaration:testName`.
- `sourceFixtureMetadata` preserves comparison fixture IDs, locked fixture
  status, spec fixture source lines, and extraction details needed for drift
  review.
- The generator verifies the live upstream HEAD. Upstream drift fails `--check`
  until the corpus and metadata are refreshed.

## Coverage Summary

| Kind | Cases |
| --- | --- |
| comparison-fixture | 457 |
| spec-case | 1028 |
| unit-exec | 186 |

## Domain Summary

| Domain | Cases |
| --- | --- |
| awk | 22 |
| basename-dirname | 15 |
| cat | 7 |
| column-join | 9 |
| comparison-tests | 15 |
| cut | 15 |
| echo | 91 |
| file-ops | 99 |
| find | 22 |
| glob | 2 |
| grep | 830 |
| head-tail | 20 |
| here-document | 5 |
| jq | 28 |
| ls | 12 |
| mkdir | 8 |
| mv | 3 |
| paste | 14 |
| pipes-redirections | 12 |
| printf | 64 |
| pwd-cd-env | 44 |
| rm | 8 |
| sed | 257 |
| sort | 22 |
| strings-split | 4 |
| tar | 4 |
| test | 1 |
| tr | 14 |
| uniq | 13 |
| wc | 11 |

## Ledger Status Summary

| Status | Cases |
| --- | --- |
| portable-pending | 1360 |
| portable-verified | 311 |

## Rust Runner Fixture

The Rust integration test consumes a generated portable subset at
`crates/just-bash/tests/fixtures/just-bash-conformance.json`. JBC-11
promotes only comparison fixture rows that the Rust backend matched exactly for
stdout, stderr, and exit code; mismatching rows remain `portable-pending`.

| Field | Value |
| --- | --- |
| Exact-pass comparison cases | 179 |
| Expected failures | 0 |
| Test command | `cargo test -p just-bash --test conformance_corpus` |

| Domain | Exact-pass cases |
| --- | --- |
| awk | 9 |
| cat | 7 |
| cut | 14 |
| echo | 26 |
| grep | 19 |
| head-tail | 12 |
| jq | 10 |
| ls | 9 |
| pipes-redirections | 10 |
| pwd-cd-env | 6 |
| sed | 22 |
| sort | 11 |
| tr | 14 |
| uniq | 10 |

## Representative Slice

The generated corpus includes the required representative domains: echo,
printf, pwd/cd/env, file operations, grep, sed, and awk. Comparison fixtures
provide exact Bash goldens for echo/grep/sed/awk and many file/text-processing
commands. Unit rows add command-level printf, pwd, env, cd, and file-operation
source traceability. Spec rows add imported grep/sed fixture cases with stdin
and expected output.

## Refresh Workflow

1. Run `npx opensrc fetch https://github.com/vercel-labs/just-bash`.
2. Verify `git ls-remote https://github.com/vercel-labs/just-bash HEAD refs/heads/main`.
3. Run `node scripts/just-bash-conformance-corpus.mjs`.
4. Run `node scripts/just-bash-conformance-corpus.mjs --check`.
5. Run `node scripts/just-bash-test-inventory.mjs --check` so the parity ledger and corpus stay aligned.

## Close Criteria

Just Bash is not parity-complete until all of these pass on current
`origin/main`:

```sh
node scripts/just-bash-test-inventory.mjs --check
node scripts/just-bash-test-inventory.mjs --strict
node scripts/just-bash-conformance-corpus.mjs --check
cargo test -p just-bash
cargo test -p just-bash --test conformance_corpus
cargo test -p just-bash-napi
npm test --prefix crates/just-bash-napi
scripts/open-agents-local-e2e.sh --just-bash-conformance
scripts/master-parity-gate.sh --check
scripts/check-naming-conventions.sh
git diff --check
```

Credential-gated or deployment-only checks may be documented as ignored live
proofs, but they do not replace the local strict inventory and conformance
gates.
