# Chat SDK Rust Package Progress

_Generated from `docs/chat/upstream-parity.md` and `docs/chat/package-progress-estimates.tsv`._

- Displayed package rows: 18
- Average estimated completion: 42.8%
- Portable package average: 31.3%
- Closed package rows: 5 / 18
- Strict portable verified rows: 2 / 15
- In-progress rows: 13
- Not-started rows: 0

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
| `@chat-sdk/chat` | 99% | In progress | core SDK package | Crate + colocated tests across 16 modules: 563 chat tests. Phase 1.5 complete. |
| `@chat-sdk/adapter-slack` | 15% | In progress | adapter package | Slice 139 scaffold + slice 152 post_message HTTP (chat.postMessage Web API). 15 tests. |
| `@chat-sdk/adapter-teams` | 15% | In progress | adapter package | Slice 138 scaffold + slice 153 post_message HTTP (Bot Framework activities + pre-minted bearer). 15 tests. |
| `@chat-sdk/adapter-gchat` | 15% | In progress | adapter package | Slice 137 scaffold + slice 154 post_message HTTP (messages.create + pre-minted bearer + thread-reply option). 17 tests. |
| `@chat-sdk/adapter-discord` | 15% | In progress | adapter package | Slice 134 scaffold + slice 149 post_message HTTP (Bot auth). 14 tests. |
| `@chat-sdk/adapter-linear` | 15% | In progress | adapter package | Slice 136 scaffold + slice 151 post_message HTTP (GraphQL commentCreate). 12 tests. |
| `@chat-sdk/adapter-github` | 18% | In progress | adapter package | Slice 131 scaffold + slice 146 post_message + slice 156 fetch_subject (GET issues/<n>). 16 tests. |
| `@chat-sdk/adapter-messenger` | 15% | In progress | adapter package | Slice 132 scaffold + slice 147 post_message HTTP (Send API). 12 tests. |
| `@chat-sdk/adapter-telegram` | 18% | In progress | adapter package | Slice 130 scaffold + slice 145 post_message (sendMessage) + slice 155 fetch_subject (getChat). 15 tests. 2 of 8... |
| `@chat-sdk/adapter-whatsapp` | 15% | In progress | adapter package | Slice 133 scaffold + slice 148 post_message HTTP (Cloud API). 13 tests. |
| `@chat-sdk/state-redis` | 10% | In progress | state backend (Redis) | Crate scaffold (slice 140): RedisStateAdapter impl StateAdapter (NotConnected placeholder). 11 tests. redis-rs wire-up... |
| `@chat-sdk/state-ioredis` | 10% | In progress | state backend (ioredis) | Crate scaffold (slice 141): IoredisStateAdapter (cluster + Sentinel support) impl StateAdapter. 11 tests. redis-rs... |
| `@chat-sdk/state-pg` | 10% | In progress | state backend (Postgres) | Crate scaffold (slice 142): PgStateAdapter impl StateAdapter. 10 tests. tokio-postgres/sqlx wire-up deferred. |

## Not Started

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
