# Open Agents Upstream Parity

This ledger is generated from the refreshed upstream Open Agents mirror and is
the OA-01 gate for future Rust implementation buckets. Every current upstream
package manifest, non-test TS/TSX source file, and test file must have a Rust
owner, a named Rust test, or an explicit documented exception.

## Source Snapshot

| Field | Value |
| --- | --- |
| Upstream repo | `vercel-labs/open-agents` |
| Inventory command | `npx opensrc fetch https://github.com/vercel-labs/open-agents` |
| Local source path | `/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/open-agents/main` |
| Remote HEAD verification | `git ls-remote https://github.com/vercel-labs/open-agents HEAD` |
| Upstream commit | `24d679c7ba3d274aa73814c15673aeffcbe3c1c2` |
| Inventory date | `2026-06-01` |
| Package manifests | 6 |
| TS/TSX files | 567 |
| Non-test source files | 461 |
| Test files | 106 |
| Gate command | `node scripts/open-agents-test-inventory.mjs --check` |

## Status Rules

Use `portable` for behavior that must be owned by Rust, `js-only-documented`
for browser, Next.js, Bun, React, Better Auth, or web-account behavior outside
the Slack-first Rust release, and `type-system-impossible` only for TypeScript
language-service assertions that cannot become Rust runtime checks.

Rows marked `in-progress` are not complete parity. They are owner-mapped
inventory rows that future Open Agents buckets must close with named Rust tests
before claiming the owning package is ported or verified. Rows marked
`js-only-documented` are explicit exclusions and must carry the reason in the
notes column.

## Gate Rules

- The refreshed upstream count is expected to be exactly 6 package manifests, 461 non-test source files, and 106 test files. If upstream changes, update this ledger in the same commit that explains the drift.
- The checker fails when any source file lacks a Rust owner or documented exclusion.
- The checker fails when any test file lacks a Rust owner, named Rust test, pending owner marker, or explicit exception.
- Extra Rust tests are additive. They do not close a portable upstream row unless the row names the Rust test or keeps an explicit pending owner marker for a later implementation bucket.

## Package Inventory

| Package id | Manifest | Package name | Source files | Test files | Rust owner/exclusion |
| --- | --- | --- | --- | --- | --- |
| apps/web | apps/web/package.json | web | 407 | 98 | open-agents-service/open-agents-runtime/open-agents-sandbox for portable server behavior; Next/React UI excluded |
| root | package.json | open-agents | 0 | 0 | workspace metadata only |
| packages/agent | packages/agent/package.json | @open-agents/agent | 33 | 3 | open-agents-runtime plus ai-sdk-rust::open_agents_tools, ::subagents, and ::skills |
| packages/sandbox | packages/sandbox/package.json | @open-agents/sandbox | 12 | 3 | open-agents-sandbox |
| packages/shared | packages/shared/package.json | @open-agents/shared | 7 | 2 | open-agents-slack/open-agents-sandbox for portable helpers; React hooks excluded |
| packages/tsconfig | packages/tsconfig/package.json | @open-agents/tsconfig | 0 | 0 | js-only-documented TypeScript build config |

## Source Summary

| Package | Source files | Portable | JS only | Type system |
| --- | --- | --- | --- | --- |
| apps/web | 407 | 119 | 288 | 0 |
| packages/agent | 33 | 33 | 0 | 0 |
| packages/sandbox | 12 | 12 | 0 | 0 |
| packages/shared | 7 | 2 | 5 | 0 |
| scripts | 2 | 1 | 1 | 0 |

## Test Summary

| Package | Test files | Case calls | Portable files | Verified files | In-progress files | JS-only files | Type-system files |
| --- | --- | --- | --- | --- | --- | --- | --- |
| apps/web | 98 | 644 | 58 | 0 | 58 | 40 | 0 |
| packages/agent | 3 | 47 | 3 | 0 | 3 | 0 | 0 |
| packages/sandbox | 3 | 30 | 3 | 0 | 3 | 0 | 0 |
| packages/shared | 2 | 10 | 1 | 0 | 1 | 1 | 0 |

## Source File Inventory

