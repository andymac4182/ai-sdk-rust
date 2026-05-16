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
