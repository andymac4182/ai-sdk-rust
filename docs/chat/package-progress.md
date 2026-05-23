# Chat SDK Rust Package Progress

_Generated from `docs/chat/upstream-parity.md` and `docs/chat/package-progress-estimates.tsv`._

- Displayed package rows: 18
- Average estimated completion: 41.6%
- Portable package average: 29.9%
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
| `@chat-sdk/adapter-slack` | 10% | In progress | adapter package | Crate scaffold (slice 139) w/ is_dm/is_group predicates. 14 tests. Web API + Socket Mode deferred. |
| `@chat-sdk/adapter-teams` | 10% | In progress | adapter package | Crate scaffold (slice 138) rsplit Bot Framework conv ids. 12 tests. HTTP I/O + Bot Framework auth deferred. |
| `@chat-sdk/adapter-gchat` | 10% | In progress | adapter package | Crate scaffold (slice 137) w/ empty-thread top-level sentinel. 14 tests. OAuth2 + HTTP I/O deferred. |
| `@chat-sdk/adapter-discord` | 15% | In progress | adapter package | Slice 134 scaffold + slice 149 post_message HTTP (Bot auth). 14 tests. |
| `@chat-sdk/adapter-linear` | 15% | In progress | adapter package | Slice 136 scaffold + slice 151 post_message HTTP (GraphQL commentCreate). 12 tests. |
| `@chat-sdk/adapter-github` | 15% | In progress | adapter package | Slice 131 scaffold + slice 146 post_message HTTP (issue/PR comment-create). 14 tests. |
| `@chat-sdk/adapter-messenger` | 15% | In progress | adapter package | Slice 132 scaffold + slice 147 post_message HTTP (Send API). 12 tests. |
| `@chat-sdk/adapter-telegram` | 15% | In progress | adapter package | Slice 130 scaffold + slice 145 post_message HTTP. 14 tests. Reference impl. |
| `@chat-sdk/adapter-whatsapp` | 15% | In progress | adapter package | Slice 133 scaffold + slice 148 post_message HTTP (Cloud API). 13 tests. |
| `@chat-sdk/state-redis` | 10% | In progress | state backend (Redis) | Crate scaffold (slice 140): RedisStateAdapter impl StateAdapter (NotConnected placeholder). 11 tests. redis-rs wire-up... |
| `@chat-sdk/state-ioredis` | 10% | In progress | state backend (ioredis) | Crate scaffold (slice 141): IoredisStateAdapter (cluster + Sentinel support) impl StateAdapter. 11 tests. redis-rs... |
| `@chat-sdk/state-pg` | 10% | In progress | state backend (Postgres) | Crate scaffold (slice 142): PgStateAdapter impl StateAdapter. 10 tests. tokio-postgres/sqlx wire-up deferred. |

## Not Started

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