| Package | Upstream source file | Classification | Rust owner/exclusion | Notes |
| --- | --- | --- | --- | --- |
| apps/web | apps/web/app/[username]/[repo]/loading.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/[username]/[repo]/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/[username]/loading.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/[username]/og/route.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/[username]/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/api/auth/[...all]/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/auth/info/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/chat/_lib/chat-context.ts | portable | open-agents-service | Chat start, stop, streaming, request parsing, model selection, and tool-result persistence. |
| apps/web | apps/web/app/api/chat/_lib/model-selection.ts | portable | open-agents-service | Chat start, stop, streaming, request parsing, model selection, and tool-result persistence. |
| apps/web | apps/web/app/api/chat/_lib/persist-tool-results.ts | portable | open-agents-service | Chat start, stop, streaming, request parsing, model selection, and tool-result persistence. |
| apps/web | apps/web/app/api/chat/_lib/request.ts | portable | open-agents-service | Chat start, stop, streaming, request parsing, model selection, and tool-result persistence. |
| apps/web | apps/web/app/api/chat/_lib/runtime.ts | portable | open-agents-service | Chat start, stop, streaming, request parsing, model selection, and tool-result persistence. |
| apps/web | apps/web/app/api/chat/[chatId]/stop/route.ts | portable | open-agents-service | Chat start, stop, streaming, request parsing, model selection, and tool-result persistence. |
| apps/web | apps/web/app/api/chat/[chatId]/stream/route.ts | portable | open-agents-service | Chat start, stop, streaming, request parsing, model selection, and tool-result persistence. |
| apps/web | apps/web/app/api/chat/route.ts | portable | open-agents-service | Chat start, stop, streaming, request parsing, model selection, and tool-result persistence. |
| apps/web | apps/web/app/api/generate-pr/_lib/generate-pr-helpers.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/app/api/generate-pr/route.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/app/api/generate-title/route.ts | portable | open-agents-runtime | Model catalog, model variants, provider options, and access policy selection. |
| apps/web | apps/web/app/api/github/app/callback/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/github/app/install/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/github/branches/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/github/connection-status/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/github/create-repo/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/github/installations/repos/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/github/installations/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/github/orgs/install-status/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/github/orgs/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/github/post-link/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/github/user/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/github/webhook/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/models/route.ts | portable | open-agents-runtime | Model catalog, model variants, provider options, and access policy selection. |
| apps/web | apps/web/app/api/sandbox/activity/route.ts | portable | open-agents-service/open-agents-sandbox | Sandbox provision, reconnect, snapshot, status, and lifecycle API semantics. |
| apps/web | apps/web/app/api/sandbox/extend/route.ts | portable | open-agents-service/open-agents-sandbox | Sandbox provision, reconnect, snapshot, status, and lifecycle API semantics. |
| apps/web | apps/web/app/api/sandbox/reconnect/route.ts | portable | open-agents-service/open-agents-sandbox | Sandbox provision, reconnect, snapshot, status, and lifecycle API semantics. |
| apps/web | apps/web/app/api/sandbox/route.ts | portable | open-agents-service/open-agents-sandbox | Sandbox provision, reconnect, snapshot, status, and lifecycle API semantics. |
| apps/web | apps/web/app/api/sandbox/snapshot/route.ts | portable | open-agents-service/open-agents-sandbox | Sandbox provision, reconnect, snapshot, status, and lifecycle API semantics. |
| apps/web | apps/web/app/api/sandbox/status/route.ts | portable | open-agents-service/open-agents-sandbox | Sandbox provision, reconnect, snapshot, status, and lifecycle API semantics. |
| apps/web | apps/web/app/api/sessions/_lib/session-context.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/[chatId]/fork/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/[chatId]/messages/[messageId]/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/[chatId]/messages/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/[chatId]/read/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/[chatId]/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/[chatId]/share/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/checks/fix/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/code-editor/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/dev-server/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/diff/_lib/diff-utils.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/diff/cached/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/diff/patch/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/diff/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/files/content/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/files/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/generate-commit-message/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/share/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/[sessionId]/skills/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/sessions/route.ts | portable | open-agents-service/open-agents-persistence | Session, chat, message, read/share/fork, skill, file, diff, and dev-server state APIs. |
| apps/web | apps/web/app/api/settings/model-variants/route.ts | portable | open-agents-runtime | Model catalog, model variants, provider options, and access policy selection. |
| apps/web | apps/web/app/api/settings/preferences/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/shared/[shareId]/markdown/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/shared/[shareId]/status/get-shared-chat-status.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/shared/[shareId]/status/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/transcribe/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/usage/_lib/query-range.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/usage/rank/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/usage/route.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/app/api/vercel/projects/[idOrName]/env/route.ts | portable | open-agents-service | Deployment-time Vercel project/env lookup and operator configuration. |
| apps/web | apps/web/app/api/vercel/repo-projects/route.ts | portable | open-agents-service | Deployment-time Vercel project/env lookup and operator configuration. |
| apps/web | apps/web/app/codespace/[sessionId]/codespace-context.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/codespace/[sessionId]/layout.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/codespace/[sessionId]/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/config.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/app/deploy-your-own/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/get-started/get-started-flow.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/get-started/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/home-page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/layout.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/lib/render-tool.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/opengraph-image.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/providers.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/chat-sidebar.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/chat-tabs.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/code-editor-menu-items.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/commit-action-button.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/dev-server-menu-items.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/diff-tab-view.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/diff-viewer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/download-diff-dialog.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/error.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/file-tab-view.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/file-tree.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/git-panel-context.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/git-panel.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/hooks/use-auto-commit-status.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/hooks/use-code-editor.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/hooks/use-dev-server.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/hooks/use-session-chat-runtime.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/hooks/use-stream-recovery.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/loading.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/not-found.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/only-chat-in-session.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/sandbox-create-error-banner.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/sandbox-create.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/session-chat-content.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/session-chat-context.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/session-header.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/stream-recovery-policy.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/workspace-file-viewer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/layout.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/loading.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/session-layout-context.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/[sessionId]/session-layout-shell.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/layout.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/sessions-index-shell.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/sessions-route-shell.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/sessions/sessions-shell-context.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/accounts-section.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/accounts/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/admin/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/connections/loading.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/connections/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/layout.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/leaderboard-section.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/leaderboard/loading.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/leaderboard/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/loading.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/model-variants-section.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/models/loading.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/models/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/preferences-section.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/preferences/loading.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/preferences/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/profile-section.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/profile/loading.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/profile/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/usage-section.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/usage/domain-usage-leaderboard-section.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/usage/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/usage/usage-insights-section.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/settings/vercel-section.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/shared/[shareId]/error.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/shared/[shareId]/loading.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/shared/[shareId]/opengraph-image.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/shared/[shareId]/page.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/shared/[shareId]/redact-shared-env-content.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/shared/[shareId]/shared-chat-content.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/shared/[shareId]/shared-chat-status-utils.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/shared/[shareId]/shared-chat-status.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/shared/[shareId]/twitter-image.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/twitter-image.tsx | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/app/types.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/app/u/[username]/og/route.tsx | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/app/u/[username]/page.tsx | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/app/workflows/chat-post-finish.ts | portable | open-agents-service | Durable workflow orchestration maps to the Slack service local runtime and workflow bridge. |
| apps/web | apps/web/app/workflows/chat-sandbox-runtime.ts | portable | open-agents-service | Durable workflow orchestration maps to the Slack service local runtime and workflow bridge. |
| apps/web | apps/web/app/workflows/chat.ts | portable | open-agents-service | Durable workflow orchestration maps to the Slack service local runtime and workflow bridge. |
| apps/web | apps/web/app/workflows/gateway-metadata.ts | portable | open-agents-service | Durable workflow orchestration maps to the Slack service local runtime and workflow bridge. |
| apps/web | apps/web/app/workflows/sandbox-lifecycle.ts | portable | open-agents-service | Durable workflow orchestration maps to the Slack service local runtime and workflow bridge. |
| apps/web | apps/web/app/workflows/sandbox-provisioning.ts | portable | open-agents-service | Durable workflow orchestration maps to the Slack service local runtime and workflow bridge. |
| apps/web | apps/web/app/workflows/usage-utils.ts | portable | open-agents-service | Durable workflow orchestration maps to the Slack service local runtime and workflow bridge. |
| apps/web | apps/web/components/assistant-file-link.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/assistant-message-groups.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/auth/auth-guard.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/auth/hero-app-mockup.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/auth/hero-icons.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/auth/sign-in-button.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/auth/signed-out-hero.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/branch-picker-dialog.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/branch-selector-compact.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/branch-selector.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/chat-switcher-dropdown.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/close-pr-dialog.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/commit-dialog.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/contribution-chart.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/create-pr-dialog.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/create-repo-dialog.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/diffs-provider.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/file-suggestions-dropdown.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/file-type-icons.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/github-reconnect-dialog.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/github-reconnect-gate.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/home-skeleton.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/image-attachments-preview.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/inbox-sidebar-rename-dialog.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/inbox-sidebar-rename.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/inbox-sidebar.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/inline-question-input.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/app-mockup.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/bento.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/feature-agent.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/feature-sandbox.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/feature-workflow.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/features.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/footer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/github-link.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/logo.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/nav.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/stage.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/terminal.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/theme-toggle.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/landing/window.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/merge-check-runs.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/merge-pr-dialog-actions.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/merge-pr-dialog.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/message-model-pill.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/model-combobox.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/model-selector-compact.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/new-session-dialog.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/pinned-todo-panel.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/provider-icons.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/repo-selection-screen.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/repo-selector-compact.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/repo-selector.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/sandbox-selector-compact.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/selection-popover.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/session-drawer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/session-list.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/session-starter-vercel-sync-section.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/session-starter.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/slash-command-dropdown.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/snippet-chip.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/task-group-view.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/text-attachments-preview.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/thinking-block.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/approval-buttons.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/file-name-pill.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/index.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/open-file-context.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/renderers/ask-user-question-renderer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/renderers/bash-renderer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/renderers/edit-renderer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/renderers/fetch-renderer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/renderers/glob-renderer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/renderers/grep-renderer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/renderers/read-renderer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/renderers/skill-renderer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/renderers/task-renderer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/renderers/todo-renderer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/renderers/write-renderer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/tool-call.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-call/tool-layout.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/tool-calls-summary-bar.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/avatar.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/button-group.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/button.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/calendar.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/card.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/command.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/date-range-picker.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/dialog.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/drawer.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/dropdown-menu.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/empty.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/field.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/input-group.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/input.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/label.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/popover.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/scroll-area.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/select.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/separator.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/sheet.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/sidebar.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/skeleton.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/switch.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/table.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/tabs.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/textarea.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/ui/tooltip.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/components/user-avatar-dropdown.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/drizzle.config.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/hooks/use-audio-recording.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-background-chat-notifications.tsx | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-file-suggestions.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-github-connection-status.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-image-attachments.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-installation-repos.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-leaderboard-rank.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-mobile.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-model-options.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-scroll-to-bottom.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-session-chats.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-session-diff.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-session-files.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-session-git-status.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-session-skills.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-session.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-sessions.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-slash-commands.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-text-attachments.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-user-preferences.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/hooks/use-vercel-repo-projects.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/instrumentation-client.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/abortable-chat-transport.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/lib/admin/actions.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/assistant-file-links.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/lib/auth/actions.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/auth/client.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/auth/config.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/auth/username.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/botid.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/chat-auto-commit.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/lib/chat-instance-manager.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/lib/chat-route-cleanup.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/lib/chat-streaming-state.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/lib/chat/auto-commit-direct.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/lib/chat/auto-pr-direct.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/lib/chat/create-cancelable-readable-stream.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/lib/chat/dedupe-message-reasoning.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/lib/db/client.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/installations.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/last-repo.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/migrate.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/public-usage-profile.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/schema.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/sessions-cache.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/sessions.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/usage-domain-leaderboard.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/usage-insights.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/usage.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/user-preferences.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/users.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/vercel-project-links.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/db/workflow-runs.ts | portable | open-agents-persistence | Durable session, chat, usage, workflow-run, installation, and idempotency storage concepts. |
| apps/web | apps/web/lib/deployment/resource-profile.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/diff/compute-diff.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/lib/diff/download-diff.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/lib/diffs-config.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/file-suggestions.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/format-relative-time.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/lib/git/actions/branch.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/git/actions/discard.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/git/branches.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/git/helpers.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/git/queries/status.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/github/access.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/actions/commit.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/actions/connection.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/actions/pr.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/app.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/client.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/commit-intent.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/commit.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/pr-content.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/pulls.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/queries/deployment.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/queries/pr.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/repos.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/status.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/sync.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/token.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/urls.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/github/users.ts | portable | open-agents-sandbox | GitHub commit, PR, repository, token, readiness, and deployment-polling automation. |
| apps/web | apps/web/lib/image-utils.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/managed-template-trial.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/lib/merge-readiness-polling.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/lib/model-access.ts | portable | open-agents-runtime | Model catalog, model variants, provider options, and access policy selection. |
| apps/web | apps/web/lib/model-availability.ts | portable | open-agents-runtime | Model catalog, model variants, provider options, and access policy selection. |
| apps/web | apps/web/lib/model-options.ts | portable | open-agents-runtime | Model catalog, model variants, provider options, and access policy selection. |
| apps/web | apps/web/lib/model-variants.ts | portable | open-agents-runtime | Model catalog, model variants, provider options, and access policy selection. |
| apps/web | apps/web/lib/models-with-context.ts | portable | open-agents-runtime | Model catalog, model variants, provider options, and access policy selection. |
| apps/web | apps/web/lib/models.ts | portable | open-agents-runtime | Model catalog, model variants, provider options, and access policy selection. |
| apps/web | apps/web/lib/onboarding.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/pr-deployment-polling.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/lib/random-city.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/rate-limit.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/redirect-safety.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/redis.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/lib/sandbox/archive-session.ts | portable | open-agents-service/open-agents-sandbox | Sandbox lifecycle, provisioning kick, archive, and configuration behavior. |
| apps/web | apps/web/lib/sandbox/config.ts | portable | open-agents-service/open-agents-sandbox | Sandbox lifecycle, provisioning kick, archive, and configuration behavior. |
| apps/web | apps/web/lib/sandbox/home-directory.ts | portable | open-agents-service/open-agents-sandbox | Sandbox lifecycle, provisioning kick, archive, and configuration behavior. |
| apps/web | apps/web/lib/sandbox/lifecycle-kick.ts | portable | open-agents-service/open-agents-sandbox | Sandbox lifecycle, provisioning kick, archive, and configuration behavior. |
| apps/web | apps/web/lib/sandbox/lifecycle.ts | portable | open-agents-service/open-agents-sandbox | Sandbox lifecycle, provisioning kick, archive, and configuration behavior. |
| apps/web | apps/web/lib/sandbox/provisioning-kick.ts | portable | open-agents-service/open-agents-sandbox | Sandbox lifecycle, provisioning kick, archive, and configuration behavior. |
| apps/web | apps/web/lib/sandbox/provisioning.ts | portable | open-agents-service/open-agents-sandbox | Sandbox lifecycle, provisioning kick, archive, and configuration behavior. |
| apps/web | apps/web/lib/sandbox/utils.ts | portable | open-agents-service/open-agents-sandbox | Sandbox lifecycle, provisioning kick, archive, and configuration behavior. |
| apps/web | apps/web/lib/session/get-server-session.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/lib/session/server.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/lib/session/types.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/lib/skills-cache.ts | portable | ai-sdk-rust::skills | Global/project skill discovery, cache, refs, and installation semantics. |
| apps/web | apps/web/lib/skills/directories.ts | portable | ai-sdk-rust::skills | Global/project skill discovery, cache, refs, and installation semantics. |
| apps/web | apps/web/lib/skills/global-skill-installer.ts | portable | ai-sdk-rust::skills | Global/project skill discovery, cache, refs, and installation semantics. |
| apps/web | apps/web/lib/skills/global-skill-refs.ts | portable | ai-sdk-rust::skills | Global/project skill discovery, cache, refs, and installation semantics. |
| apps/web | apps/web/lib/streamdown-config.tsx | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/lib/swr.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/text-attachment-utils.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/usage/compute-insights.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/lib/usage/date-range.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/lib/usage/leaderboard-domain.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/lib/usage/types.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/lib/utils.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/vercel-themes.ts | js-only-documented | excluded: js-only-documented | Web app account, settings, sharing, browser, or hosting helper not exposed by the Slack-first Rust port. |
| apps/web | apps/web/lib/vercel/projects.ts | portable | open-agents-service | Deployment-time Vercel project/env lookup and operator configuration. |
| apps/web | apps/web/lib/vercel/token.ts | portable | open-agents-service | Deployment-time Vercel project/env lookup and operator configuration. |
| apps/web | apps/web/lib/vercel/types.ts | portable | open-agents-service | Deployment-time Vercel project/env lookup and operator configuration. |
| apps/web | apps/web/lib/workspace-status-store.ts | portable | open-agents-runtime/open-agents-service | Runtime cancellation, stream state, finish actions, usage, and workspace status behavior. |
| apps/web | apps/web/next.config.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/proxy.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| apps/web | apps/web/scripts/check-migrations.ts | js-only-documented | excluded: js-only-documented | Open Agents web application surface; no direct Rust Slack release counterpart. |
| apps/web | apps/web/shiki-custom-themes.d.ts | js-only-documented | excluded: js-only-documented | Next.js/React browser UI, routing, or client instrumentation excluded from the Slack-first Rust port. |
| packages/agent | packages/agent/context-management/aggressive-compaction-helpers.ts | portable | ai-sdk-rust::subagents | Subagent profiles, inherited context, usage folding, and cache-control equivalents. |
| packages/agent | packages/agent/context-management/cache-control.ts | portable | ai-sdk-rust::subagents | Subagent profiles, inherited context, usage folding, and cache-control equivalents. |
| packages/agent | packages/agent/context-management/index.ts | portable | ai-sdk-rust::subagents | Subagent profiles, inherited context, usage folding, and cache-control equivalents. |
| packages/agent | packages/agent/index.ts | portable | open-agents-runtime | Open Agent model selection, system prompt, runtime adapter, and usage hooks. |
| packages/agent | packages/agent/models.ts | portable | open-agents-runtime | Open Agent model selection, system prompt, runtime adapter, and usage hooks. |
| packages/agent | packages/agent/open-agent.ts | portable | open-agents-runtime | Open Agent model selection, system prompt, runtime adapter, and usage hooks. |
| packages/agent | packages/agent/skills/discovery.ts | portable | ai-sdk-rust::skills | Skill discovery, frontmatter, slash-command invocation, and loaded-skill prompts. |
| packages/agent | packages/agent/skills/index.ts | portable | ai-sdk-rust::skills | Skill discovery, frontmatter, slash-command invocation, and loaded-skill prompts. |
| packages/agent | packages/agent/skills/loader.ts | portable | ai-sdk-rust::skills | Skill discovery, frontmatter, slash-command invocation, and loaded-skill prompts. |
| packages/agent | packages/agent/skills/types.ts | portable | ai-sdk-rust::skills | Skill discovery, frontmatter, slash-command invocation, and loaded-skill prompts. |
| packages/agent | packages/agent/subagents/constants.ts | portable | ai-sdk-rust::subagents | Subagent profiles, inherited context, usage folding, and cache-control equivalents. |
| packages/agent | packages/agent/subagents/design.ts | portable | ai-sdk-rust::subagents | Subagent profiles, inherited context, usage folding, and cache-control equivalents. |
| packages/agent | packages/agent/subagents/executor.ts | portable | ai-sdk-rust::subagents | Subagent profiles, inherited context, usage folding, and cache-control equivalents. |
| packages/agent | packages/agent/subagents/explorer.ts | portable | ai-sdk-rust::subagents | Subagent profiles, inherited context, usage folding, and cache-control equivalents. |
| packages/agent | packages/agent/subagents/index.ts | portable | ai-sdk-rust::subagents | Subagent profiles, inherited context, usage folding, and cache-control equivalents. |
| packages/agent | packages/agent/subagents/registry.ts | portable | ai-sdk-rust::subagents | Subagent profiles, inherited context, usage folding, and cache-control equivalents. |
| packages/agent | packages/agent/subagents/types.ts | portable | ai-sdk-rust::subagents | Subagent profiles, inherited context, usage folding, and cache-control equivalents. |
| packages/agent | packages/agent/system-prompt.ts | portable | open-agents-runtime | Open Agent model selection, system prompt, runtime adapter, and usage hooks. |
| packages/agent | packages/agent/tools/ask-user-question.ts | portable | ai-sdk-rust::open_agents_tools | Agent tool definitions, approvals, path security, and sandbox-bound execution. |
| packages/agent | packages/agent/tools/bash.ts | portable | ai-sdk-rust::open_agents_tools | Agent tool definitions, approvals, path security, and sandbox-bound execution. |
| packages/agent | packages/agent/tools/fetch.ts | portable | ai-sdk-rust::open_agents_tools | Agent tool definitions, approvals, path security, and sandbox-bound execution. |
| packages/agent | packages/agent/tools/glob.ts | portable | ai-sdk-rust::open_agents_tools | Agent tool definitions, approvals, path security, and sandbox-bound execution. |
| packages/agent | packages/agent/tools/grep.ts | portable | ai-sdk-rust::open_agents_tools | Agent tool definitions, approvals, path security, and sandbox-bound execution. |
| packages/agent | packages/agent/tools/index.ts | portable | ai-sdk-rust::open_agents_tools | Agent tool definitions, approvals, path security, and sandbox-bound execution. |
| packages/agent | packages/agent/tools/path-security.ts | portable | ai-sdk-rust::open_agents_tools | Agent tool definitions, approvals, path security, and sandbox-bound execution. |
| packages/agent | packages/agent/tools/read.ts | portable | ai-sdk-rust::open_agents_tools | Agent tool definitions, approvals, path security, and sandbox-bound execution. |
| packages/agent | packages/agent/tools/skill.ts | portable | ai-sdk-rust::open_agents_tools | Agent tool definitions, approvals, path security, and sandbox-bound execution. |
| packages/agent | packages/agent/tools/task.ts | portable | ai-sdk-rust::open_agents_tools | Agent tool definitions, approvals, path security, and sandbox-bound execution. |
| packages/agent | packages/agent/tools/todo.ts | portable | ai-sdk-rust::open_agents_tools | Agent tool definitions, approvals, path security, and sandbox-bound execution. |
| packages/agent | packages/agent/tools/utils.ts | portable | ai-sdk-rust::open_agents_tools | Agent tool definitions, approvals, path security, and sandbox-bound execution. |
| packages/agent | packages/agent/tools/write.ts | portable | ai-sdk-rust::open_agents_tools | Agent tool definitions, approvals, path security, and sandbox-bound execution. |
| packages/agent | packages/agent/types.ts | portable | open-agents-runtime | Open Agent model selection, system prompt, runtime adapter, and usage hooks. |
| packages/agent | packages/agent/usage.ts | portable | open-agents-runtime | Open Agent model selection, system prompt, runtime adapter, and usage hooks. |
| packages/sandbox | packages/sandbox/factory.ts | portable | open-agents-sandbox | Typed sandbox boundary, Vercel backend, file/exec/snapshot/state, and git helpers. |
| packages/sandbox | packages/sandbox/git.ts | portable | open-agents-sandbox | Typed sandbox boundary, Vercel backend, file/exec/snapshot/state, and git helpers. |
| packages/sandbox | packages/sandbox/index.ts | portable | open-agents-sandbox | Typed sandbox boundary, Vercel backend, file/exec/snapshot/state, and git helpers. |
| packages/sandbox | packages/sandbox/interface.ts | portable | open-agents-sandbox | Typed sandbox boundary, Vercel backend, file/exec/snapshot/state, and git helpers. |
| packages/sandbox | packages/sandbox/types.ts | portable | open-agents-sandbox | Typed sandbox boundary, Vercel backend, file/exec/snapshot/state, and git helpers. |
| packages/sandbox | packages/sandbox/vercel/config.ts | portable | open-agents-sandbox | Typed sandbox boundary, Vercel backend, file/exec/snapshot/state, and git helpers. |
| packages/sandbox | packages/sandbox/vercel/connect.ts | portable | open-agents-sandbox | Typed sandbox boundary, Vercel backend, file/exec/snapshot/state, and git helpers. |
| packages/sandbox | packages/sandbox/vercel/index.ts | portable | open-agents-sandbox | Typed sandbox boundary, Vercel backend, file/exec/snapshot/state, and git helpers. |
| packages/sandbox | packages/sandbox/vercel/sandbox.ts | portable | open-agents-sandbox | Typed sandbox boundary, Vercel backend, file/exec/snapshot/state, and git helpers. |
| packages/sandbox | packages/sandbox/vercel/snapshot-refresh.ts | portable | open-agents-sandbox | Typed sandbox boundary, Vercel backend, file/exec/snapshot/state, and git helpers. |
| packages/sandbox | packages/sandbox/vercel/state.ts | portable | open-agents-sandbox | Typed sandbox boundary, Vercel backend, file/exec/snapshot/state, and git helpers. |
| packages/sandbox | packages/sandbox/vercel/utils.ts | portable | open-agents-sandbox | Typed sandbox boundary, Vercel backend, file/exec/snapshot/state, and git helpers. |
| packages/shared | packages/shared/hooks/expanded-view-context.tsx | js-only-documented | excluded: js-only-documented | Shared React/web helper; Slack release does not expose this browser UI surface. |
| packages/shared | packages/shared/hooks/reasoning-context.tsx | js-only-documented | excluded: js-only-documented | Shared React/web helper; Slack release does not expose this browser UI surface. |
| packages/shared | packages/shared/hooks/todo-view-context.tsx | js-only-documented | excluded: js-only-documented | Shared React/web helper; Slack release does not expose this browser UI surface. |
| packages/shared | packages/shared/index.ts | js-only-documented | excluded: js-only-documented | Shared React/web helper; Slack release does not expose this browser UI surface. |
| packages/shared | packages/shared/lib/diff.ts | portable | open-agents-sandbox | Diff formatting and file-change summaries map to sandbox git automation. |
| packages/shared | packages/shared/lib/paste-blocks.ts | js-only-documented | excluded: js-only-documented | Shared React/web helper; Slack release does not expose this browser UI surface. |
| packages/shared | packages/shared/lib/tool-state.ts | portable | open-agents-slack | Tool state labels and approval status map to Slack outbound status rendering. |
| scripts | scripts/test-isolated.ts | js-only-documented | excluded: js-only-documented | Bun/Next test harness helper with no Rust runtime surface. |
| scripts | scripts/vercel-refresh-base-snapshot.ts | portable | open-agents-sandbox | Base snapshot refresh maps to sandbox snapshot setup and ignored live proof. |

