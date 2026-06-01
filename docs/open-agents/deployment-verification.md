# Open Agents Local E2E Verification

This guide captures the local verification shape for the Rust Open Agents Slack
remote-agent path. The repository currently has deterministic fixture coverage,
Slack ingress/router unit coverage, service health checks, local durable
resume/approval/error coverage, and an ignored live Slack smoke, plus a
no-credentials Slack emulator E2E lane for the fixture runtime path.

The current Rust slice extends the existing `open-agents-service` process with
configuration validation, health checks, graceful shutdown, state and sandbox
selection, local Slack event fixtures, a testable signed Slack HTTP route,
optional Slack Web API base URL override, durable run resume and block-action
approval handling, finish-action reporting, real sandbox/model error reporting,
a real Vercel AI Gateway runtime mode, deterministic Gateway async completion
coverage, a temporary safe `just-bash` virtual sandbox default for local bash
tool execution, and an ignored live Slack smoke test.

Keep this document honest as OA-01 lands the full upstream inventory: add exact
gate commands and mapped upstream test names there rather than guessing from
the local runtime closure lane.

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
- `apps/web/app/workflows/chat-post-finish.ts`
- `apps/web/app/workflows/chat-sandbox-runtime.ts`
- `packages/agent/tools/ask-user-question.ts`
- `packages/sandbox/interface.ts`

Relevant upstream constraints carried into this guide:

- chat requests start or resume durable workflow runs instead of executing the
  agent inline
- active workflow runs are resumed by stream id instead of duplicated
- the agent runs outside the sandbox and reaches the sandbox through tools
- sandbox lifecycle, git automation, and final commit/PR behavior are separate
  verification surfaces
- question prompts and approval requests resume the paused durable run instead
  of starting another run in the Slack thread
