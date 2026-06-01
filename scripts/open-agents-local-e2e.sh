#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"

usage() {
  cat <<'USAGE'
Usage: scripts/open-agents-local-e2e.sh <command>

Safe local Open Agents verification commands. No command requires real Slack
credentials unless you export them yourself.

Commands:
  --help, -h       Show this help.
  --matrix         Print the local E2E verification matrix.
  --plan           Print the no-credentials local verification command plan.
  --dry-run        Alias for --plan.
  --check-config   Run service config validation with local fixture secrets.
  --fixture        Run the deterministic open-agents-service fixture.
  --emulator       Run the Slack emulator-backed local E2E proof.
  --test           Run cargo test -p open-agents-service.
  --live-gateway   Run the ignored Open Agents live AI Gateway smoke.
  --all-local      Run --check-config, --fixture, --emulator, and --test.
USAGE
}

print_matrix() {
  cat <<'MATRIX'
| Flow | Local coverage today | Gap before durable runtime E2E |
| --- | --- | --- |
| URL verification | Signed service route and Slack ingress unit tests | Live Slack app proof still TODO |
| App mention | Emulator-backed signed service path and service route tests | Live model runtime proof still TODO |
| DM | Emulator-backed DM event plus Slack router tests | Live Slack DM proof still TODO |
| Thread routing | Emulator-backed app mention and DM thread replay; active waiting runs resume instead of duplicating | Broader thread mutation matrix still TODO |
| Durable run completion | Service route scripted runtime completes and clears active state; ignored live Gateway OpenAgent smoke available | Deployed live Slack + Gateway proof still TODO |
| Waiting, answer, approval, cancel | Emulator-backed question/approval prompts plus direct signed answer/approval/cancel block action payloads | Live Slack interaction proof still TODO |
| Outbound Slack message/update | Emulator Web API state assertions for postMessage; Slack outbound body tests cover post/update shapes | Emulator-backed chat.update scenario still TODO |
| Persistence | In-memory service route run and active-run keys | Postgres-backed persistence still TODO |
| Sandbox command | Service route records bash pwd proof through the crate-backed Just Bash virtual backend | Live Vercel sandbox and git automation proof still TODO |
| Model/sandbox errors | Scripted local model and sandbox failures persist failed run status and notify Slack | Live Gateway/Vercel failure proof still TODO |
| Git automation summary | Finish actions can emit local git no-change/commit/PR/error summaries after completed runs | Live push/PR execution proof still TODO |
| Health/readiness | Service health/readiness tests, manual probes, and emulator readiness polling | Live deployment probes still TODO |
| Live model | Ignored OpenAgent + Vercel AI Gateway smoke via --live-gateway | Requires AI_GATEWAY_API_KEY or AI_SDK_RUST_AI_GATEWAY_API_KEY |
MATRIX
}

print_plan() {
  cat <<'PLAN'
scripts/open-agents-local-e2e.sh --matrix
scripts/open-agents-local-e2e.sh --check-config
scripts/open-agents-local-e2e.sh --fixture
scripts/open-agents-local-e2e.sh --emulator
cargo test -p open-agents-service

The emulator lane starts emulate@0.6.0 programmatically, starts
open-agents-service, posts an app mention through the emulator Web API, sends a
signed Events API payload to the service route, verifies DM routing, verifies
direct signed answer/approval/cancel interaction payloads, and confirms replies
through conversations.replies.

Credential-gated live model smoke:
AI_GATEWAY_API_KEY=... scripts/open-agents-local-e2e.sh --live-gateway
PLAN
}

run_check_config() {
  (
    cd "$repo_root"
    SLACK_BOT_TOKEN="${SLACK_BOT_TOKEN:-xoxb-local-test}" \
      SLACK_SIGNING_SECRET="${SLACK_SIGNING_SECRET:-open-agents-local-signing-secret}" \
      OPEN_AGENTS_STATE="${OPEN_AGENTS_STATE:-memory}" \
      OPEN_AGENTS_SANDBOX="${OPEN_AGENTS_SANDBOX:-just-bash}" \
      OPEN_AGENTS_PLUGIN_ROOTS="${OPEN_AGENTS_PLUGIN_ROOTS:-$repo_root/crates/open-agents-service/fixtures/open-plugin/minimal}" \
      OPEN_AGENTS_PLUGIN_DATA_DIR="${OPEN_AGENTS_PLUGIN_DATA_DIR:-$repo_root/target/open-agents-plugin-data}" \
      cargo run -p open-agents-service --bin open-agents-slack -- --check-config
  )
}

run_fixture() {
  (
    cd "$repo_root"
    cargo run -p open-agents-service --bin open-agents-slack -- --fixture
  )
}

run_tests() {
  (
    cd "$repo_root"
    cargo test -p open-agents-service
  )
}

run_live_gateway() {
  (
    cd "$repo_root"
    if [[ -f .env.local ]]; then
      set -a
      source .env.local
      set +a
    fi
    cargo test -p open-agents-runtime live_open_agent_gateway_generate_text_smoke -- --ignored --nocapture
    cargo test -p open-agents-service live_gateway_runtime_handles_app_mention_without_fixture_text -- --ignored --nocapture
  )
}

run_emulator() {
  (
    cd "$repo_root"
    if [[ ! -d scripts/open-agents-local-e2e/node_modules ]]; then
      npm install --prefix scripts/open-agents-local-e2e
    fi
    npm test --prefix scripts/open-agents-local-e2e
  )
}

command="${1:---help}"
case "$command" in
  --help | -h)
    usage
    ;;
  --matrix)
    print_matrix
    ;;
  --plan | --dry-run)
    print_plan
    ;;
  --check-config)
    run_check_config
    ;;
  --fixture)
    run_fixture
    ;;
  --emulator)
    run_emulator
    ;;
  --test)
    run_tests
    ;;
  --live-gateway)
    run_live_gateway
    ;;
  --all-local)
    run_check_config
    run_fixture
    run_emulator
    run_tests
    ;;
  *)
    echo "unknown command: $command" >&2
    usage >&2
    exit 2
    ;;
esac
