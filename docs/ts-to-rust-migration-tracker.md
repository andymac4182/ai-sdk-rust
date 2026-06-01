# TypeScript To Rust Migration Tracker

This tracker records the completed work to make the Rust workspace match the
public behavior and portable tests from these upstream TypeScript projects:

- `vercel-labs/open-agents`
- `vercel-labs/just-bash`
- `vercel/workflow`
- `vercel/chat`
- `vercel/ai`

The goal is parity, not compatibility by approximation. A TypeScript package,
file, or test case is closed only when the Rust workspace has an owned row in the
appropriate ledger, a named Rust test for every portable upstream case, and an
explicit exception for every JavaScript-only or TypeScript-type-system-only row.

## Hard Rules

- Refresh the upstream source before making parity claims:
  - `npx opensrc fetch https://github.com/vercel-labs/open-agents`
  - `npx opensrc fetch https://github.com/vercel-labs/just-bash`
  - `npx opensrc fetch https://github.com/vercel/workflow`
  - `npx opensrc fetch github:vercel/chat`
  - `npx opensrc fetch github:vercel/ai`
- Work in a dedicated Codex worktree branched from current `origin/main`.
- Keep ownership narrow. Do not take behavior from another bucket unless that
  bucket is complete or the tracker row is updated with the reason.
- Every completed thread must update its owning ledger or this tracker, run the
  verification commands for the row, commit the work, and merge back to
  `origin/main` through the repo merge lane.
- Use `/tmp/ai-sdk-rust-main-merge.lock` before rebasing or pushing a completed
  thread to `origin/main`.
- Do not count live-provider, Slack, Vercel, Docker, or credential-gated checks
  as normal CI requirements. Add ignored tests and document the exact env needed.

## Source Snapshot

Refreshed on 2026-06-01 with `npx opensrc fetch`; the Just Bash row was
refreshed on 2026-06-02.

| Project | Upstream HEAD | Local cache | Package manifests | TS/TSX files | Test files | Current Rust tracker |
| --- | --- | --- | ---: | ---: | ---: | --- |
| Open Agents | `24d679c7ba3d274aa73814c15673aeffcbe3c1c2` | `~/.opensrc/repos/github.com/vercel-labs/open-agents/main` | 6 | 567 | 106 | `docs/open-agents/*` |
| Just Bash | `d64009aef6bc1556e7c84b22ed455863275ea953` | `~/.opensrc/repos/github.com/vercel-labs/just-bash/main` | 8 | 908 | 485 | `docs/open-agents/just-bash-parity.md`, `docs/open-agents/just-bash-conformance.md` |
| Workflow SDK | `ae3c833acd4f44ab84db65b44eb2ba2646eaecf9` | `~/.opensrc/repos/github.com/vercel/workflow/main` | 47 | 1019 | 149 | `docs/workflow-upstream-parity.md`, `docs/workflow-test-inventory.md` |
| Chat SDK | `ffc43fcf1f7679164be0806308bea237113c7590` | `~/.opensrc/repos/github.com/vercel/chat/main` | 23 | 504 | 131 | `docs/chat/upstream-parity.md`, `docs/chat/package-progress.md` |
| AI SDK | `ab6d66482d31afe15f4973a51c5f7cfa09c92ea6` | `~/.opensrc/repos/github.com/vercel/ai/main` | 87 | 4161 | 688 | `docs/upstream-parity.md`, `docs/package-progress.md` |

## Current Completion Picture

