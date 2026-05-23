# Chat SDK Rust Package Progress

_Generated from `docs/chat/upstream-parity.md` and `docs/chat/package-progress-estimates.tsv`._

- Displayed package rows: 18
- Average estimated completion: 39.4%
- Portable package average: 27.3%
- Closed package rows: 5 / 18
- Strict portable verified rows: 2 / 15
- In-progress rows: 12
- Not-started rows: 1

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
| `@chat-sdk/adapter-discord` | 10% | In progress | adapter package | Crate scaffold (slice 134) w/ @me DM sentinel. 13 tests. HTTP I/O deferred. |
| `@chat-sdk/adapter-linear` | 10% | In progress | adapter package | Crate scaffold (slice 136). 11 tests. HTTP I/O + GraphQL deferred. |
| `@chat-sdk/adapter-github` | 10% | In progress | adapter package | Crate scaffold (slice 131). 13 tests. HTTP I/O deferred. |
| `@chat-sdk/adapter-messenger` | 10% | In progress | adapter package | Crate scaffold (slice 132). 11 tests. HTTP I/O deferred. |
| `@chat-sdk/adapter-telegram` | 10% | In progress | adapter package | Crate scaffold (slice 130). 13 tests. HTTP I/O deferred. |
| `@chat-sdk/adapter-whatsapp` | 10% | In progress | adapter package | Crate scaffold (slice 133). 11 tests. HTTP I/O deferred. |
| `@chat-sdk/state-redis` | 10% | In progress | state backend (Redis) | Crate scaffold (slice 140): RedisStateAdapter impl StateAdapter (NotConnected placeholder). 11 tests. redis-rs wire-up... |
| `@chat-sdk/state-ioredis` | 10% | In progress | state backend (ioredis) | Crate scaffold (slice 141): IoredisStateAdapter (cluster + Sentinel support) impl StateAdapter (NotConnected... |

## Not Started

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
| `@chat-sdk/state-pg` | 0% | Not started | state backend (Postgres) |