- model and sandbox failures are terminal run states that are reported to Slack
  and clear the active-run pointer

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
cargo test -p open-agents-service gateway_async_runner_waits_for_pending_generation_future
cargo test -p open-agents-service
```

`--check-config` supplies local fixture secrets if the shell has no Slack env
and points `OPEN_AGENTS_PLUGIN_ROOTS` at the checked-in minimal Open Plugin
fixture when the shell has not selected plugin roots. `--fixture` drives the
deterministic local Slack event harness. `--emulator` starts the local Slack
emulator and service together. `--matrix` prints the coverage table from this
guide for CI logs and local handoffs.

The cross-surface live and emulator command registry lives in
`docs/live-integration-proof-registry.md`.

## Runtime Selection

`OPEN_AGENTS_RUNTIME=auto` is the default. It selects `gateway` when
`AI_GATEWAY_API_KEY` or `AI_SDK_RUST_AI_GATEWAY_API_KEY` is present, otherwise
it selects the deterministic `fixture` runtime. Operators can force either path:

```sh
export OPEN_AGENTS_RUNTIME=gateway
export AI_GATEWAY_API_KEY=...
export AI_GATEWAY_MODEL=openai/gpt-4.1-mini
export OPEN_AGENTS_MODEL_MAX_STEPS=8
export OPEN_AGENTS_MODEL_MAX_OUTPUT_TOKENS=2048
export OPEN_AGENTS_TOOL_APPROVAL=sensitive
```

For repository-changing work that must push branches and open PRs, the sandbox
command environment also needs a GitHub token:

```sh
export OPEN_AGENTS_GITHUB_TOKEN=ghp_...
```

The service passes `OPEN_AGENTS_GITHUB_TOKEN`, `GITHUB_TOKEN`, or `GH_TOKEN`
through to sandbox commands as both `GITHUB_TOKEN` and `GH_TOKEN`. Do not enable
repository mutation in production until the selected sandbox backend is
disposable and the token has only the repository permissions required by the
target workflow.

Finish automation is disabled by default. Enable report-only or remote actions
with explicit env vars:

```sh
export OPEN_AGENTS_GIT_FINISH=report
export OPEN_AGENTS_GIT_FINISH_COMMIT_MESSAGE="chore: apply Open Agents changes"
export OPEN_AGENTS_GIT_FINISH_PUSH=dry-run
export OPEN_AGENTS_GIT_FINISH_PR=dry-run
export OPEN_AGENTS_GIT_FINISH_PR_BASE=main
export OPEN_AGENTS_GIT_FINISH_PR_REPOSITORY=owner/repo
```

`OPEN_AGENTS_GIT_FINISH_PUSH` and `OPEN_AGENTS_GIT_FINISH_PR` accept
`disabled`, `dry-run`, or `execute`. Finish-action errors are emitted as Slack
error messages after the completed run is persisted; they do not turn the
completed Slack callback into a failed HTTP response.

## Open Plugin Packages

The service can load Open Plugin v1 packages from operator-provided package
roots:

```sh
export OPEN_AGENTS_PLUGIN_ROOTS="/opt/open-agents/plugins/hello-plugin:/opt/open-agents/plugins/deploy-tools"
export OPEN_AGENTS_PLUGIN_DATA_DIR="/var/lib/open-agents/plugin-data"
cargo run -p open-agents-service --bin open-agents-slack -- --check-config
```

`OPEN_AGENTS_PLUGIN_ROOTS` uses the platform path-list separator (`:` on Unix,
`;` on Windows). Each root must contain `.plugin/plugin.json`. The loader
validates the plugin name, rejects manifest paths that do not start with `./` or
that traverse outside the plugin root, discovers default `skills/` directories
and `.mcp.json` files, and respects manifest-declared `skills` and
`mcpServers` paths. Plugin skills are surfaced to the runtime as
`{plugin-name}:{skill-name}` so they do not collide with project or global
skills. MCP configs are validated and `${PLUGIN_ROOT}` and, when configured,
`${PLUGIN_DATA}` are expanded, but the service does not start plugin MCP
subprocesses yet. The runtime exposes a sanitized MCP planning surface with
server names, source paths, command labels, env var names, and the recommended
`mcp__plugin_{plugin}_{server}__<tool>` prefix.

The local fixture is
`crates/open-agents-service/fixtures/open-plugin/minimal`. It contains
`.plugin/plugin.json`, `skills/greet/SKILL.md`, and `.mcp.json`. The
no-credential verification commands are:

```sh
cargo test -p open-agents-service from_reader_loads_open_plugin_fixture_components
cargo test -p open-agents-service local_runtime_exposes_open_plugin_components_without_starting_mcp
cargo test -p open-agents-runtime open_agent_prepare_composes_prompt_context_model_and_tools
scripts/open-agents-local-e2e.sh --check-config
```

## Fixture Path

The legacy fixture path does not require Slack credentials. It parses a
synthetic `app_mention`, records progress in memory, performs a deterministic
fake sandbox proof (`sandbox.exec pwd`), persists the run, and emits a final
Slack-thread message. The service-backed local Slack route uses the default
Just Bash virtual backend instead.

```sh
cargo run -p open-agents-service -- --fixture
```

Expected assertions:

- stdout includes progress and final outbound messages for the same Slack thread
- a run record is persisted under the in-memory fixture key
- the fake sandbox step list contains `sandbox.exec pwd`
- active run state is cleared after completion
- waiting runs resume from direct thread replies without starting duplicates
- question answer, cancel, and approval block actions resume the paused durable
  state
- scripted model and sandbox errors persist `failed` run status and notify Slack
- enabled finish actions emit commit/PR/no-change/error summaries without
  corrupting the terminal run state

Targeted deterministic tests:

```sh
cargo test -p open-agents-service
```

Those tests cover URL verification, app mention handling, waiting run resume by
answer action, cancel action, in-memory persistence, health readiness, config
validation, Gateway async Pending-to-completion behavior, and the ignored live
Slack probe gate.

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
ok: approval interaction payload resumed the run ...
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
- an approval prompt posts Approve/Deny controls and the direct approval payload
  resumes the same run
- run completion persists fixture state and clears active run state
- health probes stay green during event processing

Known emulator limits:

- Slash command and interaction callbacks are not simulated by the emulator
  today. The local E2E lane posts app mentions through Slack Web API state, then
  sends the matching `event_callback` to the service directly. The question
  and approval continuations post direct `block_actions` payloads to
  `/slack/interactions`.
- Socket Mode is not implemented by the emulator. Keep local E2E on Events API
  webhooks and leave Socket Mode for live smoke or unit-level config checks.
- Chat streaming is not implemented by the emulator. Assert Slack message/update
  state and persisted run state, not stream token timing.

## Local Slack App Run

Use memory state and the default Just Bash virtual sandbox while connecting a
real Slack app:

```sh
export SLACK_BOT_TOKEN=xoxb-...
export SLACK_SIGNING_SECRET=...
export OPEN_AGENTS_STATE=memory
export OPEN_AGENTS_SANDBOX=just-bash
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
- `OPEN_AGENTS_SANDBOX=just-bash`, the default safe in-process virtual
  filesystem backend for bash tool execution. The Open Agents smoke tests cover
  `echo`, `pwd`, `cat`, `printf`, `mkdir`, `ls`, `touch`, `cd`, `export`,
  `true`, `false`, simple env expansion, pipes, `;`, `&&`, and `>`/`>>`
  redirection through `crates/just-bash`. It does not execute host `/bin/bash`
  or arbitrary host processes.