| Project | Current state | Evidence | Required next proof |
| --- | --- | --- | --- |
| Open Agents | Current tracked rows are closed. The Rust service has Slack webhook ingress, native Vercel AI Gateway runtime config, Vercel sandbox support, deployment docs, and a generated upstream inventory covering all 6 package manifests, 461 source files, and 106 test files. OA-03B replaced the one-poll Gateway runtime path with async completion coverage and an ignored live Gateway smoke path. | `docs/open-agents/upstream-parity.md`, `scripts/open-agents-test-inventory.mjs`, `docs/open-agents/bucket-ownership.md`, `docs/open-agents/slack-remote-agent-architecture.md`, `docs/open-agents/deployment-verification.md`, `crates/open-agents-*` | Keep the Open Agents gate green and rerun the live Slack/Gateway/Vercel proof paths when credentials or upstream drift change. |
| Just Bash | JB-01 through JB-07 landed the current-upstream inventory gate, in-memory filesystem/path/encoding backend, reusable session/executor contract, parser/interpreter slice, command/runtime slice, security/network policy seams, and default Open Agents Just Bash backend wiring. JBC-01 through JBC-08 landed the NAPI bridge, JS dual-engine harness, generated conformance corpus, Rust corpus runner, CI gate wiring, core command parity, text/search/structured command parity, and Open Agents service conformance proof. The ledger now maps 931 exact upstream rows to named Rust tests; 8,889 rows remain `portable-pending`, so strict parity is still intentionally open. JBC-09 through JBC-16 are active against the largest remaining buckets. | `crates/just-bash`; `crates/open-agents-*`; `docs/open-agents/just-bash-parity.md`; `docs/open-agents/just-bash-conformance.md`; `node scripts/just-bash-test-inventory.mjs --check`; `node scripts/just-bash-conformance-corpus.mjs --check`; `cargo test -p just-bash`; `scripts/open-agents-local-e2e.sh --just-bash-conformance` | Land JBC-09 through JBC-16, then continue converting remaining owned rows to `portable-verified` with named Rust tests or explicit exceptions. Closure requires `node scripts/just-bash-test-inventory.mjs --strict` and the shared conformance harness passing against both TypeScript and Rust engines. |
| Workflow SDK | Current upstream portable rows are closed by the row-level gate. The gate currently passes with 2,679 inventory rows, 22 summary packages, and 28 ledger packages. `packages/core`/`workflow-core` is `verified`: the core inventory has 1,216 rows, 1,023 portable rows all mapped to named Rust tests, 189 `js-only-documented` rows, 4 `type-system-impossible` rows, and 0 `needs-review` rows. Core E2E specifically has 154 rows: 79 portable verified and 75 `js-only-documented`. | `node scripts/workflow-test-inventory.mjs --check && node scripts/workflow-parity-check.mjs`; `cargo test -p workflow-core` | Preserve the zero-`needs-review` invariant and rerun the drift audit whenever upstream `vercel/workflow` moves. |
| Chat SDK | CHAT-02 closed current-upstream drift at `ffc43fcf1f7679164be0806308bea237113c7590`: package progress is restored to 100.0%, all 19 package rows are closed, and all 16 portable package rows are strictly verified with named Rust tests or existing explicit exceptions. | `docs/chat/upstream-parity.md`, `docs/chat/package-progress.md` | Rerun the Chat SDK drift audit when upstream `vercel/chat` moves; preserve the hard rule that every portable row needs a named Rust test or explicit exception before any future 100% claim. |
| AI SDK | Current upstream package rows are closed. Package progress is 100.0%, all 52 package rows are closed, and all 42 portable rows are strictly verified with named Rust tests or explicit JavaScript-only/type-system exceptions. | `docs/upstream-parity.md`, `docs/package-progress.md`, `docs/ai-02-openai-compatible-providers.md` | Rerun the AI SDK drift audit when upstream `vercel/ai` moves; preserve the hard rule that every portable row needs a named Rust test or explicit exception before any future 100% claim. |

## Dispatch Board

Each row is intentionally sized so one Codex thread can own the branch, tests,
commit, and merge-back. Threads may split a row further when the upstream package
is too large, but the parent row remains open until all child rows are merged.

