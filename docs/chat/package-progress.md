# Chat SDK Rust Package Progress

_Generated from `docs/chat/upstream-parity.md` and `docs/chat/package-progress-estimates.tsv`._

- Displayed package rows: 18
- Average estimated completion: 34.4%
- Portable package average: 21.3%
- Closed package rows: 5 / 18
- Strict portable verified rows: 2 / 15
- In-progress rows: 3
- Not-started rows: 10

## 100% Closed

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
| `@chat-sdk/adapter-shared` | 100% | Verified | shared adapter utilities |
| `@chat-sdk/tests` | 100% | JavaScript-only | test support library |
| `@chat-sdk/state-memory` | 100% | Verified | state backend (in-memory) |
| `@chat-sdk/adapter-web` | 100% | JavaScript-only | adapter package |
| `@chat-sdk/integration-tests` | 100% | JavaScript-only | integration tests |

## In Progress

| Package | Est. completion | Status | Kind | Basis / remaining work |
| --- | ---: | --- | --- | --- |
| `@chat-sdk/chat` | 99% | In progress | core SDK package | Crate + colocated tests across 16 modules: errors 17 + logger 13 + chat_singleton 5 + emoji 42/42 (1:1) + markdown... |
| `@chat-sdk/adapter-github` | 10% | In progress | adapter package | Crate scaffold (slice 131): GithubAdapter impl-ing chat_sdk_chat::types::Adapter (name = "github"),... |
| `@chat-sdk/adapter-telegram` | 10% | In progress | adapter package | Crate scaffold (slice 130): TelegramAdapter impl-ing chat_sdk_chat::types::Adapter (name = "telegram"),... |

## Not Started

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
| `@chat-sdk/adapter-slack` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-teams` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-gchat` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-discord` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-linear` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-messenger` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-whatsapp` | 0% | Not started | adapter package |
| `@chat-sdk/state-redis` | 0% | Not started | state backend (Redis) |
| `@chat-sdk/state-ioredis` | 0% | Not started | state backend (ioredis) |
| `@chat-sdk/state-pg` | 0% | Not started | state backend (Postgres) |
