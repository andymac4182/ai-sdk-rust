# Live Integration Proof Registry

This registry owns the credential-gated proof surface for CROSS-02 in
`docs/ts-to-rust-migration-tracker.md`. Normal local and CI checks must not
require Slack, Vercel, provider, Postgres, or Redis credentials. Live checks are
opt-in, ignored by default, and must either skip clearly when credentials are
missing or be run only through a documented live command.

Source snapshot for the Open Agents rows was refreshed on 2026-06-01 with:

```sh
npx opensrc fetch https://github.com/vercel-labs/open-agents
```

## Safe Compile And Local Gates

Use this lane to prove the registry without making live network calls:

```sh
cargo test -p chat-sdk-adapter-slack live_slack_outbound_post_update_react_ephemeral_typing_and_delete
cargo test -p open-agents-service live_slack_smoke_posts_probe_message_when_env_present
cargo test -p open-agents-service live_gateway_runtime_handles_app_mention_without_fixture_text
cargo test -p open-agents-runtime live_open_agent_gateway_generate_text_smoke
cargo test -p open-agents-sandbox live_vercel_sandbox_create_exec_read_write_list_stop_smoke
cargo test --all-features live_gateway_
cargo test --all-features live_vercel_ai_gateway_
scripts/open-agents-local-e2e.sh --emulator
```

The `cargo test ... live_*` commands intentionally omit `-- --ignored`. That
still compiles the ignored tests, but Rust reports them as ignored and does not
execute live calls. Only add `-- --ignored --nocapture` when you are deliberately
using real credentials.

## Slack Proofs

No-credentials local proofs:

```sh
scripts/open-agents-local-e2e.sh --check-config
scripts/open-agents-local-e2e.sh --fixture
scripts/open-agents-local-e2e.sh --emulator
cargo test -p open-agents-service
cargo test -p open-agents-service from_reader_loads_open_plugin_fixture_components
cargo test -p open-agents-service local_runtime_exposes_open_plugin_components_without_starting_mcp
cargo test -p open-agents-runtime open_agent_prepare_composes_prompt_context_model_and_tools
```

`scripts/open-agents-local-e2e.sh --check-config` defaults
`OPEN_AGENTS_PLUGIN_ROOTS` to
`crates/open-agents-service/fixtures/open-plugin/minimal` when the shell has not
selected plugin roots, so this no-credentials lane validates `.plugin/plugin.json`,
`skills/greet/SKILL.md`, and `.mcp.json` discovery without starting MCP
subprocesses.

Live outbound adapter proof:

```sh
SLACK_BOT_TOKEN=xoxb-... \
SLACK_SIGNING_SECRET=... \
SLACK_TEST_CHANNEL_ID=C... \
SLACK_TEST_USER_ID=U... \
cargo test -p chat-sdk-adapter-slack live_slack_outbound_post_update_react_ephemeral_typing_and_delete -- --ignored --nocapture
```

Expected behavior: posts a message, updates it, adds and removes a reaction,
sends an ephemeral message, sends typing, then deletes the probe message.

Skip semantics: the test is `#[ignore]`. If it is run with `-- --ignored`
without any required Slack variable, it prints which variable is missing and
returns successfully without touching Slack.

Live Open Agents Slack probe:

```sh
SLACK_BOT_TOKEN=xoxb-... \
SLACK_SIGNING_SECRET=... \
SLACK_TEST_CHANNEL_ID=C... \
cargo test -p open-agents-service live_slack_smoke_posts_probe_message_when_env_present -- --ignored --nocapture
```

Optional: set `OPEN_AGENTS_SLACK_API_URL` or `SLACK_API_URL` to point at a Slack
emulator-compatible Web API base URL.

Expected behavior: posts `Open Agents Rust service live smoke: fixture probe.`
to the configured channel.

Skip semantics: the test is `#[ignore]`. The helper returns `Ok(None)` and logs
a skip message if service config or `SLACK_TEST_CHANNEL_ID` is missing.

## Vercel AI Gateway Proofs

Shared credentials:

- `AI_GATEWAY_API_KEY` or `AI_SDK_RUST_AI_GATEWAY_API_KEY`
- `VERCEL_OIDC_TOKEN`, where the Gateway provider supports OIDC fallback
- `.env.local`, accepted by the root Gateway and Open Agents live smoke helpers

