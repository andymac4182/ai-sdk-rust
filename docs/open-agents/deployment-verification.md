# Open Agents Local E2E Verification

This guide captures the local verification shape for the Rust Open Agents Slack
remote-agent path. The repository currently has deterministic fixture coverage,
Slack ingress/router unit coverage, service health checks, and an ignored live
Slack smoke, plus a no-credentials Slack emulator E2E lane for the fixture
runtime path.

The current Rust slice extends the existing `open-agents-service` process with
configuration validation, health checks, graceful shutdown, state and sandbox
selection, local Slack event fixtures, a testable signed Slack HTTP route,
optional Slack Web API base URL override, and an ignored live Slack smoke test.

Keep this document honest as later durable-runtime buckets land: replace
remaining TODOs with exact commands only after those branches wire production
model behavior, persistence, and sandbox behavior.

## Source Rechecked

Re-run before changing this slice:

```sh
npx opensrc fetch https://github.com/vercel-labs/open-agents
```

Reviewed upstream files:

- `README.md`
- `apps/web/app/api/chat/route.ts`
- `apps/web/app/api/chat/_lib/runtime.ts`
- `apps/web/app/workflows/chat.ts`

Relevant upstream constraints carried into this guide:

- chat requests start or resume durable workflow runs instead of executing the
  agent inline
- active workflow runs are resumed by stream id instead of duplicated
- the agent runs outside the sandbox and reaches the sandbox through tools
- sandbox lifecycle, git automation, and final commit/PR behavior are separate
  verification surfaces

Emulator reference: <https://emulate.dev/docs/slack>. The Slack emulator is a
stateful local Slack Web API with channels, messages, threads, reactions, user
profiles, presence, files, views, OAuth v2, and incoming webhooks. It preserves
rich chat fields such as `blocks`, `attachments`, `metadata`, formatting flags,
unfurl flags, and client message ids. Supported state changes dispatch
`event_callback` payloads to configured webhook URLs. Current limits that
matter for this project: no Socket Mode, slash command simulation, interaction
simulation, chat streaming, Slack Connect, or Enterprise Grid/admin surfaces.

## Quick Local Commands

The wrapper is safe to run without real Slack credentials:

```sh
scripts/open-agents-local-e2e.sh --help
scripts/open-agents-local-e2e.sh --matrix
scripts/open-agents-local-e2e.sh --check-config
scripts/open-agents-local-e2e.sh --fixture
scripts/open-agents-local-e2e.sh --emulator
cargo test -p open-agents-service
```

`--check-config` supplies local fixture secrets if the shell has no Slack env.
`--fixture` drives the deterministic local Slack event harness. `--emulator`
starts the local Slack emulator and service together. `--matrix` prints the
coverage table from this guide for CI logs and local handoffs.

## Fixture Path

The fixture path does not require Slack credentials. It parses a synthetic
`app_mention`, records progress in memory, performs fake sandbox proof
(`sandbox.exec pwd`), persists the run, and emits a final Slack-thread message.

```sh
cargo run -p open-agents-service -- --fixture
```

Expected assertions:

- stdout includes progress and final outbound messages for the same Slack thread
- a run record is persisted under the in-memory fixture key
- the fake sandbox step list contains `sandbox.exec pwd`
- active run state is cleared after completion

Targeted deterministic tests:

```sh
cargo test -p open-agents-service
```

Those tests cover URL verification, app mention handling, waiting run resume by
answer action, cancel action, in-memory persistence, health readiness, config
validation, and the ignored live Slack probe gate.

## Emulator-Backed Path

The emulator path does not require real Slack credentials. It starts
`emulate@0.6.0` programmatically, seeds a deterministic local Slack workspace,
starts `open-agents-service`, posts an app mention through the emulated Slack
Web API, sends the corresponding signed Slack `event_callback` payload to the
local service route, and verifies the fixture run output through
`conversations.replies`.

Install the pinned local Node dependency once:

```sh
npm install --prefix scripts/open-agents-local-e2e
```

Run the local E2E proof:

```sh
npm test --prefix scripts/open-agents-local-e2e
```

The wrapper exposes the same proof:

```sh
scripts/open-agents-local-e2e.sh --emulator
```

Expected output includes:

```text
ok: Slack emulator listening at http://localhost:...
ok: open-agents-service local E2E listening at http://127.0.0.1:...
ok: app mention completed in Slack thread ...
ok: question prompt posted in Slack thread ...
ok: direct interaction payload resumed the run ...
```

The harness seeds:

- team `T123`
- user `U000000001`
- channel `C000000001`
- app `AOPENAGENT`
- bot user `UOPENAGENT`
- bot token `xoxb-open-agents-local`
- signing secret `open-agents-local-signing-secret`

The service targets the emulator through:

