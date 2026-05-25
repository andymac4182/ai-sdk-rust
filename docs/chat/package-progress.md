# Chat SDK Rust Package Progress

_Generated from `docs/chat/upstream-parity.md` and `docs/chat/package-progress-estimates.tsv`._

- Displayed package rows: 18
- Average estimated completion: 85.1%
- Portable package average: 82.1%
- Closed package rows: 9 / 18
- Strict portable verified rows: 6 / 15
- In-progress rows: 9
- Not-started rows: 0

## 100% Closed

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
| `@chat-sdk/chat` | 100% | Verified | core SDK package |
| `@chat-sdk/adapter-shared` | 100% | Verified | shared adapter utilities |
| `@chat-sdk/tests` | 100% | JavaScript-only | test support library |
| `@chat-sdk/state-memory` | 100% | Verified | state backend (in-memory) |
| `@chat-sdk/adapter-web` | 100% | JavaScript-only | adapter package |
| `@chat-sdk/state-redis` | 100% | Verified | state backend (Redis) |
| `@chat-sdk/state-ioredis` | 100% | Verified | state backend (ioredis) |
| `@chat-sdk/state-pg` | 100% | Verified | state backend (Postgres) |
| `@chat-sdk/integration-tests` | 100% | JavaScript-only | integration tests |

## In Progress

| Package | Est. completion | Status | Kind | Basis / remaining work |
| --- | ---: | --- | --- | --- |
| `@chat-sdk/adapter-slack` | 85% | In progress | adapter package | chat-sdk-adapter-slack 203 tests (slice 355 adds post_ephemeral pure helpers + 5 cases, slice 366 adds Adapter trait... |
| `@chat-sdk/adapter-teams` | 65% | In progress | adapter package | chat-sdk-adapter-teams 107 tests (slice 365 adds channel_id_from_thread_id helper + 2 cases, slice 366 adds Adapter... |
| `@chat-sdk/adapter-gchat` | 75% | In progress | adapter package | chat-sdk-adapter-gchat 134 tests (slice 357 adds post_ephemeral via privateMessageViewer pure helpers + 4 cases, slice... |
| `@chat-sdk/adapter-discord` | 75% | In progress | adapter package | chat-sdk-adapter-discord 162 tests (slice 360 splits channelIdFromThreadId into 3 1:1, slice 363 adds 3... |
| `@chat-sdk/adapter-linear` | 60% | In progress | adapter package | chat-sdk-adapter-linear 113 tests (slice 366 adds Adapter trait impls, slice 406 splits channelIdFromThreadId into 3... |
| `@chat-sdk/adapter-github` | 60% | In progress | adapter package | chat-sdk-adapter-github 107 tests (slice 368 splits bundled emojiToGitHubReaction into 16 explicit 1:1 cases, slice... |
| `@chat-sdk/adapter-messenger` | 70% | In progress | adapter package | chat-sdk-adapter-messenger 102 tests (slice 369 adds normalize_thread_id helper, slice 370 adds Adapter::open_dm trait... |
| `@chat-sdk/adapter-telegram` | 72% | In progress | adapter package | chat-sdk-adapter-telegram 133 tests (slice 370 adds Adapter::open_dm trait impl). Helpers: applyTelegramEntities... |
| `@chat-sdk/adapter-whatsapp` | 70% | In progress | adapter package | chat-sdk-adapter-whatsapp 106 tests (slice 370 adds Adapter::open_dm trait impl). splitMessage 8/8, channelId+isDM,... |

## Not Started

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
