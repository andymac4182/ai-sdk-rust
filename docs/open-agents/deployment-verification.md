# Open Agents Deployment Verification

This document captures the Rust deployment harness for the Open Agents Slack
remote-agent path. It follows the reviewed Open Agents source shape: Slack is
the chat surface, the agent run is durable, and sandbox execution remains a
separate boundary from the agent process.

The current Rust slice extends the existing `open-agents-service` process with
configuration validation, health checks, graceful shutdown, state and sandbox
selection, local Slack event fixtures, and an ignored live Slack smoke test.

## Source Rechecked

Re-run before this slice:

```sh
npx opensrc fetch https://github.com/vercel-labs/open-agents
```

Reviewed upstream files:

- `README.md`
- `apps/web/README.md`
- `apps/web/SANDBOX-LIFECYCLE.md`
- `docs/agents/architecture.md`
- `docs/release.md`

Relevant upstream constraints carried into the Rust harness:

- the agent is outside the sandbox and reaches it through tools
- runs must survive request lifecycles
- sandbox inactivity and hard-expiry behavior are operator concerns
- release packaging is application-oriented rather than a standalone CLI

## Local Fixture Run

The fixture path does not require Slack credentials. It drives a synthetic
`app_mention`, records progress, performs a fake sandbox command, persists the
run in memory state, and emits a final Slack-thread message.

```sh
cargo run -p open-agents-service -- --fixture
```

Targeted deterministic tests:

```sh
cargo test -p open-agents-service
```

## Local Slack-App Run

Use memory state and a local sandbox boundary while connecting a real Slack app:

```sh
export SLACK_BOT_TOKEN=xoxb-...
export SLACK_SIGNING_SECRET=...
export OPEN_AGENTS_STATE=memory
export OPEN_AGENTS_SANDBOX=local
export OPEN_AGENTS_SANDBOX_ROOT="$PWD"
export OPEN_AGENTS_BIND_ADDR=127.0.0.1:8080

cargo run -p open-agents-service -- --check-config
cargo run -p open-agents-service
```

Health probes:

```sh
curl -fsS http://127.0.0.1:8080/healthz
curl -fsS http://127.0.0.1:8080/readyz
curl -fsS http://127.0.0.1:8080/status
```

The HTTPS Slack callback route reserved for the runtime router is
`/slack/events`. Until the durable runtime integration lands, the fixture
harness exercises the same Slack parser directly in tests.

## Production Shape

Postgres is validated as the durable store selection now; the concrete
Postgres client remains gated by the state backend slice.

```sh
export SLACK_BOT_TOKEN=xoxb-...
export SLACK_SIGNING_SECRET=...
export OPEN_AGENTS_STATE=postgres
export OPEN_AGENTS_STATE_URL=postgres://...
export OPEN_AGENTS_SANDBOX=vercel
export OPEN_AGENTS_BIND_ADDR=0.0.0.0:8080
export AI_GATEWAY_API_KEY=...

cargo run -p open-agents-service
```

For Socket Mode:

```sh
export OPEN_AGENTS_SLACK_INGRESS=socket-mode
export SLACK_APP_TOKEN=xapp-...
```

The service fails fast when required Slack secrets are absent. Socket Mode also
requires `SLACK_APP_TOKEN`; Postgres mode requires `OPEN_AGENTS_STATE_URL` or
the upstream-compatible `POSTGRES_URL` alias.

## Slack App Configuration

OAuth bot token scopes:

- `app_mentions:read`
- `channels:history`
- `chat:write`
- `groups:history`
- `im:history`
- `im:read`
- `users:read`

Add `commands` if slash-command ingress is enabled. Add `reactions:write` once
reaction rendering is wired into the runtime.

Event subscriptions:

- `app_mention`
- `message.im`

Interactivity:

- Request URL: `https://YOUR_DOMAIN/slack/events`
- Enable block actions for answer/resume and cancel actions.
- The fixture action ids are `open_agents_answer` and `open_agents_cancel`.

Socket Mode:

- Enable Socket Mode in the Slack app.
- Create an app-level token with `connections:write`.
- Set `OPEN_AGENTS_SLACK_INGRESS=socket-mode` and `SLACK_APP_TOKEN`.

## Live Smoke

The ignored smoke posts a probe message through the Slack adapter when live
credentials are present:

```sh
export SLACK_BOT_TOKEN=xoxb-...
export SLACK_SIGNING_SECRET=...
export SLACK_TEST_CHANNEL_ID=C...

cargo test -p open-agents-service live_slack_smoke -- --ignored --nocapture
```

When the durable runtime crates land, extend this smoke to trigger a small agent
task through the Slack app, observe progress in the thread, and verify the
persisted run record.
