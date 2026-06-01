# Open Agents Slack Remote Agent Architecture

This contract fixes the Rust crate boundaries for the Slack remote-agent port
before the implementation buckets start filling in behavior. It is based on a
fresh OpenSrc fetch of `vercel-labs/open-agents` on 2026-05-31.

## Source Verification

- Refresh command run: `npx opensrc fetch https://github.com/vercel-labs/open-agents`.
- Local mirror: `/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/open-agents/main`.
- OpenSrc registry timestamp: `2026-05-31T09:58:27.398325+00:00`.
- Remote `HEAD` verified with `git ls-remote https://github.com/vercel-labs/open-agents HEAD`: `24d679c7ba3d274aa73814c15673aeffcbe3c1c2`.
- Source files rechecked for this contract: `README.md`, `AGENTS.md`, `docs/agents/architecture.md`, `packages/agent/open-agent.ts`, `packages/agent/models.ts`, `packages/agent/system-prompt.ts`, `packages/agent/tools/*.ts`, `packages/agent/subagents/registry.ts`, `packages/agent/skills/*.ts`, `packages/sandbox/interface.ts`, `packages/sandbox/factory.ts`, `packages/sandbox/vercel/*.ts`, `apps/web/app/workflows/chat.ts`, `apps/web/app/api/chat/_lib/runtime.ts`, `apps/web/lib/db/schema.ts`, `apps/web/lib/db/sessions.ts`, and `apps/web/lib/sandbox/provisioning.ts`.
- Rust APIs rechecked: `src/agent.rs`, `crates/ai-sdk-workflow`, `crates/chat-sdk-chat`, and `crates/chat-sdk-adapter-slack`.

## Definition Of Done

A working Rust Slack remote agent means:

1. A Slack mention, DM, slash command, or approved interaction is verified,
   normalized into a `chat-sdk-chat` thread/message, and mapped to exactly one
   durable remote-agent session.
2. The agent run is owned by Rust runtime code outside the sandbox. The sandbox
   is only reached through typed file, shell, git, port, snapshot, timeout, and
   credential-broker operations.
3. Every user message, assistant stream part, tool state, approval request,
   user question, step timing, usage record, cancellation, sandbox state, and
   finish action is durable enough to resume after process restart.
4. Slack receives progressive updates for streaming text, tool starts and
   finishes, pending approvals, user questions, failures, final text, and
   optional commit or pull-request summaries.
5. Secrets are read from process configuration or a secret store at the edge of
   the system. They are never serialized into chat history, sandbox state,
   telemetry fields, commit messages, or docs.

## System Boundaries

Open Agents separates `Web -> Agent workflow -> Sandbox VM`. The Rust Slack port
keeps the same control-plane rule but replaces the web UI with Slack:

```text
Slack -> open-agents-slack -> chat-sdk-chat -> open-agents-runtime -> open-agents-sandbox
```

- `open-agents-slack` owns Slack signing verification, Slack thread identity,
  Slack event routing, and outbound formatting. It composes
  `chat-sdk-adapter-slack` and must not own model, sandbox, or durable-run logic.
- `chat-sdk-chat` remains the cross-platform chat abstraction for adapters,
  threads, messages, dedupe, locks, and state-backend behavior.
- `open-agents-runtime` owns durable run orchestration, model selection, stream
  resume, cancellation, step accounting, post-finish automation, and the bridge
  from chat messages to `ToolLoopAgent` or `WorkflowAgent`.
- `open-agents-sandbox` owns the sandbox boundary. Agent tools depend on this
  crate, not on a concrete Vercel, local, or future provider API.
- `open-agents-core` owns serializable ids, statuses, event envelopes, source
  metadata, model selections, and cross-crate state vocabulary.
- `open-agents-service` is the deployable process that wires configuration,
  state backends, Slack ingress, runtime workers, and sandbox connectors.

## Current Rust Composition

- `src/agent.rs` provides `ToolLoopAgent`, `ToolLoopAgentSettings`,
  `ToolLoopAgentCallOptions`, callback aliases, UI-message stream response
  creation, model settings, tool approvals, runtime context, tools context,
  telemetry, abort signal, timeout, and a default 20-step tool loop.
- `crates/ai-sdk-workflow` provides portable workflow stream helpers,
  `WorkflowAgent`, serializable tools, workflow chat transport, stream reconnect
  behavior, and typed step/event metadata. The remote runtime should extend this
  package instead of creating a separate workflow vocabulary.
