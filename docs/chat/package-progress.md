# Chat SDK Rust Package Progress

_Generated from `docs/chat/upstream-parity.md` and `docs/chat/package-progress-estimates.tsv`._

- Displayed package rows: 18
- Average estimated completion: 33.3%
- Portable package average: 19.9%
- Closed package rows: 5 / 18
- Strict portable verified rows: 2 / 15
- In-progress rows: 1
- Not-started rows: 12

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
| `@chat-sdk/chat` | 99% | In progress | core SDK package | Crate + colocated tests across 13 modules: errors 17 + logger 13 + chat_singleton 5 + emoji 42/42 (1:1) + markdown... |

## Not Started

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
| `@chat-sdk/adapter-slack` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-teams` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-gchat` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-discord` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-linear` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-github` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-messenger` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-telegram` | 0% | Not started | adapter package |
| `@chat-sdk/adapter-whatsapp` | 0% | Not started | adapter package |
| `@chat-sdk/state-redis` | 0% | Not started | state backend (Redis) |
| `@chat-sdk/state-ioredis` | 0% | Not started | state backend (ioredis) |
| `@chat-sdk/state-pg` | 0% | Not started | state backend (Postgres) |