Common optional model overrides:

- `AI_GATEWAY_MODEL` or `AI_SDK_RUST_GATEWAY_MODEL`
- `AI_GATEWAY_EMBEDDING_MODEL` or `AI_SDK_RUST_GATEWAY_EMBEDDING_MODEL`
- `AI_GATEWAY_IMAGE_MODEL` or `AI_SDK_RUST_GATEWAY_IMAGE_MODEL`
- `AI_GATEWAY_RERANKING_MODEL` or `AI_SDK_RUST_GATEWAY_RERANKING_MODEL`
- `AI_GATEWAY_VIDEO_MODEL` or `AI_SDK_RUST_GATEWAY_VIDEO_MODEL`
- `AI_GATEWAY_OPENAI_COMPATIBLE_MODEL` or
  `AI_SDK_RUST_AI_GATEWAY_OPENAI_COMPATIBLE_MODEL`
- `AI_GATEWAY_OPENAI_RESPONSES_MODEL` or
  `AI_SDK_RUST_AI_GATEWAY_OPENAI_RESPONSES_MODEL`
- `OPEN_AGENTS_LIVE_GATEWAY_MODEL`, for the Open Agents runtime smoke

Root Gateway live smoke commands:

```sh
AI_GATEWAY_API_KEY=... cargo test --all-features live_gateway_openai_generate_text -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_gateway_openai_generate_object -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_gateway_openai_stream_text -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_gateway_openai_stream_object -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_gateway_openai_embed -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_gateway_openai_generate_image -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_gateway_rerank -- --ignored --nocapture
```

Expected behavior: each test makes one live Gateway call for the named modality
and asserts a marker string, non-empty embedding/image/ranking, or generated
object.

Vercel AI Gateway OpenAI-compatible and Responses commands:

```sh
AI_GATEWAY_API_KEY=... cargo test --all-features live_vercel_ai_gateway_openai_compatible_generate_text -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_vercel_ai_gateway_openai_compatible_stream_text -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_vercel_ai_gateway_openai_compatible_generate_object -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_vercel_ai_gateway_openai_compatible_stream_object -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_vercel_ai_gateway_openai_compatible_embed -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_vercel_ai_gateway_openai_compatible_generate_image -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_vercel_ai_gateway_openai_responses_generate_text -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_vercel_ai_gateway_openai_responses_stream_text -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_vercel_ai_gateway_openai_responses_generate_object -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test --all-features live_vercel_ai_gateway_openai_responses_stream_object -- --ignored --nocapture
```

Expected behavior: the OpenAI-compatible route and OpenAI Responses route each
prove live text, stream, object, stream-object, and selected non-language flows
against Gateway.

Telemetry live proofs:

```sh
scripts/check-otel-loopback.sh
AI_GATEWAY_API_KEY=... scripts/check-otel-loopback.sh --live-gateway
```

Expected behavior: the default command exports local dispatcher spans to the
loopback OTLP receiver without live credentials. `--live-gateway` additionally
runs ignored Gateway telemetry tests and asserts that the local receiver captures
Gateway span payloads.

Gateway provider crate account and media commands:

```sh
AI_GATEWAY_API_KEY=... cargo test -p ai-sdk-gateway live_gateway_available_models -- --ignored --nocapture
AI_GATEWAY_API_KEY=... cargo test -p ai-sdk-gateway live_gateway_generate_video -- --ignored --nocapture
```

Expected behavior: metadata includes at least one OpenAI model, and video
generation returns a non-empty video list.

Open Agents Gateway runtime smoke:

```sh
cargo test -p open-agents-service gateway_async_runner_waits_for_pending_generation_future
AI_GATEWAY_API_KEY=... scripts/open-agents-local-e2e.sh --live-gateway
AI_GATEWAY_API_KEY=... cargo test -p open-agents-service live_gateway_runtime_handles_app_mention_without_fixture_text -- --ignored --nocapture
```

Expected behavior: the no-credentials regression proves the service Gateway
runner waits across an async Pending state before completion. With credentials,
the Open Agent produces a non-fixture Gateway response through the signed Slack
event service path.