```sh
OPEN_AGENTS_SLACK_API_URL=http://127.0.0.1:4003/api
```

`SLACK_API_URL` is accepted as a compatibility alias. The emulator stores
stateful Slack messages and threads, so the verification reads the same thread
history a developer can inspect in the emulator UI.

Expected emulator assertions:

- URL verification returns the Slack challenge with HTTP 200
- `app_mention` reaches the local service and starts a fixture run
- outbound Slack `chat.postMessage` creates a thread reply in emulator state
- the completed run text appears in `conversations.replies`
- a waiting/question run posts a prompt into the same thread
- a direct `block_actions` payload to `/slack/interactions` resumes the run
- run completion persists fixture state and clears active run state
- health probes stay green during event processing

Known emulator limits:

- Slash command and interaction callbacks are not simulated by the emulator
  today. The local E2E lane posts app mentions through Slack Web API state, then
  sends the matching `event_callback` to the service directly. The question
  continuation posts a direct `block_actions` payload to `/slack/interactions`.
- Socket Mode is not implemented by the emulator. Keep local E2E on Events API
  webhooks and leave Socket Mode for live smoke or unit-level config checks.
- Chat streaming is not implemented by the emulator. Assert Slack message/update
  state and persisted run state, not stream token timing.

## Local Slack App Run

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

The local HTTP service now routes signed Slack callbacks through the same
ingress/router boundary used by the binary:

- Events API request URL: `http://127.0.0.1:8080/slack/events`
- Interactivity request URL: `http://127.0.0.1:8080/slack/interactions`
- Slash commands, when enabled: `http://127.0.0.1:8080/slack/commands`

The deterministic service tests POST signed Slack payloads through an ephemeral
listener and assert URL verification, persisted runs, durable waiting/resume
state, cancellation, and captured Slack outbound messages.

## Environment Variables

Required for the deployable service:

- `SLACK_BOT_TOKEN`
- `SLACK_SIGNING_SECRET`

Common local settings:

- `OPEN_AGENTS_BIND_ADDR`, default `127.0.0.1:8080`
- `OPEN_AGENTS_STATE=memory`, the current deterministic local state backend
- `OPEN_AGENTS_SANDBOX=local`, the current local sandbox boundary
- `OPEN_AGENTS_SANDBOX_ROOT`, default `.`

Optional production or live settings:

- `OPEN_AGENTS_STATE=postgres`
- `OPEN_AGENTS_STATE_URL` or `POSTGRES_URL`, required for Postgres mode
- `OPEN_AGENTS_SANDBOX=vercel`
- `VERCEL_TOKEN` or `VERCEL_OIDC_TOKEN`, required for live Vercel Sandbox use
- `VERCEL_TEAM_ID`, required for live Vercel Sandbox use
- `VERCEL_PROJECT_ID`, required for live Vercel Sandbox use
- `VERCEL_SANDBOX_NAME`, optional named sandbox to resume
- `VERCEL_SANDBOX_BASE_SNAPSHOT_ID`, optional Vercel sandbox base snapshot
- `VERCEL_SANDBOX_RUNTIME`, optional Vercel runtime such as `node24`
- `VERCEL_SANDBOX_VCPUS`, optional Vercel sandbox vCPU count
- `VERCEL_SANDBOX_TIMEOUT_MS`, optional Vercel sandbox timeout
- `VERCEL_SANDBOX_PERSISTENT`, optional named-sandbox persistence flag
- `AI_GATEWAY_API_KEY` or `AI_SDK_RUST_AI_GATEWAY_API_KEY`, model credential
- `OPEN_AGENTS_SLACK_API_URL` or `SLACK_API_URL`, optional Slack Web API base
  URL override for local emulators
- `OPEN_AGENTS_SLACK_INGRESS=socket-mode`
- `SLACK_APP_TOKEN`, required for Socket Mode

Optional live smoke settings:

- `SLACK_TEST_CHANNEL_ID`
- `SLACK_TEST_USER_ID`, reserved for expanded live user-targeted checks

Ignored Vercel Sandbox smoke:

```sh
cargo test -p open-agents-sandbox live_vercel_sandbox_create_exec_read_write_list_stop_smoke -- --ignored --nocapture
```

The ignored smoke requires the live Vercel variables above and creates a real
temporary sandbox.

## Slack App Configuration

OAuth bot token scopes:

- `app_mentions:read`
- `channels:history`
- `chat:write`
- `groups:history`
- `im:history`
- `im:read`
- `users:read`

Add `commands` only when slash-command ingress is enabled. Add
`reactions:write` once reaction rendering is wired into the runtime.

Event subscriptions:

- `app_mention`
- `message.im`

Interactivity:

- Request URL: `https://YOUR_DOMAIN/slack/interactions`
- Enable block actions for answer/resume and cancel actions.
- The fixture action ids are `open_agents_answer` and `open_agents_cancel`.