| ID | Status | Owner scope | Build | Verify before merge |
| --- | --- | --- | --- | --- |
| T2R-00 | complete | Master tracker and dispatch coordination | Current tracker rows are reconciled with the generated package ledgers and gate outputs for the refreshed 2026-06-01 upstream snapshots. | `git diff --check`; `scripts/master-parity-gate.sh`; relevant ledger/gate commands named in changed rows. |
| OA-01 | complete | Open Agents upstream inventory gate | Added `docs/open-agents/upstream-parity.md` plus `scripts/open-agents-test-inventory.mjs` to inventory packages, source files, and all 106 upstream tests. | Fresh `npx opensrc fetch https://github.com/vercel-labs/open-agents`; `node scripts/open-agents-test-inventory.mjs --check`; `cargo test -p open-agents-core -p open-agents-runtime -p open-agents-sandbox -p open-agents-slack -p open-agents-service`. |
| OA-02 | complete | Open Agents Slack remote-agent runtime closure | Closed the local Slack runtime gaps surfaced by OA-01: durable active-run resume, question and approval block-action resumes, finish-action Slack summaries, terminal sandbox/model error reporting, and emulator approval coverage. | `node scripts/open-agents-test-inventory.mjs --check`; `cargo test -p open-agents-core -p open-agents-runtime -p open-agents-sandbox -p open-agents-slack -p open-agents-service -p chat-sdk-adapter-slack`; `scripts/open-agents-local-e2e.sh --all-local`; ignored Slack/Gateway/Vercel proof envs documented in `docs/open-agents/deployment-verification.md`. |
| OA-03B | complete | Open Agents real Gateway runtime proof | Replaced the one-poll `poll_ready` Gateway path with a real async execution path, switched the service runtime to the native `GatewayProvider`, added deterministic Pending-then-Ready/native-provider coverage, preserved fixture/durable Slack behavior, and deployed the production service with `runtime=gateway`. | `cargo test -p open-agents-service`; `scripts/master-parity-gate.sh`; ignored live Gateway smoke when credentials exist. |
| JB-01 | complete | Just Bash strict inventory gate | Added `docs/open-agents/just-bash-parity.md` plus `scripts/just-bash-test-inventory.mjs` to inventory all current upstream package manifests, TS/TSX source files, test files, fixture roots, and 9,936 test cases. All portable rows remain pending until sibling implementation threads map them to named Rust tests or explicit documented exceptions. | Fresh `npx opensrc fetch https://github.com/vercel-labs/just-bash`; `git ls-remote https://github.com/vercel-labs/just-bash HEAD refs/heads/main`; `node scripts/just-bash-test-inventory.mjs --check`; `node scripts/just-bash-test-inventory.mjs --strict` is the documented fail-closed closure gate while pending rows remain. |
| JB-02 | complete | Just Bash core API and in-process executor contract | Added reusable session/exec/result metadata, per-exec env/cwd/stdin/args/replace-env, cancellation/time limits, persistent virtual filesystem behavior, no-external-sandbox metadata, and inline executor namespace commands to `crates/just-bash` without changing `LocalSandbox::exec` or Open Agents default wiring. The Just Bash ledger maps 34 exact upstream core/executor rows to named Rust tests and leaves `Sandbox.runCommand` runtime wiring rows pending for JB-07. | Fresh `npx opensrc fetch https://github.com/vercel-labs/just-bash`; `cargo test -p just-bash just_bash`; `node scripts/just-bash-test-inventory.mjs --check`; focused crate checks plus shared naming/diff gates. |
| JB-03 | complete | Just Bash filesystem, path, and encoding backend | Added `crates/just-bash` with an in-memory virtual filesystem, path normalization/containment, cwd/env session scoping, binary/text encoding helpers, file-reader helpers, glob matching, redirection/here-doc write sinks, symlink policy, and sanitized errors without host filesystem or shell fallback. The Just Bash ledger maps 125 exact upstream filesystem/path/encoding rows to named Rust tests and leaves command/parser/security integration rows pending. | Fresh `npx opensrc fetch https://github.com/vercel-labs/just-bash`; `git ls-remote https://github.com/vercel-labs/just-bash HEAD refs/heads/main`; `cargo test -p just-bash`; `cargo clippy -p just-bash --all-targets --all-features -- -D warnings`; `node scripts/just-bash-test-inventory.mjs --check`; `scripts/check-naming-conventions.sh`; `git diff --check`. |
| JB-04 | complete | Just Bash parser and interpreter slice | Landed command parsing, shell/interpreter behavior, pipeline/control-flow scaffolding, and named parser/runtime Rust tests on top of the filesystem/core API layers. | Fresh `npx opensrc fetch https://github.com/vercel-labs/just-bash`; `cargo test -p just-bash`; `node scripts/just-bash-test-inventory.mjs --check`; shared fmt/clippy/naming/diff gates. |
| JB-05 | complete | Just Bash command registry and runtime slice | Landed command/runtime modules for the initial portable builtin surface, integrated with the existing filesystem, executor, shell, and security layers rather than replacing them. | `cargo test -p just-bash`; focused command/runtime tests; `node scripts/just-bash-test-inventory.mjs --check`; shared fmt/clippy/naming/diff gates. |
| JB-06 | complete | Just Bash security and network policy seams | Landed security and network seams so command execution has explicit policy boundaries and no hidden host-shell or network fallback. | `cargo test -p just-bash`; focused security/network tests; `node scripts/just-bash-test-inventory.mjs --check`; shared fmt/clippy/naming/diff gates. |
| JB-07 | complete | Open Agents Just Bash backend wiring | Replaced the temporary Open Agents bash adapter path with crate-backed Just Bash calls where possible, kept default no-host-shell behavior, and preserved explicit local/Vercel backend selection. | `cargo test -p open-agents-service -p open-agents-sandbox -p just-bash`; `scripts/open-agents-local-e2e.sh --dry-run`; `scripts/open-agents-local-e2e.sh --check-config`; `scripts/check-naming-conventions.sh`; `git diff --check`. |
| JBC-01 | complete | Just Bash NAPI adapter | Exposed `crates/just-bash` to JavaScript with `napi-rs` so upstream TypeScript comparison tests can instantiate the Rust engine through the same JS harness shape. | `cargo test -p just-bash-napi`; `npm test --prefix crates/just-bash-napi`; `scripts/check-naming-conventions.sh`; `git diff --check`. |
| JBC-02 | complete | Shared JavaScript dual-engine conformance harness | Added a JS runner that can execute selected upstream Just Bash comparison/unit cases with `JUST_BASH_ENGINE=typescript` or `JUST_BASH_ENGINE=rust`, records reusable case fixtures, and avoids polluting the upstream OpenSrc inventory. | TypeScript engine smoke against upstream fixtures; Rust engine smoke via NAPI or explicit missing-addon diagnostic; `node scripts/just-bash-test-inventory.mjs --check`; `scripts/check-naming-conventions.sh`; `git diff --check`. |
| JBC-03 | complete | Generated Just Bash conformance corpus | Generated a stable JSON corpus from upstream comparison tests, unit exec cases, fixtures, command domains, expected stdout/stderr/exit codes, and ledger row ids so Rust tests and JS tests share the same source of truth. | `node scripts/just-bash-conformance-corpus.mjs --check`; `node scripts/just-bash-test-inventory.mjs --check`; `git diff --check`. |
| JBC-04 | complete | Rust conformance corpus runner | Added a data-driven Rust test runner that loads corpus cases, seeds virtual files/env/cwd, runs `just_bash::Bash`, compares stdout/stderr/exit status, and reports upstream case ids on mismatches. | `cargo test -p just-bash --test conformance_corpus`; `cargo test -p just-bash`; `cargo clippy -p just-bash --all-targets --all-features -- -D warnings`; `scripts/check-naming-conventions.sh`; `git diff --check`. |
| JBC-05 | complete | Just Bash conformance ledgers and CI gates | Wired non-strict Just Bash conformance/inventory checks into the master gate and documented strict close criteria without hiding remaining `portable-pending` rows. | `node scripts/just-bash-test-inventory.mjs --check`; `scripts/master-parity-gate.sh --check`; `bash -n scripts/master-parity-gate.sh`; `node --check scripts/just-bash-test-inventory.mjs`; `git diff --check`. |
| JBC-06 | complete | Core filesystem command parity closure | Mapped 92 exact upstream `cat`, `ls`, `mkdir`, `rm`, `cp`, and `mv` rows to named Rust tests and ledger mappings. | `cargo test -p just-bash`; `node scripts/just-bash-test-inventory.mjs --check`; `cargo clippy -p just-bash --all-targets --all-features -- -D warnings`; `scripts/check-naming-conventions.sh`; `git diff --check`. |
| JBC-07 | complete | Text search and structured command parity closure | Mapped 143 exact upstream `grep`, `rg`, `sed`, `awk`, `head`, `tail`, `wc`, `sort`, `uniq`, `cut`, `tr`, and `jq` rows to named Rust tests and ledger mappings. | `cargo test -p just-bash`; `node scripts/just-bash-test-inventory.mjs --check`; `node scripts/just-bash-conformance-corpus.mjs --check`; shared fmt/clippy/naming/diff gates. |
| JBC-08 | complete | Open Agents service Just Bash conformance proof | Added Open Agents service/local-E2E tests proving Slack-triggered remote-agent execution reaches the crate-backed Just Bash backend, persists virtual filesystem state, maps failures, and never silently falls back to host `/bin/bash`. | `cargo test -p open-agents-service`; `scripts/open-agents-local-e2e.sh --just-bash-conformance`; `cargo test -p open-agents-service -p open-agents-sandbox -p just-bash`; `scripts/check-naming-conventions.sh`; `git diff --check`. |
| JBC-09 | active | AWK command parity closure | Close the large `command:awk` gap with named Rust tests for exact portable rows covering print, fields, FS/OFS, NR/NF, BEGIN/END, simple patterns, stdin/files, and diagnostics. | Focused AWK tests; `cargo test -p just-bash`; clippy/fmt; inventory/corpus checks; naming/diff gates. |
| JBC-10 | active | Ripgrep command parity closure | Close the large `command:rg` gap with exact virtual-filesystem ripgrep rows for recursive search, path filters, case/fixed/regex flags, line numbers, context/headings, hidden/binary handling, and stdin where portable. | Focused rg tests; `cargo test -p just-bash`; clippy/fmt; inventory/corpus checks; naming/diff gates. |
| JBC-11 | active | Comparison corpus closure | Promote the generated conformance corpus into a stronger Rust/JS row-closure path and map passing portable comparison fixture rows without hiding command-family gaps. | `cargo test -p just-bash --test conformance_corpus`; corpus/inventory checks; `cargo test -p just-bash`; clippy/fmt/naming/diff gates. |
| JBC-12 | active | Syntax and transform parity closure | Close exact portable `syntax` and `transform` parser/shell/AST rows for quoting, escaping, pipelines, redirection, expansions, functions, control flow, and transforms. | Focused parser/shell tests; `cargo test -p just-bash`; clippy/fmt; inventory/corpus checks; naming/diff gates. |
| JBC-13 | active | Advanced filesystem parity closure | Close exact portable `fs:*` rows for overlay, core, read/write, mountable filesystem, path normalization, symlinks, binary/text encoding, and error shapes. | Focused FS tests; `cargo test -p just-bash`; clippy/fmt; inventory/corpus checks; naming/diff gates. |
| JBC-14 | active | Security and sandbox parity closure | Close exact portable security/sandbox/fuzz/prototype-pollution rows or classify true JS-only worker/browser rows narrowly. | Focused security tests; `cargo test -p just-bash`; clippy/fmt; inventory/corpus checks; naming/diff gates. |
| JBC-15 | active | Interpreter core and expansion parity closure | Close exact portable interpreter builtins/core/expansion rows for builtin dispatch, scoped assignment, expansion semantics, substitution, arithmetic, arrays, aliases/functions, loops, status, and diagnostics. | Focused interpreter tests; `cargo test -p just-bash`; clippy/fmt; inventory/corpus checks; naming/diff gates. |
| JBC-16 | active | Structured and data command parity closure | Close exact portable `jq`, `yq`, `xan`, `sqlite3`, and adjacent data/query command rows that can run deterministically in-memory. | Focused structured command tests; `cargo test -p just-bash`; clippy/fmt; inventory/corpus checks; naming/diff gates. |
| OP-01 | complete | Open Plugin Spec manifest gate | Added `open-agents-core::plugin` for Open Plugin Spec v1.0.0 manifest loading, metadata parsing, `skills` and `mcpServers` field-shape validation, plugin name validation, path containment, optional vendor manifest diagnostics, and non-fatal unsupported-component diagnostics. Reconciled the conformance tracker with the already-landed OP-02 skill discovery and OP-03 MCP config surfaces so plugin support cannot claim unimplemented service/runtime rows. | Fresh `npx opensrc fetch https://github.com/vercel-labs/open-plugin-spec`; `git ls-remote https://github.com/vercel-labs/open-plugin-spec HEAD`; `cargo test -p open-agents-core`; `node scripts/open-plugin-spec-gate.mjs --check`; `scripts/master-parity-gate.sh`. |
| OP-02 | verified | Open Plugin skill discovery and invocation | Default and manifest-declared plugin skill directories are discovered through `ai-sdk-rust::skills`; `SKILL.md` metadata is surfaced with namespaced plugin skill IDs and `/plugin:skill` invocation without scanning defaults when manifest paths override them. | `open_plugin_default_discovery_namespaces_skills`; `open_plugin_manifest_skill_paths_override_default`; `open_plugin_manifest_can_explicitly_retain_default_skills`; `open_plugin_namespaced_slash_invocation_loads_plugin_skill_directory`. |
| OP-03 | verified | Open Plugin MCP discovery and runtime expansion | `open-agents-core::open_plugin` loads `.mcp.json`, manifest path config, and inline `mcpServers`; resolves conflicts deterministically; keeps invalid config failures non-fatal; expands `${PLUGIN_ROOT}` and `${PLUGIN_DATA}` in MCP runtime fields. Real process/network startup remains OP-04. | `open_plugin_mcp_loads_default_mcp_json_when_manifest_field_absent`; `open_plugin_mcp_inline_config_uses_manifest_servers`; `open_plugin_mcp_duplicate_server_names_emit_conflict_and_first_source_wins`; `open_plugin_mcp_expands_plugin_root_and_data_placeholders`; `open_plugin_mcp_invalid_config_shape_does_not_block_other_sources`. |
| OP-04 | not-started | Open Plugin host conformance closure | Wire the service-level plugin load path and partial-support diagnostics once at least one core component type is loaded end to end. | `open-agents-service` tests proving minimum Open Plugin host conformance without live credentials. |
| WF-01 | verified | Workflow core E2E parity closure | Closed the remaining `packages/core` E2E rows: 79 portable rows mapped to named `workflow-core` Rust tests and 75 rows classified `js-only-documented`; also closed adjacent abort consistency and workflow set-attributes core rows needed before `packages/core` could be verified. | `node scripts/workflow-test-inventory.mjs --check`; `node scripts/workflow-parity-check.mjs`; `cargo test -p workflow-core`. |
| WF-02 | complete | Workflow current-upstream drift audit | Refreshed upstream `ae3c833...`, found only `packages/core/e2e/event-log-race-repro.test.ts` diagnostic-label drift, no package/test inventory row churn, and classified all current rows so zero `needs-review` rows remain. | `node scripts/workflow-test-inventory.mjs --check`; `node scripts/workflow-parity-check.mjs`; no unclassified `needs-review` rows. |
| CHAT-01 | complete | Chat SDK current-upstream drift audit | Reconciled refreshed upstream `ffc43fc...` with `docs/chat/upstream-parity.md`; drift found, so the prior 100% claim was retired. | `docs/chat/package-progress.md` regenerated; focused chat and affected adapter crate tests; no claim that current portable rows are all mapped. |
| CHAT-02 | complete | Chat SDK drift closure | Closed the CHAT-01 current-upstream drift: `packages/chat`, Slack, GChat, Telegram, WhatsApp, and new Twilio all have named Rust tests for every portable drift case; no new js-only/type-system exceptions were needed. | `docs/chat/package-progress.md` regenerated to 100%; `cargo test -p chat-sdk-chat -p chat-sdk-adapter-slack -p chat-sdk-adapter-gchat -p chat-sdk-adapter-telegram -p chat-sdk-adapter-whatsapp -p chat-sdk-adapter-twilio`; final merge gate passed. |
| AI-01 | complete | AI SDK large foundational providers | Ported and verified `@ai-sdk/anthropic`, `@ai-sdk/amazon-bedrock`, `@ai-sdk/google`, and `@ai-sdk/google-vertex` with row-level upstream mappings and package-owned tests. | Package-owned upstream test mapping; provider crate tests; progress regeneration. |
| AI-02 | complete | AI SDK OpenAI-compatible major providers | Closed `@ai-sdk/xai`, `@ai-sdk/groq`, `@ai-sdk/cohere`, `@ai-sdk/fireworks`, and `@ai-sdk/togetherai` with package-owned tests, shared OpenAI-compatible regression coverage, ignored live proofs where credentials are required, and explicit exceptions for non-portable rows. | Package-owned tests plus shared OpenAI-compatible regression tests where applicable; progress regeneration; `scripts/master-parity-gate.sh`. |
| AI-02B | complete | AI SDK OpenAI-compatible provider verified closure audit | Added deterministic coverage for xAI Responses/server-tool usage, Groq transcription, Cohere embed/rerank, Fireworks provider-specific image routes, and TogetherAI image/rerank abort/error mapping before the child rows closed. | Focused tests for all five providers; regenerated `docs/package-progress.md`; `scripts/master-parity-gate.sh`; child rows AI-02C through AI-02G are now closed. |
| AI-02C | complete | xAI provider full closure | Mapped every portable upstream `@ai-sdk/xai` case to named Rust tests or explicit exceptions, including chat conversion/streaming/tools/usage, Responses provider-tool IDs/input/usage/streaming, Files API, image, and video surfaces. | `cargo test xai_ --all-features`; package row mapping; progress regeneration; `scripts/master-parity-gate.sh`. |
| AI-02D | complete | Groq provider full closure | Mapped every portable upstream `@ai-sdk/groq` case to named Rust tests or explicit exceptions, including chat streaming/reasoning/errors/tools/usage, browser-search tool preparation, message and usage conversion helpers, and transcription edge parity. | `cargo test groq_ --all-features`; package row mapping; progress regeneration; `scripts/master-parity-gate.sh`. |
| AI-02E | complete | Cohere provider full closure | Mapped every portable upstream `@ai-sdk/cohere` case to named Rust tests or explicit exceptions, including chat/prompt/tool/citation surfaces plus embedding/reranking validation and error edges. | `cargo test cohere_ --all-features`; package row mapping; progress regeneration; `scripts/master-parity-gate.sh`. |
| AI-02F | complete | Fireworks provider full closure | Closed all 43 refreshed upstream `@ai-sdk/fireworks` provider/image cases with named Rust tests or explicit JS-only/type-system/credential-gated exceptions; added provider factory split/alias coverage, image error metadata, edit input variants, async multi-poll timing, timeout/failure metadata, default live transport, and ignored live proof paths. | `cargo test fireworks_ --all-features`; 43/43 case map in `docs/ai-02-openai-compatible-providers.md`; regenerated `docs/package-progress.md`; `scripts/master-parity-gate.sh`. |
| AI-02G | complete | TogetherAI provider full closure | Closed all 40 refreshed upstream `@ai-sdk/togetherai` provider/image/reranking cases with named Rust tests or explicit JS-only/credential-gated exceptions; added provider-option validation, image file input mapping, text rerank coverage, error metadata, and an ignored live image/rerank proof. | `cargo test togetherai_ --all-features`; 40/40 case map in `docs/ai-02-openai-compatible-providers.md`; regenerated `docs/package-progress.md`; `scripts/master-parity-gate.sh`. |
| AI-03 | complete | AI SDK media generation providers | Ported the portable image/video media generation surface for `@ai-sdk/fal`, `@ai-sdk/klingai`, `@ai-sdk/prodia`, and `@ai-sdk/replicate`; kept already-green `@ai-sdk/luma` and `@ai-sdk/black-forest-labs` minimal before AI-03B verified the row set. | `cargo test -p ai-sdk-black-forest-labs -p ai-sdk-luma -p ai-sdk-replicate -p ai-sdk-fal -p ai-sdk-klingai -p ai-sdk-prodia`; `docs/ai-03-media-live-proofs.md`. |
| AI-03B | complete | AI SDK media provider full parity closure | Closed the remaining `@ai-sdk/black-forest-labs`, `@ai-sdk/luma`, `@ai-sdk/fal`, `@ai-sdk/klingai`, `@ai-sdk/prodia`, and `@ai-sdk/replicate` rows after AI-03's initial media slice. | Focused tests for all six provider crates; package row mapping in `docs/upstream-parity.md`; regenerated `docs/package-progress.md`; `scripts/master-parity-gate.sh`. |
| AI-04 | complete | AI SDK speech, transcription, and audio providers | Port or close `@ai-sdk/elevenlabs`, `@ai-sdk/gladia`, `@ai-sdk/deepgram`, `@ai-sdk/hume`, `@ai-sdk/lmnt`, `@ai-sdk/revai`, `@ai-sdk/assemblyai`, and `@ai-sdk/voyage`. | Speech/transcription fixture tests, warning/error mapping tests, progress regeneration. |
| AI-05 | complete | AI SDK remaining in-progress wrappers | Finished `@ai-sdk/azure`, `@ai-sdk/baseten`, `@ai-sdk/bytedance`, `@ai-sdk/cerebras`, `@ai-sdk/deepinfra`, `@ai-sdk/huggingface`, `@ai-sdk/moonshotai`, and `@ai-sdk/vercel`. | Package-owned tests for every row named in `docs/upstream-parity.md`; progress regenerated. |
| AI-06 | complete | AI SDK public API and examples parity | Audited current root `ai` ergonomics, examples, docs snippets, and high-level generate/stream/embed/object APIs against upstream `ab6d66482d31afe15f4973a51c5f7cfa09c92ea6`; added root facade coverage for structured-output helpers, stream callbacks, and agent UI stream helpers. | `cargo test -p ai-sdk-rust --lib`; `cargo check --examples`; docs update; progress regeneration. |
| AI-07 | complete | AI SDK Alibaba provider | Initial `crates/ai-sdk-alibaba` crate landed and AI-07B closed the remaining row-level gaps before the verified claim. | Package-owned Alibaba tests, explicit exceptions for non-portable rows, and progress regeneration. |
| AI-07B | complete | AI SDK Alibaba verified closure | Mapped every remaining portable Alibaba upstream case to named Rust tests or explicit exceptions and moved the package to verified with row-level evidence. | `cargo test -p ai-sdk-alibaba`; regenerated `docs/package-progress.md`; `scripts/master-parity-gate.sh`. |
| CROSS-01 | complete | Cross-SDK examples and docs parity | Added `docs/cross-sdk-examples-docs-parity.md` plus `scripts/cross-sdk-examples-docs-inventory.mjs` to inventory public docs, README files, and example units across all four projects. | `node scripts/cross-sdk-examples-docs-inventory.mjs --check`; `cargo test --doc`; `cargo check --examples`; no undocumented docs/examples in the checked scanner scope. |
| CROSS-02 | complete | Live integration proof registry | Centralized ignored live tests and no-credentials emulator proofs in `docs/live-integration-proof-registry.md` for Slack, Vercel AI Gateway, Vercel Sandbox, provider APIs, Postgres/Redis, and Open Agents local E2E. | Registry lists env vars, commands, expected behavior, and skip semantics; ignored tests compile; local emulator tests pass without live credentials. |
| CROSS-03 | complete | Master parity gate | Adds `scripts/master-parity-gate.sh`, a single CI-safe command that runs the package progress generators, Workflow gate, Open Agents gate, and drift/status checks without requiring live credentials. | `scripts/master-parity-gate.sh`; `node scripts/workflow-test-inventory.mjs --check`; `node scripts/workflow-parity-check.mjs`; `node scripts/open-agents-test-inventory.mjs --check`; Open Agents now runs as part of the gate after OA-01 landed. |

