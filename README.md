# AI SDK Rust

An idiomatic Rust port of the Vercel AI SDK.

This repository is starting from a minimal Rust 2024 library crate with CI in
place. The public API will grow in small, tested slices as the TypeScript SDK is
ported into Rust-native modules.

This repo also hosts a parallel port of the Vercel **Chat SDK**
([`vercel/chat`](https://github.com/vercel/chat)) under `crates/chat-sdk-*`,
driven by its own Codex `/goal` session — see
[`scripts/codex-goal-chat/`](scripts/codex-goal-chat/) and
[`docs/chat/upstream-parity.md`](docs/chat/upstream-parity.md). The two `/goal`
sessions are designed to run concurrently, with file ownership partitioned by
crate prefix (`ai-sdk-*` vs `chat-sdk-*`) and per-port doc folders
(`docs/` flat for ai-sdk, `docs/chat/` for chat-sdk), and a shared merge lock
to serialize pushes to `main`.

## Development

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
scripts/check-naming-conventions.sh
cargo test --all-features
```

## Examples

```sh
cargo run --example kitchen_sink
cargo run --example vercel_ai_gateway_text
cargo run --example vercel_ai_gateway_responses
cargo run --example vercel_ai_gateway_models
cargo run --example vercel_ai_gateway_image
```

## Local OTel Receiver

Use the package-owned loopback receiver when a telemetry slice needs proof that
real OTLP/HTTP trace data is emitted:

```sh
scripts/check-otel-loopback.sh
```

That check runs the dependency-free exporter proof, root dispatcher export proof
for both `OpenTelemetry` and `LegacyOpenTelemetry`, and the real Rust
OpenTelemetry SDK OTLP/HTTP JSON exporter against the loopback receiver. To also
run the ignored live Vercel AI Gateway OpenAI-compatible text/object telemetry
tests plus OpenAI Responses generate/stream telemetry tests when `.env.local`
contains credentials:

```sh
scripts/check-otel-loopback.sh --live-gateway
```

To run only the receiver as a local process:

```sh
cargo run -p ai-sdk-otel --example local_otlp_receiver
```

The receiver prints a `127.0.0.1` `/v1/traces` endpoint and the matching
`OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` value. For manual daemon-style checks, keep
it running until stopped:

```sh
AI_SDK_RUST_OTEL_RECEIVER_SECONDS=0 \
AI_SDK_RUST_OTEL_RECEIVER_REQUESTS=0 \
cargo run -p ai-sdk-otel --example local_otlp_receiver
```

## Codex `/goal` Runner

Use the repo-local script to run Codex CLI `/goal` on GPT-5.5 with xhigh
reasoning in an explicit sibling git worktree. The goal is full portable parity
with upstream `vercel/ai`, tracked in `docs/upstream-parity.md`:

```sh
codex login
scripts/run-codex-goal-port.sh
```

The script copies the compact `/goal` condition to your clipboard. In Codex
CLI, run `/goal` and paste it.

Worktrees are created under:

```sh
/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust-goal-worktrees
```

If `.env.local` exists in the main checkout, the script symlinks it into the
worktree so ignored gateway credentials are available for optional integration
tests without being pushed.

## Chat SDK `/goal` Runner (sibling port)

To run the parallel Vercel Chat SDK port instead of (or alongside) the AI SDK
port:

```sh
scripts/run-codex-goal-chat-port.sh
```

This creates a worktree under
`/Users/andrewmcclenaghan/dev/andymac4182/ai-sdk-rust-chat-worktrees`, copies
the compact `/goal` condition to your clipboard, and starts Codex pointing at
[`scripts/codex-goal-chat/port-chat-sdk.md`](scripts/codex-goal-chat/port-chat-sdk.md).
The chat session is constrained to `crates/chat-sdk-*` plus `docs/chat/`, and
shares the same `/tmp/ai-sdk-rust-main-merge.lock` so the two sessions
serialize their pushes to `main`.

## GNHF Codex Runner

Use the repo-local script to run gnhf with Codex on GPT-5.5 using xhigh
reasoning, without changing your global `~/.gnhf/config.yml`.

```sh
npm install -g gnhf
codex login

scripts/run-gnhf-port.sh
```

By default this runs:

```sh
--current-branch --push
```

That keeps going until you stop it, gnhf reaches its failure limit, the agent
runs out of usable quota, or you pass your own stop condition. Pass custom gnhf
flags to override the defaults:

```sh
scripts/run-gnhf-port.sh --current-branch --push --max-tokens 50000000
```
