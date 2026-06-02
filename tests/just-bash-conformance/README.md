# Just Bash Conformance Harness

This harness runs the upstream `vercel-labs/just-bash` Vitest comparison tests
against either the upstream TypeScript `Bash` implementation or the Rust backend
exposed by the JBC-01 napi-rs adapter.

Refresh the upstream mirror before making parity claims:

```bash
npx opensrc fetch https://github.com/vercel-labs/just-bash
git ls-remote https://github.com/vercel-labs/just-bash HEAD refs/heads/main
```

Install upstream test dependencies once:

```bash
npx --yes pnpm@10.33.2 --dir /Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/just-bash/main install --frozen-lockfile
```

Playback mode uses the upstream fixture JSON files. The TypeScript engine runs
all upstream comparison domains by default; the Rust engine runs the generated
verified comparison corpus by default so pending command-family rows do not get
smoke-mapped. Use `--all`, `--domain`, or `JUST_BASH_DOMAINS` with the Rust
engine when you intentionally want the raw upstream comparison sweep and its
pending failures.

```bash
JUST_BASH_ENGINE=typescript JUST_BASH_DOMAINS=echo,cat node scripts/just-bash-conformance.mjs
JUST_BASH_ENGINE=rust JUST_BASH_RUST_ADDON=/absolute/path/to/jbc-01/adapter.js JUST_BASH_DOMAINS=echo,cat node scripts/just-bash-conformance.mjs
JUST_BASH_ENGINE=rust node scripts/just-bash-conformance.mjs
```

Until JBC-01 lands, the Rust path can prove harness wiring with the explicit
fixture-backed stub:

```bash
JUST_BASH_ENGINE=rust JUST_BASH_RUST_STUB=fixtures JUST_BASH_DOMAINS=echo,cat node scripts/just-bash-conformance.mjs
```

Record mode preserves upstream locked fixtures unless forced:

```bash
JUST_BASH_ENGINE=typescript JUST_BASH_DOMAINS=echo node scripts/just-bash-conformance.mjs --record
JUST_BASH_ENGINE=typescript JUST_BASH_DOMAINS=echo node scripts/just-bash-conformance.mjs --record-force
```

Domain selection accepts repeated flags or a comma-separated list:

```bash
node scripts/just-bash-conformance.mjs --list-domains
node scripts/just-bash-conformance.mjs --engine typescript --domain echo --domain cat
node scripts/just-bash-conformance.mjs --engine typescript --domains echo,cat
node scripts/just-bash-conformance.mjs --engine typescript --all
```

If `JUST_BASH_ENGINE=rust` is used without `JUST_BASH_RUST_ADDON` or
`JUST_BASH_RUST_STUB=fixtures`, the runner exits successfully with a precise
missing-addon skip diagnostic. Set `JUST_BASH_REQUIRE_RUST_ADDON=1` to make that
condition fail in CI.