- `OPEN_AGENTS_SANDBOX=local`, explicit host local sandbox boundary for
  development workflows that intentionally need host process execution
- `OPEN_AGENTS_SANDBOX_ROOT`, default `.`, used only with
  `OPEN_AGENTS_SANDBOX=local`

Optional production or live settings:

- `OPEN_AGENTS_STATE=postgres`
- `OPEN_AGENTS_STATE_URL` or `POSTGRES_URL`, required for Postgres mode
- `OPEN_AGENTS_SANDBOX=vercel`
- `OPEN_AGENTS_VERCEL_TOKEN`, `VERCEL_TOKEN`, or `VERCEL_OIDC_TOKEN`, required for live Vercel Sandbox use. Prefer `OPEN_AGENTS_VERCEL_TOKEN` in Vercel deployments.
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
- `OPEN_AGENTS_GIT_FINISH=disabled|report|true`, optional local sandbox git
  finish reporting
- `OPEN_AGENTS_GIT_FINISH_COMMIT_MESSAGE`, optional commit message for dirty
  sandbox repositories
- `OPEN_AGENTS_GIT_FINISH_PUSH=disabled|dry-run|execute`, optional push action
- `OPEN_AGENTS_GIT_FINISH_PR=disabled|dry-run|execute`, optional pull-request
  action
- `OPEN_AGENTS_GIT_FINISH_PR_BASE`, default `main`
- `OPEN_AGENTS_GIT_FINISH_PR_TITLE`, default `Open Agents changes`
- `OPEN_AGENTS_GIT_FINISH_PR_BODY`, default checked-in service text
- `OPEN_AGENTS_GIT_FINISH_PR_REPOSITORY`, optional `owner/repo` target
- `OPEN_AGENTS_PLUGIN_ROOTS`, optional platform path-list of Open Plugin package
  roots
- `OPEN_AGENTS_PLUGIN_DATA_DIR`, optional host-managed data root expanded into
  MCP config values as `${PLUGIN_DATA}`

Optional live smoke settings:

- `SLACK_TEST_CHANNEL_ID`
- `SLACK_TEST_USER_ID`, reserved for expanded live user-targeted checks

Ignored Vercel Sandbox smoke:

```sh
cargo test -p open-agents-sandbox live_vercel_sandbox_create_exec_read_write_list_stop_smoke -- --ignored --nocapture
```

The ignored smoke requires the live Vercel variables above and creates a real
temporary sandbox.

Ignored AI Gateway smoke:

```sh
export AI_GATEWAY_API_KEY=...
scripts/open-agents-local-e2e.sh --live-gateway

cargo test -p open-agents-service gateway_async_runner_waits_for_pending_generation_future
cargo test -p open-agents-service live_gateway_runtime_handles_app_mention_without_fixture_text -- --ignored --nocapture
```

