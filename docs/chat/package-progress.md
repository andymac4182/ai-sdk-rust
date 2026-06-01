# Chat SDK Rust Package Progress

_Generated from `docs/chat/upstream-parity.md` and `docs/chat/package-progress-estimates.tsv`._

- Displayed package rows: 19
- Average estimated completion: 93.8%
- Portable package average: 92.7%
- Closed package rows: 13 / 19
- Strict portable verified rows: 10 / 16
- In-progress rows: 5
- Not-started rows: 1

## 100% Closed

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
| `@chat-sdk/adapter-shared` | 100% | Verified | shared adapter utilities |
| `@chat-sdk/tests` | 100% | JavaScript-only | test support library |
| `@chat-sdk/state-memory` | 100% | Verified | state backend (in-memory) |
| `@chat-sdk/adapter-teams` | 100% | Verified | adapter package |
| `@chat-sdk/adapter-discord` | 100% | Verified | adapter package |
| `@chat-sdk/adapter-linear` | 100% | Verified | adapter package |
| `@chat-sdk/adapter-github` | 100% | Verified | adapter package |
| `@chat-sdk/adapter-messenger` | 100% | Verified | adapter package |
| `@chat-sdk/adapter-web` | 100% | JavaScript-only | adapter package |
| `@chat-sdk/state-redis` | 100% | Verified | state backend (Redis) |
| `@chat-sdk/state-ioredis` | 100% | Verified | state backend (ioredis) |
| `@chat-sdk/state-pg` | 100% | Verified | state backend (Postgres) |
| `@chat-sdk/integration-tests` | 100% | JavaScript-only | integration tests |

## In Progress

| Package | Est. completion | Status | Kind | Basis / remaining work |
| --- | ---: | --- | --- | --- |
| `@chat-sdk/chat` | 99% | In progress | core SDK package | Current upstream `ffc43fcf1f7679164be0806308bea237113c7590` added one `thread.test.ts` stream-fallback case (`should... |
| `@chat-sdk/adapter-slack` | 95% | In progress | adapter package | Current upstream `ffc43fcf1f7679164be0806308bea237113c7590` added the Slack `blocks` subpath plus two API helper... |
| `@chat-sdk/adapter-gchat` | 98% | In progress | adapter package | Current upstream `ffc43fcf1f7679164be0806308bea237113c7590` added five `markdown.test.ts` autolink/phone-formatting... |
| `@chat-sdk/adapter-telegram` | 93% | In progress | adapter package | Current upstream `ffc43fcf1f7679164be0806308bea237113c7590` added twelve `index.test.ts` streaming and markdown... |
| `@chat-sdk/adapter-whatsapp` | 98% | In progress | adapter package | Current upstream `ffc43fcf1f7679164be0806308bea237113c7590` replaced the old typing no-op assertion with two... |

## Not Started

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
| `@chat-sdk/adapter-twilio` | 0% | Not started | adapter package |
