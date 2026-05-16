#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

prompt="$(cat <<'PROMPT'
Port the Vercel AI SDK to idiomatic Rust incrementally.

Continue from the existing code and .gnhf notes. Each iteration should make one small, commit-ready improvement toward a Rust-native SDK. Prioritize a working `generate_text` vertical slice with tool-loop execution over adding more horizontal provider surface area.

Use the upstream Vercel AI SDK as the source of truth for shapes and behavior. To inspect it locally, use:
npx opensrc@latest path github:vercel/ai

Priorities:
1. Preserve the existing Rust 2024 crate style, serde shapes, builder helpers, and public exports.
2. Align JSON boundaries with upstream provider-v4 contracts, while omitting JavaScript-only concepts such as AbortSignal.
3. Add focused serialization/deserialization tests for every new public contract.
4. Run cargo fmt --all --check, cargo clippy --all-targets --all-features -- -D warnings, and cargo test --all-features before reporting success.
5. Plan the Rust workspace around the upstream Vercel AI SDK package boundaries. Prefer crate splits that mirror upstream responsibilities such as core AI APIs, provider contracts, provider utilities, and provider implementations. Introduce crates when there is enough real API surface to justify the boundary; avoid empty placeholder crates.
6. When adding a new surface, consider whether it belongs in the current crate or should start/move into a workspace crate that matches the upstream package it came from.
7. Build `generate_text(...)` against a deterministic fake/test `LanguageModel` before adding real provider networking. Prove the high-level loop can accept prompts/settings, call a model, detect tool calls, execute typed Rust tools, append tool results, continue until final text or max steps, and return `GenerateTextResult`.
8. Add deterministic end-to-end tests for plain text generation, single tool call, multi-step tool call, tool error, unknown tool, invalid tool args, and max-step exhaustion.
9. Do not churn dependencies or CI unless the next SDK slice genuinely requires it.

Update notes.md with the slice completed, upstream facts learned, and the next likely surface to port.
PROMPT
)"

if [ "$#" -eq 0 ]; then
  set -- --current-branch --push
fi

exec "$script_dir/gnhf-codex-xhigh.sh" "$@" "$prompt"
