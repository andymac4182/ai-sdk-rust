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
  --test           Run cargo test -p open-agents-service.
  --all-local      Run --check-config, --fixture, and --test.
USAGE
}

print_matrix() {
  cat <<'MATRIX'
| Flow | Local coverage today | Gap before full local E2E |
| --- | --- | --- |
| URL verification | Fixture and Slack ingress unit tests | Service binary route at /slack/events still TODO |
| App mention and DM | App mention fixture; app mention and DM router tests | Emulator event dispatch to running service still TODO |
| Thread routing | Parser/router tests and fixture interaction tests | Emulator-backed thread replay still TODO |
| Durable run completion | Fixture fake run completes and clears active state | Real durable runtime through service route still TODO |
| Waiting, answer, cancel | Synthetic block action fixture payloads | Emulator cannot simulate interactions today |
| Outbound Slack message/update | Fixture captures outbound; Slack outbound body tests cover post/update shapes | Emulator Web API state assertions still TODO |
| Persistence | In-memory fixture run and active-run keys | Postgres-backed persistence still TODO |
| Sandbox command | Fixture records fake sandbox.exec pwd proof | Real local sandbox command through runtime still TODO |
| Git automation summary | Slack renderer unit coverage only | End-to-end auto-commit/PR summary still TODO |
| Health/readiness | Service health/readiness tests and manual probes | Include probes in emulator E2E once service route lands |
MATRIX
}

print_plan() {
  cat <<'PLAN'
scripts/open-agents-local-e2e.sh --matrix
scripts/open-agents-local-e2e.sh --check-config
scripts/open-agents-local-e2e.sh --fixture
cargo test -p open-agents-service

Future emulator-backed lane after service-route and emulator-harness buckets:
npx emulate --service slack --seed docs/open-agents/emulate-slack.seed.yaml
# start open-agents-service with the emulator Slack API base URL
# drive app mention, DM, thread reply, and outbound update assertions
PLAN
}

run_check_config() {
  (
    cd "$repo_root"
    SLACK_BOT_TOKEN="${SLACK_BOT_TOKEN:-xoxb-local-test}" \
      SLACK_SIGNING_SECRET="${SLACK_SIGNING_SECRET:-open-agents-local-signing-secret}" \
      OPEN_AGENTS_STATE="${OPEN_AGENTS_STATE:-memory}" \
      OPEN_AGENTS_SANDBOX="${OPEN_AGENTS_SANDBOX:-local}" \
      OPEN_AGENTS_SANDBOX_ROOT="${OPEN_AGENTS_SANDBOX_ROOT:-$repo_root}" \
      cargo run -p open-agents-service -- --check-config
  )
}

run_fixture() {
  (
    cd "$repo_root"
    cargo run -p open-agents-service -- --fixture
  )
}

run_tests() {
  (
    cd "$repo_root"
    cargo test -p open-agents-service
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
  --test)
    run_tests
    ;;
  --all-local)
    run_check_config
    run_fixture
    run_tests
    ;;
  *)
    echo "unknown command: $command" >&2
    usage >&2
    exit 2
    ;;
esac
