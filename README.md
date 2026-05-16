# AI SDK Rust

An idiomatic Rust port of the Vercel AI SDK.

This repository is starting from a minimal Rust 2024 library crate with CI in
place. The public API will grow in small, tested slices as the TypeScript SDK is
ported into Rust-native modules.

## Development

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

## GNHF Codex Runner

Use the repo-local helper to run gnhf with Codex on GPT-5.5 using xhigh
reasoning, without changing your global `~/.gnhf/config.yml`.

```sh
npm install -g gnhf
codex login

scripts/gnhf-codex-xhigh.sh --worktree --max-iterations 10 "port the next small SDK slice"
```

All arguments after `scripts/gnhf-codex-xhigh.sh` are passed through to `gnhf`.
