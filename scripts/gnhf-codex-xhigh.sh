#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"

if ! command -v gnhf >/dev/null 2>&1; then
  echo "error: gnhf is not installed. Install it with: npm install -g gnhf" >&2
  exit 127
fi

real_codex="${CODEX_BIN:-}"
if [ -z "$real_codex" ]; then
  real_codex="$(command -v codex || true)"
fi

if [ -z "$real_codex" ]; then
  echo "error: codex is not installed or not on PATH. Install it and run: codex login" >&2
  exit 127
fi

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/ai-sdk-rust-gnhf.XXXXXX")"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

real_codex_quoted="$(printf '%q' "$real_codex")"
cat >"$tmp_dir/codex" <<WRAPPER
#!/usr/bin/env bash
exec $real_codex_quoted -m gpt-5.5 -c 'model_reasoning_effort="xhigh"' "\$@"
WRAPPER
chmod +x "$tmp_dir/codex"

cd "$repo_root"
PATH="$tmp_dir:$PATH" exec gnhf --agent codex "$@"
