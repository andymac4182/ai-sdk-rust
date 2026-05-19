#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run_live_gateway="${AI_SDK_RUST_RUN_LIVE_OTEL:-0}"
if [[ "${1:-}" == "--live-gateway" ]]; then
  run_live_gateway=1
  shift
fi

if (( $# > 0 )); then
  printf 'usage: scripts/check-otel-loopback.sh [--live-gateway]\n' >&2
  exit 2
fi

cargo test -p ai-sdk-otel --features real-opentelemetry receiver

if [[ "$run_live_gateway" == "1" ]]; then
  if [[ -f .env.local ]]; then
    set -a
    # shellcheck disable=SC1091
    source .env.local
    set +a
  fi

  cargo test --all-features \
    live_vercel_ai_gateway_openai_compatible_generate_text_with_otel \
    -- --ignored
  cargo test --all-features \
    live_vercel_ai_gateway_openai_compatible_stream_text_with_otel \
    -- --ignored
  cargo test --all-features \
    live_vercel_ai_gateway_openai_compatible_generate_object_with_otel \
    -- --ignored
  cargo test --all-features \
    live_vercel_ai_gateway_openai_compatible_stream_object_with_otel \
    -- --ignored
  cargo test --all-features \
    live_vercel_ai_gateway_openai_responses_generate_text_with_otel \
    -- --ignored
  cargo test --all-features \
    live_vercel_ai_gateway_openai_responses_stream_text_with_otel \
    -- --ignored
else
  printf 'skipping live Gateway telemetry tests; pass --live-gateway or set AI_SDK_RUST_RUN_LIVE_OTEL=1\n'
fi
