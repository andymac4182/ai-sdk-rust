# Chat SDK Rust Package Progress

_Generated from `docs/chat/upstream-parity.md` and `docs/chat/package-progress-estimates.tsv`._

- Displayed package rows: 18
- Average estimated completion: 97.8%
- Portable package average: 97.3%
- Closed package rows: 16 / 18
- Strict portable verified rows: 13 / 15
- In-progress rows: 2
- Not-started rows: 0

## 100% Closed

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
| `@chat-sdk/chat` | 100% | Verified | core SDK package |
| `@chat-sdk/adapter-shared` | 100% | Verified | shared adapter utilities |
| `@chat-sdk/tests` | 100% | JavaScript-only | test support library |
| `@chat-sdk/state-memory` | 100% | Verified | state backend (in-memory) |
| `@chat-sdk/adapter-teams` | 100% | Verified | adapter package |
| `@chat-sdk/adapter-discord` | 100% | Verified | adapter package |
| `@chat-sdk/adapter-linear` | 100% | Verified | adapter package |
| `@chat-sdk/adapter-github` | 100% | Verified | adapter package |
| `@chat-sdk/adapter-messenger` | 100% | Verified | adapter package |
| `@chat-sdk/adapter-telegram` | 100% | Verified | adapter package |
| `@chat-sdk/adapter-whatsapp` | 100% | Verified | adapter package |
| `@chat-sdk/adapter-web` | 100% | JavaScript-only | adapter package |
| `@chat-sdk/state-redis` | 100% | Verified | state backend (Redis) |
| `@chat-sdk/state-ioredis` | 100% | Verified | state backend (ioredis) |
| `@chat-sdk/state-pg` | 100% | Verified | state backend (Postgres) |
| `@chat-sdk/integration-tests` | 100% | JavaScript-only | integration tests |

## In Progress

| Package | Est. completion | Status | Kind | Basis / remaining work |
| --- | ---: | --- | --- | --- |
| `@chat-sdk/adapter-slack` | 85% | In progress | adapter package | chat-sdk-adapter-slack 203 tests (slice 355 adds post_ephemeral pure helpers + 5 cases, slice 366 adds Adapter trait... |
| `@chat-sdk/adapter-gchat` | 75% | In progress | adapter package | chat-sdk-adapter-gchat 134 tests (slice 357 adds post_ephemeral via privateMessageViewer pure helpers + 4 cases, slice... |

## Not Started

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
