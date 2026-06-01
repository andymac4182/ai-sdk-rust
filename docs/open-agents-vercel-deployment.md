# Open Agents Vercel Deployment

The deployable Vercel project is `open-agents-service`.

## Project Settings

- Root Directory: `crates/open-agents-service`
- Source Files Outside Root Directory: enabled
- SSO Deployment Protection: disabled for Slack webhook reachability

The deployment must be started from the repository root so Vercel uploads the
workspace crates used by the `open-agents-service` package:

```sh
vercel deploy --project open-agents-service
```

Use `--prod` only after the Slack credentials below are configured.

## Required Environment

Set these variables in Vercel before using the Slack routes:

- `SLACK_BOT_TOKEN`
- `SLACK_SIGNING_SECRET`
- `AI_GATEWAY_API_KEY`

Optional:

- `OPEN_AGENTS_SLACK_API_URL`
- `OPEN_AGENTS_STATE`
- `OPEN_AGENTS_SANDBOX`
- `OPEN_AGENTS_BIND_ADDR`

For Vercel Sandbox execution set:

- `OPEN_AGENTS_SANDBOX=vercel`
- `VERCEL_TOKEN` or `VERCEL_OIDC_TOKEN`
- `VERCEL_TEAM_ID`
- `VERCEL_PROJECT_ID`

Optional Vercel Sandbox settings:

- `VERCEL_SANDBOX_NAME`
- `VERCEL_SANDBOX_BASE_SNAPSHOT_ID`
- `VERCEL_SANDBOX_RUNTIME`
- `VERCEL_SANDBOX_VCPUS`
- `VERCEL_SANDBOX_TIMEOUT_MS`
- `VERCEL_SANDBOX_PERSISTENT`

## Routes

- `GET /api/healthz`
- `GET /api/readyz`
- `GET /api/status`
- `POST /api/slack/events`
- `POST /api/slack/interactions`
- `POST /api/slack/commands`

## Verification

After deployment, verify the Rust function is reachable:

```sh
curl -i "$DEPLOYMENT_URL/api/healthz"
```

Without Slack credentials the endpoint should return a plain-text
configuration error. With `SLACK_BOT_TOKEN` and `SLACK_SIGNING_SECRET` set, it
should return `200 OK`.

Run local service coverage before deploying:

```sh
cargo test -p open-agents-service
cargo clippy -p open-agents-service --all-targets -- -D warnings
scripts/open-agents-local-e2e.sh --all-local
```

Ignored live sandbox smoke:

```sh
OPEN_AGENTS_SANDBOX=vercel \
VERCEL_TOKEN=... \
VERCEL_TEAM_ID=... \
VERCEL_PROJECT_ID=... \
cargo test -p open-agents-sandbox live_vercel_sandbox_create_exec_read_write_list_stop_smoke -- --ignored --nocapture
```

See `docs/open-agents/vercel-sandbox-sdk.md` for the Rust SDK port inventory
and upstream TypeScript test mapping.
