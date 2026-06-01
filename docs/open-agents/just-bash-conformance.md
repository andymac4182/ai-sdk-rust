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

## Upstream Source

- Upstream repo: `https://github.com/vercel-labs/just-bash`
- Refreshed command: `npx opensrc fetch https://github.com/vercel-labs/just-bash`
- OpenSrc cache: `/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/just-bash/main`
- Current tracked upstream commit: `d64009aef6bc1556e7c84b22ed455863275ea953`

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

## Close Criteria

Just Bash is not parity-complete until all of these pass on current
`origin/main`:

```sh
node scripts/just-bash-test-inventory.mjs --check
node scripts/just-bash-test-inventory.mjs --strict
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