## Active Thread Map

First dispatch wave created on 2026-06-01 with `gpt-5.5` and `xhigh`
reasoning.

| Tracker row | Thread id | Thread title |
| --- | --- | --- |
| OA-01 | `019e830a-7e74-7e10-829a-4dcf0f930dd3` | Add Open Agents parity inventory |
| OA-02 | `019e830b-41e9-73f2-9136-babce50bbc2a` | Close OA-02 runtime gaps |
| OA-03B | `019e8336-56e1-7691-a400-7da8d5ec959f` | T2R OA-03B Gateway Runtime Fix |
| JB-01 | `019e8422-5ed1-72c0-8965-4db7c488a17b` | Just Bash strict inventory gate |
| JB-02 | `019e8422-ac0b-7993-80d6-ffbe0790b1cd` | Just Bash core API and executor contract |
| JB-03 | `019e8422-e981-7b22-a7c8-d0a9f08e83fc` | Just Bash filesystem/path/encoding backend |
| JB-04 | `019e8423-316a-7840-8516-18f32f9c351e` | Just Bash parser and interpreter slice |
| JB-05 | `019e8423-6ffb-73f1-ab14-ebcd01abaeb6` | Just Bash command registry and runtime slice |
| JB-06 | `019e8423-d4d5-7b31-8e66-6bd740fc4137` | Just Bash security and network policy seams |
| JB-07 | `019e8424-16ce-7f00-9ecf-a1f315af5f67` | Open Agents Just Bash backend wiring |
| JBC-01 | `019e851a-ddac-7d21-9279-4f21f4422ba2` | Just Bash NAPI adapter |
| JBC-02 | `019e851b-c778-7a52-82c6-00fa482d40e6` | Shared JS dual-engine conformance harness |
| JBC-03 | `019e851b-c77a-7682-8c48-094932b4386f` | Generated Just Bash conformance corpus |
| JBC-04 | `019e851b-c9c8-7fb2-bb48-bfcba7b18fd0` | Rust conformance corpus runner |
| JBC-05 | `019e851b-cd47-7ee1-ba37-5d8930fa8615` | Ledger and CI conformance gates |
| JBC-06 | `019e851b-d223-72d2-ab9c-825b3fb30601` | Core filesystem command parity closure |
| JBC-07 | `019e851b-d298-79c1-96e3-eef148d0f49e` | Text search and structured command parity closure |
| JBC-08 | `019e851b-d408-7561-b2db-8f8f8e1303b2` | Open Agents service Just Bash conformance proof |
| JBC-09 | `019e8539-ceed-7512-b0f1-47de938fcaf8` | Close Just Bash awk gap |
| JBC-10 | `019e8539-d03a-7522-babe-7b2c3bd5eac8` | Close rg compatibility gap |
| JBC-11 | `019e853a-aada-7361-a83a-58717697bf8d` | Update just-bash conformance corpus |
| JBC-12 | `019e853a-ad33-70e1-b10c-7cf15c8e4ee4` | Close JBC-12 parity gaps |
| JBC-13 | `019e853a-ad25-7f00-9934-17e4dd86aba9` | Close just-bash FS parity |
| JBC-14 | `019e853a-b4a8-7f83-84c0-897ff0e86427` | Close Just Bash security parity |
| JBC-15 | `019e853a-b4a6-78e2-8f1f-3dcfdf74fc00` | Close just-bash parity gaps |
| JBC-16 | `019e853a-b800-7591-8eb6-2d7793548a00` | Close Just Bash command gaps |
| WF-01 | `019e830b-4400-7193-834f-cca74a33063f` | T2R WF-01 Workflow Core E2E Closure |
| WF-02 | `019e830b-441a-7503-943d-06fb626f1f44` | Audit workflow upstream drift |
| CHAT-01 | `019e830b-44d2-7063-9030-5a3cd764a76c` | Audit Chat SDK drift |
| CHAT-02 | `019e8320-d5ae-7a11-897f-2804b78d64a4` | T2R CHAT-02 Chat SDK Drift Closure |
| AI-01 | `019e830b-fd70-7600-a380-3d6567ac3ec2` | Port foundational AI providers |
| AI-01A | `019e8320-d5be-7551-9c94-fc4bf623bdb4` | T2R AI-01A Anthropic Provider |
| AI-01B | `019e8320-d5bd-7161-9a3c-a181315d2f9a` | T2R AI-01B Amazon Bedrock Provider |
| AI-01C | `019e8320-d593-79f1-942f-b1eac9d76531` | T2R AI-01C Google Provider |
| AI-01D | `019e8320-d5c5-78c2-9c36-5f64142efa47` | T2R AI-01D Google Vertex Provider |
| AI-02 | `019e830b-fd70-7600-a380-3d77ad85215a` | Port OpenAI-compatible providers |
| AI-02B | `019e8339-3f9a-7421-97f7-f94c398ffbc3` | T2R AI-02B Provider Verified Closure |
| AI-02C | `019e835e-66f8-7de2-99d6-ed6fa4779093` | Close xAI provider parity |
| AI-02D | `019e835e-9fe0-7380-9075-5f058d1cba4d` | Complete Groq provider parity |
| AI-02E | `019e835e-dda3-7ff1-b58f-2fdc9f912866` | Close Cohere provider parity |
| AI-02F | `019e835f-1e13-7403-84fa-c8c5f360d2da` | Close Fireworks provider parity |
| AI-02G | `019e835f-5ca0-7832-a225-861f0ace378e` | T2R AI-02G TogetherAI closure |
| AI-03 | `019e830b-feaf-7322-9ef1-37f3a740f1ed` | Port media generation providers |
| AI-03B | `019e8337-2b22-73f1-96d3-31f07b23bb4a` | Close media provider parity |
| AI-04 | `019e830c-0437-7d61-9b03-0d5715493ff7` | Port AI-04 audio providers |
| AI-05 | `019e830c-029b-7440-8f0e-d64f43f6f1f8` | Finish AI-05 provider wrappers |
| AI-06 | `019e830c-0dc3-7c91-b704-0111e1c66a18` | Audit AI root API parity |
| AI-07 | `019e830c-8e7d-7b63-88d2-3034e30347d7` | Add Alibaba AI SDK provider |
| AI-07B | `019e8337-2cb3-7501-83ef-bd5625ce5f34` | Verify Alibaba provider closure |
| CROSS-01 | `019e830c-8e75-7bf3-9a9c-c4925d0bd572` | Examples and docs parity |
| CROSS-02 | `019e830c-0e17-76e3-9363-7dc7377c32c6` | Add live proof registry |
| CROSS-03 | `019e830b-48a7-7713-a041-766f1b5efcb6` | Add master parity gate |

