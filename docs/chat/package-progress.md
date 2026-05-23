# Chat SDK Rust Package Progress

_Generated from `docs/chat/upstream-parity.md` and `docs/chat/package-progress-estimates.tsv`._

- Displayed package rows: 18
- Average estimated completion: 51.0%
- Portable package average: 41.2%
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
| `@chat-sdk/chat` | 99% | In progress | core SDK package | Crate + colocated tests across 16 modules: 567 chat tests. Phase 1.5 complete. Slice 159 extended Adapter trait with... |
| `@chat-sdk/adapter-slack` | 42% | In progress | adapter package | Slice 139 scaffold + slice 152 post_message + slice 158 fetch_subject (additive) + slice 160 edit/delete/react/typing... |
| `@chat-sdk/adapter-teams` | 35% | In progress | adapter package | Slice 138 scaffold + slice 153 post_message + slice 167 edit/delete/react/typing + slice 172 errors module... |
| `@chat-sdk/adapter-gchat` | 30% | In progress | adapter package | Slice 137 scaffold + slice 154 post_message + slice 168 edit_message (PATCH with updateMask) + delete_message +... |
| `@chat-sdk/adapter-discord` | 30% | In progress | adapter package | Slice 134 scaffold + slice 149 post_message + slice 165 edit_message + delete_message + add_reaction (PUT... |
| `@chat-sdk/adapter-linear` | 32% | In progress | adapter package | Slice 136 scaffold + slice 151 post_message + slice 166 edit/delete/react/typing + slice 171 utils module... |
| `@chat-sdk/adapter-github` | 30% | In progress | adapter package | Slice 131 scaffold + slice 146 post_message + slice 156 fetch_subject (additive) + slice 162 edit_message +... |
| `@chat-sdk/adapter-messenger` | 32% | In progress | adapter package | Slice 132 scaffold + slice 147 post_message + slice 163 edit/delete/react/typing + slice 173 wire-format correction... |
| `@chat-sdk/adapter-telegram` | 30% | In progress | adapter package | Slice 130 scaffold + slice 145 post_message + slice 155 fetch_subject (additive) + slice 161 edit_message +... |
| `@chat-sdk/adapter-whatsapp` | 28% | In progress | adapter package | Slice 133 scaffold + slice 148 post_message + slice 164 edit/delete (unsupported per upstream) + add_reaction (Cloud... |
| `@chat-sdk/state-redis` | 10% | In progress | state backend (Redis) | Crate scaffold (slice 140): RedisStateAdapter impl StateAdapter (NotConnected placeholder). 11 tests. redis-rs wire-up... |
| `@chat-sdk/state-ioredis` | 10% | In progress | state backend (ioredis) | Crate scaffold (slice 141): IoredisStateAdapter (cluster + Sentinel support) impl StateAdapter. 11 tests. redis-rs... |
| `@chat-sdk/state-pg` | 10% | In progress | state backend (Postgres) | Crate scaffold (slice 142): PgStateAdapter impl StateAdapter. 10 tests. tokio-postgres/sqlx wire-up deferred. |

## Not Started

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