- `crates/chat-sdk-chat` provides `Chat`, `Thread`, `Channel`, the `Adapter`
  trait, state adapter contracts, event dispatch, thread locks, queueing, dedupe,
  modal context state, user lookup, DM opening, and postable message dispatch.
- `crates/chat-sdk-adapter-slack` provides Slack adapter options, Slack thread id
  encoding as `slack:<channel_id>:<thread_ts>`, Slack API body shaping, Slack Web
  API posting, Slack webhook signature verification, and parsed Slack webhook
  payloads including app mentions, DMs, slash commands, block actions, options
  loads, and views.

## Crate Ownership

| Crate | Owner buckets | Public responsibility |
| --- | --- | --- |
| `open-agents-core` | 01, then all buckets | Serializable ids, run/session statuses, event kinds, source metadata, model selection, secret names, and cross-crate state vocabulary. |
| `open-agents-runtime` | 02, 05, 08, 13, 14 | Durable run loop, model preparation, stream resume, cancellation, step accounting, subagent dispatch, post-finish automation, usage, telemetry, and safety limits. |
| `open-agents-sandbox` | 03, 04, 06, 13, 15 | Sandbox state, connection inputs, file/shell/git/port/snapshot operations, timeout management, skill discovery inputs, and sandbox verification fixtures. |
| `open-agents-slack` | 09, 10, 11, 12, 15 | Slack ingress, Slack thread/session mapping, outbound message rendering, approvals, questions, interactions, Socket Mode or webhook routing, and live Slack proof. |
| `open-agents-service` | 10, 12, 14, 15 | Process wiring, configuration loading, state backend setup, router/server entrypoint, health checks, graceful shutdown, and deployment proof. |
| `ai-sdk-rust` | 02, 04, 05 | `ToolLoopAgent`, model calls, tools, approvals, telemetry, and UI-message conversion. |
| `ai-sdk-workflow` | 02, 08, 14 | Workflow agent behavior, stream chunks, reconnect transport, serializable tools, and run-step metadata. |
| `chat-sdk-chat` | 09, 10, 11, 12 | Chat, thread, adapter, message, state, dedupe, locks, and event dispatch primitives. |
| `chat-sdk-adapter-slack` | 10, 11, 12, 15 | Slack-specific verification, parsing, Web API calls, thread ids, mrkdwn, modals, cards, and live adapter proof. |

## State Contract

The first release needs these durable records. Bucket 07 may choose the concrete
storage backend, but other buckets must use these names and ownership lines.

| Record | Owning crate | Required fields |
| --- | --- | --- |
| `RemoteAgentSession` | `open-agents-core` | Session id, Slack team/channel/thread/user ids, chat id, current sandbox state id, status, created/updated timestamps. |
| `RemoteAgentRun` | `open-agents-runtime` | Run id, session id, chat id, model id, status, active stream id, started/finished timestamps, cancellation state. |
| `RemoteAgentMessage` | `open-agents-runtime` | Message id, run id, chat id, role, ordered parts, source Slack ts, model metadata, usage, created timestamp. |
| `RemoteAgentStep` | `open-agents-runtime` | Run id, step number, start/end timestamps, finish reason, raw finish reason, model id, warnings, usage, cost. |
| `RemoteAgentToolCall` | `open-agents-runtime` | Tool call id, run id, step number, tool name, input, state, approval id, output or error. |
| `RemoteAgentApproval` | `open-agents-slack` | Approval id, run id, Slack action ids, requesting user id, decision, decision timestamp. |
| `RemoteAgentSandboxState` | `open-agents-sandbox` | Session id, provider kind, resumable state payload, working directory, current branch, environment details, expiry. |
| `RemoteAgentFinishAction` | `open-agents-runtime` | Run id, action kind, pending/success/error state, commit sha, PR number, URL, user-visible summary. |

## Configuration And Secrets

Required for the Slack release:

