#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

step_count=0

log() {
  printf '[master-parity-gate] %s\n' "$*"
}

fail() {
  log "FAIL: $*"
  exit 1
}

require_file() {
  local path="$1"
  if [ ! -f "$path" ]; then
    fail "missing required ledger or generated file: $path"
  fi
}

require_command() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    fail "missing required command: $command_name"
  fi
}

run_step() {
  local description="$1"
  shift
  step_count=$((step_count + 1))
  log "RUN ${step_count}: ${description}"
  "$@"
  log "PASS ${step_count}: ${description}"
}

check_generated_progress() {
  local name="$1"
  local ledger="$2"
  local estimates="$3"
  local generated="$4"
  local title="$5"
  local temp_output="$tmp_dir/${name}-package-progress.md"

  require_file "$ledger"
  require_file "$estimates"
  require_file "$generated"

  run_step \
    "regenerate ${name} package progress" \
    scripts/package-progress-table.sh \
      --ledger "$ledger" \
      --estimates "$estimates" \
      --output "$temp_output" \
      --title "$title"

  if ! diff -u "$generated" "$temp_output"; then
    fail "${generated} is stale; regenerate it with scripts/package-progress-table.sh"
  fi
  log "PASS: ${generated} matches regenerated ${name} package progress"
}

check_open_agents_gate() {
  local ledger="docs/open-agents/upstream-parity.md"
  local gate=""
  local candidate
  local gate_candidates=(
    "scripts/open-agents-parity-check.mjs"
    "scripts/open-agents-test-inventory.mjs"
    "scripts/open-agents-upstream-inventory.mjs"
    "scripts/open-agents-parity-gate.mjs"
  )

  for candidate in "${gate_candidates[@]}"; do
    if [ -f "$candidate" ]; then
      gate="$candidate"
      break
    fi
  done

  if [ -z "$gate" ] && [ ! -f "$ledger" ]; then
    log "SKIP: Open Agents parity gate unavailable; OA-01 has not landed (${ledger} and ${gate_candidates[0]} are absent)"
    return 0
  fi

  if [ -z "$gate" ]; then
    fail "Open Agents ledger exists but no Open Agents parity gate script was found"
  fi
  if [ ! -f "$ledger" ]; then
    fail "Open Agents parity gate exists but ${ledger} is missing"
  fi

  case "$gate" in
    *.mjs)
      run_step "Open Agents parity gate" node "$gate" --check
      ;;
    *.sh)
      run_step "Open Agents parity gate" "$gate"
      ;;
    *)
      fail "unsupported Open Agents parity gate script type: $gate"
      ;;
  esac
}

check_just_bash_gate() {
  require_file scripts/just-bash-test-inventory.mjs
  require_file docs/open-agents/just-bash-parity.md
  require_file docs/open-agents/just-bash-conformance.md

  local strict_gate="${JUST_BASH_STRICT_GATE:-0}"
  case "$strict_gate" in
    1|true|TRUE|yes|YES)
      run_step "Just Bash strict conformance gate" node scripts/just-bash-test-inventory.mjs --strict
      ;;
    0|false|FALSE|no|NO|"")
      run_step "Just Bash inventory conformance gate" node scripts/just-bash-test-inventory.mjs --check
      ;;
    *)
      fail "JUST_BASH_STRICT_GATE must be 0/1, true/false, or yes/no"
      ;;
  esac
}

usage() {
  cat <<'USAGE'
Usage: scripts/master-parity-gate.sh [--check]

Runs the CI-safe parity gate without live credentials:
  - verifies required AI SDK, Chat SDK, and Workflow ledger/generated files exist
  - runs the AI SDK strict upstream test inventory drift check
  - regenerates AI SDK and Chat SDK package progress into temp files and diffs them
  - runs the Chat strict generated-inventory drift check
  - runs the Workflow generated-inventory drift check and parity gate
  - runs the Open Agents parity gate when OA-01 has landed, otherwise reports a skip
  - runs the Just Bash conformance gate in non-strict inventory mode by default
  - runs the Open Plugin Spec conformance gate
  - runs git diff --check for whitespace/status drift

Set JUST_BASH_STRICT_GATE=1 after JBC-08 closes all portable rows to make the
Just Bash step run node scripts/just-bash-test-inventory.mjs --strict.
USAGE
}

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
  usage
  exit 0
fi

if [ "${1:-}" = "--check" ]; then
  shift
fi

if [ "$#" -ne 0 ]; then
  usage >&2
  fail "unknown argument: $1"
fi

require_command diff
require_command git
require_command node
require_command ruby

require_file scripts/package-progress-table.sh
require_file scripts/ai-strict-test-inventory.mjs
require_file docs/ai-strict-test-inventory.md
require_file scripts/workflow-test-inventory.mjs
require_file scripts/workflow-parity-check.mjs
require_file scripts/chat-test-inventory.mjs
require_file docs/chat/test-inventory.md
require_file scripts/open-plugin-spec-gate.mjs
require_file scripts/just-bash-test-inventory.mjs
require_file docs/workflow-test-inventory.md
require_file docs/workflow-upstream-parity.md
require_file docs/open-agents/open-plugin-spec.md
require_file docs/open-agents/just-bash-parity.md
require_file docs/open-agents/just-bash-conformance.md

run_step "AI SDK strict test inventory drift check" node scripts/ai-strict-test-inventory.mjs --check

check_generated_progress \
  "ai-sdk" \
  "docs/upstream-parity.md" \
  "docs/package-progress-estimates.tsv" \
  "docs/package-progress.md" \
  "AI SDK Rust Package Progress"

check_generated_progress \
  "chat-sdk" \
  "docs/chat/upstream-parity.md" \
  "docs/chat/package-progress-estimates.tsv" \
  "docs/chat/package-progress.md" \
  "Chat SDK Rust Package Progress"

run_step "Chat strict test inventory drift check" node scripts/chat-test-inventory.mjs --check
run_step "Workflow generated inventory drift check" node scripts/workflow-test-inventory.mjs --check
run_step "Workflow parity gate" node scripts/workflow-parity-check.mjs
check_open_agents_gate
check_just_bash_gate
run_step "Open Plugin Spec conformance gate" node scripts/open-plugin-spec-gate.mjs --check
run_step "whitespace drift check" git diff --check

log "PASS: master parity gate completed without live credentials"