Socket Mode:

- Enable Socket Mode in the Slack app.
- Create an app-level token with `connections:write`.
- Set `OPEN_AGENTS_SLACK_INGRESS=socket-mode` and `SLACK_APP_TOKEN`.

## Optional Live Slack Smoke

The ignored smoke posts a probe message through the Slack adapter when live
credentials are present:

```sh
export SLACK_BOT_TOKEN=xoxb-...
export SLACK_SIGNING_SECRET=...
export SLACK_TEST_CHANNEL_ID=C...

cargo test -p open-agents-service live_slack_smoke -- --ignored --nocapture
```

The deterministic local route tests already trigger a small scripted agent task
through the service boundary. Extend the ignored live smoke to drive the same
path through a real Slack app once live outbound assertions are available.

## Verification Matrix

| Flow | Local coverage today | Command or evidence | Gap before durable runtime E2E |
| --- | --- | --- | --- |
| URL verification | Covered through the service HTTP route and Slack ingress unit tests | `cargo test -p open-agents-service slack_events_url_verification_traverses_service_http_route`; `cargo test -p open-agents-slack events_api_url_verification_returns_challenge` | Live Slack app proof still TODO |
| App mention | Covered through the signed service route and emulator-backed local service path | `scripts/open-agents-local-e2e.sh --emulator`; `cargo test -p open-agents-service app_mention_accepts_persists_run_and_records_outbound` | Live model runtime proof still TODO |
| DM | `open-agents-slack` covers DM routing | `cargo test -p open-agents-slack dm_event_starts_run_and_routes_as_dm_thread` | Emulator-backed DM scenario still TODO |
| Thread routing | Emulator-backed app mention thread replay plus parser/router tests | `scripts/open-agents-local-e2e.sh --emulator`; `cargo test -p open-agents-slack app_mention_threaded_reply_routes_to_parent_thread_ts`; `cargo test -p open-agents-service block_action_answer_resumes_waiting_run_to_completion` | Broader DM/thread matrix still TODO |
| Durable run completion | Covered locally through the service route with a scripted durable runtime | `scripts/open-agents-local-e2e.sh --emulator`; `cargo test -p open-agents-service app_mention_accepts_persists_run_and_records_outbound` | Live model runtime proof still TODO |
| Waiting, answer, cancel | Emulator-backed question prompt plus direct signed block action payload; service cancel test | `scripts/open-agents-local-e2e.sh --emulator`; `cargo test -p open-agents-service block_action_answer_resumes_waiting_run_to_completion`; `cargo test -p open-agents-service block_action_cancel_cancels_waiting_run` | Emulator cannot simulate interactions today |
| Outbound Slack message/update | Emulator Web API state assertions for `chat.postMessage`; Slack outbound tests cover API body shapes | `scripts/open-agents-local-e2e.sh --emulator`; `cargo test -p open-agents-service app_mention_with_slack_api_url_posts_outbounds_to_slack_api`; `cargo test -p chat-sdk-adapter-slack slack_api_body_fixtures_cover_post_update_ephemeral_delete_reaction_and_typing` | Emulator-backed `chat.update` scenario still TODO |
| Persistence | In-memory service route run, active-run keys, waiting state, resume, and cancel are covered | `cargo test -p open-agents-service block_action_answer_resumes_waiting_run_to_completion` | Postgres-backed persistence still TODO |
| Sandbox command | Local service route executes `sandbox.exec pwd`; Vercel backend has deterministic mocked create/exec/read/write/stat/list/stop coverage | `cargo test -p open-agents-service app_mention_accepts_persists_run_and_records_outbound`; `cargo test -p open-agents-sandbox vercel_sandbox_backend_connects_execs_reads_writes_lists_and_stops` | Live Vercel proof is ignored and credential-gated |
| Git automation summary | Unit renderer coverage only | `cargo test -p chat-sdk-adapter-slack renderers_cover_tool_plan_error_commit_and_pr_summaries` | End-to-end auto-commit/PR summary from a run still TODO |
| Health/readiness | Covered by service tests, manual probes, and emulator readiness polling | `scripts/open-agents-local-e2e.sh --emulator`; `cargo test -p open-agents-service healthz_and_readyz_reflect_liveness_and_readiness`; `curl -fsS /healthz /readyz /status` | Live deployment probes still TODO |

## CI Shape

Minimal local/CI lane that requires no real Slack credentials:

```sh
scripts/open-agents-local-e2e.sh --matrix
scripts/open-agents-local-e2e.sh --check-config
scripts/open-agents-local-e2e.sh --fixture
scripts/open-agents-local-e2e.sh --emulator
cargo test -p open-agents-service
```

Expanded local lane:

```sh
scripts/open-agents-local-e2e.sh --all-local
```
