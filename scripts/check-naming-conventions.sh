#!/usr/bin/env bash
set -euo pipefail

# Enforce the repo convention from scripts/codex-goal/port-ai-sdk.md and
# scripts/codex-goal-chat/port-chat-sdk.md: avoid vague bucket names in source
# paths, modules, crate names, public APIs, and documented identifiers.
#
# Explicit upstream-mirroring exceptions:
# - provider_utils and ai-sdk-provider-utils mirror the upstream
#   @ai-sdk/provider-utils package boundary.
# - util mirrors the existing upstream packages/ai utility surface already
#   exposed by this crate.
# - SharedV4ProviderReference is an upstream provider-v4 type name mentioned in
#   docs for the Rust ProviderReference wrapper.
# - adapter_shared / adapter-shared / chat-sdk-adapter-shared mirror the
#   upstream vercel/chat packages/adapter-shared package boundary.
# - adapter-utils, buffer-utils, card-utils mirror the upstream vercel/chat
#   packages/adapter-shared submodule filenames (adapter-utils.ts,
#   buffer-utils.ts, card-utils.ts).

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
if [[ "$target_dir" != /* ]]; then
  target_dir="$repo_root/$target_dir"
fi

checker="$target_dir/debug/repo-naming-check"
checker_manifest="$repo_root/crates/repo-naming-check/Cargo.toml"
root_manifest="$repo_root/Cargo.toml"

needs_build=false
if [[ "${CHECK_NAMING_FORCE_BUILD:-}" == "1" || ! -x "$checker" ]]; then
  needs_build=true
elif [[ "$checker_manifest" -nt "$checker" || "$root_manifest" -nt "$checker" ]]; then
  needs_build=true
elif [[ -n "$(find "$repo_root/crates/repo-naming-check/src" -type f -name '*.rs' -newer "$checker" -print -quit)" ]]; then
  needs_build=true
fi

if [[ "$needs_build" == "true" ]]; then
  cargo build --quiet -p repo-naming-check
fi

exec "$checker"