| Name | Owner | Rule |
| --- | --- | --- |
| `SLACK_BOT_TOKEN` | `open-agents-slack` | Bot OAuth token, never persisted, only used for Slack Web API calls. |
| `SLACK_SIGNING_SECRET` | `open-agents-slack` | Required for webhook signature verification, never logged. |
| `SLACK_APP_TOKEN` | `open-agents-slack` | Optional Socket Mode token. If unset, webhook HTTP ingress owns delivery. |
| `SLACK_TEST_CHANNEL_ID` | `open-agents-slack` | Optional live-test target, ignored unless credential-gated tests are explicitly run. |
| `SLACK_TEST_USER_ID` | `open-agents-slack` | Optional live-test actor id, ignored unless credential-gated tests are explicitly run. |
| `AI_GATEWAY_API_KEY` | `open-agents-runtime` | Model credential for remote-agent runs. The existing repo alias may also be accepted. |
| `AI_SDK_RUST_AI_GATEWAY_API_KEY` | `open-agents-runtime` | Existing repo alias for model integration proof. |
| `OPEN_AGENTS_STATE_URL` | `open-agents-service` | Durable state connection string or URL. The first backend bucket defines the concrete format. |
| `OPEN_AGENTS_RESOURCE_PROFILE` | `open-agents-service` | Optional deployment sizing profile. Values beyond `default` and `hobby` need explicit docs. |
| `OPEN_AGENTS_SANDBOX` | `open-agents-service` | Selects `just-bash` (default virtual backend), `local` (explicit host process backend), or `vercel` sandbox execution. |
| `OPEN_AGENTS_VERCEL_TOKEN`, `VERCEL_TOKEN`, `VERCEL_OIDC_TOKEN` | `open-agents-sandbox` | Vercel Sandbox bearer credential. Prefer `OPEN_AGENTS_VERCEL_TOKEN` on Vercel deployments so application auth does not collide with CLI build auth. |
| `VERCEL_TEAM_ID` | `open-agents-sandbox` | Vercel team id passed to the Sandbox v2 API. |
| `VERCEL_PROJECT_ID` | `open-agents-sandbox` | Vercel project id passed to Sandbox create/list/get requests. |
| `VERCEL_SANDBOX_NAME` | `open-agents-sandbox` | Optional stable named sandbox to resume. |
| `VERCEL_SANDBOX_BASE_SNAPSHOT_ID` | `open-agents-sandbox` | Optional Vercel sandbox base snapshot id. |
| `VERCEL_SANDBOX_RUNTIME`, `VERCEL_SANDBOX_VCPUS`, `VERCEL_SANDBOX_TIMEOUT_MS`, `VERCEL_SANDBOX_PERSISTENT` | `open-agents-sandbox` | Optional Vercel sandbox create settings. |
| `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY` | `open-agents-runtime` | Optional commit, push, and PR automation credentials. |

Secrets must be injected at process startup or per-request broker boundaries.
Only stable secret names may appear in persisted records.

## Intentionally Not Ported First

The first Slack release must not chase these Open Agents web-only pieces unless
they become necessary for Slack behavior:

- Next.js pages, layouts, route handlers, React chat components, share pages,
  voice input UI, Better Auth web sessions, and browser-specific reconnect UI.
- Vercel OAuth sign-in as the primary identity surface. Slack user/team/channel
  identity is the first release identity surface.
- Read-only public share links and session sidebars.
- Browser diff views and web cached-diff display. The runtime may still persist
  diff summaries for Slack and PR automation.
- ElevenLabs voice transcription.
- Vercel-specific deployment button behavior and web project import flow.

Portable pieces from the web app that must be retained are durable workflow
semantics, session/chat persistence concepts, active stream ownership, sandbox
provision/resume/hibernate state, usage recording, cancellation, auto-commit,
and auto-PR finish actions.

## Dependency Order

1. Core ids and statuses in `open-agents-core`.
2. Runtime run loop in `open-agents-runtime` using `ToolLoopAgent` or
   `WorkflowAgent`.
3. Sandbox trait and provider connector in `open-agents-sandbox`.
4. Tool implementations over the sandbox boundary.
5. Subagents, context management, and skills.
6. Durable state schema and repository layer.
7. Stream persistence and resume.
8. Chat bridge from `chat-sdk-chat` messages to runtime requests.
9. Slack ingress, outbound updates, interactions, and session lifecycle.
10. Git automation, observability, deployment, and end-to-end proof.

## Review Rules For Later Buckets

- Do not add a new crate for an Open Agents concept unless this contract is
  first updated with the new owner and dependency reason.
- Do not let Slack crates call sandbox operations directly. Slack talks to the
  runtime through chat/run commands.
- Do not let tool crates depend on Slack adapter types. Tools receive sandbox
  and runtime context only.
- Do not store raw Slack tokens, model credentials, GitHub private keys, or
  installation tokens in any state record.
- Every live proof that needs Slack credentials must be an ignored test or a
  documented manual command guarded by the required environment variables.
