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

## Source Snapshot

| Field | Value |
| --- | --- |
| Upstream repo | `vercel-labs/just-bash` |
| Upstream refresh | `npx opensrc fetch https://github.com/vercel-labs/just-bash` |
| Upstream verification | `git ls-remote https://github.com/vercel-labs/just-bash HEAD refs/heads/main` |
| Current tracked upstream commit | `d64009aef6bc1556e7c84b22ed455863275ea953` |
| OpenSrc cache | `/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/just-bash/main` |
| Corpus path | `fixtures/just-bash-conformance/corpus.json` |
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
| portable-pending | 1629 |
| portable-verified | 42 |

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