## Test File Inventory

| Package | Upstream test file | Case calls | Portability | Rust owner/exclusion | Named Rust test or marker | Status | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| apps/web | apps/web/app/api/auth/info/route.test.ts | 6 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Better Auth and browser session identity are web-only; Slack user/team identity is the Rust release surface. |
| apps/web | apps/web/app/api/chat/_lib/model-selection.test.ts | 5 | portable | open-agents-service | open_agent_prepare_composes_prompt_context_model_and_tools | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/app/api/chat/_lib/persist-tool-results.test.ts | 6 | portable | open-agents-service | pending: owner mapped; no named Rust test yet | in-progress | Remote-agent chat, stream, stop, persistence, and finish behavior maps to Open Agents runtime/service. |
| apps/web | apps/web/app/api/chat/[chatId]/stop/route.test.ts | 12 | portable | open-agents-service | block_action_cancel_cancels_waiting_run | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/app/api/chat/[chatId]/stream/route.test.ts | 12 | portable | open-agents-service | block_action_answer_resumes_waiting_run_to_completion | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/app/api/chat/route.test.ts | 23 | portable | open-agents-service | app_mention_accepts_persists_run_and_records_outbound | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/app/api/generate-pr/_lib/generate-pr-helpers.test.ts | 6 | portable | open-agents-sandbox | pending: owner mapped; no named Rust test yet | in-progress | Git/PR behavior maps to sandbox git automation and finish actions; remaining web-specific fixtures need owner review. |
| apps/web | apps/web/app/api/generate-title/route.test.ts | 5 | portable | open-agents-runtime | pending: owner mapped; no named Rust test yet | in-progress | Portable Open Agents behavior is assigned to the Rust owner; no named parity test exists yet. |
| apps/web | apps/web/app/api/github/app/callback/route.test.ts | 3 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | GitHub web app installation/token UI is web-only unless mapped by a separate sandbox git automation row. |
| apps/web | apps/web/app/api/github/app/install/route.test.ts | 3 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | GitHub web app installation/token UI is web-only unless mapped by a separate sandbox git automation row. |
| apps/web | apps/web/app/api/github/connection-status/route.test.ts | 7 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/api/github/create-repo/route.test.ts | 3 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/api/models/route.test.ts | 5 | portable | open-agents-runtime | pending: owner mapped; no named Rust test yet | in-progress | Portable Open Agents behavior is assigned to the Rust owner; no named parity test exists yet. |
| apps/web | apps/web/app/api/sandbox/reconnect/route.test.ts | 3 | portable | open-agents-service/open-agents-sandbox | sandbox_vercel_state_serializes_upstream_factory_shape | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/app/api/sandbox/route.test.ts | 7 | portable | open-agents-service/open-agents-sandbox | vercel_sandbox_backend_connects_execs_reads_writes_lists_and_stops | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/app/api/sandbox/snapshot/route.test.ts | 4 | portable | open-agents-service/open-agents-sandbox | vercel_client_extend_timeout_and_snapshot_parse_session_updates | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/app/api/sandbox/status/route.test.ts | 2 | portable | open-agents-service/open-agents-sandbox | sandbox_context_round_trips_with_optional_fields | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/app/api/sessions/_lib/session-context.test.ts | 12 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/[chatId]/fork/route.test.ts | 9 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/[chatId]/messages/[messageId]/route.test.ts | 7 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/[chatId]/messages/route.test.ts | 6 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/[chatId]/read/route.test.ts | 3 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/[chatId]/route.test.ts | 10 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/[chatId]/share/route.test.ts | 7 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/[sessionId]/chats/route.test.ts | 7 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/[sessionId]/code-editor/route.test.ts | 6 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/[sessionId]/dev-server/route.test.ts | 7 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/[sessionId]/diff/_lib/diff-utils.test.ts | 13 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/[sessionId]/files/content/route.test.ts | 6 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/[sessionId]/share/route.test.ts | 2 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/[sessionId]/skills/route.test.ts | 2 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/sessions/route.test.ts | 9 | portable | open-agents-service/open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/app/api/settings/model-variants/route.test.ts | 15 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/api/settings/preferences/route.test.ts | 13 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/api/shared/[shareId]/markdown/route.test.ts | 3 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/api/shared/[shareId]/status/route.test.ts | 4 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/app/api/vercel/projects/[idOrName]/env/route.test.ts | 1 | portable | open-agents-service | pending: owner mapped; no named Rust test yet | in-progress | Portable Open Agents behavior is assigned to the Rust owner; no named parity test exists yet. |
| apps/web | apps/web/app/api/vercel/repo-projects/route.test.ts | 3 | portable | open-agents-service | pending: owner mapped; no named Rust test yet | in-progress | Portable Open Agents behavior is assigned to the Rust owner; no named parity test exists yet. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/page.test.ts | 3 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | React/Next page, component, hook, or browser route with no Slack-first Rust UI counterpart. |
| apps/web | apps/web/app/sessions/[sessionId]/chats/[chatId]/stream-recovery-policy.test.ts | 16 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | React/Next page, component, hook, or browser route with no Slack-first Rust UI counterpart. |
| apps/web | apps/web/app/shared/[shareId]/page.test.ts | 10 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | React/Next page, component, hook, or browser route with no Slack-first Rust UI counterpart. |
| apps/web | apps/web/app/workflows/chat-post-finish-usage.test.ts | 8 | portable | open-agents-service | open_agent_generate_records_usage_from_fake_model; run_usage_contract | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/app/workflows/chat-post-finish.test.ts | 27 | portable | open-agents-service | finish_builds_pr_command_in_dry_run_mode | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/app/workflows/chat.test.ts | 37 | portable | open-agents-service | app_mention_accepts_persists_run_and_records_outbound | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/components/inbox-sidebar-rename.test.ts | 7 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | React/Next page, component, hook, or browser route with no Slack-first Rust UI counterpart. |
| apps/web | apps/web/components/pinned-todo-panel.test.ts | 2 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | React/Next page, component, hook, or browser route with no Slack-first Rust UI counterpart. |
| apps/web | apps/web/components/tool-call/renderers/bash-renderer.test.tsx | 2 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | React/Next page, component, hook, or browser route with no Slack-first Rust UI counterpart. |
| apps/web | apps/web/components/tool-call/tool-layout.test.tsx | 6 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | React/Next page, component, hook, or browser route with no Slack-first Rust UI counterpart. |
| apps/web | apps/web/hooks/use-background-chat-notifications.test.ts | 12 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | React/Next page, component, hook, or browser route with no Slack-first Rust UI counterpart. |
| apps/web | apps/web/hooks/use-session-chats.test.ts | 9 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | React/Next page, component, hook, or browser route with no Slack-first Rust UI counterpart. |
| apps/web | apps/web/instrumentation-client.test.ts | 1 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/assistant-file-links.test.ts | 4 | portable | open-agents-slack | pending: owner mapped; no named Rust test yet | in-progress | Portable Open Agents behavior is assigned to the Rust owner; no named parity test exists yet. |
| apps/web | apps/web/lib/auth/username.test.ts | 3 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Better Auth and browser session identity are web-only; Slack user/team identity is the Rust release surface. |
| apps/web | apps/web/lib/chat-auto-commit.test.ts | 3 | portable | open-agents-runtime/open-agents-service | pending: owner mapped; no named Rust test yet | in-progress | Remote-agent chat, stream, stop, persistence, and finish behavior maps to Open Agents runtime/service. |
| apps/web | apps/web/lib/chat-route-cleanup.test.ts | 3 | portable | open-agents-runtime/open-agents-service | pending: owner mapped; no named Rust test yet | in-progress | Remote-agent chat, stream, stop, persistence, and finish behavior maps to Open Agents runtime/service. |
| apps/web | apps/web/lib/chat-streaming-state.test.ts | 14 | portable | open-agents-runtime/open-agents-service | pending: owner mapped; no named Rust test yet | in-progress | Remote-agent chat, stream, stop, persistence, and finish behavior maps to Open Agents runtime/service. |
| apps/web | apps/web/lib/chat/auto-commit-direct.test.ts | 8 | portable | open-agents-runtime/open-agents-sandbox | finish_commits_dirty_repository | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/lib/chat/auto-pr-direct.test.ts | 9 | portable | open-agents-runtime/open-agents-sandbox | finish_builds_pr_command_in_dry_run_mode | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/lib/chat/create-cancelable-readable-stream.test.ts | 10 | portable | open-agents-runtime/open-agents-sandbox | pending: owner mapped; no named Rust test yet | in-progress | Remote-agent chat, stream, stop, persistence, and finish behavior maps to Open Agents runtime/service. |
| apps/web | apps/web/lib/chat/dedupe-message-reasoning.test.ts | 9 | portable | open-agents-runtime/open-agents-sandbox | pending: owner mapped; no named Rust test yet | in-progress | Remote-agent chat, stream, stop, persistence, and finish behavior maps to Open Agents runtime/service. |
| apps/web | apps/web/lib/db/public-usage-profile.test.ts | 6 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/db/sessions.test.ts | 9 | portable | open-agents-persistence | pending: owner mapped; no named Rust test yet | in-progress | Durable session/chat/message behavior maps to Rust service and persistence contracts; named parity tests remain future work. |
| apps/web | apps/web/lib/db/usage-domain-leaderboard.test.ts | 4 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/db/user-preferences.test.ts | 8 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/diff/compute-diff.test.ts | 10 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/diff/download-diff.test.ts | 4 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/github/client.test.ts | 3 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | GitHub web app installation/token UI is web-only unless mapped by a separate sandbox git automation row. |
| apps/web | apps/web/lib/github/commit-intent.test.ts | 4 | portable | open-agents-sandbox | pending: owner mapped; no named Rust test yet | in-progress | Git/PR behavior maps to sandbox git automation and finish actions; remaining web-specific fixtures need owner review. |
| apps/web | apps/web/lib/github/commit.test.ts | 2 | portable | open-agents-sandbox | finish_commits_dirty_repository | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/lib/github/installation-repos.test.ts | 2 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | GitHub web app installation/token UI is web-only unless mapped by a separate sandbox git automation row. |
| apps/web | apps/web/lib/github/installations-sync.test.ts | 3 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | GitHub web app installation/token UI is web-only unless mapped by a separate sandbox git automation row. |
| apps/web | apps/web/lib/github/pr-content.test.ts | 4 | portable | open-agents-sandbox | finish_builds_pr_command_in_dry_run_mode | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/lib/github/repo-identifiers.test.ts | 4 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | GitHub web app installation/token UI is web-only unless mapped by a separate sandbox git automation row. |
| apps/web | apps/web/lib/github/token.test.ts | 3 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | GitHub web app installation/token UI is web-only unless mapped by a separate sandbox git automation row. |
| apps/web | apps/web/lib/merge-readiness-polling.test.ts | 7 | portable | open-agents-runtime/open-agents-service | pending: owner mapped; no named Rust test yet | in-progress | Portable Open Agents behavior is assigned to the Rust owner; no named parity test exists yet. |
| apps/web | apps/web/lib/model-access.test.ts | 5 | portable | open-agents-runtime | pending: owner mapped; no named Rust test yet | in-progress | Portable Open Agents behavior is assigned to the Rust owner; no named parity test exists yet. |
| apps/web | apps/web/lib/model-availability.test.ts | 3 | portable | open-agents-runtime | pending: owner mapped; no named Rust test yet | in-progress | Portable Open Agents behavior is assigned to the Rust owner; no named parity test exists yet. |
| apps/web | apps/web/lib/model-options.test.ts | 8 | portable | open-agents-runtime | pending: owner mapped; no named Rust test yet | in-progress | Portable Open Agents behavior is assigned to the Rust owner; no named parity test exists yet. |
| apps/web | apps/web/lib/model-variants.test.ts | 11 | portable | open-agents-runtime | pending: owner mapped; no named Rust test yet | in-progress | Portable Open Agents behavior is assigned to the Rust owner; no named parity test exists yet. |
| apps/web | apps/web/lib/models.test.ts | 2 | portable | open-agents-runtime | pending: owner mapped; no named Rust test yet | in-progress | Portable Open Agents behavior is assigned to the Rust owner; no named parity test exists yet. |
| apps/web | apps/web/lib/pr-deployment-polling.test.ts | 7 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/random-city.test.ts | 8 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/rate-limit.test.ts | 6 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/redis.test.ts | 7 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/sandbox/archive-session.test.ts | 3 | portable | open-agents-service/open-agents-sandbox | sandbox_lifecycle_contract | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/lib/sandbox/lifecycle-evaluate.test.ts | 4 | portable | open-agents-service/open-agents-sandbox | connect_options_debug_redacts_credentials_and_env_values | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/lib/sandbox/lifecycle-kick.test.ts | 2 | portable | open-agents-service/open-agents-sandbox | pending: owner mapped; no named Rust test yet | in-progress | Sandbox lifecycle/provisioning behavior maps to open-agents-sandbox and service wiring; remaining cases are owner-mapped. |
| apps/web | apps/web/lib/sandbox/lifecycle.test.ts | 4 | portable | open-agents-service/open-agents-sandbox | sandbox_state_serializes_and_reconnects_local_workspace | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/lib/skills-cache.test.ts | 3 | portable | ai-sdk-rust::skills | discovers_project_claude_and_global_skills_with_skip_diagnostics | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/lib/skills/global-skill-installer.test.ts | 2 | portable | ai-sdk-rust::skills | invoke_skill_injects_directory_and_substitutes_arguments | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/lib/skills/global-skill-refs.test.ts | 3 | portable | ai-sdk-rust::skills | loaded_skill_allowed_tools_are_deduplicated | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/lib/streamdown-config.test.ts | 1 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/swr.test.ts | 5 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/usage/compute-insights.test.ts | 2 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/usage/date-range.test.ts | 13 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/vercel/projects.test.ts | 5 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| apps/web | apps/web/lib/workspace-status-store.test.ts | 3 | portable | open-agents-runtime/open-agents-service | active_run_contract | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| apps/web | apps/web/proxy.test.ts | 4 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Web app browser, settings, analytics, Redis, share, or Vercel account helper excluded from the Slack-first Rust port. |
| packages/agent | packages/agent/models.test.ts | 18 | portable | open-agents-runtime | open_agent_prepare_composes_prompt_context_model_and_tools; open_agent_generate_records_usage_from_fake_model | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| packages/agent | packages/agent/tools/tools.test.ts | 24 | portable | ai-sdk-rust::open_agents_tools | open_agent_file_tools_read_write_and_edit_with_fake_sandbox; open_agent_search_bash_and_todo_tools_execute_with_fake_sandbox; open_agent_tool_schemas_serialize_for_tool_loop_agent | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| packages/agent | packages/agent/tools/utils.test.ts | 5 | portable | ai-sdk-rust::open_agents_tools | open_agent_path_security_blocks_escape_dotenv_and_symlink_escape | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| packages/sandbox | packages/sandbox/git.test.ts | 3 | portable | open-agents-sandbox | clone_branch_status_diff_and_commit_stay_inside_sandbox; finish_commits_dirty_repository | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| packages/sandbox | packages/sandbox/vercel/sandbox.test.ts | 24 | portable | open-agents-sandbox | vercel_sandbox_backend_connects_execs_reads_writes_lists_and_stops; vercel_client_create_sandbox_sends_upstream_shape | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| packages/sandbox | packages/sandbox/vercel/snapshot-refresh.test.ts | 3 | portable | open-agents-sandbox | vercel_client_extend_timeout_and_snapshot_parse_session_updates; live_vercel_sandbox_create_exec_read_write_list_stop_smoke | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
| packages/shared | packages/shared/lib/paste-blocks.test.ts | 4 | js-only-documented | excluded: js-only-documented | n/a | js-only-documented | Paste-token placeholders are browser text-entry helpers; Slack message ingestion uses platform payloads instead. |
| packages/shared | packages/shared/lib/tool-state.test.ts | 6 | portable | open-agents-slack | render_progress_update; render_run_terminal | in-progress | Portable upstream behavior has at least one named Rust counterpart; remaining case-level closure stays with the owning crate. |