The deterministic focused test does not need credentials and proves the service
Gateway runner waits for async completion instead of failing on an initial
Pending state. The wrapper also runs the lower-level `open-agents-runtime`
Gateway smoke. Live smoke tests are skipped unless `AI_GATEWAY_API_KEY` or
`AI_SDK_RUST_AI_GATEWAY_API_KEY` is configured.

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
- Approval buttons use encoded Open Agents action ids and carry the approval id
  in the Slack action value.

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
| App mention | Covered through the signed service route and emulator-backed local service path; Gateway mode has a credential-gated signed-event smoke | `scripts/open-agents-local-e2e.sh --emulator`; `cargo test -p open-agents-service app_mention_accepts_persists_run_and_records_outbound`; `cargo test -p open-agents-service slack_app_mention_routes_bash_tool_call_through_just_bash_without_vercel_credentials`; `cargo test -p open-agents-service live_gateway_runtime_handles_app_mention_without_fixture_text -- --ignored --nocapture` | Live deployed Slack app proof still TODO |
| DM | `open-agents-slack` covers DM routing | `cargo test -p open-agents-slack dm_event_starts_run_and_routes_as_dm_thread` | Emulator-backed DM scenario still TODO |
| Thread routing | Emulator-backed app mention thread replay plus parser/router tests; active waiting runs resume instead of duplicating | `scripts/open-agents-local-e2e.sh --emulator`; `cargo test -p open-agents-slack app_mention_threaded_reply_routes_to_parent_thread_ts`; `cargo test -p open-agents-service threaded_message_resumes_waiting_run_without_starting_duplicate` | Broader DM/thread matrix still TODO |
| Durable run completion | Covered locally through the service route with a scripted durable runtime; Gateway mode waits for async model completion and is credential-gated for live Gateway | `scripts/open-agents-local-e2e.sh --emulator`; `cargo test -p open-agents-service app_mention_accepts_persists_run_and_records_outbound`; `cargo test -p open-agents-service gateway_async_runner_waits_for_pending_generation_future`; `cargo test -p open-agents-service live_gateway_runtime_handles_app_mention_without_fixture_text -- --ignored --nocapture` | Live deployed Slack app proof still TODO |
| Waiting, answer, approval, cancel | Emulator-backed question and approval prompts plus direct signed block action payloads; service answer/approval/cancel tests | `scripts/open-agents-local-e2e.sh --emulator`; `cargo test -p open-agents-service block_action_answer_resumes_waiting_run_to_completion`; `cargo test -p open-agents-service block_action_approval_resumes_waiting_run_to_completion`; `cargo test -p open-agents-service block_action_cancel_cancels_waiting_run` | Emulator cannot simulate interactions today |
| Outbound Slack message/update | Emulator Web API state assertions for `chat.postMessage`; Slack outbound tests cover API body shapes | `scripts/open-agents-local-e2e.sh --emulator`; `cargo test -p open-agents-service app_mention_with_slack_api_url_posts_outbounds_to_slack_api`; `cargo test -p chat-sdk-adapter-slack slack_api_body_fixtures_cover_post_update_ephemeral_delete_reaction_and_typing` | Emulator-backed `chat.update` scenario still TODO |
| Open Plugin components | Config validation loads `.plugin/plugin.json`, namespaced fixture skills, and sanitized MCP server planning metadata without live Slack, Gateway, Vercel, or subprocess execution | `scripts/open-agents-local-e2e.sh --check-config`; `cargo test -p open-agents-service from_reader_loads_open_plugin_fixture_components`; `cargo test -p open-agents-service local_runtime_exposes_open_plugin_components_without_starting_mcp`; `cargo test -p open-agents-runtime open_agent_prepare_composes_prompt_context_model_and_tools` | Executable plugin MCP adapters remain a pending OP-03/runtime adapter seam |
| Persistence | In-memory service route run, active-run keys, waiting state, resume, and cancel are covered | `cargo test -p open-agents-service block_action_answer_resumes_waiting_run_to_completion` | Postgres-backed persistence still TODO |
| Sandbox command | Default local service route executes `bash`/`pwd` through the crate-backed Just Bash virtual backend; explicit local and Vercel backends remain selectable; Vercel backend has deterministic mocked create/exec/read/write/stat/list/stop coverage | `cargo test -p open-agents-service slack_app_mention_routes_bash_tool_call_through_just_bash_without_vercel_credentials`; `cargo test -p open-agents-sandbox just_bash`; `cargo test -p open-agents-sandbox vercel_sandbox_backend_connects_execs_reads_writes_lists_and_stops` | Live Vercel sandbox/git mutation proof remains credential-gated |
| Model/sandbox errors | Scripted local model and sandbox failures persist failed run status, clear active run state, and post Slack errors | `cargo test -p open-agents-service scripted_runtime_failure_is_persisted_and_reported_to_slack` | Live Gateway/Vercel failure proof remains credential-gated |
| Git automation summary | Finish actions can emit local git no-change/commit/PR/error summaries after a completed run; renderer coverage verifies Slack shapes | `cargo test -p open-agents-service finish_action_errors_are_reported_without_failing_finished_run`; `cargo test -p chat-sdk-adapter-slack renderers_cover_tool_plan_error_commit_and_pr_summaries` | Live push/PR execution remains credential-gated |
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
