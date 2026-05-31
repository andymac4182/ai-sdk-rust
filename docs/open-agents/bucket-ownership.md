# Open Agents Bucket Ownership

Each bucket must keep changes inside its owner crates unless it updates
`docs/open-agents/slack-remote-agent-architecture.md` with a reviewed reason.

| Bucket | Owner target | Required modules or APIs | Depends on | Verification path |
| --- | --- | --- | --- | --- |
| 01 Source inventory and architecture | `docs/open-agents/*`, `open-agents-core`, crate skeletons | Architecture contract, source verification, ownership table, source constants | Current Rust and Open Agents source | `cargo metadata --format-version 1`; `scripts/check-naming-conventions.sh`; manual source mapping proof |
| 02 Remote agent core runtime | `open-agents-runtime` | `run_loop`, `agent`, `model`, `events`, `cancellation` | 01, `ai-sdk-rust`, `ai-sdk-workflow` | Unit tests for run status transitions, stream event ordering, cancellation, and max-step behavior |
| 03 Sandbox execution boundary | `open-agents-sandbox` | `state`, `connect`, `files`, `shell`, `ports`, `snapshots`, `timeout` | 01 | Unit tests for state serialization and connector error mapping; ignored provider proof |
| 04 Agent tool surface | `open-agents-runtime::tools`, `open-agents-sandbox` | `read`, `write`, `edit`, `grep`, `glob`, `bash`, `todo`, `ask_user_question`, `web_fetch` | 02, 03 | Rust counterparts for Open Agents tool tests; approval tests for dotenv and dangerous commands |
| 05 Subagents and context management | `open-agents-runtime::subagents` | `registry`, `explorer`, `executor`, `design`, `context_management` | 02, 04 | Tests for registry names, delegated stream summaries, context cache-control equivalents, and usage folding |
| 06 Skills discovery and invocation | `open-agents-sandbox::skills`, `open-agents-runtime::skills` | `discover`, `frontmatter`, `loader`, `invoke` | 03, 04, 05 | Tests for frontmatter parsing, sandbox directory discovery, argument substitution, and cache behavior |
| 07 Session and chat persistence | `open-agents-core`, `open-agents-runtime::store`, state backend crate chosen by bucket | `sessions`, `runs`, `messages`, `steps`, `tool_calls`, `approvals`, `sandbox_state` | 01, 02, 03 | Migration/schema tests plus repository round trips for every state record in the architecture contract |
| 08 Durable run streaming | `open-agents-runtime::stream` | `active_stream`, `resume`, `chunk_store`, `tail_index`, `finish` | 02, 07, `ai-sdk-workflow` | Tests for reconnect, duplicate finish, lost client, stream replay, and chunk ordering |
| 09 Chat message bridge | `open-agents-runtime::chat_bridge`, `chat-sdk-chat` | `message_conversion`, `thread_mapping`, `incoming_command`, `outbound_event` | 07, 08 | Tests for `chat-sdk-chat` message to runtime request conversion and persisted response messages |
| 10 Slack ingress and routing | `open-agents-slack::ingress`, `open-agents-service` | `webhook`, `socket_mode`, `router`, `dedupe`, `dispatch` | 09, `chat-sdk-adapter-slack` | Signature, retry, event parsing, duplicate delivery, and route dispatch tests |
| 11 Slack outbound and interactions | `open-agents-slack::outbound`, `open-agents-slack::interactions` | `stream_updates`, `tool_status`, `approvals`, `questions`, `modals`, `ephemeral` | 08, 10 | Tests for Slack blocks/mrkdwn, approval action ids, question flows, and update throttling |
| 12 Slack session lifecycle | `open-agents-slack::sessions`, `open-agents-service` | `session_lookup`, `thread_resume`, `dm_open`, `unsubscribe`, `archive` | 07, 10, 11 | Tests for Slack thread to session mapping, resume, channel visibility, DM creation, and lifecycle status |
| 13 Git and repo automation | `open-agents-runtime::finish_actions`, `open-agents-sandbox::git` | `diff`, `commit`, `push`, `pr`, `post_finish` | 03, 07, 08 | Tests for no-change skip, commit summary, PR state, credential broker cleanup, and failure persistence |
| 14 Observability, safety, and limits | `open-agents-runtime::telemetry`, `open-agents-service::limits` | `usage`, `cost`, `spans`, `redaction`, `rate_limits`, `resource_profile` | 02, 07, 08 | Unit tests for redaction, usage aggregation, telemetry field allowlist, and limit enforcement |
| 15 End-to-end deployment verification | `open-agents-service`, all open-agents crates | `health`, `config`, `startup`, `shutdown`, `live_slack` | 01 through 14 | Full baseline checks plus ignored Slack/model/sandbox live proof when credentials are present |

## Cross-Bucket Rules

- Buckets 02 through 15 must point their new public types at one of the owner
  targets above.
- A bucket may add tests in dependency crates when needed, but runtime behavior
  must stay in the owner target.
- Open Agents source files named in the bucket plan are the source of truth for
  behavior. Rust may use different types only when the difference is documented.
- Any bucket that changes chat parity status must update
  `docs/chat/upstream-parity.md`, `docs/chat/package-progress-estimates.tsv`,
  and regenerate `docs/chat/package-progress.md`.