Skip semantics: all Gateway live tests are `#[ignore]`. When run with
`-- --ignored` and no key is found, tests log a `skipping live Gateway ...`
message and return. Because several helpers also read `.env.local`, do not use
`-- --ignored` on a machine with a credentialed `.env.local` unless the intent is
to make live calls.

## Vercel Sandbox Proof

Credentials:

- `OPEN_AGENTS_SANDBOX=vercel`, for the service config path
- `OPEN_AGENTS_VERCEL_TOKEN`, preferred for application code
- `VERCEL_TOKEN` or `VERCEL_OIDC_TOKEN`, accepted fallback token variables
- `VERCEL_TEAM_ID`
- `VERCEL_PROJECT_ID`

Optional settings:

- `VERCEL_SANDBOX_NAME`
- `VERCEL_SANDBOX_BASE_SNAPSHOT_ID`
- `VERCEL_SANDBOX_RUNTIME`
- `VERCEL_SANDBOX_VCPUS`
- `VERCEL_SANDBOX_TIMEOUT_MS`
- `VERCEL_SANDBOX_PERSISTENT`
- `VERCEL_SANDBOX_API_BASE_URL`

Command:

```sh
OPEN_AGENTS_SANDBOX=vercel \
OPEN_AGENTS_VERCEL_TOKEN=... \
VERCEL_TEAM_ID=... \
VERCEL_PROJECT_ID=... \
cargo test -p open-agents-sandbox live_vercel_sandbox_create_exec_read_write_list_stop_smoke -- --ignored --nocapture
```

Expected behavior: creates a temporary Vercel Sandbox, runs `pwd`, verifies `git`
is installed, writes and reads `open-agents-live.txt`, lists the workspace, and
stops the sandbox.

Skip semantics: the test is `#[ignore]`. If Vercel Sandbox config is missing, it
prints `missing Vercel sandbox credentials; skipping live smoke` and returns.

## Provider API Proofs

Provider packages use deterministic transport, fixture, and error-mapping tests
in normal runs. Direct-provider live tests must be `#[ignore]`, skip with a
clear missing-env message, and be added to this table before use.

Registered direct-provider safe compile commands:

```sh
cargo test -p ai-sdk-alibaba live_alibaba_chat_generate_text_smoke_when_env_present
cargo test -p ai-sdk-alibaba live_alibaba_video_generate_smoke_when_env_present
cargo test live_togetherai_image_and_rerank_validate_provider_contract --all-features
```

Alibaba live smoke commands:

```sh
ALIBABA_API_KEY=... cargo test -p ai-sdk-alibaba live_alibaba_chat_generate_text_smoke_when_env_present -- --ignored --nocapture
ALIBABA_API_KEY=... ALIBABA_LIVE_VIDEO=1 cargo test -p ai-sdk-alibaba live_alibaba_video_generate_smoke_when_env_present -- --ignored --nocapture
TOGETHER_API_KEY=... cargo test live_togetherai_image_and_rerank_validate_provider_contract --all-features -- --ignored --nocapture
```

Expected behavior: the Alibaba chat smoke makes one live DashScope-compatible
chat call and asserts non-empty text. The Alibaba video smoke submits one native
DashScope video task, polls until a video URL is returned, and is additionally
gated by `ALIBABA_LIVE_VIDEO=1` to avoid accidental media spend. The TogetherAI
smoke makes one image generation call and one reranking call, then asserts
parsed image/ranking outputs and response metadata.

Skip semantics: these tests are `#[ignore]`. If run with `-- --ignored` without
the required provider credential, they print a skip message and return. The
Alibaba video test also skips unless `ALIBABA_LIVE_VIDEO=1` is set. The
TogetherAI test accepts `TOGETHER_API_KEY` or deprecated `TOGETHER_AI_API_KEY`;
optional model overrides are `AI_SDK_RUST_TOGETHER_IMAGE_MODEL`/`TOGETHER_IMAGE_MODEL`
and `AI_SDK_RUST_TOGETHER_RERANKING_MODEL`/`TOGETHER_RERANKING_MODEL`.

Credential names already used by provider constructors:

| Provider surface | Environment variables |
| --- | --- |
| Alibaba Cloud DashScope | `ALIBABA_API_KEY` |
| OpenAI | `OPENAI_API_KEY` |
| Azure OpenAI | `AZURE_API_KEY`, `AZURE_RESOURCE_NAME` |
| Baseten | `BASETEN_API_KEY` |
| Black Forest Labs | `BFL_API_KEY` |
| ByteDance ModelArk | `ARK_API_KEY` |
| Cerebras | `CEREBRAS_API_KEY` |
| Deepgram | `DEEPGRAM_API_KEY` |
| DeepInfra | `DEEPINFRA_API_KEY` |
| DeepSeek | `DEEPSEEK_API_KEY` |
| Hugging Face | `HUGGINGFACE_API_KEY` |
| Hume | `HUME_API_KEY` |
| LMNT | `LMNT_API_KEY` |
| Luma | `LUMA_API_KEY` |
| Mistral | `MISTRAL_API_KEY` |
| Moonshot AI | `MOONSHOT_API_KEY` |
| Perplexity | `PERPLEXITY_API_KEY` |
| Rev.ai | `REVAI_API_KEY` |
| TogetherAI | `TOGETHER_API_KEY`, `TOGETHER_AI_API_KEY` |
| Vercel v0 | `VERCEL_API_KEY` |
| Voyage | `VOYAGE_API_KEY` |
| AssemblyAI | `ASSEMBLYAI_API_KEY` |

Default proof command for provider packages:

```sh
cargo test --all-features
```

Expected behavior today: deterministic provider tests pass without live provider
credentials. Future direct-provider live tests should make the narrowest single
provider call that proves auth, request shape, and response parsing for the row
being closed.

## Postgres And Redis Proofs

Open Agents Postgres service config:

- `OPEN_AGENTS_STATE=postgres`
- `OPEN_AGENTS_STATE_URL` or `POSTGRES_URL`

Chat SDK Postgres state package:

- The current Rust crate takes `PgStateAdapterOptions::new(database_url)`.
- Upstream env fallback rows name `POSTGRES_URL` and `DATABASE_URL`, but the
  Rust env-reader factory is not wired yet.

Chat SDK Redis state packages:

- `chat-sdk-state-redis` currently takes a Redis URL through
  `RedisStateAdapterOptions::with_url(...)`; the default is
  `redis://localhost:6379`.
- `chat-sdk-state-ioredis` currently takes cluster or Sentinel nodes through
  `IoredisStateAdapterOptions`; no environment-variable factory is wired yet.

No-credentials commands:

```sh
cargo test -p open-agents-service config_accepts_postgres_state_store
cargo test -p chat-sdk-state-pg
cargo test -p chat-sdk-state-redis
cargo test -p chat-sdk-state-ioredis
cargo test -p workflow-world-postgres
```

Expected behavior today: Postgres and Redis package tests validate config,
namespace, queue, migration metadata, token shape, and `NotConnected` placeholder
semantics without opening live database connections.

Skip semantics: there are currently no registered ignored live Postgres or Redis
tests. The upstream skipped live rows remain documented in `docs/chat/unported.md`
until real client wire-up lands. The first live database test must add exact env
vars, an ignored command, expected database-side behavior, and missing-env skip
text here.

## Open Agents Local E2E

Command:

```sh
scripts/open-agents-local-e2e.sh --emulator
```

Expected behavior: starts `emulate@0.6.0`, seeds a local Slack workspace, starts
`open-agents-service`, posts an app mention through the emulator Web API, sends a
signed Events API payload to the service, verifies DM routing, sends signed
answer and cancel interaction payloads directly to the service, and confirms
thread replies through `conversations.replies`.

Required credentials: none. The harness supplies local fixture values for
`SLACK_BOT_TOKEN`, `SLACK_SIGNING_SECRET`, `OPEN_AGENTS_SLACK_API_URL`, and
`OPEN_AGENTS_SANDBOX=just-bash`.

Expected output includes:

```text
ok: Slack emulator listening at http://localhost:...
ok: open-agents-service local E2E listening at http://127.0.0.1:...
ok: app mention completed in Slack thread ...
ok: DM message completed in Slack thread ...
ok: question prompt posted in Slack thread ...
```

Skip semantics: this is not ignored and should pass without live credentials.
If `scripts/open-agents-local-e2e/node_modules` is missing, the wrapper installs
the pinned local Node dependency before running the proof.