## AI-01 Child Rows

Created by the AI-01 inventory slice on 2026-06-01. These rows are closed after
each package mapped every portable row in
`docs/ai-foundational-provider-inventory.md` to named Rust tests in the owning
crate, while preserving explicit JavaScript-only and type-system exceptions.

| Child row | Status | Owner scope | Required proof |
| --- | --- | --- | --- |
| AI-01A | complete | `@ai-sdk/anthropic` in `crates/ai-sdk-anthropic` | Ported Anthropic language model, files, skills, cache control, prompt conversion, provider tools, usage conversion, error mapping, fixtures, and ignored live-provider proof; 418 portable upstream rows map to named Rust tests, 6 TypeScript-only generic-inference rows remain documented exceptions. |
| AI-01B | complete | `@ai-sdk/amazon-bedrock` in `crates/ai-sdk-amazon-bedrock` | Ported Bedrock chat, Anthropic-on-Bedrock, embeddings, image, reranking, event-stream handling, SigV4/API-key fetch wrappers, tool preparation, usage conversion, settings, fixtures, and ignored live-provider proof; 380 portable upstream rows map to named Rust tests and 3 JavaScript callable-constructor rows remain documented exceptions. |
| AI-01C | verified | `@ai-sdk/google` in `crates/ai-sdk-google` | All 568 current upstream Google cases are closed: 566 portable rows map to named Rust tests in `crates/ai-sdk-google`, 2 TypeScript compile-error rows are explicit `type-system-impossible` exceptions, and an ignored live-provider proof is credential-gated on `GOOGLE_GENERATIVE_AI_API_KEY`. |
| AI-01D | complete | `@ai-sdk/google-vertex` in `crates/ai-sdk-google-vertex` | Ported Vertex auth, provider base/edge variants, embedding, image, video, Anthropic-on-Vertex, MaaS, xAI-on-Vertex, fixtures, and ignored live-provider proof. |

## AI SDK Package Queue

The AI SDK is the largest open surface. Current generated progress shows:

- 52 of 52 package rows closed.
- 42 of 42 portable package rows strictly verified.
- 0 package rows in progress.
- 0 package rows not started.

In-progress packages:

None.

Not-started packages:

None. All AI SDK provider package rows are closed for the current refreshed
upstream snapshot.

## Merge-Back Checklist

Every implementation thread must finish with:

1. Fresh upstream fetch for its source project.
2. Updated ledger rows for every upstream test or functionality row touched.
3. Named Rust tests for every portable upstream case.
4. Explicit `js-only-documented` or `type-system-impossible` rows for every
   excluded case.
5. Regenerated package progress or inventory files when applicable.
6. Focused crate tests for touched crates.
7. Repository hygiene checks:
   - `cargo fmt --all --check`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `scripts/check-naming-conventions.sh`
   - `git diff --check`
8. Merge to `origin/main` only after acquiring `/tmp/ai-sdk-rust-main-merge.lock`.
