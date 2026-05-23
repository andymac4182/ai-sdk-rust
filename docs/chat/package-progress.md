# Chat SDK Rust Package Progress

_Generated from `docs/chat/upstream-parity.md` and `docs/chat/package-progress-estimates.tsv`._

- Displayed package rows: 18
- Average estimated completion: 37.2%
- Portable package average: 24.6%
- Closed package rows: 5 / 18
- Strict portable verified rows: 2 / 15
- In-progress rows: 8
- Not-started rows: 5

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
| `@chat-sdk/chat` | 99% | In progress | core SDK package | Crate + colocated tests across 16 modules: 563 chat tests. Phase 1.5 complete. Remaining: handler/event registration,... |
| `@chat-sdk/adapter-gchat` | 10% | In progress | adapter package | Crate scaffold (slice 137): GchatAdapter + thread-id codec (incl. empty-thread top-level sentinel). 14 tests. OAuth2 +... |
| `@chat-sdk/adapter-discord` | 10% | In progress | adapter package | Crate scaffold (slice 134): DiscordAdapter + thread-id codec (incl. @me DM sentinel). 13 tests. HTTP I/O deferred. |
| `@chat-sdk/adapter-linear` | 10% | In progress | adapter package | Crate scaffold (slice 136): LinearAdapter + thread-id codec. 11 tests. HTTP I/O + GraphQL deferred. |
| `@chat-sdk/adapter-github` | 10% | In progress | adapter package | Crate scaffold (slice 131): GithubAdapter + thread-id codec. 13 tests. HTTP I/O deferred. |
| `@chat-sdk/adapter-messenger` | 10% | In progress | adapter package | Crate scaffold (slice 132): MessengerAdapter + thread-id codec. 11 tests. HTTP I/O deferred. |
| `@chat-sdk/adapter-telegram` | 10% | In progress | adapter package | Crate scaffold (slice 130): TelegramAdapter + thread-id codec. 13 tests. HTTP I/O deferred. |
| `@chat-sdk/adapter-whatsapp` | 10% | In progress | adapter package | Crate scaffold (slice 133): WhatsappAdapter + thread-id codec. 11 tests. HTTP I/O deferred. |

## Not Started

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
| `@chat-sdk/adapter-slack` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-teams` | 0% | Not started | adapter package |
| `@chat-sdk/state-redis` | 0% | Not started | state backend (Redis) |
| `@chat-sdk/state-ioredis` | 0% | Not started | state backend (ioredis) |
| `@chat-sdk/state-pg` | 0% | Not started | state backend (Postgres) |
